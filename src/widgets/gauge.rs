//! A radial gauge built as an **SVG image** (a 270-degree track, an accent value
//! arc, and tick marks). It is rendered as an image rather than an iced canvas
//! because canvas geometry mis-positions for multiple instances inside an applet
//! popup (they pile onto one location) — images translate correctly there. The
//! centered percentage + label are overlaid by the caller as native iced text.

use cosmic::iced::Color;
use std::f32::consts::PI;

fn css(c: Color) -> String {
    format!(
        "rgba({},{},{},{:.3})",
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
        c.a
    )
}

/// SVG markup for a gauge at `value` (0..1). `fg` is the theme foreground (track
/// + ticks), `accent` the value arc. viewBox is 100x100; the caller sizes it.
pub fn gauge_svg(value: f32, fg: Color, accent: Color) -> String {
    let value = value.clamp(0.0, 1.0);
    let (cx, cy, r) = (50.0_f32, 50.0_f32, 33.0_f32);
    let sw = 5.5_f32;
    let start = 0.75 * PI;
    let sweep = 1.5 * PI;

    let pt = |a: f32, rad: f32| (cx + rad * a.cos(), cy + rad * a.sin());
    let (x1, y1) = pt(start, r);
    let (x2, y2) = pt(start + sweep, r);

    let track = css(Color { a: 0.16, ..fg });
    let tickc = css(Color { a: 0.55, ..fg });
    let acc = css(accent);

    // Ticks: thicker than 1 SVG unit, since the 100-unit viewBox is drawn at
    // ~72-104px, so a 1-unit stroke would render sub-pixel and vanish.
    let mut ticks = String::new();
    for i in 0..=10 {
        let a = start + sweep * (i as f32 / 10.0);
        let (tx0, ty0) = pt(a, r + sw * 0.9);
        let (tx1, ty1) = pt(a, r + sw * if i % 5 == 0 { 2.1 } else { 1.5 });
        ticks.push_str(&format!(
            "<line x1='{tx0:.2}' y1='{ty0:.2}' x2='{tx1:.2}' y2='{ty1:.2}' stroke='{tickc}' stroke-width='1.6' stroke-linecap='round'/>"
        ));
    }

    // Omit the value arc near 0 to avoid a degenerate same-point arc.
    let value_arc = if value > 0.005 {
        let (vx, vy) = pt(start + sweep * value, r);
        let large = u8::from(sweep * value > PI);
        format!(
            "<path d='M{x1:.2} {y1:.2} A{r} {r} 0 {large} 1 {vx:.2} {vy:.2}' fill='none' stroke='{acc}' stroke-width='{sw}' stroke-linecap='round'/>"
        )
    } else {
        String::new()
    };

    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'>\
         <path d='M{x1:.2} {y1:.2} A{r} {r} 0 1 1 {x2:.2} {y2:.2}' fill='none' stroke='{track}' stroke-width='{sw}' stroke-linecap='round'/>\
         {value_arc}{ticks}</svg>"
    )
}
