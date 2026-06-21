//! The hub shell: holds the placed module instances, renders them as
//! draggable tiles in a reflowing grid, and routes interactions. Layout
//! changes (add / remove / reorder) persist to cosmic-config immediately.

use crate::config::{Config, InstanceConfig};
use crate::module::manifest::{Manifest, ManifestModule};
use crate::module::{
    ControlValue, GRID_GAP, GRID_UNIT, InstanceId, ListEntry, Module, ModuleDescriptor, Payload,
    TileSize, ValueAnim, builtin,
};
use crate::plugins;
use crate::theme;
use cosmic::app::{Core, Task};
use cosmic::iced::{Alignment, Length, Subscription, time};
use cosmic::prelude::*;
use cosmic::widget;
use std::time::Duration;

/// A live, placed tile. The hub owns the per-instance `size` (so the same
/// module type can be Normal in one spot and Wide in another).
struct Instance {
    id: InstanceId,
    module: Box<dyn Module>,
    size: TileSize,
    /// Animated tile width (px) — eases when `size` changes for a smooth resize,
    /// and drives the add (0→natural) / remove (natural→0) reveal.
    width: ValueAnim,
    /// Set when the tile is animating out; once its width hits 0 the hub frees
    /// it (see the `Frame` handler). Kept in the vec until then so the exit plays.
    removing: bool,
}

/// The full control-center UI + state, independent of how it's hosted. Both the
/// window app (`App`, the editor) and the panel applet (`Applet`, display +
/// interact only) embed a `Hub` and delegate `view`/`update`/`subscription` to
/// it. `allow_edit` gates the editing chrome: the window sets it true; the
/// applet sets it false and its gear launches the editor instead of toggling.
pub struct Hub {
    config: Config,
    /// Manifests discovered from the plugin dirs, for the palette + factory.
    plugins: Vec<Manifest>,
    instances: Vec<Instance>,
    edit: bool,
    /// Whether this host permits editing (the window app: yes; the applet: no).
    allow_edit: bool,
    palette_open: bool,
    /// A destructive power action awaiting confirmation.
    pending_power: Option<PowerAction>,
    /// The tile whose inline selection list is currently expanded (Wi-Fi /
    /// Bluetooth / VPN). Stays set while the drawer animates closed, then cleared
    /// in `Frame`. Reset when the popup reopens.
    expanded: Option<InstanceId>,
    /// Whether the drawer is open. (Open/close is instant — the popup is too
    /// heavy to re-render at animation framerates; see the perf follow-up.)
    expand_open: bool,
    /// A scan for the open drawer is in flight (shows a spinner; cleared when its
    /// results arrive). Only set for user-initiated scans (open / manual
    /// refresh), not the silent background poll.
    expand_loading: bool,
    /// (Editor) the tile whose option picker (gear) is revealed, if any.
    config_open: Option<InstanceId>,
    /// (Editor) whether the app-wide Settings panel is open in the right sidebar.
    settings_open: bool,
    /// Latest battery reading (percent, charging?), refreshed on poll. `None`
    /// on desktops with no battery — the readout is simply hidden.
    battery: Option<(u8, bool)>,
    /// Force frame ticks until this instant. A layout-changing toggle (edit /
    /// palette) only triggers one redraw, but with a multi-buffered Vulkan
    /// swapchain an older buffer (still showing the edit chrome) can be
    /// re-presented; a short redraw burst refreshes every buffer so no ghost
    /// of the previous layout lingers.
    redraw_until: Option<std::time::Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ToggleEdit,
    OpenPalette,
    AddModule(String),
    RemoveInstance(InstanceId),
    /// Cycle a tile's size (Normal ↔ Wide) in edit mode.
    ResizeInstance(InstanceId),
    /// Pick a tile's configurable option in the editor (e.g. a disk gauge's
    /// mount): (instance, index into the module's `option_choices`).
    SetOption(InstanceId, usize),
    /// (Editor) toggle the option picker (gear) for a tile.
    ToggleConfig(InstanceId),
    /// (Editor) toggle the app-wide Settings panel.
    ToggleSettings,
    /// (Editor) pick the panel applet's icon mode (index into the choices).
    SetAppletIcons(usize),
    /// (Editor) toggle one indicator's visibility in the status cluster.
    ToggleClusterIcon(crate::config::ClusterIcon),
    /// Toggle a tile's inline selection list (Wi-Fi networks, devices, VPN
    /// profiles). Expanding triggers a one-off scan of that module.
    Expand(InstanceId),
    /// Manually re-scan the open drawer's list (the refresh button), showing the
    /// loading spinner until results arrive.
    RefreshEntries(InstanceId),
    Reordered(Vec<InstanceId>),
    /// A module control changed: (instance, control id, new value).
    Control(InstanceId, String, ControlValue),
    /// A module's on-demand background refresh finished (drawer open / manual).
    StateLoaded(InstanceId, Payload),
    /// A poll's batched refresh finished — all module results applied in one go,
    /// so the popup repaints once per poll rather than once per module.
    StateBatch(Vec<(InstanceId, Payload)>),
    /// Periodic tick — refresh polled module state (e.g. manifest `get`s).
    Poll,
    /// ~60fps tick while a module is animating (drives the redraw; no-op state).
    Frame,
    /// A power/session action was pressed (may need confirmation first).
    Power(PowerAction),
    /// Confirm the pending destructive power action.
    PowerConfirm,
    /// Dismiss the pending power action.
    PowerCancel,
    /// Launch the companion config app (the applet's gear, when editing is off).
    OpenConfig,
    /// (applet) A status source (D-Bus / pactl) reported new system state for
    /// the panel status-icon cluster. Ignored by the editor.
    Status(crate::status::Update),
    /// A desktop notification arrived (from the passive D-Bus monitor), routed
    /// to the notification-center tile.
    Notify(crate::notifications::Notification),
    /// (applet) Layer-shell surface plumbing for the panel popup.
    Surface(cosmic::surface::Action),
    /// (applet) The popup window was closed.
    PopupClosed(cosmic::iced::window::Id),
    /// (applet) Activation-token subscription output, used to launch the editor
    /// with a Wayland activation token so it can raise its window.
    Token(cosmic::applet::token::subscription::TokenUpdate),
}

/// Session/power actions in the always-present footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Lock,
    Sleep,
    Logout,
    Restart,
    Shutdown,
}

