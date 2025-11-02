use cosmic::cosmic_config::cosmic_config_derive::CosmicConfigEntry;
use cosmic::cosmic_config::CosmicConfigEntry;
use serde::{Deserialize, Serialize};

pub const APP_LIST_ID: &str = "com.system76.CosmicAppList";

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub enum TopLevelFilter {
    #[default]
    ActiveWorkspace,
    ConfiguredOutput,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
#[serde(deny_unknown_fields)]
pub struct AppListConfig {
    pub filter_top_levels: Option<TopLevelFilter>,
    pub favorites: Vec<String>,
    pub enable_drag_source: bool,
}

impl Default for AppListConfig {
    fn default() -> Self {
        Self {
            filter_top_levels: None,
            favorites: Vec::new(),
            enable_drag_source: true,
        }
    }
}
