//! Multi-account management with rate limit aware rotation
//!
//! This module provides intelligent account rotation to handle API rate limits:
//! - Tracks rate limit status per account per model family (Claude vs Gemini)
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

/// Model family for per-family rate limit tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelFamily {
    /// Claude models (Sonnet, Opus)
    Claude,
    /// Gemini models (Pro, Flash)
    Gemini,
}

impl ModelFamily {
    /// Determines the model family from a model ID string
    pub fn from_model_id(model_id: &str) -> Self {
        let lower = model_id.to_lowercase();
        if lower.contains("claude") {
            ModelFamily::Claude
        } else {
            // Default to Gemini for any non-Claude model
            ModelFamily::Gemini
        }
    }
}

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

/// Rate limit tracking for an account per model family
#[derive(Debug, Clone)]
struct RateLimitInfo {
    /// When the rate limit expires
    until: DateTime<Utc>,

    /// Number of consecutive rate limits
    consecutive_count: u32,
}

/// Per-model-family rate limit tracking for an account
#[derive(Debug, Clone)]
struct AccountRateLimits {
    /// Rate limit info for Claude models
    claude: Option<RateLimitInfo>,
    /// Rate limit info for Gemini models
    gemini: Option<RateLimitInfo>,
}

impl AccountRateLimits {
    fn new() -> Self {
        Self {
            claude: None,
            gemini: None,
        }
    }

    /// Gets the rate limit info for a specific model family
    fn get(&self, family: ModelFamily) -> &Option<RateLimitInfo> {
        match family {
            ModelFamily::Claude => &self.claude,
            ModelFamily::Gemini => &self.gemini,
        }
    }

    /// Sets the rate limit info for a specific model family
    fn set(&mut self, family: ModelFamily, info: RateLimitInfo) {
        match family {
            ModelFamily::Claude => self.claude = Some(info),
            ModelFamily::Gemini => self.gemini = Some(info),
        }
    }

    /// Clears the rate limit for a specific model family
    fn clear(&mut self, family: ModelFamily) {
        match family {
            ModelFamily::Claude => self.claude = None,
            ModelFamily::Gemini => self.gemini = None,
        }
    }

    /// Checks if the account is rate-limited for a specific model family
    fn is_rate_limited(&self, family: ModelFamily, now: DateTime<Utc>) -> bool {
        if let Some(info) = self.get(family) {
            now < info.until
        } else {
            false
        }
    }

    /// Gets the earliest expiration time across all model families
    fn earliest_expiration(&self) -> Option<DateTime<Utc>> {
        let mut earliest = None;
        
        if let Some(ref claude_info) = self.claude {
            earliest = Some(claude_info.until);
        }
        
        if let Some(ref gemini_info) = self.gemini {
            if earliest.map(|e| gemini_info.until < e).unwrap_or(true) {
                earliest = Some(gemini_info.until);
            }
        }
        
        earliest
    }
}

/// Manages multiple OAuth accounts with intelligent rotation
pub struct AccountManager {
    /// Persistent storage (None for empty/uninitialized state)
    storage: Option<TokenStorage>,

    /// Loaded accounts with runtime state
    accounts: Arc<RwLock<Vec<Account>>>,

    /// Rate limit tracking per account index per model family
    /// This allows separate rate limits for Claude vs Gemini models
    rate_limits: Arc<RwLock<HashMap<usize, AccountRateLimits>>>,

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

