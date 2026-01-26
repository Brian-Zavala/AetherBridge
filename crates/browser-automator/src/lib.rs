pub mod auth;
pub mod google_driver;
pub mod protocol_driver;
pub mod visual_driver;

use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Provider: Send + Sync {
    /// Generates a response for a given text prompt.
    async fn generate(&self, prompt: &str) -> Result<String>;
}

use common::config::Config;
use protocol_driver::ProtocolDriver;
use visual_driver::VisualDriver;

pub struct Automator {
    pub protocol: Option<ProtocolDriver>,
    pub visual: VisualDriver,
}

impl Automator {
    pub fn new(config: &Config) -> Result<Self> {
        // Initialize protocol driver if we have accounts, or try default
        // For now, create a dummy account if typically empty, or rely on internal logic
        // As ProtocolDriver::new ignores account for now anyway:

        let dummy_account = common::config::Account {
            provider: "google".to_string(),
            credentials: std::collections::HashMap::new(),
        };

        let protocol = match ProtocolDriver::new(&dummy_account, config.server.browser_profile_path.as_deref()) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::error!("Failed to initialize protocol driver: {}", e);
                None
            }
        };

        Ok(Self {
            protocol,
            visual: VisualDriver::new()?,
        })
    }

    pub fn visual(&mut self) -> &mut VisualDriver {
        &mut self.visual
    }
}
