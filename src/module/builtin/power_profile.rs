//! Power profile control via `powerprofilesctl`, rendered as a split pill (same
//! style as Wi-Fi): tapping the body **cycles** through the available profiles
//! (power-saver → balanced → performance → …), the chevron opens COSMIC power
//! settings. Not a binary toggle, so tap = "next profile".

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;

pub struct PowerProfileModule {
    desc: ModuleDescriptor,
    profiles: Vec<String>,
    current: String,
    available: bool,
    /// Live average CPU clock (GHz), shown as the secondary line.
    freq_ghz: Option<f32>,
}

fn pretty(p: &str) -> String {
    match p {
        "power-saver" => "Power Saver".into(),
        "balanced" => "Balanced".into(),
        "performance" => "Performance".into(),
        other => other.to_string(),
    }
}

/// Per-profile glyph, so the icon reflects the mode. Uses the `battery-profile-*`
/// names (present in Breeze/Papirus; the `power-profile-*` set is Adwaita-only,
/// which isn't in COSMIC's icon-lookup fallback chain, so it rendered blank).
fn profile_icon(p: &str) -> &'static str {
    match p {
        "power-saver" => "battery-profile-powersave-symbolic",
        "performance" => "battery-profile-performance-symbolic",
        _ => "battery-profile-balanced-symbolic",
    }
}

/// Average current CPU clock across all cpufreq policies, in GHz.
fn read_freq_ghz() -> Option<f32> {
    let dir = std::fs::read_dir("/sys/devices/system/cpu/cpufreq").ok()?;
    let (mut sum, mut n) = (0u64, 0u64);
    for entry in dir.flatten() {
        if let Ok(s) = std::fs::read_to_string(entry.path().join("scaling_cur_freq")) {
            if let Ok(khz) = s.trim().parse::<u64>() {
                sum += khz;
                n += 1;
            }
        }
    }
    (n > 0).then(|| sum as f32 / n as f32 / 1_000_000.0)
}

/// Rank for the cycle order (ascending power); unknown profiles sort last.
fn rank(p: &str) -> u8 {
    match p {
        "power-saver" => 0,
        "balanced" => 1,
        "performance" => 2,
        _ => 3,
    }
}

impl PowerProfileModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.power_profile".into(),
                name: "Power Profile".into(),
                icon: "power-profile-balanced-symbolic".into(),
                size: TileSize::Medium,
                resizable: true,
            },
            profiles: Vec::new(),
            current: String::new(),
            available: false,
            freq_ghz: None,
        };
        // Discover available profiles once (names are lines ending in ':').
        if let Some(list) = super::out("powerprofilesctl list") {
            for line in list.lines() {
                let t = line.trim().trim_start_matches('*').trim();
                if let Some(name) = t.strip_suffix(':') {
                    let name = name.trim();
                    if !name.is_empty() {
                        m.profiles.push(name.to_string());
                    }
                }
            }
        }
        if m.profiles.is_empty() {
            m.profiles = vec!["power-saver".into(), "balanced".into(), "performance".into()];
        }
        // Cycle in ascending-power order regardless of how the daemon lists them.
        m.profiles.sort_by_key(|p| rank(p));
        m.read();
        m
    }

    fn read(&mut self) {
        if let Some(c) = super::out("powerprofilesctl get") {
            self.current = c;
            self.available = true;
        }
        self.freq_ghz = read_freq_ghz();
    }
}

impl Module for PowerProfileModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        let icon = profile_icon(&self.current);
        let label = if self.current.is_empty() {
            "Power Mode".to_string()
        } else {
            pretty(&self.current)
        };
        // Secondary line = live CPU clock (the GHz readout), else a hint.
        let status = match self.freq_ghz {
            Some(f) if self.available => format!("{f:.1} GHz"),
            _ if self.available => "Tap to cycle".to_string(),
            _ => "Unavailable".to_string(),
        };
        // Accent the tile when boosted (performance), like Wi-Fi lights up when on.
        let on = self.current == "performance";
        super::toggle_tile(id, width, on, edit, icon, &label, &status, true)
    }

    fn on_control(&mut self, control: &str, _value: ControlValue) -> Task<Message> {
        match control {
            // toggle_tile emits "on" on tap; for a cycle we ignore the bool and
            // advance to the next profile.
            "on" => {
                if self.available && !self.profiles.is_empty() {
                    let i = self
                        .profiles
                        .iter()
                        .position(|p| p == &self.current)
                        .unwrap_or(0);
                    let next = self.profiles[(i + 1) % self.profiles.len()].clone();
                    super::run(&format!("powerprofilesctl set {next}"));
                    self.current = next; // optimistic; corrected on next poll
                }
            }
            "settings" => super::run("cosmic-settings power"),
            _ => {}
        }
        Task::none()
    }

    fn refresh(&mut self) -> Task<Message> {
        self.read();
        Task::none()
    }
}
