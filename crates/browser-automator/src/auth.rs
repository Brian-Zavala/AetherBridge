use anyhow::{Result, anyhow};
use headless_chrome::{Browser, LaunchOptions};
use std::time::Duration;

pub struct CookieExtractor;

impl CookieExtractor {
    /// Extracts cookies for a specific domain by launching a headful browser
    /// and allowing the user to log in (or retrieving existing session if using a user-data-dir).
    pub fn extract_cookies(domain: &str, cookie_names: &[&str], user_data_dir: Option<&str>) -> Result<String> {
        let mut builder = LaunchOptions::default_builder();
        builder
            .headless(false) // Headful so user can login if needed
            .idle_browser_timeout(Duration::from_secs(300));

        if let Some(path) = user_data_dir {
            builder.user_data_dir(Some(std::path::PathBuf::from(path)));
        }

        let launch_options = builder.build()
            .map_err(|e| anyhow!("Failed to build launch options: {}", e))?;

        let browser = Browser::new(launch_options)
            .map_err(|e| anyhow!("Failed to launch browser: {}", e))?;

        let tab = browser.new_tab()
            .map_err(|e| anyhow!("Failed to create tab: {}", e))?;

        // Navigate to the domain
        let url = format!("https://{}", domain);
        tab.navigate_to(&url)
            .map_err(|e| anyhow!("Failed to navigate to {}: {}", url, e))?;

        tab.wait_for_element("body")
            .map_err(|e| anyhow!("Failed to load page: {}", e))?;

        // In a real scenario, we would wait for a specific logged-in indicator
        // For now, we just grab cookies after a short delay or user interaction
        // simpler approach: just try to get the cookies

        let cookies = tab.get_cookies()
             .map_err(|e| anyhow!("Failed to get cookies: {}", e))?;

        let mut cookie_string = String::new();
        for cookie in cookies {
            if cookie_names.contains(&cookie.name.as_str()) {
                if !cookie_string.is_empty() {
                    cookie_string.push_str("; ");
                }
                cookie_string.push_str(&format!("{}={}", cookie.name, cookie.value));
            }
        }

        if cookie_string.is_empty() {
             return Err(anyhow!("No matching cookies found for domain {}. Are you logged in?", domain));
        }

        Ok(cookie_string)
    }
}