impl PowerAction {
    fn icon(self) -> &'static str {
        match self {
            PowerAction::Lock => "system-lock-screen-symbolic",
            PowerAction::Sleep => "system-suspend-symbolic",
            PowerAction::Logout => "system-log-out-symbolic",
            PowerAction::Restart => "system-reboot-symbolic",
            PowerAction::Shutdown => "system-shutdown-symbolic",
        }
    }

    fn label(self) -> &'static str {
        match self {
            PowerAction::Lock => "Lock",
            PowerAction::Sleep => "Sleep",
            PowerAction::Logout => "Log Out",
            PowerAction::Restart => "Restart",
            PowerAction::Shutdown => "Shut Down",
        }
    }

    /// Lock/Sleep are reversible → run immediately. The rest lose work → confirm.
    fn needs_confirm(self) -> bool {
        matches!(
            self,
            PowerAction::Logout | PowerAction::Restart | PowerAction::Shutdown
        )
    }

    fn run(self) {
        let cmd = match self {
            PowerAction::Lock => "loginctl lock-session",
            PowerAction::Sleep => "systemctl suspend",
            PowerAction::Logout => "loginctl terminate-session \"$XDG_SESSION_ID\"",
            PowerAction::Restart => "systemctl reboot",
            PowerAction::Shutdown => "systemctl poweroff",
        };
        let _ = std::process::Command::new("sh").arg("-c").arg(cmd).spawn();
    }
}

impl Hub {
    /// Build a module by id from either the built-in registry or a discovered
    /// plugin manifest. The hub treats both identically.
    fn make(
        &self,
        module_id: &str,
        params: &std::collections::BTreeMap<String, String>,
    ) -> Option<Box<dyn Module>> {
        if let Some(m) = builtin::make(module_id, params) {
            return Some(m);
        }
        self.plugins
            .iter()
            .find(|m| m.id == module_id)
            .map(|m| Box::new(ManifestModule::from_manifest(m)) as Box<dyn Module>)
    }

    /// All addable module types: built-ins + discovered plugins.
    fn available(&self) -> Vec<ModuleDescriptor> {
        let mut descs = builtin::descriptors();
        descs.extend(self.plugins.iter().map(|m| m.descriptor()));
        descs
    }

    /// Instantiate a module by id and append it as a new tile.
    fn add_module(&mut self, module_id: &str) {
        // Single-instance modules can't be added twice (the palette disables
        // them too, but guard here as well).
        if !builtin::allows_multiple(module_id)
            && self.instances.iter().any(|i| i.module.descriptor().id == module_id)
        {
            return;
        }
        if let Some(m) = self.make(module_id, &Default::default()) {
            let id = self.config.next_id;
            self.config.next_id += 1;
            let size = m.descriptor().size;
            // Start at 0 width and ease out to the natural width — the tile
            // grows into place and neighbours reflow around it.
            let mut width = ValueAnim::new(0.0);
            width.set(size.width());
            self.instances.push(Instance {
                id,
                module: m,
                size,
                width,
                removing: false,
            });
            self.persist();
        }
    }

    /// Rebuild live instances from saved config (on startup).
    fn build_instances(&mut self) {
        for c in self.config.instances.clone() {
            if let Some(m) = self.make(&c.module, &c.params) {
                // Clamp a saved size up to the module's minimum span.
                let mut size = c.size;
                while size.cols() < m.min_cols() {
                    size = size.toggled();
                }
                self.instances.push(Instance {
                    id: c.id,
                    module: m,
                    size,
                    width: ValueAnim::new(size.width()),
                    removing: false,
                });
            }
        }
    }

    /// Mirror the current instance list (order + sizes) into config + save.
    fn persist(&mut self) {
        self.config.instances = self
            .instances
            .iter()
            .filter(|i| !i.removing)
            .map(|i| InstanceConfig {
                id: i.id,
                module: i.module.descriptor().id.clone(),
                size: i.size,
                params: i.module.params(),
            })
            .collect();
        self.config.save();
    }

    fn reorder(&mut self, order: Vec<InstanceId>) {
        let mut reordered = Vec::with_capacity(self.instances.len());
        for id in &order {
            if let Some(pos) = self.instances.iter().position(|i| i.id == *id) {
                reordered.push(self.instances.remove(pos));
            }
        }
        reordered.extend(self.instances.drain(..)); // keep any not named
        self.instances = reordered;
        self.persist();
    }

    /// Edit-mode ruler: the four grid columns, numbered, so the user can see
    /// the column rhythm tiles snap to. Same total width as a Full tile.
    fn grid_ruler<'a>() -> Element<'a, Message> {
        let mut row = widget::Row::new().spacing(GRID_GAP);
        for i in 0..4 {
            row = row.push(
                widget::container(widget::text::caption(format!("{}", i + 1)))
                    .width(Length::Fixed(GRID_UNIT))
                    .height(Length::Fixed(22.0))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .class(theme::card(false, theme::accent())),
            );
        }
        row.into()
    }

    /// Always-present session/power controls at the bottom of the hub. Shows a
    /// confirm prompt for destructive actions.
    /// The top bar: power actions split to the edges (Lock + Log Out left;
    /// Sleep, Restart, Shut Down right) with the edit controls centered. When a
    /// destructive action is pending it becomes an inline confirm prompt.
    fn power_bar(&self) -> Element<'_, Message> {
        if let Some(a) = self.pending_power {
            return widget::Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(widget::text::body(format!("{}?", a.label())))
                .push(widget::space::horizontal())
                .push(widget::button::suggested("Confirm").on_press(Message::PowerConfirm))
                .push(widget::button::standard("Cancel").on_press(Message::PowerCancel))
                .into();
        }

        let pbtn = |a: PowerAction| {
            widget::button::icon(widget::icon::from_name(a.icon()).size(20))
                .on_press(Message::Power(a))
        };
        let left = widget::Row::new()
            .spacing(6)
            .align_y(Alignment::Center)
            .push(pbtn(PowerAction::Lock))
            .push(pbtn(PowerAction::Logout));
        let right = widget::Row::new()
            .spacing(6)
            .align_y(Alignment::Center)
            .push(pbtn(PowerAction::Sleep))
            .push(pbtn(PowerAction::Restart))
            .push(pbtn(PowerAction::Shutdown));

