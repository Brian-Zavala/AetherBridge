use std::sync::Arc;
use tokio::sync::Mutex;
use common::config::Config;
use browser_automator::Automator;
use oauth::AccountManager;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Application configuration
    pub config: Arc<Config>,
    /// Browser automator for legacy protocol driver
    pub automator: Arc<Mutex<Automator>>,
    /// OAuth account manager for Antigravity authentication
    pub account_manager: Arc<AccountManager>,
}

impl AppState {
    /// Creates a new AppState with legacy automator
    pub fn new(config: Config, automator: Automator) -> Self {
        // Create a placeholder account manager that will be initialized lazily
        // This maintains backwards compatibility with existing code
        Self {
            config: Arc::new(config),
            automator: Arc::new(Mutex::new(automator)),
            account_manager: Arc::new(AccountManager::empty()),
        }
    }

    /// Creates a new AppState with OAuth account manager
    pub async fn with_oauth(config: Config, automator: Automator) -> anyhow::Result<Self> {
        let account_manager = AccountManager::new().await?;

        Ok(Self {
            config: Arc::new(config),
            automator: Arc::new(Mutex::new(automator)),
            account_manager: Arc::new(account_manager),
        })
    }

    /// Sets the account manager
    pub fn set_account_manager(&mut self, manager: AccountManager) {
        self.account_manager = Arc::new(manager);
    }
}
