//! Notification center (issue #9): collects desktop notifications observed by
//! the passive D-Bus monitor (see `crate::notifications`) and shows them inline.
//! The list is live from when the monitor starts; it's separate from COSMIC's
//! own notification history.
//!
//! Unlike the toggle tiles this isn't a click-to-expand drawer — notifications
//! are content you want at a glance, so the tile renders them directly: a slim
//! "No notifications" row when empty, the full (scrolling) list when there's any.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use crate::notifications::{Notification, read_dnd, write_dnd};
use crate::theme;
use cosmic::app::Task;
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;

/// How many notifications to keep (newest first).
const CAP: usize = 30;
/// Scroll the list past this height instead of growing the popup unbounded.
const LIST_MAX: f32 = 320.0;

struct Item {
    /// Stable id so dismiss targets the right card even as new ones arrive.
    id: u64,
    n: Notification,
}

pub struct NotificationsModule {
    desc: ModuleDescriptor,
    items: Vec<Item>,
    next: u64,
    /// Cached Do Not Disturb state (the COSMIC system setting), re-read on poll.
    dnd: bool,
}

impl NotificationsModule {
    pub fn new() -> Self {
        Self {
            desc: ModuleDescriptor {
                id: "builtin.notifications".into(),
                name: "Notifications".into(),
                icon: "notification-symbolic".into(),
                // A panel reads best wide; resizable down if the user wants.
                size: TileSize::Large,
                resizable: true,
            },
            items: Vec::new(),
            next: 0,
            dnd: read_dnd(),
        }
    }
}

impl Module for NotificationsModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn min_cols(&self) -> u32 {
        2
    }

    /// A bell with a badge when there's anything to read, plain otherwise.
    fn status_icon(&self) -> String {
        if self.items.is_empty() {
            "notification-symbolic".into()
        } else {
            "notification-new-symbolic".into()
        }
    }

    fn ingest_notification(&mut self, n: Notification) {
        self.items.insert(0, Item { id: self.next, n });
        self.next = self.next.wrapping_add(1);
        self.items.truncate(CAP);
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        // Do Not Disturb as a compact switching icon: a bell when off, a slashed
        // bell (accent-filled) when on. Shared by both layouts.
        let dnd_icon = if self.dnd {
            "notification-disabled-symbolic"
        } else {
            "notification-symbolic"
        };
        let mut dnd = widget::button::icon(widget::icon::from_name(dnd_icon).size(16))
            .padding(6)
            .class(if self.dnd {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Icon
            });
        if !edit {
            dnd = dnd.on_press(Message::Control(id, "dnd".into(), ControlValue::Bool(!self.dnd)));
        }

        // Empty: a compact single row (label + the DnD icon), so the tile stays
        // small until something arrives.
        if self.items.is_empty() {
            let row = widget::Row::new()
                .spacing(10)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .push(
                    widget::text::caption("No notifications")
                        .class(cosmic::style::Text::Custom(theme::dim_text)),
                )
                .push(widget::space::horizontal())
                .push(dnd);
            return super::tile(width, false, row);
        }

        // Header: just the controls (the cards below speak for themselves) — the
        // DnD icon and Clear all, right-aligned.
        let mut clear = widget::button::text("Clear all").class(cosmic::theme::Button::Text);
        if !edit {
            clear = clear.on_press(Message::Control(id, "clear".into(), ControlValue::Trigger));
        }
        let header = widget::Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(widget::space::horizontal())
            .push(dnd)
            .push(clear);

        let mut list = widget::Column::new().spacing(8).width(Length::Fill);
        for item in &self.items {
            list = list.push(card(id, item, edit));
        }

        let content = widget::Column::new()
            .spacing(10)
            .width(Length::Fill)
            .push(header)
            .push(widget::container(widget::scrollable(list)).max_height(LIST_MAX));
        super::tile(width, false, content)
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match control {
            "clear" => self.items.clear(),
            "dnd" => {
                if let ControlValue::Bool(b) = value {
                    self.dnd = b;
                    write_dnd(b);
                }
            }
            "dismiss" => {
                if let ControlValue::Text(key) = value {
                    if let Ok(id) = key.parse::<u64>() {
                        self.items.retain(|it| it.id != id);
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    // Re-read DnD on poll so a change made elsewhere (COSMIC Settings, the stock
    // applet) reflects here while the popup is open.
    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
        self.dnd = read_dnd();
        Task::none()
    }
}

/// One notification as an inset card: app name (dim), summary, an optional body
/// line (dim), and a dismiss button.
fn card<'a>(id: InstanceId, item: &Item, edit: bool) -> Element<'a, Message> {
    let n = &item.n;
    let mut text = widget::Column::new()
        .spacing(2)
        .width(Length::Fill)
        .push(
            widget::text::caption(n.app.clone())
                .class(cosmic::style::Text::Custom(theme::dim_text)),
        )
        .push(widget::text::body(n.summary.clone()));
    if !n.body.is_empty() {
        text = text.push(
            widget::text::caption(n.body.clone())
                .class(cosmic::style::Text::Custom(theme::dim_text)),
        );
    }

    let mut dismiss =
        widget::button::icon(widget::icon::from_name("window-close-symbolic").size(14)).padding(4);
    if !edit {
        dismiss = dismiss.on_press(Message::Control(
            id,
            "dismiss".into(),
            ControlValue::Text(item.id.to_string()),
        ));
    }

    let row = widget::Row::new()
        .spacing(8)
        .align_y(Alignment::Start)
        .push(leading_icon(&n.icon))
        .push(text)
        .push(dismiss);

    widget::container(row)
        .padding(10)
        .width(Length::Fill)
        .class(theme::inset())
        .into()
}

/// The per-notification icon. The `app_icon` from Notify is often an icon name
/// (usable directly) but can be a file path or empty; fall back to a bell so a
/// path we can't resolve doesn't render a broken-image box.
fn leading_icon<'a>(app_icon: &str) -> Element<'a, Message> {
    let name = if app_icon.is_empty() || app_icon.contains('/') {
        "notification-symbolic"
    } else {
        app_icon
    };
    widget::icon::from_name(name.to_string()).size(20).into()
}
