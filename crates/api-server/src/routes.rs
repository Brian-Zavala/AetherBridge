use axum::{
    extract::{Json, State},
    response::{Html, IntoResponse, Sse, sse::Event},
    http::StatusCode,
};
use serde_json::{Value, json};
use browser_automator::{AntigravityClient, AntigravityModel, Message as AntigravityMessage};
use futures_util::stream::Stream;
use std::convert::Infallible;

use crate::state::AppState;
use crate::session_recovery::{recover_session, is_recoverable_error, format_recovery_summary};
use oauth::accounts::ModelFamily;

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

/// Helper to convert Anthropic tools to Gemini function declarations
fn convert_anthropic_tools(payload: &Value) -> Option<Vec<Value>> {
    if let Some(tools_array) = payload.get("tools").and_then(|t| t.as_array()) {
        let converted: Vec<Value> = tools_array.iter().map(|tool| {
            let mut params = tool["input_schema"].clone();
            sanitize_schema(&mut params);

            serde_json::json!({
                "name": tool["name"],
                "description": tool["description"],
                "parameters": params
            })
        }).collect();

        if !converted.is_empty() {
            return Some(converted);
        }
    }
    None
}

/// Recursively sanitizes JSON schema to remove fields forbidden by Antigravity API
fn sanitize_schema(schema: &mut Value) {
    if let Some(obj) = schema.as_object_mut() {
        // 1. Remove forbidden top-level keys
        let forbidden_keys = [
            "$schema", "$id", "default", "title", "examples",
            "minLength", "maxLength", "pattern", "format",
            "minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum", "multipleOf",
            "minItems", "maxItems", "uniqueItems",
            "minProperties", "maxProperties", "propertyNames",
            "const", "contentMediaType", "contentEncoding",
            "additionalProperties" // Often strict in Gemini
        ];

        for key in forbidden_keys {
            obj.remove(key);
        }

        // 2. Recursively process known nested structures
        if let Some(properties) = obj.get_mut("properties").and_then(|p| p.as_object_mut()) {
            for (_, prop_schema) in properties.iter_mut() {
                sanitize_schema(prop_schema);
            }
        }

        if let Some(items) = obj.get_mut("items") {
             sanitize_schema(items);
        }

        // Handle array of schemas (allOf, anyOf, oneOf)
        for key in ["allOf", "anyOf", "oneOf"] {
            if let Some(arr) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
                for sub_schema in arr {
                    sanitize_schema(sub_schema);
                }
            }
        }
    }
}

