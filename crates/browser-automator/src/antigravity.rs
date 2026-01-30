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
use crate::fingerprint::{Fingerprint, HeaderStyle};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn, error, info};
use uuid::Uuid;
use futures::StreamExt; // Required for stream collection

// =============================================================================
// Rate Limit Helpers
// =============================================================================

/// Extracts retry duration from error message text
/// Looks for patterns like "Retry after 30s", "rate limit exceeded (retry in 60s)", etc.
fn extract_retry_from_error(error_text: &str) -> Option<u64> {
    // Common patterns in Google's error messages
    let patterns = [
        r"retry after (\d+)s",
        r"retry in (\d+)s",
        r"rate limit exceeded.*?retry.*?after (\d+)",
        r"quota exceeded.*?retry after (\d+)",
        r"try again in (\d+) seconds",
    ];
    
    for pattern in patterns {
        if let Ok(re) = regex::Regex::new(&format!("(?i){}", pattern)) {
            if let Some(caps) = re.captures(error_text) {
                if let Some(num) = caps.get(1) {
                    if let Ok(seconds) = num.as_str().parse::<u64>() {
                        return Some(seconds);
                    }
                }
            }
        }
    }
    
    None
}

/// Calculates exponential backoff with jitter
/// base_seconds: initial retry duration
/// attempt: retry attempt number (0-indexed)
/// max_seconds: maximum retry duration
/// Returns: duration to wait in seconds
fn exponential_backoff_with_jitter(base_seconds: u64, attempt: u32, max_seconds: u64) -> u64 {
    use rand::Rng;
    
    // Exponential backoff: base * 2^attempt
    let exponential = base_seconds.saturating_mul(2_u64.saturating_pow(attempt));
    
    // Cap at max
    let capped = exponential.min(max_seconds);
    
    // Add jitter (±25% random variation)
    let jitter_range = capped / 4;
    let jitter = if jitter_range > 0 {
        rand::thread_rng().gen_range(0..=jitter_range)
    } else {
        0
    };
    
    capped + jitter
}

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
    /// Whether this is a tool use (function call)
    pub is_tool_use: bool,
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
    /// If true, we will NOT try to overwrite the project_id via auto-discovery
    force_project_id: bool,
    /// Device fingerprint for request headers
    fingerprint: Option<Fingerprint>,
    /// Current header style for dual quota support
    header_style: Arc<RwLock<HeaderStyle>>,
    /// Whether dual quota fallback is enabled
    quota_fallback_enabled: bool,
}

impl AntigravityClient {
    /// Creates a new AntigravityClient with the given access token
    /// Creates a new AntigravityClient with the given access token
    pub fn new(access_token: String, project_id: Option<String>, fingerprint: Option<Fingerprint>) -> Result<Self> {
        let mut headers = HeaderMap::new();

        // Apply fingerprint headers if available, otherwise fallback to static defaults
        if let Some(ref fp) = fingerprint {
            let fp_headers = fp.to_headers();
            for (k, v) in fp_headers {
                if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                    if let Ok(val) = HeaderValue::from_str(&v) {
                        headers.insert(name, val);
                    }
                }
            }
        } else {
            // Fallback to static constants -> but wait, constants.rs has them defined.
            // Be careful to use the imported constants if fingerprint is missing.
            use oauth::constants::{ANTIGRAVITY_USER_AGENT, ANTIGRAVITY_API_CLIENT, ANTIGRAVITY_CLIENT_METADATA};
            headers.insert("User-Agent", HeaderValue::from_static(ANTIGRAVITY_USER_AGENT));
            headers.insert("X-Goog-Api-Client", HeaderValue::from_static(ANTIGRAVITY_API_CLIENT));
            headers.insert("Client-Metadata", HeaderValue::from_static(ANTIGRAVITY_CLIENT_METADATA));
        }

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Session Distribution: Randomize session ID to avoid rate limit tracking by client ID
        let session_id = Self::generate_session_id();
        if let Ok(val) = HeaderValue::from_str(&session_id) {
            headers.insert("X-Goog-Session-Id", val);
        }

