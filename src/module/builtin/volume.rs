//! A REAL built-in: system volume via `wpctl` (PipeWire/WirePlumber, always
//! present on COSMIC). Proves a built-in can drive live system state, not just
//! hold demo values. Reads happen off the UI thread (async refresh); writes are
//! fire-and-forget, so neither blocks the popup.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;
use std::process::Command;

const SINK: &str = "@DEFAULT_AUDIO_SINK@";

/// Speaker icon by mute state and level (Cosmic theme has the full set). Shared
/// by the volume tile and the panel status cluster.
pub fn volume_icon(muted: bool, level: f32) -> &'static str {
    if muted || level <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if level < 0.34 {
        "audio-volume-low-symbolic"
    } else if level < 0.67 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

pub struct VolumeModule {
    desc: ModuleDescriptor,
    value: f32,
    muted: bool,
    available: bool,
}

#[derive(Default)]
struct VolData {
    value: f32,
    muted: bool,
    available: bool,
}

/// Parse `wpctl get-volume @DEFAULT_AUDIO_SINK@` → "Volume: 0.45 [MUTED]".
fn fetch() -> VolData {
    let mut d = VolData::default();
    if let Ok(out) = Command::new("wpctl").args(["get-volume", SINK]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(num) = s.split_whitespace().nth(1) {
                if let Ok(v) = num.parse::<f32>() {
                    d.value = v.clamp(0.0, 1.5);
                    d.muted = s.contains("[MUTED]");
                    d.available = true;
                }
            }
        }
    }
    d
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
        m.set(fetch());
        m
    }

    fn set(&mut self, d: VolData) {
        // Keep the default until wpctl actually answers.
        if d.available {
            self.value = d.value;
            self.muted = d.muted;
            self.available = true;
        }
    }

    fn write(&self, v: f32) {
        let _ = Command::new("wpctl")
            .args(["set-volume", SINK, &format!("{v:.2}")])
            .spawn();
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
        let icon = volume_icon(self.muted, self.value);
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
                        .spawn();
                }
            }
            _ => {}
        }
        Task::none()
    }
}
