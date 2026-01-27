use anyhow::{Result, Ok};
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use reqwest::ClientBuilder;
use std::sync::Arc;
use common::config::Account;
use crate::google_driver::GoogleClient;
use crate::auth::CookieExtractor;
use crate::Provider;

#[derive(Clone)]
pub struct ProtocolDriver {
    driver:  Arc<Box<dyn Provider>>,
}

impl ProtocolDriver {
    pub fn new(account: &Account, browser_profile_path: Option<&str>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let mut using_oauth = false;

        // 1. Try OAuth token from account config
        if let Some(token) = account.credentials.get("access_token") {
            tracing::info!("Using OAuth token for authentication");
            let mut auth_val = HeaderValue::from_str(&format!("Bearer {}", token))?;
            auth_val.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, auth_val);
            using_oauth = true;
        }

        // 2. Fallback to cookies if no OAuth
        if !using_oauth {
            // Attempt to extract Google cookies
            // We use "google.com" to catch cookies set on .google.com (like __Secure-3PSID)
            let cookies = CookieExtractor::extract_cookies("google.com", &["__Secure-3PSID"], browser_profile_path)
                .unwrap_or_else(|e| {
                    tracing::warn!("Failed to extract cookies: {}. Proceeding without auth (request will likely fail).", e);
                    String::new()
                });

            if !cookies.is_empty() {
                 let mut cookie_val = HeaderValue::from_str(&cookies)?;
                 cookie_val.set_sensitive(true);
                 headers.insert(COOKIE, cookie_val);
            }
        }

        let client = ClientBuilder::new()
            .default_headers(headers)
            .cookie_store(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()?;

        // Detect provider type and initialize appropriate driver.
        // For now, we default to Google.
        let driver_impl: Box<dyn Provider> = Box::new(GoogleClient::new(client));

        Ok(Self { driver: Arc::new(driver_impl) })
    }

    pub async fn chat_completion(&self, prompt: &str) -> Result<String> {
        self.driver.generate(prompt).await
    }
}
