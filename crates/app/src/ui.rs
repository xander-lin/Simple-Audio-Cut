//! View layer: pure functions from application state to iced elements.
//!
//! Layout, typography and interaction decisions are documented in `DESIGN.md`.
//! Three breakpoints keep the same hierarchy on every window size:
//!
//! * wide (≥1280): library | timeline | inspector
//! * medium (≥940): library | timeline (inspector collapses into a bar)
//! * narrow: library strip on top, timeline below

use iced::widget::canvas;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::Space;
use iced::widget::{
    button, checkbox, column, container, pick_list, responsive, row, rule, scrollable, stack, text,
    text_input,
};
use iced::{Alignment, Element, Font, Length, Padding, Size};

use simple_audio_cut_core::denoise::DenoiseCheckState;
use simple_audio_cut_core::region::Region;

use crate::message::Message;
use crate::model::{
    ClipStatus, ContextMenu, ContextMenuKind, DenoiseStatus, MediaClip, MediaId, Track,
};
use crate::theme;
use crate::update::App;
use crate::waveform::{canvas_height, TrackCanvas};

const LABEL_WIDTH: f32 = 164.0;
const HANDLE_WIDTH: f32 = 10.0;
/// Horizontal chrome around a waveform row: handle, label, card padding,
/// scrollbar and the timeline panel margins.
const ROW_CHROME: f32 = 56.0;
const LIBRARY_WIDTH: f32 = 264.0;
const INSPECTOR_WIDTH: f32 = 280.0;
const TITLEBAR_HEIGHT: f32 = 54.0;
/// Approximate height of the transport strip, used to anchor overlays.
const TRANSPORT_HEIGHT: f32 = 62.0;
/// Fallback threshold used until the monitor size is known.
const WIDE_MIN_WIDTH: f32 = 1180.0;
/// A window covering at least this share of its monitor counts as full screen.
const FULL_SCREEN_SHARE: f32 = 0.72;

/// How the main regions are stacked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Full screen: library | timeline | inspector, side by side.
    Columns,
    /// Half screen or smaller: library strip on top, timeline below.
    Stacked,
}

/// Picks the layout from the window width and the size of its monitor.
///
/// A tiled half-screen window on a 4K display is ~1920px wide — wider than
/// most "wide" breakpoints, yet there is no room for three columns. Comparing
/// against the monitor is what makes the rule hold on every screen.
pub fn layout_for(window_width: f32, monitor_width: Option<f32>) -> Layout {
    let wide_enough = window_width >= WIDE_MIN_WIDTH;
    let full_screen = match monitor_width {
        Some(monitor) if monitor.is_finite() && monitor > 0.0 => {
            window_width >= monitor * FULL_SCREEN_SHARE
        }
        _ => wide_enough,
    };
    if wide_enough && full_screen {
        Layout::Columns
    } else {
        Layout::Stacked
    }
}

/// Width available for one waveform canvas inside a given window width.
pub fn canvas_width(window_width: f32) -> f32 {
    (window_width - LABEL_WIDTH - HANDLE_WIDTH - ROW_CHROME).max(160.0)
}

pub fn format_time_standard(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0:00".into();
    }
    let hours = (seconds / 3600.0).floor() as u64;
    let minutes = ((seconds % 3600.0) / 60.0).floor() as u64;
    let secs = (seconds % 60.0).floor() as u64;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

pub fn format_time_compact(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0s".into();
    }
    let hours = (seconds / 3600.0).floor() as u64;
    let minutes = ((seconds % 3600.0) / 60.0).floor() as u64;
    let secs = (seconds % 60.0).floor() as u64;
    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    parts.push(format!("{secs}s"));
    parts.join("")
}

pub fn format_time_precise(seconds: f64) -> String {
    let safe = if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    };
    let minutes = (safe / 60.0).floor() as u64;
    let secs = safe % 60.0;
    format!("{minutes}:{secs:05.2}")
}

/// Ruler labels: sub-second ticks need one decimal, longer spans do not.
pub fn format_ruler_time(seconds: f64, step: f64) -> String {
    let safe = if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    };
    let hours = (safe / 3600.0).floor() as u64;
    let minutes = ((safe % 3600.0) / 60.0).floor() as u64;
    let secs = safe % 60.0;
    if step < 1.0 {
        if hours > 0 {
            format!("{hours}:{minutes:02}:{secs:04.1}")
        } else {
            format!("{minutes}:{secs:04.1}")
        }
    } else if hours > 0 {
        format!("{hours}:{minutes:02}:{:02}", secs.floor() as u64)
    } else {
        format!("{minutes}:{:02}", secs.floor() as u64)
    }
}

fn mono() -> Font {
    Font::MONOSPACE
}

fn strong_font() -> Font {
    Font {
        weight: iced::font::Weight::Semibold,
        ..Font::DEFAULT
    }
}

fn micro_label(content: &str) -> Element<'_, Message> {
    text(content.to_uppercase())
        .size(9.0)
        .color(theme::MUTED)
        .font(Font {
            weight: iced::font::Weight::Bold,
            ..Font::DEFAULT
        })
        .into()
}

fn section_kicker(content: &str) -> Element<'_, Message> {
    text(content.to_uppercase())
        .size(10.0)
        .color(theme::MUTED)
        .font(Font {
            weight: iced::font::Weight::Bold,
            ..Font::DEFAULT
        })
        .into()
}

fn keycap(label: &str) -> Element<'_, Message> {
    container(text(label.to_string()).size(9.0).color(theme::TEXT_SOFT))
        .padding(Padding {
            top: 1.0,
            bottom: 1.0,
            left: 4.0,
            right: 4.0,
        })
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::PANEL_RAISED)),
            border: iced::Border {
                color: theme::LINE,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn denoise_color(status: DenoiseStatus) -> iced::Color {
    match status {
        DenoiseStatus::Complete => theme::GREEN,
        DenoiseStatus::Queued | DenoiseStatus::Processing => theme::ACCENT,
        DenoiseStatus::Failed => theme::RED,
        DenoiseStatus::Unavailable => theme::MUTED,
        DenoiseStatus::Skipped => theme::BLUE,
    }
}

fn clip_color(clip: &MediaClip) -> iced::Color {
    if matches!(clip.status, ClipStatus::Failed(_)) {
        theme::RED
    } else if clip.processing() {
        theme::ACCENT
    } else {
        denoise_color(clip.denoise.status)
    }
}

fn status_dot(color: iced::Color, size: f32) -> Element<'static, Message> {
    container(Space::new())
        .width(size)
        .height(size)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::Border {
                radius: (size / 2.0).into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

fn pill(label: impl std::fmt::Display, color: iced::Color) -> Element<'static, Message> {
    container(text(label.to_string()).size(9.0).color(color))
        .padding(Padding {
            top: 1.0,
            bottom: 1.0,
            left: 7.0,
            right: 7.0,
        })
        .style(theme::pill(color))
        .into()
}

/// Flexible horizontal gap. iced's `space()` helper is `Shrink`-sized, which
/// silently collapses to zero inside rows; this one actually expands.
fn spacer() -> Element<'static, Message> {
    Space::new().width(Length::Fill).into()
}

