//! System monitor metrics as **separate** single-gauge modules: CPU, GPU, RAM.
//! Each is its own tile so the user can place/size them independently. They
//! share the cheap file-based readers below (no process spawns).
//!   - CPU: delta of /proc/stat idle vs total between polls
//!   - RAM: /proc/meminfo (1 - MemAvailable/MemTotal)
//!   - GPU: amdgpu's /sys/class/drm/card*/device/gpu_busy_percent

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize, ValueAnim};
use crate::theme;
use crate::widgets::gauge::{gauge_svg, sparkline_svg};
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

/// A filesystem's capacity snapshot: used fraction (for the gauge) plus absolute
/// byte figures (for the wide capacity bar).
#[derive(Clone, Copy, Default)]
struct DiskInfo {
    frac: f32,
    used: u64,
    total: u64,
    free: u64,
}

/// Capacity of the filesystem at `mount` via statvfs(2) — a syscall, not a
/// subprocess, so the disk gauge stays as cheap as the /proc readers.
fn read_disk(mount: &str) -> Option<DiskInfo> {
    use std::mem::MaybeUninit;
    let path = std::ffi::CString::new(mount).ok()?;
    // SAFETY: `path` is a valid NUL-terminated C string; statvfs only writes
    // into `stat`, which we treat as initialized only on a 0 return.
    unsafe {
        let mut stat = MaybeUninit::<libc::statvfs>::uninit();
        if libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) != 0 {
            return None;
        }
        let s = stat.assume_init();
        if s.f_blocks == 0 {
            return None;
        }
        let bs = s.f_frsize as u64;
        let total = s.f_blocks as u64 * bs;
        let free = s.f_bavail as u64 * bs;
        // Used as the user sees it (blocks not available to them), matching
        // `df` closely enough.
        let used = (s.f_blocks - s.f_bfree) as u64 * bs;
        let frac = (1.0 - s.f_bavail as f64 / s.f_blocks as f64).clamp(0.0, 1.0) as f32;
        Some(DiskInfo {
            frac,
            used,
            total,
            free,
        })
    }
}

/// Human-readable byte size in decimal units (matches how drive capacity is
/// usually labelled, e.g. a "512 GB" SSD).
fn fmt_bytes(b: u64) -> String {
    let gb = b as f64 / 1e9;
    if gb >= 1000.0 {
        format!("{:.1} TB", gb / 1000.0)
    } else if gb >= 1.0 {
        format!("{gb:.0} GB")
    } else {
        format!("{:.0} MB", b as f64 / 1e6)
    }
}

/// Wide disk tile (3col+): a used/free capacity bar with absolute figures —
/// more useful than a time graph for a value that barely moves.
fn capacity_tile<'a>(width: f32, label: &str, info: DiskInfo) -> Element<'a, Message> {
    let head = widget::Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(widget::icon::from_name("drive-multidisk-symbolic").size(20))
        .push(widget::text::body(label.to_string()))
        .push(widget::space::horizontal())
        .push(widget::text::body(format!("{:.0}%", info.frac * 100.0)));
    let bar = widget::progress_bar::determinate_linear(info.frac)
        .width(Length::Fill)
        .girth(Length::Fixed(8.0));
    let detail = widget::text::caption(format!(
        "{} of {} used · {} free",
        fmt_bytes(info.used),
        fmt_bytes(info.total),
        fmt_bytes(info.free)
    ))
    .class(cosmic::style::Text::Custom(theme::dim_text));
    let col = widget::Column::new()
        .spacing(8)
        .push(head)
        .push(bar)
        .push(detail);
    super::tile(width, false, col)
}

/// Real, mounted filesystems the disk gauge can target, from /proc/mounts.
/// Deduped by **device**, not mount point: btrfs subvolumes (/, /home, /var/log,
/// …) share one device and report identical usage, so listing each is just
/// noise — collapse them to one entry (the shortest mount point, e.g. "/") per
/// device. Distinct physical filesystems (a separate /boot, /mnt/storage) stay.
fn list_mounts() -> Vec<String> {
    let Ok(s) = fs::read_to_string("/proc/mounts") else {
        return vec!["/".to_string()];
    };
    // (device, chosen mount point) — keep the shortest mount point per device.
    let mut by_dev: Vec<(String, String)> = Vec::new();
    for l in s.lines() {
        let mut it = l.split_whitespace();
        let (Some(dev), Some(mnt)) = (it.next(), it.next()) else {
            continue;
        };
        // Only block-device-backed filesystems (skips proc, sysfs, tmpfs, …).
        if !dev.starts_with("/dev/") {
            continue;
        }
        match by_dev.iter_mut().find(|(d, _)| d == dev) {
            Some((_, m)) if mnt.len() < m.len() => *m = mnt.to_string(),
            Some(_) => {}
            None => by_dev.push((dev.to_string(), mnt.to_string())),
        }
    }
    let mut mounts: Vec<String> = by_dev.into_iter().map(|(_, m)| m).collect();
    mounts.sort();
    if mounts.is_empty() {
        mounts.push("/".to_string());
    }
    mounts
}

