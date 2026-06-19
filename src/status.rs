//! Live system status for the panel applet's status-icon cluster (issue #21).
//!
//! Sourced from D-Bus signals where the service exposes them, so it is
//! event-driven with no idle polling: power-profiles-daemon, BlueZ, and
//! NetworkManager. Audio (PipeWire) has no D-Bus interface, so it is driven by
//! the `pactl subscribe` event stream instead — still events, not polling.
//!
//! Each source is an iced subscription that emits `Message::Status(Update)`;
//! they run only while the cluster icon mode is enabled (see the applet).

use crate::app::Message;
use crate::config::ClusterIcons;
use crate::module::builtin::{bluetooth, power_profile, volume, vpn, wifi};
use cosmic::iced::Subscription;
use cosmic::iced::futures::{SinkExt, StreamExt};

/// A coalesced view of the system state the cluster renders. Each field is fed
/// by its own source; defaults read as "nothing connected".
#[derive(Debug, Clone, Default)]
pub struct StatusSnapshot {
    pub wifi_on: bool,
    pub wifi_connected: bool,
    pub wifi_signal: u32,
    pub vpn_active: bool,
    pub bt_on: bool,
    pub bt_connected: usize,
    pub audio_muted: bool,
    pub audio_volume: f32,
    /// "balanced" / "performance" / "power-saver" / "" (unknown / daemon absent).
    pub profile: String,
}

/// One source's update, applied to the snapshot.
#[derive(Debug, Clone)]
pub enum Update {
    Network {
        on: bool,
        connected: bool,
        signal: u32,
        vpn: bool,
    },
    Bluetooth {
        on: bool,
        connected: usize,
    },
    Audio {
        muted: bool,
        volume: f32,
    },
    Profile(String),
}

impl StatusSnapshot {
    /// Merge a source update into the snapshot. (Named `apply_update`, not
    /// `apply`, to avoid the `Apply` blanket method cosmic's prelude brings in.)
    pub fn apply_update(&mut self, u: Update) {
        match u {
            Update::Network { on, connected, signal, vpn } => {
                self.wifi_on = on;
                self.wifi_connected = connected;
                self.wifi_signal = signal;
                self.vpn_active = vpn;
            }
            Update::Bluetooth { on, connected } => {
                self.bt_on = on;
                self.bt_connected = connected;
            }
            Update::Audio { muted, volume } => {
                self.audio_muted = muted;
                self.audio_volume = volume;
            }
            Update::Profile(p) => self.profile = p,
        }
    }

    /// The ordered icon names for the cluster, reusing the same state->icon
    /// helpers as the tiles, filtered by which indicators the user enabled.
    /// The network slot shows Wi-Fi (or VPN when one is up); Bluetooth appears
    /// only when powered, power-profile only when not the default Balanced.
    pub fn icons(&self, show: ClusterIcons) -> Vec<&'static str> {
        let mut v = Vec::new();
        if show.power {
            v.push("system-shutdown-symbolic");
        }
        if show.network {
            v.push(if self.vpn_active {
                vpn::state_icon(true)
            } else {
                wifi::signal_icon(self.wifi_on, self.wifi_connected, Some(self.wifi_signal))
            });
        }
        if show.audio {
            v.push(volume::volume_icon(self.audio_muted, self.audio_volume));
        }
        if show.bluetooth && self.bt_on {
            v.push(bluetooth::state_icon(true, self.bt_connected));
        }
        if show.power_profile && !self.profile.is_empty() && self.profile != "balanced" {
            v.push(power_profile::profile_icon(&self.profile));
        }
        v
    }
}

/// The channel the sources push updates into.
type Out = cosmic::iced::futures::channel::mpsc::Sender<Message>;

/// The enabled status sources, batched. Run only while the cluster is on; each
/// source is skipped when its indicator is hidden, so we don't watch (or read)
/// a service the user isn't showing. The Power indicator is decorative (no
/// source).
pub fn subscription(show: ClusterIcons) -> Subscription<Message> {
    let mut subs = Vec::new();
    if show.network {
        subs.push(network_source());
    }
    if show.audio {
        subs.push(audio_source());
    }
    if show.bluetooth {
        subs.push(bluetooth_source());
    }
    if show.power_profile {
        subs.push(profile_source());
    }
    Subscription::batch(subs)
}

/// Wrap a long-lived source future as an iced subscription. On error or end it
/// stops sending but keeps the stream pending, so iced doesn't restart-loop it.
macro_rules! source {
    ($id:literal, $run:ident) => {
        Subscription::run_with($id, |_| {
            cosmic::iced::stream::channel(8, |mut output| async move {
                let _ = $run(&mut output).await;
                std::future::pending::<()>().await
            })
        })
    };
}

// --- power-profiles-daemon: the `ActiveProfile` property (emits-change) -------

