//! Entry point for the panel-applet binary.

use std::process::ExitCode;

fn main() -> ExitCode {
    // `--toggle` (spawned by the global shortcut) just pokes the running applet
    // over D-Bus to open/close its popup, then exits.
    if std::env::args().any(|a| a == "--toggle") {
        cosmic_ext_control_center::trigger::send_toggle();
        return ExitCode::SUCCESS;
    }
    cosmic_ext_control_center::prefer_vulkan_backend();
    match cosmic::applet::run::<cosmic_ext_control_center::applet::Applet>(()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cosmic-ext-control-center-applet: {e}");
            ExitCode::from(1)
        }
    }
}
