//! System monitor metrics as **separate** single-gauge modules: CPU, GPU, RAM.
//! Each is its own tile so the user can place/size them independently. They
//! share the cheap file-based readers below (no process spawns).
//!   - CPU: delta of /proc/stat idle vs total between polls
//!   - RAM: /proc/meminfo (1 - MemAvailable/MemTotal)
//!   - GPU: amdgpu's /sys/class/drm/card*/device/gpu_busy_percent

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize, ValueAnim};
use crate::theme;
use crate::widgets::gauge::gauge_svg;
use cosmic::app::Task;
use cosmic::iced::widget::Stack;
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;
use std::fs;
use std::path::{Path, PathBuf};

// --- shared readers ---

fn read_cpu_times() -> Option<(u64, u64)> {
    let s = fs::read_to_string("/proc/stat").ok()?;
    let line = s.lines().next()?;
    let mut it = line.split_whitespace();
    if it.next()? != "cpu" {
        return None;
    }
    let vals: Vec<u64> = it.filter_map(|x| x.parse().ok()).collect();
    if vals.len() < 4 {
        return None;
    }
    let idle = vals[3] + vals.get(4).copied().unwrap_or(0);
    let total: u64 = vals.iter().sum();
    Some((total, idle))
}

fn read_ram() -> Option<f32> {
    let s = fs::read_to_string("/proc/meminfo").ok()?;
    let (mut total, mut avail) = (0u64, 0u64);
    for l in s.lines() {
        if let Some(v) = l.strip_prefix("MemTotal:") {
            total = v.split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = l.strip_prefix("MemAvailable:") {
            avail = v.split_whitespace().next()?.parse().ok()?;
        }
    }
    (total != 0).then(|| (total.saturating_sub(avail) as f32 / total as f32).clamp(0.0, 1.0))
}

fn find_gpu() -> Option<PathBuf> {
    for entry in fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let p = entry.path().join("device/gpu_busy_percent");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn read_gpu(path: &Path) -> Option<f32> {
    fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<f32>()
        .ok()
        .map(|v| (v / 100.0).clamp(0.0, 1.0))
}

/// Shared tile body: a centered square gauge (SVG dial + native text overlay).
fn gauge_tile<'a>(width: f32, value: f32, label: &str) -> Element<'a, Message> {
    let accent = theme::ACCENTS[0].1;
    // A definite square, capped to the tile's content width so it fits a 1col
    // tile. The dial is an SVG image (positions correctly in the applet popup,
    // unlike a canvas) with the percentage + label overlaid as native text.
    let side = (width - 28.0).clamp(1.0, 104.0);
    let dial = widget::svg(widget::svg::Handle::from_memory(
        gauge_svg(value, theme::fg(), accent).into_bytes(),
    ))
    .width(Length::Fixed(side))
    .height(Length::Fixed(side));

    let text = widget::Column::new()
        .align_x(Alignment::Center)
        .push(widget::text(format!("{:.0}%", value * 100.0)).size(side * 0.22))
        .push(
            widget::text(label.to_string())
                .size((side * 0.11).max(8.0))
                .class(cosmic::style::Text::Custom(theme::dim_text)),
        );

    let stacked = Stack::new().push(dial).push(
        widget::container(text)
            .width(Length::Fixed(side))
            .height(Length::Fixed(side))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    );
    let centered = widget::container(stacked)
        .width(Length::Fill)
        .center_x(Length::Fill);
    super::tile(width, false, centered)
}

fn metric_desc(id: &str, name: &str, icon: &str) -> ModuleDescriptor {
    ModuleDescriptor {
        id: id.into(),
        name: name.into(),
        icon: icon.into(),
        size: TileSize::Small,
        resizable: true,
    }
}

// --- CPU ---

pub struct CpuModule {
    desc: ModuleDescriptor,
    value: ValueAnim,
    prev: Option<(u64, u64)>,
}

impl CpuModule {
    pub fn new() -> Self {
        Self {
            desc: metric_desc("builtin.cpu", "CPU", "utilities-system-monitor-symbolic"),
            value: ValueAnim::new(0.0),
            prev: read_cpu_times(),
        }
    }
    fn read(&mut self) {
        if let Some((total, idle)) = read_cpu_times() {
            if let Some((pt, pi)) = self.prev {
                let dt = total.saturating_sub(pt);
                let di = idle.saturating_sub(pi);
                if dt > 0 {
                    self.value.set((1.0 - di as f32 / dt as f32).clamp(0.0, 1.0));
                }
            }
            self.prev = Some((total, idle));
        }
    }
}

impl Module for CpuModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }
    fn view(&self, _id: InstanceId, _edit: bool, width: f32) -> Element<'_, Message> {
        gauge_tile(width, self.value.current(), "CPU")
    }
    fn on_control(&mut self, _c: &str, _v: ControlValue) -> Task<Message> {
        Task::none()
    }
    fn refresh(&mut self) -> Task<Message> {
        self.read();
        Task::none()
    }
    fn animating(&self) -> bool {
        self.value.animating()
    }
}

// --- RAM ---

pub struct RamModule {
    desc: ModuleDescriptor,
    value: ValueAnim,
}

impl RamModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: metric_desc("builtin.ram", "RAM", "drive-harddisk-symbolic"),
            value: ValueAnim::new(0.0),
        };
        if let Some(r) = read_ram() {
            m.value.set(r);
        }
        m
    }
}

impl Module for RamModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }
    fn view(&self, _id: InstanceId, _edit: bool, width: f32) -> Element<'_, Message> {
        gauge_tile(width, self.value.current(), "RAM")
    }
    fn on_control(&mut self, _c: &str, _v: ControlValue) -> Task<Message> {
        Task::none()
    }
    fn refresh(&mut self) -> Task<Message> {
        if let Some(r) = read_ram() {
            self.value.set(r);
        }
        Task::none()
    }
    fn animating(&self) -> bool {
        self.value.animating()
    }
}

// --- GPU ---

pub struct GpuModule {
    desc: ModuleDescriptor,
    value: ValueAnim,
    path: Option<PathBuf>,
}

impl GpuModule {
    pub fn new() -> Self {
        let path = find_gpu();
        let mut m = Self {
            desc: metric_desc("builtin.gpu", "GPU", "video-display-symbolic"),
            value: ValueAnim::new(0.0),
            path,
        };
        if let Some(p) = &m.path {
            if let Some(g) = read_gpu(p) {
                m.value.set(g);
            }
        }
        m
    }
}

impl Module for GpuModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }
    fn view(&self, _id: InstanceId, _edit: bool, width: f32) -> Element<'_, Message> {
        gauge_tile(width, self.value.current(), "GPU")
    }
    fn on_control(&mut self, _c: &str, _v: ControlValue) -> Task<Message> {
        Task::none()
    }
    fn refresh(&mut self) -> Task<Message> {
        if let Some(p) = &self.path {
            if let Some(g) = read_gpu(p) {
                self.value.set(g);
            }
        }
        Task::none()
    }
    fn animating(&self) -> bool {
        self.value.animating()
    }
}
