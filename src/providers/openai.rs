use super::{Message, Provider, ProviderResponse};
use crate::providers::openai_compatible::OpenAiCompatibleProvider;
use crate::tools::ToolDefinition;
use anyhow::Result;
use async_trait::async_trait;

pub struct OpenAiProvider {
    inner: OpenAiCompatibleProvider,
}

impl OpenAiProvider {
    pub fn new(api_key: &str, base_url_override: Option<&str>) -> Result<Self> {
        Ok(Self {
            inner: OpenAiCompatibleProvider::new("openai", api_key, base_url_override)?,
        })
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn chat(&self, model: &str, message: &str) -> Result<String> {
        self.inner.chat(model, message).await
    }

    async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> Result<ProviderResponse> {
        self.inner.chat_with_tools(model, messages, tools).await
    }
}