        widget::Row::new()
            .align_y(Alignment::Center)
            .push(left)
            .push(widget::space::horizontal())
            .push(right)
            .into()
    }

    /// The bottom bar: battery readout (left, hidden on batteryless machines) and
    /// a settings/gear button (right) that currently doubles as the edit toggle.
    /// `＋ Add` joins it on the right while editing, grouping the edit controls.
    fn bottom_bar(&self) -> Element<'_, Message> {
        let left: Element<'_, Message> = match self.battery {
            Some((pct, charging)) => widget::Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .push(widget::icon::from_name(battery_icon(pct, charging)).size(18))
                .push(widget::text::body(format!("{pct}%")))
                .into(),
            None => widget::Row::new().into(),
        };

        let mut right = widget::Row::new().spacing(8).align_y(Alignment::Center);
        if self.edit {
            right = right.push(widget::button::standard("＋ Add").on_press(Message::OpenPalette));
        }
        // The gear toggles edit in the editor; in the applet (no editing) it
        // launches the companion editor window instead.
        let gear_msg = if self.allow_edit {
            Message::ToggleEdit
        } else {
            Message::OpenConfig
        };
        right = right.push(
            widget::button::icon(widget::icon::from_name("emblem-system-symbolic").size(20))
                .on_press(gear_msg),
        );

        widget::Column::new()
            .spacing(10)
            .push(widget::divider::horizontal::default())
            .push(
                widget::Row::new()
                    .align_y(Alignment::Center)
                    .push(left)
                    .push(widget::space::horizontal())
                    .push(right),
            )
            .into()
    }

    /// The add-module picker, shown as a centered modal card (see
    /// `palette_overlay`) instead of being appended to the bottom of the page.
    fn palette(&self) -> Element<'_, Message> {
        let header = widget::Row::new()
            .align_y(Alignment::Center)
            .push(widget::text::title4("Add a module"))
            .push(widget::space::horizontal())
            .push(round_btn("window-close-symbolic", Message::OpenPalette));

        // Module choices as a wrapping grid of icon+label cards, echoing the
        // tile grid itself.
        let mut grid = widget::Row::new().spacing(8);
        for d in self.available() {
            grid = grid.push(palette_item(&d.icon, &d.name, Message::AddModule(d.id.clone())));
        }

        let card = widget::Column::new()
            .spacing(14)
            .push(header)
            .push(grid.wrap());
        widget::container(card)
            .padding(16)
            .width(Length::Fill)
            .class(theme::card(false, theme::accent()))
            .into()
    }
}

/// A single choice in the add-module palette: a tile-like button with the
/// module's icon above its name.
fn palette_item<'a>(icon: &str, name: &str, msg: Message) -> Element<'a, Message> {
    let content = widget::Column::new()
        .spacing(8)
        .align_x(Alignment::Center)
        .push(widget::icon::from_name(icon).size(28))
        .push(widget::text::body(name.to_string()));
    let style = |bg: f32| {
        move |_focused: bool, t: &cosmic::Theme| {
            // Foreground from the theme so the glyph/label stay legible in light
            // mode (white-on-light was invisible).
            let fg: cosmic::iced::Color = t.cosmic().background.on.into();
            let mut s = cosmic::widget::button::Style::new();
            s.background = Some(cosmic::iced::Background::Color(theme::alpha(fg, bg)));
            s.border_radius = 12.0.into();
            s.icon_color = Some(fg);
            s.text_color = Some(fg);
            s
        }
    };
    widget::button::custom(content)
        .width(Length::Fixed(96.0))
        .height(Length::Fixed(96.0))
        .padding(10)
        .class(cosmic::theme::Button::Custom {
            active: Box::new(style(0.07)),
            disabled: Box::new(move |t| style(0.04)(false, t)),
            hovered: Box::new(style(0.16)),
            pressed: Box::new(style(0.22)),
        })
        .on_press(msg)
        .into()
}

/// A row in a tile's inline selection list: label + detail, a check on the
/// active entry; the whole row is a button that selects it.
fn entry_row<'a>(id: InstanceId, e: &ListEntry) -> Element<'a, Message> {
    let mut info = widget::Column::new()
        .spacing(1)
        .width(Length::Fill)
        .push(widget::text::body(e.label.clone()));
    if !e.detail.is_empty() {
        info = info.push(
            widget::text::caption(e.detail.clone())
                .class(cosmic::style::Text::Custom(theme::dim_text)),
        );
    }
    let mut row = widget::Row::new()
        .spacing(10)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .push(info);
    if e.active {
        row = row.push(widget::icon::from_name("object-select-symbolic").size(18));
    }
    // `MenuItem` is the built-in tappable-row style (hover highlight, rounded).
    // The previous `Button::Custom` allocated four boxed style closures per row
    // every render — ~60-80 allocations/frame across a full network list. The
    // connected network is marked by the check icon above instead of a tint.
    widget::button::custom(row)
        .class(cosmic::theme::Button::MenuItem)
        .padding([8, 12])
        .width(Length::Fill)
        .on_press(Message::Control(
            id,
            "select".into(),
            ControlValue::Text(e.key.clone()),
        ))
        .into()
}

/// A selected secured network expanded in place: one tinted card holding the
/// network name and, below it, the password field with Connect / Cancel. (The
/// row can't be a button here, since the field lives inside it.)
fn expanded_entry<'a>(e: &ListEntry, value: String, id: InstanceId) -> Element<'a, Message> {
    let info = widget::Column::new()
        .spacing(1)
        .push(widget::text::body(e.label.clone()))
        .push(
            widget::text::caption(e.detail.clone())
                .class(cosmic::style::Text::Custom(theme::dim_text)),
        );
    let field = widget::secure_input("Password", value, None, true)
        .on_input(move |v| Message::Control(id, "input".into(), ControlValue::Text(v)))
        .on_submit(move |_| Message::Control(id, "submit".into(), ControlValue::Trigger));
    let buttons = widget::Row::new()
        .spacing(8)
        .push(
            widget::button::standard("Cancel")
                .on_press(Message::Control(id, "cancel".into(), ControlValue::Trigger)),
        )
        .push(widget::space::horizontal())
        .push(
            widget::button::suggested("Connect")
                .on_press(Message::Control(id, "submit".into(), ControlValue::Trigger)),
        );
    let content = widget::Column::new()
        .spacing(8)
        .push(info)
        .push(field)
        .push(buttons);
    widget::container(content)
        .padding([8, 12])
        .width(Length::Fill)
        .class(theme::card(true, theme::accent()))
        .into()
}

