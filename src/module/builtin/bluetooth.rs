//! Bluetooth quick toggle via `bluetoothctl` (power on/off). Split pill: tap to
//! toggle, chevron expands an inline device picker.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, ListEntry, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;

pub struct BluetoothModule {
    desc: ModuleDescriptor,
    on: bool,
    available: bool,
    connected: usize,
    /// Name of the single connected device (shown when exactly one is connected).
    name: Option<String>,
    /// Lowest battery percentage across connected devices that report one.
    battery: Option<u8>,
    /// Paired devices for the inline selection list, populated when expanded.
    entries: Vec<ListEntry>,
    /// Whether the drawer is open (so the device list is only scanned then).
    want_entries: bool,
}

/// A snapshot fetched off the UI thread.
#[derive(Default)]
struct BtData {
    on: bool,
    available: bool,
    connected: usize,
    name: Option<String>,
    battery: Option<u8>,
    entries: Vec<ListEntry>,
}

/// Gather Bluetooth state off the UI thread. `want_entries` adds the paired
/// device list. The per-device battery query (`bluetoothctl info`) is the slow
/// part this keeps off the UI thread.
fn fetch(want_entries: bool) -> BtData {
    let mut d = BtData::default();
    if let Some(o) = super::out("bluetoothctl show") {
        d.available = !o.is_empty();
        d.on = o.lines().any(|l| l.trim() == "Powered: yes");
    }
    let conn = devices("bluetoothctl devices Connected");
    d.connected = conn.len();
    d.name = (conn.len() == 1)
        .then(|| conn[0].1.clone())
        .filter(|n| !n.is_empty());
    // Lowest battery across connected devices that report one (org.bluez.Battery1,
    // surfaced by `bluetoothctl info` as "Battery Percentage: 0xNN (NN)").
    let mut lowest: Option<u8> = None;
    for (mac, _) in &conn {
        if let Some(info) = super::out(&format!("bluetoothctl info {mac}")) {
            for line in info.lines() {
                if let Some(pct) = line
                    .trim()
                    .strip_prefix("Battery Percentage:")
                    .and_then(|v| v.split_once('('))
                    .and_then(|(_, r)| r.split_once(')'))
                    .and_then(|(n, _)| n.trim().parse::<u8>().ok())
                {
                    lowest = Some(lowest.map_or(pct, |c| c.min(pct)));
                }
            }
        }
    }
    d.battery = lowest;
    if want_entries {
        d.entries = scan_devices();
    }
    d
}

/// The Bluetooth icon for a power/connection state. Shared by the tile and the
/// panel status cluster (all names are in the Cosmic icon theme).
pub fn state_icon(on: bool, connected: usize) -> &'static str {
    if !on {
        "bluetooth-disabled-symbolic"
    } else if connected > 0 {
        "bluetooth-active-symbolic"
    } else {
        "bluetooth-symbolic"
    }
}

/// Parse a `bluetoothctl devices ...` listing into (mac, name) pairs.
fn devices(cmd: &str) -> Vec<(String, String)> {
    super::out(cmd)
        .map(|o| {
            o.lines()
                .filter_map(|l| {
                    let rest = l.strip_prefix("Device ")?;
                    let mut it = rest.splitn(2, ' ');
                    let mac = it.next()?.to_string();
                    Some((mac, it.next().unwrap_or("").trim().to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn connected_macs() -> Vec<String> {
    devices("bluetoothctl devices Connected")
        .into_iter()
        .map(|(m, _)| m)
        .collect()
}

/// Paired devices, marking which are currently connected.
fn scan_devices() -> Vec<ListEntry> {
    let connected = connected_macs();
    devices("bluetoothctl devices Paired")
        .into_iter()
        .map(|(mac, name)| {
            let active = connected.contains(&mac);
            ListEntry {
                label: if name.is_empty() { mac.clone() } else { name },
                detail: if active { "Connected".into() } else { String::new() },
                active,
                key: mac,
            }
        })
        .collect()
}

impl BluetoothModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.bluetooth".into(),
                name: "Bluetooth".into(),
                icon: "bluetooth-symbolic".into(),
                size: TileSize::Medium,
                resizable: true,
            },
            on: false,
            available: false,
            connected: 0,
            name: None,
            battery: None,
            entries: Vec::new(),
            want_entries: false,
        };
        m.set(fetch(false));
        m
    }

    fn set(&mut self, d: BtData) {
        self.on = d.on;
        self.available = d.available;
        self.connected = d.connected;
        self.name = d.name;
        self.battery = d.battery;
        if self.want_entries {
            self.entries = d.entries;
        }
    }
}

impl Module for BluetoothModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn status_icon(&self) -> String {
        state_icon(self.on, self.connected).to_string()
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        // Base label: the device name when one is connected, else a count.
        let status = if !self.on {
            "Off".to_string()
        } else if self.connected == 0 {
            "On".to_string()
        } else {
            let base = match (self.connected, &self.name) {
                (1, Some(n)) => n.clone(),
                (1, None) => "1 device".to_string(),
                (n, _) => format!("{n} devices"),
            };
            match self.battery {
                Some(b) => format!("{base} · {b}%"),
                None => base,
            }
        };
        let icon = self.status_icon();
        super::toggle_tile(
            id,
            width,
            self.on,
            edit,
            &icon,
            "Bluetooth",
            &status,
            super::Chevron::Expand,
        )
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match control {
            "on" => {
                if let ControlValue::Bool(b) = value {
                    self.on = b;
                    if self.available {
                        super::run(&format!(
                            "bluetoothctl power {}",
                            if b { "on" } else { "off" }
                        ));
                    }
                }
            }
            "settings" => super::run("cosmic-settings bluetooth"),
            // Drawer open/close: flag whether the device list is fetched.
            "expand" => {
                // Keep the cached list while the drawer animates closed (the flag
                // stops it refreshing) so an empty state doesn't flash mid-close.
                if let ControlValue::Bool(b) = value {
                    self.want_entries = b;
                }
            }
            "select" => {
                if let ControlValue::Text(mac) = value {
                    let action = if connected_macs().contains(&mac) {
                        "disconnect"
                    } else {
                        "connect"
                    };
                    super::run(&format!("bluetoothctl {action} {mac}"));
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
        if let Some(d) = data.downcast_ref::<BtData>() {
            self.set(BtData {
                on: d.on,
                available: d.available,
                connected: d.connected,
                name: d.name.clone(),
                battery: d.battery,
                entries: d.entries.clone(),
            });
        }
    }
}
