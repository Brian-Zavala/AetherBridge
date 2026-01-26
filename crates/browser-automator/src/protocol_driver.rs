use anyhow::{Result, Ok};
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use reqwest::ClientBuilder;
use std::sync::Arc;
use common::config::Account;
use crate::google_driver::GoogleClient;
use crate::auth::CookieExtractor;

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
        // Attempt to extract Google cookies
        // In a real app, we'd check account type. defaulting to Google.
        let cookies = CookieExtractor::extract_cookies("ide.google.com", &["__Secure-3PSID"])
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to extract cookies: {}. Proceeding without auth (request will likely fail).", e);
                String::new()
            });

        // Initialize header map with cookies if found
        let mut headers = HeaderMap::new();
        if !cookies.is_empty() {
             let mut cookie_val = HeaderValue::from_str(&cookies)?;
             cookie_val.set_sensitive(true);
             headers.insert(COOKIE, cookie_val);
        }

        let client = ClientBuilder::new()
            .default_headers(headers)
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
