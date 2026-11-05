//! Palette, typography and widget styles shared by every view.
//!
//! The visual language is documented in `DESIGN.md`: flat graphite surfaces
//! separated by hairlines, one warm accent, and type rather than boxes to carry
//! hierarchy. Everything here is a pure function of the widget status so views
//! stay declarative.

use std::sync::OnceLock;

use iced::font::Weight;
use iced::theme::Palette;
use iced::widget::{button, container, pick_list, scrollable, text_input};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector};

const fn hex(rgb: u32) -> Color {
    Color::from_rgb(
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
    )
}

const fn alpha(color: Color, a: f32) -> Color {
    Color { a, ..color }
}

// ------------------------------------------------------------------ surfaces

/// Timeline and window background.
pub const BG: Color = hex(0x0c0c0e);
/// Library and inspector columns, one step above the timeline.
pub const SIDEBAR: Color = hex(0x111114);
/// Hovered list rows.
pub const HOVER: Color = hex(0x17171b);
/// Selected list rows, pressed buttons, menus.
pub const RAISED: Color = hex(0x1c1c21);
/// Recessed fields.
pub const INPUT_BG: Color = hex(0x09090b);
/// Hairline separator.
pub const LINE: Color = hex(0x222228);
/// Focus and hover outlines.
pub const LINE_STRONG: Color = hex(0x393941);

// ---------------------------------------------------------------------- text

pub const TEXT: Color = hex(0xececef);
pub const TEXT_SOFT: Color = hex(0xa7a7b0);
pub const MUTED: Color = hex(0x6f6f79);

// -------------------------------------------------------------------- signal

/// The only accent: playhead, pending export, envelope, primary focus.
pub const ACCENT: Color = hex(0xe4ad5b);
pub const RED: Color = hex(0xe5604f);
pub const GREEN: Color = hex(0x7dbf90);
/// Range selection and edit points: brightness, not a second hue.
pub const SELECTION: Color = hex(0xf2f2f4);

// ------------------------------------------------------------------ waveform

pub const WAVE_BG: Color = BG;
pub const WAVE_SELECTED_BG: Color = hex(0x131317);
/// Peaks are near-white: the audio is the brightest thing on screen.
pub const WAVE_FILL: Color = hex(0xa9b8ff);
pub const WAVE_FILL_DIM: Color = hex(0x5f6582);
pub const RMS_FILL: Color = hex(0x7d8fe8);
pub const RMS_EDGE: Color = hex(0xb4b6c0);
pub const RULER_BG: Color = BG;
pub const RULER_TICK: Color = hex(0x34343c);
pub const RULER_TEXT: Color = hex(0x6a6a74);
pub const GRID: Color = alpha(hex(0xffffff), 0.035);
pub const PLAYHEAD: Color = ACCENT;

// -------------------------------------------------------------- typography

/// Type scale. Nothing smaller than `CAPTION` appears outside the ruler.
pub mod size {
    pub const CAPTION: f32 = 11.0;
    pub const BODY: f32 = 13.0;
    pub const LEAD: f32 = 15.0;
    pub const TITLE: f32 = 20.0;
    pub const TIMECODE: f32 = 22.0;
}

pub fn medium() -> Font {
    Font {
        weight: Weight::Medium,
        ..Font::DEFAULT
    }
}

pub fn strong() -> Font {
    Font {
        weight: Weight::Semibold,
        ..Font::DEFAULT
    }
}

pub fn mono() -> Font {
    Font::MONOSPACE
}

pub fn theme() -> Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME
        .get_or_init(|| {
            Theme::custom(
                "Simple Audio Cut",
                Palette {
                    background: BG,
                    text: TEXT,
                    primary: ACCENT,
                    success: GREEN,
                    warning: ACCENT,
                    danger: RED,
                },
            )
        })
        .clone()
}

// ---------------------------------------------------------------- containers

fn fill(color: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(color)),
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

/// Window background; the timeline sits directly on it.
pub fn canvas_style(_theme: &Theme) -> container::Style {
    fill(BG)
}

/// Library and inspector columns.
pub fn sidebar_style(_theme: &Theme) -> container::Style {
    fill(SIDEBAR)
}

