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
use cosmic::iced::platform_specific::shell::wayland::commands::layer_surface::{
    self, KeyboardInteractivity, destroy_layer_surface, get_layer_surface,
};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::runtime::platform_specific::wayland::layer_surface::SctkLayerSurfaceSettings;
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Limits, Subscription};
use cosmic::prelude::*;
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
    /// True when `popup` is a layer surface (opened by the global shortcut),
    /// false when it's a grabbing popup (opened by a panel click) — so we tear
    /// it down with the matching destroy call.
    popup_is_layer_surface: bool,
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
                popup_is_layer_surface: false,
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
        let mut subs = vec![
            activation_token_subscription(0).map(Message::Token),
            // Always listen for the external open trigger (the global shortcut).
            crate::trigger::subscription(),
        ];
        if matches!(self.hub.applet_icons(), AppletIcons::Status) {
            subs.push(crate::status::subscription(self.hub.cluster_icons()));
        }
        // The notification monitor runs whenever a notifications tile is placed,
        // so the list is current the moment the popup opens (not just while open).
        if self.hub.has_notifications() {
            subs.push(crate::notifications::subscription());
        }
        if self.popup.is_some() {
            subs.push(self.hub.subscription());
            // While a surface is open, dismiss it on click-away (the full-width
            // layer surface) or Escape.
            use cosmic::iced::Event;
            use cosmic::iced::core::keyboard::{self, key::Named as NamedKey};
            use cosmic::iced::event::{Status, listen_raw, listen_with};
            subs.push(listen_with(|event, _status, window_id| {
                match event {
                    Event::Window(cosmic::iced::window::Event::Unfocused) => {
                        Some(Message::WindowUnfocused(window_id))
                    }
                    _ => None,
                }
            }));
            subs.push(listen_raw(|event, status, _| {
                if status != Status::Ignored {
                    return None;
                }
                match event {
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(NamedKey::Escape),
                        ..
                    }) => Some(Message::ToggleSurface),
                    _ => None,
                }
            }));
        }
        Subscription::batch(subs)
    }

    fn view(&self) -> Element<'_, Message> {
        match self.hub.applet_icons() {
            AppletIcons::Single => self
                .core
                .applet
                .icon_button("emblem-system-symbolic")
                .on_press(Message::TogglePopup)
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
                    .on_press(Message::TogglePopup);
                // Unlike `icon_button` (fixed single-icon size), the cluster is
                // wider than the default panel slot, so wrap it in an autosize
                // window to request a surface that fits the whole row.
                self.core.applet.autosize_window(button).into()
            }
        }
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        // `popup_container`'s autosize shrinks the surface to the content (so the
        // layer surface has no transparent full-width band to swallow clicks, and
        // keyboard/Escape focus the card). It defaults to a 360px max_width, so
        // override the limits to fit the full layout.
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
                    self.popup_is_layer_surface = false;
                }
                Task::none()
            }
            // Click-away dismiss, but only for the layer surface (the grabbing
            // popup handles its own click-outside dismiss via the grab).
            Message::WindowUnfocused(id) => {
                if self.popup == Some(id) && self.popup_is_layer_surface {
                    if let Some(p) = self.popup.take() {
                        return self.close_popup(p);
                    }
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
            // Panel click: a normal grabbing popup (it carries an input serial, so
            // it maps and auto-dismisses on click-outside).
            Message::TogglePopup => {
                if let Some(p) = self.popup.take() {
                    return self.close_popup(p);
                }
                let new_id = Id::unique();
                self.popup = Some(new_id);
                self.popup_is_layer_surface = false;
                self.hub.reload();
                let mut settings = self.core.applet.get_popup_settings(
                    self.core.main_window_id().unwrap(),
                    new_id,
                    None,
                    None,
                    None,
                );
                settings.positioner.size_limits = Limits::NONE
                    .min_width(POPUP_WIDTH)
                    .max_width(POPUP_WIDTH + 40.0)
                    .min_height(200.0)
                    .max_height(900.0);
                get_popup(settings)
            }
            // Global shortcut: a layer surface (no input serial needed, unlike a
            // grabbing popup — that's why an external trigger can open it).
            Message::ToggleSurface => {
                if let Some(p) = self.popup.take() {
                    return self.close_popup(p);
                }
                let new_id = Id::unique();
                self.popup = Some(new_id);
                self.popup_is_layer_surface = true;
                self.hub.reload();
                // A single-edge anchor with a fixed width doesn't map in
                // cosmic-comp, but spanning the top (LEFT|RIGHT) does. So span the
                // top and right-align the content in `view_window` so the card
                // lands top-right, near the applet.
                get_layer_surface(SctkLayerSurfaceSettings {
                    id: new_id,
                    keyboard_interactivity: KeyboardInteractivity::OnDemand,
                    anchor: layer_surface::Anchor::TOP
                        | layer_surface::Anchor::LEFT
                        | layer_surface::Anchor::RIGHT,
                    namespace: "cosmic-ext-control-center".into(),
                    size: Some((None, Some(860))),
                    size_limits: Limits::NONE.min_width(1.0).min_height(1.0),
                    ..Default::default()
                })
            }
            other => self.hub.update(other),
        }
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl Applet {
    /// Tear down the open surface with the matching destroy call, and clear the
    /// layer-surface flag. `self.popup` is expected to already be taken.
    fn close_popup(&mut self, p: Id) -> Task<Message> {
        let is_layer = self.popup_is_layer_surface;
        self.popup_is_layer_surface = false;
        if is_layer {
            destroy_layer_surface(p)
        } else {
            destroy_popup(p)
        }
    }
}
