//! Cookie extraction from browser profiles
//!
//! Reads cookies directly from the browser's SQLite database without launching a browser.
//! This is faster and doesn't cause browser windows to pop up.

use anyhow::{Result, anyhow, Context};
use std::path::Path;

pub struct CookieExtractor;

impl CookieExtractor {
    /// Extracts cookies for a specific domain by reading from the browser's cookie database.
    ///
    /// # Arguments
    /// * `domain` - The domain to extract cookies for (e.g., "ide.google.com")
    /// * `cookie_names` - List of cookie names to extract
    /// * `browser_profile_path` - Path to the browser profile directory (contains Cookies file)
    pub fn extract_cookies(domain: &str, cookie_names: &[&str], browser_profile_path: Option<&str>) -> Result<String> {
        let profile_path = browser_profile_path
            .ok_or_else(|| anyhow!("No browser profile path provided. Cannot extract cookies."))?;

        // Try multiple possible cookie file locations
        let cookie_paths = [
            format!("{}/Cookies", profile_path),
            format!("{}/Default/Cookies", profile_path),
            format!("{}/Network/Cookies", profile_path),
            format!("{}/Default/Network/Cookies", profile_path),
        ];

        let cookie_db = cookie_paths.iter()
            .find(|p| Path::new(p).exists())
            .ok_or_else(|| anyhow!(
                "Cookie database not found. Tried paths:\n{}",
                cookie_paths.join("\n")
            ))?;

        tracing::info!("Reading cookies from: {}", cookie_db);

        // Copy the database to a temp location (browser may have it locked)
        let temp_db = std::env::temp_dir().join("aether_cookies_tmp.db");
        std::fs::copy(cookie_db, &temp_db)
            .context("Failed to copy cookie database. Is the browser open? Close it first.")?;

        // Open with rusqlite
        let conn = rusqlite::Connection::open_with_flags(
            &temp_db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).context("Failed to open cookie database")?;

        // Query for matching cookies
        // Chrome/Chromium store cookies with host_key having a leading dot for domain cookies
        let domain_patterns: Vec<String> = vec![
            domain.to_string(),
            format!(".{}", domain),
        ];

        let mut cookie_string = String::new();

        for name in cookie_names {
            let query = "SELECT name, value, encrypted_value FROM cookies WHERE host_key IN (?1, ?2) AND name = ?3";

            let result: Result<(String, Vec<u8>, Vec<u8>), _> = conn.query_row(
                query,
                rusqlite::params![&domain_patterns[0], &domain_patterns[1], name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            );

            if let Ok((name, value, encrypted_value)) = result {
                // Try unencrypted value first (older Chrome versions)
                let cookie_value = if !value.is_empty() {
                    String::from_utf8_lossy(&value).to_string()
                } else if encrypted_value.len() > 3 {
                    // Encrypted cookies start with v10, v11, etc.
                    // For now, we can't decrypt without OS keyring integration
                    tracing::warn!(
                        "Cookie '{}' is encrypted. AetherBridge cannot decrypt browser cookies on this system. \
                        Please ensure you're logged in and cookies are accessible.",
                        name
                    );
                    continue;
                } else {
                    continue;
                };

                if !cookie_string.is_empty() {
                    cookie_string.push_str("; ");
                }
                cookie_string.push_str(&format!("{}={}", name, cookie_value));
            }
        }

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_db);

        if cookie_string.is_empty() {
            return Err(anyhow!(
                "No accessible cookies found for domain '{}'. \
                Please ensure:\n\
                1. You are logged into {} in your browser\n\
                2. The browser is CLOSED before starting AetherBridge\n\
                3. On Linux, you may need to use the headless_chrome fallback",
                domain, domain
            ));
        }

        tracing::info!("Successfully extracted cookies for {}", domain);
        Ok(cookie_string)
    }
}
