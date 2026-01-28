use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Result;
use directories::ProjectDirs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub project_id: Option<String>,
    pub accounts: HashMap<String, Account>,
    pub providers: HashMap<String, ProviderConfig>,
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub provider: String,
    pub credentials: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    pub api_type: ProviderType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderType {
    OpenAI,
    Anthropic,
    Google,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub browser_profile_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            project_id: None,
            accounts: HashMap::new(),
            providers: HashMap::new(),
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                browser_profile_path: None,
            },
        }
    }
}

impl Config {
    /// Get the configuration directory path
    pub fn get_config_dir() -> PathBuf {
        if let Some(proj_dirs) = ProjectDirs::from("com", "Brian-Zavala", "aether-bridge") {
            proj_dirs.config_dir().to_path_buf()
        } else {
            PathBuf::from(".config/aether-bridge")
        }
    }

    /// Get the configuration file path
    pub fn get_config_path() -> PathBuf {
        Self::get_config_dir().join("config.json")
    }

    /// Load configuration from disk
    pub fn load() -> Result<Self> {
        let path = Self::get_config_path();
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let config: Config = serde_json::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Save configuration to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::get_config_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}