fn divider() -> Element<'static, Message> {
    rule::horizontal(1).style(theme::rule_style).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let base = column![titlebar(app), body(app)]
        .width(Length::Fill)
        .height(Length::Fill);

    // Children publish first, so a drop on the timeline runs `DropItem` and
    // this trailing `DragCancel` becomes a no-op; releasing anywhere else
    // simply cancels.
    let base = iced::widget::mouse_area(base).on_release(Message::DragCancel);

    let mut layers: Vec<Element<'_, Message>> = vec![base.into()];
    if let Some(menu) = &app.ui.context_menu {
        layers.push(context_menu_overlay(app, menu));
    }
    if app.ui.settings_open {
        layers.push(settings_overlay(app));
    }
    if app.ui.confirm_quit {
        layers.push(confirm_quit_overlay(app));
    }
    if app.ui.dragging.is_some() || app.ui.files_hovering {
        layers.push(drop_hint(app));
    }
    stack(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn body(app: &App) -> Element<'_, Message> {
    let monitor_width = app.monitor_size.map(|size| size.width);
    // `responsive` only rebuilds when its own size changes, so it must not be
    // the place that decides the layout: the monitor probe would then be
    // ignored until the next resize.
    match layout_for(app.window_size.width, monitor_width) {
        Layout::Columns => row![
            library_sidebar(app),
            timeline(app, true),
            inspector_panel(app),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        Layout::Stacked => column![library_strip(app), timeline(app, false)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    }
}

// ---------------------------------------------------------------- title bar

fn titlebar(app: &App) -> Element<'_, Message> {
    let recording = app.session.recording.is_some();
    let busy = app.ui.recording_busy;
    let record_label = if busy {
        "Working…".into()
    } else if recording {
        format!(
            "Stop  {}",
            format_time_standard(app.session.recording_seconds)
        )
    } else {
        "● Record".into()
    };
    let record_button = button(text(record_label).size(12.0).font(strong_font()))
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 14.0,
            right: 14.0,
        })
        .on_press_maybe((!busy).then_some(Message::RecordToggle))
        .style(theme::record_button(recording));

    let import_button = button(text("Import").size(12.0))
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 14.0,
            right: 14.0,
        })
        .on_press(Message::ImportPressed)
        .style(theme::tool_button);

    let lufs = text_input("-14", &app.ui.target_lufs_input)
        .on_input(Message::TargetLufsInput)
        .on_submit(Message::TargetLufsSubmit)
        .width(54.0)
        .size(11.0)
        .font(mono())
        .style(theme::input_style);

    let changed = app.session.changed_tracks().len();
    let pending = app.session.pending_imports() + app.session.pending_denoise();
    let summary = if changed > 0 || pending > 0 {
        let mut parts = Vec::new();
        if changed > 0 {
            parts.push(format!("{changed} unexported"));
        }
        if pending > 0 {
            parts.push(format!("{pending} processing"));
        }
        parts.join("  ·  ")
    } else {
        "Up to date".into()
    };
    let summary_color = if changed > 0 || pending > 0 {
        theme::ACCENT
    } else {
        theme::GREEN
    };

    let provider = app
        .session
        .denoise_status
        .as_ref()
        .map(|status| {
            (
                status.provider_name.clone(),
                status.ready,
                status.summary.clone(),
            )
        })
        .unwrap_or_else(|| ("Denoising".into(), false, "Open denoising settings".into()));
    let provider_ready = provider.1;
    let provider_automatic_off = app
        .session
        .denoise_status
        .as_ref()
        .is_some_and(|status| !status.effective.enabled && status.has_provider());
    let provider_button = button(
        row![
            status_dot(
                if provider_ready {
                    theme::GREEN
                } else if provider_automatic_off {
                    theme::ACCENT
                } else {
                    theme::RED
                },
                7.0
            ),
            text(provider.0.clone()).size(11.0),
            text("⚙").size(11.0).color(theme::MUTED),
        ]
        .spacing(6.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 7.0,
        bottom: 7.0,
        left: 10.0,
        right: 10.0,
    })
    .style(theme::tool_button)
    .on_press(Message::OpenSettings);

    container(
        row![
            row![
                text("Simple Audio Cut").size(14.0).font(strong_font()),
                text("LOCAL   ·   NO UPLOAD")
                    .size(8.0)
                    .color(theme::MUTED)
                    .font(Font {
                        weight: iced::font::Weight::Bold,
                        ..Font::DEFAULT
                    }),
            ]
            .spacing(10.0)
            .align_y(Alignment::Center)
            .width(Length::Fill),
            row![record_button, import_button]
                .spacing(7.0)
                .align_y(Alignment::Center),
            divider_vertical(18.0),
            row![
                text("Target").size(10.0).color(theme::MUTED),
                lufs,
                text("LUFS").size(9.0).color(theme::MUTED),
            ]
            .spacing(6.0)
            .align_y(Alignment::Center),
            divider_vertical(18.0),
            row![
                text(summary).size(11.0).color(summary_color),
                provider_button,
            ]
            .spacing(12.0)
            .align_y(Alignment::Center),
        ]
        .spacing(12.0)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(TITLEBAR_HEIGHT))
    .padding(Padding {
        top: 0.0,
        bottom: 0.0,
        left: 16.0,
        right: 12.0,
    })
    .style(theme::titlebar_style)
    .into()
}

fn divider_vertical(height: f32) -> Element<'static, Message> {
    container(Space::new())
        .width(1.0)
        .height(height)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(theme::LINE)),
            ..container::Style::default()
        })
        .into()
}

// ------------------------------------------------------------------ library

fn library_sidebar(app: &App) -> Element<'_, Message> {
    let rows: Vec<Element<'_, Message>> = app
        .session
        .media
        .iter()
        .map(|clip| library_row(app, clip))
        .collect();

    let content: Element<'_, Message> = if rows.is_empty() {
        container(
            column![
                text("No source media").size(12.0).color(theme::TEXT_SOFT),
                text("Record or import audio to begin.")
                    .size(10.0)
                    .color(theme::MUTED),
            ]
            .spacing(4.0),
        )
        .padding(Padding {
            top: 16.0,
            bottom: 16.0,
            left: 12.0,
            right: 12.0,
        })
        .into()
    } else {
        scrollable(column(rows).spacing(2.0).padding(Padding {
            top: 6.0,
            bottom: 8.0,
            left: 6.0,
            right: 6.0,
        }))
        .direction(Direction::Vertical(
            Scrollbar::new().width(3.0).scroller_width(3.0),
        ))
        .height(Length::Fill)
        .style(theme::scrollable_style)
        .into()
    };

    container(
        column![
            container(
                row![
                    section_kicker("Library"),
                    spacer(),
                    text(format!("{}", app.session.media.len()))
                        .size(10.0)
                        .color(theme::MUTED)
                        .font(mono()),
                ]
                .spacing(8.0)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .padding(Padding {
                    top: 10.0,
                    bottom: 8.0,
                    left: 12.0,
                    right: 12.0,
                }),
            )
            .width(Length::Fill)
            .style(theme::panel_header_style),
            divider(),
            content,
            divider(),
            container(
                text(if app.session.media.is_empty() {
                    "Record or import to fill the library"
                } else {
                    "Enter adds the selected take  ·  + adds it directly"
                })
                .size(9.0)
                .color(theme::MUTED),
            )
            .padding(Padding {
                top: 8.0,
                bottom: 8.0,
                left: 12.0,
                right: 12.0,
            }),
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(LIBRARY_WIDTH))
    .height(Length::Fill)
    .style(theme::sidebar_style)
    .into()
}

