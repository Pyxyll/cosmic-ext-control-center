//! Wi-Fi quick toggle via NetworkManager (`nmcli radio wifi`). Rendered as a
//! split pill: tap to toggle, chevron expands an inline network picker.

use crate::app::Message;
use crate::module::{ControlValue, InstanceId, ListEntry, Module, ModuleDescriptor, TileSize};
use cosmic::app::Task;
use cosmic::prelude::*;

pub struct WifiModule {
    desc: ModuleDescriptor,
    on: bool,
    available: bool,
    ssid: Option<String>,
    /// Negotiated link rate in Mb/s for the active connection, if any.
    rate: Option<u32>,
    /// Networks for the inline selection list, populated when expanded.
    nets: Vec<Net>,
    /// Whether the drawer is open, so the (slower) network scan is only fetched
    /// when it's actually shown.
    want_entries: bool,
    /// SSID awaiting a password (a new secured network), and the typed value.
    pending: Option<String>,
    password: String,
}

/// One scanned network.
#[derive(Clone, Default)]
struct Net {
    ssid: String,
    signal: u32,
    active: bool,
    /// Has security (needs a password unless already saved).
    secured: bool,
    /// A saved NM connection exists, so it connects without a prompt.
    known: bool,
}

/// A snapshot fetched off the UI thread, then applied to the module.
#[derive(Default)]
struct WifiData {
    on: bool,
    available: bool,
    ssid: Option<String>,
    rate: Option<u32>,
    nets: Vec<Net>,
}

/// Gather Wi-Fi state off the UI thread. `want_entries` adds the network scan
/// (only when the picker is open); `rescan` forces NetworkManager to scan afresh
/// (the manual refresh) rather than reading its cached list.
fn fetch(want_entries: bool, rescan: bool) -> WifiData {
    let mut d = WifiData::default();
    if let Some(o) = super::out("nmcli radio wifi") {
        d.on = o.trim() == "enabled";
        d.available = true;
    }
    // Active SSID, if connected: the "yes:<ssid>" line.
    d.ssid = super::out("nmcli -t -f active,ssid dev wifi").and_then(|o| {
        o.lines()
            .find_map(|l| l.strip_prefix("yes:").map(str::to_string))
            .filter(|s| !s.is_empty())
    });
    // Link rate of the active AP (queried separately from the SSID so an SSID
    // containing ':' can't break field parsing).
    d.rate = super::out("nmcli -t -f active,rate dev wifi")
        .and_then(|o| o.lines().find_map(|l| l.strip_prefix("yes:").and_then(parse_rate)));
    if want_entries {
        d.nets = scan_networks(rescan);
    }
    d
}

/// Available networks via `nmcli`, strongest-signal-per-SSID, active first.
/// `rescan` forces a fresh scan (`--rescan yes`, a few seconds) instead of
/// reading the cached list — used by the manual refresh.
fn scan_networks(rescan: bool) -> Vec<Net> {
    let list_cmd = if rescan {
        "nmcli -t -f ACTIVE,SIGNAL,SECURITY,SSID dev wifi list --rescan yes"
    } else {
        "nmcli -t -f ACTIVE,SIGNAL,SECURITY,SSID dev wifi list --rescan no"
    };
    let Some(o) = super::out(list_cmd) else {
        return Vec::new();
    };
    // Saved connection names, to mark which networks connect without a prompt.
    let saved: Vec<String> = super::out("nmcli -t -f NAME connection show")
        .map(|o| o.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default();

    let mut best: Vec<Net> = Vec::new();
    for l in o.lines() {
        // SSID is the last field and may contain ':', so split into at most 4.
        let mut it = l.splitn(4, ':');
        let active = it.next() == Some("yes");
        let signal: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let security = it.next().unwrap_or("");
        let ssid = it.next().unwrap_or("").trim().to_string();
        if ssid.is_empty() {
            continue;
        }
        let secured = !(security.is_empty() || security == "--");
        match best.iter_mut().find(|n| n.ssid == ssid) {
            Some(n) => {
                n.active |= active;
                n.signal = n.signal.max(signal);
            }
            None => best.push(Net {
                known: saved.iter().any(|s| s == &ssid),
                ssid,
                signal,
                active,
                secured,
            }),
        }
    }
    best.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    best
}

/// Connect to an SSID, with a password for a new secured network.
fn connect(ssid: &str, password: Option<&str>) {
    let esc = |s: &str| s.replace('\'', "'\\''");
    let cmd = match password {
        Some(pw) => format!(
            "nmcli dev wifi connect '{}' password '{}'",
            esc(ssid),
            esc(pw)
        ),
        None => format!("nmcli dev wifi connect '{}'", esc(ssid)),
    };
    super::run(&cmd);
}

/// Parse nmcli's rate field ("540 Mbit/s") into Mb/s, dropping a zero rate.
fn parse_rate(s: &str) -> Option<u32> {
    s.split_whitespace()
        .next()?
        .parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
}

/// Truncate a long label to fit a tile, with an ellipsis.
fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    } else {
        s.to_string()
    }
}

