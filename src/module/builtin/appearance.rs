//! Light / Dark appearance switcher. Flips COSMIC's system theme via the
//! `com.system76.CosmicTheme.Mode` config (`is_dark`) — the exact key COSMIC
//! Settings writes, so the whole desktop re-themes (and our own window follows).
//!
//! ON/OFF is genuinely ambiguous for a theme (some users default light, some
//! dark), so we frame it as a **Dark Mode** toggle: ON = dark, OFF = light —
//! the conventional mapping. The icon also tracks the live state (moon when
//! dark, sun when light) so it reads correctly whichever way the user leans.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::cosmic_config::ConfigSet;
use cosmic::cosmic_theme::ThemeMode;
use cosmic::prelude::*;

pub struct AppearanceModule {
    desc: ModuleDescriptor,
    dark: bool,
}

impl AppearanceModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.appearance".into(),
                name: "Appearance".into(),
                icon: "weather-clear-night-symbolic".into(),
                size: TileSize::Medium,
                resizable: true,
            },
            dark: true,
        };
        m.read();
        m
    }

    fn read(&mut self) {
        if let Ok(config) = ThemeMode::config() {
            if let Ok(d) = ThemeMode::is_dark(&config) {
                self.dark = d;
            }
        }
    }
}

impl Module for AppearanceModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        let icon = if self.dark {
            "weather-clear-night-symbolic"
        } else {
            "weather-clear-symbolic"
        };
        let status = if self.dark { "On" } else { "Off" };
        super::toggle_tile(id, width, self.dark, edit, icon, "Dark Mode", status, super::Chevron::Settings)
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match control {
            "on" => {
                if let ControlValue::Bool(b) = value {
                    self.dark = b;
                    if let Ok(config) = ThemeMode::config() {
                        // Write is_dark and pin auto_switch off so the manual
                        // choice holds (otherwise time-based switching fights it).
                        let _ = config.set("is_dark", b);
                        let _ = config.set("auto_switch", false);
                    }
                }
            }
            "settings" => super::run("cosmic-settings appearance"),
            _ => {}
        }
        Task::none()
    }

    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
        self.read();
        Task::none()
    }
}
