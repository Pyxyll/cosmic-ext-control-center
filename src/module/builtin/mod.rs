//! Built-in (Rust) modules + their registry. Each implements `Module`
//! directly, so they get full libcosmic power. The registry exposes their
//! descriptors (for the "add" palette) and a factory to instantiate by id.

mod airplane;
mod appearance;
mod bluetooth;
mod divider;
mod media;
mod microphone;
mod mpris;
mod power_profile;
mod sysmon;
mod volume;
mod vpn;
mod wifi;

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, cols_for_width};
use crate::theme;
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;
use std::process::Command;
pub use airplane::AirplaneModule;
pub use appearance::AppearanceModule;
pub use bluetooth::BluetoothModule;
pub use divider::DividerModule;
pub use media::MediaModule;
pub use microphone::MicrophoneModule;
pub use power_profile::PowerProfileModule;
pub use sysmon::{CpuModule, DiskModule, GpuModule, RamModule, SysMonModule};
pub use volume::VolumeModule;
pub use vpn::VpnModule;
pub use wifi::WifiModule;

/// Descriptors for every built-in, for the add-module palette.
pub fn descriptors() -> Vec<ModuleDescriptor> {
    vec![
        VolumeModule::new().descriptor().clone(),
        MicrophoneModule::new().descriptor().clone(),
        MediaModule::new().descriptor().clone(),
        MediaModule::new_framed().descriptor().clone(),
        PowerProfileModule::new().descriptor().clone(),
        WifiModule::new().descriptor().clone(),
        VpnModule::new().descriptor().clone(),
        BluetoothModule::new().descriptor().clone(),
        AirplaneModule::new().descriptor().clone(),
        AppearanceModule::new().descriptor().clone(),
        DividerModule::new().descriptor().clone(),
        CpuModule::new().descriptor().clone(),
        GpuModule::new().descriptor().clone(),
        RamModule::new().descriptor().clone(),
        DiskModule::new().descriptor().clone(),
        SysMonModule::new().descriptor().clone(),
    ]
}

/// Instantiate a built-in by its module id, seeding any saved per-instance
/// params (e.g. a disk gauge's mount). Returns None for unknown ids.
pub fn make(id: &str, params: &std::collections::BTreeMap<String, String>) -> Option<Box<dyn Module>> {
    match id {
        "builtin.volume" => Some(Box::new(VolumeModule::new())),
        "builtin.microphone" => Some(Box::new(MicrophoneModule::new())),
        "builtin.media" => Some(Box::new(MediaModule::new())),
        "builtin.media_art" => Some(Box::new(MediaModule::new_framed())),
        "builtin.power_profile" => Some(Box::new(PowerProfileModule::new())),
        "builtin.wifi" => Some(Box::new(WifiModule::new())),
        "builtin.vpn" => Some(Box::new(VpnModule::new())),
        "builtin.bluetooth" => Some(Box::new(BluetoothModule::new())),
        "builtin.airplane" => Some(Box::new(AirplaneModule::new())),
        "builtin.appearance" => Some(Box::new(AppearanceModule::new())),
        "builtin.divider" => Some(Box::new(DividerModule::new())),
        "builtin.cpu" => Some(Box::new(CpuModule::new())),
        "builtin.gpu" => Some(Box::new(GpuModule::new())),
        "builtin.ram" => Some(Box::new(RamModule::new())),
        "builtin.disk" => Some(Box::new(DiskModule::with_mount(
            params.get("mount").cloned().unwrap_or_else(|| "/".to_string()),
        ))),
        "builtin.sysmon" => Some(Box::new(SysMonModule::new())),
        _ => None,
    }
}

/// Shared tile chrome: a sized card, tinted with the accent when `active`.
pub(crate) fn tile<'a>(
    width: f32,
    active: bool,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let accent = theme::ACCENTS[0].1; // cerise
    widget::container(content)
        .padding(14)
        .width(Length::Fixed(width))
        .class(theme::card(active, accent))
        .into()
}