fn library_row<'a>(app: &'a App, clip: &'a MediaClip) -> Element<'a, Message> {
    let ready = clip.ready();
    let selected = app.session.selected_media == Some(clip.id);
    let renaming = app.ui.rename.as_ref().filter(|draft| draft.id == clip.id);
    let name_value = renaming
        .map(|draft| draft.value.as_str())
        .unwrap_or(clip.name.as_str());

    let name = text_input(clip.name.as_str(), name_value)
        .on_input(move |value| Message::RenameMedia(clip.id, value))
        .on_submit(Message::RenameCommit)
        .size(11.0)
        .padding(Padding {
            top: 3.0,
            bottom: 3.0,
            left: 6.0,
            right: 6.0,
        })
        .width(Length::Fill)
        .style(theme::input_style);

    let meta = row![
        text(format_time_standard(clip.duration))
            .size(9.0)
            .color(theme::MUTED)
            .font(mono()),
        text("·").size(9.0).color(theme::LINE_STRONG),
        text(
            clip.lufs
                .map(|value| format!("{value:.1} LUFS"))
                .unwrap_or_else(|| "-- LUFS".into())
        )
        .size(9.0)
        .color(theme::MUTED)
        .font(mono()),
    ]
    .spacing(5.0)
    .align_y(Alignment::Center);

    let status = clip.denoise.status;
    let state =
        button(
            text(clip.status_label().to_string())
                .size(9.0)
                .color(clip_color(clip)),
        )
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 2.0,
            right: 2.0,
        })
        .style(theme::flat_button)
        .on_press_maybe(status.action_target().filter(|_| ready).map(|enabled| {
            Message::SetDenoise {
                id: clip.id,
                enabled,
            }
        }));

    let add = button(text("+").size(13.0))
        .padding(Padding {
            top: 2.0,
            bottom: 2.0,
            left: 7.0,
            right: 7.0,
        })
        .style(theme::tool_button)
        .on_press_maybe(ready.then_some(Message::AddToTimeline(clip.id)));

    let remove = button(text("–").size(12.0))
        .padding(Padding {
            top: 2.0,
            bottom: 2.0,
            left: 7.0,
            right: 7.0,
        })
        .style(theme::flat_button)
        .on_press(Message::DeleteMedia(clip.id));

    let mut body = column![
        row![name, add, remove]
            .spacing(3.0)
            .align_y(Alignment::Center),
        row![status_dot(clip_color(clip), 6.0), meta, spacer(), state]
            .spacing(6.0)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    ]
    .spacing(3.0)
    .width(Length::Fill);

    if let Some(error) = clip.error() {
        body = body.push(text(error.to_string()).size(9.0).color(theme::RED));
    } else if matches!(clip.status, ClipStatus::Failed(_)) && clip.source_path.is_some() {
        body = body.push(
            button(text("Retry").size(9.0))
                .padding(2.0)
                .style(theme::tool_button)
                .on_press(Message::RetryImport(clip.id)),
        );
    }

    let failed = matches!(clip.status, ClipStatus::Failed(_));
    let card_style: fn(&iced::Theme) -> container::Style = if selected {
        theme::selected_card
    } else if failed {
        theme::error_card
    } else {
        theme::soft_card
    };
    let card = container(body.width(Length::Fill))
        .padding(Padding {
            top: 7.0,
            bottom: 7.0,
            left: 8.0,
            right: 6.0,
        })
        .width(Length::Fill)
        .style(card_style);

    // A press both selects the take and arms a drag; releasing over the
    // timeline adds it as a track. (`Message::DragItem` selects first, so a
    // plain click behaves exactly like before.)
    iced::widget::mouse_area(card)
        .on_press(Message::DragItem(clip.id))
        .into()
}

/// Narrow windows get a horizontal library strip instead of the sidebar.
fn library_strip(app: &App) -> Element<'_, Message> {
    let cards: Vec<Element<'_, Message>> = app
        .session
        .media
        .iter()
        .map(|clip| library_card(app, clip))
        .collect();
    let content: Element<'_, Message> = if cards.is_empty() {
        container(
            row![
                text("No source media").size(11.0).color(theme::TEXT_SOFT),
                text("Record or import audio to begin")
                    .size(10.0)
                    .color(theme::MUTED),
            ]
            .spacing(8.0)
            .align_y(Alignment::Center),
        )
        .padding(10.0)
        .into()
    } else {
        scrollable(row(cards).spacing(6.0).padding(Padding {
            top: 6.0,
            bottom: 6.0,
            left: 8.0,
            right: 8.0,
        }))
        .direction(Direction::Horizontal(
            Scrollbar::new().width(2.0).scroller_width(2.0),
        ))
        .width(Length::Fill)
        .height(Length::Shrink)
        .style(theme::scrollable_style)
        .into()
    };

    container(
        column![
            row![
                section_kicker("Library"),
                spacer(),
                text(format!("{}", app.session.media.len()))
                    .size(9.0)
                    .color(theme::MUTED)
                    .font(mono()),
            ]
            .spacing(8.0)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .padding(Padding {
                top: 6.0,
                bottom: 0.0,
                left: 12.0,
                right: 12.0,
            }),
            content,
        ]
        .spacing(2.0),
    )
    .width(Length::Fill)
    .style(theme::panel_header_style)
    .into()
}

fn library_card<'a>(app: &'a App, clip: &'a MediaClip) -> Element<'a, Message> {
    let ready = clip.ready();
    let selected = app.session.selected_media == Some(clip.id);
    let duration = if ready {
        format_time_standard(clip.duration)
    } else {
        "--:--".into()
    };
    let lufs = clip
        .lufs
        .map(|value| format!("{value:.1} LUFS"))
        .unwrap_or_else(|| "-- LUFS".into());

    let add = button(text("+").size(12.0))
        .padding(4.0)
        .style(theme::tool_button)
        .on_press_maybe(ready.then_some(Message::AddToTimeline(clip.id)));
    let remove = button(text("–").size(11.0))
        .padding(4.0)
        .style(theme::flat_button)
        .on_press(Message::DeleteMedia(clip.id));

    let card = container(
        column![
            row![
                text(clip.name.clone()).size(11.0).width(Length::Fill),
                add,
                remove,
            ]
            .spacing(4.0)
            .align_y(Alignment::Center),
            row![
                text(duration).size(9.0).color(theme::MUTED).font(mono()),
                text(lufs).size(9.0).color(theme::MUTED).font(mono()),
                spacer(),
                text(clip.status_label().to_string())
                    .size(9.0)
                    .color(clip_color(clip)),
            ]
            .spacing(8.0)
            .align_y(Alignment::Center),
        ]
        .spacing(4.0)
        .width(Length::Fill),
    )
    .padding(8.0)
    .width(Length::Fixed(196.0))
    .style(if selected {
        theme::selected_card
    } else {
        theme::soft_card
    });

    iced::widget::mouse_area(card)
        .on_press(Message::DragItem(clip.id))
        .into()
}

// ----------------------------------------------------------------- timeline

fn timeline(app: &App, wide: bool) -> Element<'_, Message> {
    let header = transport(app);

    let body: Element<'_, Message> = if app.session.tracks.is_empty() {
        empty_timeline()
    } else {
        let rows: Vec<Element<'_, Message>> = app
            .session
            .tracks
            .iter()
            .map(|track| track_row(app, track))
            .collect();
        let total: f64 = app.session.tracks.iter().map(Track::duration).sum();
        scrollable(
            column(rows)
                .push(
                    container(
                        row![
                            text(format!(
                                "End of timeline · {} · {}",
                                if app.session.tracks.len() == 1 {
                                    "1 track".to_string()
                                } else {
                                    format!("{} tracks", app.session.tracks.len())
                                },
                                format_time_standard(total)
                            ))
                            .size(9.0)
                            .color(theme::MUTED),
                            spacer(),
                            text("New takes open as their own track")
                                .size(9.0)
                                .color(theme::MUTED),
                        ]
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                    )
                    .padding(Padding {
                        top: 2.0,
                        bottom: 0.0,
                        left: 12.0,
                        right: 10.0,
                    }),
                )
                .spacing(6.0)
                .padding(Padding {
                    top: 6.0,
                    bottom: 12.0,
                    left: 12.0,
                    right: 12.0,
                }),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::scrollable_style)
        .into()
    };

    let dragging = app.ui.dragging.is_some();
    let body = iced::widget::mouse_area(container(body).width(Length::Fill).height(Length::Fill))
        // `on_release` only fires while the cursor is over this widget, which is
        // exactly the "dropped on the timeline" test.
        .on_release(Message::DropItem);

    let mut content = column![header];
    if !wide {
        content = content.push(inspector_bar(app));
    }
    let mut panel = container(
        content
            .push(body)
            .push(status_bar(app))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill);
    if dragging {
        panel = panel.style(theme::drop_target_style);
    } else {
        panel = panel.style(theme::panel_style);
    }
    panel.into()
}

