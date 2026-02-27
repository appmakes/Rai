pub mod poe;

use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Provider {
    async fn chat(&self, model: &str, message: &str) -> Result<String>;
}