    /// Gets an available account for a specific model family (not rate-limited for that family)
    /// 
    /// This is the primary method for account selection when the model family is known.
    /// It ensures that Claude rate limits don't affect Gemini requests and vice versa.
    pub async fn get_available_account_for_model(&self, model_id: &str) -> Option<Account> {
        let family = ModelFamily::from_model_id(model_id);
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

            // Check rate limit for this specific model family
            if let Some(account_limits) = rate_limits.get(&idx) {
                if account_limits.is_rate_limited(family, now) {
                    if let Some(account) = accounts.get(idx) {
                        debug!("Account {} is rate-limited for {:?} until {:?}", 
                               account.email, family, account_limits.get(family).as_ref().map(|i| i.until));
                    }
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

            // Check rate limit for any model family
            if let Some(account_limits) = rate_limits.get(&idx) {
                if account_limits.is_rate_limited(ModelFamily::Claude, now) ||
                   account_limits.is_rate_limited(ModelFamily::Gemini, now) {
                    debug!("Account {} is rate-limited", idx);
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

    /// Gets an account ignoring rate limits (used for fallback retry with different model)
    pub async fn get_available_account_ignoring_rate_limit(&self) -> Option<Account> {
        let mut accounts = self.accounts.write().await;
        // let rate_limits = self.rate_limits.read().await; // Ignored
        let last_used = *self.last_used_index.read().await;

        if accounts.is_empty() {
            return None;
        }

        let account_count = accounts.len();

        // Try all accounts starting from next in rotation
        for i in 0..account_count {
            let idx = (last_used + 1 + i) % account_count;
            let account = accounts.get_mut(idx).expect("Account should exist");

            // Refresh if needed
            if account.needs_refresh() {
                debug!("Refreshing token for account {} (fallback)", account.email);
                 match refresh_access_token(&account.refresh_token).await {
                    Ok(new_tokens) => {
                        account.access_token = new_tokens.access_token;
                        account.expires_at = new_tokens.expires_at;
                        if new_tokens.refresh_token != account.refresh_token {
                             account.refresh_token = new_tokens.refresh_token;
                        }
                    }
                    Err(e) => {
                        error!("Failed to refresh token for {}: {} (skipping in fallback)", account.email, e);
                        continue; // Try next account
                    }
                }
            }

            // Found a usable account
            *self.last_used_index.write().await = idx;
            return Some(account.clone());
        }

        error!("All accounts failed refresh in fallback selection");
        None
    }

    /// Marks an account as rate-limited for a specific model family
    /// 
    /// This allows separate rate limit tracking for Claude vs Gemini models,
    /// so that hitting a Claude rate limit doesn't prevent Gemini requests.
    pub async fn mark_rate_limited(&self, index: usize, family: ModelFamily, until: DateTime<Utc>) {
        let mut rate_limits = self.rate_limits.write().await;

        let account_limits = rate_limits.entry(index).or_insert_with(AccountRateLimits::new);

        let current_count = account_limits.get(family).as_ref().map(|i| i.consecutive_count).unwrap_or(0);
        
        account_limits.set(family, RateLimitInfo {
            until,
            consecutive_count: current_count + 1,
        });

        if let Some(account) = self.accounts.read().await.get(index) {
            warn!(
                "Account {} rate-limited for {:?} until {} (consecutive: {})",
                account.email, family, until, current_count + 1
            );
        }
    }

    /// Clears the rate limit for an account and model family (on successful request)
    pub async fn clear_rate_limit(&self, index: usize, family: ModelFamily) {
        let mut rate_limits = self.rate_limits.write().await;
        if let Some(account_limits) = rate_limits.get_mut(&index) {
            account_limits.clear(family);
            // If both families are clear, remove the entry entirely
            if account_limits.claude.is_none() && account_limits.gemini.is_none() {
                rate_limits.remove(&index);
            }
        }
    }

    /// Gets the minimum wait time until any account becomes available for a model family
    pub async fn get_min_wait_time_for_model(&self, model_id: &str) -> Option<std::time::Duration> {
        let family = ModelFamily::from_model_id(model_id);
        let rate_limits = self.rate_limits.read().await;
        let accounts = self.accounts.read().await;
        let now = Utc::now();

        // Check if any account is available for this model family
        let any_available = accounts.iter().any(|a| {
            if let Some(account_limits) = rate_limits.get(&a.index) {
                !account_limits.is_rate_limited(family, now)
            } else {
                true // No rate limits for this account
            }
        });

        if any_available {
            return None;
        }

        // Find the earliest expiration across all accounts for this family
        rate_limits
            .values()
            .filter_map(|account_limits| account_limits.get(family).as_ref())
            .filter(|info| info.until > now)
            .map(|info| (info.until - now).to_std().unwrap_or_default())
            .min()
    }

    /// Gets the minimum wait time until any account becomes available (legacy, checks all families)
    pub async fn get_min_wait_time(&self) -> Option<std::time::Duration> {
        let rate_limits = self.rate_limits.read().await;
        let accounts = self.accounts.read().await;
        let now = Utc::now();

        // Check if any account has no rate limits at all
        let any_available = accounts.iter().any(|a| {
            if let Some(account_limits) = rate_limits.get(&a.index) {
                // Account is available if neither family is rate-limited
                !account_limits.is_rate_limited(ModelFamily::Claude, now) &&
                !account_limits.is_rate_limited(ModelFamily::Gemini, now)
            } else {
                true // No rate limits for this account
            }
        });

        if any_available {
            return None;
        }

        // Find the earliest expiration across all accounts and families
        rate_limits
            .values()
            .filter_map(|account_limits| account_limits.earliest_expiration())
            .filter(|until| *until > now)
            .map(|until| (until - now).to_std().unwrap_or_default())
            .min()
    }

    /// Checks if all accounts are currently rate-limited for a specific model family
    pub async fn all_rate_limited_for_model(&self, model_id: &str) -> bool {
        let family = ModelFamily::from_model_id(model_id);
        let rate_limits = self.rate_limits.read().await;
        let accounts = self.accounts.read().await;
        let now = Utc::now();

        if accounts.is_empty() {
            return false;
        }

        accounts.iter().all(|a| {
            rate_limits
                .get(&a.index)
                .map(|account_limits| account_limits.is_rate_limited(family, now))
                .unwrap_or(false)
        })
    }

    /// Checks if all accounts are currently rate-limited (legacy, checks if all families limited)
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
                .map(|account_limits| {
                    account_limits.is_rate_limited(ModelFamily::Claude, now) &&
                    account_limits.is_rate_limited(ModelFamily::Gemini, now)
                })
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

    #[tokio::test]
    async fn test_get_available_account_ignoring_rate_limit() {
        let manager = AccountManager::empty();

        // Add a dummy account
        let token_pair = TokenPair {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            email: "test@example.com".into(),
        };
        manager.add_account(token_pair).await.unwrap();

        // Mark it as rate limited
        manager.mark_rate_limited(0, Utc::now() + chrono::Duration::hours(1)).await;

        // Should be None normally
        assert!(manager.get_available_account().await.is_none());

        // Should be Some ignoring limit
        let account = manager.get_available_account_ignoring_rate_limit().await;
        assert!(account.is_some());
        assert_eq!(account.unwrap().email, "test@example.com");
    }
}
