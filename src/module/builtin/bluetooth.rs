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
    /// Paired devices for the inline selection list, populated on expand.
    entries: Vec<ListEntry>,
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
        };
        m.read();
        m
    }

    fn read(&mut self) {
        if let Some(o) = super::out("bluetoothctl show") {
            self.available = !o.is_empty();
            self.on = o.lines().any(|l| l.trim() == "Powered: yes");
        }
        // Each `devices Connected` line is "Device <MAC> <Name>".
        let (mut macs, mut names) = (Vec::new(), Vec::new());
        if let Some(o) = super::out("bluetoothctl devices Connected") {
            for l in o.lines() {
                if let Some(rest) = l.strip_prefix("Device ") {
                    let mut it = rest.splitn(2, ' ');
                    if let Some(mac) = it.next() {
                        macs.push(mac.to_string());
                        names.push(it.next().unwrap_or("").trim().to_string());
                    }
                }
            }
        }
        self.connected = macs.len();
        self.name = (names.len() == 1).then(|| names.remove(0)).filter(|n| !n.is_empty());
        // Lowest battery across devices that report one (org.bluez.Battery1,
        // surfaced by `bluetoothctl info` as "Battery Percentage: 0xNN (NN)").
        let mut lowest: Option<u8> = None;
        for mac in &macs {
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
        self.battery = lowest;
    }
}

impl Module for BluetoothModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
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
        super::toggle_tile(
            id,
            width,
            self.on,
            edit,
            self.desc.icon.as_str(),
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
            // Inline picker: list paired devices on expand, toggle connect on select.
            "expand" => self.entries = scan_devices(),
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

    fn refresh(&mut self) -> Task<Message> {
        self.read();
        Task::none()
    }
}