fn profile_source() -> Subscription<Message> {
    source!("status-profile", run_profile)
}

async fn run_profile(output: &mut Out) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    let proxy = zbus::Proxy::new(
        &conn,
        "net.hadess.PowerProfiles",
        "/net/hadess/PowerProfiles",
        "net.hadess.PowerProfiles",
    )
    .await?;
    if let Ok(p) = proxy.get_property::<String>("ActiveProfile").await {
        let _ = output.send(Message::Status(Update::Profile(p))).await;
    }
    let mut changes = proxy
        .receive_property_changed::<String>("ActiveProfile")
        .await;
    while let Some(change) = changes.next().await {
        if let Ok(p) = change.get().await {
            let _ = output.send(Message::Status(Update::Profile(p))).await;
        }
    }
    Ok(())
}

// --- NetworkManager: re-read on each StateChanged (connect/disconnect/VPN) ----

fn network_source() -> Subscription<Message> {
    source!("status-network", run_network)
}

async fn run_network(output: &mut Out) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    let proxy = zbus::Proxy::new(
        &conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .await?;
    emit(output, read_network).await;
    // StateChanged fires on connectivity transitions; we re-read the details via
    // nmcli off-thread. (Gradual signal drift isn't an event, so the bars update
    // on the next transition - acceptable for a glanceable indicator.)
    let mut signals = proxy.receive_signal("StateChanged").await?;
    while signals.next().await.is_some() {
        emit(output, read_network).await;
    }
    Ok(())
}

// --- BlueZ: re-read on any org.bluez signal (power / connect / pair) ----------

fn bluetooth_source() -> Subscription<Message> {
    source!("status-bluetooth", run_bluetooth)
}

async fn run_bluetooth(output: &mut Out) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    emit(output, read_bluetooth).await;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.bluez")?
        .build();
    let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, Some(16)).await?;
    while stream.next().await.is_some() {
        emit(output, read_bluetooth).await;
    }
    Ok(())
}

// --- Audio: PipeWire has no D-Bus, so drive off `pactl subscribe` events ------

fn audio_source() -> Subscription<Message> {
    source!("status-audio", run_audio)
}

async fn run_audio(output: &mut Out) {
    emit(output, read_audio_update).await;
    let mut tx = output.clone();
    // `pactl subscribe` blocks on a long-lived child; read its lines off-thread
    // and push a fresh reading whenever a sink/server event arrives.
    let _ = tokio::task::spawn_blocking(move || {
        let mut child = match std::process::Command::new("pactl")
            .arg("subscribe")
            .stdout(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        let Some(stdout) = child.stdout.take() else { return };
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.contains("sink") || line.contains("server") {
                if let Some(u) = read_audio() {
                    let _ = tx.try_send(Message::Status(u));
                }
            }
        }
    })
    .await;
}

/// Read a source off the UI thread and push the result.
async fn emit(output: &mut Out, read: fn() -> Update) {
    if let Ok(u) = tokio::task::spawn_blocking(read).await {
        let _ = output.send(Message::Status(u)).await;
    }
}

/// `read_audio` wrapped to always yield an `Update` (muted when unreadable), for
/// the uniform `emit` path.
fn read_audio_update() -> Update {
    read_audio().unwrap_or(Update::Audio { muted: true, volume: 0.0 })
}

// --- blocking readers (run on the blocking pool, only on events) --------------

fn out(cmd: &str) -> Option<String> {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn read_network() -> Update {
    let on = out("nmcli radio wifi").map(|o| o.trim() == "enabled").unwrap_or(false);
    let (connected, signal) = out("nmcli -t -f active,signal dev wifi")
        .and_then(|o| {
            o.lines()
                .find_map(|l| l.strip_prefix("yes:").and_then(|s| s.trim().parse::<u32>().ok()))
        })
        .map_or((false, 0), |s| (true, s));
    let vpn = out("nmcli -t -f TYPE connection show --active")
        .map(|o| o.lines().any(|l| matches!(l.trim(), "vpn" | "wireguard")))
        .unwrap_or(false);
    Update::Network { on, connected, signal, vpn }
}

fn read_bluetooth() -> Update {
    let on = out("bluetoothctl show")
        .map(|o| o.lines().any(|l| l.trim() == "Powered: yes"))
        .unwrap_or(false);
    let connected = out("bluetoothctl devices Connected")
        .map(|o| o.lines().filter(|l| l.starts_with("Device ")).count())
        .unwrap_or(0);
    Update::Bluetooth { on, connected }
}

fn read_audio() -> Option<Update> {
    // "Volume: 0.45" or "Volume: 0.45 [MUTED]"
    let o = out("wpctl get-volume @DEFAULT_AUDIO_SINK@")?;
    let muted = o.contains("[MUTED]");
    let volume = o.split_whitespace().nth(1)?.parse::<f32>().ok()?;
    Some(Update::Audio { muted, volume })
}
