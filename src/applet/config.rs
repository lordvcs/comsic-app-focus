use cosmic_config::{cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
#[serde(deny_unknown_fields)]
pub struct AppletConfig {
    #[serde(default)]
    pub pinned: Vec<String>,
}

impl Default for AppletConfig {
    fn default() -> Self {
        Self { pinned: Vec::new() }
    }
}