fn empty_timeline() -> Element<'static, Message> {
    container(
        column![
            text("Build your timeline").size(20.0).font(strong_font()),
            text("Add a take from the library. Each track keeps its own edits and exports as one WAV.")
                .size(11.0)
                .color(theme::MUTED),
            row![
                keycap("Drag"),
                text("add take").size(9.0).color(theme::MUTED),
                keycap("Enter"),
                text("add selected").size(9.0).color(theme::MUTED),
                keycap("Left-drag"),
                text("mute range").size(9.0).color(theme::MUTED),
                keycap("Right-drag →"),
                text("delete").size(9.0).color(theme::MUTED),
                keycap("Space"),
                text("play").size(9.0).color(theme::MUTED),
            ]
            .spacing(8.0)
            .align_y(Alignment::Center),
        ]
        .spacing(10.0)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn transport(app: &App) -> Element<'_, Message> {
    let selected = app.session.selected_track();
    let title = selected
        .map(|track| track.clip.name.clone())
        .unwrap_or_else(|| {
            if app.session.tracks.is_empty() {
                "Untitled session".into()
            } else {
                "Select a track".into()
            }
        });
    let duration = selected.map(Track::duration).unwrap_or(0.0);
    let playing = app.session.playback.is_some();
    let changed = app.session.changed_tracks().len();
    let zoom_locked = selected.is_some_and(|track| track.zoom_locked);
    let pps = selected
        .map(|track| track.pixels_per_second)
        .unwrap_or(48.0);

    let play = button(text(if playing { "⏸" } else { "▶" }).size(15.0))
        .padding(Padding {
            top: 6.0,
            bottom: 6.0,
            left: 14.0,
            right: 14.0,
        })
        .style(theme::primary_button)
        .on_press_maybe(selected.is_some().then_some(Message::PlayPause));

    let stop = button(text("■").size(10.0))
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 10.0,
            right: 10.0,
        })
        .style(theme::tool_button)
        .on_press_maybe(playing.then_some(Message::StopPlayback));

    let zoom_out = button(text("−").size(13.0))
        .padding(Padding {
            top: 3.0,
            bottom: 3.0,
            left: 8.0,
            right: 8.0,
        })
        .style(theme::flat_button)
        .on_press_maybe(selected.is_some().then_some(Message::ZoomSelected(false)));
    let zoom_in = button(text("+").size(12.0))
        .padding(Padding {
            top: 3.0,
            bottom: 3.0,
            left: 8.0,
            right: 8.0,
        })
        .style(theme::flat_button)
        .on_press_maybe(selected.is_some().then_some(Message::ZoomSelected(true)));

    let export = button(
        row![
            text(if app.session.exporting.is_empty() {
                "Export changed".into()
            } else {
                "Exporting…".to_string()
            })
            .size(11.0)
            .font(strong_font()),
            if changed > 0 {
                pill(changed, theme::ACCENT)
            } else {
                text("").size(9.0).into()
            },
        ]
        .spacing(7.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 8.0,
        bottom: 8.0,
        left: 12.0,
        right: 12.0,
    })
    .style(theme::primary_button)
    .on_press_maybe(
        (changed > 0 && app.session.exporting.is_empty()).then_some(Message::ExportChanged),
    );

    container(
        row![
            column![
                row![
                    section_kicker("Timeline"),
                    text(if selected.is_some() {
                        format!("· {changed} changed")
                    } else {
                        String::new()
                    })
                    .size(9.0)
                    .color(if changed > 0 {
                        theme::ACCENT
                    } else {
                        theme::MUTED
                    }),
                ]
                .spacing(6.0)
                .align_y(Alignment::Center),
                text(title).size(15.0).font(strong_font()),
            ]
            .spacing(1.0)
            .width(Length::Fill),
            row![play, stop].spacing(4.0).align_y(Alignment::Center),
            column![
                text(format_time_precise(app.session.playhead))
                    .size(17.0)
                    .font(mono()),
                text(format!("of {}", format_time_precise(duration)))
                    .size(9.0)
                    .font(mono())
                    .color(theme::MUTED),
            ]
            .spacing(0.0),
            row![
                micro_label("Zoom"),
                zoom_out,
                text(if zoom_locked {
                    format!("{:.2}×", pps / 48.0)
                } else {
                    "fit".to_string()
                })
                .size(9.0)
                .font(mono())
                .color(theme::TEXT_SOFT),
                zoom_in,
            ]
            .spacing(4.0)
            .align_y(Alignment::Center),
            export,
        ]
        .spacing(14.0)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding {
        top: 9.0,
        bottom: 9.0,
        left: 14.0,
        right: 14.0,
    })
    .style(theme::transport_style)
    .into()
}

/// Compact inspector used below the medium breakpoint.
fn inspector_bar(app: &App) -> Element<'_, Message> {
    let Some(track) = app.session.selected_track() else {
        return container(
            text("Select a track to edit silence, loudness and export state.")
                .size(9.0)
                .color(theme::MUTED),
        )
        .padding(Padding {
            top: 6.0,
            bottom: 6.0,
            left: 14.0,
            right: 14.0,
        })
        .into();
    };

    let threshold = text_input("", &app.ui.silence_threshold_input)
        .on_input(Message::SilenceThresholdInput)
        .on_submit(Message::SilenceSettingsSubmit)
        .width(52.0)
        .size(10.0)
        .font(mono())
        .style(theme::input_style);
    let minimum = text_input("", &app.ui.min_silence_input)
        .on_input(Message::MinSilenceInput)
        .on_submit(Message::SilenceSettingsSubmit)
        .width(58.0)
        .size(10.0)
        .font(mono())
        .style(theme::input_style);
    let detect = button(
        text(if track.silence_detection {
            "Silence cuts on"
        } else {
            "Detect silence"
        })
        .size(9.0),
    )
    .padding(Padding {
        top: 4.0,
        bottom: 4.0,
        left: 6.0,
        right: 6.0,
    })
    .style(theme::tool_button)
    .on_press(if track.silence_detection {
        Message::ClearSilence
    } else {
        Message::DetectSilence
    });

    let selection_action: Element<'_, Message> = match track.selected_range {
        Some(range) if !track.collapsed => {
            let muted = simple_audio_cut_core::region::region_is_covered(&track.muted, range);
            button(
                text(format!(
                    "{} {}",
                    if muted { "Restore sound" } else { "Mute range" },
                    format_time_standard(range.duration())
                ))
                .size(9.0),
            )
            .padding(Padding {
                top: 4.0,
                bottom: 4.0,
                left: 6.0,
                right: 6.0,
            })
            .style(if muted {
                theme::primary_button
            } else {
                theme::tool_button
            })
            .on_press(Message::MuteRange)
            .into()
        }
        _ => text("").size(9.0).into(),
    };

    container(
        row![
            text(track.clip.name.clone())
                .size(10.0)
                .color(theme::TEXT_SOFT),
            divider_vertical(14.0),
            text("Silence").size(9.0).color(theme::MUTED),
            threshold,
            text("dBFS").size(8.0).color(theme::MUTED),
            minimum,
            text("ms").size(8.0).color(theme::MUTED),
            detect,
            selection_action,
            spacer(),
            text(if track.collapsed {
                "Collapsed · restore disabled"
            } else if app.ui.blade_mode {
                "Blade active · click to cut"
            } else {
                "Left-drag mute · right-drag delete/restore"
            })
            .size(9.0)
            .color(theme::MUTED),
            button(text("•••").size(10.0))
                .padding(3.0)
                .style(theme::flat_button)
                .on_press(Message::ContextMenu {
                    track: track.id(),
                    kind: ContextMenuKind::Actions,
                    x: (app.window_size.width - 220.0).max(4.0),
                    y: TITLEBAR_HEIGHT + TRANSPORT_HEIGHT + 30.0,
                }),
        ]
        .spacing(6.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 5.0,
        bottom: 5.0,
        left: 14.0,
        right: 14.0,
    })
    .width(Length::Fill)
    .into()
}

// ---------------------------------------------------------------- inspector

fn inspector_panel(app: &App) -> Element<'_, Message> {
    let more: Element<'_, Message> = match app.session.selected_track {
        Some(id) => button(text("•••").size(10.0))
            .padding(3.0)
            .style(theme::flat_button)
            .on_press(Message::ContextMenu {
                track: id,
                kind: ContextMenuKind::Actions,
                x: (app.window_size.width - 220.0).max(4.0),
                y: TITLEBAR_HEIGHT + 34.0,
            })
            .into(),
        None => text("").size(9.0).into(),
    };
    let header =
        container(row![section_kicker("Inspector"), spacer(), more].align_y(Alignment::Center))
            .padding(Padding {
                top: 10.0,
                bottom: 8.0,
                left: 14.0,
                right: 10.0,
            })
            .width(Length::Fill)
            .style(theme::panel_header_style);

    let content: Element<'_, Message> = match app.session.selected_track() {
        Some(track) => inspector_content(app, track),
        None => container(
            column![
                text("Nothing selected").size(12.0).color(theme::TEXT_SOFT),
                text("Select a timeline track to inspect its source, silence cuts, envelope and export state.")
                    .size(10.0)
                    .color(theme::MUTED),
            ]
            .spacing(5.0),
        )
        .padding(14.0)
        .into(),
    };

    container(column![header, divider(), content].height(Length::Fill))
        .width(Length::Fixed(INSPECTOR_WIDTH))
        .height(Length::Fill)
        .style(theme::sidebar_style)
        .into()
}

