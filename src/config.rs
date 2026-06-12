//! Persistent layout, stored via cosmic-config (RON), mirroring the
//! cosmic-toys pattern. The `instances` Vec order == on-screen tile order.

use crate::module::TileSize;
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "com.pyxyll.CosmicControlCenter";

/// One placed tile: which module type, its instance id, and its size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub id: u32,
    /// Module type id — a built-in id ("builtin.toggle") or a plugin id.
    pub module: String,
    pub size: TileSize,
}

#[derive(Debug, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    /// Placed tiles, in display order.
    pub instances: Vec<InstanceConfig>,
    /// Monotonic counter for assigning new instance ids.
    pub next_id: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            instances: Vec::new(),
            next_id: 1,
        }
    }
}

impl Config {
    /// Load from cosmic-config, falling back to defaults.
    pub fn load() -> Self {
        cosmic_config::Config::new(APP_ID, Config::VERSION)
            .map(|ctx| match Config::get_entry(&ctx) {
                Ok(c) => c,
                Err((_e, c)) => c,
            })
            .unwrap_or_default()
    }

    /// Persist the whole config.
    pub fn save(&self) {
        if let Ok(ctx) = cosmic_config::Config::new(APP_ID, Config::VERSION) {
            let _ = self.write_entry(&ctx);
        }
    }
}
