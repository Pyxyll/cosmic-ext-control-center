//! A REAL built-in: system volume via `wpctl` (PipeWire/WirePlumber, always
//! present on COSMIC). Proves a built-in can drive live system state, not just
//! hold demo values.
//!
//! Phase-1 simplification: reads/writes synchronously via `wpctl` (fast, a few
//! ms). A later pass moves this to async polling so the UI never blocks.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;
use std::process::Command;

const SINK: &str = "@DEFAULT_AUDIO_SINK@";

pub struct VolumeModule {
    desc: ModuleDescriptor,
    value: f32,
    muted: bool,
    available: bool,
}

impl VolumeModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.volume".into(),
                name: "Volume".into(),
                icon: "audio-volume-high-symbolic".into(),
                size: TileSize::Full,
                resizable: true,
            },
            value: 0.5,
            muted: false,
            available: false,
        };
        m.read();
        m
    }

    /// Parse `wpctl get-volume @DEFAULT_AUDIO_SINK@` → "Volume: 0.45 [MUTED]".
    fn read(&mut self) {
        if let Ok(out) = Command::new("wpctl").args(["get-volume", SINK]).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                if let Some(num) = s.split_whitespace().nth(1) {
                    if let Ok(v) = num.parse::<f32>() {
                        self.value = v.clamp(0.0, 1.5);
                        self.muted = s.contains("[MUTED]");
                        self.available = true;
                    }
                }
            }
        }
    }

    fn write(&self, v: f32) {
        let _ = Command::new("wpctl")
            .args(["set-volume", SINK, &format!("{v:.2}")])
            .status();
    }
}

impl Module for VolumeModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        let pct = if !self.available {
            "n/a".to_string()
        } else if self.muted {
            "Muted".to_string()
        } else {
            format!("{:.0}%", self.value * 100.0)
        };
        let icon = if self.muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        };
        // Volume can exceed 100%; cap the displayed bar at 1.0.
        super::slider_tile(
            id,
            width,
            self.value.min(1.0),
            icon,
            pct,
            "value",
            Some(("mute", self.muted)),
            edit,
        )
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match (control, value) {
            ("value", ControlValue::Float(v)) => {
                self.value = v as f32;
                if self.available {
                    self.write(self.value);
                }
            }
            ("mute", ControlValue::Bool(b)) => {
                self.muted = b;
                if self.available {
                    let _ = Command::new("wpctl")
                        .args(["set-mute", SINK, if b { "1" } else { "0" }])
                        .status();
                }
            }
            _ => {}
        }
        Task::none()
    }
}
