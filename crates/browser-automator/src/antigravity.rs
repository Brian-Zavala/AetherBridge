//! Antigravity (Cloud Code Assist) API client
//!
//! This module provides direct access to Google's Cloud Code Assist API,
//! which powers the Antigravity IDE. It supports:
//! - Gemini 3 Pro/Flash models
//! - Claude Sonnet/Opus 4.5 models (via Google's proxy)
//! - Thinking/reasoning modes
//! - Streaming responses (SSE)

use anyhow::{anyhow, Result};
use oauth::constants::{
    ANTIGRAVITY_ENDPOINTS, ANTIGRAVITY_USER_AGENT,
    ANTIGRAVITY_API_CLIENT, ANTIGRAVITY_CLIENT_METADATA,
    ANTIGRAVITY_DEFAULT_PROJECT_ID,
};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn, error, info};
use uuid::Uuid;

// =============================================================================
// Model Definitions
// =============================================================================

/// Available models via Antigravity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntigravityModel {
    /// Gemini 3 Pro - advanced reasoning model
    Gemini3Pro,
    /// Gemini 3 Flash - fast, efficient model
    Gemini3Flash,
    /// Claude Sonnet 4.5 - balanced performance
    ClaudeSonnet45,
    /// Claude Sonnet 4.5 with thinking/reasoning
    ClaudeSonnet45Thinking,
    /// Claude Opus 4.5 with thinking/reasoning (most capable)
    ClaudeOpus45Thinking,
}

impl AntigravityModel {
    /// Returns the API model identifier
    pub fn api_id(&self) -> &'static str {
        match self {
            Self::Gemini3Pro => "gemini-3-pro",
            Self::Gemini3Flash => "gemini-3-flash",
            Self::ClaudeSonnet45 => "claude-sonnet-4-5",
            Self::ClaudeSonnet45Thinking => "claude-sonnet-4-5-thinking",
            Self::ClaudeOpus45Thinking => "claude-opus-4-5-thinking",
        }
    }

    /// Returns a human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Gemini3Pro => "Gemini 3 Pro",
            Self::Gemini3Flash => "Gemini 3 Flash",
            Self::ClaudeSonnet45 => "Claude Sonnet 4.5",
            Self::ClaudeSonnet45Thinking => "Claude Sonnet 4.5 (Thinking)",
            Self::ClaudeOpus45Thinking => "Claude Opus 4.5 (Thinking)",
        }
    }

    /// Whether this model supports thinking/reasoning mode
    pub fn supports_thinking(&self) -> bool {
        matches!(
            self,
            Self::ClaudeSonnet45Thinking
            | Self::ClaudeOpus45Thinking
            | Self::Gemini3Pro
            | Self::Gemini3Flash
        )
    }

    /// Whether this is a Claude model
    pub fn is_claude(&self) -> bool {
        matches!(
            self,
            Self::ClaudeSonnet45 | Self::ClaudeSonnet45Thinking | Self::ClaudeOpus45Thinking
        )
    }

    /// Gets the default thinking budget for this model (if applicable)
    pub fn default_thinking_budget(&self) -> Option<u32> {
        match self {
            Self::ClaudeSonnet45Thinking => Some(8192),
            Self::ClaudeOpus45Thinking => Some(16384),
            Self::Gemini3Pro | Self::Gemini3Flash => None, // Uses thinkingLevel instead
            _ => None,
        }
    }

    /// Parses a model string into an AntigravityModel
    pub fn from_str(s: &str) -> Option<Self> {
        let lower = s.to_lowercase();

        // Handle various naming conventions
        if lower.contains("opus") && lower.contains("thinking") {
            Some(Self::ClaudeOpus45Thinking)
        } else if lower.contains("sonnet") && lower.contains("thinking") {
            Some(Self::ClaudeSonnet45Thinking)
        } else if lower.contains("sonnet") || lower.contains("claude-sonnet") {
            Some(Self::ClaudeSonnet45)
        } else if lower.contains("gemini-3-pro") || lower.contains("gemini3pro") {
            Some(Self::Gemini3Pro)
        } else if lower.contains("gemini-3-flash") || lower.contains("gemini3flash") {
            Some(Self::Gemini3Flash)
        } else {
            None
        }
    }

    /// Returns all available models
    pub fn all() -> Vec<Self> {
        vec![
            Self::Gemini3Pro,
            Self::Gemini3Flash,
            Self::ClaudeSonnet45,
            Self::ClaudeSonnet45Thinking,
            Self::ClaudeOpus45Thinking,
        ]
    }
}

