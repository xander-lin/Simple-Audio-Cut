//! Palette, typography and widget styles shared by every view.
//!
//! The visual language is documented in `DESIGN.md`: graphite surfaces, amber
//! for transport and primary actions, teal for selection, green/red only for
//! state. Everything here is a pure function of the widget status so views stay
//! declarative.

use iced::widget::{button, container, pick_list, rule, scrollable, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

/// Window background behind every panel.
pub const BG: Color = Color::from_rgb(0x0e as f32 / 255.0, 0x10 as f32 / 255.0, 0x12 as f32 / 255.0);
/// Standard panel surface (library, timeline, inspector).
pub const PANEL: Color = Color::from_rgb(0x14 as f32 / 255.0, 0x17 as f32 / 255.0, 0x19 as f32 / 255.0);
/// Slightly lighter surface used for rows and inset cards.
pub const PANEL_SOFT: Color = Color::from_rgb(0x18 as f32 / 255.0, 0x1c as f32 / 255.0, 0x1f as f32 / 255.0);
/// Interactive surface: buttons, menu items, hovered rows.
pub const PANEL_RAISED: Color = Color::from_rgb(0x1f as f32 / 255.0, 0x24 as f32 / 255.0, 0x28 as f32 / 255.0);
/// Hairline separator.
pub const LINE: Color = Color::from_rgb(0x2b as f32 / 255.0, 0x31 as f32 / 255.0, 0x36 as f32 / 255.0);
/// Emphasised border for inputs and cards on hover.
pub const LINE_STRONG: Color = Color::from_rgb(0x3c as f32 / 255.0, 0x44 as f32 / 255.0, 0x4a as f32 / 255.0);
pub const TEXT: Color = Color::from_rgb(0xec as f32 / 255.0, 0xee as f32 / 255.0, 0xf0 as f32 / 255.0);
pub const TEXT_SOFT: Color = Color::from_rgb(0xc3 as f32 / 255.0, 0xc9 as f32 / 255.0, 0xce as f32 / 255.0);
pub const MUTED: Color = Color::from_rgb(0x86 as f32 / 255.0, 0x8f as f32 / 255.0, 0x97 as f32 / 255.0);
/// Transport, pending export and primary actions.
pub const ACCENT: Color = Color::from_rgb(0xe9 as f32 / 255.0, 0xb9 as f32 / 255.0, 0x49 as f32 / 255.0);
pub const ACCENT_SOFT: Color = Color::from_rgb(0x3a as f32 / 255.0, 0x2f as f32 / 255.0, 0x17 as f32 / 255.0);
/// Selection, ranges and edit points.
pub const CYAN: Color = Color::from_rgb(0x5f as f32 / 255.0, 0xbf as f32 / 255.0, 0xae as f32 / 255.0);
pub const CYAN_SOFT: Color = Color::from_rgb(0x1b as f32 / 255.0, 0x33 as f32 / 255.0, 0x30 as f32 / 255.0);
pub const GREEN: Color = Color::from_rgb(0x6f as f32 / 255.0, 0xc0 as f32 / 255.0, 0x8b as f32 / 255.0);
pub const RED: Color = Color::from_rgb(0xe2 as f32 / 255.0, 0x7a as f32 / 255.0, 0x6f as f32 / 255.0);
pub const RED_SOFT: Color = Color::from_rgb(0x3a as f32 / 255.0, 0x23 as f32 / 255.0, 0x21 as f32 / 255.0);
pub const BLUE: Color = Color::from_rgb(0x7b as f32 / 255.0, 0xa7 as f32 / 255.0, 0xc9 as f32 / 255.0);

// Waveform canvas colours. Kept here so the canvas and the chrome agree.
pub const WAVE_BG: Color = Color::from_rgb(0x12 as f32 / 255.0, 0x16 as f32 / 255.0, 0x18 as f32 / 255.0);
pub const WAVE_SELECTED_BG: Color = Color::from_rgb(0x15 as f32 / 255.0, 0x1b as f32 / 255.0, 0x1d as f32 / 255.0);
pub const WAVE_FILL: Color = Color::from_rgb(0x86 as f32 / 255.0, 0xd8 as f32 / 255.0, 0xbe as f32 / 255.0);
pub const WAVE_FILL_DIM: Color = Color::from_rgb(0x6d as f32 / 255.0, 0xae as f32 / 255.0, 0x9d as f32 / 255.0);
pub const RMS_FILL: Color = Color::from_rgb(0x7b as f32 / 255.0, 0xa7 as f32 / 255.0, 0xc9 as f32 / 255.0);
pub const RMS_EDGE: Color = Color::from_rgb(0x9d as f32 / 255.0, 0xbe as f32 / 255.0, 0xd6 as f32 / 255.0);
pub const RULER_BG: Color = Color::from_rgb(0x0f as f32 / 255.0, 0x12 as f32 / 255.0, 0x14 as f32 / 255.0);
pub const RULER_TICK: Color = Color::from_rgb(0x44 as f32 / 255.0, 0x4d as f32 / 255.0, 0x53 as f32 / 255.0);
pub const RULER_TEXT: Color = Color::from_rgb(0x74 as f32 / 255.0, 0x7f as f32 / 255.0, 0x87 as f32 / 255.0);
pub const PLAYHEAD: Color = ACCENT;

pub fn theme() -> Theme {
    Theme::Dark
}

fn rounded(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Border::default()
    }
}

