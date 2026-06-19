//! Persistent layout, stored via cosmic-config (RON), mirroring the
//! cosmic-toys pattern. The `instances` Vec order == on-screen tile order.

use crate::module::TileSize;
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const APP_ID: &str = "com.pyxyll.CosmicExtControlCenter";

/// How the panel applet's button presents itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AppletIcons {
    /// A single control-center icon — the original behaviour.
    #[default]
    Single,
    /// A cluster of live status icons (Wi-Fi/VPN, audio, Bluetooth, …).
    Status,
}

/// One placed tile: which module type, its instance id, and its size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub id: u32,
    /// Module type id — a built-in id ("builtin.toggle") or a plugin id.
    pub module: String,
    pub size: TileSize,
    /// Per-instance settings (e.g. {"mount": "/home"} for a disk gauge).
    /// `default` keeps configs written before this field was added loadable.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    /// Placed tiles, in display order.
    pub instances: Vec<InstanceConfig>,
    /// Monotonic counter for assigning new instance ids.
    pub next_id: u32,
    /// App-wide settings, distinct from the per-tile layout above. A separate
    /// cosmic-config key, so it defaults cleanly for configs written before it.
    pub settings: Settings,
}

/// App-wide preferences (the editor's Settings surface). Grouped in one struct
/// so future global options land here without touching the layout fields.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Single control-center icon vs. a live status cluster in the panel.
    #[serde(default)]
    pub applet_icons: AppletIcons,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            instances: Vec::new(),
            next_id: 1,
            settings: Settings::default(),
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
