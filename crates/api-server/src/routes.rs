use axum::{
    extract::{Json, State},
    response::{Html, IntoResponse, Sse, sse::Event},
    http::StatusCode,
    body::Body,
};
use serde_json::Value;
use browser_automator::{AntigravityClient, AntigravityModel, Message as AntigravityMessage};
use futures_util::stream::{self, Stream};
use std::convert::Infallible;

use crate::state::AppState;

/// Health check / welcome page at root
pub async fn health_check() -> Html<&'static str> {
    Html(r#"<!DOCTYPE html>
<html>
<head>
    <title>AetherBridge</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
               background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
               color: #e0e0e0; min-height: 100vh; margin: 0; padding: 40px;
               display: flex; flex-direction: column; align-items: center; }
        .container { max-width: 600px; text-align: center; }
        h1 { color: #00d4aa; font-size: 2.5em; margin-bottom: 0.5em; }
        .status { background: rgba(0, 212, 170, 0.1); border: 1px solid #00d4aa;
                  padding: 20px; border-radius: 8px; margin: 20px 0; }
        .status::before { content: "●"; color: #00d4aa; margin-right: 8px; }
        code { background: rgba(255,255,255,0.1); padding: 2px 8px; border-radius: 4px; }
        .endpoints { text-align: left; background: rgba(0,0,0,0.3); padding: 20px;
                     border-radius: 8px; margin-top: 20px; }
        .endpoint { margin: 10px 0; }
        .method { color: #00d4aa; font-weight: bold; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🌉 AetherBridge</h1>
        <p>Local AI Bridge Server</p>
        <div class="status">Server Running</div>
        <div class="endpoints">
            <h3>Endpoints</h3>
            <div class="endpoint"><span class="method">POST</span> <code>/v1/chat/completions</code> - OpenAI compatible</div>
            <div class="endpoint"><span class="method">POST</span> <code>/v1/messages</code> - Anthropic compatible</div>
            <div class="endpoint"><span class="method">GET</span> <code>/v1/models</code> - List available models</div>
            <div class="endpoint"><span class="method">GET</span> <code>/health</code> - Health check</div>
        </div>
    </div>
</body>
</html>"#)
}

/// Simple health check endpoint
pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "service": "aether-bridge",
        "version": env!("CARGO_PKG_VERSION")
    })))
}

/// List available models (OpenAI compatible)
pub async fn list_models() -> impl IntoResponse {
    Json(serde_json::json!({
        "object": "list",
        "data": [
            {
                "id": "antigravity-gemini-3-pro",
                "object": "model",
                "created": 1700000000,
                "owned_by": "google",
                "permission": [],
                "root": "gemini-3-pro",
                "parent": null
            },
            {
                "id": "antigravity-gemini-3-flash",
                "object": "model",
                "created": 1700000000,
                "owned_by": "google",
                "permission": [],
                "root": "gemini-3-flash",
                "parent": null
            },
            {
                "id": "antigravity-claude-sonnet-4-5",
                "object": "model",
                "created": 1700000000,
                "owned_by": "anthropic",
                "permission": [],
                "root": "claude-sonnet-4.5",
                "parent": null
            },
            {
                "id": "antigravity-claude-sonnet-4-5-thinking",
                "object": "model",
                "created": 1700000000,
                "owned_by": "anthropic",
                "permission": [],
                "root": "claude-sonnet-4.5-thinking",
                "parent": null
            },
            {
                "id": "antigravity-claude-opus-4-5-thinking",
                "object": "model",
                "created": 1700000000,
                "owned_by": "anthropic",
                "permission": [],
                "root": "claude-opus-4.5-thinking",
                "parent": null
            },
            {
                "id": "google-bridge",
                "object": "model",
                "created": 1700000000,
                "owned_by": "aether-bridge",
                "permission": [],
                "root": "google-bridge",
                "parent": null
            }
        ]
    }))
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    tracing::info!("Received chat completion request");

    // Extract model from request
    let model_id = payload["model"].as_str().unwrap_or("antigravity-claude-sonnet-4-5");
    tracing::info!("Requested model: {}", model_id);

    // Check if this is an Antigravity model request
    if model_id.starts_with("antigravity-") {
        return handle_antigravity_request(&state, &payload, model_id).await;
    }

    // Legacy protocol driver fallback
    let empty_vec = vec![];
    let messages = payload["messages"].as_array().unwrap_or(&empty_vec);
    let prompt = messages.iter()
        .filter(|m| m["role"] == "user")
        .last()
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");

    tracing::info!("Prompt: {}", prompt);

    let automator = state.automator.lock().await;

    let response_text = if let Some(protocol) = &automator.protocol {
        match protocol.chat_completion(prompt).await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("Protocol driver error: {}", e);
                format!("Error: {}", e)
            }
        }
    } else {
        "Error: No protocol driver available. Please use 'antigravity-*' models with OAuth authentication.".to_string()
    };

    Json(serde_json::json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model_id,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": response_text
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        }
    })).into_response()
}

