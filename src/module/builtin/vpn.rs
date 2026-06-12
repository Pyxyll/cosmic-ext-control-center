//! VPN quick toggle via NetworkManager. Rendered as a split pill (same style as
//! Wi-Fi): tap to bring the VPN connection up/down, chevron opens COSMIC network
//! settings. Works with any NM-managed VPN (OpenVPN, WireGuard, etc.).

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;

pub struct VpnModule {
    desc: ModuleDescriptor,
    /// A VPN connection is currently active.
    on: bool,
    /// Connection to act on: the active VPN when on, else the first configured
    /// one. `None` when no NM VPN exists.
    name: Option<String>,
}

/// Truncate a long label to fit a tile, with an ellipsis.
fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    } else {
        s.to_string()
    }
}

/// First VPN/WireGuard connection name in an `nmcli -t -f NAME,TYPE` listing.
/// The type is the last `:`-field, so split from the right; unescape nmcli's
/// `\:` / `\\` so the real name is usable in a follow-up `nmcli` call.
fn first_vpn(listing: &str) -> Option<String> {
    listing.lines().find_map(|l| {
        let (name, typ) = l.rsplit_once(':')?;
        if matches!(typ, "vpn" | "wireguard") && !name.is_empty() {
            Some(name.replace("\\:", ":").replace("\\\\", "\\"))
        } else {
            None
        }
    })
}

impl VpnModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.vpn".into(),
                name: "VPN".into(),
                icon: "network-vpn-symbolic".into(),
                size: TileSize::Medium,
                resizable: true,
            },
            on: false,
            name: None,
        };
        m.read();
        m
    }

    fn read(&mut self) {
        let active = super::out("nmcli -t -f NAME,TYPE connection show --active")
            .and_then(|o| first_vpn(&o));
        let configured =
            super::out("nmcli -t -f NAME,TYPE connection show").and_then(|o| first_vpn(&o));
        self.on = active.is_some();
        // Toggle target: the active VPN when connected, else the first set up.
        self.name = active.or(configured);
    }
}

impl Module for VpnModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        // Primary line = the connection name when connected (mirrors Wi-Fi's
        // SSID); falls back to "VPN".
        let label = match &self.name {
            Some(n) if self.on => ellipsize(n, 18),
            _ => "VPN".to_string(),
        };
        let status = if self.name.is_none() {
            "Not set up".to_string()
        } else if self.on {
            "Connected".to_string()
        } else {
            "Off".to_string()
        };
        super::toggle_tile(id, width, self.on, edit, self.desc.icon.as_str(), &label, &status, true)
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match control {
            "on" => {
                if let ControlValue::Bool(b) = value {
                    self.on = b; // optimistic; corrected on next poll
                    if let Some(name) = &self.name {
                        let action = if b { "up" } else { "down" };
                        super::run(&format!("nmcli connection {action} '{name}'"));
                    }
                }
            }
            "settings" => super::run("cosmic-settings network"),
            _ => {}
        }
        Task::none()
    }

    fn refresh(&mut self) -> Task<Message> {
        self.read();
        Task::none()
    }
}