impl std::fmt::Display for AntigravityModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// =============================================================================
// Request/Response Types
// =============================================================================

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role: "user", "assistant", or "system"
    pub role: String,
    /// Message content
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
}

/// Configuration for thinking/reasoning mode
#[derive(Debug, Clone, Default)]
pub struct ThinkingConfig {
    /// Token budget for thinking (Claude models)
    pub budget: Option<u32>,
    /// Thinking level for Gemini 3 models: "minimal", "low", "medium", "high"
    pub level: Option<String>,
    /// Whether to include thinking content in response
    pub include_thoughts: bool,
}

/// Response from a chat completion request
#[derive(Debug, Clone)]
pub struct ChatResponse {
    /// The main response content
    pub content: String,
    /// Thinking/reasoning content (if model supports it and was requested)
    pub thinking: Option<String>,
    /// The model that generated the response
    pub model: String,
    /// Finish reason
    pub finish_reason: String,
    /// Token usage (if available)
    pub usage: Option<Usage>,
}

/// Token usage information
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// A streaming chunk from the API
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// Delta text content
    pub delta: String,
    /// Whether this is thinking content
    pub is_thinking: bool,
    /// Whether this is the final chunk
    pub done: bool,
}

/// Error type for rate limiting
#[derive(Debug)]
pub struct RateLimitError {
    /// Seconds until rate limit resets
    pub retry_after_seconds: u64,
    /// Optional error message
    pub message: Option<String>,
}

// =============================================================================
// Antigravity Client
// =============================================================================

/// Client for Google's Cloud Code Assist (Antigravity) API
pub struct AntigravityClient {
    /// HTTP client
    client: reqwest::Client,
    /// Current access token
    access_token: Arc<RwLock<String>>,
    /// Project ID for API calls
    project_id: Arc<RwLock<String>>,
    /// Current endpoint (can fallback)
    endpoint_index: Arc<RwLock<usize>>,
}

impl AntigravityClient {
    /// Creates a new AntigravityClient with the given access token
    pub fn new(access_token: String, project_id: Option<String>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert("User-Agent", HeaderValue::from_static(ANTIGRAVITY_USER_AGENT));
        headers.insert("X-Goog-Api-Client", HeaderValue::from_static(ANTIGRAVITY_API_CLIENT));
        headers.insert("Client-Metadata", HeaderValue::from_static(ANTIGRAVITY_CLIENT_METADATA));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Session Distribution: Randomize session ID to avoid rate limit tracking by client ID
        let session_id = Self::generate_session_id();
        if let Ok(val) = HeaderValue::from_str(&session_id) {
            headers.insert("X-Goog-Session-Id", val);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(3600)) // 1 hour timeout for queuing + long thinking
            .build()?;

        Ok(Self {
            client,
            access_token: Arc::new(RwLock::new(access_token)),
            project_id: Arc::new(RwLock::new(
                std::env::var("GOOGLE_CLOUD_PROJECT").ok()
                    .or(project_id)
                    .unwrap_or_else(|| ANTIGRAVITY_DEFAULT_PROJECT_ID.to_string())
            )),
            endpoint_index: Arc::new(RwLock::new(0)),
        })
    }

    /// Updates the access token (for token refresh)
    pub async fn set_access_token(&self, token: String) {
        *self.access_token.write().await = token;
    }