impl WifiModule {
    pub fn new() -> Self {
        let mut m = Self {
            desc: ModuleDescriptor {
                id: "builtin.wifi".into(),
                name: "Wi-Fi".into(),
                icon: "network-wireless-symbolic".into(),
                size: TileSize::Medium,
                resizable: true,
            },
            on: false,
            available: false,
            ssid: None,
            rate: None,
            nets: Vec::new(),
            want_entries: false,
            pending: None,
            password: String::new(),
        };
        m.set(fetch(false, false)); // one-time synchronous read so the tile opens populated
        m
    }

    /// Apply a fetched snapshot to the cached state.
    fn set(&mut self, d: WifiData) {
        self.on = d.on;
        self.available = d.available;
        self.ssid = d.ssid;
        self.rate = d.rate;
        // Keep the existing list when this refresh didn't scan (drawer closed).
        if self.want_entries {
            self.nets = d.nets;
        }
    }
}

impl Module for WifiModule {
    fn descriptor(&self) -> &ModuleDescriptor {
        &self.desc
    }

    fn view(&self, id: InstanceId, edit: bool, width: f32) -> Element<'_, Message> {
        // Primary line is the SSID when connected (the "Wi-Fi" name is dropped —
        // the icon already identifies it); falls back to "Wi-Fi" otherwise.
        let connected = self.on && self.ssid.is_some();
        let label = match &self.ssid {
            Some(ssid) if self.on => ellipsize(ssid, 18),
            _ => "Wi-Fi".to_string(),
        };
        // Secondary line: link rate when connected, else the connection state.
        let status = if !self.on {
            "Off".to_string()
        } else if connected {
            match self.rate {
                Some(r) => format!("{r} Mb/s"),
                None => "Connected".to_string(),
            }
        } else {
            "Not connected".to_string()
        };
        super::toggle_tile(id, width, self.on, edit, self.desc.icon.as_str(), &label, &status, super::Chevron::Expand)
    }

    fn on_control(&mut self, control: &str, value: ControlValue) -> Task<Message> {
        match control {
            "on" => {
                if let ControlValue::Bool(b) = value {
                    self.on = b;
                    if self.available {
                        super::run(&format!("nmcli radio wifi {}", if b { "on" } else { "off" }));
                    }
                }
            }
            "settings" => super::run("cosmic-settings network"),
            // Drawer open/close: flag whether the (slower) scan is fetched; the
            // hub dispatches the actual async refresh.
            "expand" => {
                if let ControlValue::Bool(b) = value {
                    self.want_entries = b;
                    // Keep the cached list while the drawer animates closed (the
                    // flag already stops it refreshing); clearing it here made an
                    // empty "Nothing available" flash mid-close. Just drop any
                    // half-entered password.
                    if !b {
                        self.pending = None;
                    }
                }
            }
            "select" => {
                if let ControlValue::Text(ssid) = value {
                    let net = self.nets.iter().find(|n| n.ssid == ssid);
                    let needs_pw = net.is_some_and(|n| n.secured && !n.known && !n.active);
                    if needs_pw {
                        // New secured network: ask for a password instead of
                        // attempting a doomed connect.
                        self.pending = Some(ssid);
                        self.password.clear();
                    } else {
                        connect(&ssid, None);
                        self.ssid = Some(ssid); // optimistic; corrected on next poll
                        self.on = true;
                    }
                }
            }
            "input" => {
                if let ControlValue::Text(v) = value {
                    self.password = v;
                }
            }
            "submit" => {
                if let Some(ssid) = self.pending.take() {
                    connect(&ssid, Some(&self.password));
                    self.password.clear();
                    self.ssid = Some(ssid); // optimistic
                    self.on = true;
                }
            }
            "cancel" => {
                self.pending = None;
                self.password.clear();
            }
            _ => {}
        }
        Task::none()
    }

    fn expandable(&self) -> bool {
        true
    }

    fn entries(&self) -> Vec<ListEntry> {
        self.nets
            .iter()
            .map(|n| ListEntry {
                key: n.ssid.clone(),
                label: n.ssid.clone(),
                detail: if n.secured {
                    format!("{}% · secured", n.signal)
                } else {
                    format!("{}%", n.signal)
                },
                active: n.active,
            })
            .collect()
    }

    fn pending_input(&self) -> Option<(String, String)> {
        self.pending
            .as_ref()
            .map(|ssid| (ssid.clone(), self.password.clone()))
    }

    fn refresh(&mut self, id: InstanceId) -> Task<Message> {
        let want_entries = self.want_entries;
        super::fetch_task(id, move || fetch(want_entries, false))
    }

    /// The refresh button forces a real rescan so new networks actually appear
    /// (re-reading the cached list looked like nothing happened).
    fn refresh_manual(&mut self, id: InstanceId) -> Task<Message> {
        let want_entries = self.want_entries;
        super::fetch_task(id, move || fetch(want_entries, true))
    }

    fn apply_data(&mut self, data: &dyn std::any::Any) {
        if let Some(d) = data.downcast_ref::<WifiData>() {
            // Clone out of the shared payload into our cached state.
            self.set(WifiData {
                on: d.on,
                available: d.available,
                ssid: d.ssid.clone(),
                rate: d.rate,
                nets: d.nets.clone(),
            });
        }
    }
}
