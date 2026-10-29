//! Palette and widget styles shared by every view.

use iced::widget::{button, container, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

pub const BG: Color = Color::from_rgb(0x11 as f32 / 255.0, 0x13 as f32 / 255.0, 0x15 as f32 / 255.0);
pub const PANEL: Color = Color::from_rgb(0x15 as f32 / 255.0, 0x19 as f32 / 255.0, 0x18 as f32 / 255.0);
pub const PANEL_RAISED: Color =
    Color::from_rgb(0x22 as f32 / 255.0, 0x2a as f32 / 255.0, 0x27 as f32 / 255.0);
pub const PANEL_SOFT: Color =
    Color::from_rgb(0x1b as f32 / 255.0, 0x21 as f32 / 255.0, 0x1f as f32 / 255.0);
pub const LINE: Color = Color::from_rgb(0x37 as f32 / 255.0, 0x42 as f32 / 255.0, 0x3f as f32 / 255.0);
pub const LINE_STRONG: Color =
    Color::from_rgb(0x4d as f32 / 255.0, 0x5a as f32 / 255.0, 0x56 as f32 / 255.0);
pub const TEXT: Color = Color::from_rgb(0xe7 as f32 / 255.0, 0xeb as f32 / 255.0, 0xea as f32 / 255.0);
pub const TEXT_SOFT: Color =
    Color::from_rgb(0xc5 as f32 / 255.0, 0xcf as f32 / 255.0, 0xcb as f32 / 255.0);
pub const MUTED: Color = Color::from_rgb(0x8d as f32 / 255.0, 0xa3 as f32 / 255.0, 0x9b as f32 / 255.0);
pub const ACCENT: Color = Color::from_rgb(0xd7 as f32 / 255.0, 0xbd as f32 / 255.0, 0x58 as f32 / 255.0);
pub const ACCENT_SOFT: Color =
    Color::from_rgb(0x37 as f32 / 255.0, 0x32 as f32 / 255.0, 0x1f as f32 / 255.0);
pub const CYAN: Color = Color::from_rgb(0x75 as f32 / 255.0, 0xad as f32 / 255.0, 0x93 as f32 / 255.0);
pub const CYAN_SOFT: Color =
    Color::from_rgb(0x20 as f32 / 255.0, 0x33 as f32 / 255.0, 0x2b as f32 / 255.0);
pub const GREEN: Color = Color::from_rgb(0x58 as f32 / 255.0, 0xa9 as f32 / 255.0, 0x79 as f32 / 255.0);
pub const RED: Color = Color::from_rgb(0xe1 as f32 / 255.0, 0x77 as f32 / 255.0, 0x6d as f32 / 255.0);
pub const RED_SOFT: Color = Color::from_rgb(0x4a as f32 / 255.0, 0x29 as f32 / 255.0, 0x27 as f32 / 255.0);
pub const BLUE: Color = Color::from_rgb(0x68 as f32 / 255.0, 0x93 as f32 / 255.0, 0xaa as f32 / 255.0);

pub fn theme() -> Theme {
    Theme::Dark
}

fn rounded(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Border::default()
    }
}

pub fn panel_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn plain_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn border_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn titlebar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.07, 0.086, 0.082))),
        border: Border {
            color: LINE,
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn statusbar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.078, 0.094, 0.09))),
        border: Border {
            color: LINE,
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(MUTED),
        ..container::Style::default()
    }
}

pub fn raised_card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL_RAISED)),
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn soft_card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL_SOFT)),
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn selected_card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CYAN_SOFT)),
        border: Border {
            color: CYAN,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn error_card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(RED_SOFT)),
        border: Border {
            color: RED,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn drawer_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        border: Border {
            color: LINE_STRONG,
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(TEXT),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
            offset: Vector::new(-12.0, 0.0),
            blur_radius: 32.0,
        },
        ..container::Style::default()
    }
}

pub fn scrim_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
        ..container::Style::default()
    }
}

pub fn tool_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color) = match status {
        button::Status::Hovered => (PANEL_RAISED, LINE_STRONG),
        button::Status::Pressed => (CYAN_SOFT, CYAN),
        button::Status::Disabled => (PANEL_SOFT, LINE),
        _ => (PANEL_RAISED, LINE_STRONG),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: match status {
            button::Status::Disabled => MUTED,
            _ => TEXT,
        },
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 5.0.into(),
        },
        shadow: Shadow::default(),
        ..button::Style::default()
    }
}

pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Color::from_rgb(0.56, 0.50, 0.26),
        button::Status::Disabled => Color::from_rgb(0.30, 0.28, 0.19),
        _ => ACCENT_SOFT,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: match status {
            button::Status::Disabled => MUTED,
            _ => ACCENT,
        },
        border: Border {
            color: ACCENT,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..button::Style::default()
    }
}

pub fn danger_button(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => Color::from_rgb(0.38, 0.20, 0.19),
            button::Status::Disabled => PANEL_SOFT,
            _ => RED_SOFT,
        })),
        text_color: if matches!(status, button::Status::Disabled) { MUTED } else { RED },
        border: Border {
            color: RED,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..button::Style::default()
    }
}

pub fn flat_button(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => {
                Some(Background::Color(PANEL_RAISED))
            }
            _ => None,
        },
        text_color: match status {
            button::Status::Disabled => MUTED,
            _ => TEXT_SOFT,
        },
        border: rounded(4.0),
        ..button::Style::default()
    }
}

pub fn record_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if active {
                if hovered {
                    Color::from_rgb(0.38, 0.18, 0.16)
                } else {
                    Color::from_rgb(0.33, 0.16, 0.15)
                }
            } else if hovered {
                Color::from_rgb(0.18, 0.22, 0.21)
            } else {
                PANEL_RAISED
            })),
            text_color: TEXT,
            border: Border {
                color: if active { RED } else { LINE_STRONG },
                width: 1.0,
                radius: 5.0.into(),
            },
            ..button::Style::default()
        }
    }
}

pub fn input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => CYAN,
        text_input::Status::Hovered => LINE_STRONG,
        _ => LINE,
    };
    text_input::Style {
        background: Background::Color(Color::from_rgb(0.078, 0.098, 0.094)),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 4.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: TEXT,
        selection: CYAN_SOFT,
    }
}

pub fn scrollable_style(
    _theme: &Theme,
    _status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    let rail = iced::widget::scrollable::Rail {
        background: None,
        border: rounded(0.0),
        scroller: iced::widget::scrollable::Scroller {
            background: Background::Color(LINE_STRONG),
            border: rounded(3.0),
        },
    };
    iced::widget::scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: iced::widget::scrollable::AutoScroll {
            background: Background::Color(Color::from_rgba(0.11, 0.13, 0.125, 0.9)),
            border: rounded(12.0),
            shadow: Shadow::default(),
            icon: TEXT_SOFT,
        },
    }
}

pub fn pick_list_style(_theme: &Theme, status: iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    let hovered = matches!(status, iced::widget::pick_list::Status::Hovered | iced::widget::pick_list::Status::Opened { .. });
    iced::widget::pick_list::Style {
        text_color: TEXT,
        placeholder_color: MUTED,
        handle_color: MUTED,
        background: Background::Color(if hovered {
            PANEL_RAISED
        } else {
            Color::from_rgb(0.078, 0.098, 0.094)
        }),
        border: Border {
            color: if hovered { LINE_STRONG } else { LINE },
            width: 1.0,
            radius: 4.0.into(),
        },
    }
}

pub fn menu_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.11, 0.13, 0.125))),
        border: Border {
            color: LINE_STRONG,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(TEXT),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..container::Style::default()
    }
}
