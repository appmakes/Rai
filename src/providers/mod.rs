pub mod poe;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::tools::{ToolCall, ToolDefinition};

#[derive(Debug, Clone)]
pub enum Message {
    System { content: String },
    User { content: String },
    Assistant { content: String },
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

    pub fn assistant(content: &str) -> Self {
        Message::Assistant {
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
            Message::Assistant { content } => serde_json::json!({
                "role": "assistant",
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
