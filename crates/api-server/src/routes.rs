use axum::{
    extract::{Json, State},
    response::IntoResponse,
};
use common::config::Config;
use serde_json::Value;
use std::sync::Arc;

use crate::state::AppState;

pub async fn chat_completions(
    State(_state): State<AppState>,
    Json(_payload): Json<Value>,
) -> impl IntoResponse {
    tracing::info!("Received chat completion request");
    // TODO: Implement actual logic
    Json(serde_json::json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1677652288,
        "model": "gpt-3.5-turbo",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! This is a stub response from AetherBridge."
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 9,
            "completion_tokens": 12,
            "total_tokens": 21
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
