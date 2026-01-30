use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// =============================================================================
// Constants
// =============================================================================

const OS_VERSIONS_MACOS: &[&str] = &["13.5.2", "14.2.1", "14.5", "15.0", "15.1", "15.2"];

const OS_VERSIONS_WINDOWS: &[&str] = &[
    "10.0.22631",
    "10.0.26100",
];

const OS_VERSIONS_LINUX: &[&str] = &["6.5.0", "6.6.0", "6.8.0", "6.9.0", "6.10.0", "6.11.0"];

const ARCHITECTURES: &[&str] = &["x64", "arm64"];

// Antigravity IDE version (not plugin version)
// Current version as of Jan 2026: 1.15.8
const ANTIGRAVITY_IDE_VERSION: &str = "1.15.8";

const IDE_TYPES: &[&str] = &[
    "IDE_UNSPECIFIED",
    "VSCODE",
    "INTELLIJ",
    "ANDROID_STUDIO",
    "CLOUD_SHELL_EDITOR",
];

const PLATFORMS: &[&str] = &["PLATFORM_UNSPECIFIED", "WINDOWS", "MACOS", "LINUX"];

const SDK_CLIENTS: &[&str] = &[
    "google-cloud-sdk vscode_cloudshelleditor/0.1",
    "google-cloud-sdk vscode/1.96.0",
    "google-cloud-sdk vscode/1.95.0",
    "google-cloud-sdk jetbrains/2024.3",
    "google-cloud-sdk vscode/1.97.0",
];

// Gemini CLI style headers (for dual quota system)
const GEMINI_CLI_USER_AGENTS: &[&str] = &[
    "google-api-nodejs-client/10.3.0",
    "google-api-nodejs-client/9.15.1",
    "google-api-nodejs-client/9.14.0",
    "google-api-nodejs-client/9.13.0",
];

const GEMINI_CLI_API_CLIENTS: &[&str] = &[
    "gl-node/22.18.0",
    "gl-node/22.17.0",
    "gl-node/22.12.0",
    "gl-node/20.18.0",
    "gl-node/21.7.0",
];

// =============================================================================
// Types
// =============================================================================

/// Header style for API requests
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderStyle {
    /// Antigravity IDE style headers (default)
    Antigravity,
    /// Gemini CLI style headers (for dual quota)
    GeminiCli,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientMetadata {
    pub ide_type: String,
    pub platform: String,
    pub plugin_type: String,
    pub os_version: String,
    pub arch: String,
    pub sqm_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub device_id: String,
    pub session_token: String,
    pub user_agent: String,
    pub api_client: String,
    pub client_metadata: ClientMetadata,
    pub quota_user: String,
    pub created_at: u64,
}

// =============================================================================
// Implementation
// =============================================================================

impl Fingerprint {
    /// Generates a randomized device fingerprint
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();

        let platform_key = ["darwin", "win32", "linux"].choose(&mut rng).unwrap();
        let arch = ARCHITECTURES.choose(&mut rng).unwrap();

        let os_version = match *platform_key {
            "darwin" => OS_VERSIONS_MACOS.choose(&mut rng).unwrap(),
            "win32" => OS_VERSIONS_WINDOWS.choose(&mut rng).unwrap(),
            _ => OS_VERSIONS_LINUX.choose(&mut rng).unwrap(),
        };

        // Use current Antigravity IDE version (not randomized to avoid "version no longer supported" errors)
        let antigravity_version = ANTIGRAVITY_IDE_VERSION;

        let matching_platform = match *platform_key {
            "darwin" => "MACOS",
            "win32" => "WINDOWS",
            "linux" => "LINUX",
            _ => PLATFORMS.choose(&mut rng).unwrap(),
        };

        // Generate identifiers
        let device_id = Uuid::new_v4().to_string();
        let sqm_id = format!("{{{{{}}}}}", Uuid::new_v4().to_string().to_uppercase());

        // Session token: 16 random bytes hex
        // We'll use Uuid simple as a proxy for 16 bytes randomness
        let session_token = Uuid::new_v4().simple().to_string();

        // Quota User: device-{hex}
        // Using first 16 chars of uuid as random hex
        let random_hex = Uuid::new_v4().simple().to_string();
        let quota_user = format!("device-{}", &random_hex[..16]);

        let user_agent = format!(
            "antigravity/{} {}/{}",
            antigravity_version, platform_key, arch
        );
        let api_client = SDK_CLIENTS.choose(&mut rng).unwrap().to_string();
        let ide_type = IDE_TYPES.choose(&mut rng).unwrap().to_string();

        Self {
            device_id,
            session_token,
            user_agent,
            api_client,
            client_metadata: ClientMetadata {
                ide_type,
                platform: matching_platform.to_string(),
                plugin_type: "GEMINI".to_string(),
                os_version: os_version.to_string(),
                arch: arch.to_string(),
                sqm_id: Some(sqm_id),
            },
            quota_user,
            created_at: chrono::Utc::now().timestamp() as u64,
        }
    }

    /// Builds the HTTP headers for this fingerprint (Antigravity style by default)
    pub fn to_headers(&self) -> HashMap<String, String> {
        self.to_headers_with_style(HeaderStyle::Antigravity)
    }

    /// Builds the HTTP headers with a specific style
    pub fn to_headers_with_style(&self, style: HeaderStyle) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        
        match style {
            HeaderStyle::Antigravity => {
                headers.insert("User-Agent".to_string(), self.user_agent.clone());
                headers.insert("X-Goog-Api-Client".to_string(), self.api_client.clone());
                if let Ok(json_metadata) = serde_json::to_string(&self.client_metadata) {
                    headers.insert("Client-Metadata".to_string(), json_metadata);
                }
            }
            HeaderStyle::GeminiCli => {
                let mut rng = rand::thread_rng();
                let user_agent = GEMINI_CLI_USER_AGENTS.choose(&mut rng).unwrap().to_string();
                let api_client = GEMINI_CLI_API_CLIENTS.choose(&mut rng).unwrap().to_string();
                headers.insert("User-Agent".to_string(), user_agent);
                headers.insert("X-Goog-Api-Client".to_string(), api_client);
                // Gemini CLI uses a different Client-Metadata format (key=value pairs)
                headers.insert("Client-Metadata".to_string(), 
                    "ideType=IDE_UNSPECIFIED,platform=PLATFORM_UNSPECIFIED,pluginType=GEMINI".to_string());
            }
        }

        headers.insert("X-Goog-QuotaUser".to_string(), self.quota_user.clone());
        headers.insert("X-Client-Device-Id".to_string(), self.device_id.clone());
        headers.insert("X-Goog-Session-Id".to_string(), self.session_token.clone());

        headers
    }
}
