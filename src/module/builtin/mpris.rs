//! MPRIS over D-Bus via `playerctld`, using zbus's **blocking** API so it fits
//! the control center's synchronous poll (the blocking connection runs its own
//! internal executor thread — no conflict with cosmic's tokio runtime).
//!
//! Ported from the cosmic-mediaplayer-applet (which uses the async variant).

use std::collections::HashMap;
use zbus::blocking::Connection;
use zbus::proxy;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

fn owned_to_string(v: &OwnedValue) -> Option<String> {
    let cloned = v.try_clone().ok()?;
    let value: Value<'static> = cloned.into();
    match value {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

fn owned_to_string_array(v: &OwnedValue) -> Option<Vec<String>> {
    let cloned = v.try_clone().ok()?;
    Vec::<String>::try_from(cloned).ok()
}

fn owned_to_i64(v: &OwnedValue) -> Option<i64> {
    let cloned = v.try_clone().ok()?;
    if let Ok(n) = i64::try_from(&cloned) {
        return Some(n);
    }
    if let Ok(n) = u64::try_from(&cloned) {
        return Some(n as i64);
    }
    None
}

#[proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_service = "org.mpris.MediaPlayer2.playerctld",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait Player {
    fn play_pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn position(&self) -> zbus::Result<i64>;
    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;
    #[zbus(property)]
    fn can_go_next(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn can_go_previous(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn can_seek(&self) -> zbus::Result<bool>;
}

#[derive(Debug, Clone, Default)]
pub struct MprisState {
    pub title: String,
    pub artist: String,
    pub art_path: Option<String>,
    pub length_us: i64,
    pub position_us: i64,
    pub playing: bool,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_seek: bool,
    pub track_id: Option<String>,
    pub has_player: bool,
}

pub fn connect() -> zbus::Result<Connection> {
    Connection::session()
}

pub fn fetch_state(conn: &Connection) -> zbus::Result<MprisState> {
    let player = PlayerProxyBlocking::new(conn)?;

    // If there's no active player, metadata/status calls error — treat as empty.
    let status = match player.playback_status() {
        Ok(s) => s,
        Err(_) => return Ok(MprisState::default()),
    };
    let metadata = player.metadata().unwrap_or_default();
    let position = player.position().unwrap_or(0);
    let can_next = player.can_go_next().unwrap_or(false);
    let can_prev = player.can_go_previous().unwrap_or(false);
    let can_seek = player.can_seek().unwrap_or(false);
    let track_id = metadata.get("mpris:trackid").and_then(owned_to_string);

    let title = metadata
        .get("xesam:title")
        .and_then(owned_to_string)
        .unwrap_or_default();
    let artist = metadata
        .get("xesam:artist")
        .and_then(owned_to_string_array)
        .map(|v| v.join(", "))
        .unwrap_or_default();
    let art_path = metadata
        .get("mpris:artUrl")
        .and_then(owned_to_string)
        .and_then(|s| s.strip_prefix("file://").map(str::to_string));
    let length_us = metadata.get("mpris:length").and_then(owned_to_i64).unwrap_or(0);

    Ok(MprisState {
        title,
        artist,
        art_path,
        length_us,
        position_us: position,
        playing: status == "Playing",
        can_next,
        can_prev,
        can_seek,
        track_id,
        has_player: !(status == "Stopped" && metadata.is_empty()),
    })
}

pub fn set_position(conn: &Connection, track_id: &str, position_us: i64) {
    if let Ok(p) = PlayerProxyBlocking::new(conn) {
        if let Ok(path) = ObjectPath::try_from(track_id) {
            let _ = p.set_position(path, position_us);
        }
    }
}

pub fn play_pause(conn: &Connection) {
    if let Ok(p) = PlayerProxyBlocking::new(conn) {
        let _ = p.play_pause();
    }
}
pub fn next(conn: &Connection) {
    if let Ok(p) = PlayerProxyBlocking::new(conn) {
        let _ = p.next();
    }
}
pub fn previous(conn: &Connection) {
    if let Ok(p) = PlayerProxyBlocking::new(conn) {
        let _ = p.previous();
    }
}
