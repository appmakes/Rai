pub mod poe;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

use crate::tools::{ToolCall, ToolDefinition};

#[derive(Debug, Clone)]
pub enum Message {
    System {
        content: String,
    },
    User {
        content: String,
    },
    AssistantToolCalls {
        content: Option<String>,
        tool_calls: Vec<ApiToolCall>,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ApiToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug)]
pub enum ProviderResponse {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BillingStats {
    pub api_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

fn billing_store() -> &'static Mutex<BillingStats> {
    static BILLING: OnceLock<Mutex<BillingStats>> = OnceLock::new();
    BILLING.get_or_init(|| Mutex::new(BillingStats::default()))
}

fn with_billing_mut<R>(f: impl FnOnce(&mut BillingStats) -> R) -> R {
    let mut guard = billing_store()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    f(&mut guard)
}

pub fn reset_billing_stats() {
    with_billing_mut(|stats| *stats = BillingStats::default());
}

pub fn record_api_call() {
    with_billing_mut(|stats| stats.api_calls += 1);
}

pub fn record_token_usage(input_tokens: u64, output_tokens: u64) {
    with_billing_mut(|stats| {
        stats.input_tokens += input_tokens;
        stats.output_tokens += output_tokens;
    });
}

pub fn get_billing_stats() -> BillingStats {
    let guard = billing_store()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *guard
}

pub fn extract_usage_tokens(response_json: &serde_json::Value) -> (u64, u64) {
    let usage = &response_json["usage"];
    let input_tokens = usage["prompt_tokens"]
        .as_u64()
        .or_else(|| usage["input_tokens"].as_u64())
        .or_else(|| usage["promptTokens"].as_u64())
        .unwrap_or(0);
    let output_tokens = usage["completion_tokens"]
        .as_u64()
        .or_else(|| usage["output_tokens"].as_u64())
        .or_else(|| usage["completionTokens"].as_u64())
        .unwrap_or(0);
    (input_tokens, output_tokens)
}

pub fn record_usage_from_response(response_json: &serde_json::Value) {
    let (input_tokens, output_tokens) = extract_usage_tokens(response_json);
    record_token_usage(input_tokens, output_tokens);
}

impl Message {
    pub fn system(content: &str) -> Self {
        Message::System {
            content: content.to_string(),
        }
    }

    pub fn user(content: &str) -> Self {
        Message::User {
            content: content.to_string(),
        }
    }

    pub fn tool_result(tool_call_id: &str, content: &str) -> Self {
        Message::ToolResult {
            tool_call_id: tool_call_id.to_string(),
            content: content.to_string(),
        }
    }

    pub fn assistant_tool_calls(tool_calls: &[ToolCall]) -> Self {
        let api_calls: Vec<ApiToolCall> = tool_calls
            .iter()
            .map(|tc| ApiToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: ApiToolCallFunction {
                    name: tc.name.clone(),
                    arguments: tc.arguments.to_string(),
                },
            })
            .collect();
        Message::AssistantToolCalls {
            content: None,
            tool_calls: api_calls,
        }
    }

    pub fn to_api_json(&self) -> serde_json::Value {
        match self {
            Message::System { content } => serde_json::json!({
                "role": "system",
                "content": content,
            }),
            Message::User { content } => serde_json::json!({
                "role": "user",
                "content": content,
            }),
            Message::AssistantToolCalls {
                content,
                tool_calls,
            } => {
                let tc_json: Vec<serde_json::Value> = tool_calls
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": tc.call_type,
                            "function": {
                                "name": tc.function.name,
                                "arguments": tc.function.arguments,
                            }
                        })
                    })
                    .collect();
                serde_json::json!({
                    "role": "assistant",
                    "content": content,
                    "tool_calls": tc_json,
                })
            }
            Message::ToolResult {
                tool_call_id,
                content,
            } => serde_json::json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": content,
            }),
        }
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, model: &str, message: &str) -> Result<String>;

    async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<ProviderResponse>;
}

#[cfg(test)]
mod tests {
    use super::{
        extract_usage_tokens, get_billing_stats, record_api_call, record_token_usage,
        reset_billing_stats,
    };
    use serde_json::json;

    #[test]
    fn extract_usage_tokens_supports_openai_style_fields() {
        let response = json!({
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 7
            }
        });
        let (input, output) = extract_usage_tokens(&response);
        assert_eq!(input, 12);
        assert_eq!(output, 7);
    }

    #[test]
    fn extract_usage_tokens_supports_alt_fields() {
        let response = json!({
            "usage": {
                "input_tokens": 5,
                "output_tokens": 9
            }
        });
        let (input, output) = extract_usage_tokens(&response);
        assert_eq!(input, 5);
        assert_eq!(output, 9);
    }

    #[test]
    fn extract_usage_tokens_defaults_to_zero_when_usage_missing() {
        let response = json!({});
        let (input, output) = extract_usage_tokens(&response);
        assert_eq!(input, 0);
        assert_eq!(output, 0);
    }

    #[test]
    fn billing_stats_accumulate_calls_and_tokens() {
        reset_billing_stats();
        record_api_call();
        record_api_call();
        record_token_usage(10, 4);
        record_token_usage(1, 2);

        let stats = get_billing_stats();
        assert_eq!(stats.api_calls, 2);
        assert_eq!(stats.input_tokens, 11);
        assert_eq!(stats.output_tokens, 6);
    }
}
