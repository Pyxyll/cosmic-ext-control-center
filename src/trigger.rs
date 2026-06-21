//! Lets a global shortcut open the applet popup (issue #34). The running applet
//! serves a tiny D-Bus interface; `cosmic-ext-control-center-applet --toggle`
//! (what the configured shortcut spawns) calls it, and the applet toggles its
//! popup in response.

use crate::app::Message;
use cosmic::iced::Subscription;

pub const NAME: &str = "com.pyxyll.CosmicExtControlCenter";
pub const PATH: &str = "/com/pyxyll/CosmicExtControlCenter";

type Out = cosmic::iced::futures::channel::mpsc::Sender<Message>;

struct Trigger {
    tx: Out,
}

#[zbus::interface(name = "com.pyxyll.CosmicExtControlCenter")]
impl Trigger {
    /// Open or close the applet popup (as a layer surface — no input serial).
    async fn toggle(&self) {
        let _ = self.tx.clone().try_send(Message::ToggleSurface);
    }
}

/// Serve the toggle interface (run from the applet). The served object holds the
/// subscription's sender, so a `Toggle` call surfaces as `Message::TogglePopup`.
pub fn subscription() -> Subscription<Message> {
    Subscription::run_with("applet-trigger", |_| {
        cosmic::iced::stream::channel(4, |output| async move {
            let _ = serve(output).await;
            std::future::pending::<()>().await
        })
    })
}

async fn serve(output: Out) -> zbus::Result<()> {
    let conn = zbus::connection::Builder::session()?
        .serve_at(PATH, Trigger { tx: output })?
        .build()
        .await?;
    // The newest applet instance wins the name (panel restarts can briefly
    // overlap two instances).
    let _ = conn
        .request_name_with_flags(
            NAME,
            zbus::fdo::RequestNameFlags::ReplaceExisting
                | zbus::fdo::RequestNameFlags::AllowReplacement,
        )
        .await;
    // Keep the connection (and the served object) alive.
    std::future::pending::<()>().await;
    Ok(())
}

/// Client side of `--toggle`: call `Toggle` on the running applet, then return.
/// Best-effort — if no applet is running there's nothing to toggle.
pub fn send_toggle() {
    let result = (|| -> zbus::Result<()> {
        let conn = zbus::blocking::Connection::session()?;
        conn.call_method(Some(NAME), PATH, Some(NAME), "Toggle", &())?;
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("cosmic-ext-control-center-applet --toggle: {e}");
    }
}
