//! A canvas-drawn radial gauge: a 270° track, a glowing accent value arc,
//! tick marks, and a centered percentage. The glow pulses on a time base.
//! Pure 2D vector drawing — the middle tier, no GPU shader.

use cosmic::iced::advanced::text::Alignment as TextAlignment;
use cosmic::iced::alignment::Vertical;
use cosmic::iced::{Color, Point, Radians, Rectangle, mouse};
use cosmic::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke, Text, path::Arc};

pub struct Gauge {
    /// 0.0..=1.0
    pub value: f32,
    pub accent: Color,
    /// Seconds since app start; drives the glow pulse.
    pub anim: f32,
    pub label: String,
}

impl<M> canvas::Program<M, cosmic::Theme, cosmic::Renderer> for Gauge {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &cosmic::Renderer,
        theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<cosmic::Renderer>> {
        use std::f32::consts::PI;
        // Foreground colour from the theme so the dial + text flip with the
        // light/dark switch (canvas text can't fall back to the default).
        let fg: Color = theme.cosmic().background.on.into();
        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        // Everything scales off the available half-extent, so the gauge looks
        // right whether it's a 96px sysmon cell or a big standalone dial.
        let half = bounds.width.min(bounds.height) / 2.0;
        let radius = half * 0.72;
        let sw = (radius * 0.16).max(2.5); // arc stroke width

        let start = 0.75 * PI; // 135°
        let sweep = 1.5 * PI; // 270°
        let value = self.value.clamp(0.0, 1.0);

        // Background track.
        let track = Path::new(|b| {
            b.arc(Arc {
                center,
                radius,
                start_angle: Radians(start),
                end_angle: Radians(start + sweep),
            });
        });
        frame.stroke(
            &track,
            Stroke::default()
                .with_width(sw)
                .with_color(Color { a: 0.12, ..fg })
                .with_line_cap(LineCap::Round),
        );

        // Value arc, with a wide pulsing glow drawn underneath it.
        let value_path = Path::new(|b| {
            b.arc(Arc {
                center,
                radius,
                start_angle: Radians(start),
                end_angle: Radians(start + sweep * value),
            });
        });
        let pulse = 0.5 + 0.5 * (self.anim * 2.2).sin();
        frame.stroke(
            &value_path,
            Stroke::default()
                .with_width(sw * 2.0)
                .with_color(Color { a: 0.10 + 0.14 * pulse, ..self.accent })
                .with_line_cap(LineCap::Round),
        );
        frame.stroke(
            &value_path,
            Stroke::default()
                .with_width(sw)
                .with_color(self.accent)
                .with_line_cap(LineCap::Round),
        );

        // Tick marks (longer every fifth), just outside the arc.
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let a = start + sweep * t;
            let (s, c) = a.sin_cos();
            let r0 = radius + sw * 0.9;
            let r1 = radius + sw * if i % 5 == 0 { 1.9 } else { 1.4 };
            frame.stroke(
                &Path::line(
                    Point::new(center.x + c * r0, center.y + s * r0),
                    Point::new(center.x + c * r1, center.y + s * r1),
                ),
                Stroke::default()
                    .with_width((sw * 0.16).max(1.0))
                    .with_color(Color { a: 0.3, ..fg }),
            );
        }

        // Centered percentage + label beneath it, sized to the gauge.
        frame.fill_text(Text {
            content: format!("{:.0}%", value * 100.0),
            position: Point::new(center.x, center.y - radius * 0.06),
            color: fg,
            size: (radius * 0.6).into(),
            align_x: TextAlignment::Center,
            align_y: Vertical::Center,
            ..Text::default()
        });
        frame.fill_text(Text {
            content: self.label.clone(),
            position: Point::new(center.x, center.y + radius * 0.46),
            color: Color { a: 0.6, ..fg },
            size: (radius * 0.26).into(),
            align_x: TextAlignment::Center,
            align_y: Vertical::Center,
            ..Text::default()
        });

        vec![frame.into_geometry()]
    }
}