fn inspector_content<'a>(app: &'a App, track: &'a Track) -> Element<'a, Message> {
    let id = track.id();
    let channels = track.audio().channel_count();
    let sample_rate = track.audio().sample_rate();
    let lufs = track
        .clip
        .lufs
        .map(|value| format!("{value:.1} LUFS"))
        .unwrap_or_else(|| "-- LUFS".into());
    let cuts = track.deleted().len();
    let muted = track.muted.len();
    let selected_range = track.selected_range;

    let source = column![
        stat_row("Duration", format_time_precise(track.duration())),
        stat_row(
            "Sample rate",
            format!("{:.1} kHz", sample_rate as f64 / 1_000.0)
        ),
        stat_row("Channels", channels.to_string()),
        stat_row("Loudness", lufs),
    ]
    .spacing(4.0);

    let threshold = text_input("", &app.ui.silence_threshold_input)
        .on_input(Message::SilenceThresholdInput)
        .on_submit(Message::SilenceSettingsSubmit)
        .width(Length::Fill)
        .size(10.0)
        .font(mono())
        .style(theme::input_style);
    let minimum = text_input("", &app.ui.min_silence_input)
        .on_input(Message::MinSilenceInput)
        .on_submit(Message::SilenceSettingsSubmit)
        .width(Length::Fill)
        .size(10.0)
        .font(mono())
        .style(theme::input_style);
    let detect = button(
        text(if track.silence_detection {
            "Clear silence cuts"
        } else {
            "Detect silence"
        })
        .size(10.0),
    )
    .width(Length::Fill)
    .padding(5.0)
    .style(theme::tool_button)
    .on_press(if track.silence_detection {
        Message::ClearSilence
    } else {
        Message::DetectSilence
    });

    let silence = column![
        field_row("Threshold", threshold, "dBFS"),
        field_row("Minimum", minimum, "ms"),
        detect,
        text(format!("{cuts} cut(s) · {muted} muted range(s)"))
            .size(9.0)
            .color(theme::MUTED),
    ]
    .spacing(6.0);

    let envelope = column![
        text(format!("{} keyframe(s)", track.envelope.len()))
            .size(10.0)
            .color(theme::TEXT_SOFT),
        text("Click the amber curve to add a point; Alt-click a point to remove it.")
            .size(9.0)
            .color(theme::MUTED),
        button(text("Clear envelope").size(10.0))
            .width(Length::Fill)
            .padding(5.0)
            .style(theme::tool_button)
            .on_press_maybe((!track.envelope.is_empty()).then_some(Message::ClearEnvelope)),
    ]
    .spacing(6.0);

    let (export_state, export_color) = if app.session.exporting.contains(&id) {
        ("Exporting", theme::ACCENT)
    } else if app.session.failed_exports.contains(&id) {
        ("Export failed", theme::RED)
    } else if track.needs_export() {
        ("Changes pending", theme::ACCENT)
    } else {
        ("Exported", theme::GREEN)
    };
    let export = column![
        row![
            status_dot(export_color, 6.0),
            text(export_state).size(10.0).color(export_color)
        ]
        .spacing(6.0)
        .align_y(Alignment::Center),
        button(text("Force export this track").size(10.0))
            .width(Length::Fill)
            .padding(5.0)
            .style(theme::tool_button)
            .on_press_maybe(
                (track.clip.ready() && app.session.exporting.is_empty())
                    .then_some(Message::ExportTrack(id))
            ),
    ]
    .spacing(6.0);

    let actions = column![
        button(
            text(if track.collapsed {
                "Show all edits"
            } else {
                "Collapse deleted audio"
            })
            .size(10.0)
        )
        .width(Length::Fill)
        .padding(5.0)
        .style(theme::tool_button)
        .on_press_maybe((cuts > 0).then_some(Message::ToggleCollapse(id))),
        button(text("Move to library").size(10.0))
            .width(Length::Fill)
            .padding(5.0)
            .style(theme::tool_button)
            .on_press(Message::ReturnTrackToPool(id)),
        button(text("Delete track").size(10.0))
            .width(Length::Fill)
            .padding(5.0)
            .style(theme::danger_button)
            .on_press(Message::RemoveTrack(id)),
    ]
    .spacing(5.0);

    let selection = match selected_range {
        Some(range) if !track.collapsed => {
            let is_muted = simple_audio_cut_core::region::region_is_covered(&track.muted, range);
            let mut block = column![
                text(format!(
                    "Range {} – {}",
                    format_time_precise(range.start),
                    format_time_precise(range.end)
                ))
                .size(10.0)
                .font(mono())
                .color(theme::TEXT_SOFT),
                row![
                    button(
                        text(if is_muted {
                            "Restore sound"
                        } else {
                            "Mute range"
                        })
                        .size(10.0)
                    )
                    .width(Length::Fill)
                    .padding(5.0)
                    .style(if is_muted {
                        theme::primary_button
                    } else {
                        theme::tool_button
                    })
                    .on_press(Message::MuteRange),
                    button(text("Cut out").size(10.0))
                        .width(Length::Fill)
                        .padding(5.0)
                        .style(theme::danger_button)
                        .on_press(Message::DeleteRange),
                ]
                .spacing(6.0),
            ]
            .spacing(6.0);
            block = block.push(
                text("M toggles mute; Delete cuts the range out. Left-drag mutes a new range.")
                    .size(9.0)
                    .color(theme::MUTED),
            );
            Some(block)
        }
        _ => None,
    };

    let status = track.clip.denoise.status;
    let denoise_busy = status.is_busy();
    let denoise_on = status.is_active();
    let mut denoise = column![row![
        status_dot(denoise_color(status), 6.0),
        text(track.clip.status_label().to_string())
            .size(10.0)
            .color(denoise_color(status)),
        spacer(),
        text(if denoise_busy || denoise_on {
            app.session
                .denoise_status
                .as_ref()
                .map(|status| status.effective.device.clone())
                .unwrap_or_else(|| "cpu".into())
        } else {
            "off".to_string()
        })
        .size(9.0)
        .color(theme::MUTED)
        .font(mono()),
    ]
    .spacing(6.0)
    .align_y(Alignment::Center),]
    .spacing(6.0);
    if let Some(reason) = &track.clip.denoise.reason {
        denoise = denoise.push(text(reason.clone()).size(9.0).color(theme::MUTED));
    }
    denoise = denoise.push(
        button(text(status.action_label()).size(10.0))
            .width(Length::Fill)
            .padding(5.0)
            .style(theme::tool_button)
            .on_press_maybe(
                status
                    .action_target()
                    .map(|enabled| Message::SetDenoise { id, enabled }),
            ),
    );
    denoise = denoise.push(
        text("Denoising never blocks editing or export.")
            .size(9.0)
            .color(theme::MUTED),
    );

    let mut content = column![
        text(track.clip.name.clone()).size(14.0).font(strong_font()),
        text(if track.collapsed {
            "Collapsed view — deleted audio is hidden"
        } else {
            "Original timeline"
        })
        .size(9.0)
        .color(theme::MUTED),
        inspector_section("Source", source.into()),
        inspector_section("Denoise", denoise.into()),
        inspector_section("Silence", silence.into()),
    ]
    .spacing(12.0)
    .width(Length::Fill);

    if let Some(selection) = selection {
        content = content.push(inspector_section("Selection", selection.into()));
    }
    content = content
        .push(inspector_section("Loudness", envelope.into()))
        .push(inspector_section("Export", export.into()))
        .push(inspector_section("Track", actions.into()));

    scrollable(content.padding(Padding {
        top: 12.0,
        bottom: 16.0,
        left: 14.0,
        right: 14.0,
    }))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::scrollable_style)
    .into()
}

