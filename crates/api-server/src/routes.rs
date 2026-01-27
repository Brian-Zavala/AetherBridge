use axum::{
    extract::{Json, State},
    response::{Html, IntoResponse},
    http::StatusCode,
};
use serde_json::Value;
use browser_automator::{AntigravityClient, AntigravityModel, Message as AntigravityMessage};

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

pub async fn messages(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    tracing::info!("Received Anthropic messages request");

    // Extract prompt from messages
    let empty_vec = vec![];
    let messages = payload["messages"].as_array().unwrap_or(&empty_vec);
    let prompt = messages.iter()
        .filter(|m| m["role"] == "user")
        .last()
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");

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
        "Error: No protocol driver available. Please ensure you are logged into your AI provider.".to_string()
    };

    Json(serde_json::json!({
        "id": format!("msg_{}", chrono::Utc::now().timestamp()),
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": response_text
            }
        ],
        "model": "claude-3-opus-20240229",
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 10,
            "output_tokens": 20
        }
    }))
}
