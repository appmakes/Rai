use super::{record_api_call, record_token_usage, Message, Provider, ProviderResponse};
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

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

pub struct GoogleProvider {
    api_key: String,
    base_url: String,
    client: Client,
}

impl GoogleProvider {
    pub fn new(api_key: &str, base_url_override: Option<&str>) -> Result<Self> {
        if api_key.trim().is_empty() {
            anyhow::bail!("Google provider requires an API key");
        }
        let base_url = base_url_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Ok(Self {
            api_key: api_key.trim().to_string(),
            base_url,
            client: Client::new(),
        })
    }

    fn endpoint_for_model(&self, model: &str) -> String {
        if self.base_url.contains("{model}") {
            return self.base_url.replace("{model}", model);
        }
        let trimmed = self.base_url.trim_end_matches('/');
        if trimmed.contains(":generateContent") {
            return trimmed.to_string();
        }
        format!("{}/v1beta/models/{}:generateContent", trimmed, model)
    }

    async fn send_request(&self, model: &str, body: Value) -> Result<Value> {
        let endpoint = self.endpoint_for_model(model);
        let endpoint_with_key = if endpoint.contains('?') {
            format!("{}&key={}", endpoint, self.api_key)
        } else {
            format!("{}?key={}", endpoint, self.api_key)
        };
        let _spinner = Spinner::start("[rai] processing");
        let response = self
            .client
            .post(endpoint_with_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Google API")?;
        record_api_call();

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Google API error: {}", error_text);
        }

        let response_json: Value = response
            .json()
            .await
            .context("Failed to parse Google API response")?;
        let usage = &response_json["usageMetadata"];
        let input_tokens = usage["promptTokenCount"]
            .as_u64()
            .or_else(|| usage["promptTokensDetails"][0]["tokenCount"].as_u64())
            .unwrap_or(0);
        let output_tokens = usage["candidatesTokenCount"]
            .as_u64()
            .or_else(|| usage["responseTokenCount"].as_u64())
            .unwrap_or(0);
        record_token_usage(input_tokens, output_tokens);
        Ok(response_json)
    }

    fn build_body(&self, messages: &[Message], tools: &[ToolDefinition]) -> Value {
        let mut system_chunks = Vec::new();
        let mut contents = Vec::new();
        let mut tool_name_by_id: HashMap<String, String> = HashMap::new();

        for message in messages {
            match message {
                Message::System { content } => {
                    if !content.trim().is_empty() {
                        system_chunks.push(content.clone());
                    }
                }
                Message::User { content } => {
                    contents.push(json!({
                        "role": "user",
                        "parts": [{ "text": content }]
                    }));
                }
                Message::AssistantToolCalls {
                    content,
                    tool_calls,
                } => {
                    let mut parts = Vec::new();
                    if let Some(text) = content.as_ref().filter(|text| !text.trim().is_empty()) {
                        parts.push(json!({ "text": text }));
                    }
                    for tool_call in tool_calls {
                        let arguments =
                            serde_json::from_str::<Value>(&tool_call.function.arguments)
                                .unwrap_or_else(|_| json!({}));
                        tool_name_by_id
                            .insert(tool_call.id.clone(), tool_call.function.name.clone());
                        parts.push(json!({
                            "functionCall": {
                                "name": tool_call.function.name,
                                "args": arguments
                            }
                        }));
                    }
                    if !parts.is_empty() {
                        contents.push(json!({
                            "role": "model",
                            "parts": parts
                        }));
                    }
                }
                Message::ToolResult {
                    tool_call_id,
                    content,
                } => {
                    let name = tool_name_by_id
                        .get(tool_call_id)
                        .cloned()
                        .unwrap_or_else(|| "tool".to_string());
                    contents.push(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": name,
                                "response": {
                                    "content": content
                                }
                            }
                        }]
                    }));
                }
            }
        }

        let mut body = json!({
            "contents": contents
        });
        if !system_chunks.is_empty() {
            body["systemInstruction"] = json!({
                "parts": [{
                    "text": system_chunks.join("\n\n")
                }]
            });
        }
        if !tools.is_empty() {
            let declarations = tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters
                    })
                })
                .collect::<Vec<_>>();
            body["tools"] = json!([{
                "functionDeclarations": declarations
            }]);
            body["toolConfig"] = json!({
                "functionCallingConfig": {
                    "mode": "AUTO"
                }
            });
        }
        body
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    async fn chat(&self, model: &str, message: &str) -> Result<String> {
        let messages = vec![Message::User {
            content: message.to_string(),
        }];
        let body = self.build_body(&messages, &[]);
        let response_json = self.send_request(model, body).await?;
        let parts = response_json["candidates"][0]["content"]["parts"]
            .as_array()
            .context("Google response missing candidates[0].content.parts")?;
        let text = parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }

    async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<ProviderResponse> {
        let body = self.build_body(messages, tools);
        let response_json = self.send_request(model, body).await?;
        let parts = response_json["candidates"][0]["content"]["parts"]
            .as_array()
            .context("Google response missing candidates[0].content.parts")?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if let Some(text) = part["text"].as_str() {
                text_parts.push(text.to_string());
            }
            if let Some(function_call) = part.get("functionCall") {
                let name = function_call["name"].as_str().unwrap_or("").to_string();
                let arguments = function_call["args"].clone();
                let id = function_call["id"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("function_call_{}", index + 1));
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
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