fn bordered(background: Color, color: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(background)),
        border: Border {
            color,
            width: 1.0,
            radius: radius.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
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

pub fn border_style(_theme: &Theme) -> container::Style {
    bordered(PANEL, LINE, 8.0)
}

pub fn titlebar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(
            0x11 as f32 / 255.0,
            0x14 as f32 / 255.0,
            0x16 as f32 / 255.0,
        ))),
        border: Border {
            color: LINE,
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

/// Header strip at the top of the library and the inspector.
pub fn panel_header_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        border: Border {
            color: LINE,
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn sidebar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn transport_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(
            0x16 as f32 / 255.0,
            0x1a as f32 / 255.0,
            0x1d as f32 / 255.0,
        ))),
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
        background: Some(Background::Color(Color::from_rgb(
            0x11 as f32 / 255.0,
            0x14 as f32 / 255.0,
            0x16 as f32 / 255.0,
        ))),
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
    bordered(PANEL_RAISED, LINE, 8.0)
}

pub fn soft_card(_theme: &Theme) -> container::Style {
    bordered(PANEL_SOFT, LINE, 8.0)
}

pub fn inset_card(_theme: &Theme) -> container::Style {
    bordered(Color::from_rgb(0x0f as f32 / 255.0, 0x12 as f32 / 255.0, 0x14 as f32 / 255.0), LINE, 6.0)
}

pub fn selected_card(_theme: &Theme) -> container::Style {
    bordered(CYAN_SOFT, CYAN, 8.0)
}

pub fn error_card(_theme: &Theme) -> container::Style {
    bordered(RED_SOFT, RED, 8.0)
}

/// A neutral pill used for status badges (denoise state, export state, counts).
pub fn pill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(Color {
            a: 0.14,
            ..color
        })),
        border: Border {
            color: Color { a: 0.35, ..color },
            width: 1.0,
            radius: 999.0.into(),
        },
        text_color: Some(color),
        ..container::Style::default()
    }
}

pub fn scrim_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
        ..container::Style::default()
    }
}

pub fn tool_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, border_color) = match status {
        button::Status::Hovered => (PANEL_RAISED, LINE_STRONG),
        button::Status::Pressed => (CYAN_SOFT, CYAN),
        button::Status::Disabled => (PANEL_SOFT, LINE),
        _ => (PANEL_SOFT, LINE),
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
            radius: 6.0.into(),
        },
        shadow: Shadow::default(),
        ..button::Style::default()
    }
}

pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Color::from_rgb(0.95, 0.79, 0.43),
        button::Status::Pressed => Color::from_rgb(0.82, 0.62, 0.24),
        button::Status::Disabled => Color::from_rgb(0.24, 0.22, 0.17),
        _ => ACCENT,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: match status {
            button::Status::Disabled => MUTED,
            _ => Color::from_rgb(0.10, 0.09, 0.06),
        },
        border: Border {
            color: ACCENT,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..button::Style::default()
    }
}

