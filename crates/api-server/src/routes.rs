use axum::{
    extract::{Json, State},
    response::{Html, IntoResponse},
    http::StatusCode,
};
use serde_json::Value;

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
                "id": "google-bridge",
                "object": "model",
                "created": 1700000000,
                "owned_by": "aether-bridge",
                "permission": [],
                "root": "google-bridge",
                "parent": null
            },
            {
                "id": "anthropic-bridge",
                "object": "model",
                "created": 1700000000,
                "owned_by": "aether-bridge",
                "permission": [],
                "root": "anthropic-bridge",
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

    // Extract prompt from the last user message
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
        "Error: No protocol driver available. Please ensure you are logged into your AI provider in a supported browser before starting AetherBridge.".to_string()
    };

    Json(serde_json::json!({
        "id": "chatcmpl-bridge",
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": "google-bridge",
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
    }))
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