/// A short label for a mount point: "ROOT" for /, else the last path segment
/// uppercased (e.g. /mnt/games -> GAMES).
fn mount_label(mount: &str) -> String {
    match mount.rsplit('/').find(|s| !s.is_empty()) {
        Some(name) => name.to_uppercase(),
        None => "ROOT".to_string(),
    }
}

/// One square gauge: an SVG dial (positions correctly in the applet popup,
/// unlike a canvas) with the percentage + label overlaid as native text. Sized
/// to `side` px; no card — the caller frames it.
fn gauge_visual<'a>(side: f32, value: f32, label: &str) -> Element<'a, Message> {
    let accent = theme::accent();
    // Quantize to whole percent so the SVG bytes are byte-identical across the
    // ease frames that land on the same percent. iced caches rasterized SVGs by
    // content hash, so this turns ~60 re-rasters/sec (per gauge, during a value
    // animation) into one per percent step — the main cause of input lag when
    // several gauges animate at once.
    let pct = (value.clamp(0.0, 1.0) * 100.0).round();
    let dial = widget::svg(widget::svg::Handle::from_memory(
        gauge_svg(pct / 100.0, theme::fg(), accent).into_bytes(),
    ))
    .width(Length::Fixed(side))
    .height(Length::Fixed(side));

    let text = widget::Column::new()
        .align_x(Alignment::Center)
        .push(widget::text(format!("{pct:.0}%")).size(side * 0.22))
        .push(
            widget::text(label.to_string())
                .size((side * 0.11).max(8.0))
                .class(cosmic::style::Text::Custom(theme::dim_text)),
        );

    Stack::new()
        .push(dial)
        .push(
            widget::container(text)
                .width(Length::Fixed(side))
                .height(Length::Fixed(side))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .into()
}

/// Shared single-metric tile body: a centered square gauge in a card. The
/// square is capped to the tile's content width so it fits a 1col tile.
fn gauge_tile<'a>(width: f32, value: f32, label: &str) -> Element<'a, Message> {
    let side = (width - 28.0).clamp(1.0, 104.0);
    let centered = widget::container(gauge_visual(side, value, label))
        .width(Length::Fill)
        .center_x(Length::Fill);
    super::tile(width, false, centered)
}

/// Rolling history of recent samples, kept by each metric module to feed the
/// large-size line graph. ~2 minutes at the 2s applet poll.
const HIST_CAP: usize = 64;

fn push_hist(hist: &mut Vec<f32>, v: f32) {
    hist.push(v);
    if hist.len() > HIST_CAP {
        hist.remove(0);
    }
}

/// Wide-tile body (3col+): a history line graph with the label and the current
/// value overlaid top-left.
fn graph_tile<'a>(width: f32, value: f32, hist: &[f32], label: &str) -> Element<'a, Message> {
    let accent = theme::accent();
    let cw = (width - 28.0).max(1.0);
    let gh = 84.0_f32;
    let graph = widget::svg(widget::svg::Handle::from_memory(
        sparkline_svg(hist, cw, gh, theme::fg(), accent).into_bytes(),
    ))
    .width(Length::Fixed(cw))
    .height(Length::Fixed(gh));

    // A container with only padding leaves its content top-left.
    let overlay = widget::container(
        widget::Column::new()
            .spacing(1)
            .push(
                widget::text(label.to_string())
                    .size(11)
                    .class(cosmic::style::Text::Custom(theme::dim_text)),
            )
            .push(widget::text(format!("{:.0}%", value * 100.0)).size(20)),
    )
    .width(Length::Fixed(cw))
    .height(Length::Fixed(gh))
    .padding(6);

    let stacked = Stack::new().push(graph).push(overlay);
    super::tile(width, false, stacked)
}

/// Pick the gauge (1-2col) or the history graph (3col+) for a metric tile.
fn metric_tile<'a>(width: f32, value: f32, hist: &[f32], label: &str) -> Element<'a, Message> {
    if crate::module::cols_for_width(width) >= 3 {
        graph_tile(width, value, hist, label)
    } else {
        gauge_tile(width, value, label)
    }
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
    hist: Vec<f32>,
}

impl CpuModule {
    pub fn new() -> Self {
        Self {
            desc: metric_desc("builtin.cpu", "CPU", "utilities-system-monitor-symbolic"),
            value: ValueAnim::new(0.0),
            prev: read_cpu_times(),
            hist: Vec::new(),
        }
    }
    fn read(&mut self) {
        if let Some((total, idle)) = read_cpu_times() {
            if let Some((pt, pi)) = self.prev {
                let dt = total.saturating_sub(pt);
                let di = idle.saturating_sub(pi);
                if dt > 0 {
                    let v = (1.0 - di as f32 / dt as f32).clamp(0.0, 1.0);
                    self.value.set(v);
                    push_hist(&mut self.hist, v);
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
        metric_tile(width, self.value.current(), &self.hist, "CPU")
    }
    fn on_control(&mut self, _c: &str, _v: ControlValue) -> Task<Message> {
        Task::none()
    }
    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
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
    hist: Vec<f32>,
}

impl RamModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: metric_desc("builtin.ram", "RAM", "drive-harddisk-symbolic"),
            value: ValueAnim::new(0.0),
            hist: Vec::new(),
        };
        if let Some(r) = read_ram() {
            m.value.set(r);
            push_hist(&mut m.hist, r);
        }
        m
    }
}

impl Module for RamModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }
    fn view(&self, _id: InstanceId, _edit: bool, width: f32) -> Element<'_, Message> {
        metric_tile(width, self.value.current(), &self.hist, "RAM")
    }
    fn on_control(&mut self, _c: &str, _v: ControlValue) -> Task<Message> {
        Task::none()
    }
    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
        if let Some(r) = read_ram() {
            self.value.set(r);
            push_hist(&mut self.hist, r);
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
    hist: Vec<f32>,
}

