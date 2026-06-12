//! Wi-Fi quick toggle via NetworkManager (`nmcli radio wifi`). Rendered as a
//! split pill: tap to toggle, chevron opens COSMIC network settings.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;

pub struct WifiModule {
    desc: ModuleDescriptor,
    on: bool,
    available: bool,
    ssid: Option<String>,
    /// Negotiated link rate in Mb/s for the active connection, if any.
    rate: Option<u32>,
}

/// Parse nmcli's rate field ("540 Mbit/s") into Mb/s, dropping a zero rate.
fn parse_rate(s: &str) -> Option<u32> {
    s.split_whitespace()
        .next()?
        .parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
}

/// Truncate a long label to fit a tile, with an ellipsis.
fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    } else {
        s.to_string()
    }
}

impl WifiModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.wifi".into(),
                name: "Wi-Fi".into(),
                icon: "network-wireless-symbolic".into(),
                size: TileSize::Medium,
                resizable: true,
            },
            on: false,
            available: false,
            ssid: None,
            rate: None,
        };
        m.read();
        m
    }

    fn read(&mut self) {
        if let Some(o) = super::out("nmcli radio wifi") {
            self.on = o.trim() == "enabled";
            self.available = true;
        }
        // Active SSID, if connected: the "yes:<ssid>" line.
        self.ssid = super::out("nmcli -t -f active,ssid dev wifi").and_then(|o| {
            o.lines()
                .find_map(|l| l.strip_prefix("yes:").map(str::to_string))
                .filter(|s| !s.is_empty())
        });
        // Link rate of the active AP (queried separately from the SSID so an
        // SSID containing ':' can't break field parsing).
        self.rate = super::out("nmcli -t -f active,rate dev wifi").and_then(|o| {
            o.lines().find_map(|l| l.strip_prefix("yes:").and_then(parse_rate))
        });
    }
}

impl Module for WifiModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        // Primary line is the SSID when connected (the "Wi-Fi" name is dropped —
        // the icon already identifies it); falls back to "Wi-Fi" otherwise.
        let connected = self.on && self.ssid.is_some();
        let label = match &self.ssid {
            Some(ssid) if self.on => ellipsize(ssid, 18),
            _ => "Wi-Fi".to_string(),
        };
        // Secondary line: link rate when connected, else the connection state.
        let status = if !self.on {
            "Off".to_string()
        } else if connected {
            match self.rate {
                Some(r) => format!("{r} Mb/s"),
                None => "Connected".to_string(),
            }
        } else {
            "Not connected".to_string()
        };
        super::toggle_tile(id, width, self.on, edit, self.desc.icon.as_str(), &label, &status, true)
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match control {
            "on" => {
                if let ControlValue::Bool(b) = value {
                    self.on = b;
                    if self.available {
                        super::run(&format!("nmcli radio wifi {}", if b { "on" } else { "off" }));
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