pub fn danger_button(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => Color::from_rgb(0.42, 0.23, 0.21),
            button::Status::Pressed => Color::from_rgb(0.48, 0.25, 0.23),
            button::Status::Disabled => PANEL_SOFT,
            _ => RED_SOFT,
        })),
        text_color: if matches!(status, button::Status::Disabled) {
            MUTED
        } else {
            RED
        },
        border: Border {
            color: if matches!(status, button::Status::Disabled) {
                LINE
            } else {
                Color { a: 0.45, ..RED }
            },
            width: 1.0,
            radius: 6.0.into(),
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
        border: rounded(6.0),
        ..button::Style::default()
    }
}

pub fn record_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if active {
                if hovered {
                    Color::from_rgb(0.42, 0.20, 0.18)
                } else {
                    Color::from_rgb(0.34, 0.17, 0.16)
                }
            } else if hovered {
                Color::from_rgb(0.20, 0.25, 0.24)
            } else {
                PANEL_RAISED
            })),
            text_color: if active { RED } else { TEXT },
            border: Border {
                color: if active { RED } else { LINE_STRONG },
                width: 1.0,
                radius: 6.0.into(),
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
        background: Background::Color(Color::from_rgb(
            0x0e as f32 / 255.0,
            0x11 as f32 / 255.0,
            0x13 as f32 / 255.0,
        )),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 5.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: TEXT,
        selection: CYAN_SOFT,
    }
}

pub fn rule_style(_theme: &Theme) -> rule::Style {
    rule::Style {
        color: LINE,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

pub fn scrollable_style(
    _theme: &Theme,
    _status: scrollable::Status,
) -> scrollable::Style {
    let rail = scrollable::Rail {
        background: None,
        border: rounded(0.0),
        scroller: scrollable::Scroller {
            background: Background::Color(Color {
                a: 0.55,
                ..LINE_STRONG
            }),
            border: rounded(3.0),
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(Color::from_rgba(0.11, 0.13, 0.125, 0.9)),
            border: rounded(12.0),
            shadow: Shadow::default(),
            icon: TEXT_SOFT,
        },
    }
}

pub fn pick_list_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let hovered = matches!(
        status,
        pick_list::Status::Hovered | pick_list::Status::Opened { .. }
    );
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: MUTED,
        handle_color: MUTED,
        background: Background::Color(if hovered {
            PANEL_RAISED
        } else {
            Color::from_rgb(0x0e as f32 / 255.0, 0x11 as f32 / 255.0, 0x13 as f32 / 255.0)
        }),
        border: Border {
            color: if hovered { LINE_STRONG } else { LINE },
            width: 1.0,
            radius: 5.0.into(),
        },
    }
}

pub fn menu_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(
            0x1c as f32 / 255.0,
            0x21 as f32 / 255.0,
            0x25 as f32 / 255.0,
        ))),
        border: Border {
            color: LINE_STRONG,
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Some(TEXT),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 22.0,
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_keeps_text_readable_on_every_surface() {
        fn luminance(color: Color) -> f32 {
            0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b
        }
        for surface in [BG, PANEL, PANEL_SOFT, PANEL_RAISED] {
            assert!(
                luminance(TEXT) - luminance(surface) > 0.55,
                "text contrast is too low on {surface:?}"
            );
            assert!(
                luminance(TEXT_SOFT) - luminance(surface) > 0.45,
                "soft text contrast is too low on {surface:?}"
            );
        }
        assert!(luminance(ACCENT) > luminance(PANEL));
        assert!(luminance(CYAN) > luminance(PANEL));
    }

    #[test]
    fn disabled_buttons_stay_visually_quiet() {
        let disabled = tool_button(&theme(), button::Status::Disabled);
        let active = tool_button(&theme(), button::Status::Active);
        assert_eq!(disabled.text_color, MUTED);
        assert_eq!(active.text_color, TEXT);
    }
}
