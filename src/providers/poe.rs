use super::Provider;
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

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
}

#[async_trait]
impl Provider for PoeProvider {
    async fn chat(&self, model: &str, message: &str) -> Result<String> {
        let url = "https://api.poe.com/v1/chat/completions";
        
        let body = json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": message
                }
            ]
        });

        let response = self.client.post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Poe API")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Poe API error: {}", error_text);
        }

        let response_json: serde_json::Value = response.json()
            .await
            .context("Failed to parse Poe API response")?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .context("Failed to get content from response")?;

        Ok(content.to_string())
    }
}