fn inspector_section<'a>(title: &'a str, content: Element<'a, Message>) -> Element<'a, Message> {
    column![
        container(
            text(title.to_uppercase())
                .size(9.0)
                .color(theme::MUTED)
                .font(Font {
                    weight: iced::font::Weight::Bold,
                    ..Font::DEFAULT
                })
        )
        .padding(Padding {
            top: 0.0,
            bottom: 2.0,
            left: 0.0,
            right: 0.0,
        }),
        content,
    ]
    .spacing(5.0)
    .width(Length::Fill)
    .into()
}

fn stat_row(label: &str, value: String) -> Element<'static, Message> {
    row![
        text(label.to_string()).size(10.0).color(theme::MUTED),
        spacer(),
        text(value).size(10.0).font(mono()).color(theme::TEXT_SOFT),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn field_row<'a>(
    label: &'a str,
    input: iced::widget::TextInput<'a, Message>,
    unit: &'a str,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(10.0)
            .color(theme::MUTED)
            .width(Length::Fixed(62.0)),
        input,
        text(unit).size(9.0).color(theme::MUTED),
    ]
    .spacing(6.0)
    .align_y(Alignment::Center)
    .into()
}

// --------------------------------------------------------------- track rows

fn track_row<'a>(app: &'a App, track: &'a Track) -> Element<'a, Message> {
    let id = track.id();
    let Some(cache) = app.session.cache(id) else {
        return container(
            text("Waveform cache unavailable")
                .size(11.0)
                .color(theme::MUTED),
        )
        .padding(8.0)
        .into();
    };
    let selected = app.session.selected_track == Some(id);
    let collapsed = track.collapsed;
    let duration = if collapsed {
        track.edited_duration()
    } else {
        track.duration()
    };
    let audio = track.display_audio();
    let rms = track.display_rms();
    let pinned_pps = track.pixels_per_second;
    let zoom_locked = track.zoom_locked;
    let display_duration = duration;
    let deleted = if collapsed {
        Vec::new()
    } else {
        track.deleted()
    };
    let muted = if collapsed {
        Vec::new()
    } else {
        track.muted.clone()
    };
    let envelope = if collapsed {
        Vec::new()
    } else {
        track.envelope.clone()
    };
    let edit_points = if collapsed {
        Vec::new()
    } else {
        track.edit_points.clone()
    };
    let selected_range = if collapsed {
        None
    } else {
        track.selected_range
    };
    let kept = track.kept();
    let playhead = if selected {
        Some(original_display_time(
            app.session.playhead,
            &kept,
            collapsed,
        ))
    } else {
        None
    };
    let revision = track.revision;
    let threshold = track.silence_threshold_db;
    let blade = app.ui.blade_mode && !collapsed;
    let show_envelope = selected && !collapsed;
    let show_threshold = selected;

    let fallback_width = canvas_width(app.window_size.width);
    let canvas_element = responsive(move |size| {
        let width = if size.width.is_finite() && size.width > 0.0 {
            size.width
        } else {
            fallback_width
        };
        let pps = if zoom_locked {
            pinned_pps
        } else {
            fit_scale(width, display_duration, pinned_pps)
        };
        let height = canvas_height(width, duration, pps);
        let mut program = TrackCanvas::new(cache, id, audio.clone(), rms.clone(), duration, pps);
        program.deleted = deleted.clone();
        program.muted = muted.clone();
        program.envelope = envelope.clone();
        program.edit_points = edit_points.clone();
        program.selected_range = selected_range;
        program.playhead = playhead;
        program.blade = blade;
        program.show_envelope = show_envelope;
        program.show_threshold = show_threshold;
        program.threshold_db = threshold;
        program.revision = revision;
        program.selected = selected;
        canvas(program)
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .into()
    })
    .height(Length::Shrink);

    let (export_label, export_color) = if app.session.exporting.contains(&id) {
        ("exporting", theme::ACCENT)
    } else if app.session.failed_exports.contains(&id) {
        ("failed", theme::RED)
    } else if track.needs_export() {
        ("pending", theme::ACCENT)
    } else {
        ("exported", theme::GREEN)
    };
    let handle = canvas::Canvas::new(ScaleHandle {
        track_id: id,
        color: export_color,
    })
    .width(HANDLE_WIDTH)
    .height(Length::Fill);

    let label = container(
        column![
            text(track.clip.name.clone()).size(11.0).font(strong_font()),
            text(track.clip.status_label().to_string())
                .size(9.0)
                .color(denoise_color(track.clip.denoise.status)),
            row![
                text(if collapsed { "Collapsed" } else { "Timeline" })
                    .size(9.0)
                    .color(theme::MUTED),
                text(if zoom_locked {
                    format!("{:.2}×", pinned_pps / 48.0)
                } else {
                    "fit".into()
                })
                .size(9.0)
                .color(theme::MUTED)
                .font(mono()),
                text(export_label).size(9.0).color(export_color),
            ]
            .spacing(7.0),
        ]
        .spacing(2.0),
    )
    .width(Length::Fixed(LABEL_WIDTH))
    .padding(Padding {
        top: 10.0,
        bottom: 8.0,
        left: 8.0,
        right: 6.0,
    });

    let label = iced::widget::mouse_area(label).on_press(Message::SelectTrack(id));

    container(
        row![
            handle,
            label,
            container(canvas_element)
                .width(Length::Fill)
                .height(Length::Shrink),
        ]
        .spacing(2.0)
        .align_y(Alignment::Start)
        .width(Length::Fill),
    )
    .padding(2.0)
    .width(Length::Fill)
    .style(move |_| {
        if selected {
            container::Style {
                background: Some(iced::Background::Color(theme::PANEL_SOFT)),
                border: iced::Border {
                    color: iced::Color {
                        a: 0.65,
                        ..theme::CYAN
                    },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..container::Style::default()
            }
        } else {
            container::Style {
                background: Some(iced::Background::Color(theme::BG)),
                border: iced::Border {
                    color: theme::LINE,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..container::Style::default()
            }
        }
    })
    .into()
}

/// Scale that fits `duration` on a single row of `width` pixels, used until
/// the user zooms.
fn fit_scale(width: f32, duration: f64, fallback: f32) -> f32 {
    simple_audio_cut_core::timeline::fit_pixels_per_second(width, duration).unwrap_or(fallback)
}

fn original_display_time(time: f64, kept: &[Region], collapsed: bool) -> f64 {
    if collapsed {
        simple_audio_cut_core::timeline::edited_offset_at_original_time(time, kept)
    } else {
        time
    }
}

// ---------------------------------------------------------------- status bar

fn status_bar(app: &App) -> Element<'_, Message> {
    let pending = app.session.pending_imports() + app.session.pending_denoise();
    let mut line = row![text(app.ui.message.clone())
        .size(10.0)
        .color(theme::TEXT_SOFT)
        .width(Length::Fill),]
    .align_y(Alignment::Center)
    .spacing(10.0);
    if pending > 0 {
        line = line.push(
            text(format!("{pending} processing"))
                .size(9.0)
                .color(theme::ACCENT),
        );
    }
    line = line.push(
        row![
            keycap("Space"),
            text("Play").size(9.0).color(theme::MUTED),
            keycap("J/L"),
            text("±5s").size(9.0).color(theme::MUTED),
            keycap("M"),
            text("Toggle mute").size(9.0).color(theme::MUTED),
            keycap("B"),
            text("Blade").size(9.0).color(theme::MUTED),
            keycap("[ ]"),
            text("Zoom").size(9.0).color(theme::MUTED),
        ]
        .spacing(4.0)
        .align_y(Alignment::Center),
    );

    container(line)
        .padding(Padding {
            top: 6.0,
            bottom: 6.0,
            left: 14.0,
            right: 14.0,
        })
        .width(Length::Fill)
        .style(theme::statusbar_style)
        .into()
}

/// Thin colored strip on the left of every track: wheel zooms, right click
/// opens the track menu, left click selects.
struct ScaleHandle {
    track_id: MediaId,
    color: iced::Color,
}

impl canvas::Program<Message> for ScaleHandle {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        use iced::mouse;
        // iced forwards every canvas event to the program, whether or not the
        // pointer is inside this widget. Without this guard each track's strip
        // would react to a click anywhere in the window and the last one would
        // win — which made playback always use the last track.
        let position = cursor.position_in(bounds)?;
        match event {
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let delta_y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if delta_y == 0.0 {
                    return None;
                }
                Some(canvas::Action::publish(Message::ScaleTrack(
                    self.track_id,
                    delta_y,
                )))
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Some(canvas::Action::publish(Message::SelectTrack(self.track_id)))
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                Some(canvas::Action::publish(Message::ContextMenu {
                    track: self.track_id,
                    kind: ContextMenuKind::Export,
                    x: bounds.x + position.x,
                    y: bounds.y + position.y,
                }))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            bounds.size(),
            iced::Color::from_rgba(0.09, 0.11, 0.12, 0.9),
        );
        frame.fill_rectangle(
            iced::Point::new(2.0, 4.0),
            Size::new(
                (bounds.width - 4.0).max(2.0),
                (bounds.height - 8.0).max(2.0),
            ),
            self.color,
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        _bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if cursor.is_over(_bounds) {
            iced::mouse::Interaction::ResizingHorizontally
        } else {
            iced::mouse::Interaction::default()
        }
    }
}

// --------------------------------------------------------------------- menus

fn context_menu_overlay<'a>(app: &'a App, menu: &'a ContextMenu) -> Element<'a, Message> {
    let width = 216.0_f32;
    let mut items: Vec<Element<'_, Message>> = Vec::new();

    let item = |label: String,
                message: Option<Message>,
                style: fn(&iced::Theme, button::Status) -> button::Style| {
        button(text(label).size(11.0))
            .width(Length::Fill)
            .padding(7.0)
            .style(style)
            .on_press_maybe(message)
            .into()
    };

    if menu.kind == ContextMenuKind::Export {
        let can_export = app
            .session
            .track(menu.track_id)
            .is_some_and(|track| track.clip.ready() && app.session.exporting.is_empty());
        items.push(item(
            "Force export track".into(),
            can_export.then_some(Message::ExportTrack(menu.track_id)),
            theme::flat_button,
        ));
    } else if let Some(track) = app.session.track(menu.track_id) {
        let id = track.id();
        let deleted = track.deleted().len();
        let collapsed = track.collapsed;
        let denoise_busy = track.clip.denoise.status.is_busy();
        let denoise_on = !matches!(
            track.clip.denoise.status,
            DenoiseStatus::Skipped | DenoiseStatus::Unavailable
        );
        let envelope_items = !track.envelope.is_empty();

        // Everything the inspector offers is reachable from this menu, which
        // matters in the stacked layout where the inspector panel is absent.
        items.push(item(
            if denoise_busy {
                "Denoising…".to_string()
            } else if denoise_on {
                "Use original audio".to_string()
            } else {
                "Denoise this take".to_string()
            },
            (!denoise_busy).then_some(Message::SetDenoise {
                id,
                enabled: !denoise_on,
            }),
            theme::flat_button,
        ));
        items.push(item(
            "Clear volume envelope".into(),
            envelope_items.then_some(Message::ClearEnvelope),
            theme::flat_button,
        ));
        items.push(item(
            if collapsed {
                "Show all edits".to_string()
            } else {
                "Collapse deleted audio".to_string()
            },
            (deleted > 0).then_some(Message::ToggleCollapse(id)),
            theme::flat_button,
        ));
        items.push(item(
            "Force export track".into(),
            (track.clip.ready() && app.session.exporting.is_empty())
                .then_some(Message::ExportTrack(id)),
            theme::flat_button,
        ));
        items.push(item(
            "Move to library".into(),
            Some(Message::ReturnTrackToPool(id)),
            theme::flat_button,
        ));
        items.push(item(
            "Delete track".into(),
            Some(Message::RemoveTrack(id)),
            theme::danger_button,
        ));
    }

    let height = (items.len() as f32) * 31.0 + 12.0;
    let x = menu
        .x
        .min((app.window_size.width - width - 6.0).max(4.0))
        .max(4.0);
    let y = menu
        .y
        .min((app.window_size.height - height - 6.0).max(4.0))
        .max(4.0);

    let panel = container(column(items).spacing(2.0))
        .padding(5.0)
        .width(width)
        .style(theme::menu_style);

    // A context menu closes on any outside click, but must not dim the editor.
    let dismiss = iced::widget::mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(Message::CloseContextMenu);

    let positioned = column![Space::new().height(y), row![Space::new().width(x), panel]];

    stack![dismiss, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[derive(Clone, Debug, PartialEq)]
struct Choice {
    value: String,
    label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

fn settings_overlay(app: &App) -> Element<'_, Message> {
    let busy = app.ui.settings.busy;
    let status = app.ui.settings.status.clone();
    let draft = app.ui.settings.draft.clone();

    let dismiss = iced::widget::mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::scrim_style),
    )
    .on_press(Message::CloseSettings);

    let header = container(
        row![
            column![
                section_kicker("Processing provider"),
                text("Denoising").size(18.0).font(strong_font()),
            ]
            .spacing(2.0)
            .width(Length::Fill),
            button(text("×").size(16.0))
                .padding(4.0)
                .style(theme::flat_button)
                .on_press(Message::CloseSettings),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 12.0,
        bottom: 12.0,
        left: 16.0,
        right: 12.0,
    });

    let mut body = column![].spacing(14.0);

    if let Some(status) = &status {
        let tone = if status.ready {
            theme::GREEN
        } else {
            theme::RED
        };
        body = body.push(
            container(
                column![
                    row![
                        text(if status.ready { "✓" } else { "!" })
                            .size(13.0)
                            .color(tone),
                        text(status.provider_name.clone()).size(12.0),
                        text(if status.ready { "Ready" } else { "Needs setup" })
                            .size(9.0)
                            .color(tone),
                    ]
                    .spacing(8.0)
                    .align_y(Alignment::Center),
                    text(status.summary.clone()).size(10.0).color(theme::MUTED),
                ]
                .spacing(5.0),
            )
            .padding(10.0)
            .width(Length::Fill)
            .style(if status.ready {
                theme::soft_card
            } else {
                theme::error_card
            }),
        );
    }

    if let Some(draft) = &draft {
        body = body.push(
            checkbox(draft.enabled)
                .label("Denoise new media automatically (each take can be switched back in the inspector)")
                .text_size(11.0)
                .size(16.0)
                .on_toggle(Message::SettingsEnabled),
        );

        body = body.push(field_label(
            "Runtime",
            "Use an environment you own. The app never installs or changes Python packages.",
        ));
        body = body.push(
            column![
                text("Python executable").size(10.0).color(theme::TEXT_SOFT),
                row![
                    text_input("/path/to/environment/bin/python", &draft.executable_path)
                        .on_input(Message::SettingsExecutable)
                        .size(11.0)
                        .padding(6.0)
                        .width(Length::Fill)
                        .style(theme::input_style),
                    button(text("Browse").size(10.0))
                        .padding(6.0)
                        .style(theme::tool_button)
                        .on_press(Message::PickExecutable),
                ]
                .spacing(6.0),
            ]
            .spacing(4.0),
        );

        body = body.push(field_label(
            "Model library",
            "Select the root folder, its checkpoints folder, or one installed model folder.",
        ));
        body = body.push(
            column![
                text("Library folder").size(10.0).color(theme::TEXT_SOFT),
                row![
                    text_input("/path/to/model-library", &draft.model_root)
                        .on_input(Message::SettingsModelRoot)
                        .size(11.0)
                        .padding(6.0)
                        .width(Length::Fill)
                        .style(theme::input_style),
                    button(text("Browse").size(10.0))
                        .padding(6.0)
                        .style(theme::tool_button)
                        .on_press(Message::PickModelRoot),
                ]
                .spacing(6.0),
            ]
            .spacing(4.0),
        );

        let (models, devices) = status
            .as_ref()
            .map(|status| (status.models.clone(), status.devices.clone()))
            .unwrap_or_default();
        let ready_models: Vec<String> = models
            .iter()
            .filter(|model| model.ready)
            .map(|model| model.id.clone())
            .collect();
        let model_summary = match (ready_models.len(), models.len()) {
            (0, 0) => "No models discovered yet".to_string(),
            (ready, total) if ready == total => {
                format!("All {total} discovered model(s) are complete")
            }
            (ready, total) => format!("{ready} of {total} discovered models are complete"),
        };
        let mut device_choices: Vec<Choice> = devices
            .iter()
            .map(|device| Choice {
                value: device.id.clone(),
                label: device.label.clone(),
            })
            .collect();
        if !device_choices
            .iter()
            .any(|choice| choice.value == draft.device)
        {
            device_choices.insert(
                0,
                Choice {
                    value: draft.device.clone(),
                    label: match draft.device.as_str() {
                        "auto" => "Automatic".into(),
                        "cpu" => "CPU".into(),
                        other => other.to_string(),
                    },
                },
            );
        }
        let device_selected = device_choices
            .iter()
            .find(|choice| choice.value == draft.device)
            .cloned();

        body = body.push(
            column![
                text(model_summary).size(10.0).color(theme::MUTED),
                row![
                    column![
                        text("44.1 / 48 kHz audio")
                            .size(10.0)
                            .color(theme::TEXT_SOFT),
                        pick_list(
                            ready_models.clone(),
                            Some(draft.high_sample_rate_model.clone()),
                            Message::SettingsHighModel,
                        )
                        .width(Length::Fill)
                        .text_size(11.0)
                        .style(theme::pick_list_style),
                    ]
                    .spacing(4.0)
                    .width(Length::Fill),
                    column![
                        text("Low-rate audio").size(10.0).color(theme::TEXT_SOFT),
                        pick_list(
                            ready_models,
                            Some(draft.low_sample_rate_model.clone()),
                            Message::SettingsLowModel,
                        )
                        .width(Length::Fill)
                        .text_size(11.0)
                        .style(theme::pick_list_style),
                    ]
                    .spacing(4.0)
                    .width(Length::Fill),
                ]
                .spacing(10.0),
                column![
                    text("Processing device").size(10.0).color(theme::TEXT_SOFT),
                    pick_list(device_choices, device_selected, |choice: Choice| {
                        Message::SettingsDevice(choice.value)
                    },)
                    .width(Length::Fill)
                    .text_size(11.0)
                    .style(theme::pick_list_style),
                ]
                .spacing(4.0),
            ]
            .spacing(8.0),
        );

        if let Some(status) = &status {
            if !status.environment_overrides.is_empty() {
                body = body.push(
                    container(
                        column![
                            text("Launch overrides are active")
                                .size(10.0)
                                .color(theme::ACCENT),
                            text(format!(
                                "{} come from environment variables. Saved values do not take effect until those overrides are removed.",
                                status.environment_overrides.join(", ")
                            ))
                            .size(10.0)
                            .color(theme::MUTED),
                        ]
                        .spacing(3.0),
                    )
                    .padding(9.0)
                    .width(Length::Fill)
                    .style(theme::soft_card),
                );
            }
            let mut checks = column![].spacing(6.0);
            for check in &status.checks {
                let tone = match check.state {
                    DenoiseCheckState::Ready => theme::GREEN,
                    DenoiseCheckState::Warning => theme::ACCENT,
                    DenoiseCheckState::Error => theme::RED,
                };
                checks = checks.push(
                    row![
                        text(if matches!(check.state, DenoiseCheckState::Ready) {
                            "✓"
                        } else {
                            "!"
                        })
                        .size(10.0)
                        .color(tone),
                        column![
                            text(check.label.clone()).size(10.0),
                            text(check.detail.clone()).size(9.0).color(theme::MUTED),
                        ]
                        .spacing(1.0),
                    ]
                    .spacing(8.0),
                );
            }
            body = body.push(
                column![
                    field_label(
                        "Validation",
                        "Checks run locally and do not download or upload anything."
                    ),
                    checks
                ]
                .spacing(8.0),
            );
        }
    } else if busy {
        body = body.push(
            text("Inspecting the local environment…")
                .size(11.0)
                .color(theme::MUTED),
        );
    }

    if let Some(note) = &app.session.denoise_note {
        body = body.push(
            container(text(note.clone()).size(10.0).color(theme::MUTED))
                .padding(9.0)
                .width(Length::Fill)
                .style(theme::soft_card),
        );
    }

    if let Some(error) = &app.ui.settings.error {
        body = body.push(
            container(text(error.clone()).size(10.0).color(theme::RED))
                .padding(9.0)
                .width(Length::Fill)
                .style(theme::error_card),
        );
    }

    let footer = container(
        row![
            spacer(),
            button(text(if busy { "Checking…" } else { "Check setup" }).size(11.0))
                .padding(7.0)
                .style(theme::tool_button)
                .on_press_maybe((!busy && draft.is_some()).then_some(Message::SettingsCheck)),
            button(text(if busy { "Working…" } else { "Save settings" }).size(11.0))
                .padding(7.0)
                .style(theme::primary_button)
                .on_press_maybe((!busy && draft.is_some()).then_some(Message::SettingsSave)),
        ]
        .spacing(8.0),
    )
    .padding(Padding {
        top: 10.0,
        bottom: 10.0,
        left: 16.0,
        right: 16.0,
    });

    let drawer = container(
        column![
            header,
            divider(),
            scrollable(body.padding(Padding {
                top: 12.0,
                bottom: 10.0,
                left: 16.0,
                right: 16.0,
            }))
            .height(Length::Fill)
            .style(theme::scrollable_style),
            divider(),
            footer,
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(500.0))
    .height(Length::Fill)
    .style(theme::drawer_style);

    stack![
        dismiss,
        row![spacer(), drawer]
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn field_label<'a>(title: &'a str, detail: &'a str) -> Element<'a, Message> {
    column![
        text(title.to_uppercase())
            .size(9.0)
            .color(theme::MUTED)
            .font(Font {
                weight: iced::font::Weight::Bold,
                ..Font::DEFAULT
            }),
        text(detail).size(10.0).color(theme::MUTED),
    ]
    .spacing(2.0)
    .into()
}

/// Thin banner that explains what a drop will do. Deliberately not a scrim:
/// the user still needs to see where the take will land.
fn drop_hint(app: &App) -> Element<'_, Message> {
    let label = if let Some(id) = app.ui.dragging {
        let name = app
            .session
            .clip_by_id(id)
            .map(|clip| clip.name.clone())
            .unwrap_or_else(|| "Take".into());
        format!("Release over the timeline to add “{name}” as a new track")
    } else {
        "Release to import the dropped files".to_string()
    };
    let banner = container(
        row![
            text("⤓").size(12.0).color(theme::ACCENT),
            text(label).size(11.0).color(theme::TEXT),
        ]
        .spacing(8.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 8.0,
        bottom: 8.0,
        left: 14.0,
        right: 14.0,
    })
    .style(theme::menu_style);

    container(banner)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::End)
        .align_x(Alignment::Center)
        .padding(Padding {
            top: 0.0,
            bottom: 54.0,
            left: 0.0,
            right: 0.0,
        })
        .into()
}

fn confirm_quit_overlay(app: &App) -> Element<'_, Message> {
    let _ = app;
    let scrim = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::scrim_style);

    let dialog = container(
        column![
            text("Quit Simple Audio Cut?")
                .size(17.0)
                .font(strong_font()),
            text("Recording, processing, or unexported changes are still active. Quit anyway?")
                .size(11.0)
                .color(theme::MUTED),
            row![
                spacer(),
                button(text("Keep editing").size(11.0))
                    .padding(6.0)
                    .style(theme::tool_button)
                    .on_press(Message::CancelQuit),
                button(text("Quit anyway").size(11.0))
                    .padding(6.0)
                    .style(theme::danger_button)
                    .on_press(Message::ConfirmQuit),
            ]
            .spacing(8.0),
        ]
        .spacing(10.0),
    )
    .padding(18.0)
    .width(Length::Fixed(430.0))
    .style(theme::menu_style);

    stack![
        scrim,
        container(dialog)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests;
