//! Entry point for the panel-applet binary.

use std::process::ExitCode;

fn main() -> ExitCode {
    match cosmic::applet::run::<cosmic_control_center::applet::Applet>(()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cosmic-control-center-applet: {e}");
            ExitCode::from(1)
        }
    }
}
