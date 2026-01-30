//! Session Recovery Module
//!
//! This module provides automatic recovery from corrupted conversation states
//! that can occur when using Claude models through the Antigravity API.
//!
//! Common error patterns handled:
//! 1. "tool_use without tool_result" - Missing tool results after tool_use blocks
//! 2. "Expected thinking but found text" - Thinking blocks out of order
//!
//! Based on: https://github.com/NoeFabris/opencode-antigravity-auth/blob/main/docs/ARCHITECTURE.md

use serde_json::{json, Value};
use tracing::{info, warn};

/// Result of session recovery analysis
#[derive(Debug)]
pub struct RecoveryResult {
    /// Whether recovery was performed
    pub was_recovered: bool,
    /// The recovered messages (or original if no recovery needed)
    pub messages: Vec<Value>,
    /// Description of what was fixed
    pub recovery_notes: Vec<String>,
}

/// Analyzes and recovers a corrupted conversation session
///
/// This function detects and fixes common conversation corruption patterns:
/// - tool_use blocks without corresponding tool_result blocks
/// - thinking blocks in incorrect order
pub fn recover_session(messages: &[Value]) -> RecoveryResult {
    if messages.is_empty() {
        return RecoveryResult {
            was_recovered: false,
            messages: messages.to_vec(),
            recovery_notes: vec![],
        };
    }

    let mut recovered_messages = messages.to_vec();
    let mut recovery_notes = Vec::new();
    let mut was_recovered = false;

    // Check 1: Fix tool_use without tool_result
    let tool_fix_result = fix_missing_tool_results(&recovered_messages);
    if tool_fix_result.was_fixed {
        let fix_count = tool_fix_result.fix_notes.len();
        recovered_messages = tool_fix_result.messages;
        recovery_notes.extend(tool_fix_result.fix_notes);
        was_recovered = true;
        info!("Session recovery: Fixed {} tool_use issues", fix_count);
    }

    // Check 2: Fix thinking block order issues
    let thinking_fix_result = fix_thinking_order(&recovered_messages);
    if thinking_fix_result.was_fixed {
        let fix_count = thinking_fix_result.fix_notes.len();
        recovered_messages = thinking_fix_result.messages;
        recovery_notes.extend(thinking_fix_result.fix_notes);
        was_recovered = true;
        info!(
            "Session recovery: Fixed {} thinking order issues",
            fix_count
        );
    }

    RecoveryResult {
        was_recovered,
        messages: recovered_messages,
        recovery_notes,
    }
}

/// Result of a specific fix operation
#[derive(Debug)]
struct FixResult {
    was_fixed: bool,
    messages: Vec<Value>,
    fix_notes: Vec<String>,
}