/// A full-width row in the editor sidebar: icon + module name, tap to add.
fn sidebar_item<'a>(icon: &str, name: &str, msg: Message, enabled: bool) -> Element<'a, Message> {
    let mut content = widget::Row::new()
        .spacing(10)
        .align_y(Alignment::Center)
        .push(widget::icon::from_name(icon).size(18))
        .push(widget::text::body(name.to_string()));
    // A check on the right marks an already-placed single-instance module.
    if !enabled {
        content = content
            .push(widget::space::horizontal())
            .push(widget::icon::from_name("object-select-symbolic").size(14));
    }
    let style = move |bg: f32| {
        move |_focused: bool, t: &cosmic::Theme| {
            let fg: cosmic::iced::Color = t.cosmic().background.on.into();
            let mut s = cosmic::widget::button::Style::new();
            s.background = Some(cosmic::iced::Background::Color(theme::alpha(fg, bg)));
            s.border_radius = 8.0.into();
            // Dim a disabled (already-placed) row so it reads as unavailable.
            let alpha = if enabled { 1.0 } else { 0.4 };
            s.icon_color = Some(theme::alpha(fg, alpha));
            s.text_color = Some(theme::alpha(fg, alpha));
            s
        }
    };
    let mut btn = widget::button::custom(content)
        .width(Length::Fill)
        .padding([8, 10])
        .class(cosmic::theme::Button::Custom {
            active: Box::new(style(0.0)),
            disabled: Box::new(move |t| style(0.0)(false, t)),
            hovered: Box::new(style(if enabled { 0.10 } else { 0.0 })),
            pressed: Box::new(style(if enabled { 0.16 } else { 0.0 })),
        });
    if enabled {
        btn = btn.on_press(msg);
    }
    btn.into()
}

impl Hub {
    /// Build the hub. `allow_edit` is the editor (window) vs display-only
    /// (applet) distinction; only the editor seeds a default layout on first run.
    pub fn new(allow_edit: bool) -> Hub {
        let mut hub = Hub {
            config: Config::load(),
            plugins: plugins::discover(),
            instances: Vec::new(),
            // The editor is a dedicated edit surface — always in edit mode; the
            // applet is display-only.
            edit: allow_edit,
            allow_edit,
            palette_open: false,
            pending_power: None,
            expanded: None,
            expand_open: false,
            expand_loading: false,
            config_open: None,
            settings_open: false,
            battery: read_battery(),
            redraw_until: None,
        };
        hub.rebuild(allow_edit);
        hub
    }

    /// (Re)build the live instances from the loaded config. `seed` plants a
    /// default layout when the config is empty (the editor does; the applet
    /// shows nothing until the editor has set something up).
    fn rebuild(&mut self, seed: bool) {
        self.instances.clear();
        if self.config.instances.is_empty() && seed {
            for m in ["builtin.volume", "builtin.wifi", "builtin.bluetooth"] {
                self.add_module(m);
            }
        } else {
            self.build_instances();
        }
    }

    /// Reload from disk — the applet calls this each time its popup opens, so it
    /// reflects the latest layout set in the editor. Rebuilding re-instantiates
    /// every module, which spawns external commands (nmcli/bluetoothctl/wpctl/…)
    /// + a D-Bus connect — far too slow to do on the open path. So only rebuild
    /// when the layout actually changed; otherwise keep the live instances (and
    /// their open connections), making the popup open instantly.
    pub fn reload(&mut self) {
        // Each popup open starts collapsed.
        self.expanded = None;
        self.expand_open = false;
        self.expand_loading = false;
        let fresh = Config::load();
        // Always adopt the latest config (so settings changes are picked up), but
        // only rebuild the tiles when the layout actually changed — rebuilding
        // recreates the modules, dropping their live state.
        let layout_changed = fresh.instances != self.config.instances;
        self.config = fresh;
        if layout_changed {
            self.rebuild(false);
        }
    }

    /// The panel applet's icon presentation (single icon vs. status cluster).
    pub fn applet_icons(&self) -> crate::config::AppletIcons {
        self.config.settings.applet_icons
    }

    /// Which indicators the status cluster should show.
    pub fn cluster_icons(&self) -> crate::config::ClusterIcons {
        self.config.settings.cluster
    }

    /// Whether a notification-center tile is placed, so the applet only runs the
    /// notification monitor when something will display it.
    pub fn has_notifications(&self) -> bool {
        self.instances
            .iter()
            .any(|i| i.module.descriptor().id == "builtin.notifications")
    }

