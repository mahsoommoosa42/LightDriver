use crate::{Message, style};
use iced::{
    Color, Point, Rectangle, Renderer, Theme, mouse,
    widget::canvas::{self, Frame, Geometry, Path, Stroke, Text},
};
use lightdriver_core::PumpCurve;

pub struct Curve {
    pub curve: PumpCurve,
}

impl canvas::Program<Message> for Curve {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let left = 42.0;
        let right = bounds.width - 18.0;
        let top = 26.0;
        let bottom = bounds.height - 45.0;
        let min_t = self
            .curve
            .points
            .first()
            .map_or(25, |p| (p.coolant_c / 5 * 5).min(25));
        let max_t = self
            .curve
            .points
            .last()
            .map_or(50, |p| (p.coolant_c.div_ceil(5) * 5).max(50));
        let x = |t: f32| left + (t - min_t as f32) / (max_t - min_t) as f32 * (right - left);
        let y = |d: f32| bottom - (d - 20.0) / 80.0 * (bottom - top);
        for duty in [20, 60, 100] {
            let p = Point::new(left, y(duty as f32));
            frame.stroke(
                &Path::line(p, Point::new(right, p.y)),
                Stroke::default().with_color(style::LINE).with_width(1.0),
            );
            frame.fill_text(Text {
                content: duty.to_string(),
                position: Point::new(8.0, p.y - 7.0),
                color: style::MUTED,
                size: 11.0.into(),
                ..Default::default()
            });
        }
        for temperature in (min_t..=max_t).step_by(5) {
            frame.fill_text(Text {
                content: temperature.to_string(),
                position: Point::new(x(temperature as f32) - 7.0, bottom + 11.0),
                color: style::MUTED,
                size: 11.0.into(),
                ..Default::default()
            });
        }
        frame.fill_text(Text {
            content: "Duty (%)".into(),
            position: Point::new(left, 3.0),
            color: style::MUTED,
            size: 11.0.into(),
            ..Default::default()
        });
        frame.fill_text(Text {
            content: "Coolant (°C)".into(),
            position: Point::new(bounds.width / 2.0 - 30.0, bounds.height - 15.0),
            color: style::MUTED,
            size: 11.0.into(),
            ..Default::default()
        });
        if let (Some(first), Some(last)) = (self.curve.points.first(), self.curve.points.last()) {
            let points: Vec<_> = std::iter::once(Point::new(x(min_t as f32), y(first.duty as f32)))
                .chain(
                    self.curve
                        .points
                        .iter()
                        .map(|p| Point::new(x(p.coolant_c as f32), y(p.duty as f32))),
                )
                .chain(std::iter::once(Point::new(
                    x(max_t as f32),
                    y(last.duty as f32),
                )))
                .collect();
            let line = Path::new(|b| {
                b.move_to(points[0]);
                for p in &points[1..] {
                    b.line_to(*p);
                }
            });
            let area = Path::new(|b| {
                b.move_to(Point::new(left, bottom));
                for p in &points {
                    b.line_to(*p);
                }
                b.line_to(Point::new(right, bottom));
                b.close();
            });
            frame.fill(
                &area,
                Color {
                    a: 0.08,
                    ..style::ACCENT
                },
            );
            frame.stroke(
                &line,
                Stroke::default().with_color(style::ACCENT).with_width(2.2),
            );
            for p in &self.curve.points {
                let dot = Path::circle(Point::new(x(p.coolant_c as f32), y(p.duty as f32)), 3.5);
                frame.fill(&dot, style::PANEL);
                frame.stroke(
                    &dot,
                    Stroke::default().with_color(style::ACCENT).with_width(2.0),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

pub struct Ring {
    pub rgb: [u8; 3],
    pub brightness: u8,
}
impl canvas::Program<Message> for Ring {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let radius = (bounds.width.min(bounds.height) / 2.0 - 16.0).min(95.0);
        let intensity = self.brightness as f32 / 100.0;
        let color = Color::from_rgb(
            self.rgb[0] as f32 / 255.0 * intensity,
            self.rgb[1] as f32 / 255.0 * intensity,
            self.rgb[2] as f32 / 255.0 * intensity,
        );
        frame.fill(&Path::circle(center, radius), style::BG);
        for (offset, opacity, thickness) in [(0.0, 1.0, 3.0), (11.0, 0.7, 2.0), (20.0, 0.35, 1.0)] {
            frame.stroke(
                &Path::circle(center, radius - offset),
                Stroke::default().with_width(thickness).with_color(Color {
                    a: opacity,
                    ..color
                }),
            );
        }
        frame.fill_text(Text {
            content: "NZXT".into(),
            position: Point::new(center.x - 24.0, center.y - 10.0),
            color,
            size: 19.0.into(),
            font: style::medium_font(),
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}