/// Handles requests for Antigravity models via OAuth
async fn handle_antigravity_request(
    state: &AppState,
    payload: &Value,
    model_id: &str,
) -> axum::response::Response {
    // Parse the model
    let model = match AntigravityModel::from_str(model_id) {
        Some(m) => m,
        None => {
            tracing::warn!("Unknown Antigravity model: {}", model_id);
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": {
                    "message": format!("Unknown model: {}", model_id),
                    "type": "invalid_request_error"
                }
            }))).into_response();
        }
    };

    // Get an available account
    let account = match state.account_manager.get_available_account().await {
        Some(acc) => acc,
        None => {
            // Check if rate limited
            if let Some(wait_time) = state.account_manager.get_min_wait_time().await {
                tracing::warn!("All accounts rate limited. Wait {} seconds", wait_time.as_secs());
                return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
                    "error": {
                        "message": format!("All accounts rate limited. Retry after {} seconds", wait_time.as_secs()),
                        "type": "rate_limit_error"
                    }
                }))).into_response();
            }

            tracing::error!("No OAuth accounts configured");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "error": {
                    "message": "No Google accounts configured. Please run 'aether login' first.",
                    "type": "authentication_error"
                }
            }))).into_response();
        }
    };

    tracing::info!("Using account: {} for model {}", account.email, model);

    // Create the Antigravity client
    let client = match AntigravityClient::new(account.access_token.clone(), None) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create Antigravity client: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": {
                    "message": format!("Failed to initialize client: {}", e),
                    "type": "api_error"
                }
            }))).into_response();
        }
    };

    // Convert messages
    let empty_vec = vec![];
    let raw_messages = payload["messages"].as_array().unwrap_or(&empty_vec);
    let messages: Vec<AntigravityMessage> = raw_messages.iter()
        .filter_map(|m| {
            let role = m["role"].as_str()?;
            let content = m["content"].as_str()?;
            Some(AntigravityMessage {
                role: role.to_string(),
                content: content.to_string(),
            })
        })
        .collect();

    // Make the API call
    match client.chat_completion(model, messages, None).await {
        Ok(response) => {
            // Clear rate limit on success
            state.account_manager.clear_rate_limit(account.index).await;

            let usage = response.usage.as_ref();
            Json(serde_json::json!({
                "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                "object": "chat.completion",
                "created": chrono::Utc::now().timestamp(),
                "model": model_id,
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": response.content
                    },
                    "finish_reason": response.finish_reason
                }],
                "usage": {
                    "prompt_tokens": usage.map(|u| u.prompt_tokens).unwrap_or(0),
                    "completion_tokens": usage.map(|u| u.completion_tokens).unwrap_or(0),
                    "total_tokens": usage.map(|u| u.total_tokens).unwrap_or(0)
                }
            })).into_response()
        }
        Err(e) => {
            let error_str = e.to_string();

            // Check for rate limiting
            if error_str.starts_with("RATE_LIMITED:") {
                let parts: Vec<&str> = error_str.splitn(3, ':').collect();
                let seconds = parts.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(60);
                let until = chrono::Utc::now() + chrono::Duration::seconds(seconds as i64);

                state.account_manager.mark_rate_limited(account.index, until).await;

                // Try again with next account (recursive call would be cleaner but let's be explicit)
                tracing::warn!("Account {} rate limited, marked for {} seconds", account.email, seconds);

                return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
                    "error": {
                        "message": format!("Rate limited. Retry after {} seconds", seconds),
                        "type": "rate_limit_error"
                    }
                }))).into_response();
            }

            tracing::error!("Antigravity API error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": {
                    "message": error_str,
                    "type": "api_error"
                }
            }))).into_response()
        }
    }
}