impl GpuModule {
    pub fn new() -> Self {
        let path = find_gpu();
        let mut m = Self {
            desc: metric_desc("builtin.gpu", "GPU", "video-display-symbolic"),
            value: ValueAnim::new(0.0),
            path,
            hist: Vec::new(),
        };
        if let Some(p) = &m.path {
            if let Some(g) = read_gpu(p) {
                m.value.set(g);
                push_hist(&mut m.hist, g);
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
        metric_tile(width, self.value.current(), &self.hist, "GPU")
    }
    fn on_control(&mut self, _c: &str, _v: ControlValue) -> Task<Message> {
        Task::none()
    }
    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
        if let Some(p) = &self.path {
            if let Some(g) = read_gpu(p) {
                self.value.set(g);
                push_hist(&mut self.hist, g);
            }
        }
        Task::none()
    }
    fn animating(&self) -> bool {
        self.value.animating()
    }
}

// --- Disk ---

pub struct DiskModule {
    desc: ModuleDescriptor,
    value: ValueAnim,
    /// Which filesystem this tile watches. Per-instance, so several disk tiles
    /// can each track a different mount.
    mount: String,
    info: DiskInfo,
}

impl DiskModule {
    /// Default tile (root filesystem) — used for the palette descriptor.
    pub fn new() -> Self {
        Self::with_mount("/".to_string())
    }

    pub fn with_mount(mount: String) -> Self {
        let mut m = Self {
            desc: metric_desc("builtin.disk", "Disk", "drive-multidisk-symbolic"),
            value: ValueAnim::new(0.0),
            mount,
            info: DiskInfo::default(),
        };
        m.read();
        m
    }

