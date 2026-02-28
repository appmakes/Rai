use super::{record_api_call, record_usage_from_response, Message, Provider, ProviderResponse};
use crate::tools::{tools_to_api_json, ToolCall, ToolDefinition};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

pub struct PoeProvider {
    api_key: String,
    client: Client,
}

impl PoeProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: Client::new(),
        }
    }

    async fn send_request(&self, body: Value) -> Result<Value> {
        let url = "https://api.poe.com/v1/chat/completions";

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Poe API")?;
        record_api_call();

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Poe API error: {}", error_text);
        }

        let response_json: Value = response
            .json()
            .await
            .context("Failed to parse Poe API response")?;
        record_usage_from_response(&response_json);
        Ok(response_json)
    }
}

#[async_trait]
impl Provider for PoeProvider {
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

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .context("Failed to get content from response")?;

        Ok(content.to_string())
    }

    async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<ProviderResponse> {
        let messages_json: Vec<Value> = messages.iter().map(|m| m.to_api_json()).collect();

        let mut body = json!({
            "model": model,
            "messages": messages_json,
        });

        if !tools.is_empty() {
            let tools_json = tools_to_api_json(tools);
            body["tools"] = tools_json;
        }

        let response_json = self.send_request(body).await?;

        let choice = &response_json["choices"][0];
        let message = &choice["message"];
        let finish_reason = choice["finish_reason"].as_str().unwrap_or("stop");

        if finish_reason == "tool_calls"
            || (message.get("tool_calls").is_some()
                && message["tool_calls"].is_array()
                && !message["tool_calls"].as_array().unwrap().is_empty())
        {
            let tool_calls_json = message["tool_calls"]
                .as_array()
                .context("Expected tool_calls array")?;

            let mut tool_calls = Vec::new();
            for tc in tool_calls_json {
                let id = tc["id"].as_str().unwrap_or("").to_string();
                let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let arguments: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }

            Ok(ProviderResponse::ToolCalls(tool_calls))
        } else {
            let content = message["content"].as_str().unwrap_or("").to_string();
            Ok(ProviderResponse::Text(content))
        }
    }
}
