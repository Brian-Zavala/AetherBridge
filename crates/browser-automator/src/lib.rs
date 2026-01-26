pub mod google_driver;
pub mod protocol_driver;
pub mod visual_driver;

use anyhow::Result;
use common::config::Config;
use protocol_driver::ProtocolDriver;
use visual_driver::VisualDriver;

pub struct Automator {
    pub protocol: Option<ProtocolDriver>,
    pub visual: VisualDriver,
}

impl Automator {
    pub fn new(_config: &Config) -> Result<Self> {
        Ok(Self {
            protocol: None,
            visual: VisualDriver::new()?,
        })
    }

    pub fn visual(&mut self) -> &mut VisualDriver {
        &mut self.visual
    }
}
