use axum::{
    extract::{Json, State},
    response::IntoResponse,
};
use common::config::Config;
use serde_json::Value;
use std::sync::Arc;

use crate::state::AppState;

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

    let automator = state.automator.lock().await; // Async mutex lock

    let response_text = if let Some(protocol) = &automator.protocol {
        match protocol.chat_completion(prompt).await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("Protocol driver error: {}", e);
                format!("Error: {}", e)
            }
        }
    } else {
        "Error: No protocol driver available".to_string()
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
    State(_state): State<AppState>,
    Json(_payload): Json<Value>,
) -> impl IntoResponse {
    tracing::info!("Received messages request");
    // TODO: Implement actual logic
    Json(serde_json::json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": "Hello! This is a stub response for Anthropic Messages API."
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
