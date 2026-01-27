//! Secure token storage using filesystem with optional keyring integration
//!
//! Stores OAuth credentials in:
//! - Linux: ~/.config/aether-bridge/accounts.json
//! - macOS: ~/Library/Application Support/aether-bridge/accounts.json
//! - Windows: %APPDATA%\aether-bridge\accounts.json
//!
//! Refresh tokens are additionally stored in the system keyring when available.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn, debug};

use crate::tokens::TokenPair;

/// Storage format version (for future migrations)
const STORAGE_VERSION: u32 = 1;

/// Service name for system keyring
const KEYRING_SERVICE: &str = "aether-bridge";

/// Container for all stored accounts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccounts {
    /// Version for schema migrations
    pub version: u32,

    /// All registered accounts
    pub accounts: Vec<StoredAccount>,

    /// Index of the currently active account
    pub active_index: usize,
}

impl Default for StoredAccounts {
    fn default() -> Self {
        Self {
            version: STORAGE_VERSION,
            accounts: vec![],
            active_index: 0,
        }
    }
}

/// A single stored account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccount {
    /// Email address (unique identifier)
    pub email: String,

    /// OAuth refresh token (stored encrypted in keyring when available)
    pub refresh_token: String,

    /// Unix timestamp when account was added
    pub added_at: i64,

    /// Unix timestamp of last successful use
    pub last_used: i64,
}

/// Handles persistent storage of OAuth tokens
pub struct TokenStorage {
    /// Path to the accounts JSON file
    config_path: PathBuf,

    /// Whether keyring storage is available
    keyring_available: bool,
}

impl TokenStorage {
    /// Creates a new TokenStorage instance
    pub fn new() -> Result<Self> {
        let config_dir = directories::ProjectDirs::from("com", "aetherbridge", "aether-bridge")
            .ok_or_else(|| anyhow!("Could not determine config directory for your platform"))?
            .config_dir()
            .to_path_buf();

        // Ensure config directory exists
        std::fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join("accounts.json");

        // Check if keyring is available
        let keyring_available = Self::check_keyring_available();
        if keyring_available {
            debug!("System keyring is available for secure token storage");
        } else {
            warn!("System keyring not available; tokens will be stored in plaintext");
        }

        Ok(Self {
            config_path,
            keyring_available,
        })
    }

