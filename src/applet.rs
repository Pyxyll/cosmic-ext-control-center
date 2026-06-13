//! The panel applet — a COSMIC panel icon whose popup hosts the control center
//! in **display + interact** mode (no editing chrome). It shares the exact same
//! UI as the window app via the `Hub`, but with `allow_edit = false`: tiles are
//! live and interactive, and the gear launches the companion editor window.
//!
//! The popup reloads the layout from cosmic-config each time it opens, so it
//! always reflects what the editor last set up.

use crate::app::{Hub, Message};
use cosmic::app::{Core, Task};
use cosmic::applet::token::subscription::{
    TokenRequest, TokenUpdate, activation_token_subscription,
};
use cosmic::cctk::sctk::reexports::calloop::channel::Sender;
use cosmic::iced::window::Id;
use cosmic::iced::{Limits, Subscription};
use cosmic::prelude::*;
use cosmic::surface::action::{app_popup, destroy_popup};

/// Popup width. `popup_container` hard-caps itself at 360px via its own autosize
/// limits, so we override those (below) — this is the real width control. Wide
/// enough for the full 4-column block (430px) + the view's padding.
const POPUP_WIDTH: f32 = 500.0;

const APP_ID: &str = "com.pyxyll.CosmicExtControlCenterApplet";

pub struct Applet {
    core: Core,
    popup: Option<Id>,
    hub: Hub,
    /// Channel to request a Wayland activation token (for launching the editor
    /// so it can raise its window). Set once the token subscription initializes.
    token_tx: Option<Sender<TokenRequest>>,
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
                token_tx: None,
            },
            Task::none(),
        )
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        // Always listen for activation-token requests; add the hub's poll/anim
        // timers only while the popup is open.
        let token = activation_token_subscription(0).map(Message::Token);
        if self.popup.is_some() {
            Subscription::batch([self.hub.subscription(), token])
        } else {
            token
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
            // The gear: request an activation token, then launch the editor with
            // it (so the editor's single-instance handler can raise its window).
            // We're handling a user click, so the compositor should grant one.
            Message::OpenConfig => {
                if let Some(tx) = &self.token_tx {
                    let _ = tx.send(TokenRequest {
                        app_id: crate::config::APP_ID.to_string(),
                        exec: "cosmic-ext-control-center".to_string(),
                    });
                } else {
                    // No token channel yet — launch without one (won't raise).
                    let _ = std::process::Command::new("cosmic-ext-control-center").spawn();
                }
                Task::none()
            }
            Message::Token(update) => {
                match update {
                    TokenUpdate::Init(tx) => self.token_tx = Some(tx),
                    TokenUpdate::ActivationToken { token, exec } => {
                        let mut cmd = std::process::Command::new(&exec);
                        if let Some(token) = token {
                            cmd.env("XDG_ACTIVATION_TOKEN", &token);
                            cmd.env("DESKTOP_STARTUP_ID", &token);
                        }
                        let _ = cmd.spawn();
                    }
                    TokenUpdate::Finished => {}
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
