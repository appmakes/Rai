use super::{
    record_api_call, record_usage_from_response, ApiToolCall, Message, Provider, ProviderResponse,
};
use crate::tools::{ToolCall, ToolDefinition};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write as _;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;

const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_MAX_TOKENS: u64 = 4096;

pub struct AnthropicProvider {
    api_key: String,
    endpoint: String,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(api_key: &str, endpoint_override: Option<&str>) -> Result<Self> {
        if api_key.trim().is_empty() {
            anyhow::bail!("Anthropic provider requires an API key");
        }
        let endpoint = endpoint_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
        Ok(Self {
            api_key: api_key.trim().to_string(),
            endpoint,
            client: Client::new(),
        })
    }

    async fn send_request(&self, body: Value) -> Result<Value> {
        let _spinner = Spinner::start("[rai] processing");
        let response = self
            .client
            .post(&self.endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;
        record_api_call();

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error: {}", error_text);
        }

        let response_json: Value = response
            .json()
            .await
            .context("Failed to parse Anthropic API response")?;
        record_usage_from_response(&response_json);
        Ok(response_json)
    }

    fn build_body(&self, model: &str, messages: &[Message], tools: &[ToolDefinition]) -> Value {
        let mut system_chunks = Vec::new();
        let mut anthropic_messages: Vec<Value> = Vec::new();
        let mut tool_name_by_id: HashMap<String, String> = HashMap::new();

        for message in messages {
            match message {
                Message::System { content } => {
                    if !content.trim().is_empty() {
                        system_chunks.push(content.clone());
                    }
                }
                Message::User { content } => {
                    anthropic_messages.push(json!({
                        "role": "user",
                        "content": [{ "type": "text", "text": content }]
                    }));
                }
                Message::AssistantToolCalls {
                    content,
                    tool_calls,
                } => {
                    let mut content_blocks: Vec<Value> = Vec::new();
                    if let Some(text) = content.as_ref().filter(|text| !text.trim().is_empty()) {
                        content_blocks.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }

                    for tool_call in tool_calls {
                        tool_name_by_id
                            .insert(tool_call.id.clone(), tool_call.function.name.clone());
                        content_blocks.push(convert_tool_call(tool_call));
                    }

                    if !content_blocks.is_empty() {
                        anthropic_messages.push(json!({
                            "role": "assistant",
                            "content": content_blocks
                        }));
                    }
                }
                Message::ToolResult {
                    tool_call_id,
                    content,
                } => {
                    let _tool_name = tool_name_by_id.get(tool_call_id).cloned();
                    anthropic_messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content
                        }]
                    }));
                }
            }
        }

        let mut body = json!({
            "model": model,
            "max_tokens": DEFAULT_MAX_TOKENS,
            "messages": anthropic_messages
        });

        if !system_chunks.is_empty() {
            body["system"] = Value::String(system_chunks.join("\n\n"));
        }

        if !tools.is_empty() {
            let tools_json = tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.parameters
                    })
                })
                .collect::<Vec<_>>();
            body["tools"] = Value::Array(tools_json);
        }

        body
    }
}

fn convert_tool_call(tool_call: &ApiToolCall) -> Value {
    let arguments =
        serde_json::from_str::<Value>(&tool_call.function.arguments).unwrap_or_else(|_| json!({}));
    json!({
        "type": "tool_use",
        "id": tool_call.id,
        "name": tool_call.function.name,
        "input": arguments
    })
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat(&self, model: &str, message: &str) -> Result<String> {
        let messages = vec![Message::User {
            content: message.to_string(),
        }];
        let body = self.build_body(model, &messages, &[]);
        let response_json = self.send_request(body).await?;
        let content = response_json["content"]
            .as_array()
            .context("Anthropic response missing content array")?
            .iter()
            .filter_map(|block| {
                if block["type"].as_str() == Some("text") {
                    block["text"].as_str().map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(content)
    }

    async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<ProviderResponse> {
        let body = self.build_body(model, messages, tools);
        let response_json = self.send_request(body).await?;
        let content_blocks = response_json["content"]
            .as_array()
            .context("Anthropic response missing content array")?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        for (index, block) in content_blocks.iter().enumerate() {
            match block["type"].as_str().unwrap_or("") {
                "text" => {
                    if let Some(text) = block["text"].as_str() {
                        text_parts.push(text.to_string());
                    }
                }
                "tool_use" => {
                    let id = block["id"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("tool_use_{}", index + 1));
                    let name = block["name"].as_str().unwrap_or("").to_string();
                    let arguments = block["input"].clone();
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
                _ => {}
            }
        }

        if !tool_calls.is_empty() {
            Ok(ProviderResponse::ToolCalls(tool_calls))
        } else {
            Ok(ProviderResponse::Text(text_parts.join("\n")))
        }
    }
}

struct Spinner {
    active: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    width: usize,
}

impl Spinner {
    fn start(prefix: &str) -> Self {
        if !spinner_enabled() {
            return Self {
                active: Arc::new(AtomicBool::new(false)),
                handle: None,
                width: 0,
            };
        }

        let frames = ['|', '/', '-', '\\'];
        let active = Arc::new(AtomicBool::new(true));
        let active_thread = Arc::clone(&active);
        let prefix_text = prefix.to_string();
        let width = prefix_text.len() + 2;

        let handle = std::thread::spawn(move || {
            let mut index = 0usize;
            while active_thread.load(Ordering::Relaxed) {
                let frame = frames[index % frames.len()];
                eprint!("\r{} {}", prefix_text, frame);
                let _ = std::io::stderr().flush();
                index += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
        });

        Self {
            active,
            handle: Some(handle),
            width,
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.active.store(false, Ordering::Relaxed);
            let _ = handle.join();
            if self.width > 0 {
                eprint!("\r{:width$}\r", "", width = self.width);
                let _ = std::io::stderr().flush();
            }
        }
    }
}

fn spinner_enabled() -> bool {
    atty::is(atty::Stream::Stderr) && std::env::var_os("CI").is_none()
}