/// Mock organization endpoint - Claude CLI calls this on startup
pub async fn get_organization() -> impl IntoResponse {
    Json(serde_json::json!({
        "id": "org_aetherbridge",
        "name": "AetherBridge Local",
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
    }))
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
    // Get an available account with retry queueing
    let account = loop {
        match state.account_manager.get_available_account().await {
            Some(acc) => break acc,
            None => {
                // Check wait time
                if let Some(wait_time) = state.account_manager.get_min_wait_time_for_model(&model_id.to_string()).await {
                    let wait_secs = wait_time.as_secs();
                    if wait_secs > 600 { // Cap wait time at 10 minutes (claude-code-router default timeout is 1h)
                         tracing::warn!("All accounts rate limited. Wait time {}s too long.", wait_secs);
                         return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
                            "error": {
                                "message": format!("All accounts rate limited. Retry after {} seconds", wait_secs),
                                "type": "rate_limit_error"
                            }
                        }))).into_response();
                    }

                    tracing::info!("All accounts rate limited. Queuing request for {} seconds...", wait_secs);
                    tokio::time::sleep(wait_time + std::time::Duration::from_secs(1)).await;
                    continue;
                }

                tracing::error!("No OAuth accounts configured");
                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                    "error": {
                        "message": "No Google accounts configured. Please run 'aether login' first.",
                        "type": "authentication_error"
                    }
                }))).into_response();
            }
        }
    };

    tracing::info!("Using account: {} for model {}", account.email, model);

    // Create the Antigravity client with user's project ID from config
    let project_id = state.config.project_id.clone();
    let client = match AntigravityClient::new(account.access_token.clone(), project_id, Some((*state.fingerprint).clone())) {
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

    // Extract valid tools
    let tools = convert_anthropic_tools(payload);

    // Make the API call
    match client.chat_completion(model, messages, None, tools).await {
        Ok(response) => {
            // Clear rate limit on success
            state.account_manager.clear_rate_limit(account.index, ModelFamily::from_model_id(&model.api_id().to_string())).await;

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

            // Check for rate limiting or capacity errors
            if error_str.starts_with("RATE_LIMITED:") || error_str.starts_with("CAPACITY_ERROR:") {
                let parts: Vec<&str> = error_str.splitn(3, ':').collect();
                let seconds = parts.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(60);
                
                // Use longer backoff for capacity errors
                let is_capacity = error_str.starts_with("CAPACITY_ERROR:");
                let effective_seconds = if is_capacity {
                    std::cmp::max(seconds, 45)
                } else {
                    seconds
                };
                
                let until = chrono::Utc::now() + chrono::Duration::seconds(effective_seconds as i64);

                state.account_manager.mark_rate_limited(account.index, ModelFamily::from_model_id(&model.api_id().to_string()), until).await;

                let error_type = if is_capacity { "capacity_error" } else { "rate_limit_error" };
                tracing::warn!("Account {} {} for {} seconds", account.email, error_type, effective_seconds);

                return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
                    "error": {
                        "message": format!("Rate limited. Retry after {} seconds", effective_seconds),
                        "type": error_type
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
    tracing::info!(">>> PAYLOAD: {:?}", payload); // DEBUG: PROOF OF LIFE

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
    let mut model = map_anthropic_to_antigravity(requested_model);
    tracing::info!("Mapped to Antigravity model: {:?}", model);

    // Check for extended thinking via anthropic-beta header or thinking field
    let thinking_enabled = payload.get("thinking").is_some()
        || payload.get("extended_thinking").is_some();

    // Get an available OAuth account
    // Get an available OAuth account with retry queuing
    let account = loop {
        match state.account_manager.get_available_account().await {
            Some(acc) => break acc,
            None => {
                // Check for Pre-emptive Spoofing (Strategy 0)
                tracing::info!("Primary model rate limited. Checking Strategy 0 fallback for {:?}", model);
                if let Some(spoof_model) = get_spoof_model(model) {
                     tracing::info!("Spoof model available: {:?}", spoof_model);
                     if let Some(acc) = state.account_manager.get_available_account_ignoring_rate_limit().await {
                         // Log the pre-emptive switch
                         tracing::info!("Strategy 0: Ignoring rate limit and using account {} for spoof model {:?}", acc.email, spoof_model);
                         // Swap model and proceed
                         model = spoof_model;
                         break acc;
                     } else {
                         tracing::warn!("Strategy 0 Failed: Could not find ANY account (even ignoring rate limits) to try spoofing.");
                     }
                } else {
                    tracing::info!("No spoof model defined for {:?}, skipping Strategy 0.", model);
                }

                if let Some(wait_time) = state.account_manager.get_min_wait_time_for_model(&requested_model).await {
                    let wait_secs = wait_time.as_secs();
                    if wait_secs > 600 {
                         tracing::warn!("All accounts rate limited. Wait time {}s too long.", wait_secs);
                         return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
                            "type": "error",
                            "error": {
                                "type": "rate_limit_error",
                                "message": format!("Rate limited. Retry after {} seconds", wait_secs)
                            }
                        }))).into_response();
                    }

                    tracing::info!("All accounts rate limited. Queuing Anthropic request for {} seconds...", wait_secs);
                    tokio::time::sleep(wait_time + std::time::Duration::from_secs(1)).await;
                    continue;
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
        }
    };

    tracing::info!("Using account: {} for Anthropic request", account.email);

    // Create Antigravity client with user's project ID from config
    let project_id = state.config.project_id.clone();
    let client = match AntigravityClient::new(account.access_token.clone(), project_id.clone(), Some((*state.fingerprint).clone())) {
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

        let level = if let Some(b) = budget {
            if b < 5000 { "low" } else if b < 15000 { "medium" } else { "high" }
        } else {
            "low"
        };

        Some(browser_automator::ThinkingConfig {
            budget: budget,
            level: Some(level.to_string()),
            include_thoughts: true,
        })
    } else {
        None
    };

    // Extract tools and convert to Gemini format
    // Extract tools from payload
    let tools = convert_anthropic_tools(&payload);

    // Make the API call with potential spoofing
    let result = client.chat_completion(model, messages.clone(), thinking_config.clone(), tools.clone()).await;

    // Track if we used a fallback strategy (don't clear rate limit if we did)
    let mut used_fallback = false;

     let api_result = match result {
         Err(e) => {
             let error_str = e.to_string();
             tracing::warn!("Antigravity API Error: '{}'", error_str);

             // Check if this is a recoverable session error (tool_use without tool_result, etc.)
             if is_recoverable_error(&error_str) {
                 tracing::warn!("Recoverable session error detected: {}. Attempting recovery and retry...", error_str);
                 
                 // Re-convert messages with session recovery applied
                 let recovered_messages = convert_anthropic_messages(&payload);
                 
                 // Retry the request with recovered messages
                 match client.chat_completion(model, recovered_messages, thinking_config.clone(), tools.clone()).await {
                     Ok(res) => {
                         tracing::info!("Session recovery retry succeeded!");
                         Ok(res)
                     }
                     Err(e2) => {
                         tracing::error!("Session recovery retry failed: {}", e2);
                         Err(e2)
                     }
                 }
             } else if error_str.starts_with("RATE_LIMITED:")
                 || error_str.starts_with("CAPACITY_ERROR:")
                 || error_str.contains("429")
                 || error_str.contains("503")
                 || error_str.contains("529")
             {
                 used_fallback = true; // Mark that we're using fallback strategies
                 
                 // Parse retry duration from RATE_LIMITED:seconds:error or CAPACITY_ERROR:seconds:error
                 let parts: Vec<&str> = error_str.splitn(3, ':').collect();
                 let seconds = parts.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(60);
                 
                 // Use exponential backoff for capacity errors (base 45s with exponential increase)
                 let is_capacity = error_str.starts_with("CAPACITY_ERROR:");
                 let effective_seconds = if is_capacity {
                     // For capacity errors, use longer initial backoff
                     std::cmp::max(seconds, 45)
                 } else {
                     seconds
                 };
                 
                 let until = chrono::Utc::now() + chrono::Duration::seconds(effective_seconds as i64);

                  // Mark CURRENT account as rate limited
                  state.account_manager.mark_rate_limited(account.index, ModelFamily::from_model_id(&model.api_id().to_string()), until).await;
                  tracing::warn!("Account {} rate limited. Attempting mitigation strategies...", account.index);

                 // Strategy 1: Spoof on SAME account
                 let mut spoof_success = false;
                 let mut final_res = Err(e); // Default to original error

                 if let Some(spoof_model) = get_spoof_model(model) {
                     tracing::info!("Strategy 1: Spoofing {:?} on same account...", spoof_model);
                     let spoof_config = adapt_config_for_spoof(&thinking_config, spoof_model);
                     match client.chat_completion(spoof_model, messages.clone(), spoof_config.clone(), tools.clone()).await {
                         Ok(res) => {
                             spoof_success = true;
                             final_res = Ok(res);
                         },
                         Err(e2) => {
                             tracing::warn!("Strategy 1 Failed: {}", e2);
                             // If this failed, it's likely a project-wide ban. We MUST rotate.
                         }
                     }
                 }

                  if !spoof_success {
                      // Strategy 1.5: Dual Quota Fallback (Gemini CLI headers)
                      // Only for Gemini models - try alternate quota pool before rotating accounts
                      if model.is_gemini() {
                          tracing::info!("Strategy 1.5: Attempting dual quota fallback with Gemini CLI headers...");
                          
                          // Create a new client with Gemini CLI headers
                          let cli_client = match AntigravityClient::new(
                              account.access_token.clone(), 
                              project_id.clone(), 
                              Some((*state.fingerprint).clone())
                          ) {
                              Ok(mut c) => {
                                  // Enable dual quota mode
                                  c.set_quota_fallback(true).await;
                                  // Switch to Gemini CLI headers
                                  if let Err(e) = c.switch_to_gemini_cli_headers().await {
                                      tracing::warn!("Failed to switch to Gemini CLI headers: {}", e);
                                      None
                                  } else {
                                      Some(c)
                                  }
                              }
                              Err(e) => {
                                  tracing::warn!("Failed to create CLI client: {}", e);
                                  None
                              }
                          };
                          
                          if let Some(ref cli_c) = cli_client {
                              // Try the same model with Gemini CLI headers
                              match cli_c.chat_completion(model, messages.clone(), thinking_config.clone(), tools.clone()).await {
                                  Ok(res) => {
                                      tracing::info!("Strategy 1.5 SUCCESS: Dual quota worked!");
                                      spoof_success = true;
                                      final_res = Ok(res);
                                  }
                                  Err(e2) => {
                                      tracing::warn!("Strategy 1.5 Failed: {}", e2);
                                      // Continue to Strategy 2
                                  }
                              }
                          }
                      }
                  }

                  if !spoof_success {
                      // Strategy 2: Rotate Account (Absolute Fallback)
                      tracing::info!("Strategy 2: Rotating account...");
                      if let Some(new_account) = state.account_manager.get_available_account().await {
                          tracing::info!("Switched to account: {}", new_account.email);
                          if let Ok(new_client) = AntigravityClient::new(new_account.access_token.clone(), project_id.clone(), Some((*state.fingerprint).clone())) {

                              // Try Spoof immediately on new account
                              let target_model = if let Some(spoof) = get_spoof_model(model) { spoof } else { model };
                              let target_config = if target_model != model {
                                  adapt_config_for_spoof(&thinking_config, target_model)
                              } else {
                                  thinking_config.clone()
                              };

                               match new_client.chat_completion(target_model, messages, target_config, tools.clone()).await {
                                   Ok(res) => {
                                       // NOTE: Don't clear rate limit on original account
                                       // The primary model is still rate-limited, we just used a fallback
                                       final_res = Ok(res);
                                   },
                                   Err(e3) => {
                                       tracing::error!("Strategy 2 Failed: {}", e3);
                                       final_res = Err(e3);
                                   }
                               }
                          }
                      } else {
                          tracing::error!("No alternative accounts available.");
                      }
                  }
                 final_res
            } else {
                Err(e)
            }
        },
        Ok(res) => Ok(res),
    };

    match api_result {
        Ok(response) => {
            // Only clear rate limit if the PRIMARY request succeeded (not fallback)
            if !used_fallback {
                state.account_manager.clear_rate_limit(account.index, ModelFamily::from_model_id(&model.api_id().to_string())).await;
            }

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

            // Handle rate limiting and capacity errors
            if error_str.starts_with("RATE_LIMITED:") || error_str.starts_with("CAPACITY_ERROR:") {
                let parts: Vec<&str> = error_str.splitn(3, ':').collect();
                let seconds = parts.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(60);
                
                // Use longer backoff for capacity errors
                let is_capacity = error_str.starts_with("CAPACITY_ERROR:");
                let effective_seconds = if is_capacity {
                    std::cmp::max(seconds, 45)
                } else {
                    seconds
                };
                
                let until = chrono::Utc::now() + chrono::Duration::seconds(effective_seconds as i64);

                state.account_manager.mark_rate_limited(account.index, ModelFamily::from_model_id(&model.api_id().to_string()), until).await;
                let error_type = if is_capacity { "capacity_error" } else { "rate_limit_error" };
                tracing::warn!("Account {} {} for {} seconds", account.email, error_type, effective_seconds);

                return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": error_type,
                        "message": format!("Rate limited. Retry after {} seconds", effective_seconds)
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

/// Returns the Gemini spoof model for a given Anthropic model
fn get_spoof_model(model: AntigravityModel) -> Option<AntigravityModel> {
    match model {
        AntigravityModel::ClaudeOpus45Thinking => Some(AntigravityModel::Gemini3Pro),
        AntigravityModel::ClaudeSonnet45Thinking | AntigravityModel::ClaudeSonnet45 => Some(AntigravityModel::Gemini3Flash),
        _ => None,
    }
}

/// Adapts thinking configuration when spoofing (e.g., mapping budget to level)
fn adapt_config_for_spoof(
    config: &Option<browser_automator::ThinkingConfig>,
    target_model: AntigravityModel,
) -> Option<browser_automator::ThinkingConfig> {
    let mut new_config = config.clone();

    if let Some(ref mut cfg) = new_config {
        // If switching to Gemini (which uses level)
        if !target_model.is_claude() {
            // Flash doesn't support "high" or has different constraints. safely force medium.
            if matches!(target_model, AntigravityModel::Gemini3Flash) {
                 cfg.level = Some("medium".to_string());
            } else if cfg.level.is_none() {
                // Default to "high" for Pro if not specified
                cfg.level = Some("high".to_string());
            }
        }
    }

    new_config
}

/// Converts Anthropic message format to Antigravity format
fn convert_anthropic_messages(payload: &Value) -> Vec<AntigravityMessage> {
    let mut messages = Vec::new();

    // Handle system prompt
    let system_text = if let Some(system) = payload.get("system") {
        if let Some(s) = system.as_str() {
            s.to_string()
        } else if let Some(arr) = system.as_array() {
            // System can be array of content blocks
            arr.iter()
                .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Handle conversation messages
    let mut conversation_messages: Vec<Value> = Vec::new();
    if let Some(msgs) = payload.get("messages").and_then(|m| m.as_array()) {
        conversation_messages = msgs.clone();
    }

    // Apply session recovery to fix corrupted conversation states
    // This handles: tool_use without tool_result, thinking block order issues
    let recovery_result = recover_session(&conversation_messages);
    if recovery_result.was_recovered {
        tracing::info!("{}", format_recovery_summary(&recovery_result));
        conversation_messages = recovery_result.messages;
    }

    // Add system message if present
    if !system_text.is_empty() {
        messages.push(AntigravityMessage {
            role: "system".to_string(),
            content: system_text,
        });
    }

    // Convert recovered messages to Antigravity format
    for msg in conversation_messages {
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
    let project_id = state.config.project_id.clone();
    let fingerprint = state.fingerprint.clone();

    // Create the stream
    let stream = async_stream::stream! {
        // 1. Emit message_start IMMEDIATELY to ack connection
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

        // 2. Start a "System Log" block to report status (as text so it's visible)
        let mut block_index = 0;
        let status_block_index = block_index;
        // Track whether status block is still open - critical for preventing malformed SSE
        // Note: Compiler warns about unused assignments due to async stream macro limitations,
        // but this variable IS read across yield boundaries in error handling paths
        #[allow(unused_assignments)]
        let mut status_block_open = true;

        // Use a text block for status updates because 'thinking' blocks are often hidden/collapsed in UIs
        let block_start = serde_json::json!({
            "type": "content_block_start",
            "index": status_block_index,
            "content_block": {
                "type": "text",
                "text": ""
            }
        });
        yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));

        // Helper to send status text
        let status_msg = "> **AetherBridge System Log**\n> Finding available account...\n";
        let delta = serde_json::json!({
             "type": "content_block_delta",
             "index": status_block_index,
             "delta": { "type": "text_delta", "text": status_msg }
        });
        yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));


        // 3. Get Account Loop with Status Updates
        let mut model = model; // Make mutable for spoofing
        // Track if we used a fallback strategy (don't clear rate limit if we did)
        let mut used_fallback = false;
        // Track the original model for rate limit clearing
        let original_model = model;
        let account = loop {
             match account_manager.get_available_account().await {
                Some(acc) => break acc,
                None => {
                    // Check for Pre-emptive Spoofing (Strategy 0)
                    tracing::info!("Primary model rate limited. Checking Strategy 0 fallback for {:?}", model);
                    if let Some(spoof_model) = get_spoof_model(model) {
                         tracing::info!("Spoof model available: {:?}", spoof_model);
                          if let Some(acc) = account_manager.get_available_account_ignoring_rate_limit().await {
                              // Log the pre-emptive switch with clear messaging about which model is rate limited
                              tracing::info!("Strategy 0: {} is rate limited. Spoofing to {} on account {}", model.display_name(), spoof_model.display_name(), acc.email);
                              let msg = format!("> ⚠️  {} is currently rate limited.\n> 🔄  Switching to {} (fallback model) on account {}...\n", model.display_name(), spoof_model.display_name(), acc.email);
                              let delta = serde_json::json!({
                                   "type": "content_block_delta",
                                   "index": status_block_index,
                                   "delta": { "type": "text_delta", "text": msg }
                              });
                              yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));

                              // Swap model and mark that we used a fallback
                              model = spoof_model;
                              used_fallback = true;
                              break acc;
                         } else {
                             tracing::warn!("Strategy 0 Failed: Could not find ANY account (even ignoring rate limits) to try spoofing.");
                         }
                    } else {
                        tracing::info!("No spoof model defined for {:?}, skipping Strategy 0.", model);
                    }

                    if let Some(wait_time) = account_manager.get_min_wait_time_for_model(&requested_model).await {
                        let wait_secs = wait_time.as_secs();
                        if wait_secs > 600 {
                            // Close status block
                            let block_stop = serde_json::json!({ "type": "content_block_stop", "index": status_block_index });
                            yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

                            // Report Error
                            let error_event = serde_json::json!({
                                "type": "error",
                                "error": {
                                    "type": "rate_limit_error",
                                    "message": format!("Rate limited. Retry after {} seconds", wait_secs)
                                }
                            });
                            yield Ok(Event::default().event("error").data(error_event.to_string()));
                            return;
                        }

                        // Report waiting status
                        let msg = format!("> Rate limited. Queuing for {} seconds...\n", wait_secs);
                        let delta = serde_json::json!({
                             "type": "content_block_delta",
                             "index": status_block_index,
                             "delta": { "type": "text_delta", "text": msg }
                        });
                        yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));

                        tokio::time::sleep(wait_time + std::time::Duration::from_secs(1)).await;
                        continue;
                    }

                    // No accounts configured
                     let block_stop = serde_json::json!({ "type": "content_block_stop", "index": status_block_index });
                     yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

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
            }
        };

        tracing::info!("Streaming with account: {}", account.email);

        // Report Processing
        let msg = format!("> Using account: {}. Generating response...\n\n", account.email);
        let delta = serde_json::json!({
                "type": "content_block_delta",
                "index": status_block_index,
                "delta": { "type": "text_delta", "text": msg }
        });
        yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));


        // 4. Create Client
        let client = match AntigravityClient::new(account.access_token.clone(), project_id.clone(), Some((*fingerprint).clone())) {
            Ok(c) => c,
            Err(e) => {
                let block_stop = serde_json::json!({ "type": "content_block_stop", "index": status_block_index });
                yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

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

        // Close our status block so the real answer starts clean (or continues?)
        // Let's close it so the real answer is distinct.
        let block_stop = serde_json::json!({ "type": "content_block_stop", "index": status_block_index });
        yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));
        // Mark that status block is now closed - subsequent status messages need a new block
        status_block_open = false;
        block_index += 1;

        // 5. Convert Messages & Config
        let messages = convert_anthropic_messages(&payload);
        let tools = convert_anthropic_tools(&payload);

        let thinking_config = if thinking_enabled && model.supports_thinking() {
             // Extract budget from request if specified
             let budget = payload["thinking"]
                 .get("budget_tokens")
                 .and_then(|v| v.as_u64())
                 .map(|v| v as u32)
                 .or(Some(10000)); // Default budget

             // FIXED: Map budget to level for Gemini fallbacks
             let level = if let Some(b) = budget {
                if b < 5000 { "low" } else if b < 15000 { "medium" } else { "high" }
             } else {
                "low"
             };

             Some(browser_automator::ThinkingConfig {
                 budget: budget,
                 level: Some(level.to_string()),
                 include_thoughts: true,
             })
        } else {
            None
        };

        // 6. Make API Streaming Request
        tracing::info!("Starting streaming request to Antigravity model: {:?}", model);
        let start_time = std::time::Instant::now();
        let result = client.chat_completion_stream(model, messages.clone(), thinking_config.clone(), tools.clone()).await;

        match result {
            Ok(output_stream) => { // Removed mut here, pin! handles it
                 // Only clear rate limit if the PRIMARY request succeeded (not fallback)
                 // This prevents clearing the wrong model's rate limit when spoofing
                 if !used_fallback {
                     account_manager.clear_rate_limit(account.index, ModelFamily::from_model_id(&original_model.api_id().to_string())).await;
                 }

                 use futures_util::StreamExt;
                 // Pin the stream so we can call next()
                 tokio::pin!(output_stream);

                 // We will simply stream everything into a single text block to guarantee visibility.
                 // System logs (index 0) are closed. We start index 1.
                 let mut text_index = block_index;

                 let block_start = serde_json::json!({
                    "type": "content_block_start",
                    "index": text_index,
                    "content_block": { "type": "text", "text": "" }
                 });
                 yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));

                  let mut inside_thought = false;
                  let mut has_tool_use = false; // Track if we encountered tool_use for stop_reason

                  while let Some(chunk_res) = output_stream.next().await {
                     match chunk_res {
                         Ok(chunk) => {
                             if chunk.done { break; }

                              if chunk.is_tool_use {
                                  has_tool_use = true; // Mark that we have tool_use for stop_reason
                                  
                                  // Close current text block if open
                                  let block_stop = serde_json::json!({ "type": "content_block_stop", "index": text_index });
                                  yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

                                  // Increment block index for tool use
                                  text_index += 1; // Actually tool_index, but we reuse the variable for sequential indexing

                                 // Parse tool use JSON
                                 if let Ok(mut tool_json) = serde_json::from_str::<Value>(&chunk.delta) {
                                      // Extract input for delta
                                      let input_obj = tool_json.get("input").cloned().unwrap_or(json!({}));
                                      // Remove input from start block (or set to empty)
                                      if let Some(obj) = tool_json.as_object_mut() {
                                           obj.insert("input".to_string(), json!({}));
                                      }

                                      let block_start = serde_json::json!({
                                          "type": "content_block_start",
                                          "index": text_index,
                                          "content_block": tool_json
                                      });
                                      yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));

                                      // Emit input as delta
                                      let input_str = serde_json::to_string(&input_obj).unwrap_or_default();
                                      let delta = serde_json::json!({
                                          "type": "content_block_delta",
                                          "index": text_index,
                                          "delta": { "type": "input_json_delta", "partial_json": input_str }
                                      });
                                      yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));

                                      // Evaluate block stop immediately as tools are atomic in this stream logic
                                      let block_stop = serde_json::json!({ "type": "content_block_stop", "index": text_index });
                                      yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

                                      // Prepare for next text block
                                      text_index += 1;
                                      let block_start = serde_json::json!({
                                          "type": "content_block_start",
                                          "index": text_index,
                                          "content_block": { "type": "text", "text": "" }
                                      });
                                      yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));
                                 }
                             } else {
                                 // Normal text/thinking processing
                                 let mut text_to_emit = chunk.delta;

                                 // Optional: Visual indication of thinking vs answer
                                 if chunk.is_thinking {
                                     if !inside_thought {
                                         // Start of a thought sequence
                                         text_to_emit = format!("\n> *Thinking: {}*", text_to_emit);
                                         inside_thought = true;
                                     } else {
                                         // Continue thought - maybe italicize?
                                         // Markdown within a stream is tricky, usually we just dump text.
                                         // Let's just dump it. formatting every chunk is risky.
                                     }
                                 } else {
                                     if inside_thought {
                                         // End of thought sequence
                                         text_to_emit = format!("\n\n{}", text_to_emit);
                                         inside_thought = false;
                                     }
                                 }

                                 let delta = serde_json::json!({
                                    "type": "content_block_delta",
                                    "index": text_index,
                                    "delta": { "type": "text_delta", "text": text_to_emit }
                                 });
                                 yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));
                             }
                         },
                         Err(e) => {
                             let err_msg = e.to_string();
                             tracing::error!("Stream chunk error: {}", err_msg);
                             let error_event = serde_json::json!({
                                "type": "error",
                                "error": { "type": "api_error", "message": err_msg }
                            });
                            yield Ok(Event::default().event("error").data(error_event.to_string()));
                            return;
                         }
                     }
                 }

                 let elapsed = start_time.elapsed();
                 tracing::info!("Stream finished in {:.2?}", elapsed);

                 // Close text block
                 let block_stop = serde_json::json!({ "type": "content_block_stop", "index": text_index });
                 yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

                  // Message Delta and Stop
                  // Use correct stop_reason: "tool_use" if tools were called, "end_turn" otherwise
                  let stop_reason = if has_tool_use { "tool_use" } else { "end_turn" };
                  let message_delta = serde_json::json!({
                     "type": "message_delta",
                     "delta": { "stop_reason": stop_reason, "stop_sequence": null },
                     "usage": { "output_tokens": 0 }
                  });
                  yield Ok(Event::default().event("message_delta").data(message_delta.to_string()));

                 let message_stop = serde_json::json!({ "type": "message_stop" });
                 yield Ok(Event::default().event("message_stop").data(message_stop.to_string()));
            }
            Err(e) => {
                let error_str = e.to_string();
                tracing::warn!("Antigravity API Error: '{}'", error_str);

                // Rate Limit & Capacity Error Handling
                if error_str.starts_with("RATE_LIMITED:") || error_str.starts_with("CAPACITY_ERROR:") {
                     let parts: Vec<&str> = error_str.splitn(3, ':').collect();
                     let seconds = parts.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(60);
                     
                     // Use longer backoff for capacity errors
                     let is_capacity = error_str.starts_with("CAPACITY_ERROR:");
                     let effective_seconds = if is_capacity {
                         std::cmp::max(seconds, 45)
                     } else {
                         seconds
                     };
                     
                     let until = chrono::Utc::now() + chrono::Duration::seconds(effective_seconds as i64);
                     account_manager.mark_rate_limited(account.index, ModelFamily::from_model_id(&model.api_id().to_string()), until).await;

                       // Strategy 1: Spoofing Fallback
                       if let Some(spoof_model) = get_spoof_model(model) {
                           // Mark that we used a fallback strategy
                           used_fallback = true;
                           
                           // Determine which block index to use for fallback status messages
                           // If original status block is closed, we need to open a new one
                           let fallback_status_index = if status_block_open {
                               // Use the original status block
                               status_block_index
                           } else {
                               // Open a new status block since the original is closed
                               block_index += 1;
                               let block_start = serde_json::json!({
                                   "type": "content_block_start",
                                   "index": block_index,
                                   "content_block": { "type": "text", "text": "" }
                               });
                               yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));
                               block_index // Use the new block index
                           };
                           
                           let msg = format!("\n> ⚠️  Rate limit hit while using {}.\n> 🔄  Fallback Strategy 1: Switching to {} on same account...\n", model.display_name(), spoof_model.display_name());
                           let delta = serde_json::json!({
                                "type": "content_block_delta",
                                "index": fallback_status_index,
                                "delta": { "type": "text_delta", "text": msg }
                           });
                           yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));

                          // Adapt config and retry
                          let spoof_config = adapt_config_for_spoof(&thinking_config, spoof_model);
                           match client.chat_completion_stream(spoof_model, messages.clone(), spoof_config.clone(), tools.clone()).await {
                               Ok(spoof_stream) => {
                                   // SUCCESS: Reuse the stream handling logic
                                   // We need to duplicate the stream handling loop here or refactor.
                                   // For now, duplication is safer to avoid complex borrow checker issues with recursion/closures in async gen blocks.

                                   // NOTE: Don't clear rate limit - primary model is still rate-limited
                                   // We successfully used a fallback, but the account should stay marked
                                   // so next request knows to use Strategy 0 (pre-emptive spoofing)
                                   use futures_util::StreamExt;
                                  let output_stream = spoof_stream; // Move ownership
                                  tokio::pin!(output_stream);

                                    // Close the status block we used for fallback messages
                                    let block_stop = serde_json::json!({ "type": "content_block_stop", "index": fallback_status_index });
                                    yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

                                 // Start text block
                                 let mut text_index = block_index + 1; // Increment for new block
                                 let block_start = serde_json::json!({
                                    "type": "content_block_start",
                                    "index": text_index,
                                    "content_block": { "type": "text", "text": "" }
                                 });
                                 yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));

                                  let mut inside_thought = false;
                                  let mut has_tool_use = false; // Track if we encountered tool_use for stop_reason
                                  
                                  while let Some(chunk_res) = output_stream.next().await {
                                      match chunk_res {
                                          Ok(chunk) => {
                                              if chunk.done { break; }

                                              if chunk.is_tool_use {
                                                   has_tool_use = true; // Mark that we have tool_use for stop_reason
                                                   
                                                   // Close current text block if open
                                                   let block_stop = serde_json::json!({ "type": "content_block_stop", "index": text_index });
                                                   yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

                                                   // Increment block index for tool use
                                                   text_index += 1;

                                                  // Parse tool use JSON
                                                  if let Ok(mut tool_json) = serde_json::from_str::<Value>(&chunk.delta) {
                                                       // Extract input for delta
                                                       let input_obj = tool_json.get("input").cloned().unwrap_or(json!({}));
                                                       // Remove input from start block (or set to empty)
                                                       if let Some(obj) = tool_json.as_object_mut() {
                                                            obj.insert("input".to_string(), json!({}));
                                                       }

                                                       let block_start = serde_json::json!({
                                                           "type": "content_block_start",
                                                           "index": text_index,
                                                           "content_block": tool_json
                                                       });
                                                       yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));

                                                       // Emit input as delta
                                                       let input_str = serde_json::to_string(&input_obj).unwrap_or_default();
                                                       let delta = serde_json::json!({
                                                           "type": "content_block_delta",
                                                           "index": text_index,
                                                           "delta": { "type": "input_json_delta", "partial_json": input_str }
                                                       });
                                                       yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));

                                                       // Evaluate block stop immediately as tools are atomic in this stream logic
                                                       let block_stop = serde_json::json!({ "type": "content_block_stop", "index": text_index });
                                                       yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));

                                                       // Prepare for next text block
                                                       text_index += 1;
                                                       let block_start = serde_json::json!({
                                                           "type": "content_block_start",
                                                           "index": text_index,
                                                           "content_block": { "type": "text", "text": "" }
                                                       });
                                                       yield Ok(Event::default().event("content_block_start").data(block_start.to_string()));
                                                  }
                                             } else {
                                                 let mut text_to_emit = chunk.delta;
                                                 if chunk.is_thinking {
                                                     if !inside_thought {
                                                         text_to_emit = format!("\n> *Thinking: {}*", text_to_emit);
                                                         inside_thought = true;
                                                     }
                                                 } else {
                                                     if inside_thought {
                                                         text_to_emit = format!("\n\n{}", text_to_emit);
                                                         inside_thought = false;
                                                     }
                                                 }
                                                 let delta = serde_json::json!({
                                                    "type": "content_block_delta",
                                                    "index": text_index,
                                                    "delta": { "type": "text_delta", "text": text_to_emit }
                                                 });
                                                 yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));
                                             }
                                         },
                                         Err(e) => {
                                             let err_msg = e.to_string();
                                             tracing::error!("Spoof Stream chunk error: {}", err_msg);
                                              let error_event = serde_json::json!({
                                                "type": "error",
                                                "error": { "type": "api_error", "message": err_msg }
                                            });
                                            yield Ok(Event::default().event("error").data(error_event.to_string()));
                                            return;
                                         }
                                     }
                                 }
                                  // Stream finished successfully
                                  let block_stop = serde_json::json!({ "type": "content_block_stop", "index": text_index });
                                  yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));
                                  // Use correct stop_reason: "tool_use" if tools were called, "end_turn" otherwise
                                  let stop_reason = if has_tool_use { "tool_use" } else { "end_turn" };
                                  let message_delta = serde_json::json!({
                                     "type": "message_delta",
                                     "delta": { "stop_reason": stop_reason, "stop_sequence": null },
                                     "usage": { "output_tokens": 0 }
                                  });
                                  yield Ok(Event::default().event("message_delta").data(message_delta.to_string()));
                                  let message_stop = serde_json::json!({ "type": "message_stop" });
                                  yield Ok(Event::default().event("message_stop").data(message_stop.to_string()));
                                  return; // Done
                             },
                              Err(e2) => {
                                  tracing::error!("Spoofing attempt failed: {}", e2);
                                  // Check if status block is still open before sending error message
                                  if status_block_open {
                                      let msg = format!("> Spoofing failed: {}\n", e2);
                                      let delta = serde_json::json!({
                                           "type": "content_block_delta",
                                           "index": status_block_index,
                                           "delta": { "type": "text_delta", "text": msg }
                                      });
                                      yield Ok(Event::default().event("content_block_delta").data(delta.to_string()));
                                  }
                                  // Fall through to original error report
                              }
                          }
                      }
                 }

                 // Close status block before error (only if still open)
                 if status_block_open {
                     let block_stop = serde_json::json!({ "type": "content_block_stop", "index": status_block_index });
                     yield Ok(Event::default().event("content_block_stop").data(block_stop.to_string()));
                     // No need to set status_block_open = false here - we're about to return
                 }

                // Emit original error
                 let error_event = serde_json::json!({
                    "type": "error",
                    "error": { "type": "api_error", "message": error_str }
                });
                yield Ok(Event::default().event("error").data(error_event.to_string()));
            }
        };
    };

    Sse::new(stream)
}

/// Token counting endpoint
/// Returns approximated token count (characters / 4)
pub async fn count_tokens(
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let mut total_chars = 0;

    // Count system prompt
    if let Some(system) = payload.get("system") {
        if let Some(s) = system.as_str() {
            total_chars += s.len();
        } else if let Some(arr) = system.as_array() {
            for block in arr {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    total_chars += text.len();
                }
            }
        }
    }

    // Count messages
    if let Some(msgs) = payload.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            if let Some(content) = msg.get("content") {
                if let Some(text) = content.as_str() {
                    total_chars += text.len();
                } else if let Some(blocks) = content.as_array() {
                    for block in blocks {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            total_chars += text.len();
                        }
                    }
                }
            }
        }
    }

    // Rough approximation: 1 token ~= 4 characters
    let token_count = (total_chars as f64 / 4.0).ceil() as u32;

    Json(serde_json::json!({
        "input_tokens": token_count
    }))
}
