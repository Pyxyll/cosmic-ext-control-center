//! Reusable visual building blocks: an accent palette and custom container
//! styles (frosted glass, gradient panels, solid cards). These lean on the
//! cosmic theme tokens (`theme.cosmic()`) so they track light/dark and the
//! system corner radii, while layering on the translucency + soft shadows
//! that read as "2026."

use cosmic::iced::gradient::Linear;
use cosmic::iced::{Background, Border, Color, Gradient, Radians, Shadow, Vector};
use cosmic::theme;
use cosmic::widget::container;

/// A hand-picked vibrant palette for the demo, independent of the system
/// accent so we can show colour theory side by side. First entry is the
/// user's cerise (#DA2862-ish) from their Jellyfin/COSMIC theming.
pub const ACCENTS: [(&str, Color); 5] = [
    ("Cerise", Color { r: 0.855, g: 0.157, b: 0.384, a: 1.0 }),
    ("Aurora", Color { r: 0.357, g: 0.78, b: 0.98, a: 1.0 }),
    ("Viridian", Color { r: 0.18, g: 0.82, b: 0.60, a: 1.0 }),
    ("Amber", Color { r: 0.98, g: 0.71, b: 0.18, a: 1.0 }),
    ("Violet", Color { r: 0.55, g: 0.45, b: 0.96, a: 1.0 }),
];

/// Copy of a colour with a new alpha.
pub fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// The active theme's on-background colour — dark on light themes, light on
/// dark themes. Use where a *concrete* colour is needed (canvas drawing, manual
/// foreground styling) instead of the default theme-aware text colour, so the
/// content stays legible after a light/dark switch.
pub fn fg() -> Color {
    cosmic::theme::active().cosmic().background.on.into()
}

/// The live COSMIC system accent colour. The control center tints its active
/// tiles, sliders, and highlights with this so it follows the user's accent
/// setting (not just light/dark). Read per build, so it tracks changes.
pub fn accent() -> Color {
    cosmic::theme::active().cosmic().accent_color().into()
}

/// A divider painted in the window's base background colour, so a strip of it
/// inside a card reads as a gap "cut" through to the desktop behind.
pub fn divider_gap() -> theme::Container<'static> {
    theme::Container::custom(|t| container::Style {
        background: Some(Background::Color(t.cosmic().background.base.into())),
        ..Default::default()
    })
}

/// A theme-aware dimmed-text style for secondary captions (tracks light/dark).
pub fn dim_text(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    cosmic::iced::widget::text::Style {
        color: Some(alpha(theme.cosmic().background.on.into(), 0.6)),
        ..Default::default()
    }
}

/// Linearly interpolate two colours (used for animated highlights).
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// Frosted-glass surface: translucent fill, a faint top-highlight border, and
/// a soft drop shadow. Real backdrop blur needs a compositor protocol COSMIC
/// doesn't expose yet — but over a busy/animated background, translucency +
/// the highlight edge reads convincingly as glass.
pub fn glass() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        let base: Color = c.background.base.into();
        container::Style {
            background: Some(Background::Color(alpha(base, 0.42))),
            border: Border {
                color: alpha(Color::WHITE, 0.16),
                width: 1.0,
                radius: c.corner_radii.radius_l.into(),
            },
            shadow: Shadow {
                color: alpha(Color::BLACK, 0.35),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 28.0,
            },
            text_color: Some(c.background.on.into()),
            ..Default::default()
        }
    })
}

/// A subtle inset surface (a faint fill, gently rounded) for nested list items
/// such as the notification-center cards, so they read as distinct rows inside
/// a drawer without a heavy border.
pub fn inset() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        let fg: Color = c.background.on.into();
        container::Style {
            background: Some(Background::Color(alpha(fg, 0.05))),
            border: Border {
                radius: c.corner_radii.radius_s.into(),
                ..Default::default()
            },
            text_color: Some(fg),
            ..Default::default()
        }
    })
}

/// A solid, slightly-raised card for control-center tiles.
pub fn card(active: bool, tint: Color) -> theme::Container<'static> {
    theme::Container::custom(move |t| {
        let c = t.cosmic();
        let base: Color = c.background.component.base.into();
        let bg = if active { mix(base, tint, 0.55) } else { alpha(base, 0.85) };
        container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: if active { alpha(tint, 0.9) } else { alpha(Color::WHITE, 0.08) },
                width: 1.0,
                radius: c.corner_radii.radius_m.into(),
            },
            shadow: Shadow {
                color: alpha(Color::BLACK, if active { 0.30 } else { 0.18 }),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 14.0,
            },
            text_color: Some(c.background.on.into()),
            ..Default::default()
        }
    })
}

/// A diagonal gradient fill — the cheap, always-available way to get depth
/// without the GPU shader tier.
pub fn gradient(a: Color, b: Color, radius: [f32; 4]) -> theme::Container<'static> {
    theme::Container::custom(move |_t| {
        let grad = Gradient::Linear(
            Linear::new(Radians(2.3))
                .add_stop(0.0, a)
                .add_stop(1.0, b),
        );
        container::Style {
            background: Some(Background::Gradient(grad)),
            border: Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
}