    /// The tile grid. In edit mode it's a `ReorderableFlexRow` (drag to reorder)
    /// with a resize/remove control bar above each tile; otherwise a plain
    /// `flex_row` so the tiles' own controls stay interactive.
    fn grid(&self) -> Element<'_, Message> {
        if self.edit {
            let mut row = widget::ReorderableFlexRow::new(Message::Reordered)
                .spacing(GRID_GAP)
                .width(Length::Fill);
            for inst in &self.instances {
                let w = inst.width.current();
                // A small control bar ABOVE each tile (resize + remove) — kept
                // outside the card so it never overlaps the tile's own controls.
                let has_options = !inst.module.option_choices().is_empty();
                let mut actions = widget::Row::new()
                    .spacing(3)
                    .align_y(Alignment::Center)
                    .push(widget::space::horizontal());
                // A gear (only when the module has an option) reveals the picker
                // below the tile, rather than always showing it.
                if has_options {
                    actions = actions
                        .push(round_btn("emblem-system-symbolic", Message::ToggleConfig(inst.id)));
                }
                if inst.module.descriptor().resizable {
                    actions = actions.push(round_btn(
                        "view-fullscreen-symbolic",
                        Message::ResizeInstance(inst.id),
                    ));
                }
                // A close (×), not a minus: this removes the tile, it doesn't
                // shrink or collapse it.
                actions = actions.push(round_btn(
                    "window-close-symbolic",
                    Message::RemoveInstance(inst.id),
                ));
                // Tight horizontal padding so three action buttons (gear + resize
                // + remove) still fit on a 1-col tile like the disk gauge without
                // clipping the rightmost one.
                let bar = widget::container(actions).width(Length::Fixed(w)).padding([0, 2]);
                // Clip the body to the (animating) width so a tile growing in or
                // shrinking out reveals/wipes cleanly instead of letting oversized
                // children (e.g. album art) spill past the shrinking card.
                let body = widget::container(inst.module.view(inst.id, true, w))
                    .width(Length::Fixed(w))
                    .clip(true);
                // The gear selects the tile; its settings show in the right-hand
                // config sidebar (see `config_sidebar`), not inline.
                let tile = widget::Column::new().spacing(2).push(bar).push(body);
                row = row.push(inst.id, tile);
            }
            row.into()
        } else {
            // Fixed 4-column block: pack tiles into rows ourselves (greedy, by
            // column span) so an expanded selection list can be injected inline,
            // right under the row its tile sits in (GNOME-style), rather than
            // replacing the whole grid.
            let mut rows: Vec<(Vec<Element<'_, Message>>, bool)> = Vec::new();
            let mut cur: Vec<Element<'_, Message>> = Vec::new();
            let mut used = 0u32;
            let mut has_expanded = false;
            for inst in &self.instances {
                let cols = inst.size.cols();
                if used + cols > 4 && !cur.is_empty() {
                    rows.push((std::mem::take(&mut cur), has_expanded));
                    used = 0;
                    has_expanded = false;
                }
                let w = inst.width.current();
                cur.push(
                    widget::container(inst.module.view(inst.id, false, w))
                        .width(Length::Fixed(w))
                        .clip(true)
                        .into(),
                );
                used += cols;
                has_expanded |= self.expanded == Some(inst.id);
            }
            if !cur.is_empty() {
                rows.push((cur, has_expanded));
            }

            let mut col = widget::Column::new().spacing(GRID_GAP).width(Length::Fill);
            for (tiles, exp) in rows {
                col = col.push(widget::row(tiles).spacing(GRID_GAP));
                if exp {
                    if let Some(inst) =
                        self.expanded.and_then(|id| self.instances.iter().find(|i| i.id == id))
                    {
                        // Instant: the popup is too heavy to re-render smoothly at
                        // animation framerates (see the perf follow-up issue).
                        col = col.push(self.selection_panel(inst));
                    }
                }
            }
            col.into()
        }
    }

    /// The inline selection list for an expandable tile (Wi-Fi, Bluetooth, VPN):
    /// a header plus the module's current entries, each connectable. Rendered
    /// directly under the tile's row (GNOME-style vertical expansion), so it's a
    /// block-wide drawer rather than a separate page.
    fn selection_panel<'a>(&'a self, inst: &'a Instance) -> Element<'a, Message> {
        let id = inst.id;
        let desc = inst.module.descriptor();
        let mut header = widget::Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(widget::icon::from_name(desc.icon.as_str()).size(18))
            .push(widget::text::body(desc.name.clone()))
            .push(widget::space::horizontal());
        // A spinner while a scan is in flight, a manual refresh, then close.
        if self.expand_loading {
            header = header.push(widget::progress_bar::indeterminate_circular().size(16.0));
        }
        let header = header
            .push(round_btn("view-refresh-symbolic", Message::RefreshEntries(id)))
            .push(round_btn("window-close-symbolic", Message::Expand(id)));

        let entries = inst.module.entries();
        // The entry (if any) awaiting text input expands to show the field.
        let pending = inst.module.pending_input();
        let mut list = widget::Column::new().spacing(4).width(Length::Fill);
        if entries.is_empty() {
            list = list.push(
                widget::container(
                    widget::text::caption("Nothing available")
                        .class(cosmic::style::Text::Custom(theme::dim_text)),
                )
                .padding(8),
            );
        } else {
            for e in &entries {
                // The pending entry renders as one expanded card containing its
                // name plus the password field; the rest stay tappable rows.
                match &pending {
                    Some((key, value)) if key == &e.key => {
                        list = list.push(expanded_entry(e, value.clone(), id));
                    }
                    _ => list = list.push(entry_row(id, e)),
                }
            }
        }

        // Cap the list height so a long scan (many Wi-Fi networks) scrolls
        // instead of ballooning the popup.
        let card = widget::Column::new()
            .spacing(10)
            .push(header)
            .push(widget::container(widget::scrollable(list)).max_height(260.0));
        widget::container(card)
            .padding(12)
            .width(Length::Fill)
            .class(theme::card(false, theme::accent()))
            .into()
    }

    /// Compact layout used by the **applet** popup: power actions, the tile grid,
    /// and the bottom bar, in a centered fixed-width column.
    pub fn view(&self) -> Element<'_, Message> {
        let header = self.power_bar();

        let mut col = widget::Column::new()
            .spacing(16)
            .width(Length::Fill)
            .push(header);
        // Add-module picker sits right under the header (next to the Add button),
        // not buried at the bottom of the page.
        if self.palette_open {
            col = col.push(self.palette());
        }
        if self.edit {
            col = col.push(
                widget::text::caption("Drag to rearrange · ⛶ resizes · × removes")
                    .class(cosmic::style::Text::Custom(theme::dim_text)),
            );
            col = col.push(Self::grid_ruler());
        }
        // The grid injects the expanded selection list inline (under the row of
        // the tile whose chevron is open), so it doesn't appear here.
        col = col.push(self.grid());
        col = col.push(self.bottom_bar());

        // Center the fixed-width 4-column block in the window.
        widget::scrollable(
            widget::container(widget::container(col).max_width(TileSize::Full.width()))
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .into()
    }

    /// The **editor** layout (window app): a persistent left sidebar to add
    /// modules, and the live grid as the arrangement canvas (always in edit
    /// mode). Reuses `grid()` — this is the same grid the applet renders.
    pub fn editor_view(&self) -> Element<'_, Message> {
        let hint = widget::text::caption("Drag to rearrange · ⛶ resizes · × removes")
            .class(cosmic::style::Text::Custom(theme::dim_text));

        // The grid is a fixed 4-column block; cap it with an inner container,
        // then center that block in the (wider) canvas — same nesting the applet
        // view uses (max_width and centering on the SAME container misbehaves).
        let block = widget::container(
            widget::Column::new()
                .spacing(12)
                .push(hint)
                .push(Self::grid_ruler())
                .push(self.grid()),
        )
        .max_width(TileSize::Full.width());
        let canvas = widget::scrollable(
            widget::container(block)
                .padding(24)
                .width(Length::Fill)
                .center_x(Length::Fill),
        );

