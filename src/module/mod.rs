//! The module abstraction. Every tile in the hub — built-in (Rust) or
//! plugin (manifest) — is a `Module`. The hub stores `Box<dyn Module>` and
//! routes interactions by `InstanceId`, so it never needs to know which kind
//! a tile is.

pub mod builtin;
pub mod manifest;

use crate::app::Message;
use cosmic::app::Task;
use cosmic::iced::Subscription;
use cosmic::prelude::*;
use std::time::{Duration, Instant};

/// Unique id for a *placed* tile (one module type can be placed many times).
pub type InstanceId = u32;

/// An opaque, thread-safe payload carrying a module's freshly-fetched data back
/// from a background refresh to the UI thread (delivered via
/// `Message::StateLoaded`, applied by `Module::apply`). Each module boxes its
/// own data type and downcasts it on the way in. The manual `Debug`/`Clone`
/// keep `Message`'s derives working (`dyn Any` is neither).
#[derive(Clone)]
pub struct Payload(pub std::sync::Arc<dyn std::any::Any + Send + Sync>);

impl Payload {
    pub fn new<T: std::any::Any + Send + Sync>(data: T) -> Self {
        Payload(std::sync::Arc::new(data))
    }
}

impl std::fmt::Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Payload(..)")
    }
}

/// How long a `ValueAnim` takes to ease to a new target. Snappy but visible.
const ANIM_DUR: Duration = Duration::from_millis(260);

/// A smoothly-animated f32, built on `cosmic::anim`. Call `set` with a new
/// target (e.g. each poll); `current()` returns the eased value for `view`
/// (pure — no mutation), and `animating()` says whether a sweep is in flight
/// (used to gate the hub's frame ticks).
#[derive(Default)]
pub struct ValueAnim {
    from: f32,
    target: f32,
    state: cosmic::anim::State,
}

impl ValueAnim {
    pub fn new(v: f32) -> Self {
        Self {
            from: v,
            target: v,
            state: cosmic::anim::State::default(),
        }
    }

    pub fn set(&mut self, v: f32) {
        if (v - self.target).abs() > 0.001 {
            // Capture the currently-displayed value (handles re-targeting mid
            // animation), then restart the clock. Stamp `last_change` directly
            // rather than `State::changed()`, which is built for reversible
            // toggles and mis-seeds the clock once a prior anim has elapsed
            // (causing the next one to read as instantly done).
            self.from = self.current();
            self.target = v;
            self.state.last_change = Some(Instant::now());
        }
    }

    pub fn current(&self) -> f32 {
        let t = cosmic::anim::smootherstep(self.state.t(ANIM_DUR, true).clamp(0.0, 1.0));
        cosmic::anim::lerp(self.from, self.target, t)
    }

    pub fn animating(&self) -> bool {
        self.state
            .last_change
            .is_some_and(|t| Instant::now().duration_since(t) < ANIM_DUR)
    }
}

/// Gap between tiles in the grid (px). Tile widths are unit multiples of this
/// gap + a base column unit, so tiles align into clean columns.
pub const GRID_GAP: u16 = 10;
/// One grid column = this px. The grid is a fixed 4-column block (Full spans
/// all four), centered in the window — the standard control-center popover.
pub const GRID_UNIT: f32 = 100.0;

/// Tiles span 1–4 grid columns. Resizing in edit mode cycles through them, so
/// the user gets incremental control rather than a single normal/wide flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TileSize {
    Small,
    Medium,
    Large,
    Full,
}

impl TileSize {
    pub fn cols(self) -> u32 {
        match self {
            TileSize::Small => 1,
            TileSize::Medium => 2,
            TileSize::Large => 3,
            TileSize::Full => 4,
        }
    }

    /// Width in px: N columns of `GRID_UNIT` joined by `GRID_GAP` gaps.
    pub fn width(self) -> f32 {
        let c = self.cols() as f32;
        c * GRID_UNIT + (c - 1.0) * GRID_GAP as f32
    }

    /// Next size up, wrapping Full → Small — the edit-mode resize cycle.
    pub fn toggled(self) -> Self {
        match self {
            TileSize::Small => TileSize::Medium,
            TileSize::Medium => TileSize::Large,
            TileSize::Large => TileSize::Full,
            TileSize::Full => TileSize::Small,
        }
    }
}

/// Recover a tile's column span (1–4) from its resolved pixel width — lets a
/// module's `view(.., width)` pick a layout variant for its size.
pub fn cols_for_width(width: f32) -> u32 {
    let step = GRID_UNIT + GRID_GAP as f32;
    (((width + GRID_GAP as f32) / step).round() as u32).clamp(1, 4)
}