    /// Gets the current endpoint URL
    async fn current_endpoint(&self) -> &'static str {
        let idx = *self.endpoint_index.read().await;
        ANTIGRAVITY_ENDPOINTS.get(idx).copied().unwrap_or(ANTIGRAVITY_ENDPOINTS[0])
    }

    /// Helper to generate a dynamic session ID for request anonymity
    fn generate_session_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Tries the next endpoint in the fallback list
    async fn try_next_endpoint(&self) -> bool {
        let mut idx = self.endpoint_index.write().await;
        if *idx + 1 < ANTIGRAVITY_ENDPOINTS.len() {
            *idx += 1;
            warn!("Falling back to endpoint: {}", ANTIGRAVITY_ENDPOINTS[*idx]);
            true
        } else {
            false
        }
    }

    /// Fetches the provisioned project ID (using loadCodeAssist)
    /// This returns the "Golden Ticket" project ID that has quotas enabled.
    async fn fetch_provisioned_project_id(&self) {
        // Only fetch if we are currently using the default project or a manually configured one
        // that might be invalid. We always try to upgrade to the official one.
        let current = self.project_id.read().await.clone();

        debug!("Attempting to discover provisioned project ID...");
        let token = self.access_token.read().await.clone();

        // Try endpoints in order (Prod -> Daily -> Autopush)


        for (idx, endpoint) in ANTIGRAVITY_ENDPOINTS.iter().enumerate() {
             let url = format!("{}/v1internal:loadCodeAssist", endpoint);
             let body = json!({
                 "metadata": {
                     "ideType": "IDE_UNSPECIFIED",
                     "platform": "PLATFORM_UNSPECIFIED",
                     "pluginType": "GEMINI"
                 }
             });

             match self.client
                 .post(&url)
                 .header(AUTHORIZATION, format!("Bearer {}", token))
                 .json(&body)
                 .send()
                 .await
             {
                 Ok(resp) => {
                     if resp.status().is_success() {
                         if let Ok(json) = resp.json::<Value>().await {
                             // Check for cloudaicompanionProject (string or object with id)
                             let extracted_id = if let Some(id_str) = json.get("cloudaicompanionProject").and_then(|v| v.as_str()) {
                                 Some(id_str.to_string())
                             } else if let Some(id_str) = json.get("cloudaicompanionProject")
                                 .and_then(|v| v.get("id"))
                                 .and_then(|v| v.as_str())
                             {
                                 Some(id_str.to_string())
                             } else {
                                 None
                             };

                             if let Some(id) = extracted_id {
                                 if !id.is_empty() {
                                     info!("Discovered provisioned project ID: {} (via {})", id, endpoint);
                                     *self.project_id.write().await = id;
                                     // IMPORTANT: Set the endpoint index to the one that worked!
                                     *self.endpoint_index.write().await = idx;
                                     return;
                                 }
                             }
                         }
                     } else {
                         debug!("loadCodeAssist failed at {}: {}", endpoint, resp.status());
                     }
                 },
                 Err(e) => debug!("Error calling loadCodeAssist at {}: {}", endpoint, e),
             }
        }

        warn!("Failed to discover provisioned project ID. Continuing with: {}", current);
    }

    /// Builds the request body for a chat completion
    fn build_request_body(
        &self,
        project_id: &str,
        model: AntigravityModel,
        messages: &[Message],
        thinking: Option<&ThinkingConfig>,
    ) -> Value {
        // Convert messages to Gemini format (contents array)
        let contents: Vec<Value> = messages.iter().map(|m| {
            json!({
                "role": if m.role == "assistant" { "model" } else { &m.role },
                "parts": [{"text": &m.content}]
            })
        }).collect();

        // Build generation config
        let mut generation_config = json!({
            "maxOutputTokens": 8192,
            "temperature": 0.7,
        });

        // Add thinking configuration if supported
        if model.supports_thinking() {
            if let Some(thinking) = thinking {
                if model.is_claude() {
                    // Claude uses thinkingBudget ONLY. Do NOT send thinkingLevel.
                    if let Some(budget) = thinking.budget.or(model.default_thinking_budget()) {
                        generation_config["thinkingConfig"] = json!({
                            "thinkingBudget": budget,
                            "includeThoughts": thinking.include_thoughts
                        });
                    }
                } else {
                    // Gemini 3 uses thinkingLevel ONLY. Do NOT send thinkingBudget.
                    if let Some(level) = &thinking.level {
                        let mut effective_level = level.as_str();

                        // Gemini 3 Pro only supports "low" and "high"
                        if matches!(model, AntigravityModel::Gemini3Pro) {
                            match effective_level {
                                "minimal" => effective_level = "low",
                                "medium" => effective_level = "high",
                                _ => {}
                            }
                        }

                        generation_config["thinkingConfig"] = json!({
                            "thinkingLevel": effective_level,
                            "includeThoughts": thinking.include_thoughts
                        });
                    }
                }
            }
        }

        // Determine the actual model ID string to send
        let mut api_model_id = model.api_id().to_string();

        if matches!(model, AntigravityModel::Gemini3Pro) {
            // Gemini 3 Pro requires the tier in the model name (e.g., gemini-3-pro-low)
            // It does NOT use the bare name like Flash does.
            let level = thinking.and_then(|t| t.level.as_deref()).unwrap_or("low");
            let effective_level = match level {
                "minimal" => "low",
                "medium" => "high",
                other => other,
            };
            api_model_id = format!("{}-{}", api_model_id, effective_level);
        }

        // Build the full request body
        json!({
            "project": project_id,
            "model": api_model_id,
            "request": {
                "contents": contents,
                "generationConfig": generation_config,
            }
        })
    }

    /// Sends a chat completion request
    pub async fn chat_completion(
        &self,
        model: AntigravityModel,
        messages: Vec<Message>,
        thinking: Option<ThinkingConfig>,
    ) -> Result<ChatResponse> {
        // Ensure we have a valid project ID
        self.fetch_provisioned_project_id().await;

        let endpoint = self.current_endpoint().await;
        let url = format!("{}/v1internal:generateContent", endpoint);
        let token = self.access_token.read().await.clone();
        let project_id = self.project_id.read().await.clone();

        let body = self.build_request_body(&project_id, model, &messages, thinking.as_ref());

        debug!("Sending request to {}", url);
        debug!("Request body: {}", serde_json::to_string_pretty(&body)?);

        let mut request = self.client
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .json(&body);

        // Header Injection: Claude models need specific beta headers for thinking
        if model.is_claude() {
             request = request.header("anthropic-beta", "interleaved-thinking-2025-05-14");
             // Note: OpenCode also mentions prompt-caching headers if used, but we don't support that yet.
        }

        let response = request.send().await?;

        let status = response.status();

        // Handle rate limiting
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            // Try to extract more info from body
            let body_text = response.text().await.unwrap_or_default();
            warn!("Rate limited. Retry after {} seconds. Body: {}", retry_after, body_text);

            return Err(anyhow!("RATE_LIMITED:{}:{}", retry_after, body_text));
        }

        // Handle endpoint failures (try fallback)
        if status.is_server_error() || status == reqwest::StatusCode::BAD_GATEWAY {
            error!("Server error from {}: {}", endpoint, status);
            if self.try_next_endpoint().await {
                // Retry with next endpoint
                return Box::pin(self.chat_completion(model, messages, thinking)).await;
            }
        }

        if !status.is_success() {
            let error_text = response.text().await?;
            error!("API error: {} - {}", status, error_text);
            return Err(anyhow!("API error {}: {}", status, error_text));
        }

        let raw: Value = response.json().await?;
        debug!("Response: {}", serde_json::to_string_pretty(&raw)?);

        self.parse_response(raw, model)
    }

    /// Parses the API response into a ChatResponse
    fn parse_response(&self, raw: Value, model: AntigravityModel) -> Result<ChatResponse> {
        // Check for "response" wrapper first (sometimes API wraps it)
        let root = if let Some(inner) = raw.get("response") {
            inner
        } else {
            &raw
        };

        // Extract from candidates[0].content.parts
        let candidates = root.get("candidates")
            .and_then(|c| c.as_array())
            .ok_or_else(|| anyhow!("No candidates in response"))?;

        if candidates.is_empty() {
            return Err(anyhow!("Empty candidates array in response"));
        }

        let first_candidate = &candidates[0];
        let parts = first_candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .ok_or_else(|| anyhow!("No content parts in response"))?;

        let mut content = String::new();
        let mut thinking = None;

        for part in parts {
            // Check if this is a thinking part
            let is_thought = part.get("thought")
                .and_then(|t| t.as_bool())
                .unwrap_or(false);

            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                if is_thought {
                    thinking = Some(text.to_string());
                } else {
                    content.push_str(text);
                }
            }
        }

        let finish_reason = first_candidate
            .get("finishReason")
            .and_then(|r| r.as_str())
            .unwrap_or("stop")
            .to_string();

        // Extract usage if available
        let usage = raw.get("usageMetadata").map(|u| Usage {
            prompt_tokens: u.get("promptTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: u.get("candidatesTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u.get("totalTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        });

        Ok(ChatResponse {
            content,
            thinking,
            model: model.api_id().to_string(),
            finish_reason,
            usage,
        })
    }

    /// Sends a streaming chat completion request
    pub async fn chat_completion_stream(
        &self,
        model: AntigravityModel,
        messages: Vec<Message>,
        thinking: Option<ThinkingConfig>,
    ) -> Result<impl futures::Stream<Item = Result<StreamChunk>> + Send> {
        // Ensure we have a valid project ID
        self.fetch_provisioned_project_id().await;

        let endpoint = self.current_endpoint().await;
        // Use streamGenerateContent with alt=sse
        let url = format!("{}/v1internal:streamGenerateContent?alt=sse", endpoint);
        let token = self.access_token.read().await.clone();
        let project_id = self.project_id.read().await.clone();

        let body = self.build_request_body(&project_id, model, &messages, thinking.as_ref());

        debug!("Sending streaming request to {}", url);

        let mut request = self.client
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .json(&body);

        // Header Injection: Claude models need specific beta headers for thinking
        if model.is_claude() {
             request = request.header("anthropic-beta", "interleaved-thinking-2025-05-14");
        }

        let response = request.send().await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await?;
            // Handle rate limiting specifically
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                 return Err(anyhow!("RATE_LIMITED:60:{}", error_text));
            }
            error!("Streaming API error: {} - {}", status, error_text);
            return Err(anyhow!("API error {}: {}", status, error_text));
        }

        // Process the byte stream
        let stream = response.bytes_stream();

        // Use async-stream to yield parsed chunks
        let output_stream = async_stream::try_stream! {
            let mut line_buffer = String::new();
            let mut byte_stream = Box::pin(stream); // Pin the stream

            use futures::StreamExt;
            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = chunk_result?;
                let chunk_str = String::from_utf8_lossy(&bytes);
                // tracing::debug!("Raw stream chunk: {:?}", chunk_str); // Uncomment for deep debug
                line_buffer.push_str(&chunk_str);

                while let Some(newline_idx) = line_buffer.find('\n') {
                    let line = line_buffer[..newline_idx].to_string();
                    line_buffer.drain(..newline_idx + 1);

                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }

                    tracing::debug!("Processing stream line: {}", trimmed); // DEBUG LOG

                    if let Some(data) = trimmed.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            break;
                        }

                        match serde_json::from_str::<Value>(data) {
                             Ok(value) => {
                                 // Check for response wrapper in stream chunks too
                                 let root = if let Some(inner) = value.get("response") { inner } else { &value };

                                 if let Some(candidates) = root.get("candidates").and_then(|c| c.as_array()) {
                                     if let Some(first) = candidates.first() {
                                         if let Some(parts) = first.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                             for part in parts {
                                                 let is_thought = part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false);
                                                 if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                     yield StreamChunk {
                                                         delta: text.to_string(),
                                                         is_thinking: is_thought,
                                                         done: false,
                                                     };
                                                 }
                                             }
                                         }
                                     }
                                 }
                             },
                             Err(e) => {
                                 tracing::warn!("Failed to parse stream JSON: {} | Data: {}", e, data);
                             }
                        }
                    } else {
                        // Try parsing raw line (maybe no data: prefix?)
                         match serde_json::from_str::<Value>(trimmed) {
                             Ok(value) => {
                                 // Check for response wrapper in stream chunks too
                                 let root = if let Some(inner) = value.get("response") { inner } else { &value };

                                 if let Some(candidates) = root.get("candidates").and_then(|c| c.as_array()) {
                                     if let Some(first) = candidates.first() {
                                         if let Some(parts) = first.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                             for part in parts {
                                                 let is_thought = part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false);
                                                 if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                                     yield StreamChunk {
                                                         delta: text.to_string(),
                                                         is_thinking: is_thought,
                                                         done: false,
                                                     };
                                                 }
                                             }
                                         }
                                     }
                                 }
                             },
                             Err(_) => {
                                 // Just ignore non-json lines that don't start with data:
                                 tracing::debug!("Ignored non-data line: {}", trimmed);
                             }
                        }
                    }
                }
            }
            yield StreamChunk { delta: "".into(), is_thinking: false, done: true };
        };

        Ok(output_stream)
    }

    /// Returns the list of available models
    pub fn available_models() -> Vec<AntigravityModel> {
        AntigravityModel::all()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_from_str() {
        assert_eq!(
            AntigravityModel::from_str("claude-sonnet-4.5-thinking"),
            Some(AntigravityModel::ClaudeSonnet45Thinking)
        );
        assert_eq!(
            AntigravityModel::from_str("gemini-3-pro"),
            Some(AntigravityModel::Gemini3Pro)
        );
        assert_eq!(
            AntigravityModel::from_str("unknown-model"),
            None
        );
    }

    #[test]
    fn test_message_construction() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");

        let msg = Message::assistant("Hi there!");
        assert_eq!(msg.role, "assistant");
    }

    #[test]
    fn test_model_properties() {
        assert!(AntigravityModel::ClaudeSonnet45Thinking.supports_thinking());
        assert!(AntigravityModel::ClaudeSonnet45Thinking.is_claude());
        assert!(!AntigravityModel::Gemini3Pro.is_claude());
        assert!(AntigravityModel::Gemini3Pro.supports_thinking());
    }
}
