//! The configuration **editor** window — sidebar to add modules + the live grid
//! as the arrangement canvas. The runtime surface is the applet binary.

use cosmic_ext_control_center::app;
use std::process::ExitCode;

fn main() -> ExitCode {
    cosmic_ext_control_center::prefer_vulkan_backend();
    let settings = cosmic::app::Settings::default()
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(720.0)
                .min_height(520.0),
        )
        // Roomy by default: sidebar (240) + the 4-column grid canvas.
        .size(cosmic::iced::Size::new(900.0, 740.0));

    match cosmic::app::run_single_instance::<app::App>(settings, app::Flags) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cosmic-ext-control-center: {e}");
            ExitCode::from(1)
        }
    }
}
