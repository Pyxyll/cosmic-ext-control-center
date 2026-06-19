//! The panel applet — a COSMIC panel icon whose popup hosts the control center
//! in **display + interact** mode (no editing chrome). It shares the exact same
//! UI as the window app via the `Hub`, but with `allow_edit = false`: tiles are
//! live and interactive, and the gear launches the companion editor window.
//!
//! The popup reloads the layout from cosmic-config each time it opens, so it
//! always reflects what the editor last set up.

use crate::app::{Hub, Message};
use crate::config::AppletIcons;
use crate::status::StatusSnapshot;
use cosmic::app::{Core, Task};
use cosmic::applet::token::subscription::{
    TokenRequest, TokenUpdate, activation_token_subscription,
};
use cosmic::cctk::sctk::reexports::calloop::channel::Sender;
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Limits, Subscription};
use cosmic::prelude::*;
use cosmic::surface::Action as SurfaceAction;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget;

/// Popup width. `popup_container` hard-caps itself at 360px via its own autosize
/// limits, so we override those (below) — this is the real width control. Wide
/// enough for the full 4-column block (430px) + the view's padding.
const POPUP_WIDTH: f32 = 500.0;

const APP_ID: &str = "com.pyxyll.CosmicExtControlCenterApplet";

pub struct Applet {
    core: Core,
    popup: Option<Id>,
    hub: Hub,
    /// Live system state for the panel status-icon cluster (issue #21), fed by
    /// the D-Bus / pactl status sources. Only meaningful in `Status` icon mode.
    status: StatusSnapshot,
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
                status: StatusSnapshot::default(),
                token_tx: None,
            },
            Task::none(),
        )
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        // Always listen for activation-token requests. Add the status sources
        // whenever the cluster icon mode is on (they keep the panel icons live
        // even with the popup closed), and the hub's poll/anim timers only while
        // the popup is open.
        let mut subs = vec![activation_token_subscription(0).map(Message::Token)];
        if matches!(self.hub.applet_icons(), AppletIcons::Status) {
            subs.push(crate::status::subscription(self.hub.cluster_icons()));
        }
        if self.popup.is_some() {
            subs.push(self.hub.subscription());
        }
        Subscription::batch(subs)
    }

    fn view(&self) -> Element<'_, Message> {
        let popup_id = self.popup;
        // Either presentation toggles the same popup on click.
        let press = move |_offset, _bounds| Message::Surface(popup_action(popup_id));

        match self.hub.applet_icons() {
            AppletIcons::Single => self
                .core
                .applet
                .icon_button("emblem-system-symbolic")
                .on_press_with_rectangle(press)
                .into(),
            // A cluster of live status icons laid along the panel's major axis,
            // in one applet-styled button (so it's a single click target).
            AppletIcons::Status => {
                let app = &self.core.applet;
                let icon_px = app.suggested_size(true).0;
                let horizontal = app.is_horizontal();
                let (pad_major, pad_minor) = app.suggested_padding(true);
                let icon = |name: &'static str| widget::icon::from_name(name).size(icon_px);
                let mut names = self.status.icons(self.hub.cluster_icons());
                // Never render an empty (unclickable) button: if every indicator
                // is hidden or inactive, fall back to the control-center gear.
                if names.is_empty() {
                    names.push("emblem-system-symbolic");
                }
                let content: Element<'_, Message> = if horizontal {
                    let mut r = widget::Row::new().spacing(8).align_y(Alignment::Center);
                    for n in names {
                        r = r.push(icon(n));
                    }
                    r.into()
                } else {
                    let mut c = widget::Column::new().spacing(8).align_x(Alignment::Center);
                    for n in names {
                        c = c.push(icon(n));
                    }
                    c.into()
                };
                let pad = if horizontal {
                    [pad_minor as f32, pad_major as f32]
                } else {
                    [pad_major as f32, pad_minor as f32]
                };
                let button = widget::button::custom(content)
                    .class(cosmic::theme::Button::AppletIcon)
                    .padding(pad)
                    .on_press_with_rectangle(press);
                // Unlike `icon_button` (fixed single-icon size), the cluster is
                // wider than the default panel slot, so wrap it in an autosize
                // window to request a surface that fits the whole row.
                self.core.applet.autosize_window(button).into()
            }
        }
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
            Message::Status(update) => {
                self.status.apply_update(update);
                Task::none()
            }
            other => self.hub.update(other),
        }
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

/// Toggle the popup: destroy it if open, else open it (reloading the layout so
/// it reflects the editor's latest save). Shared by both panel presentations.
fn popup_action(popup_id: Option<Id>) -> SurfaceAction {
    if let Some(id) = popup_id {
        return destroy_popup(id);
    }
    app_popup::<Applet>(
        |state: &mut Applet| {
            let new_id = Id::unique();
            state.popup = Some(new_id);
            state.hub.reload();
            let mut settings = state.core.applet.get_popup_settings(
                state.core.main_window_id().unwrap(),
                new_id,
                None,
                None,
                None,
            );
            // Wide enough for the full 4-column block (430px) + the view's
            // padding + the popup_container inset.
            settings.positioner.size_limits = Limits::NONE
                .min_width(POPUP_WIDTH)
                .max_width(POPUP_WIDTH + 40.0)
                .min_height(200.0)
                .max_height(900.0);
            settings
        },
        None,
    )
}