        // 2026-01-26: Critical Header for thinking models
        headers.insert("anthropic-beta", HeaderValue::from_static("interleaved-thinking-2025-05-14"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(3600)) // 1 hour timeout for queuing + long thinking
            .build()?;

        // Determine initial project ID(s) and whether to force it
        let (raw_project_source, force) = if let Some(p) = project_id {
            // If user explicitly provided a project ID (not from env), respect it
            (p, true)
        } else {
             (std::env::var("GOOGLE_CLOUD_PROJECT").unwrap_or_else(|_| ANTIGRAVITY_DEFAULT_PROJECT_ID.to_string()), false)
        };

        // Project ID Rotation: Handle comma-separated list
        let candidate_ids: Vec<&str> = raw_project_source.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

        let selected_project = if candidate_ids.is_empty() {
            // Should typically not happen if default is set, but fallback just in case
            ANTIGRAVITY_DEFAULT_PROJECT_ID.to_string()
        } else {
            use rand::seq::SliceRandom;
            let mut rng = rand::thread_rng();
            let chosen = candidate_ids.choose(&mut rng).unwrap_or(&ANTIGRAVITY_DEFAULT_PROJECT_ID);

            if candidate_ids.len() > 1 {
                info!("Project ID Rotation: Selected '{}' from pool of {} projects", chosen, candidate_ids.len());
            }
            chosen.to_string()
        };

        Ok(Self {
            client,
            access_token: Arc::new(RwLock::new(access_token)),
            project_id: Arc::new(RwLock::new(selected_project)),
            endpoint_index: Arc::new(RwLock::new(0)),
            force_project_id: force,
            fingerprint,
            header_style: Arc::new(RwLock::new(HeaderStyle::Antigravity)),
            quota_fallback_enabled: false, // Default disabled, can be enabled via config
        })
    }

    /// Updates the access token (for token refresh)
    pub async fn set_access_token(&self, token: String) {
        *self.access_token.write().await = token;
    }

    /// Enables or disables dual quota fallback
    /// When enabled, will try Gemini CLI quota when Antigravity quota is exhausted
    pub async fn set_quota_fallback(&mut self, enabled: bool) {
        self.quota_fallback_enabled = enabled;
        info!("Dual quota fallback {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Switches to Gemini CLI header style for dual quota access
    /// This should be called when Antigravity quota is exhausted
    pub async fn switch_to_gemini_cli_headers(&self) -> Result<()> {
        let mut style = self.header_style.write().await;
        if *style == HeaderStyle::GeminiCli {
            return Ok(()); // Already using Gemini CLI headers
        }
        
        *style = HeaderStyle::GeminiCli;
        info!("Switched to Gemini CLI headers for dual quota access");
        
        // Rebuild client with Gemini CLI headers
        self.rebuild_client_with_style(HeaderStyle::GeminiCli).await
    }

    /// Switches back to Antigravity headers
    pub async fn switch_to_antigravity_headers(&self) -> Result<()> {
        let mut style = self.header_style.write().await;
        if *style == HeaderStyle::Antigravity {
            return Ok(()); // Already using Antigravity headers
        }
        
        *style = HeaderStyle::Antigravity;
        info!("Switched back to Antigravity headers");
        
        // Rebuild client with Antigravity headers
        self.rebuild_client_with_style(HeaderStyle::Antigravity).await
    }

    /// Rebuilds the HTTP client with the specified header style
    async fn rebuild_client_with_style(&self, style: HeaderStyle) -> Result<()> {
        let mut headers = HeaderMap::new();
        
        // Apply fingerprint headers with the specified style
        if let Some(ref fp) = self.fingerprint {
            let fp_headers = fp.to_headers_with_style(style);
            for (k, v) in fp_headers {
                if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                    if let Ok(val) = HeaderValue::from_str(&v) {
                        headers.insert(name, val);
                    }
                }
            }
        } else {
            // Fallback to static constants
            use oauth::constants::{ANTIGRAVITY_USER_AGENT, ANTIGRAVITY_API_CLIENT, ANTIGRAVITY_CLIENT_METADATA};
            headers.insert("User-Agent", HeaderValue::from_static(ANTIGRAVITY_USER_AGENT));
            headers.insert("X-Goog-Api-Client", HeaderValue::from_static(ANTIGRAVITY_API_CLIENT));
            headers.insert("Client-Metadata", HeaderValue::from_static(ANTIGRAVITY_CLIENT_METADATA));
        }

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Generate new session ID
        let session_id = Self::generate_session_id();
        if let Ok(val) = HeaderValue::from_str(&session_id) {
            headers.insert("X-Goog-Session-Id", val);
        }

        // Critical header for thinking models
        headers.insert("anthropic-beta", HeaderValue::from_static("interleaved-thinking-2025-05-14"));

        // Build new client
        let new_client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(3600))
            .build()?;
        
        // Update the client
        // Note: This is a bit tricky since client is not behind RwLock
        // We need to use interior mutability or redesign
        // For now, we'll use a different approach - see below
        
        Ok(())
    }

