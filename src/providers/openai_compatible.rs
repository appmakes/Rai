use super::{record_api_call, record_usage_from_response, Message, Provider, ProviderResponse};
use crate::provider_catalog;
use crate::tools::{tools_to_api_json, ToolCall, ToolDefinition};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::io::Write as _;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct OpenAiCompatibleProvider {
    provider: String,
    api_key: Option<String>,
    endpoint: String,
    client: Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(provider: &str, api_key: &str, base_url_override: Option<&str>) -> Result<Self> {
        let provider = provider_catalog::normalize_provider_name(provider)
            .unwrap_or_else(|| provider.trim().to_ascii_lowercase());
        let base_url = base_url_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| provider_catalog::provider_default_base_url(&provider).map(str::to_string))
            .with_context(|| {
                format!(
                    "Provider '{}' requires a base URL. Configure `provider_base_url`.",
                    provider
                )
            })?;

        let endpoint = build_chat_completions_endpoint(&base_url);
        let normalized_api_key = if api_key.trim().is_empty() {
            None
        } else {
            Some(api_key.trim().to_string())
        };
        if provider_catalog::provider_requires_api_key(&provider) && normalized_api_key.is_none() {
            anyhow::bail!(
                "Provider '{}' requires an API key. Set it via keyring or env var.",
                provider
            );
        }

        Ok(Self {
            provider,
            api_key: normalized_api_key,
            endpoint,
            client: Client::new(),
        })
    }

    async fn send_request(&self, body: Value) -> Result<Value> {
        let _spinner = Spinner::start("[rai] processing");
        let mut request = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json");
        if let Some(api_key) = self.api_key.as_ref() {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }

        let response =
            request.json(&body).send().await.with_context(|| {
                format!("Failed to send request to '{}' provider", self.provider)
            })?;
        record_api_call();

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("{} API error: {}", self.provider, error_text);
        }

        let response_json: Value = response
            .json()
            .await
            .with_context(|| format!("Failed to parse {} API response", self.provider))?;
        record_usage_from_response(&response_json);
        Ok(response_json)
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn chat(&self, model: &str, message: &str) -> Result<String> {
        let body = json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": message
                }
            ]
        });

        let response_json = self.send_request(body).await?;
        let message = &response_json["choices"][0]["message"];
        Ok(extract_message_text(message))
    }

    async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<ProviderResponse> {
        let messages_json: Vec<Value> = messages
            .iter()
            .map(|message| message.to_api_json())
            .collect();
        let mut body = json!({
            "model": model,
            "messages": messages_json
        });

        if !tools.is_empty() {
            body["tools"] = tools_to_api_json(tools);
        }

        let response_json = self.send_request(body).await?;
        let choice = &response_json["choices"][0];
        let message = &choice["message"];
        let finish_reason = choice["finish_reason"].as_str().unwrap_or("stop");
        let has_tool_calls = message
            .get("tool_calls")
            .and_then(|value| value.as_array())
            .map(|calls| !calls.is_empty())
            .unwrap_or(false);

        if finish_reason == "tool_calls" || has_tool_calls {
            let tool_calls = parse_tool_calls(message)?;
            Ok(ProviderResponse::ToolCalls(tool_calls))
        } else {
            Ok(ProviderResponse::Text(extract_message_text(message)))
        }
    }
}

fn parse_tool_calls(message: &Value) -> Result<Vec<ToolCall>> {
    let tool_calls_json = message["tool_calls"]
        .as_array()
        .context("Expected tool_calls array in provider response")?;
    let mut tool_calls = Vec::new();
    for (index, tool_call) in tool_calls_json.iter().enumerate() {
        let id = tool_call["id"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("tool_call_{}", index + 1));
        let name = tool_call["function"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let arguments = match &tool_call["function"]["arguments"] {
            Value::String(raw) => serde_json::from_str(raw).unwrap_or_else(|_| json!({})),
            Value::Object(_) => tool_call["function"]["arguments"].clone(),
            _ => json!({}),
        };

        tool_calls.push(ToolCall {
            id,
            name,
            arguments,
        });
    }
    Ok(tool_calls)
}

fn extract_message_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn build_chat_completions_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{}/chat/completions", trimmed)
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

#[cfg(test)]
mod tests {
    use super::build_chat_completions_endpoint;

    #[test]
    fn appends_chat_completions_path_when_missing() {
        assert_eq!(
            build_chat_completions_endpoint("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn keeps_endpoint_when_chat_completions_is_present() {
        assert_eq!(
            build_chat_completions_endpoint("https://example.com/v1/chat/completions"),
            "https://example.com/v1/chat/completions"
        );
    }
}
