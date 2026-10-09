use crate::{Message, style};
use iced::{
    Color, Point, Rectangle, Renderer, Theme, mouse,
    widget::canvas::{self, Geometry, Path, Stroke, Text},
};
use lightdriver_core::PumpCurve;
use std::cell::RefCell;

pub struct GeometryState<T> {
    cache: canvas::Cache,
    value: RefCell<Option<T>>,
}
impl<T> Default for GeometryState<T> {
    fn default() -> Self {
        Self {
            cache: canvas::Cache::new(),
            value: RefCell::new(None),
        }
    }
}
impl<T: PartialEq + Clone> GeometryState<T> {
    fn update(&self, value: T) {
        if self.value.borrow().as_ref() != Some(&value) {
            self.cache.clear();
            *self.value.borrow_mut() = Some(value);
        }
    }
}

pub struct Curve {
    pub curve: PumpCurve,
}

impl canvas::Program<Message> for Curve {
    type State = GeometryState<PumpCurve>;
    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        state.update(self.curve.clone());
        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
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
            if let (Some(first), Some(last)) = (self.curve.points.first(), self.curve.points.last())
            {
                let points: Vec<_> =
                    std::iter::once(Point::new(x(min_t as f32), y(first.duty as f32)))
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
                    let dot =
                        Path::circle(Point::new(x(p.coolant_c as f32), y(p.duty as f32)), 3.5);
                    frame.fill(&dot, style::PANEL);
                    frame.stroke(
                        &dot,
                        Stroke::default().with_color(style::ACCENT).with_width(2.0),
                    );
                }
            }
        });
        vec![geometry]
    }
}

pub struct Ring {
    pub rgb: [u8; 3],
    pub brightness: u8,
}
impl canvas::Program<Message> for Ring {
    type State = GeometryState<([u8; 3], u8)>;
    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        state.update((self.rgb, self.brightness));
        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
            let radius = (bounds.width.min(bounds.height) / 2.0 - 16.0).min(95.0);
            let intensity = self.brightness as f32 / 100.0;
            let color = Color::from_rgb(
                self.rgb[0] as f32 / 255.0 * intensity,
                self.rgb[1] as f32 / 255.0 * intensity,
                self.rgb[2] as f32 / 255.0 * intensity,
            );
            frame.fill(&Path::circle(center, radius), style::BG);
            for (offset, opacity, thickness) in
                [(0.0, 1.0, 3.0), (11.0, 0.7, 2.0), (20.0, 0.35, 1.0)]
            {
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
        });
        vec![geometry]
    }
}

pub struct SensorHistory {
    pub samples: Vec<lightdriver_core::SensorSample>,
    pub unit: lightdriver_core::Unit,
}
impl canvas::Program<Message> for SensorHistory {
    type State = GeometryState<(Vec<lightdriver_core::SensorSample>, lightdriver_core::Unit)>;
    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        state.update((self.samples.clone(), self.unit));
        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let samples: Vec<_> = self
                .samples
                .iter()
                .filter(|s| self.unit.accepts(s.value))
                .collect();
            if samples.is_empty() {
                frame.fill_text(Text {
                    content: "No measured history yet".into(),
                    position: Point::new(12.0, 42.0),
                    color: style::MUTED,
                    size: 13.0.into(),
                    ..Default::default()
                });
                return;
            }
            let first = samples[0];
            let last = samples[samples.len() - 1];
            let min = samples.iter().fold(first.value, |v, s| v.min(s.value));
            let max = samples.iter().fold(first.value, |v, s| v.max(s.value));
            let padding = ((max - min) * 0.15).max(1.0);
            let low = min - padding;
            let high = max + padding;
            let left = 55.0;
            let right = (bounds.width - 15.0).max(left + 1.0);
            let top = 12.0;
            let bottom = (bounds.height - 23.0).max(top + 1.0);
            let duration = last.at_ms.saturating_sub(first.at_ms).max(1) as f32;
            let x =
                |at: u64| left + at.saturating_sub(first.at_ms) as f32 / duration * (right - left);
            let y = |value: f64| bottom - ((value - low) / (high - low)) as f32 * (bottom - top);
            for value in [low, (low + high) / 2.0, high] {
                frame.stroke(
                    &Path::line(Point::new(left, y(value)), Point::new(right, y(value))),
                    Stroke::default().with_color(style::LINE).with_width(1.0),
                );
                frame.fill_text(Text {
                    content: format!("{value:.1}"),
                    position: Point::new(3.0, y(value) - 6.0),
                    color: style::MUTED,
                    size: 10.0.into(),
                    ..Default::default()
                });
            }
            for pair in samples.windows(2) {
                if pair[1].at_ms >= pair[0].at_ms
                    && pair[1].at_ms - pair[0].at_ms <= lightdriver_core::STALE_AFTER_MS
                {
                    frame.stroke(
                        &Path::line(
                            Point::new(x(pair[0].at_ms), y(pair[0].value)),
                            Point::new(x(pair[1].at_ms), y(pair[1].value)),
                        ),
                        Stroke::default().with_color(style::ACCENT).with_width(2.0),
                    );
                }
            }
            frame.fill(
                &Path::circle(Point::new(x(last.at_ms), y(last.value)), 3.0),
                style::ACCENT,
            );
            frame.fill_text(Text {
                content: format!("{:.0}s ago", duration / 1000.0),
                position: Point::new(left, bottom + 7.0),
                color: style::MUTED,
                size: 10.0.into(),
                ..Default::default()
            });
            frame.fill_text(Text {
                content: self.unit.label().into(),
                position: Point::new(right - 18.0, bottom + 7.0),
                color: style::MUTED,
                size: 10.0.into(),
                ..Default::default()
            });
        });
        vec![geometry]
    }
}

pub struct RadialLoad {
    pub value: Option<f32>,
}
impl canvas::Program<Message> for RadialLoad {
    type State = GeometryState<Option<f32>>;
    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let value = self
            .value
            .filter(|v| v.is_finite() && (0.0..=100.0).contains(v));
        state.update(value);
        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
            let radius = (bounds.width.min(bounds.height) / 2.0 - 9.0).max(1.0);
            let start = std::f32::consts::PI * 0.75;
            let sweep = std::f32::consts::PI * 1.5;
            let arc = |end| {
                Path::new(|builder| {
                    builder.arc(canvas::path::Arc {
                        center,
                        radius,
                        start_angle: iced::Radians(start),
                        end_angle: iced::Radians(end),
                    })
                })
            };
            let stroke = Stroke::default()
                .with_width(6.0)
                .with_line_cap(canvas::LineCap::Round);
            frame.stroke(&arc(start + sweep), stroke.with_color(style::LINE));
            if let Some(v) = value.filter(|v| *v > 0.0) {
                frame.stroke(
                    &arc(start + sweep * v / 100.0),
                    stroke.with_color(style::ACCENT),
                );
            }
            frame.fill_text(Text {
                content: value
                    .map(|v| format!("{v:.0}%"))
                    .unwrap_or_else(|| "—".into()),
                position: Point::new(center.x, center.y - 3.0),
                color: if value.is_some() {
                    style::FG
                } else {
                    style::MUTED
                },
                size: 25.0.into(),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..Default::default()
            });
        });
        vec![geometry]
    }
}