/// A value flowing between a control and its module.
#[derive(Debug, Clone)]
pub enum ControlValue {
    Bool(bool),
    Float(f64),
    Trigger,
    Text(String),
}

/// One selectable target in a module's expandable list (a Wi-Fi network, a
/// Bluetooth device, a VPN profile). `key` is passed back to `on_control` as the
/// "select" value; `active` marks the currently-connected one.
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub key: String,
    pub label: String,
    pub detail: String,
    pub active: bool,
}

/// Identity + presentation of a module type.
#[derive(Debug, Clone)]
pub struct ModuleDescriptor {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// Default size when first placed.
    pub size: TileSize,
    /// Whether the user may resize placed instances (Normal ↔ Wide) in edit
    /// mode. Some modules (e.g. sliders) need the width and opt out.
    pub resizable: bool,
}

/// A live, placed module. Built-in modules implement this directly; the
/// manifest plugin loader (phase 2) implements it once over any manifest.
pub trait Module {
    fn descriptor(&self) -> &ModuleDescriptor;

    /// The tile body. `edit` tells the module to render controls inert (so a
    /// reorder drag isn't swallowed by a slider); `width` is the tile's
    /// resolved pixel width (the hub computes it from the instance's column
    /// span and the available width — a responsive 4-column grid). The hub
    /// draws the remove/resize buttons itself.
    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message>;

    /// The user changed one of this module's controls. (Named `on_control`,
    /// not `apply`, to avoid colliding with the `Apply` trait that
    /// `cosmic::prelude` brings into scope as a blanket impl.)
    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message>;

    /// Minimum column span this module is useful at. The edit-mode resize cycle
    /// skips sizes below this (e.g. Media needs ≥2 to show art + controls).
    fn min_cols(&self) -> u32 {
        1
    }

    /// Whether the module has an in-flight animation. The hub runs ~60fps frame
    /// ticks only while at least one module reports true, so idle is cheap.
    fn animating(&self) -> bool {
        false
    }

    /// Live data sources (e.g. fan RPM). Default: none.
    fn subscription(&self, _id: InstanceId) -> Subscription<Message> {
        Subscription::none()
    }

    /// Poll current state. A module with cheap I/O (e.g. sysmon reading /proc)
    /// may update itself synchronously here and return `Task::none()`; one with
    /// blocking I/O (subprocess, D-Bus) should instead return a `Task` that runs
    /// the work off the UI thread (see `builtin::fetch_task`) and delivers the
    /// result to `apply` via `Message::StateLoaded`. `id` tags that result.
    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
        Task::none()
    }

    /// A user-initiated refresh (the drawer's refresh button). Defaults to the
    /// normal `refresh`; modules can override to do a deeper scan — e.g. Wi-Fi
    /// forces a real rescan instead of re-reading the cached network list.
    fn refresh_manual(&mut self, id: InstanceId) -> Task<Message> {
        self.refresh(id)
    }

    /// Apply data produced by a background `refresh`. The module downcasts the
    /// payload to its own data type and updates its cached state. Default: no-op
    /// (synchronous modules don't use this path). Named `apply_data`, not
    /// `apply`, to dodge the `Apply` blanket method `cosmic::prelude` adds.
    fn apply_data(&mut self, _data: &dyn std::any::Any) {}

    /// Per-instance settings to persist (e.g. {"mount": "/home"}). Empty means
    /// the module is stateless and the same wherever it's placed.
    fn params(&self) -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::new()
    }

    /// Editor picker: the selectable values for this tile's one configurable
    /// option (e.g. the list of disk mounts). Empty means no picker is shown.
    fn option_choices(&self) -> Vec<String> {
        Vec::new()
    }

    /// Index (into `option_choices`) of the currently-selected value.
    fn option_selected(&self) -> usize {
        0
    }

    /// Apply a picker choice (an index into `option_choices`).
    fn set_option(&mut self, _index: usize) {}

    /// Whether this module's tile chevron opens an inline selection list (Wi-Fi
    /// networks, Bluetooth devices, VPN profiles) rather than external settings.
    fn expandable(&self) -> bool {
        false
    }

    /// The current selectable targets for the expanded list. Populated when the
    /// hub sends the `expand` control (so the scan only runs on demand); a
    /// `select` control with an entry's `key` acts on it.
    fn entries(&self) -> Vec<ListEntry> {
        Vec::new()
    }

    /// While the module awaits text input (e.g. a Wi-Fi password for a new
    /// secured network), the (entry key, current value) — the key identifies
    /// which list entry to expand the field under. When `Some`, that row shows a
    /// secure field; edits arrive as the "input" control, confirm as "submit",
    /// dismiss as "cancel".
    fn pending_input(&self) -> Option<(String, String)> {
        None
    }
}
