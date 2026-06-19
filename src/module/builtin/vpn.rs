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
    /// Profiles for the inline selection list, populated when expanded.
    entries: Vec<ListEntry>,
    /// Whether the drawer is open (so the profile list is only scanned then).
    want_entries: bool,
}

/// A snapshot fetched off the UI thread.
#[derive(Default)]
struct VpnData {
    on: bool,
    name: Option<String>,
    entries: Vec<ListEntry>,
}

/// The VPN icon for an active/inactive state. Shared by the tile and the panel
/// status cluster (the `network-vpn-*` names come from the Pop theme Cosmic
/// inherits).
pub fn state_icon(on: bool) -> &'static str {
    if on {
        "network-vpn-symbolic"
    } else {
        "network-vpn-disconnected-symbolic"
    }
}

/// Gather VPN state off the UI thread. `want_entries` adds the profile list.
fn fetch(want_entries: bool) -> VpnData {
    let active =
        super::out("nmcli -t -f NAME,TYPE connection show --active").and_then(|o| first_vpn(&o));
    let configured =
        super::out("nmcli -t -f NAME,TYPE connection show").and_then(|o| first_vpn(&o));
    VpnData {
        on: active.is_some(),
        // Toggle target: the active VPN when connected, else the first set up.
        name: active.or(configured),
        entries: if want_entries { scan_profiles() } else { Vec::new() },
    }
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
            want_entries: false,
        };
        m.set(fetch(false));
        m
    }

    fn set(&mut self, d: VpnData) {
        self.on = d.on;
        self.name = d.name;
        if self.want_entries {
            self.entries = d.entries;
        }
    }
}

impl Module for VpnModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn status_icon(&self) -> String {
        state_icon(self.on).to_string()
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
        let icon = self.status_icon();
        super::toggle_tile(id, width, self.on, edit, &icon, &label, &status, super::Chevron::Expand)
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
            // Drawer open/close: flag whether the profile list is fetched.
            "expand" => {
                // Keep the cached list while the drawer animates closed (the flag
                // stops it refreshing) so an empty state doesn't flash mid-close.
                if let ControlValue::Bool(b) = value {
                    self.want_entries = b;
                }
            }
            "select" => {
                if let ControlValue::Text(name) = value {
                    let esc = |s: &str| s.replace('\'', "'\\''");
                    let active = active_vpns();
                    if active.contains(&name) {
                        // Tapping the active profile disconnects it.
                        super::run(&format!("nmcli connection down '{}'", esc(&name)));
                    } else {
                        // One VPN at a time: bring down any other active VPN
                        // first, then up the selected one — chained in a single
                        // shell so they run in order (fire-and-forget can't).
                        let mut cmd = String::new();
                        for other in active.iter().filter(|o| *o != &name) {
                            cmd.push_str(&format!("nmcli connection down '{}'; ", esc(other)));
                        }
                        cmd.push_str(&format!("nmcli connection up '{}'", esc(&name)));
                        super::run(&cmd);
                    }
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

    fn fetch_job(&self) -> Option<Box<dyn FnOnce() -> crate::module::Payload + Send>> {
        let want_entries = self.want_entries;
        Some(Box::new(move || crate::module::Payload::new(fetch(want_entries))))
    }

    fn apply_data(&mut self, data: &dyn std::any::Any) {
        if let Some(d) = data.downcast_ref::<VpnData>() {
            self.set(VpnData {
                on: d.on,
                name: d.name.clone(),
                entries: d.entries.clone(),
            });
        }
    }

}
