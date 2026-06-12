//! Declarative plugin modules. A plugin is a RON manifest file describing a
//! tile: an id/name/icon/size plus a list of controls, each with a command to
//! read state (`get`) and/or act (`set`/`run`). `ManifestModule` implements
//! `Module` once, interpreting any manifest — so third parties add tiles with
//! zero Rust and no recompile.
//!
//! SECURITY: `Cmd` actions run with the user's privileges via `sh -c`. Plugins
//! are trusted artefacts the user installs (same model as AUR helpers/scripts).
//!
//! v1 runs commands synchronously (fast ones are fine); a later pass moves
//! polling/get to async tasks so the UI never blocks.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone, Deserialize)]
pub enum Action {
    /// Shell command. In `set`/`run`, `{value}` is substituted; in `get`, the
    /// command's stdout is parsed per control type.
    Cmd(String),
}

#[derive(Debug, Clone, Deserialize)]
pub enum Control {
    Slider {
        id: String,
        label: String,
        #[serde(default)]
        min: f64,
        #[serde(default = "default_max")]
        max: f64,
        #[serde(default)]
        get: Option<Action>,
        #[serde(default)]
        set: Option<Action>,
        #[serde(default)]
        poll: Option<f64>,
    },
    Toggle {
        id: String,
        label: String,
        #[serde(default)]
        get: Option<Action>,
        #[serde(default)]
        set: Option<Action>,
        #[serde(default)]
        poll: Option<f64>,
    },
    Label {
        id: String,
        label: String,
        get: Action,
        #[serde(default)]
        poll: Option<f64>,
    },
    Button {
        id: String,
        label: String,
        run: Action,
    },
}

fn default_max() -> f64 {
    100.0
}
fn default_size() -> TileSize {
    TileSize::Medium
}

impl Control {
    pub fn id(&self) -> &str {
        match self {
            Control::Slider { id, .. }
            | Control::Toggle { id, .. }
            | Control::Label { id, .. }
            | Control::Button { id, .. } => id,
        }
    }
    fn polls(&self) -> bool {
        matches!(
            self,
            Control::Slider { poll: Some(_), .. }
                | Control::Toggle { poll: Some(_), .. }
                | Control::Label { poll: Some(_), .. }
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub icon: String,
    #[serde(default = "default_size")]
    pub size: TileSize,
    /// Whether the user may resize this tile in edit mode. Defaults to true.
    #[serde(default = "default_true")]
    pub resizable: bool,
    pub controls: Vec<Control>,
}

fn default_true() -> bool {
    true
}

impl Manifest {
    /// Parse a manifest, allowing bare `Some` values (`get: Cmd("..")` instead
    /// of `get: Some(Cmd(".."))`).
    pub fn parse(s: &str) -> Result<Manifest, ron::error::SpannedError> {
        let opts = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        opts.from_str::<Manifest>(s)
    }

    pub fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: self.id.clone(),
            name: self.name.clone(),
            icon: self.icon.clone(),
            size: self.size,
            resizable: self.resizable,
        }
    }
}

// --- command execution ---

