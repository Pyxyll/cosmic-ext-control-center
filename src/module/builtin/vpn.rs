//! VPN quick toggle via NetworkManager. Rendered as a split pill (same style as
//! Wi-Fi): tap to bring the VPN connection up/down, chevron expands an inline
//! profile picker. Works with any NM-managed VPN (OpenVPN, WireGuard, etc.).

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, ListEntry, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;

pub struct VpnModule {
    desc: ModuleDescriptor,
    /// A VPN connection is currently active.
    on: bool,
    /// Connection to act on: the active VPN when on, else the first configured
    /// one. `None` when no NM VPN exists.
    name: Option<String>,
    /// Profiles for the inline selection list, populated on expand.
    entries: Vec<ListEntry>,
}

/// Truncate a long label to fit a tile, with an ellipsis.
fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    } else {
        s.to_string()
    }
}

/// All VPN/WireGuard connection names in an `nmcli -t -f NAME,TYPE` listing. The
/// type is the last `:`-field, so split from the right; unescape nmcli's `\:` /
/// `\\` so the real name is usable in a follow-up `nmcli` call.
fn vpn_names(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|l| {
            let (name, typ) = l.rsplit_once(':')?;
            (matches!(typ, "vpn" | "wireguard") && !name.is_empty())
                .then(|| name.replace("\\:", ":").replace("\\\\", "\\"))
        })
        .collect()
}

fn first_vpn(listing: &str) -> Option<String> {
    vpn_names(listing).into_iter().next()
}

/// Names of the currently-active VPN connections.
fn active_vpns() -> Vec<String> {
    super::out("nmcli -t -f NAME,TYPE connection show --active")
        .map(|o| vpn_names(&o))
        .unwrap_or_default()
}

/// All configured VPN profiles, marking which are active.
fn scan_profiles() -> Vec<ListEntry> {
    let all = super::out("nmcli -t -f NAME,TYPE connection show")
        .map(|o| vpn_names(&o))
        .unwrap_or_default();
    let active = active_vpns();
    all.into_iter()
        .map(|name| {
            let is_active = active.contains(&name);
            ListEntry {
                key: name.clone(),
                label: name,
                detail: if is_active { "Connected".into() } else { String::new() },
                active: is_active,
            }
        })
        .collect()
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
            entries: Vec::new(),
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
        super::toggle_tile(id, width, self.on, edit, self.desc.icon.as_str(), &label, &status, super::Chevron::Expand)
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
            // Inline picker: list profiles on expand, toggle the chosen one on select.
            "expand" => self.entries = scan_profiles(),
            "select" => {
                if let ControlValue::Text(name) = value {
                    let action = if active_vpns().contains(&name) { "down" } else { "up" };
                    let esc = name.replace('\'', "'\\''");
                    super::run(&format!("nmcli connection {action} '{esc}'"));
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn expandable(&self) -> bool {
        true
    }

    fn entries(&self) -> Vec<ListEntry> {
        self.entries.clone()
    }

    fn refresh(&mut self) -> Task<Message> {
        self.read();
        Task::none()
    }
}
