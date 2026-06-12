//! The panel applet — a COSMIC panel icon whose popup hosts the control center
//! in **display + interact** mode (no editing chrome). It shares the exact same
//! UI as the window app via the `Hub`, but with `allow_edit = false`: tiles are
//! live and interactive, and the gear launches the companion editor window.
//!
//! The popup reloads the layout from cosmic-config each time it opens, so it
//! always reflects what the editor last set up.

use crate::app::{Hub, Message};
use cosmic::app::{Core, Task};
use cosmic::iced::window::Id;
use cosmic::iced::{Limits, Subscription};
use cosmic::prelude::*;
use cosmic::surface::action::{app_popup, destroy_popup};

/// Popup width. `popup_container` hard-caps itself at 360px via its own autosize
/// limits, so we override those (below) — this is the real width control. Wide
/// enough for the full 4-column block (430px) + the view's padding.
const POPUP_WIDTH: f32 = 500.0;

const APP_ID: &str = "com.pyxyll.CosmicControlCenterApplet";

pub struct Applet {
    core: Core,
    popup: Option<Id>,
    hub: Hub,
}

impl cosmic::Application for Applet {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        (
            Applet {
                core,
                popup: None,
                hub: Hub::new(false),
            },
            Task::none(),
        )
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        // Only run the hub's poll/animation timers while the popup is open.
        if self.popup.is_some() {
            self.hub.subscription()
        } else {
            Subscription::none()
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let popup_id = self.popup;
        self.core
            .applet
            .icon_button("emblem-system-symbolic")
            .on_press_with_rectangle(move |_offset, _bounds| {
                if let Some(id) = popup_id {
                    Message::Surface(destroy_popup(id))
                } else {
                    Message::Surface(app_popup::<Applet>(
                        |state: &mut Applet| {
                            let new_id = Id::unique();
                            state.popup = Some(new_id);
                            // Reflect the latest layout the editor saved.
                            state.hub.reload();
                            let mut settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );
                            // Wide enough for the full 4-column block (430px) +
                            // the view's padding + the popup_container inset.
                            // The view fills to width, so this sets the popup
                            // width; match the window app's comfortable 560.
                            settings.positioner.size_limits = Limits::NONE
                                .min_width(POPUP_WIDTH)
                                .max_width(POPUP_WIDTH + 40.0)
                                .min_height(200.0)
                                .max_height(900.0);
                            settings
                        },
                        None,
                    ))
                }
            })
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        // `popup_container` defaults to a 360px max_width via its autosize limits;
        // override them so the full layout fits. The view fills to width, so
        // min == max pins the popup to exactly POPUP_WIDTH.
        self.core
            .applet
            .popup_container(self.hub.view())
            .limits(
                Limits::NONE
                    .min_width(POPUP_WIDTH)
                    .max_width(POPUP_WIDTH)
                    .min_height(1.0)
                    .max_height(1000.0),
            )
            .into()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Surface(a) => cosmic::task::message(cosmic::Action::Cosmic(
                cosmic::app::Action::Surface(a),
            )),
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
                Task::none()
            }
            other => self.hub.update(other),
        }
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