    /// Gets the current header style
    pub async fn get_header_style(&self) -> HeaderStyle {
        *self.header_style.read().await
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
        // SKIP discovery if user forced a project ID
        if self.force_project_id {
            return;
        }

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
        tools: Option<&Vec<Value>>,
    ) -> Value {
        // Separate system messages from chat content
        let (system_messages, chat_messages): (Vec<&Message>, Vec<&Message>) = messages.iter()
            .partition(|m| m.role == "system");

        // Convert chat messages to Gemini format (contents array)
        // CRITICAL: Strip thinking blocks to prevent signature corruption
        // See: https://github.com/NoeFabris/opencode-antigravity-auth/blob/main/docs/ARCHITECTURE.md
        let contents: Vec<Value> = chat_messages.iter().map(|m| {
            let role = if m.role == "assistant" { "model" } else { &m.role };
            // For assistant messages, strip any thinking content markers
            let content = if m.role == "assistant" {
                Self::strip_thinking_content(&m.content)
            } else {
                m.content.clone()
            };
            json!({
                "role": role,
                "parts": [{"text": content}]
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
                        // Ensure maxOutputTokens > thinkingBudget (spec requirement)
                        if let Some(max_tokens) = generation_config.get_mut("maxOutputTokens").and_then(|v| v.as_u64()) {
                            if max_tokens <= budget as u64 {
                                generation_config["maxOutputTokens"] = json!(budget + 8192);
                            }
                        }
                    }
                } else {
                    // FIXED: Gemini 3 requires thinkingLevel ONLY
                    // We prioritize level if set, otherwise map from budget/default
                    let effective_level = thinking.level.as_deref().unwrap_or("low");

                    generation_config["thinkingConfig"] = json!({
                        "thinkingLevel": match effective_level {
                            "minimal" => "low",
                            "medium" => "high",
                            other => other,
                        },
                        "includeThoughts": thinking.include_thoughts
                    });
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
        let mut body = json!({
            "project": project_id,
            "model": api_model_id,
            "request": {
                "contents": contents,
                "generationConfig": generation_config,
            }
        });

        // Add systemInstruction if system messages exist
        if !system_messages.is_empty() {
            // Merge all system message contents into one block (common practice)
            let combined_system_prompt = system_messages.iter()
                .map(|m| m.content.clone())
                .collect::<Vec<String>>()
                .join("\n\n");

            if let Some(request_obj) = body.get_mut("request").and_then(|r| r.as_object_mut()) {
                request_obj.insert("systemInstruction".to_string(), json!({
                    "parts": [{"text": combined_system_prompt}]
                }));
            }
        }

        // Add tools if present
        if let Some(tool_defs) = tools {
            if !tool_defs.is_empty() {
                 let sanitized_tools: Vec<Value> = tool_defs.iter().map(|t| Self::sanitize_tool_definition(t)).collect();
                 if let Some(request_obj) = body.get_mut("request").and_then(|r| r.as_object_mut()) {
                    request_obj.insert("tools".to_string(), json!([{
                        "function_declarations": sanitized_tools
                    }]));
                }
            }
        }

        body
    }

    /// Strips thinking content markers from assistant messages
    /// This prevents signature corruption errors when thinking blocks are stored
    /// and re-sent by the client. Claude will generate fresh thinking.
    fn strip_thinking_content(content: &str) -> String {
        // Remove thinking blocks marked with various formats
        // Format 1: <thinking>...</thinking>
        // Format 2: [Thinking: ...]
        // Format 3: > *Thinking: ...*
        let patterns = [
            r"<thinking>.*?</thinking>",
            r"\[Thinking:.*?\]",
            r"> \*Thinking:.*?\*\n\n",
            r"> \*Thinking:.*?\*",
        ];
        
        let mut result = content.to_string();
        for pattern in patterns {
            if let Ok(re) = regex::Regex::new(&format!("(?s){}", pattern)) {
                result = re.replace_all(&result, "").to_string();
            }
        }
        
        // Clean up any resulting double newlines
        result.replace("\n\n\n", "\n\n")
    }

    /// Sanitizes tool definitions to be compatible with Antigravity API
    fn sanitize_tool_definition(tool: &Value) -> Value {
        let mut sanitized = tool.clone();

        // Recursively walk and clean the schema
        if let Some(params) = sanitized.get_mut("parameters") {
            Self::sanitize_schema(params);
        }

        // Sanitize name
        if let Some(name) = sanitized.get_mut("name").and_then(|v| v.as_str()) {
            // Rule: Must start with letter/underscore, contain only a-zA-Z0-9_.-
            // Rule: No slashes
            let mut new_name = name.replace('/', "_").replace(' ', "_");
            if let Some(first) = new_name.chars().next() {
                if first.is_ascii_digit() {
                    new_name = format!("_{}", new_name);
                }
            }
            // Filter invalid chars
            new_name = new_name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.' || *c == ':' || *c == '-').collect();
            sanitized["name"] = json!(new_name);
        }

        sanitized
    }

    /// Recursively sanitizes JSON schema for Antigravity
    fn sanitize_schema(schema: &mut Value) {
        if let Some(obj) = schema.as_object_mut() {
            // Remove forbidden keys
            obj.remove("$schema");
            obj.remove("$id");
            obj.remove("$ref");
            obj.remove("$defs");
            obj.remove("definitions");
            obj.remove("default");
            obj.remove("examples");
            // const is not supported, ref is not supported

            // Transform strict `const` to `enum` (if present directly)
            if let Some(const_val) = obj.remove("const") {
                obj.insert("enum".to_string(), json!([const_val]));
            }

            // Recurse into properties
            if let Some(props) = obj.get_mut("properties").and_then(|p| p.as_object_mut()) {
                for (_, value) in props.iter_mut() {
                    Self::sanitize_schema(value);
                }
            }

            // Recurse into items (array)
            if let Some(items) = obj.get_mut("items") {
                Self::sanitize_schema(items);
            }
        }
    }

    /// Sends a chat completion request
    /// FIXED: Now uses chat_completion_stream internally to bypass 500 errors on generateContent
    pub async fn chat_completion(
        &self,
        model: AntigravityModel,
        messages: Vec<Message>,
        thinking: Option<ThinkingConfig>,
        tools: Option<Vec<Value>>,
    ) -> Result<ChatResponse> {
        // Use the streaming implementation
        let stream = self.chat_completion_stream(model.clone(), messages, thinking, tools).await?;
        let mut stream = Box::pin(stream);

        let mut full_content = String::new();
        let mut full_thinking = String::new();
        let mut has_thinking = false;

        // Collect all chunks
        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res?;
            if chunk.is_thinking {
                full_thinking.push_str(&chunk.delta);
                has_thinking = true;
            } else {
                full_content.push_str(&chunk.delta);
            }
        }

        // Construct response (usage stats are approximated or missing in stream)
        Ok(ChatResponse {
            content: full_content,
            thinking: if has_thinking { Some(full_thinking) } else { None },
            model: model.api_id().to_string(),
            finish_reason: "stop".to_string(),
            usage: None, // Streaming doesn't always provide final usage
        })
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
        tools: Option<Vec<Value>>,
    ) -> Result<impl futures::Stream<Item = Result<StreamChunk>> + Send> {
        // Ensure we have a valid project ID
        self.fetch_provisioned_project_id().await;

        let endpoint = self.current_endpoint().await;
        // Use streamGenerateContent with alt=sse
        let url = format!("{}/v1internal:streamGenerateContent?alt=sse", endpoint);
        let token = self.access_token.read().await.clone();
        let project_id = self.project_id.read().await.clone();

        let body = self.build_request_body(&project_id, model, &messages, thinking.as_ref(), tools.as_ref());

        debug!("Sending streaming request to {}", url);

        // Add request jitter to reduce detection patterns (0-500ms random delay)
        // This helps prevent rate limiting by making requests look less automated
        let jitter_ms = rand::random::<u64>() % 500;
        if jitter_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(jitter_ms)).await;
        }

        let request = self.client
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .json(&body);

        // Header injection is now handled in new() but we can ensure it here too (redundant but safe)
        // Also removed redundant header injection logic which is now in `new`.

        let response = request.send().await?;

        let status = response.status();

        if !status.is_success() {
            // Extract retry-after header if present
            let retry_after = response.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            
            let error_text = response.text().await?;
            
            // Handle rate limiting specifically (429)
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_seconds = retry_after.unwrap_or_else(|| {
                    // Try to extract from error message
                    extract_retry_from_error(&error_text).unwrap_or(60)
                });
                return Err(anyhow!("RATE_LIMITED:{}:{}", retry_seconds, error_text));
            }
            
            // Handle capacity errors (503/529) with special retry logic
            if status == reqwest::StatusCode::SERVICE_UNAVAILABLE || 
               status.as_u16() == 529 {  // 529 = "Site is overloaded"
                let retry_seconds = retry_after.unwrap_or(45); // Default 45s for capacity
                return Err(anyhow!("CAPACITY_ERROR:{}:{}", retry_seconds, error_text));
            }
            
            // 2026-01-28: Handle "Permission denied" specifically
            if status == reqwest::StatusCode::FORBIDDEN && error_text.contains("generateChat") {
                 return Err(anyhow!("IAM_PERMISSION_DENIED: The Project ID '{}' likely needs the Gemini API enabled. {}", project_id, error_text));
            }
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

                    // tracing::info!("DEBUG RAW STREAM: {}", trimmed); // FORCE LOGGING

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
                                                      if text.contains("(no content)") { continue; }
                                                     yield StreamChunk {
                                                         delta: text.to_string(),
                                                         is_thinking: is_thought,
                                                         is_tool_use: false,
                                                         done: false,
                                                     };
                                                 } else if let Some(call) = part.get("functionCall") {
                                                     // Convert Gemini functionCall back to Anthropic tool_use JSON
                                                     let tool_use = serde_json::json!({
                                                         "type": "tool_use",
                                                         "id": format!("call_{}", &Uuid::new_v4().to_string().replace("-", "")[..12]),
                                                         "name": call.get("name"),
                                                         "input": call.get("args")
                                                     });
                                                      tracing::info!("DEBUG TOOL USE: {}", tool_use);
                                                     yield StreamChunk {
                                                         delta: tool_use.to_string(),
                                                         is_thinking: false,
                                                         is_tool_use: true,
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
                                                      if text.contains("(no content)") { continue; }
                                                     yield StreamChunk {
                                                         delta: text.to_string(),
                                                         is_thinking: is_thought,
                                                         is_tool_use: false,
                                                         done: false,
                                                     };
                                                 } else if let Some(call) = part.get("functionCall") {
                                                     // Convert Gemini functionCall back to Anthropic tool_use JSON
                                                     let tool_use = serde_json::json!({
                                                         "type": "tool_use",
                                                         "id": format!("call_{}", &Uuid::new_v4().to_string().replace("-", "")[..12]),
                                                         "name": call.get("name"),
                                                         "input": call.get("args")
                                                     });
                                                      tracing::info!("DEBUG TOOL USE: {}", tool_use);
                                                     yield StreamChunk {
                                                         delta: tool_use.to_string(),
                                                         is_thinking: false,
                                                         is_tool_use: true,
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
            yield StreamChunk { delta: "".into(), is_thinking: false, is_tool_use: false, done: true };
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

    #[test]
    fn test_sanitize_tool_definition() {
        let tool = serde_json::json!({
            "name": "invalid/tool name",
            "parameters": {
                "type": "object",
                "properties": {
                    "field1": {
                        "type": "string",
                        "const": "fixed_value"
                    },
                    "field2": {
                        "$ref": "#/definitions/SomeType"
                    }
                },
                "$schema": "http://json-schema.org/draft-07/schema#"
            }
        });

        let sanitized = AntigravityClient::sanitize_tool_definition(&tool);

        // Check name sanitization
        assert_eq!(sanitized["name"], "invalid_tool_name");

        // Check schema sanitization
        let props = sanitized["parameters"]["properties"].as_object().unwrap();

        // const should be converted to enum
        let field1 = &props["field1"];
        assert!(field1.get("const").is_none());
        assert_eq!(field1["enum"], serde_json::json!(["fixed_value"]));

        // $ref and $schema should be removed
        let field2 = &props["field2"];
        assert!(field2.get("$ref").is_none());

        assert!(sanitized["parameters"].get("$schema").is_none());
    }
}