/// What the split pill's right-hand chevron does.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Chevron {
    /// No chevron (toggle only).
    None,
    /// Opens the module's external settings (sends the "settings" control).
    Settings,
    /// Expands an inline selection list (Wi-Fi networks, devices, profiles).
    Expand,
}

/// GNOME-style quick-toggle pill: the whole left area toggles the module; an
/// optional smaller right segment opens its settings or an inline list. The tile
/// is tinted with the accent when on. Controls go inert in edit mode (so the
/// reorder drag isn't swallowed). Modules handle the "on" / "settings" controls.
pub(crate) fn toggle_tile<'a>(
    id: InstanceId,
    width: f32,
    on: bool,
    edit: bool,
    icon: &str,
    label: &str,
    status: &str,
    chevron: Chevron,
) -> Element<'a, Message> {
    let accent = theme::ACCENTS[0].1;
    // A definite tile height. The full-height divider uses `Length::Fill`, and
    // an *unbounded* Fill collapses the tile's measured height inside flex_row
    // (which made the next row draw on top of it). A fixed card height bounds
    // the Fill so measurement — and the layout — stay correct.
    const H: f32 = 64.0;

    // 1col: there's no room for the label/status/chevron pill, so collapse to a
    // centered icon that toggles on tap (still tinted when on). Settings aren't
    // reachable here — that's the 2col+ layout.
    if cols_for_width(width) <= 1 {
        let mut ma = widget::mouse_area(
            widget::container(widget::icon::from_name(icon).size(26))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        );
        if !edit {
            ma = ma.on_press(Message::Control(id, "on".into(), ControlValue::Bool(!on)));
        }
        return widget::container(ma)
            .width(Length::Fixed(width))
            .height(Length::Fixed(H))
            .class(theme::card(on, accent))
            .into();
    }

    let info = widget::Row::new()
        .spacing(10)
        .align_y(Alignment::Center)
        .push(widget::icon::from_name(icon).size(22))
        .push(
            widget::Column::new()
                .spacing(1)
                .push(widget::text::body(label.to_string()))
                .push(widget::text::caption(status.to_string())),
        );
    let mut left = widget::mouse_area(
        widget::container(info)
            .width(Length::Fill)
            .center_y(Length::Fill)
            .padding([0, 14]),
    );
    if !edit {
        left = left.on_press(Message::Control(id, "on".into(), ControlValue::Bool(!on)));
    }

    let mut row = widget::Row::new()
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .push(left);
    if chevron != Chevron::None {
        // A full-height divider in the window background colour separates the
        // toggle area from the chevron — reads as a gap between the two halves of
        // the split pill.
        row = row.push(
            widget::container(widget::Space::new())
                .width(Length::Fixed(2.0))
                .height(Length::Fill)
                .class(theme::divider_gap()),
        );
        // Expand uses a down chevron (reveals the list); Settings keeps the
        // forward chevron (leaves to an external page).
        let glyph = if chevron == Chevron::Expand {
            "pan-down-symbolic"
        } else {
            "go-next-symbolic"
        };
        let mut chev =
            widget::button::icon(widget::icon::from_name(glyph).size(16)).padding(14);
        if !edit {
            let msg = match chevron {
                Chevron::Expand => Message::Expand(id),
                _ => Message::Control(id, "settings".into(), ControlValue::Trigger),
            };
            chev = chev.on_press(msg);
        }
        row = row.push(chev);
    }

    widget::container(row)
        .width(Length::Fixed(width))
        .height(Length::Fixed(H))
        .class(theme::card(on, accent))
        .into()
}