/// Anthropic Messages API endpoint (Claude CLI compatible)
/// This enables: ANTHROPIC_BASE_URL=http://127.0.0.1:8080 claude-code
pub async fn messages(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    tracing::info!("Received Anthropic messages request");

    // Check if streaming is requested
    let is_streaming = payload.get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if is_streaming {
        tracing::info!("Streaming mode requested");
        return messages_streaming(state, payload).await.into_response();
    }

    // Extract model from request and map to Antigravity
    let requested_model = payload["model"].as_str().unwrap_or("claude-3-5-sonnet-20241022");
    tracing::info!("Anthropic model requested: {}", requested_model);

    // Map Anthropic model IDs to Antigravity models
    let model = map_anthropic_to_antigravity(requested_model);
    tracing::info!("Mapped to Antigravity model: {:?}", model);

    // Check for extended thinking via anthropic-beta header or thinking field
    let thinking_enabled = payload.get("thinking").is_some()
        || payload.get("extended_thinking").is_some();

    // Get an available OAuth account
    let account = match state.account_manager.get_available_account().await {
        Some(acc) => acc,
        None => {
            if let Some(wait_time) = state.account_manager.get_min_wait_time().await {
                tracing::warn!("All accounts rate limited. Wait {} seconds", wait_time.as_secs());
                return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "rate_limit_error",
                        "message": format!("Rate limited. Retry after {} seconds", wait_time.as_secs())
                    }
                }))).into_response();
            }

            tracing::error!("No OAuth accounts configured");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "authentication_error",
                    "message": "No Google accounts configured. Run AetherBridge TUI and press [L] to login."
                }
            }))).into_response();
        }
    };

    tracing::info!("Using account: {} for Anthropic request", account.email);

    // Create Antigravity client
    let client = match AntigravityClient::new(account.access_token.clone(), None) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create Antigravity client: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "api_error",
                    "message": format!("Failed to initialize client: {}", e)
                }
            }))).into_response();
        }
    };

    // Convert Anthropic messages to Antigravity format
    let messages = convert_anthropic_messages(&payload);

    // Configure thinking if enabled and supported
    let thinking_config = if thinking_enabled && model.supports_thinking() {
        // Extract budget from request if specified
        let budget = payload["thinking"]
            .get("budget_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .or(Some(10000)); // Default budget
        Some(browser_automator::ThinkingConfig {
            budget: budget,
            level: None,
            include_thoughts: true,
        })
    } else {
        None
    };

    // Make the API call
    match client.chat_completion(model, messages, thinking_config).await {
        Ok(response) => {
            state.account_manager.clear_rate_limit(account.index).await;

            // Build content blocks (Anthropic format)
            let mut content_blocks = Vec::new();

            // Add thinking block if present
            if let Some(ref thinking) = response.thinking {
                content_blocks.push(serde_json::json!({
                    "type": "thinking",
                    "thinking": thinking
                }));
            }

            // Add main text content
            content_blocks.push(serde_json::json!({
                "type": "text",
                "text": response.content
            }));

            let usage = response.usage.as_ref();

            Json(serde_json::json!({
                "id": format!("msg_{}", &uuid::Uuid::new_v4().to_string().replace("-", "")[..24]),
                "type": "message",
                "role": "assistant",
                "content": content_blocks,
                "model": requested_model,
                "stop_reason": &response.finish_reason,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": usage.map(|u| u.prompt_tokens).unwrap_or(0),
                    "output_tokens": usage.map(|u| u.completion_tokens).unwrap_or(0)
                }
            })).into_response()
        }
        Err(e) => {
            let error_str = e.to_string();

            // Handle rate limiting
            if error_str.starts_with("RATE_LIMITED:") {
                let parts: Vec<&str> = error_str.splitn(3, ':').collect();
                let seconds = parts.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(60);
                let until = chrono::Utc::now() + chrono::Duration::seconds(seconds as i64);

                state.account_manager.mark_rate_limited(account.index, until).await;
                tracing::warn!("Account {} rate limited for {} seconds", account.email, seconds);

                return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "rate_limit_error",
                        "message": format!("Rate limited. Retry after {} seconds", seconds)
                    }
                }))).into_response();
            }

            tracing::error!("Antigravity API error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "api_error",
                    "message": error_str
                }
            }))).into_response()
        }
    }
}

