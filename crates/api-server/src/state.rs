use std::sync::Arc;
use tokio::sync::Mutex;
use common::config::Config;
use browser_automator::Automator;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub automator: Arc<Mutex<Automator>>,
}

impl AppState {
    pub fn new(config: Config, automator: Automator) -> Self {
        Self {
            config: Arc::new(config),
            automator: Arc::new(Mutex::new(automator)),
        }
    }
}
