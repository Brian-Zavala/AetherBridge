//! Multi-account management with rate limit aware rotation
//!
//! This module provides intelligent account rotation to handle API rate limits:
//! - Tracks rate limit status per account
//! - Automatically rotates to available accounts when one is rate-limited
//! - Refreshes access tokens as needed
//! - Persists account state to disk

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use tracing::{info, warn, debug, error};
use anyhow::Result;

use crate::storage::{TokenStorage, StoredAccount, StoredAccounts};
use crate::tokens::{TokenPair, refresh_access_token};

/// Represents a loaded account with runtime state
#[derive(Debug, Clone)]
pub struct Account {
    /// Index in the accounts list
    pub index: usize,

    /// Email address
    pub email: String,

    /// Current access token
    pub access_token: String,

    /// When the access token expires
    pub expires_at: DateTime<Utc>,

    /// Refresh token for obtaining new access tokens
    pub refresh_token: String,
}

impl Account {
    /// Checks if the access token needs refreshing (with 5 min buffer)
    pub fn needs_refresh(&self) -> bool {
        Utc::now() + chrono::Duration::minutes(5) >= self.expires_at
    }
}

/// Rate limit tracking for an account
#[derive(Debug, Clone)]
struct RateLimitInfo {
    /// When the rate limit expires
    #[allow(dead_code)]
    until: DateTime<Utc>,

    /// Number of consecutive rate limits
    #[allow(dead_code)]
    consecutive_count: u32,
}

/// Manages multiple OAuth accounts with intelligent rotation
pub struct AccountManager {
    /// Persistent storage (None for empty/uninitialized state)
    storage: Option<TokenStorage>,

    /// Loaded accounts with runtime state
    accounts: Arc<RwLock<Vec<Account>>>,

    /// Rate limit tracking per account index
    rate_limits: Arc<RwLock<HashMap<usize, RateLimitInfo>>>,

    /// Index of the last used account (for round-robin)
    last_used_index: Arc<RwLock<usize>>,
}

impl AccountManager {
    /// Creates an empty AccountManager (for backwards compatibility)
    ///
    /// This creates an uninitialized manager that has no accounts and
    /// cannot persist tokens. Use `new()` for full functionality.
    pub fn empty() -> Self {
        Self {
            storage: None,
            accounts: Arc::new(RwLock::new(vec![])),
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            last_used_index: Arc::new(RwLock::new(0)),
        }
    }

    /// Checks if this manager is properly initialized
    pub fn is_initialized(&self) -> bool {
        self.storage.is_some()
    }

    /// Creates a new AccountManager and loads accounts from storage
    pub async fn new() -> Result<Self> {
        let storage = TokenStorage::new()?;
        let stored = storage.load_accounts()?;

        let manager = Self {
            storage: Some(storage),
            accounts: Arc::new(RwLock::new(vec![])),
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            last_used_index: Arc::new(RwLock::new(stored.active_index)),
        };

        // Load and refresh accounts
        manager.load_accounts_from_storage(&stored).await?;

        Ok(manager)
    }

    /// Loads accounts from storage and refreshes access tokens
    async fn load_accounts_from_storage(&self, stored: &StoredAccounts) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        accounts.clear();

        for (idx, stored_account) in stored.accounts.iter().enumerate() {
            match self.refresh_token_for_account(stored_account).await {
                Ok(token_pair) => {
                    accounts.push(Account {
                        index: idx,
                        email: stored_account.email.clone(),
                        access_token: token_pair.access_token,
                        expires_at: token_pair.expires_at,
                        refresh_token: token_pair.refresh_token,
                    });
                    info!("Loaded account: {}", stored_account.email);
                }
                Err(e) => {
                    warn!("Failed to refresh token for {}: {}", stored_account.email, e);
                    // Still add the account but with empty access token
                    // Will attempt refresh on use
                    accounts.push(Account {
                        index: idx,
                        email: stored_account.email.clone(),
                        access_token: String::new(),
                        expires_at: Utc::now() - chrono::Duration::hours(1), // Expired
                        refresh_token: stored_account.refresh_token.clone(),
                    });
                }
            }
        }