        let mut row = widget::Row::new()
            .height(Length::Fill)
            .push(self.sidebar())
            .push(widget::container(canvas).width(Length::Fill).height(Length::Fill));
        // Right-hand surface: the app-wide Settings panel takes precedence, else
        // the selected tile's config (its gear sets `config_open`). Only one
        // occupies the slot at a time.
        if self.settings_open {
            row = row.push(self.settings_sidebar());
        } else if let Some(inst) = self
            .config_open
            .and_then(|id| self.instances.iter().find(|i| i.id == id))
        {
            row = row.push(self.config_sidebar(inst));
        }
        row.into()
    }

    /// The editor's right sidebar when app-wide Settings is open: the foundation
    /// for global preferences. First control: the panel applet's icon mode.
    fn settings_sidebar(&self) -> Element<'_, Message> {
        let header = widget::Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(widget::icon::from_name("preferences-system-symbolic").size(18))
            .push(widget::text::title4("Settings"))
            .push(widget::space::horizontal())
            .push(round_btn("window-close-symbolic", Message::ToggleSettings));

        let choices = vec!["Single icon".to_string(), "Status cluster".to_string()];
        let selected = match self.config.settings.applet_icons {
            crate::config::AppletIcons::Single => 0,
            crate::config::AppletIcons::Status => 1,
        };
        let applet_icons = widget::Column::new()
            .spacing(6)
            .push(widget::text::body("Panel applet icons"))
            .push(
                widget::text::caption(
                    "A single control-center icon, or a cluster of live status icons \
                     (Wi-Fi, audio, Bluetooth, …).",
                )
                .class(cosmic::style::Text::Custom(theme::dim_text)),
            )
            .push(widget::dropdown(choices, Some(selected), Message::SetAppletIcons));

        let mut inner = widget::Column::new()
            .spacing(16)
            .push(header)
            .push(widget::divider::horizontal::default())
            .push(applet_icons);

        // The per-indicator toggles only matter in cluster mode.
        if matches!(self.config.settings.applet_icons, crate::config::AppletIcons::Status) {
            let cluster = self.config.settings.cluster;
            let mut toggles = widget::Column::new()
                .spacing(8)
                .push(widget::text::body("Show in cluster"));
            for (icon, label) in crate::config::ClusterIcon::ALL {
                toggles = toggles.push(
                    widget::checkbox(cluster.enabled(icon))
                        .label(label)
                        .on_toggle(move |_| Message::ToggleClusterIcon(icon)),
                );
            }
            inner = inner.push(toggles);
        }

        widget::container(inner)
            .width(Length::Fixed(260.0))
            .height(Length::Fill)
            .padding(16)
            .class(theme::card(false, theme::accent()))
            .into()
    }

    /// The editor's right sidebar: settings for the selected tile. Grows as
    /// modules gain configurable options; for now the disk gauge's mount.
    fn config_sidebar<'a>(&'a self, inst: &'a Instance) -> Element<'a, Message> {
        let id = inst.id;
        let desc = inst.module.descriptor();
        let header = widget::Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(widget::icon::from_name(desc.icon.as_str()).size(18))
            .push(widget::text::title4(desc.name.clone()))
            .push(widget::space::horizontal())
            .push(round_btn("window-close-symbolic", Message::ToggleConfig(id)));

        let mut inner = widget::Column::new()
            .spacing(16)
            .push(header)
            .push(widget::divider::horizontal::default());

        let choices = inst.module.option_choices();
        if choices.is_empty() {
            inner = inner.push(
                widget::text::caption("No settings for this tile.")
                    .class(cosmic::style::Text::Custom(theme::dim_text)),
            );
        } else {
            let picker = widget::dropdown(
                choices,
                Some(inst.module.option_selected()),
                move |i| Message::SetOption(id, i),
            );
            inner = inner.push(
                widget::Column::new()
                    .spacing(6)
                    .push(widget::text::body(inst.module.option_label()))
                    .push(picker),
            );
        }

        widget::container(inner)
            .width(Length::Fixed(260.0))
            .height(Length::Fill)
            .padding(16)
            .class(theme::card(false, theme::accent()))
            .into()
    }

    /// The editor's left sidebar: the "Add a module" list, grouped into
    /// categories (alphabetical within each). Single-instance modules already
    /// placed are shown disabled; disk and divider can always be added again.
    fn sidebar(&self) -> Element<'_, Message> {
        let placed: std::collections::HashSet<String> = self
            .instances
            .iter()
            .map(|i| i.module.descriptor().id.clone())
            .collect();
        let descs = self.available();

        let mut list = widget::Column::new().spacing(4);
        for cat in builtin::CATEGORIES {
            let mut items: Vec<&ModuleDescriptor> =
                descs.iter().filter(|d| builtin::category(&d.id) == cat).collect();
            if items.is_empty() {
                continue;
            }
            items.sort_by_key(|d| d.name.to_lowercase());
            list = list.push(
                widget::container(
                    widget::text::caption(cat)
                        .class(cosmic::style::Text::Custom(theme::dim_text)),
                )
                .padding([8, 10, 2, 10]),
            );
            for d in items {
                let enabled = builtin::allows_multiple(&d.id) || !placed.contains(&d.id);
                list = list.push(sidebar_item(
                    &d.icon,
                    &d.name,
                    Message::AddModule(d.id.clone()),
                    enabled,
                ));
            }
        }

        let inner = widget::Column::new()
            .spacing(12)
            .push(widget::text::title4("Add a module"))
            // Right padding so the scrollbar doesn't overlap the row check marks.
            .push(
                widget::scrollable(widget::container(list).padding([0, 12, 0, 0]))
                    .height(Length::Fill),
            )
            // App-wide settings, pinned at the bottom (the Fill scrollable above
            // pushes it down).
            .push(widget::divider::horizontal::default())
            .push(sidebar_item(
                "preferences-system-symbolic",
                "Settings",
                Message::ToggleSettings,
                true,
            ));
        widget::container(inner)
            .width(Length::Fixed(264.0))
            .height(Length::Fill)
            .padding(16)
            .class(theme::card(false, theme::accent()))
            .into()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleEdit => {
                // Only the editor (window) may enter edit mode.
                if self.allow_edit {
                    self.edit = !self.edit;
                    if !self.edit {
                        self.palette_open = false;
                        self.config_open = None;
                        self.settings_open = false;
                    }
                    self.bump_redraw();
                }
            }
            Message::OpenConfig => {
                // The applet's gear: launch the companion editor window.
                let _ = std::process::Command::new("cosmic-ext-control-center").spawn();
            }
            // Applet-only plumbing — handled by the Applet host, never reaches
            // the editor; arms here keep the match exhaustive.
            Message::Surface(_)
            | Message::PopupClosed(_)
            | Message::Token(_)
            | Message::Status(_) => {}
            Message::OpenPalette => {
                self.palette_open = !self.palette_open;
                self.bump_redraw();
            }
            Message::AddModule(id) => {
                self.add_module(&id);
                self.palette_open = false;
            }
            Message::RemoveInstance(id) => {
                // Animate the tile out (width → 0); the Frame handler frees it
                // once the exit finishes. Persist now so config drops it
                // immediately (persist() skips `removing` tiles).
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                    inst.removing = true;
                    inst.width.set(0.0);
                }
                self.persist();
            }
            Message::ResizeInstance(id) => {
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                    let min = inst.module.min_cols();
                    let mut next = inst.size.toggled();
                    while next.cols() < min {
                        next = next.toggled();
                    }
                    inst.size = next;
                    inst.width.set(next.width()); // animate to the new width
                    self.persist();
                }
            }
            Message::SetOption(id, index) => {
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                    inst.module.set_option(index);
                    self.bump_redraw();
                    self.persist();
                }
            }
            Message::ToggleConfig(id) => {
                self.config_open = if self.config_open == Some(id) { None } else { Some(id) };
                // The two right-sidebar surfaces are mutually exclusive.
                self.settings_open = false;
                self.bump_redraw();
            }
            Message::ToggleSettings => {
                self.settings_open = !self.settings_open;
                if self.settings_open {
                    self.config_open = None;
                }
                self.bump_redraw();
            }
            Message::SetAppletIcons(index) => {
                self.config.settings.applet_icons = match index {
                    1 => crate::config::AppletIcons::Status,
                    _ => crate::config::AppletIcons::Single,
                };
                self.config.save();
                self.bump_redraw();
            }
            Message::ToggleClusterIcon(icon) => {
                self.config.settings.cluster.toggle(icon);
                self.config.save();
                self.bump_redraw();
            }
            Message::Expand(id) => {
                // Toggle the drawer. The `expand` control sets the module's
                // "wants entries" flag; the list is fetched off-thread by the
                // dispatched refresh. Closing eases to 0 then `Frame` clears it.
                if self.expanded == Some(id) && self.expand_open {
                    self.expanded = None;
                    self.expand_open = false;
                    self.expand_loading = false;
                    if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                        let _ = inst.module.on_control("expand", ControlValue::Bool(false));
                    }
                    self.bump_redraw();
                } else if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                    let _ = inst.module.on_control("expand", ControlValue::Bool(true));
                    self.expanded = Some(id);
                    self.expand_open = true;
                    self.expand_loading = true;
                    let task = inst.module.refresh(id);
                    self.bump_redraw();
                    return task;
                }
            }
            Message::RefreshEntries(id) => {
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                    self.expand_loading = true;
                    let task = inst.module.refresh_manual(id);
                    self.bump_redraw();
                    return task;
                }
            }
            Message::Reordered(order) => self.reorder(order),
            Message::Poll => {
                // Collect each module's off-thread fetch into one batch (applied
                // together → a single repaint per poll). Sync, cheap modules
                // (sysmon /proc) have no job and just update themselves in place.
                let mut jobs = Vec::new();
                for i in self.instances.iter_mut() {
                    match i.module.fetch_job() {
                        Some(job) => jobs.push((i.id, job)),
                        None => {
                            let _ = i.module.refresh(i.id);
                        }
                    }
                }
                self.battery = read_battery();
                if jobs.is_empty() {
                    return Task::none();
                }
                return builtin::poll_batch(jobs);
            }
            // The message itself drives a redraw; animated values interpolate
            // off the wall clock in their own view(). Also reap any tile whose
            // exit animation has finished (width settled at 0).
            Message::Frame => {
                self.instances
                    .retain(|i| !(i.removing && !i.width.animating()));
            }
            Message::Power(a) => {
                if a.needs_confirm() {
                    self.pending_power = Some(a);
                } else {
                    a.run();
                }
            }
            Message::PowerConfirm => {
                if let Some(a) = self.pending_power.take() {
                    a.run();
                }
            }
            Message::PowerCancel => self.pending_power = None,
            Message::Control(id, control, value) => {
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                    let task = inst.module.on_control(&control, value);
                    // If this changed the open drawer (e.g. a Wi-Fi password
                    // prompt appeared/closed), repaint for the new layout.
                    if self.expand_open && self.expanded == Some(id) {
                        self.bump_redraw();
                    }
                    return task;
                }
            }
            Message::StateLoaded(id, payload) => {
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                    inst.module.apply_data(payload.0.as_ref());
                }
                // On-demand result (drawer open / manual): stop the spinner.
                if self.expanded == Some(id) {
                    self.expand_loading = false;
                }
            }
            Message::Notify(n) => {
                // Routed to whichever module collects notifications (only the
                // notification center does); harmless no-op for the rest.
                for inst in &mut self.instances {
                    inst.module.ingest_notification(n.clone());
                }
                self.bump_redraw();
            }
            Message::StateBatch(results) => {
                // One poll's worth of results, applied together → a single repaint.
                for (id, payload) in &results {
                    if let Some(inst) = self.instances.iter_mut().find(|i| i.id == *id) {
                        inst.module.apply_data(payload.0.as_ref());
                    }
                }
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // The editor (preview) has no live data, so it never polls — only frame
        // ticks for animations. The applet polls for live module state.
        let mut subs = Vec::new();
        if !builtin::preview() {
            subs.push(time::every(Duration::from_secs(2)).map(|_| Message::Poll));
        }
        // Run frame ticks while something is animating OR a redraw burst is in
        // flight (after a layout-changing toggle) — idle hubs otherwise stay at
        // the 2s poll, no wasted 60fps redraws.
        let bursting = self
            .redraw_until
            .is_some_and(|t| std::time::Instant::now() < t);
        let animating = self.expand_loading // keep ticking so the spinner spins
            || self.instances.iter().any(|i| i.module.animating() || i.width.animating());
        if bursting || animating {
            subs.push(time::every(Duration::from_millis(16)).map(|_| Message::Frame));
        }
        Subscription::batch(subs)
    }
}