/// Fixes missing tool_result blocks after tool_use blocks
///
/// When a conversation has a tool_use block but the client never sent the tool_result,
/// the API will error with "tool_use without tool_result". This function detects
/// such patterns and injects synthetic tool results.
fn fix_missing_tool_results(messages: &[Value]) -> FixResult {
    let mut fixed_messages = Vec::new();
    let mut fix_notes = Vec::new();
    let mut was_fixed = false;

    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];
        fixed_messages.push(msg.clone());

        // Check if this is an assistant message with tool_use
        if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
            if role == "assistant" {
                if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                    let has_tool_use = content.iter().any(|block| {
                        block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                    });

                    if has_tool_use {
                        // Check if next message is a tool_result
                        let next_is_tool_result = if i + 1 < messages.len() {
                            let next_msg = &messages[i + 1];
                            if let Some(next_role) = next_msg.get("role").and_then(|r| r.as_str()) {
                                if next_role == "user" {
                                    if let Some(next_content) =
                                        next_msg.get("content").and_then(|c| c.as_array())
                                    {
                                        next_content.iter().any(|block| {
                                            block.get("type").and_then(|t| t.as_str())
                                                == Some("tool_result")
                                        })
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if !next_is_tool_result {
                            // Inject synthetic tool result
                            let tool_use_blocks: Vec<&Value> = content
                                .iter()
                                .filter(|block| {
                                    block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                                })
                                .collect();

                            for tool_use in tool_use_blocks {
                                if let Some(tool_id) = tool_use.get("id").and_then(|id| id.as_str())
                                {
                                    if let Some(tool_name) =
                                        tool_use.get("name").and_then(|n| n.as_str())
                                    {
                                        let synthetic_result = json!({
                                            "role": "user",
                                            "content": [{
                                                "type": "tool_result",
                                                "tool_use_id": tool_id,
                                                "content": format!(
                                                    "Tool '{}' was not executed. The previous operation was interrupted. \
                                                     Please continue with the available information or ask the user to retry.",
                                                    tool_name
                                                )
                                            }]
                                        });

                                        fixed_messages.push(synthetic_result);
                                        fix_notes.push(format!(
                                            "Injected synthetic tool_result for tool '{}' (id: {})",
                                            tool_name, tool_id
                                        ));
                                        was_fixed = true;
                                        warn!(
                                            "Missing tool_result detected for tool '{}' (id: {}). Injected synthetic result.",
                                            tool_name, tool_id
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        i += 1;
    }

    FixResult {
        was_fixed,
        messages: fixed_messages,
        fix_notes,
    }
}

/// Fixes thinking blocks that are out of order
///
/// Claude expects thinking blocks to appear in a specific order. When thinking
/// blocks are corrupted or out of order, the API returns "Expected thinking but found text".
/// This function detects and removes corrupted thinking blocks.
fn fix_thinking_order(messages: &[Value]) -> FixResult {
    let mut fixed_messages = Vec::new();
    let mut fix_notes = Vec::new();
    let mut was_fixed = false;

    for (idx, msg) in messages.iter().enumerate() {
        let mut fixed_msg = msg.clone();

        // Check if this is an assistant message with content blocks
        if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
            if role == "assistant" {
                if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                    let mut fixed_content = Vec::new();
                    let mut removed_thinking = false;

                    for (block_idx, block) in content.iter().enumerate() {
                        let block_type = block.get("type").and_then(|t| t.as_str());

                        // Check for thinking blocks that might be corrupted
                        if block_type == Some("thinking") {
                            // Validate thinking block structure
                            let has_signature = block.get("signature").is_some();
                            let has_thinking =
                                block.get("thinking").is_some() || block.get("text").is_some();

                            if !has_signature || !has_thinking {
                                // Corrupted thinking block - remove it
                                removed_thinking = true;
                                fix_notes.push(format!(
                                    "Removed corrupted thinking block at message {}, block {}: missing {}",
                                    idx, block_idx,
                                    if !has_signature && !has_thinking {
                                        "signature and content"
                                    } else if !has_signature {
                                        "signature"
                                    } else {
                                        "content"
                                    }
                                ));
                                warn!(
                                    "Removed corrupted thinking block at message {}, block {}",
                                    idx, block_idx
                                );
                                continue;
                            }
                        }

                        fixed_content.push(block.clone());
                    }

                    if removed_thinking {
                        was_fixed = true;
                        if let Some(obj) = fixed_msg.as_object_mut() {
                            obj.insert("content".to_string(), json!(fixed_content));
                        }
                    }
                }
            }
        }

        fixed_messages.push(fixed_msg);
    }

    FixResult {
        was_fixed,
        messages: fixed_messages,
        fix_notes,
    }
}

/// Checks if an error message indicates a recoverable session error
pub fn is_recoverable_error(error_text: &str) -> bool {
    let recoverable_patterns = [
        "tool_use without tool_result",
        "tool result missing",
        "expected thinking but found text",
        "thinking block out of order",
        "invalid thinking signature",
    ];

    let lower_error = error_text.to_lowercase();
    recoverable_patterns
        .iter()
        .any(|pattern| lower_error.contains(pattern))
}

/// Generates a recovery summary message for logging
pub fn format_recovery_summary(result: &RecoveryResult) -> String {
    if result.was_recovered {
        format!(
            "Session recovery performed: {} fixes applied. Notes: {}",
            result.recovery_notes.len(),
            result.recovery_notes.join("; ")
        )
    } else {
        "No session recovery needed".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fix_missing_tool_results() {
        let messages = vec![
            json!({
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "tool_123",
                    "name": "read_file",
                    "input": {"path": "/test.txt"}
                }]
            }),
            // Missing tool_result here!
            json!({
                "role": "user",
                "content": "Continue"
            }),
        ];

        let result = fix_missing_tool_results(&messages);
        assert!(result.was_fixed);
        assert_eq!(result.messages.len(), 3); // Original 2 + 1 injected

        // Check that synthetic tool_result was injected
        let injected = &result.messages[1];
        assert_eq!(injected["role"], "user");
        assert!(injected["content"][0]["type"] == "tool_result");
    }

    #[test]
    fn test_no_fix_when_tool_result_present() {
        let messages = vec![
            json!({
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "tool_123",
                    "name": "read_file",
                    "input": {}
                }]
            }),
            json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "tool_123",
                    "content": "File contents"
                }]
            }),
        ];

        let result = fix_missing_tool_results(&messages);
        assert!(!result.was_fixed);
        assert_eq!(result.messages.len(), 2);
    }

    #[test]
    fn test_is_recoverable_error() {
        assert!(is_recoverable_error("tool_use without tool_result"));
        assert!(is_recoverable_error("Expected thinking but found text"));
        assert!(is_recoverable_error("Invalid thinking signature"));
        assert!(!is_recoverable_error("Rate limit exceeded"));
        assert!(!is_recoverable_error("Invalid API key"));
    }
}
