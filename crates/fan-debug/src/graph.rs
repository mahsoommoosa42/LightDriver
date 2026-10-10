//! The graph shows a research estimate, never a measured PWM/RPM response.
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{Canvas, canvas},
};
use lightdriver_gigabyte::firmware::reference_layout::{
    GraphPoint, HeaderSettings, LevelPreview, manual_graph,
};
const MINT: Color = Color::from_rgb8(101, 223, 204);
const AMBER: Color = Color::from_rgb8(215, 182, 116);
pub struct CurveGraph {
    stored: Vec<GraphPoint>,
    draft: Option<LevelPreview>,
}
impl CurveGraph {
    pub fn new<Message>(
        settings: HeaderSettings,
        draft: Option<&LevelPreview>,
    ) -> Result<Canvas<Self, Message>, &'static str> {
        Ok(canvas(Self {
            stored: manual_graph(settings)?,
            draft: draft.cloned(),
        })
        .width(iced::Length::Fill)
        .height(220))
    }
}
impl<Message> canvas::Program<Message> for CurveGraph {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let left = 44.0;
        let top = 14.0;
        let width = (bounds.width - 62.0).max(1.0);
        let height = (bounds.height - 44.0).max(1.0);
        let at = |temperature: f32, level: f32| {
            Point::new(
                left + width * temperature / 100.0,
                top + height * (1.0 - level / 100.0),
            )
        };
        let grid = Color::from_rgb8(53, 63, 76);
        let muted = Color::from_rgb8(153, 167, 185);
        for v in [0, 25, 50, 75, 100] {
            let p = at(0.0, v as f32);
            frame.stroke(
                &canvas::Path::line(p, at(100.0, v as f32)),
                canvas::Stroke::default().with_color(grid),
            );
            frame.fill_text(canvas::Text {
                content: format!("{v}%"),
                position: Point::new(0.0, p.y - 7.0),
                color: muted,
                size: 11.0.into(),
                ..Default::default()
            });
        }
        for t in [0, 25, 50, 75, 100] {
            let p = at(t as f32, 0.0);
            frame.fill_text(canvas::Text {
                content: format!("{t}°C"),
                position: Point::new(p.x - 10.0, p.y + 8.0),
                color: muted,
                size: 11.0.into(),
                ..Default::default()
            });
        }
        let path = canvas::Path::new(|builder| {
            for (index, p) in self.stored.iter().enumerate() {
                let pos = at(p.temperature, p.percent);
                if index == 0 {
                    builder.move_to(pos);
                } else {
                    builder.line_to(pos);
                }
            }
        });
        frame.stroke(
            &path,
            canvas::Stroke::default().with_width(2.0).with_color(MINT),
        );
        if let Some(draft) = &self.draft {
            let path = canvas::Path::new(|builder| {
                builder.move_to(at(
                    f32::from(draft.start_temperature),
                    draft.representable_percent,
                ));
                builder.line_to(at(
                    f32::from(draft.full_temperature) - 0.5,
                    draft.representable_percent,
                ));
                builder.line_to(at(f32::from(draft.full_temperature), 100.0));
            });
            frame.stroke(
                &path,
                canvas::Stroke::default().with_width(2.0).with_color(AMBER),
            );
        }
        // A small marker establishes the drawing bounds without any interactivity.
        frame.fill_rectangle(Point::new(left, top + height), Size::new(width, 1.0), grid);
        vec![frame.into_geometry()]
    }
}
