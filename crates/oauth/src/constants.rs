//! OAuth constants for Antigravity (Google Cloud Code Assist)
//!
//! These values are extracted from OpenCode's opencode-antigravity-auth plugin.

/// Client ID for Antigravity OAuth application
pub const ANTIGRAVITY_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";

/// Client secret for Antigravity OAuth application
pub const ANTIGRAVITY_CLIENT_SECRET: &str =
    "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";

/// Required OAuth scopes for full Antigravity access
pub const ANTIGRAVITY_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

/// Local callback port for OAuth redirect
pub const OAUTH_CALLBACK_PORT: u16 = 51121;

/// OAuth redirect URI (must match Google Console configuration)
pub const ANTIGRAVITY_REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";

/// Google OAuth authorization endpoint
pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Google OAuth token exchange endpoint
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Google userinfo endpoint for fetching email
pub const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

// =============================================================================
// Cloud Code Assist API Endpoints
// =============================================================================

/// Primary endpoint (daily sandbox - same as CLIProxy/Vibeproxy)
pub const ANTIGRAVITY_ENDPOINT_DAILY: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";

/// Secondary endpoint (autopush sandbox)
pub const ANTIGRAVITY_ENDPOINT_AUTOPUSH: &str = "https://autopush-cloudcode-pa.sandbox.googleapis.com";

/// Production endpoint
pub const ANTIGRAVITY_ENDPOINT_PROD: &str = "https://cloudcode-pa.googleapis.com";

/// Endpoint fallback order (daily → autopush → prod)
pub const ANTIGRAVITY_ENDPOINTS: &[&str] = &[
    ANTIGRAVITY_ENDPOINT_DAILY,
    ANTIGRAVITY_ENDPOINT_AUTOPUSH,
    ANTIGRAVITY_ENDPOINT_PROD,
];

/// Default project ID - UPDATED JAN 2026
/// WARNING: 'rising-fact-p41fc' was revoked. Using a placeholder to force discovery.
pub const ANTIGRAVITY_DEFAULT_PROJECT_ID: &str = "REQUIRE_USER_PROJECT_ID";

// =============================================================================
// Request Headers (impersonating Antigravity IDE)
// =============================================================================

/// User-Agent header for Antigravity requests
pub const ANTIGRAVITY_USER_AGENT: &str = "antigravity/1.11.5 linux/amd64";

/// X-Goog-Api-Client header
pub const ANTIGRAVITY_API_CLIENT: &str = "google-cloud-sdk vscode_cloudshelleditor/0.1";

/// Client-Metadata header (JSON)
pub const ANTIGRAVITY_CLIENT_METADATA: &str =
    r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#;
