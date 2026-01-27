//! Token types and refresh logic

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::constants::{
    ANTIGRAVITY_CLIENT_ID, ANTIGRAVITY_CLIENT_SECRET, GOOGLE_TOKEN_URL,
};

/// Represents an OAuth token pair with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    /// OAuth access token (short-lived, ~1 hour)
    pub access_token: String,

    /// OAuth refresh token (long-lived, used to obtain new access tokens)
    pub refresh_token: String,

    /// When the access token expires
    pub expires_at: DateTime<Utc>,

    /// Email associated with this Google account
    pub email: String,
}

impl TokenPair {
    /// Checks if the access token has expired (with 5 minute buffer)
    pub fn is_expired(&self) -> bool {
        Utc::now() + chrono::Duration::minutes(5) >= self.expires_at
    }
}

/// Response from Google's token endpoint
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    token_type: String,
}

/// Error response from Google's token endpoint
#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
    error_description: Option<String>,
}

/// Refreshes an access token using a refresh token
///
/// # Arguments
/// * `refresh_token` - The refresh token to use
///
/// # Returns
/// A new TokenPair with a fresh access token (and potentially rotated refresh token)
pub async fn refresh_access_token(refresh_token: &str) -> Result<TokenPair> {
    let client = reqwest::Client::new();

    let response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("client_id", ANTIGRAVITY_CLIENT_ID),
            ("client_secret", ANTIGRAVITY_CLIENT_SECRET),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;

        // Try to parse as error response
        if let Ok(error_resp) = serde_json::from_str::<TokenErrorResponse>(&error_text) {
            if error_resp.error == "invalid_grant" {
                return Err(anyhow!(
                    "Refresh token revoked or expired. Please re-authenticate with `aether login`."
                ));
            }
            return Err(anyhow!(
                "Token refresh failed: {} - {}",
                error_resp.error,
                error_resp.error_description.unwrap_or_default()
            ));
        }

        return Err(anyhow!("Token refresh failed: {}", error_text));
    }

    let token_response: TokenResponse = response.json().await?;

    // Calculate expiry time
    let expires_at = Utc::now() + chrono::Duration::seconds(token_response.expires_in);

    // Fetch user email with new access token
    let email = fetch_user_email(&token_response.access_token).await?;

    Ok(TokenPair {
        access_token: token_response.access_token,
        // Use new refresh token if rotated, otherwise keep the original
        refresh_token: token_response.refresh_token.unwrap_or_else(|| refresh_token.to_string()),
        expires_at,
        email,
    })
}

/// Fetches the user's email from Google's userinfo endpoint
async fn fetch_user_email(access_token: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct UserInfo {
        email: String,
    }

    let client = reqwest::Client::new();
    let response: UserInfo = client
        .get(crate::constants::GOOGLE_USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await?
        .json()
        .await?;

    Ok(response.email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_expiry() {
        let token = TokenPair {
            access_token: "test".into(),
            refresh_token: "test".into(),
            expires_at: Utc::now() - chrono::Duration::hours(1),
            email: "test@example.com".into(),
        };
        assert!(token.is_expired());

        let token = TokenPair {
            access_token: "test".into(),
            refresh_token: "test".into(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            email: "test@example.com".into(),
        };
        assert!(!token.is_expired());
    }
}