fn run_get(cmd: &str) -> Option<String> {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn run_set(template: &str, value: &str) {
    let cmd = template.replace("{value}", value);
    let _ = Command::new("sh").arg("-c").arg(cmd).spawn();
}

fn parse_bool(s: &str) -> bool {
    matches!(s.trim(), "true" | "True" | "1" | "on" | "yes")
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

// --- the module ---

pub struct ManifestModule {
    desc: ModuleDescriptor,
    controls: Vec<Control>,
    /// Current state by control id.
    values: HashMap<String, ControlValue>,
}

impl ManifestModule {
    pub fn from_manifest(m: &Manifest) -> Self {
        let mut me = Self {
            desc: m.descriptor(),
            controls: m.controls.clone(),
            values: HashMap::new(),
        };
        // Seed initial state from every `get`.
        let controls = me.controls.clone();
        for c in &controls {
            me.read_control(c);
        }
        me
    }

    /// Run a control's `get` and store the parsed value.
    fn read_control(&mut self, c: &Control) {
        match c {
            Control::Slider { id, get, .. } => {
                if let Some(Action::Cmd(cmd)) = get {
                    if let Some(o) = run_get(cmd) {
                        if let Ok(v) = o.split_whitespace().next().unwrap_or(&o).parse::<f64>() {
                            self.values.insert(id.clone(), ControlValue::Float(v));
                        }
                    }
                }
            }
            Control::Toggle { id, get, .. } => {
                if let Some(Action::Cmd(cmd)) = get {
                    if let Some(o) = run_get(cmd) {
                        self.values.insert(id.clone(), ControlValue::Bool(parse_bool(&o)));
                    }
                }
            }
            Control::Label { id, get: Action::Cmd(cmd), .. } => {
                if let Some(o) = run_get(cmd) {
                    self.values.insert(id.clone(), ControlValue::Text(o));
                }
            }
            Control::Button { .. } => {}
        }
    }

    fn float(&self, id: &str, default: f64) -> f64 {
        match self.values.get(id) {
            Some(ControlValue::Float(v)) => *v,
            _ => default,
        }
    }
    fn bool(&self, id: &str) -> bool {
        matches!(self.values.get(id), Some(ControlValue::Bool(true)))
    }
    fn text(&self, id: &str) -> String {
        match self.values.get(id) {
            Some(ControlValue::Text(t)) => t.clone(),
            _ => "—".to_string(),
        }
    }

    fn control_view(&self, inst: InstanceId, c: &Control, edit: bool) -> Element<'_, Message> {
        match c {
            Control::Slider { id, label, min, max, .. } => {
                let v = self.float(id, *min) as f32;
                let header = widget::Row::new()
                    .push(widget::text::caption(label.clone()))
                    .push(widget::space::horizontal())
                    .push(widget::text::caption(fmt_num(v as f64)));
                // Inert progress bar while editing.
                let control: Element<'_, Message> = if edit {
                    let norm = (((v as f64) - min) / (max - min).max(1e-9)).clamp(0.0, 1.0);
                    widget::container(widget::progress_bar::determinate_linear(norm as f32))
                        .width(Length::Fill)
                        .into()
                } else {
                    let cid = id.clone();
                    let step = (((max - min) / 100.0).max(1e-3)) as f32;
                    widget::slider(*min as f32..=*max as f32, v, move |nv| {
                        Message::Control(inst, cid.clone(), ControlValue::Float(nv as f64))
                    })
                    .step(step)
                    .width(Length::Fill)
                    .into()
                };
                widget::Column::new().spacing(4).push(header).push(control).into()
            }
            Control::Toggle { id, label, .. } => {
                let mut t = widget::toggler(self.bool(id));
                if !edit {
                    let cid = id.clone();
                    t = t.on_toggle(move |b| {
                        Message::Control(inst, cid.clone(), ControlValue::Bool(b))
                    });
                }
                widget::settings::item(label.clone(), t).into()
            }
            Control::Label { id, label, .. } => widget::Row::new()
                .spacing(8)
                .push(widget::text::caption(label.clone()))
                .push(widget::space::horizontal())
                .push(widget::text::body(self.text(id)))
                .into(),
            Control::Button { id, label, .. } => {
                let mut b = widget::button::standard(label.clone()).width(Length::Fill);
                if !edit {
                    let cid = id.clone();
                    b = b.on_press(Message::Control(inst, cid.clone(), ControlValue::Trigger));
                }
                b.into()
            }
        }
    }
}

impl Module for ManifestModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, inst: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        let mut col = widget::Column::new().spacing(10).push(
            widget::Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(widget::icon::from_name(self.desc.icon.as_str()).size(18))
                .push(widget::text::body(self.desc.name.clone())),
        );
        for c in &self.controls {
            col = col.push(self.control_view(inst, c, edit));
        }
        crate::module::builtin::tile(width, false, col)
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        if let Some(c) = self.controls.iter().find(|c| c.id() == control).cloned() {
            match (&c, &value) {
                (Control::Slider { set: Some(Action::Cmd(t)), .. }, ControlValue::Float(v)) => {
                    run_set(t, &fmt_num(*v));
                    self.values.insert(control.to_string(), value);
                }
                (Control::Toggle { set: Some(Action::Cmd(t)), .. }, ControlValue::Bool(b)) => {
                    run_set(t, if *b { "true" } else { "false" });
                    self.values.insert(control.to_string(), value);
                }
                (Control::Button { run: Action::Cmd(t), .. }, _) => {
                    run_set(t, "");
                }
                _ => {}
            }
        }
        Task::none()
    }

    fn refresh(&mut self) -> Task<Message> {
        let controls = self.controls.clone();
        for c in &controls {
            if c.polls() {
                self.read_control(c);
            }
        }
        Task::none()
    }
}
