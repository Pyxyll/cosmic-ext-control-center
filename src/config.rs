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
    /// Which indicators appear in the status cluster (independent of the tile
    /// layout — the popup still exposes everything).
    #[serde(default)]
    pub cluster: ClusterIcons,
}

/// Per-indicator visibility for the panel status cluster. All on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterIcons {
    #[serde(default = "yes")]
    pub power: bool,
    #[serde(default = "yes")]
    pub network: bool,
    #[serde(default = "yes")]
    pub audio: bool,
    #[serde(default = "yes")]
    pub bluetooth: bool,
    #[serde(default = "yes")]
    pub power_profile: bool,
}

fn yes() -> bool {
    true
}

impl Default for ClusterIcons {
    fn default() -> Self {
        Self {
            power: true,
            network: true,
            audio: true,
            bluetooth: true,
            power_profile: true,
        }
    }
}

/// One configurable cluster indicator, for the Settings toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterIcon {
    Power,
    Network,
    Audio,
    Bluetooth,
    PowerProfile,
}

impl ClusterIcon {
    /// In display order, with their settings labels.
    pub const ALL: [(ClusterIcon, &'static str); 5] = [
        (ClusterIcon::Power, "Power"),
        (ClusterIcon::Network, "Wi-Fi / VPN"),
        (ClusterIcon::Audio, "Audio"),
        (ClusterIcon::Bluetooth, "Bluetooth"),
        (ClusterIcon::PowerProfile, "Power profile"),
    ];
}

impl ClusterIcons {
    pub fn enabled(&self, i: ClusterIcon) -> bool {
        match i {
            ClusterIcon::Power => self.power,
            ClusterIcon::Network => self.network,
            ClusterIcon::Audio => self.audio,
            ClusterIcon::Bluetooth => self.bluetooth,
            ClusterIcon::PowerProfile => self.power_profile,
        }
    }

    pub fn toggle(&mut self, i: ClusterIcon) {
        let slot = match i {
            ClusterIcon::Power => &mut self.power,
            ClusterIcon::Network => &mut self.network,
            ClusterIcon::Audio => &mut self.audio,
            ClusterIcon::Bluetooth => &mut self.bluetooth,
            ClusterIcon::PowerProfile => &mut self.power_profile,
        };
        *slot = !*slot;
    }
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