/// Maps Anthropic model IDs to Antigravity models
fn map_anthropic_to_antigravity(model_id: &str) -> AntigravityModel {
    if model_id.contains("opus") {
        // Claude Opus models → Claude Opus 4.5 Thinking
        AntigravityModel::ClaudeOpus45Thinking
    } else if model_id.contains("sonnet") {
        // Check for thinking/extended patterns
        if model_id.contains("think") {
            AntigravityModel::ClaudeSonnet45Thinking
        } else {
            AntigravityModel::ClaudeSonnet45
        }
    } else if model_id.contains("haiku") {
        // Haiku → use Flash for speed
        AntigravityModel::Gemini3Flash
    } else if model_id.contains("gemini") {
        if model_id.contains("flash") {
            AntigravityModel::Gemini3Flash
        } else {
            AntigravityModel::Gemini3Pro
        }
    } else {
        // Default to Sonnet 4.5
        AntigravityModel::ClaudeSonnet45
    }
}

/// Converts Anthropic message format to Antigravity format
fn convert_anthropic_messages(payload: &Value) -> Vec<AntigravityMessage> {
    let mut messages = Vec::new();

    // Handle system prompt
    if let Some(system) = payload.get("system") {
        let system_text = if let Some(s) = system.as_str() {
            s.to_string()
        } else if let Some(arr) = system.as_array() {
            // System can be array of content blocks
            arr.iter()
                .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };

        if !system_text.is_empty() {
            messages.push(AntigravityMessage {
                role: "system".to_string(),
                content: system_text,
            });
        }
    }

    // Handle conversation messages
    if let Some(msgs) = payload.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");

            // Content can be string or array of content blocks
            let content = if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                text.to_string()
            } else if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                // Extract text from content blocks
                blocks.iter()
                    .filter_map(|block| {
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            block.get("text").and_then(|t| t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            };

            if !content.is_empty() {
                messages.push(AntigravityMessage {
                    role: role.to_string(),
                    content,
                });
            }
        }
    }

    messages
}

