use anyhow::Result;
use reqwest::ClientBuilder;
use std::sync::Arc;
use common::config::{Account, ProviderType};
use crate::google_driver::GoogleClient;

#[derive(Clone)]
pub enum DriverImpl {
    Google(GoogleClient),
    // Anthropic(AnthropicClient),
}

#[derive(Clone)]
pub struct ProtocolDriver {
    driver:  Arc<DriverImpl>,
}

impl ProtocolDriver {
    pub fn new(_account: &Account) -> Result<Self> {
        let client = ClientBuilder::new()
            .cookie_store(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()?;

        // Detect provider type and initialize appropriate driver.
        // For now, we default to Google.
        let driver_impl = DriverImpl::Google(GoogleClient::new(client));

        Ok(Self { driver: Arc::new(driver_impl) })
    }

    pub async fn chat_completion(&self, prompt: &str) -> Result<String> {
        match self.driver.as_ref() {
            DriverImpl::Google(d) => d.generate(prompt).await,
        }
    }
}
