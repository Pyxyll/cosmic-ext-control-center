//! Bluetooth quick toggle via `bluetoothctl` (power on/off). Split pill: tap to
//! toggle, chevron opens COSMIC bluetooth settings.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
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
            true,
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
            _ => {}
        }
        Task::none()
    }

    fn refresh(&mut self) -> Task<Message> {
        self.read();
        Task::none()
    }
}
