//! A standalone Do Not Disturb toggle, for users who want the silence switch
//! without the full notification center. Reads/writes the same COSMIC
//! cosmic-config key the notification daemon honours.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use crate::notifications::{read_dnd, write_dnd};
use cosmic::app::Task;
use cosmic::prelude::*;

pub struct DndModule {
    desc: ModuleDescriptor,
    on: bool,
}

impl DndModule {
    pub fn new() -> Self {
        Self {
            desc: ModuleDescriptor {
                id: "builtin.do_not_disturb".into(),
                name: "Do Not Disturb".into(),
                icon: "notification-disabled-symbolic".into(),
                size: TileSize::Medium,
                resizable: true,
            },
            on: read_dnd(),
        }
    }
}

impl Module for DndModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        // Slashed bell (tile tinted) when silencing, plain bell otherwise.
        let icon = if self.on {
            "notification-disabled-symbolic"
        } else {
            "notification-symbolic"
        };
        let status = if self.on { "On" } else { "Off" };
        super::toggle_tile(
            id,
            width,
            self.on,
            edit,
            icon,
            "Do Not Disturb",
            status,
            super::Chevron::None,
        )
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        if control == "on" {
            if let ControlValue::Bool(b) = value {
                self.on = b;
                write_dnd(b);
            }
        }
        Task::none()
    }

    // Re-read on poll so a change from COSMIC Settings or the notification center
    // reflects here while the popup is open.
    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
        self.on = read_dnd();
        Task::none()
    }
}
