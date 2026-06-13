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

/// SVG markup for a history sparkline (a filled area under a line) at the given
/// pixel size. The viewBox matches `w`x`h` so strokes aren't distorted by
/// non-uniform scaling. `history` is oldest-to-newest, each 0..1. Drawn as an
/// image for the same applet-popup reason as the gauge.
pub fn sparkline_svg(history: &[f32], w: f32, h: f32, fg: Color, accent: Color) -> String {
    let (w, h) = (w.max(1.0), h.max(1.0));
    let grid = css(Color { a: 0.10, ..fg });
    // Baseline + quarter gridlines, always drawn so an empty/short history still
    // reads as a chart frame.
    let mut gridlines = String::new();
    for f in [0.25_f32, 0.5, 0.75] {
        let y = h * f;
        gridlines.push_str(&format!(
            "<line x1='0' y1='{y:.2}' x2='{w:.2}' y2='{y:.2}' stroke='{grid}' stroke-width='1'/>"
        ));
    }

    let body = if history.len() >= 2 {
        let n = history.len();
        let dx = w / (n as f32 - 1.0);
        // A small top/bottom inset so a 100% or 0% sample isn't clipped by the
        // stroke width.
        let pad = 2.0_f32;
        let pts: Vec<(f32, f32)> = history
            .iter()
            .enumerate()
            .map(|(i, v)| (i as f32 * dx, h - pad - v.clamp(0.0, 1.0) * (h - 2.0 * pad)))
            .collect();
        let line = pts
            .iter()
            .map(|(x, y)| format!("{x:.2},{y:.2}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut area = format!("M0 {h:.2} ");
        for (x, y) in &pts {
            area.push_str(&format!("L{x:.2} {y:.2} "));
        }
        area.push_str(&format!("L{w:.2} {h:.2} Z"));
        let fillc = css(Color { a: 0.20, ..accent });
        let linec = css(accent);
        format!(
            "<path d='{area}' fill='{fillc}' stroke='none'/>\
             <polyline points='{line}' fill='none' stroke='{linec}' stroke-width='2' stroke-linejoin='round' stroke-linecap='round'/>"
        )
    } else {
        String::new()
    };

    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {w:.2} {h:.2}'>{gridlines}{body}</svg>"
    )
}