impl Hub {
    /// Schedule a brief redraw burst so a layout change fully repaints across
    /// all swapchain buffers (see `redraw_until`).
    fn bump_redraw(&mut self) {
        self.redraw_until = Some(std::time::Instant::now() + Duration::from_millis(300));
    }
}

/// Flags for the editor. Single-instance activation requires `CosmicFlags`; the
/// editor takes no subcommands or args, so the trait defaults apply.
#[derive(Debug, Clone, Default)]
pub struct Flags;

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = String;
    type Args = Vec<String>;
}

/// The standalone window app — the configuration **editor**. A thin
/// `cosmic::Application` shell around a `Hub` with editing enabled. Runs
/// single-instance (see `main.rs`) so a second launch focuses the existing
/// window instead of opening a duplicate.
pub struct App {
    core: Core,
    hub: Hub,
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;
    const APP_ID: &'static str = crate::config::APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Flags) -> (Self, Task<Message>) {
        // The editor is a data-free layout surface — no live fetches/polling.
        builtin::set_preview(true);
        (App { core, hub: Hub::new(true) }, Task::none())
    }

    fn view(&self) -> Element<'_, Message> {
        self.hub.editor_view()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.hub.update(message)
    }

    fn subscription(&self) -> Subscription<Message> {
        self.hub.subscription()
    }

    /// A second launch activated this instance; raise our window. On Wayland this
    /// only takes effect when the new launch passed an activation token (the app
    /// launcher does; a bare `Command::spawn`, e.g. the applet gear, does not).
    fn dbus_activation(&mut self, _msg: cosmic::dbus_activation::Message) -> Task<Message> {
        self.core
            .main_window_id()
            .map(cosmic::iced::window::gain_focus)
            .unwrap_or_else(Task::none)
    }
}