/// The leading icon for a level control. When `mute = Some((control, muted))`
/// the icon is a button that toggles mute (the caller passes a glyph already
/// reflecting the muted state); otherwise it's a static icon.
fn level_icon<'a>(
    id: InstanceId,
    icon: &str,
    mute: Option<(&'static str, bool)>,
    edit: bool,
    size: u16,
) -> Element<'a, Message> {
    match (mute, edit) {
        (Some((ctrl, muted)), false) => {
            // `button::icon` ignores the handle size (renders its fixed 16px), so
            // use `button::custom` with the Icon style to honor `size`.
            widget::button::custom(widget::icon::from_name(icon).size(size))
                .class(cosmic::theme::Button::Icon)
                .padding(0)
                .on_press(Message::Control(id, ctrl.into(), ControlValue::Bool(!muted)))
                .into()
        }
        _ => widget::container(widget::icon::from_name(icon).size(size)).into(),
    }
}

/// A level control (volume / mic / brightness): 1col = compact icon + %, 2col+
/// = a single inline row of icon + slider + %. The icon doubles as a mute
/// toggle when `mute` is set. Inert in edit mode. Intentionally has **no card
/// background** — it sits directly on the hub surface.
pub(crate) fn slider_tile<'a>(
    id: InstanceId,
    width: f32,
    value: f32,
    icon: &str,
    pct: String,
    value_control: &'static str,
    mute: Option<(&'static str, bool)>,
    edit: bool,
) -> Element<'a, Message> {
    let cols = cols_for_width(width);

    // Bare wrapper: keeps the grid width + horizontal padding, but no card and
    // only a little vertical padding (these are bgless rows — full card padding
    // made stacked sliders feel far apart).
    let bare = |content: Element<'a, Message>| -> Element<'a, Message> {
        widget::container(content)
            .padding([4, 14])
            .width(Length::Fixed(width))
            .into()
    };

    // Compact (1col): just the icon (mute toggle) and the value.
    if cols <= 1 {
        return bare(
            widget::Column::new()
                .spacing(4)
                .align_x(Alignment::Center)
                .push(level_icon(id, icon, mute, edit, 22))
                .push(widget::text::caption(pct))
                .into(),
        );
    }

    let on_change = move |v: f32| {
        Message::Control(id, value_control.into(), ControlValue::Float(v as f64))
    };

    // 2col+: a single inline row — icon, slider (fills), then the percentage.
    let s: Element<'a, Message> = if edit {
        // Width/girth must go on the Linear itself; it defaults to a fixed 100px
        // width + thin girth, and a container wrapper does NOT stretch it.
        widget::progress_bar::determinate_linear(value)
            .width(Length::Fill)
            .girth(Length::Fixed(6.0))
            .into()
    } else {
        widget::slider(0.0..=1.0, value, on_change)
            .step(0.01)
            .width(Length::Fill)
            .into()
    };
    bare(
        widget::Row::new()
            .spacing(12)
            .align_y(Alignment::Center)
            .push(level_icon(id, icon, mute, edit, 24))
            .push(s)
            .push(widget::text::caption(pct))
            .into(),
    )
}

// --- preview mode ---------------------------------------------------------
// The editor is a layout surface — it doesn't need live data, just the form of
// each tile. In preview mode every external query/action is a no-op, so modules
// render their default state with zero subprocess/D-Bus/image work (the editor
// was painfully slow doing the real fetches on build + every poll).

use std::sync::atomic::{AtomicBool, Ordering};

static PREVIEW: AtomicBool = AtomicBool::new(false);

/// Enable preview (data-free) mode for this process. The editor sets this; the
/// applet leaves it off so it shows live data.
pub fn set_preview(on: bool) {
    PREVIEW.store(on, Ordering::Relaxed);
}

/// Whether this process is a data-free preview (the editor).
pub fn preview() -> bool {
    PREVIEW.load(Ordering::Relaxed)
}

// --- shared command helpers (synchronous; commands here are fast) ---

/// Run a command, return trimmed stdout on success. No-op in preview mode.
pub(crate) fn out(cmd: &str) -> Option<String> {
    if preview() {
        return None;
    }
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Fire a command, ignoring output (fire-and-forget). No-op in preview mode.
pub(crate) fn run(cmd: &str) {
    if preview() {
        return;
    }
    let _ = Command::new("sh").arg("-c").arg(cmd).spawn();
}