/// A selected list row or track label.
pub fn selected_row_style(_theme: &Theme) -> container::Style {
    fill(RAISED)
}

/// Label column of the selected track; matches the waveform behind it.
pub fn track_selected_style(_theme: &Theme) -> container::Style {
    fill(WAVE_SELECTED_BG)
}

/// One-pixel separator, horizontal or vertical depending on its size.
pub fn hairline_style(_theme: &Theme) -> container::Style {
    fill(LINE)
}

/// Two-pixel bar marking the selected row.
pub fn marker_style(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| fill(color)
}

pub fn dot_style(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(color)),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Keyboard hint: outline only, so a row of hints stays quiet.
pub fn keycap_style(_theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: LINE_STRONG,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(TEXT_SOFT),
        ..container::Style::default()
    }
}

/// Timeline while a library take is dragged over it.
pub fn drop_target_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(alpha(ACCENT, 0.035))),
        border: Border {
            color: alpha(ACCENT, 0.5),
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    }
}

pub fn scrim_style(_theme: &Theme) -> container::Style {
    fill(alpha(hex(0x000000), 0.55))
}

/// Floating surfaces: menus, dialogs, the drop banner.
pub fn popover_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(RAISED)),
        border: Border {
            color: LINE_STRONG,
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Some(TEXT),
        shadow: Shadow {
            color: alpha(hex(0x000000), 0.6),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..container::Style::default()
    }
}

/// Settings drawer: a full-height sheet sliding over the editor.
pub fn drawer_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SIDEBAR)),
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(TEXT),
        shadow: Shadow {
            color: alpha(hex(0x000000), 0.6),
            offset: Vector::new(-16.0, 0.0),
            blur_radius: 40.0,
        },
        ..container::Style::default()
    }
}

// ------------------------------------------------------------------- buttons

fn button_base(background: Option<Color>, text: Color, border: Color) -> button::Style {
    button::Style {
        background: background.map(Background::Color),
        text_color: text,
        border: Border {
            color: border,
            width: if border.a > 0.0 { 1.0 } else { 0.0 },
            radius: 6.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

const CLEAR: Color = Color::TRANSPARENT;

/// Default button: text only until hovered.
pub fn ghost_button(_theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered => button_base(Some(HOVER), TEXT, CLEAR),
        button::Status::Pressed => button_base(Some(RAISED), TEXT, CLEAR),
        button::Status::Disabled => button_base(None, alpha(MUTED, 0.6), CLEAR),
        button::Status::Active => button_base(None, TEXT_SOFT, CLEAR),
    }
}

/// Secondary action that must read as a button at rest.
pub fn outline_button(_theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered => button_base(Some(HOVER), TEXT, LINE_STRONG),
        button::Status::Pressed => button_base(Some(RAISED), TEXT, LINE_STRONG),
        button::Status::Disabled => button_base(None, alpha(MUTED, 0.6), LINE),
        button::Status::Active => button_base(None, TEXT, LINE_STRONG),
    }
}

/// The one filled button on screen at a time.
pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let ink = hex(0x17120a);
    match status {
        button::Status::Hovered => button_base(Some(hex(0xecbd74)), ink, CLEAR),
        button::Status::Pressed => button_base(Some(hex(0xc9944a)), ink, CLEAR),
        button::Status::Disabled => button_base(None, alpha(MUTED, 0.6), LINE),
        button::Status::Active => button_base(Some(ACCENT), ink, CLEAR),
    }
}

/// Destructive action: red text, red wash on hover.
pub fn danger_button(_theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => {
            button_base(Some(alpha(RED, 0.12)), RED, CLEAR)
        }
        button::Status::Disabled => button_base(None, alpha(MUTED, 0.6), CLEAR),
        button::Status::Active => button_base(None, alpha(RED, 0.85), CLEAR),
    }
}

/// Record: neutral at rest, unmistakably red while recording.
pub fn record_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if !active {
            return outline_button(theme, status);
        }
        let hovered = matches!(status, button::Status::Hovered);
        button_base(
            Some(alpha(RED, if hovered { 0.24 } else { 0.16 })),
            TEXT,
            alpha(RED, 0.7),
        )
    }
}

