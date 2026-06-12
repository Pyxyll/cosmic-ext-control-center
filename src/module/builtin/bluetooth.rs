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
        };
        m.read();
        m
    }

    fn read(&mut self) {
        if let Some(o) = super::out("bluetoothctl show") {
            self.available = !o.is_empty();
            self.on = o.lines().any(|l| l.trim() == "Powered: yes");
        }
        // Count connected devices (one per line of `devices Connected`).
        self.connected = super::out("bluetoothctl devices Connected")
            .map(|o| o.lines().filter(|l| l.starts_with("Device ")).count())
            .unwrap_or(0);
    }
}

impl Module for BluetoothModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        let status = if !self.on {
            "Off".to_string()
        } else if self.connected == 1 {
            "1 device".to_string()
        } else if self.connected > 1 {
            format!("{} devices", self.connected)
        } else {
            "On".to_string()
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