    fn read(&mut self) {
        if let Some(d) = read_disk(&self.mount) {
            self.info = d;
            self.value.set(d.frac);
        }
    }
}

impl Module for DiskModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }
    fn view(&self, _id: InstanceId, _edit: bool, width: f32) -> Element<'_, Message> {
        // 1-2col: a percentage gauge. 3col+: a capacity bar with byte figures (a
        // time graph is pointless for a value that barely moves).
        if crate::module::cols_for_width(width) >= 3 {
            capacity_tile(width, &mount_label(&self.mount), self.info)
        } else {
            gauge_tile(width, self.value.current(), &mount_label(&self.mount))
        }
    }
    fn on_control(&mut self, _c: &str, _v: ControlValue) -> Task<Message> {
        Task::none()
    }
    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
        self.read();
        Task::none()
    }
    fn animating(&self) -> bool {
        self.value.animating()
    }
    fn params(&self) -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::from([("mount".to_string(), self.mount.clone())])
    }
    fn option_choices(&self) -> Vec<String> {
        list_mounts()
    }
    fn option_selected(&self) -> usize {
        list_mounts().iter().position(|m| m == &self.mount).unwrap_or(0)
    }
    fn option_label(&self) -> &'static str {
        "Mount"
    }
    fn set_option(&mut self, index: usize) {
        if let Some(m) = list_mounts().get(index) {
            if m != &self.mount {
                self.mount = m.clone();
                self.read();
            }
        }
    }
}

// --- Combined (CPU + GPU + RAM in one tile) ---

/// All three core metrics in a single tile, as an alternative to placing the
/// separate gauges. 3col+ shows three gauges in a row; narrower falls back to a
/// compact text readout so it stays legible.
pub struct SysMonModule {
    desc: ModuleDescriptor,
    cpu: ValueAnim,
    gpu: ValueAnim,
    ram: ValueAnim,
    prev: Option<(u64, u64)>,
    gpu_path: Option<PathBuf>,
}

impl SysMonModule {
    pub fn new() -> Self {
        let gpu_path = find_gpu();
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.sysmon".into(),
                name: "System Monitor".into(),
                icon: "utilities-system-monitor-symbolic".into(),
                size: TileSize::Large,
                resizable: true,
            },
            cpu: ValueAnim::new(0.0),
            gpu: ValueAnim::new(0.0),
            ram: ValueAnim::new(0.0),
            prev: read_cpu_times(),
            gpu_path,
        };
        if let Some(r) = read_ram() {
            m.ram.set(r);
        }
        if let Some(p) = &m.gpu_path {
            if let Some(g) = read_gpu(p) {
                m.gpu.set(g);
            }
        }
        m
    }
}

impl Module for SysMonModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }
    fn view(&self, _id: InstanceId, _edit: bool, width: f32) -> Element<'_, Message> {
        let (cpu, gpu, ram) = (self.cpu.current(), self.gpu.current(), self.ram.current());
        let cols = crate::module::cols_for_width(width);

        // Narrow: a compact text readout instead of cramming three dials.
        if cols < 3 {
            let line = |label: &str, v: f32| -> Element<'_, Message> {
                widget::Row::new()
                    .spacing(8)
                    .push(widget::text::body(label.to_string()))
                    .push(widget::space::horizontal())
                    .push(widget::text::body(format!("{:.0}%", v * 100.0)))
                    .into()
            };
            let body = widget::Column::new()
                .spacing(4)
                .width(Length::Fill)
                .push(line("CPU", cpu))
                .push(line("GPU", gpu))
                .push(line("RAM", ram));
            return super::tile(width, false, body);
        }

        // 3col+: three gauges in a row. Split the content width three ways
        // (card padding 14 each side, ~12 between dials).
        let side = ((width - 28.0 - 24.0) / 3.0).clamp(1.0, 104.0);
        let row = widget::Row::new()
            .spacing(12)
            .align_y(Alignment::Center)
            .push(gauge_visual(side, cpu, "CPU"))
            .push(gauge_visual(side, gpu, "GPU"))
            .push(gauge_visual(side, ram, "RAM"));
        let centered = widget::container(row)
            .width(Length::Fill)
            .center_x(Length::Fill);
        super::tile(width, false, centered)
    }
    fn on_control(&mut self, _c: &str, _v: ControlValue) -> Task<Message> {
        Task::none()
    }
    fn refresh(&mut self, _id: InstanceId) -> Task<Message> {
        if let Some((total, idle)) = read_cpu_times() {
            if let Some((pt, pi)) = self.prev {
                let dt = total.saturating_sub(pt);
                let di = idle.saturating_sub(pi);
                if dt > 0 {
                    self.cpu.set((1.0 - di as f32 / dt as f32).clamp(0.0, 1.0));
                }
            }
            self.prev = Some((total, idle));
        }
        if let Some(r) = read_ram() {
            self.ram.set(r);
        }
        if let Some(p) = &self.gpu_path {
            if let Some(g) = read_gpu(p) {
                self.gpu.set(g);
            }
        }
        Task::none()
    }
    fn animating(&self) -> bool {
        self.cpu.animating() || self.gpu.animating() || self.ram.animating()
    }
}
