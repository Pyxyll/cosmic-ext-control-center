//! Desktop notifications for the notification-center module (issue #9).
//!
//! COSMIC's notification daemon owns `org.freedesktop.Notifications` and exposes
//! no history to query, so we can't ask it for past notifications. Instead we
//! attach a passive D-Bus **monitor** (the same mechanism as `dbus-monitor`):
//! it observes every `Notify` method call on the session bus without owning the
//! name, so it never conflicts with the real daemon — both see each notification.
//!
//! Caveats: we only capture notifications that arrive while the monitor runs (no
//! retroactive history), and this list is independent of COSMIC's own.

use crate::app::Message;
use cosmic::cosmic_config::{ConfigGet, ConfigSet};
use cosmic::iced::Subscription;
use cosmic::iced::futures::{SinkExt, StreamExt};
use std::collections::HashMap;

// --- Do Not Disturb -----------------------------------------------------------
// COSMIC's notification daemon watches the `do_not_disturb` key of its
// cosmic-config; flipping it silences popups system-wide (our monitor still
// logs them). Shared by the notification center and the standalone DnD toggle.

fn dnd_config() -> Option<cosmic::cosmic_config::Config> {
    cosmic::cosmic_config::Config::new("com.system76.CosmicNotifications", 1).ok()
}

pub fn read_dnd() -> bool {
    dnd_config()
        .and_then(|c| c.get::<bool>("do_not_disturb").ok())
        .unwrap_or(false)
}

pub fn write_dnd(on: bool) {
    if let Some(c) = dnd_config() {
        let _ = c.set("do_not_disturb", on);
    }
}

/// One captured notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub app: String,
    pub summary: String,
    pub body: String,
    /// `app_icon` from the Notify call — an icon name or a file path, or empty.
    pub icon: String,
}

/// The always-on notification monitor as an iced subscription. Runs whenever a
/// notifications tile is placed; emits `Message::Notify` per incoming notification.
pub fn subscription() -> Subscription<Message> {
    Subscription::run_with("notification-monitor", |_| {
        cosmic::iced::stream::channel(16, |mut output| async move {
            // On error (no bus / monitor refused) stop sending but stay pending,
            // so iced doesn't treat the ended stream as a restart loop.
            let _ = run(&mut output).await;
            std::future::pending::<()>().await
        })
    })
}

/// Notify's D-Bus signature `susssasa{sv}i`:
/// (app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout).
type NotifyArgs = (
    String,
    u32,
    String,
    String,
    String,
    Vec<String>,
    HashMap<String, zbus::zvariant::OwnedValue>,
    i32,
);

async fn run(
    output: &mut cosmic::iced::futures::channel::mpsc::Sender<Message>,
) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    let monitor = zbus::fdo::MonitoringProxy::new(&conn).await?;
    // Watch only the Notify method calls into the notification daemon.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::MethodCall)
        .interface("org.freedesktop.Notifications")?
        .member("Notify")?
        .build();
    monitor.become_monitor(&[rule], 0).await?;

    // After BecomeMonitor the connection only receives the monitored traffic.
    let mut stream = zbus::MessageStream::from(&conn);
    while let Some(Ok(msg)) = stream.next().await {
        if msg.header().member().map(|m| m.as_str()) != Some("Notify") {
            continue;
        }
        if let Ok((app, _replaces, icon, summary, body, _actions, _hints, _expire)) =
            msg.body().deserialize::<NotifyArgs>()
        {
            // Drop the rare fully-empty notification; otherwise fill a sensible
            // app label.
            if summary.is_empty() && body.is_empty() {
                continue;
            }
            let _ = output
                .send(Message::Notify(Notification {
                    app: if app.is_empty() { "Notification".into() } else { app },
                    summary,
                    body,
                    icon,
                }))
                .await;
        }
    }
    Ok(())
}