        info!("Loaded {} accounts", accounts.len());
        Ok(())
    }

    /// Refreshes the access token for a stored account
    async fn refresh_token_for_account(&self, stored: &StoredAccount) -> Result<TokenPair> {
        refresh_access_token(&stored.refresh_token).await
    }

    /// Returns the number of configured accounts
    pub async fn account_count(&self) -> usize {
        self.accounts.read().await.len()
    }

    /// Gets all account emails for display
    pub async fn get_account_emails(&self) -> Vec<String> {
        self.accounts.read().await.iter().map(|a| a.email.clone()).collect()
    }

    /// Adds a new account from a token pair
    pub async fn add_account(&self, token_pair: TokenPair) -> Result<()> {
        // Save to storage if available
        if let Some(storage) = &self.storage {
            storage.add_account(&token_pair)?;
        }

        // Add to in-memory list
        let mut accounts = self.accounts.write().await;

        // Check if account already exists (update it)
        if let Some(existing) = accounts.iter_mut().find(|a| a.email == token_pair.email) {
            existing.access_token = token_pair.access_token;
            existing.expires_at = token_pair.expires_at;
            existing.refresh_token = token_pair.refresh_token;
            info!("Updated existing account: {}", token_pair.email);
        } else {
            let index = accounts.len();
            accounts.push(Account {
                index,
                email: token_pair.email.clone(),
                access_token: token_pair.access_token,
                expires_at: token_pair.expires_at,
                refresh_token: token_pair.refresh_token,
            });
            info!("Added new account: {}", token_pair.email);
        }

        Ok(())
    }

    /// Removes an account by email
    pub async fn remove_account(&self, email: &str) -> Result<bool> {
        let removed = if let Some(storage) = &self.storage {
            storage.remove_account(email)?
        } else {
            // If no storage, just remove from memory
            let accounts = self.accounts.read().await;
            accounts.iter().any(|a| a.email == email)
        };

        if removed {
            let mut accounts = self.accounts.write().await;
            accounts.retain(|a| a.email != email);

            // Re-index accounts
            for (i, account) in accounts.iter_mut().enumerate() {
                account.index = i;
            }

            info!("Removed account: {}", email);
        }

        Ok(removed)
    }

    /// Gets the next available account (not rate-limited) with fresh access token
    pub async fn get_available_account(&self) -> Option<Account> {
        let now = Utc::now();
        let mut accounts = self.accounts.write().await;
        let rate_limits = self.rate_limits.read().await;
        let last_used = *self.last_used_index.read().await;

        if accounts.is_empty() {
            return None;
        }

        // Start from the account after last used (round-robin)
        let account_count = accounts.len();
        for offset in 0..account_count {
            let idx = (last_used + offset + 1) % account_count;

            // Check rate limit
            if let Some(limit_info) = rate_limits.get(&idx) {
                if now < limit_info.until {
                    debug!("Account {} is rate-limited until {}", idx, limit_info.until);
                    continue;
                }
            }

            let account = &mut accounts[idx];

            // Refresh if needed
            if account.needs_refresh() {
                debug!("Refreshing token for account {}", account.email);
                match refresh_access_token(&account.refresh_token).await {
                    Ok(new_tokens) => {
                        account.access_token = new_tokens.access_token;
                        account.expires_at = new_tokens.expires_at;
                        if new_tokens.refresh_token != account.refresh_token {
                            account.refresh_token = new_tokens.refresh_token;
                        }
                    }
                    Err(e) => {
                        error!("Failed to refresh token for {}: {}", account.email, e);
                        continue; // Try next account
                    }
                }
            }

            // Update last used index
            drop(rate_limits);
            *self.last_used_index.write().await = idx;

            return Some(account.clone());
        }

        None
    }

    /// Marks an account as rate-limited
    pub async fn mark_rate_limited(&self, index: usize, until: DateTime<Utc>) {
        let mut rate_limits = self.rate_limits.write().await;

        let info = rate_limits.entry(index).or_insert(RateLimitInfo {
            until,
            consecutive_count: 0,
        });

        info.until = until;
        info.consecutive_count += 1;

        if let Some(account) = self.accounts.read().await.get(index) {
            warn!(
                "Account {} rate-limited until {} (consecutive: {})",
                account.email, until, info.consecutive_count
            );
        }
    }

    /// Clears the rate limit for an account (on successful request)
    pub async fn clear_rate_limit(&self, index: usize) {
        let mut rate_limits = self.rate_limits.write().await;
        rate_limits.remove(&index);
    }

    /// Gets the minimum wait time until any account becomes available
    pub async fn get_min_wait_time(&self) -> Option<std::time::Duration> {
        let rate_limits = self.rate_limits.read().await;
        let accounts = self.accounts.read().await;
        let now = Utc::now();

        // If there are accounts not in the rate_limits map, they're available immediately
        if accounts.iter().any(|a| !rate_limits.contains_key(&a.index)) {
            return None;
        }

        rate_limits
            .values()
            .filter(|info| info.until > now)
            .map(|info| (info.until - now).to_std().unwrap_or_default())
            .min()
    }

    /// Checks if all accounts are currently rate-limited
    pub async fn all_rate_limited(&self) -> bool {
        let rate_limits = self.rate_limits.read().await;
        let accounts = self.accounts.read().await;
        let now = Utc::now();

        if accounts.is_empty() {
            return false;
        }

        accounts.iter().all(|a| {
            rate_limits
                .get(&a.index)
                .map(|info| info.until > now)
                .unwrap_or(false)
        })
    }

    /// Reloads accounts from storage (useful after external changes)
    pub async fn reload(&self) -> Result<()> {
        if let Some(storage) = &self.storage {
            let stored = storage.load_accounts()?;
            self.load_accounts_from_storage(&stored).await
        } else {
            Ok(()) // No storage to reload from
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_account_needs_refresh() {
        let account = Account {
            index: 0,
            email: "test@example.com".into(),
            access_token: "token".into(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            refresh_token: "refresh".into(),
        };
        assert!(!account.needs_refresh());

        let expired_account = Account {
            index: 0,
            email: "test@example.com".into(),
            access_token: "token".into(),
            expires_at: Utc::now() - chrono::Duration::hours(1),
            refresh_token: "refresh".into(),
        };
        assert!(expired_account.needs_refresh());
    }
}