/// Streaming version of /v1/messages endpoint
/// Returns SSE events in Anthropic format: message_start, content_block_delta, message_stop
async fn messages_streaming(
    state: AppState,
    payload: Value,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Generate message ID upfront
    let message_id = format!("msg_{}", &uuid::Uuid::new_v4().to_string().replace("-", "")[..24]);
    let requested_model = payload["model"].as_str().unwrap_or("claude-3-5-sonnet-20241022").to_string();
    let model = map_anthropic_to_antigravity(&requested_model);

    // Check for thinking mode
    let thinking_enabled = payload.get("thinking").is_some()
        || payload.get("extended_thinking").is_some();

    // Clone state for async move
    let account_manager = state.account_manager.clone();

    // Create the stream
    let stream = async_stream::stream! {
        // Try to get an account
        let account = match account_manager.get_available_account().await {
            Some(acc) => acc,
            None => {
                // Emit error event
                let error_event = serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "authentication_error",
                        "message": "No Google accounts configured. Run AetherBridge TUI and press [L] to login."
                    }
                });
                yield Ok(Event::default().event("error").data(error_event.to_string()));
                return;
            }
        };

        tracing::info!("Streaming with account: {}", account.email);

        // Create client
        let client = match AntigravityClient::new(account.access_token.clone(), None) {
            Ok(c) => c,
            Err(e) => {
                let error_event = serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": format!("Failed to initialize client: {}", e)
                    }
                });
                yield Ok(Event::default().event("error").data(error_event.to_string()));
                return;
            }
        };

        // Convert messages
        let messages = convert_anthropic_messages(&payload);

        // Configure thinking
        let thinking_config = if thinking_enabled && model.supports_thinking() {
            let budget = payload["thinking"]
                .get("budget_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .or(Some(10000));
            Some(browser_automator::ThinkingConfig {
                budget,
                level: None,
                include_thoughts: true,
            })
        } else {
            None
        };

        // Emit message_start event
        let message_start = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": &message_id,
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": &requested_model,
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0
                }
            }
        });
        yield Ok(Event::default().event("message_start").data(message_start.to_string()));

        // Make the actual API call (non-streaming for now - Antigravity doesn't expose SSE)
        match client.chat_completion(model, messages, thinking_config).await {
            Ok(response) => {
                account_manager.clear_rate_limit(account.index).await;

                let mut block_index = 0;

                // If there's thinking content, emit it first
                if let Some(ref thinking) = response.thinking {
                    // content_block_start for thinking
                    let block_start = serde_json::json!({
                        "type": "content_block_start",
                        "index": block_index,
                        "content_block": {
                            "type": "thinking",
                            "thinking": ""
                        }
                    });
                    yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));

                    // Emit thinking content in chunks (simulate streaming)
                    for chunk in thinking.chars().collect::<Vec<_>>().chunks(50) {
                        let chunk_str: String = chunk.iter().collect();
                        let delta = serde_json::json!({
                            "type": "content_block_delta",
                            "index": block_index,
                            "delta": {
                                "type": "thinking_delta",
                                "thinking": chunk_str
                            }
                        });
                        yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));
                        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                    }

                    // content_block_stop
                    let block_stop = serde_json::json!({
                        "type": "content_block_stop",
                        "index": block_index
                    });
                    yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

                    block_index += 1;
                }

                // Emit main text content
                let text_block_start = serde_json::json!({
                    "type": "content_block_start",
                    "index": block_index,
                    "content_block": {
                        "type": "text",
                        "text": ""
                    }
                });
                yield Ok(Event::default().event("content_block_start").data(text_block_start.to_string()));

                // Stream the text content in chunks (simulate streaming)
                for chunk in response.content.chars().collect::<Vec<_>>().chunks(20) {
                    let chunk_str: String = chunk.iter().collect();
                    let delta = serde_json::json!({
                        "type": "content_block_delta",
                        "index": block_index,
                        "delta": {
                            "type": "text_delta",
                            "text": chunk_str
                        }
                    });
                    yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }

                // content_block_stop
                let text_block_stop = serde_json::json!({
                    "type": "content_block_stop",
                    "index": block_index
                });
                yield Ok(Event::default().event("content_block_stop").data(text_block_stop.to_string()));

                // Emit message_delta with stop reason
                let usage = response.usage.as_ref();
                let message_delta = serde_json::json!({
                    "type": "message_delta",
                    "delta": {
                        "stop_reason": &response.finish_reason,
                        "stop_sequence": null
                    },
                    "usage": {
                        "output_tokens": usage.map(|u| u.completion_tokens).unwrap_or(0)
                    }
                });
                yield Ok(Event::default().event("message_delta").data(message_delta.to_string()));

                // Emit message_stop
                let message_stop = serde_json::json!({
                    "type": "message_stop"
                });
                yield Ok(Event::default().event("message_stop").data(message_stop.to_string()));
            }
            Err(e) => {
                let error_str = e.to_string();

                // Check for rate limiting
                if error_str.starts_with("RATE_LIMITED:") {
                    let parts: Vec<&str> = error_str.splitn(3, ':').collect();
                    let seconds = parts.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(60);
                    let until = chrono::Utc::now() + chrono::Duration::seconds(seconds as i64);
                    account_manager.mark_rate_limited(account.index, until).await;
                }

                let error_event = serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": error_str
                    }
                });
                yield Ok(Event::default().event("error").data(error_event.to_string()));
            }
        }
    };

    Sse::new(stream)
}
