//! A full-width section divider: a horizontal rule spanning all four columns,
//! so users can visually separate groups of tiles. Always Full width and not
//! resizable — being a 4-col tile, it forces a row break, which is what cleaves
//! the grid into sections.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget;

pub struct DividerModule {
    desc: ModuleDescriptor,
}

impl DividerModule {
    pub fn new() -> Self {
        Self {
            desc: ModuleDescriptor {
                id: "builtin.divider".into(),
                name: "Divider".into(),
                icon: "list-remove-symbolic".into(),
                size: TileSize::Full,
                resizable: false,
            },
        }
    }
}

impl Module for DividerModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    /// Always spans the full grid width.
    fn min_cols(&self) -> u32 {
        4
    }

    fn view(&self, _id: InstanceId, _edit: bool, width: f32) -> Element<'_, Message> {
        // A theme-aware hairline with vertical breathing room either side.
        widget::container(widget::divider::horizontal::default())
            .width(Length::Fixed(width))
            .padding([12, 0])
            .into()
    }

    fn on_control(&mut self, _control: &str, _value: ControlValue) -> Task<Message> {
        Task::none()
    }
}
