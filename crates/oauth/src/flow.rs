//! OAuth 2.0 Authorization Code flow with PKCE
//!
//! Implements the secure OAuth flow for desktop applications:
//! 1. Generate PKCE code verifier/challenge
//! 2. Open browser for user authorization
//! 3. Listen for OAuth callback on localhost
//! 4. Exchange authorization code for tokens

use anyhow::{anyhow, Result};
use axum::{
    extract::Query,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn, error};

use crate::constants::*;
use crate::tokens::TokenPair;

/// Generates a cryptographically secure state parameter
fn generate_state() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Generates PKCE code verifier and challenge
///
/// Returns (verifier, challenge) tuple
fn generate_pkce() -> (String, String) {
    // Generate a random 32-byte verifier
    let verifier: [u8; 32] = rand::thread_rng().gen();
    let verifier_str = URL_SAFE_NO_PAD.encode(verifier);

    // Create SHA-256 hash of verifier for challenge
    let mut hasher = Sha256::new();
    hasher.update(verifier_str.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    (verifier_str, challenge)
}

/// Manages the OAuth 2.0 authorization flow
pub struct OAuthFlow {
    state: String,
    code_verifier: String,
    code_challenge: String,
}

impl OAuthFlow {
    /// Creates a new OAuth flow with fresh PKCE parameters
    pub fn new() -> Self {
        let (verifier, challenge) = generate_pkce();
        Self {
            state: generate_state(),
            code_verifier: verifier,
            code_challenge: challenge,
        }
    }

    /// Returns the authorization URL to open in the browser
    pub fn authorization_url(&self) -> String {
        let scopes = ANTIGRAVITY_SCOPES.join(" ");
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
            GOOGLE_AUTH_URL,
            ANTIGRAVITY_CLIENT_ID,
            urlencoding::encode(ANTIGRAVITY_REDIRECT_URI),
            urlencoding::encode(&scopes),
            &self.state,
            &self.code_challenge,
        )
    }

    /// Starts the local callback server and waits for the OAuth redirect
    ///
    /// This spawns a temporary HTTP server on the callback port that waits
    /// for Google to redirect the user back after authorization.
    ///
    /// # Returns
    /// The authorization code from the callback
    pub async fn wait_for_callback(&self) -> Result<String> {
        let expected_state = self.state.clone();
        let (tx, rx) = oneshot::channel::<Result<String>>();
        let tx = Arc::new(Mutex::new(Some(tx)));

        // Build the callback handler
        let app = Router::new().route(
            "/oauth-callback",
            get({
                let tx = tx.clone();
                let expected_state = expected_state.clone();
                move |Query(params): Query<CallbackParams>| {
                    let tx = tx.clone();
                    let expected_state = expected_state.clone();
                    async move {
                        // Validate state to prevent CSRF
                        if params.state != expected_state {
                            warn!("OAuth callback received with invalid state");
                            if let Some(tx) = tx.lock().await.take() {
                                let _ = tx.send(Err(anyhow!("Invalid OAuth state - possible CSRF attack")));
                            }
                            return Html(ERROR_HTML).into_response();
                        }

                        // Check for error response
                        if let Some(error) = params.error {
                            error!("OAuth error: {}", error);
                            if let Some(tx) = tx.lock().await.take() {
                                let _ = tx.send(Err(anyhow!("OAuth error: {}", error)));
                            }
                            return Html(ERROR_HTML).into_response();
                        }

                        // Extract authorization code
                        let code = match params.code {
                            Some(code) => code,
                            None => {
                                if let Some(tx) = tx.lock().await.take() {
                                    let _ = tx.send(Err(anyhow!("No authorization code in callback")));
                                }
                                return Html(ERROR_HTML).into_response();
                            }
                        };

                        info!("OAuth callback received successfully");
                        if let Some(tx) = tx.lock().await.take() {
                            let _ = tx.send(Ok(code));
                        }

                        Html(SUCCESS_HTML).into_response()
                    }
                }
            }),
        );

        // Start the callback server
        let listener = tokio::net::TcpListener::bind(
            format!("127.0.0.1:{}", OAUTH_CALLBACK_PORT)
        ).await.map_err(|e| {
            anyhow!("Failed to bind OAuth callback port {}: {}. Is another instance running?",
                    OAUTH_CALLBACK_PORT, e)
        })?;

        info!("OAuth callback server listening on port {}", OAUTH_CALLBACK_PORT);

        // Spawn server task
        let server_handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!("OAuth callback server error: {}", e);
            }
        });

        // Wait for callback with 5 minute timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            rx
        ).await
            .map_err(|_| anyhow!("OAuth timeout - no callback received within 5 minutes"))?
            .map_err(|_| anyhow!("OAuth callback channel closed unexpectedly"))?;

        // Shutdown the server
        server_handle.abort();

        result
    }

    /// Exchanges the authorization code for access and refresh tokens
    pub async fn exchange_code(&self, code: &str) -> Result<TokenPair> {
        info!("Exchanging authorization code for tokens");

        let client = reqwest::Client::new();

        let response = client
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("client_id", ANTIGRAVITY_CLIENT_ID),
                ("client_secret", ANTIGRAVITY_CLIENT_SECRET),
                ("code", code),
                ("code_verifier", &self.code_verifier),
                ("grant_type", "authorization_code"),
                ("redirect_uri", ANTIGRAVITY_REDIRECT_URI),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Token exchange failed: {}", error_text));
        }

        let token_response: TokenResponse = response.json().await?;

        // Fetch user email
        let email = Self::fetch_user_email(&token_response.access_token).await?;

        // Calculate expiry
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(token_response.expires_in);

        info!("Successfully authenticated as {}", email);

        Ok(TokenPair {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_at,
            email,
        })
    }

    /// Fetches user email from Google's userinfo endpoint
    async fn fetch_user_email(access_token: &str) -> Result<String> {
        let client = reqwest::Client::new();
        let response: UserInfo = client
            .get(GOOGLE_USERINFO_URL)
            .bearer_auth(access_token)
            .send()
            .await?
            .json()
            .await?;

        Ok(response.email)
    }
}