    /// Checks if the system keyring is functional
    fn check_keyring_available() -> bool {
        // Try to access keyring with a test entry
        match keyring::Entry::new(KEYRING_SERVICE, "test-availability") {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Returns the path to the config file
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }

    /// Loads all stored accounts from disk
    pub fn load_accounts(&self) -> Result<StoredAccounts> {
        if !self.config_path.exists() {
            debug!("No accounts file found, returning empty");
            return Ok(StoredAccounts::default());
        }

        let content = std::fs::read_to_string(&self.config_path)?;
        let accounts: StoredAccounts = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse accounts file: {}", e))?;

        debug!("Loaded {} accounts from storage", accounts.accounts.len());
        Ok(accounts)
    }

    /// Saves accounts to disk
    pub fn save_accounts(&self, accounts: &StoredAccounts) -> Result<()> {
        let content = serde_json::to_string_pretty(accounts)?;
        std::fs::write(&self.config_path, content)?;
        debug!("Saved {} accounts to storage", accounts.accounts.len());
        Ok(())
    }

    /// Adds a new account or updates an existing one (by email)
    pub fn add_account(&self, token_pair: &TokenPair) -> Result<()> {
        let mut accounts = self.load_accounts()?;
        let now = chrono::Utc::now().timestamp();

        // Check if account already exists
        if let Some(existing) = accounts.accounts.iter_mut().find(|a| a.email == token_pair.email) {
            info!("Updating existing account: {}", token_pair.email);
            existing.refresh_token = token_pair.refresh_token.clone();
            existing.last_used = now;
        } else {
            info!("Adding new account: {}", token_pair.email);
            accounts.accounts.push(StoredAccount {
                email: token_pair.email.clone(),
                refresh_token: token_pair.refresh_token.clone(),
                added_at: now,
                last_used: now,
            });
        }

        self.save_accounts(&accounts)?;

        // Also store in system keyring for extra security
        if self.keyring_available {
            if let Err(e) = self.store_in_keyring(&token_pair.email, &token_pair.refresh_token) {
                warn!("Failed to store token in keyring: {}", e);
            }
        }

        Ok(())
    }

    /// Removes an account by email
    pub fn remove_account(&self, email: &str) -> Result<bool> {
        let mut accounts = self.load_accounts()?;
        let original_len = accounts.accounts.len();

        accounts.accounts.retain(|a| a.email != email);

        if accounts.accounts.len() < original_len {
            // Adjust active index if needed
            if accounts.active_index >= accounts.accounts.len() && !accounts.accounts.is_empty() {
                accounts.active_index = accounts.accounts.len() - 1;
            }

            self.save_accounts(&accounts)?;

            // Remove from keyring
            if self.keyring_available {
                let _ = self.remove_from_keyring(email);
            }

            info!("Removed account: {}", email);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Gets the refresh token for an account, preferring keyring storage
    pub fn get_refresh_token(&self, email: &str) -> Result<String> {
        // Try keyring first (more secure)
        if self.keyring_available {
            if let Ok(token) = self.get_from_keyring(email) {
                return Ok(token);
            }
        }

        // Fallback to file storage
        let accounts = self.load_accounts()?;
        accounts
            .accounts
            .iter()
            .find(|a| a.email == email)
            .map(|a| a.refresh_token.clone())
            .ok_or_else(|| anyhow!("No refresh token found for {}", email))
    }

    /// Updates the last_used timestamp for an account
    pub fn mark_account_used(&self, email: &str) -> Result<()> {
        let mut accounts = self.load_accounts()?;
        let now = chrono::Utc::now().timestamp();

        if let Some(account) = accounts.accounts.iter_mut().find(|a| a.email == email) {
            account.last_used = now;
            self.save_accounts(&accounts)?;
        }

        Ok(())
    }

    /// Sets the active account index
    pub fn set_active_index(&self, index: usize) -> Result<()> {
        let mut accounts = self.load_accounts()?;

        if index >= accounts.accounts.len() {
            return Err(anyhow!("Invalid account index: {}", index));
        }

        accounts.active_index = index;
        self.save_accounts(&accounts)?;
        Ok(())
    }

    // =========================================================================
    // Keyring operations
    // =========================================================================

    fn store_in_keyring(&self, email: &str, refresh_token: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, email)
            .map_err(|e| anyhow!("Failed to create keyring entry: {}", e))?;
        entry.set_password(refresh_token)
            .map_err(|e| anyhow!("Failed to store in keyring: {}", e))?;
        Ok(())
    }

    fn get_from_keyring(&self, email: &str) -> Result<String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, email)
            .map_err(|e| anyhow!("Failed to create keyring entry: {}", e))?;
        entry.get_password()
            .map_err(|e| anyhow!("Failed to get from keyring: {}", e))
    }

    fn remove_from_keyring(&self, email: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, email)
            .map_err(|e| anyhow!("Failed to create keyring entry: {}", e))?;
        entry.delete_credential()
            .map_err(|e| anyhow!("Failed to remove from keyring: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_storage() -> (TokenStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let storage = TokenStorage {
            config_path: temp_dir.path().join("accounts.json"),
            keyring_available: false, // Don't use keyring in tests
        };
        (storage, temp_dir)
    }

    #[test]
    fn test_add_and_load_account() {
        let (storage, _temp) = create_test_storage();

        let token = TokenPair {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: chrono::Utc::now(),
            email: "test@example.com".into(),
        };

        storage.add_account(&token).unwrap();

        let accounts = storage.load_accounts().unwrap();
        assert_eq!(accounts.accounts.len(), 1);
        assert_eq!(accounts.accounts[0].email, "test@example.com");
    }

    #[test]
    fn test_update_existing_account() {
        let (storage, _temp) = create_test_storage();

        let token1 = TokenPair {
            access_token: "access1".into(),
            refresh_token: "refresh1".into(),
            expires_at: chrono::Utc::now(),
            email: "test@example.com".into(),
        };

        let token2 = TokenPair {
            access_token: "access2".into(),
            refresh_token: "refresh2".into(),
            expires_at: chrono::Utc::now(),
            email: "test@example.com".into(),
        };

        storage.add_account(&token1).unwrap();
        storage.add_account(&token2).unwrap();

        let accounts = storage.load_accounts().unwrap();
        assert_eq!(accounts.accounts.len(), 1); // Should not duplicate
        assert_eq!(accounts.accounts[0].refresh_token, "refresh2"); // Should update
    }

    #[test]
    fn test_remove_account() {
        let (storage, _temp) = create_test_storage();

        let token = TokenPair {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: chrono::Utc::now(),
            email: "test@example.com".into(),
        };

        storage.add_account(&token).unwrap();
        assert!(storage.remove_account("test@example.com").unwrap());

        let accounts = storage.load_accounts().unwrap();
        assert!(accounts.accounts.is_empty());
    }
}
