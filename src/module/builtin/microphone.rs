//! Microphone: input level slider + mute toggle, via `wpctl` on the default
//! audio source.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;

const SRC: &str = "@DEFAULT_AUDIO_SOURCE@";

pub struct MicrophoneModule {
    desc: ModuleDescriptor,
    level: f32,
    muted: bool,
    available: bool,
}

impl MicrophoneModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.microphone".into(),
                name: "Microphone".into(),
                icon: "audio-input-microphone-symbolic".into(),
                size: TileSize::Full,
                resizable: true,
            },
            level: 0.5,
            muted: false,
            available: false,
        };
        m.read();
        m
    }

    fn read(&mut self) {
        // "Volume: 0.40 [MUTED]"
        if let Some(o) = super::out(&format!("wpctl get-volume {SRC}")) {
            if let Some(num) = o.split_whitespace().nth(1) {
                if let Ok(v) = num.parse::<f32>() {
                    self.level = v.clamp(0.0, 1.5);
                    self.muted = o.contains("[MUTED]");
                    self.available = true;
                }
            }
        }
    }
}

impl Module for MicrophoneModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        let pct = if self.available {
            format!("{:.0}%", self.level * 100.0)
        } else {
            "n/a".to_string()
        };
        let icon = if self.muted {
            "microphone-sensitivity-muted-symbolic"
        } else {
            "audio-input-microphone-symbolic"
        };
        super::slider_tile(
            id,
            width,
            self.level.min(1.0),
            icon,
            pct,
            "level",
            Some(("mute", self.muted)),
            edit,
        )
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match (control, value) {
            ("level", ControlValue::Float(v)) => {
                self.level = v as f32;
                if self.available {
                    super::run(&format!("wpctl set-volume {SRC} {:.2}", self.level));
                }
            }
            ("mute", ControlValue::Bool(b)) => {
                self.muted = b;
                if self.available {
                    super::run(&format!("wpctl set-mute {SRC} {}", if b { "1" } else { "0" }));
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn refresh(&mut self) -> Task<Message> {
        self.read();
        Task::none()
    }
}