impl Default for OAuthFlow {
    fn default() -> Self {
        Self::new()
    }
}

/// Query parameters from OAuth callback
#[derive(serde::Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: String,
    error: Option<String>,
}

/// Token endpoint response
#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

/// Userinfo endpoint response
#[derive(serde::Deserialize)]
struct UserInfo {
    email: String,
}

/// HTML shown on successful OAuth callback
const SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>AetherBridge - Login Successful</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
            color: white;
        }
        .container {
            text-align: center;
            padding: 40px;
            background: rgba(255,255,255,0.1);
            border-radius: 16px;
            backdrop-filter: blur(10px);
        }
        .checkmark {
            font-size: 64px;
            margin-bottom: 20px;
        }
        h1 { margin: 0 0 10px 0; }
        p { opacity: 0.8; }
    </style>
</head>
<body>
    <div class="container">
        <div class="checkmark">✅</div>
        <h1>Login Successful!</h1>
        <p>You can close this window and return to AetherBridge.</p>
    </div>
</body>
</html>"#;

/// HTML shown on OAuth error
const ERROR_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>AetherBridge - Login Failed</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background: linear-gradient(135deg, #2e1a1a 0%, #3e1621 100%);
            color: white;
        }
        .container {
            text-align: center;
            padding: 40px;
            background: rgba(255,255,255,0.1);
            border-radius: 16px;
            backdrop-filter: blur(10px);
        }
        .error-icon {
            font-size: 64px;
            margin-bottom: 20px;
        }
        h1 { margin: 0 0 10px 0; }
        p { opacity: 0.8; }
    </style>
</head>
<body>
    <div class="container">
        <div class="error-icon">❌</div>
        <h1>Login Failed</h1>
        <p>An error occurred during authentication. Please try again.</p>
    </div>
</body>
</html>"#;