/// Round play control: light at rest, accent while playing.
pub fn play_button(playing: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = match status {
            button::Status::Disabled => RAISED,
            _ if playing => ACCENT,
            button::Status::Hovered => hex(0xffffff),
            button::Status::Pressed => hex(0xc8c8ce),
            button::Status::Active => TEXT,
        };
        let ink = if matches!(status, button::Status::Disabled) {
            alpha(MUTED, 0.6)
        } else {
            hex(0x0c0c0e)
        };
        let mut style = button_base(Some(background), ink, CLEAR);
        style.border.radius = 999.0.into();
        style
    }
}

/// Segmented toggle (e.g. blade mode): accent text when on.
pub fn toggle_button(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if on {
            button_base(Some(alpha(ACCENT, 0.14)), ACCENT, CLEAR)
        } else {
            ghost_button(theme, status)
        }
    }
}

/// A whole list row that behaves like a button (library takes, menu items).
pub fn row_button(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = match status {
            _ if selected => Some(RAISED),
            button::Status::Hovered | button::Status::Pressed => Some(HOVER),
            _ => None,
        };
        let mut style = button_base(background, TEXT, CLEAR);
        style.border.radius = 0.0.into();
        style
    }
}

pub fn menu_item(_theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => {
            button_base(Some(HOVER), TEXT, CLEAR)
        }
        button::Status::Disabled => button_base(None, alpha(MUTED, 0.6), CLEAR),
        button::Status::Active => button_base(None, TEXT, CLEAR),
    }
}

// -------------------------------------------------------------------- fields

pub fn input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border = match status {
        text_input::Status::Focused { .. } => alpha(ACCENT, 0.7),
        text_input::Status::Hovered => LINE_STRONG,
        _ => LINE,
    };
    text_input::Style {
        background: Background::Color(INPUT_BG),
        border: Border {
            color: border,
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: TEXT,
        selection: alpha(ACCENT, 0.3),
    }
}

/// Inline name field: reads as plain text in every state. Deliberately no
/// background and no outline — renaming must not look like a popup or a card,
/// only the caret and the selection show that the field is live.
pub fn inline_input_style(_theme: &Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(CLEAR),
        border: Border {
            color: CLEAR,
            width: 0.0,
            radius: 0.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: TEXT,
        selection: alpha(ACCENT, 0.3),
    }
}

pub fn pick_list_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let open = matches!(
        status,
        pick_list::Status::Hovered | pick_list::Status::Opened { .. }
    );
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: MUTED,
        handle_color: MUTED,
        background: Background::Color(INPUT_BG),
        border: Border {
            color: if open { LINE_STRONG } else { LINE },
            width: 1.0,
            radius: 6.0.into(),
        },
    }
}

pub fn scrollable_style(_theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let active = matches!(
        status,
        scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
    );
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(alpha(TEXT, if active { 0.22 } else { 0.08 })),
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(RAISED),
            border: Border {
                radius: 12.0.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            icon: TEXT_SOFT,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color) -> f32 {
        0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b
    }

    #[test]
    fn palette_keeps_text_readable_on_every_surface() {
        for surface in [BG, SIDEBAR, HOVER, RAISED, INPUT_BG] {
            assert!(
                luminance(TEXT) - luminance(surface) > 0.55,
                "text contrast is too low on {surface:?}"
            );
            assert!(
                luminance(TEXT_SOFT) - luminance(surface) > 0.45,
                "soft text contrast is too low on {surface:?}"
            );
        }
        assert!(luminance(ACCENT) > luminance(SIDEBAR));
        assert!(luminance(SELECTION) > luminance(SIDEBAR));
    }

    #[test]
    fn surfaces_step_up_in_a_fixed_order() {
        let order = [INPUT_BG, BG, SIDEBAR, HOVER, RAISED];
        for pair in order.windows(2) {
            assert!(luminance(pair[0]) < luminance(pair[1]));
        }
    }

    #[test]
    fn disabled_buttons_stay_visually_quiet() {
        let theme = theme();
        for style in [ghost_button, outline_button, primary_button] {
            let disabled = style(&theme, button::Status::Disabled);
            let active = style(&theme, button::Status::Active);
            assert!(disabled.text_color.a < 1.0 || disabled.text_color == MUTED);
            assert_ne!(disabled.text_color, active.text_color);
        }
    }
}
