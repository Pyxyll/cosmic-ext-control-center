//! Airplane mode — blocks/unblocks every radio (Wi-Fi, Bluetooth, WWAN) via
//! `rfkill`, the canonical mechanism COSMIC's own toggle uses. Reading state is
//! always permitted; toggling works for the active session (systemd grants
//! `/dev/rfkill` access via `uaccess`).

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;

pub struct AirplaneModule {
    desc: ModuleDescriptor,
    /// All radios are soft-blocked.
    on: bool,
    /// At least one rfkill device exists.
    available: bool,
}

impl AirplaneModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.airplane".into(),
                name: "Airplane Mode".into(),
                icon: "airplane-mode-symbolic".into(),
                size: TileSize::Medium,
                resizable: true,
            },
            on: false,
            available: false,
        };
        m.read();
        m
    }

    fn read(&mut self) {
        if let Some(o) = super::out("rfkill list") {
            // One "Soft blocked: yes/no" line per radio; airplane is on when
            // every radio is soft-blocked.
            let softs: Vec<bool> = o
                .lines()
                .filter_map(|l| {
                    l.trim()
                        .strip_prefix("Soft blocked:")
                        .map(|v| v.trim() == "yes")
                })
                .collect();
            self.available = !softs.is_empty();
            self.on = self.available && softs.iter().all(|&b| b);
        }
    }
}

impl Module for AirplaneModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        let status = if self.on { "On" } else { "Off" };
        super::toggle_tile(
            id,
            width,
            self.on,
            edit,
            self.desc.icon.as_str(),
            "Airplane Mode",
            status,
            super::Chevron::None,
        )
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        if control == "on" {
            if let ControlValue::Bool(b) = value {
                self.on = b; // optimistic; corrected on next poll
                super::run(if b { "rfkill block all" } else { "rfkill unblock all" });
            }
        }
        Task::none()
    }

    fn refresh(&mut self) -> Task<Message> {
        self.read();
        Task::none()
    }
}