/// A small circular icon button for the edit-mode resize/remove controls, so
/// they read clearly as buttons. Built with `button::custom` (a real button →
/// reliably rendered + hover feedback) and a fully-round custom style.
fn round_btn<'a>(icon: &str, msg: Message) -> Element<'a, Message> {
    let style = |bg: f32| {
        move |_focused: bool, t: &cosmic::Theme| {
            // Foreground from the theme so the glyph/label stay legible in light
            // mode (white-on-light was invisible).
            let fg: cosmic::iced::Color = t.cosmic().background.on.into();
            let mut s = cosmic::widget::button::Style::new();
            s.background = Some(cosmic::iced::Background::Color(theme::alpha(fg, bg)));
            s.border_radius = 12.0.into();
            s.icon_color = Some(fg);
            s.text_color = Some(fg);
            s
        }
    };
    // `button::icon` is cosmic's standard icon button — it reliably centres the
    // glyph (the manual `button::custom` layout did not). It renders the icon at
    // a fixed 16px; padding 4 on every side makes a 24×24 square, so the rounded
    // style (radius 12) is a true circle.
    widget::button::icon(widget::icon::from_name(icon).size(16))
        .padding(4)
        .class(cosmic::theme::Button::Custom {
            active: Box::new(style(0.16)),
            disabled: Box::new(move |t| style(0.10)(false, t)),
            hovered: Box::new(style(0.30)),
            pressed: Box::new(style(0.38)),
        })
        .on_press(msg)
        .into()
}

/// Read the **system** battery from sysfs as (percent, charging?). Returns
/// `None` on desktops (no system `power_supply` of type `Battery`).
///
/// Skips peripheral batteries (wireless mouse/keyboard/controller), which also
/// report `type=Battery` but are `scope=Device` and often read 0% — picking one
/// of those was why the readout disagreed with COSMIC's battery applet.
fn read_battery() -> Option<(u8, bool)> {
    let read = |p: &std::path::Path, f: &str| std::fs::read_to_string(p.join(f));
    let mut fallback: Option<(u8, bool)> = None;
    for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let path = entry.path();
        if read(&path, "type").unwrap_or_default().trim() != "Battery" {
            continue;
        }
        // Peripherals are scope=Device; the laptop battery is System or absent.
        if read(&path, "scope").unwrap_or_default().trim() == "Device" {
            continue;
        }
        let Some(pct) = battery_pct(&path) else {
            continue;
        };
        let charging = matches!(read(&path, "status").unwrap_or_default().trim(), "Charging" | "Full");
        // Prefer the conventional BAT* main battery; otherwise keep the first
        // non-peripheral battery (covers names like CMB0, macsmc-battery).
        if entry.file_name().to_string_lossy().starts_with("BAT") {
            return Some((pct, charging));
        }
        fallback.get_or_insert((pct, charging));
    }
    fallback
}

/// Battery percentage from `capacity`, or computed from `energy_*`/`charge_*`
/// when that file is absent (some firmware doesn't expose `capacity`).
fn battery_pct(path: &std::path::Path) -> Option<u8> {
    let read = |f: &str| std::fs::read_to_string(path.join(f));
    if let Ok(cap) = read("capacity") {
        if let Ok(p) = cap.trim().parse::<u8>() {
            return Some(p.min(100));
        }
    }
    for (now, full) in [("energy_now", "energy_full"), ("charge_now", "charge_full")] {
        if let (Ok(n), Ok(f)) = (read(now), read(full)) {
            if let (Ok(n), Ok(f)) = (n.trim().parse::<f64>(), f.trim().parse::<f64>()) {
                if f > 0.0 {
                    return Some((n / f * 100.0).round().clamp(0.0, 100.0) as u8);
                }
            }
        }
    }
    None
}

/// Pick a freedesktop battery icon for a charge level + charging state.
fn battery_icon(pct: u8, charging: bool) -> &'static str {
    if charging {
        return "battery-good-charging-symbolic";
    }
    match pct {
        90..=100 => "battery-full-symbolic",
        55..=89 => "battery-good-symbolic",
        25..=54 => "battery-low-symbolic",
        10..=24 => "battery-caution-symbolic",
        _ => "battery-empty-symbolic",
    }
}
