use crate::Message;
use iced::{
    Background, Border, Color, Element, Font, Theme, font,
    widget::{button, container, text},
};

pub const ACCENT: Color = Color::from_rgb8(101, 223, 204);
pub const BG: Color = Color::from_rgb8(16, 19, 25);
pub const PANEL: Color = Color::from_rgb8(25, 29, 37);
pub const SIDE: Color = Color::from_rgb8(20, 24, 32);
pub const LINE: Color = Color::from_rgb8(42, 49, 60);
pub const MUTED: Color = Color::from_rgb8(153, 167, 185);
pub const FG: Color = Color::from_rgb8(237, 242, 248);
pub const AMBER: Color = Color::from_rgb8(215, 182, 116);

pub fn medium_font() -> Font {
    Font {
        weight: font::Weight::Medium,
        ..Font::with_name("Segoe UI")
    }
}

pub fn theme() -> Theme {
    Theme::custom(
        "LightDriver",
        iced::theme::Palette {
            background: BG,
            text: FG,
            primary: ACCENT,
            success: ACCENT,
            danger: Color::from_rgb8(235, 123, 134),
            warning: AMBER,
        },
    )
}

pub fn label<'a>(content: impl Into<String>) -> iced::widget::Text<'a> {
    text(content.into()).size(12).color(MUTED)
}
pub fn heading<'a>(content: impl Into<String>, size: u16) -> iced::widget::Text<'a> {
    text(content.into())
        .size(u32::from(size))
        .font(medium_font())
}

pub fn box_style(
    background: Color,
    border: bool,
    radius: f32,
) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(background.into()),
        text_color: Some(FG),
        border: Border {
            color: LINE,
            width: if border { 1.0 } else { 0.0 },
            radius: radius.into(),
        },
        ..Default::default()
    }
}

pub fn selector<'a>(
    content: impl Into<Element<'a, Message>>,
    message: Message,
    selected: bool,
) -> iced::widget::Button<'a, Message> {
    button(content)
        .on_press(message)
        .padding([10, 14])
        .style(move |_, status| button::Style {
            background: Some(Background::Color(if selected {
                PANEL
            } else if status == button::Status::Hovered {
                SIDE
            } else {
                BG
            })),
            text_color: if selected { ACCENT } else { MUTED },
            border: Border {
                color: if selected { LINE } else { BG },
                width: 1.0,
                radius: 7.0.into(),
            },
            ..Default::default()
        })
}

pub fn primary(_: &Theme, status: button::Status) -> button::Style {
    let disabled = status == button::Status::Disabled;
    button::Style {
        background: Some(if disabled { LINE } else { ACCENT }.into()),
        text_color: if disabled { MUTED } else { BG },
        border: Border {
            radius: 7.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn secondary(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            if status == button::Status::Hovered {
                LINE
            } else {
                PANEL
            }
            .into(),
        ),
        text_color: if status == button::Status::Disabled {
            MUTED
        } else {
            FG
        },
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    }
}

pub fn badge<'a>(content: impl Into<String>, tone: Color) -> Element<'a, Message> {
    container(text(content.into()).size(11).color(tone))
        .padding([5, 9])
        .style(move |_| container::Style {
            background: Some(BG.into()),
            border: Border {
                width: 1.0,
                color: LINE,
                radius: 5.0.into(),
            },
            ..Default::default()
        })
        .into()
}
