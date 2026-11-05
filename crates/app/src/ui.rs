//! View layer: pure functions from application state to iced elements.
//!
//! The design is documented in `DESIGN.md`. In short: no cards. Regions are
//! flat surfaces separated by hairlines; hierarchy comes from type size and
//! weight; one warm accent marks what is live (playhead, selection marker,
//! pending export). Two layouts share the same hierarchy:
//!
//! * full screen: library | timeline | inspector
//! * half screen: library strip on top, timeline below, inspector as a bar

use iced::widget::canvas;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::Space;
use iced::widget::text::Wrapping;
use iced::widget::{
    button, checkbox, column, container, pick_list, responsive, row, scrollable, stack, text,
    text_input,
};
use iced::{Alignment, Element, Length, Padding, Size};

use simple_audio_cut_core::denoise::{DenoiseCheckState, DenoiseProviderStatus};
use simple_audio_cut_core::region::Region;

use crate::message::Message;
use crate::model::{
    ClipStatus, ContextMenu, ContextMenuKind, DenoiseStatus, MediaClip, MediaId, Track,
};
use crate::theme::{self, size};
use crate::update::App;
use crate::waveform::{canvas_height, TrackCanvas};

const LABEL_WIDTH: f32 = 144.0;
const HANDLE_WIDTH: f32 = 10.0;
/// Horizontal chrome around a waveform: handle, label, hairline and the
/// scrollbar gutter.
const ROW_CHROME: f32 = 20.0;
const LIBRARY_WIDTH: f32 = 248.0;
const INSPECTOR_WIDTH: f32 = 288.0;
const TOPBAR_HEIGHT: f32 = 60.0;
const TIMELINE_HEADER_HEIGHT: f32 = 52.0;
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

// ------------------------------------------------------------------ helpers

/// Flexible horizontal gap. iced's `space()` helper is `Shrink`-sized, which
/// silently collapses to zero inside rows; this one actually expands.
fn spacer() -> Element<'static, Message> {
    Space::new().width(Length::Fill).into()
}

fn gap(height: f32) -> Element<'static, Message> {
    Space::new().height(height).into()
}

/// Centres content inside a fixed-size button; without it iced pins the
/// content to the top-left corner.
fn centered<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).center(Length::Fill).into()
}

fn hairline() -> Element<'static, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(1.0)
        .style(theme::hairline_style)
        .into()
}

fn vline() -> Element<'static, Message> {
    container(Space::new())
        .width(1.0)
        .height(Length::Fill)
        .style(theme::hairline_style)
        .into()
}

fn dot(color: iced::Color, diameter: f32) -> Element<'static, Message> {
    container(Space::new())
        .width(diameter)
        .height(diameter)
        .style(theme::dot_style(color))
        .into()
}

/// Secondary line under a primary label. The only small text in the app.
fn caption<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    text(content).size(size::CAPTION).color(theme::MUTED)
}

fn body<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    text(content).size(size::BODY).color(theme::TEXT_SOFT)
}

fn heading<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    text(content)
        .size(size::BODY)
        .font(theme::medium())
        .color(theme::TEXT)
}

fn keycap(label: &'static str) -> Element<'static, Message> {
    container(text(label).size(size::CAPTION).font(theme::mono()))
        .padding(Padding {
            top: 1.0,
            bottom: 1.0,
            left: 6.0,
            right: 6.0,
        })
        .style(theme::keycap_style)
        .into()
}

fn denoise_color(status: DenoiseStatus) -> iced::Color {
    match status {
        DenoiseStatus::Complete => theme::GREEN,
        DenoiseStatus::Queued | DenoiseStatus::Processing => theme::ACCENT,
        DenoiseStatus::Failed => theme::RED,
        DenoiseStatus::Unavailable | DenoiseStatus::Skipped => theme::MUTED,
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

// --------------------------------------------------------------------- root

pub fn view(app: &App) -> Element<'_, Message> {
    let base = column![topbar(app), hairline(), main_area(app), hairline(), status_bar(app)]
        .width(Length::Fill)
        .height(Length::Fill);
    let base = container(base)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::canvas_style);

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
        layers.push(confirm_quit_overlay());
    }
    if app.ui.dragging.is_some() || app.ui.files_hovering {
        layers.push(drop_hint(app));
    }
    stack(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn main_area(app: &App) -> Element<'_, Message> {
    let monitor_width = app.monitor_size.map(|size| size.width);
    // `responsive` only rebuilds when its own size changes, so it must not be
    // the place that decides the layout: the monitor probe would then be
    // ignored until the next resize.
    match layout_for(app.window_size.width, monitor_width) {
        Layout::Columns => row![
            library_sidebar(app),
            vline(),
            timeline(app, true),
            vline(),
            inspector_panel(app),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        Layout::Stacked => column![library_strip(app), hairline(), timeline(app, false)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    }
}

// ------------------------------------------------------------------ top bar

/// One bar for the whole session: name on the left, transport in the centre,
/// capture and output on the right.
fn topbar(app: &App) -> Element<'_, Message> {
    let brand = text("Simple Audio Cut")
        .size(size::LEAD)
        .font(theme::strong())
        .color(theme::TEXT);

    // Brand and actions keep their natural width; the flexible gaps absorb
    // the rest, so the transport sits centred when there is room and never
    // squeezes the export button when there is not.
    // Below ~1100px the name would push the export button off screen; the
    // window title already carries it.
    let mut bar = row![].spacing(20.0).align_y(Alignment::Center);
    if app.window_size.width >= 1100.0 {
        bar = bar.push(brand).push(spacer());
    }
    container(
        bar.push(transport(app))
            .push(spacer())
            .push(session_actions(app)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(TOPBAR_HEIGHT))
    .padding(Padding {
        top: 0.0,
        bottom: 0.0,
        left: 20.0,
        right: 16.0,
    })
    .center_y(Length::Fixed(TOPBAR_HEIGHT))
    .style(theme::sidebar_style)
    .into()
}

fn transport(app: &App) -> Element<'_, Message> {
    let selected = app.session.selected_track();
    let playing = app.session.playback.is_some();
    let duration = selected.map(Track::duration).unwrap_or(0.0);

    let play = button(centered(
        text(if playing { "■" } else { "▶" }).size(if playing { 12.0 } else { 13.0 }),
    ))
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(36.0))
    .padding(0.0)
    .style(theme::play_button(playing))
    .on_press_maybe(selected.is_some().then_some(Message::PlayPause));

    row![
        play,
        row![
            text(format_time_precise(app.session.playhead))
                .size(size::TIMECODE)
                .font(theme::mono())
                .color(if selected.is_some() {
                    theme::TEXT
                } else {
                    theme::MUTED
                }),
            text(format!("/ {}", format_time_precise(duration)))
                .size(size::BODY)
                .font(theme::mono())
                .color(theme::MUTED),
        ]
        .spacing(10.0)
        .align_y(Alignment::Center),
    ]
    .spacing(16.0)
    .align_y(Alignment::Center)
    .into()
}

fn session_actions(app: &App) -> Element<'_, Message> {
    let recording = app.session.recording.is_some();
    let busy = app.ui.recording_busy;
    let record_label = if busy {
        "Working…".to_string()
    } else if recording {
        format_time_standard(app.session.recording_seconds)
    } else {
        "Record".to_string()
    };
    let record = button(centered(
        row![
            dot(theme::RED, 8.0),
            text(record_label)
                .size(size::BODY)
                .font(if recording { theme::mono() } else { theme::medium() }),
        ]
        .spacing(8.0)
        .align_y(Alignment::Center),
    ))
    .width(Length::Fixed(104.0))
    .height(Length::Fixed(34.0))
    .padding(0.0)
    .style(theme::record_button(recording))
    .on_press_maybe((!busy).then_some(Message::RecordToggle));

    let import = button(centered(text("Import").size(size::BODY)))
        .width(Length::Fixed(72.0))
        .height(Length::Fixed(34.0))
        .padding(0.0)
        .style(theme::ghost_button)
        .on_press(Message::ImportPressed);

    let status = app.session.denoise_status.as_ref();
    let ready = status.is_some_and(|status| status.ready);
    let automatic_off =
        status.is_some_and(|status| !status.effective.enabled && status.has_provider());
    let denoise_tone = if ready {
        theme::GREEN
    } else if automatic_off {
        theme::ACCENT
    } else {
        theme::MUTED
    };
    let denoise = button(centered(
        row![
            dot(denoise_tone, 6.0),
            text(if ready { "Denoise" } else { "No denoise" }).size(size::BODY),
        ]
        .spacing(8.0)
        .align_y(Alignment::Center),
    ))
    .width(Length::Fixed(112.0))
    .height(Length::Fixed(34.0))
    .padding(0.0)
    .style(theme::ghost_button)
    .on_press(Message::OpenSettings);

    let changed = app.session.changed_tracks().len();
    let exporting = !app.session.exporting.is_empty();
    let export_label = if exporting {
        "Exporting…".to_string()
    } else if changed > 0 {
        format!("Export {changed}")
    } else {
        "Exported".to_string()
    };
    let export = button(centered(
        text(export_label).size(size::BODY).font(theme::medium()),
    ))
    .width(Length::Fixed(104.0))
    .height(Length::Fixed(34.0))
    .padding(0.0)
    .style(theme::primary_button)
    .on_press_maybe((changed > 0 && !exporting).then_some(Message::ExportChanged));

    row![denoise, import, record, export]
        .spacing(8.0)
        .align_y(Alignment::Center)
        .into()
}

// ------------------------------------------------------------------ library

fn section_title(title: &'static str, count: Option<usize>) -> Element<'static, Message> {
    let mut line = row![text(title)
        .size(size::BODY)
        .font(theme::strong())
        .color(theme::TEXT)]
    .spacing(8.0)
    .align_y(Alignment::Center);
    if let Some(count) = count {
        line = line.push(caption(count.to_string()).font(theme::mono()));
    }
    line.into()
}

/// Header row shared by the three columns so their titles share a baseline.
fn column_header<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(TIMELINE_HEADER_HEIGHT))
        .center_y(Length::Fixed(TIMELINE_HEADER_HEIGHT))
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 20.0,
            right: 20.0,
        })
        .into()
}

/// Loudness target for new takes. Lives with the library because it only
/// affects media as it comes in.
fn loudness_target(app: &App) -> Element<'_, Message> {
    row![
        body("Normalize to"),
        text_input("-14", &app.ui.target_lufs_input)
            .on_input(Message::TargetLufsInput)
            .on_submit(Message::TargetLufsSubmit)
            .width(52.0)
            .size(size::BODY)
            .font(theme::mono())
            .padding(Padding {
                top: 4.0,
                bottom: 4.0,
                left: 8.0,
                right: 8.0,
            })
            .style(theme::input_style),
        caption("LUFS"),
    ]
    .spacing(8.0)
    .align_y(Alignment::Center)
    .into()
}

fn library_sidebar(app: &App) -> Element<'_, Message> {
    let content: Element<'_, Message> = if app.session.media.is_empty() {
        container(
            column![
                heading("No takes yet"),
                caption("Record or import audio to begin."),
            ]
            .spacing(4.0),
        )
        .padding(Padding {
            top: 8.0,
            bottom: 0.0,
            left: 20.0,
            right: 20.0,
        })
        .into()
    } else {
        let rows: Vec<Element<'_, Message>> = app
            .session
            .media
            .iter()
            .map(|clip| library_row(app, clip))
            .collect();
        scrollable(column(rows))
            .direction(Direction::Vertical(
                Scrollbar::new().width(4.0).scroller_width(4.0),
            ))
            .height(Length::Fill)
            .style(theme::scrollable_style)
            .into()
    };

    container(
        column![
            column_header(section_title("Library", Some(app.session.media.len()))),
            hairline(),
            gap(6.0),
            container(content).height(Length::Fill),
            hairline(),
            container(loudness_target(app)).padding(Padding {
                top: 14.0,
                bottom: 14.0,
                left: 20.0,
                right: 20.0,
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
    let failed = matches!(clip.status, ClipStatus::Failed(_));
    let renaming = app.ui.rename.as_ref().is_some_and(|draft| draft.id == clip.id);
    // The name is plain text until it is being renamed. Right-click one of the
    // two name targets (this row, or the track label) to open the inline field.
    let name: Element<'_, Message> = if renaming {
        let value = app
            .ui
            .rename
            .as_ref()
            .map(|draft| draft.value.as_str())
            .unwrap_or(clip.name.as_str());
        text_input(clip.name.as_str(), value)
            .id(name_input_id(clip.id))
            .on_input(move |value| Message::RenameMedia(clip.id, value))
            .on_submit(Message::RenameCommit)
            .size(size::BODY)
            .font(theme::medium())
            .padding(Padding {
                top: 2.0,
                bottom: 2.0,
                left: 0.0,
                right: 4.0,
            })
            .width(Length::Fill)
            .style(theme::inline_input_style)
            .into()
    } else {
        text(clip.name.clone())
            .size(size::BODY)
            .font(theme::medium())
            .color(if selected { theme::TEXT } else { theme::TEXT_SOFT })
            .wrapping(Wrapping::None)
            .width(Length::Fill)
            .into()
    };

    let duration = if ready {
        format_time_standard(clip.duration)
    } else {
        "–:––".into()
    };
    let loudness = clip
        .lufs
        .map(|value| format!("{value:.1} LUFS"))
        .unwrap_or_default();
    let status = clip.denoise.status;
    let state = button(
        text(clip.status_label().to_string())
            .size(size::CAPTION)
            .color(clip_color(clip)),
    )
    .padding(Padding {
        top: 1.0,
        bottom: 1.0,
        left: 4.0,
        right: 4.0,
    })
    .style(theme::ghost_button)
    .on_press_maybe(
        status
            .action_target()
            .filter(|_| ready)
            .map(|enabled| Message::SetDenoise {
                id: clip.id,
                enabled,
            }),
    );

    let meta = row![
        caption(duration).font(theme::mono()),
        caption(loudness).font(theme::mono()),
        spacer(),
        state,
    ]
    .spacing(10.0)
    .align_y(Alignment::Center);

    let add = button(centered(text("+").size(size::LEAD)))
        .width(26.0)
        .height(26.0)
        .padding(0.0)
        .style(theme::ghost_button)
        .on_press_maybe(ready.then_some(Message::AddToTimeline(clip.id)));
    let remove = button(centered(text("×").size(size::LEAD)))
        .width(26.0)
        .height(26.0)
        .padding(0.0)
        .style(theme::ghost_button)
        .on_press(Message::DeleteMedia(clip.id));

    let mut content = column![
        row![name, add, remove].spacing(2.0).align_y(Alignment::Center),
        meta,
    ]
    .spacing(2.0)
    .width(Length::Fill);

    if let Some(error) = clip.error() {
        content = content.push(text(error.to_string()).size(size::CAPTION).color(theme::RED));
        if clip.source_path.is_some() {
            content = content.push(
                button(text("Retry import").size(size::CAPTION))
                    .padding(Padding {
                        top: 2.0,
                        bottom: 2.0,
                        left: 0.0,
                        right: 0.0,
                    })
                    .style(theme::ghost_button)
                    .on_press(Message::RetryImport(clip.id)),
            );
        }
    }

    let marker_color = if failed {
        theme::RED
    } else if selected {
        theme::ACCENT
    } else {
        iced::Color::TRANSPARENT
    };
    let row_body = row![
        container(Space::new())
            .width(2.0)
            .height(Length::Fill)
            .style(theme::marker_style(marker_color)),
        container(content).padding(Padding {
            top: 10.0,
            bottom: 10.0,
            left: 16.0,
            right: 12.0,
        }),
    ]
    .height(Length::Shrink);

    let row_body = container(row_body).width(Length::Fill);
    let row_body = if selected {
        row_body.style(theme::selected_row_style)
    } else {
        row_body
    };

    // A left press selects the take and arms a drag; releasing over the
    // timeline adds it as a track. A right press opens the inline name field,
    // so renaming works in the library before the take joins the timeline.
    iced::widget::mouse_area(row_body)
        .on_press(Message::DragItem(clip.id))
        .on_right_press(Message::BeginRename(clip.id))
        .into()
}

/// Half-screen layout: the library becomes one horizontal strip.
fn library_strip(app: &App) -> Element<'_, Message> {
    let items: Element<'_, Message> = if app.session.media.is_empty() {
        caption("No takes yet. Record or import audio to begin.").into()
    } else {
        let chips: Vec<Element<'_, Message>> = app
            .session
            .media
            .iter()
            .map(|clip| library_chip(app, clip))
            .collect();
        scrollable(row(chips).spacing(4.0))
            .direction(Direction::Horizontal(
                Scrollbar::new().width(3.0).scroller_width(3.0).spacing(4.0),
            ))
            .width(Length::Fill)
            .style(theme::scrollable_style)
            .into()
    };

    container(
        row![
            section_title("Library", Some(app.session.media.len())),
            container(items).width(Length::Fill),
            loudness_target(app),
        ]
        .spacing(20.0)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding {
        top: 10.0,
        bottom: 10.0,
        left: 20.0,
        right: 16.0,
    })
    .style(theme::sidebar_style)
    .into()
}

fn library_chip<'a>(app: &'a App, clip: &'a MediaClip) -> Element<'a, Message> {
    let ready = clip.ready();
    let selected = app.session.selected_media == Some(clip.id);
    let duration = if ready {
        format_time_standard(clip.duration)
    } else {
        "–:––".into()
    };
    let renaming = app.ui.rename.as_ref().is_some_and(|draft| draft.id == clip.id);
    let name: Element<'_, Message> = if renaming {
        let value = app
            .ui
            .rename
            .as_ref()
            .map(|draft| draft.value.as_str())
            .unwrap_or(clip.name.as_str());
        text_input(clip.name.as_str(), value)
            .id(name_input_id(clip.id))
            .on_input(move |value| Message::RenameMedia(clip.id, value))
            .on_submit(Message::RenameCommit)
            .size(size::BODY)
            .font(theme::medium())
            .padding(Padding::ZERO)
            .width(Length::Fixed(140.0))
            .style(theme::inline_input_style)
            .into()
    } else {
        text(clip.name.clone())
            .size(size::BODY)
            .color(if selected { theme::TEXT } else { theme::TEXT_SOFT })
            .into()
    };
    let chip = row![
        dot(clip_color(clip), 6.0),
        name,
        caption(duration).font(theme::mono()),
        button(centered(text("+").size(size::LEAD)))
            .width(24.0)
            .height(24.0)
            .padding(0.0)
            .style(theme::ghost_button)
            .on_press_maybe(ready.then_some(Message::AddToTimeline(clip.id))),
        button(centered(text("×").size(size::LEAD)))
            .width(24.0)
            .height(24.0)
            .padding(0.0)
            .style(theme::ghost_button)
            .on_press(Message::DeleteMedia(clip.id)),
    ]
    .spacing(8.0)
    .align_y(Alignment::Center);

    let chip = container(chip).padding(Padding {
        top: 4.0,
        bottom: 4.0,
        left: 12.0,
        right: 4.0,
    });
    let chip = if selected {
        chip.style(theme::selected_row_style)
    } else {
        chip
    };
    iced::widget::mouse_area(chip)
        .on_press(Message::DragItem(clip.id))
        .on_right_press(Message::BeginRename(clip.id))
        .into()
}

// ----------------------------------------------------------------- timeline

fn timeline(app: &App, wide: bool) -> Element<'_, Message> {
    let body: Element<'_, Message> = if app.session.tracks.is_empty() {
        empty_timeline()
    } else {
        let mut rows = column![];
        for track in &app.session.tracks {
            rows = rows.push(track_row(app, track)).push(hairline());
        }
        let hint = if app.session.media.is_empty() {
            "Record or import to add another track"
        } else {
            "Drag a take here to add it as a track"
        };
        rows = rows.push(
            container(caption(hint))
                .width(Length::Fill)
                .padding(Padding {
                    top: 28.0,
                    bottom: 28.0,
                    left: 0.0,
                    right: 0.0,
                })
                .center_x(Length::Fill),
        );
        scrollable(rows)
            .direction(Direction::Vertical(
                Scrollbar::new().width(4.0).scroller_width(4.0),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::scrollable_style)
            .into()
    };

    let body = iced::widget::mouse_area(container(body).width(Length::Fill).height(Length::Fill))
        // `on_release` only fires while the cursor is over this widget, which is
        // exactly the "dropped on the timeline" test.
        .on_release(Message::DropItem);

    let mut content = column![timeline_header(app), hairline()];
    if !wide {
        if let Some(bar) = inspector_bar(app) {
            content = content.push(bar).push(hairline());
        }
    }
    let panel = container(content.push(body).width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill);
    if app.ui.dragging.is_some() {
        panel.style(theme::drop_target_style).into()
    } else {
        panel.style(theme::canvas_style).into()
    }
}

fn timeline_header(app: &App) -> Element<'_, Message> {
    let selected = app.session.selected_track();
    let count = app.session.tracks.len();
    let total: f64 = app.session.tracks.iter().map(Track::duration).sum();
    let summary = if count == 0 {
        String::new()
    } else {
        format!(
            "{count} {}  ·  {}",
            if count == 1 { "track" } else { "tracks" },
            format_time_standard(total)
        )
    };
    let zoom_label = match selected {
        Some(track) if track.zoom_locked => format!("{:.2}×", track.pixels_per_second / 48.0),
        Some(_) => "Fit".into(),
        None => String::new(),
    };
    let has_track = selected.is_some();
    let zoom_button = |label: &'static str, zoom_in: bool| {
        button(centered(text(label).size(size::LEAD)))
            .width(28.0)
            .height(28.0)
            .padding(0.0)
            .style(theme::ghost_button)
            .on_press_maybe(has_track.then_some(Message::ZoomSelected(zoom_in)))
    };

    // The selected track is already named in its row and in the inspector;
    // the header describes the session instead.
    let mut line = row![
        text("Timeline")
            .size(size::LEAD)
            .font(theme::strong())
            .color(theme::TEXT),
        caption(summary).font(theme::mono()),
        spacer(),
    ]
    .spacing(12.0)
    .align_y(Alignment::Center);

    if let Some(track) = selected {
        line = line
            .push(
                button(centered(text("Blade").size(size::BODY)))
                    .width(64.0)
                    .height(28.0)
                    .padding(0.0)
                    .style(theme::toggle_button(app.ui.blade_mode))
                    .on_press(Message::SetBlade(!app.ui.blade_mode)),
            )
            .push(
                row![
                    zoom_button("−", false),
                    text(zoom_label)
                        .size(size::BODY)
                        .font(theme::mono())
                        .color(theme::TEXT_SOFT)
                        .width(Length::Fixed(52.0))
                        .align_x(text::Alignment::Center),
                    zoom_button("+", true),
                ]
                .align_y(Alignment::Center),
            )
            .push(
                button(centered(text("•••").size(size::BODY)))
                    .width(32.0)
                    .height(28.0)
                    .padding(0.0)
                    .style(theme::ghost_button)
                    .on_press(Message::ContextMenu {
                        track: track.id(),
                        kind: ContextMenuKind::Actions,
                        x: (app.window_size.width - 240.0).max(4.0),
                        y: TOPBAR_HEIGHT + TIMELINE_HEADER_HEIGHT,
                    }),
            );
    }

    container(line)
        .width(Length::Fill)
        .height(Length::Fixed(TIMELINE_HEADER_HEIGHT))
        .center_y(Length::Fixed(TIMELINE_HEADER_HEIGHT))
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 20.0,
            right: 12.0,
        })
        .into()
}

fn empty_timeline() -> Element<'static, Message> {
    let hint = |key: &'static str, label: &'static str| {
        row![keycap(key), caption(label)]
            .spacing(8.0)
            .align_y(Alignment::Center)
    };
    container(
        column![
            text("Build your timeline")
                .size(size::TITLE)
                .font(theme::strong())
                .color(theme::TEXT),
            body("Add a take from the library. Each track keeps its own edits and exports as one WAV."),
            gap(8.0),
            row![
                hint("Drag", "add a take"),
                hint("Left-drag →", "mute"),
                hint("Left-drag ←", "restore"),
                hint("Right-drag", "delete"),
                hint("Space", "play"),
            ]
            .spacing(20.0),
        ]
        .spacing(10.0)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center(Length::Fill)
    .into()
}

// ---------------------------------------------------------------- inspector

fn inspector_panel(app: &App) -> Element<'_, Message> {
    let content: Element<'_, Message> = match app.session.selected_track() {
        Some(track) => inspector_content(app, track),
        None => container(
            column![
                heading("Nothing selected"),
                caption("Select a track to edit silence, loudness and export."),
            ]
            .spacing(4.0),
        )
        .padding(Padding {
            top: 8.0,
            bottom: 0.0,
            left: 20.0,
            right: 20.0,
        })
        .into(),
    };

    container(
        column![
            column_header(section_title("Inspector", None)),
            hairline(),
            content,
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(INSPECTOR_WIDTH))
    .height(Length::Fill)
    .style(theme::sidebar_style)
    .into()
}

/// One inspector section: a title, then content. Sections are separated by
/// hairlines, never boxed.
fn section<'a>(title: &'static str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![
        hairline(),
        container(
            column![
                text(title)
                    .size(size::CAPTION)
                    .font(theme::medium())
                    .color(theme::MUTED),
                content.into(),
            ]
            .spacing(12.0),
        )
        .padding(Padding {
            top: 16.0,
            bottom: 18.0,
            left: 20.0,
            right: 20.0,
        }),
    ]
    .into()
}

fn stat(label: &'static str, value: String) -> Element<'static, Message> {
    row![
        body(label),
        spacer(),
        text(value)
            .size(size::BODY)
            .font(theme::mono())
            .color(theme::TEXT),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn field<'a>(
    label: &'static str,
    input: iced::widget::TextInput<'a, Message>,
    unit: &'static str,
) -> Element<'a, Message> {
    row![
        body(label).width(Length::Fill),
        input,
        caption(unit).width(Length::Fixed(32.0)),
    ]
    .spacing(8.0)
    .align_y(Alignment::Center)
    .into()
}

fn number_input<'a>(
    value: &'a str,
    on_input: fn(String) -> Message,
    width: f32,
) -> iced::widget::TextInput<'a, Message> {
    text_input("", value)
        .on_input(on_input)
        .on_submit(Message::SilenceSettingsSubmit)
        .width(width)
        .size(size::BODY)
        .font(theme::mono())
        .padding(Padding {
            top: 4.0,
            bottom: 4.0,
            left: 8.0,
            right: 8.0,
        })
        .style(theme::input_style)
}

fn wide_button<'a>(
    label: impl text::IntoFragment<'a>,
    style: fn(&iced::Theme, button::Status) -> button::Style,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(centered(text(label).size(size::BODY)))
        .width(Length::Fill)
        .height(32.0)
        .padding(0.0)
        .style(style)
        .on_press_maybe(message)
        .into()
}

fn inspector_content<'a>(app: &'a App, track: &'a Track) -> Element<'a, Message> {
    let id = track.id();
    let cuts = track.deleted().len();
    let lufs = track
        .clip
        .lufs
        .map(|value| format!("{value:.1} LUFS"))
        .unwrap_or_else(|| "–".into());

    let source = column![
        stat("Duration", format_time_precise(track.duration())),
        stat(
            "Sample rate",
            format!("{:.1} kHz", track.audio().sample_rate() as f64 / 1_000.0)
        ),
        stat("Channels", track.audio().channel_count().to_string()),
        stat("Loudness", lufs),
    ]
    .spacing(8.0);

    let status = track.clip.denoise.status;
    let mut denoise = column![row![
        dot(denoise_color(status), 6.0),
        text(track.clip.status_label().to_string())
            .size(size::BODY)
            .color(theme::TEXT),
        spacer(),
        caption(if status.is_busy() || status.is_active() {
            app.session
                .denoise_status
                .as_ref()
                .map(|status| status.effective.device.to_uppercase())
                .unwrap_or_else(|| "CPU".into())
        } else {
            String::new()
        })
        .font(theme::mono()),
    ]
    .spacing(8.0)
    .align_y(Alignment::Center)]
    .spacing(10.0);
    if let Some(reason) = &track.clip.denoise.reason {
        denoise = denoise.push(caption(reason.clone()));
    }
    denoise = denoise.push(wide_button(
        status.action_label(),
        theme::outline_button,
        status
            .action_target()
            .map(|enabled| Message::SetDenoise { id, enabled }),
    ));

    let silence = column![
        field(
            "Threshold",
            number_input(&app.ui.silence_threshold_input, Message::SilenceThresholdInput, 64.0),
            "dBFS"
        ),
        field(
            "Minimum",
            number_input(&app.ui.min_silence_input, Message::MinSilenceInput, 64.0),
            "ms"
        ),
        wide_button(
            if track.silence_detection {
                "Clear silence cuts"
            } else {
                "Detect silence"
            },
            theme::outline_button,
            Some(if track.silence_detection {
                Message::ClearSilence
            } else {
                Message::DetectSilence
            }),
        ),
        caption(format!(
            "{cuts} {} · {} muted",
            if cuts == 1 { "cut" } else { "cuts" },
            track.muted.len()
        )),
    ]
    .spacing(10.0);

    let mut content = column![container(
        column![
            text(track.clip.name.clone())
                .size(size::TITLE)
                .font(theme::strong())
                .color(theme::TEXT),
            caption(if track.collapsed {
                "Deleted audio hidden"
            } else {
                "Original timeline"
            }),
        ]
        .spacing(4.0)
    )
    .padding(Padding {
        top: 20.0,
        bottom: 20.0,
        left: 20.0,
        right: 20.0,
    })];

    if let Some(range) = track.selected_range.filter(|_| !track.collapsed) {
        let muted = simple_audio_cut_core::region::region_is_covered(&track.muted, range);
        content = content.push(section(
            "Selection",
            column![
                text(format!(
                    "{}  –  {}",
                    format_time_precise(range.start),
                    format_time_precise(range.end)
                ))
                .size(size::BODY)
                .font(theme::mono())
                .color(theme::TEXT),
                row![
                    wide_button(
                        if muted { "Unmute" } else { "Mute" },
                        theme::outline_button,
                        Some(Message::MuteRange),
                    ),
                    wide_button("Cut out", theme::danger_button, Some(Message::DeleteRange)),
                ]
                .spacing(8.0),
            ]
            .spacing(10.0),
        ));
    }

    content = content
        .push(section("Source", source))
        .push(section("Denoise", denoise))
        .push(section("Silence", silence))
        .push(section(
            "Volume",
            column![
                row![
                    body("Envelope points"),
                    spacer(),
                    text(track.envelope.len().to_string())
                        .size(size::BODY)
                        .font(theme::mono())
                        .color(theme::TEXT),
                ]
                .align_y(Alignment::Center),
                caption("Click the curve to add a point. Alt-click removes it."),
                wide_button(
                    "Clear envelope",
                    theme::outline_button,
                    (!track.envelope.is_empty()).then_some(Message::ClearEnvelope),
                ),
            ]
            .spacing(10.0),
        ))
        .push(section("Track", track_actions(app, track)));

    scrollable(content.padding(Padding {
        top: 0.0,
        bottom: 24.0,
        left: 0.0,
        right: 0.0,
    }))
    .direction(Direction::Vertical(
        Scrollbar::new().width(4.0).scroller_width(4.0),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::scrollable_style)
    .into()
}

fn export_state(app: &App, track: &Track) -> (&'static str, iced::Color) {
    let id = track.id();
    if app.session.exporting.contains(&id) {
        ("Exporting", theme::ACCENT)
    } else if app.session.failed_exports.contains(&id) {
        ("Export failed", theme::RED)
    } else if track.needs_export() {
        ("Changed", theme::ACCENT)
    } else {
        ("Exported", theme::GREEN)
    }
}

fn track_actions<'a>(app: &'a App, track: &'a Track) -> Element<'a, Message> {
    let id = track.id();
    let (label, color) = export_state(app, track);
    column![
        row![
            dot(color, 6.0),
            text(label).size(size::BODY).color(theme::TEXT),
        ]
        .spacing(8.0)
        .align_y(Alignment::Center),
        wide_button(
            "Rename track",
            theme::ghost_button,
            Some(Message::BeginRename(id)),
        ),
        wide_button(
            "Export this track",
            theme::outline_button,
            (track.clip.ready() && app.session.exporting.is_empty())
                .then_some(Message::ExportTrack(id)),
        ),
        wide_button(
            if track.collapsed {
                "Show all edits"
            } else {
                "Collapse deleted audio"
            },
            theme::ghost_button,
            (!track.deleted().is_empty()).then_some(Message::ToggleCollapse(id)),
        ),
        wide_button(
            "Move to library",
            theme::ghost_button,
            Some(Message::ReturnTrackToPool(id)),
        ),
        wide_button("Delete track", theme::danger_button, Some(Message::RemoveTrack(id))),
    ]
    .spacing(6.0)
    .into()
}

/// Half-screen layout: the inspector's most used controls in one line. The
/// rest is in the track menu (•••).
fn inspector_bar(app: &App) -> Option<Element<'_, Message>> {
    let track = app.session.selected_track()?;

    let detect = button(centered(
        text(if track.silence_detection {
            "Clear silence"
        } else {
            "Detect silence"
        })
        .size(size::BODY),
    ))
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(30.0))
    .padding(0.0)
    .style(theme::outline_button)
    .on_press(if track.silence_detection {
        Message::ClearSilence
    } else {
        Message::DetectSilence
    });

    let mut line = row![
        body("Silence below"),
        number_input(&app.ui.silence_threshold_input, Message::SilenceThresholdInput, 56.0),
        caption("dBFS"),
        body("for"),
        number_input(&app.ui.min_silence_input, Message::MinSilenceInput, 64.0),
        caption("ms"),
        detect,
        spacer(),
    ]
    .spacing(8.0)
    .align_y(Alignment::Center);

    if let Some(range) = track.selected_range.filter(|_| !track.collapsed) {
        let muted = simple_audio_cut_core::region::region_is_covered(&track.muted, range);
        line = line
            .push(
                text(format_time_standard(range.duration()))
                    .size(size::BODY)
                    .font(theme::mono())
                    .color(theme::TEXT_SOFT),
            )
            .push(
                button(centered(text(if muted { "Unmute" } else { "Mute" }).size(size::BODY)))
                    .width(Length::Fixed(76.0))
                    .height(Length::Fixed(30.0))
                    .padding(0.0)
                    .style(theme::outline_button)
                    .on_press(Message::MuteRange),
            )
            .push(
                button(centered(text("Cut out").size(size::BODY)))
                    .width(Length::Fixed(76.0))
                    .height(Length::Fixed(30.0))
                    .padding(0.0)
                    .style(theme::danger_button)
                    .on_press(Message::DeleteRange),
            );
    }

    Some(
        container(line)
            .width(Length::Fill)
            .padding(Padding {
                top: 10.0,
                bottom: 10.0,
                left: 20.0,
                right: 16.0,
            })
            .into(),
    )
}

// --------------------------------------------------------------- track rows

/// Widget id of an inline rename field, so the app can focus it from
/// `BeginRename` without an additional state machine. A take lives either in
/// the library or on a track, never both, so one id per media is enough.
pub fn name_input_id(id: MediaId) -> iced::widget::Id {
    iced::widget::Id::from(format!("name-{id}"))
}

fn track_row<'a>(app: &'a App, track: &'a Track) -> Element<'a, Message> {
    let id = track.id();
    let Some(cache) = app.session.cache(id) else {
        return container(caption("Waveform unavailable"))
            .padding(20.0)
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
    let unless_collapsed = |value: Vec<Region>| if collapsed { Vec::new() } else { value };
    let deleted = unless_collapsed(track.deleted());
    let muted = unless_collapsed(track.muted.clone());
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
    let playhead =
        selected.then(|| original_display_time(app.session.playhead, &kept, collapsed));
    let revision = track.revision;
    let threshold = track.silence_threshold_db;
    let blade = app.ui.blade_mode && !collapsed;
    let show_envelope = selected && !collapsed;

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
            fit_scale(width, duration, pinned_pps)
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
        program.show_threshold = selected;
        program.threshold_db = threshold;
        program.revision = revision;
        program.selected = selected;
        canvas(program)
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .into()
    })
    .height(Length::Shrink);

    let (_, export_color) = export_state(app, track);
    let handle = canvas::Canvas::new(ScaleHandle {
        track_id: id,
        color: if selected {
            theme::ACCENT
        } else {
            iced::Color {
                a: 0.0,
                ..theme::ACCENT
            }
        },
    })
    .width(HANDLE_WIDTH)
    .height(Length::Fill);

    let zoom = if zoom_locked {
        format!("{:.2}×", pinned_pps / 48.0)
    } else {
        "Fit".into()
    };
    // Two lines only: the name (with the export state as a dot) and one
    // caption. Everything else lives in the inspector.
    let detail = if collapsed { "Collapsed".to_string() } else { zoom };
    let rename_draft = app
        .ui
        .rename
        .as_ref()
        .filter(|draft| draft.id == id)
        .map(|draft| draft.value.as_str());
    // Double-click renames in place: while a draft exists the label becomes a
    // bare text field (no box, no outline), otherwise it is plain text.
    let name: Element<'_, Message> = match rename_draft {
        Some(value) => text_input("Name", value)
            .id(name_input_id(id))
            .on_input(move |value| Message::RenameMedia(id, value))
            .on_submit(Message::RenameCommit)
            .size(size::BODY)
            .font(theme::medium())
            .padding(Padding::ZERO)
            .width(Length::Fill)
            .style(theme::inline_input_style)
            .into(),
        None => text(track.clip.name.clone())
            .size(size::BODY)
            .font(theme::medium())
            .color(if selected { theme::TEXT } else { theme::TEXT_SOFT })
            .wrapping(Wrapping::None)
            .into(),
    };
    let label = container(
        column![
            row![
                dot(export_color, 6.0),
                name,
            ]
            .spacing(8.0)
            .width(Length::Fill)
            .align_y(Alignment::Center),
            container(
                caption(format!(
                    "{}  ·  {}",
                    format_time_standard(duration),
                    detail
                ))
                .font(theme::mono())
                .wrapping(Wrapping::None)
            )
            .padding(Padding {
                top: 0.0,
                bottom: 0.0,
                left: 14.0,
                right: 0.0,
            }),
        ]
        .spacing(4.0),
    )
    .width(Length::Fixed(LABEL_WIDTH))
    .height(Length::Fill)
    .clip(true)
    .padding(Padding {
        top: 16.0,
        bottom: 12.0,
        left: 10.0,
        right: 8.0,
    });
    let label = if selected {
        label.style(theme::track_selected_style)
    } else {
        label
    };
    // Right-click the name to rename in place. A left press still selects.
    // Double-click is deliberately not wired: a gesture that must work every
    // time should not depend on click timing.
    let label = iced::widget::mouse_area(label)
        .on_press(Message::SelectTrack(id))
        .on_right_press(Message::BeginRename(id));

    row![
        handle,
        label,
        container(canvas_element)
            .width(Length::Fill)
            .height(Length::Shrink),
    ]
    .height(Length::Shrink)
    .width(Length::Fill)
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
    let mut line = row![body(app.ui.message.clone()).width(Length::Fill)]
        .spacing(16.0)
        .align_y(Alignment::Center);
    if pending > 0 {
        line = line.push(
            row![
                dot(theme::ACCENT, 6.0),
                caption(format!("{pending} processing")).color(theme::ACCENT),
            ]
            .spacing(6.0)
            .align_y(Alignment::Center),
        );
    }
    let hint = |key: &'static str, label: &'static str| {
        row![keycap(key), caption(label)]
            .spacing(6.0)
            .align_y(Alignment::Center)
    };
    line = line.push(
        row![
            hint("Space", "Play"),
            hint("B", "Blade"),
            hint("[ ]", "Zoom"),
        ]
        .spacing(16.0)
        .align_y(Alignment::Center),
    );

    container(line)
        .width(Length::Fill)
        .height(Length::Fixed(36.0))
        .center_y(Length::Fixed(36.0))
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 20.0,
            right: 16.0,
        })
        .style(theme::sidebar_style)
        .into()
}

/// Thin strip on the left of every track: wheel zooms, right click opens the
/// export menu, left click selects. Painted in the accent when selected.
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
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let color = if self.color.a > 0.0 {
            self.color
        } else if cursor.is_over(bounds) {
            iced::Color {
                a: 0.35,
                ..theme::TEXT
            }
        } else {
            iced::Color::TRANSPARENT
        };
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            Size::new(2.0, bounds.height),
            color,
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if cursor.is_over(bounds) {
            iced::mouse::Interaction::ResizingHorizontally
        } else {
            iced::mouse::Interaction::default()
        }
    }
}

// --------------------------------------------------------------------- menus

fn context_menu_overlay<'a>(app: &'a App, menu: &'a ContextMenu) -> Element<'a, Message> {
    let width = 232.0_f32;
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    let item = |label: String,
                message: Option<Message>,
                style: fn(&iced::Theme, button::Status) -> button::Style| {
        button(text(label).size(size::BODY))
            .width(Length::Fill)
            .padding(Padding {
                top: 7.0,
                bottom: 7.0,
                left: 12.0,
                right: 12.0,
            })
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
            "Export this track".into(),
            can_export.then_some(Message::ExportTrack(menu.track_id)),
            theme::menu_item,
        ));
    } else if let Some(track) = app.session.track(menu.track_id) {
        let id = track.id();
        let denoise_busy = track.clip.denoise.status.is_busy();
        let denoise_on = !matches!(
            track.clip.denoise.status,
            DenoiseStatus::Skipped | DenoiseStatus::Unavailable
        );

        // Everything the inspector offers is reachable from this menu, which
        // matters in the stacked layout where the inspector panel is absent.
        items.push(item(
            "Rename track".into(),
            Some(Message::BeginRename(id)),
            theme::menu_item,
        ));
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
            theme::menu_item,
        ));
        items.push(item(
            "Clear volume envelope".into(),
            (!track.envelope.is_empty()).then_some(Message::ClearEnvelope),
            theme::menu_item,
        ));
        items.push(item(
            if track.collapsed {
                "Show all edits".to_string()
            } else {
                "Collapse deleted audio".to_string()
            },
            (!track.deleted().is_empty()).then_some(Message::ToggleCollapse(id)),
            theme::menu_item,
        ));
        items.push(item(
            "Export this track".into(),
            (track.clip.ready() && app.session.exporting.is_empty())
                .then_some(Message::ExportTrack(id)),
            theme::menu_item,
        ));
        items.push(item(
            "Move to library".into(),
            Some(Message::ReturnTrackToPool(id)),
            theme::menu_item,
        ));
        items.push(
            container(hairline())
                .padding(Padding {
                    top: 4.0,
                    bottom: 4.0,
                    left: 0.0,
                    right: 0.0,
                })
                .into(),
        );
        items.push(item(
            "Delete track".into(),
            Some(Message::RemoveTrack(id)),
            theme::danger_button,
        ));
    }

    let height = (items.len() as f32) * 34.0 + 12.0;
    let x = menu
        .x
        .min((app.window_size.width - width - 6.0).max(4.0))
        .max(4.0);
    let y = menu
        .y
        .min((app.window_size.height - height - 6.0).max(4.0))
        .max(4.0);

    let panel = container(column(items).spacing(2.0))
        .padding(6.0)
        .width(width)
        .style(theme::popover_style);

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

// ------------------------------------------------------------------ settings

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

fn path_field<'a>(
    label: &'static str,
    placeholder: &'static str,
    value: &'a str,
    on_input: fn(String) -> Message,
    browse: Message,
) -> Element<'a, Message> {
    column![
        body(label),
        row![
            text_input(placeholder, value)
                .on_input(on_input)
                .size(size::BODY)
                .padding(8.0)
                .width(Length::Fill)
                .style(theme::input_style),
            button(centered(text("Browse").size(size::BODY)))
                .width(80.0)
                .height(34.0)
                .padding(0.0)
                .style(theme::outline_button)
                .on_press(browse),
        ]
        .spacing(8.0)
        .align_y(Alignment::Center),
    ]
    .spacing(6.0)
    .into()
}

fn picker<'a, T>(
    label: &'static str,
    options: Vec<T>,
    selected: Option<T>,
    on_select: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    column![
        body(label),
        pick_list(options, selected, on_select)
            .width(Length::Fill)
            .text_size(size::BODY)
            .padding(8.0)
            .style(theme::pick_list_style),
    ]
    .spacing(6.0)
    .width(Length::Fill)
    .into()
}

/// Settings section: title and one explanatory line, then the controls.
fn settings_group<'a>(
    title: &'static str,
    detail: &'static str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        column![heading(title), caption(detail)].spacing(4.0),
        content.into(),
    ]
    .spacing(14.0)
    .into()
}

fn settings_status(status: &DenoiseProviderStatus) -> Element<'static, Message> {
    let tone = if status.ready { theme::GREEN } else { theme::RED };
    column![
        row![
            dot(tone, 8.0),
            heading(status.provider_name.clone()),
            spacer(),
            caption(if status.ready { "Ready" } else { "Needs setup" }).color(tone),
        ]
        .spacing(10.0)
        .align_y(Alignment::Center),
        caption(status.summary.clone()),
    ]
    .spacing(6.0)
    .into()
}

fn settings_checks(status: &DenoiseProviderStatus) -> Element<'static, Message> {
    let mut checks = column![].spacing(10.0);
    for check in &status.checks {
        let tone = match check.state {
            DenoiseCheckState::Ready => theme::GREEN,
            DenoiseCheckState::Warning => theme::ACCENT,
            DenoiseCheckState::Error => theme::RED,
        };
        checks = checks.push(
            row![
                container(dot(tone, 6.0)).padding(Padding {
                    top: 6.0,
                    bottom: 0.0,
                    left: 0.0,
                    right: 0.0,
                }),
                column![body(check.label.clone()).color(theme::TEXT), caption(check.detail.clone())]
                    .spacing(2.0),
            ]
            .spacing(10.0),
        );
    }
    checks.into()
}

fn settings_overlay(app: &App) -> Element<'_, Message> {
    let busy = app.ui.settings.busy;
    let status = app.ui.settings.status.as_ref();
    let draft = app.ui.settings.draft.as_ref();

    let dismiss = iced::widget::mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::scrim_style),
    )
    .on_press(Message::CloseSettings);

    let header = container(
        row![
            text("Denoising")
                .size(size::TITLE)
                .font(theme::strong())
                .color(theme::TEXT),
            spacer(),
            button(centered(text("×").size(size::TITLE)))
                .width(32.0)
                .height(32.0)
                .padding(0.0)
                .style(theme::ghost_button)
                .on_press(Message::CloseSettings),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 20.0,
        bottom: 16.0,
        left: 28.0,
        right: 20.0,
    });

    let mut sections: Vec<Element<'_, Message>> = Vec::new();
    if let Some(status) = status {
        sections.push(settings_status(status));
    }

    if let Some(draft) = draft {
        sections.push(
            checkbox(draft.enabled)
                .label("Denoise new takes automatically")
                .text_size(size::BODY)
                .size(16.0)
                .on_toggle(Message::SettingsEnabled)
                .into(),
        );
        sections.push(settings_group(
            "Runtime",
            "Use a Python environment you own. Nothing is installed or changed.",
            column![
                path_field(
                    "Python executable",
                    "/path/to/environment/bin/python",
                    &draft.executable_path,
                    Message::SettingsExecutable,
                    Message::PickExecutable,
                ),
                path_field(
                    "Model library",
                    "/path/to/model-library",
                    &draft.model_root,
                    Message::SettingsModelRoot,
                    Message::PickModelRoot,
                ),
            ]
            .spacing(14.0),
        ));
        sections.push(settings_models(status, draft));
    } else if busy {
        sections.push(body("Inspecting the local environment…").into());
    }

    if let Some(status) = status {
        if !status.environment_overrides.is_empty() {
            sections.push(
                column![
                    body("Launch overrides are active").color(theme::ACCENT),
                    caption(format!(
                        "{} come from environment variables. Saved values apply once those are removed.",
                        status.environment_overrides.join(", ")
                    )),
                ]
                .spacing(4.0)
                .into(),
            );
        }
        if !status.checks.is_empty() {
            sections.push(settings_group(
                "Checks",
                "Run locally. Nothing is downloaded or uploaded.",
                settings_checks(status),
            ));
        }
    }
    if let Some(note) = &app.session.denoise_note {
        sections.push(caption(note.clone()).into());
    }
    if let Some(error) = &app.ui.settings.error {
        sections.push(body(error.clone()).color(theme::RED).into());
    }

    let mut content = column![].spacing(0.0);
    for (index, section) in sections.into_iter().enumerate() {
        if index > 0 {
            content = content.push(
                container(hairline()).padding(Padding {
                    top: 22.0,
                    bottom: 22.0,
                    left: 0.0,
                    right: 0.0,
                }),
            );
        }
        content = content.push(section);
    }

    let can_act = !busy && draft.is_some();
    let footer = container(
        row![
            spacer(),
            button(centered(
                text(if busy { "Checking…" } else { "Check setup" }).size(size::BODY)
            ))
            .width(112.0)
            .height(36.0)
            .padding(0.0)
            .style(theme::ghost_button)
            .on_press_maybe(can_act.then_some(Message::SettingsCheck)),
            button(centered(
                text(if busy { "Working…" } else { "Save" })
                    .size(size::BODY)
                    .font(theme::medium())
            ))
            .width(96.0)
            .height(36.0)
            .padding(0.0)
            .style(theme::primary_button)
            .on_press_maybe(can_act.then_some(Message::SettingsSave)),
        ]
        .spacing(8.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 14.0,
        bottom: 14.0,
        left: 28.0,
        right: 20.0,
    });

    let drawer = container(
        column![
            header,
            scrollable(content.padding(Padding {
                top: 8.0,
                bottom: 24.0,
                left: 28.0,
                right: 28.0,
            }))
            .direction(Direction::Vertical(
                Scrollbar::new().width(4.0).scroller_width(4.0),
            ))
            .height(Length::Fill)
            .style(theme::scrollable_style),
            hairline(),
            footer,
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(480.0))
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

fn settings_models<'a>(
    status: Option<&'a DenoiseProviderStatus>,
    draft: &'a simple_audio_cut_core::denoise::DenoiseConfig,
) -> Element<'a, Message> {
    let (models, devices) = status
        .map(|status| (status.models.clone(), status.devices.clone()))
        .unwrap_or_default();
    let ready_models: Vec<String> = models
        .iter()
        .filter(|model| model.ready)
        .map(|model| model.id.clone())
        .collect();
    let summary = match (ready_models.len(), models.len()) {
        (0, 0) => "No models found yet.".to_string(),
        (ready, total) if ready == total => format!("{total} models found, all complete."),
        (ready, total) => format!("{ready} of {total} models are complete."),
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

    let content = column![
        caption(summary),
        row![
            picker(
                "44.1 / 48 kHz",
                ready_models.clone(),
                Some(draft.high_sample_rate_model.clone()),
                Message::SettingsHighModel,
            ),
            picker(
                "Lower rates",
                ready_models,
                Some(draft.low_sample_rate_model.clone()),
                Message::SettingsLowModel,
            ),
        ]
        .spacing(12.0),
        picker("Device", device_choices, device_selected, |choice: Choice| {
            Message::SettingsDevice(choice.value)
        }),
    ]
    .spacing(14.0);

    settings_group(
        "Models",
        "Pick the model for each sample rate and where it runs.",
        content,
    )
}

// ----------------------------------------------------------------- overlays

/// Banner that explains what a drop will do. Deliberately not a scrim: the
/// user still needs to see where the take will land.
fn drop_hint(app: &App) -> Element<'_, Message> {
    let label = if let Some(id) = app.ui.dragging {
        let name = app
            .session
            .clip_by_id(id)
            .map(|clip| clip.name.clone())
            .unwrap_or_else(|| "Take".into());
        format!("Release over the timeline to add “{name}”")
    } else {
        "Release to import the dropped files".to_string()
    };
    let banner = container(
        row![dot(theme::ACCENT, 6.0), body(label).color(theme::TEXT)]
            .spacing(10.0)
            .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 10.0,
        bottom: 10.0,
        left: 16.0,
        right: 18.0,
    })
    .style(theme::popover_style);

    container(banner)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::End)
        .align_x(Alignment::Center)
        .padding(Padding {
            top: 0.0,
            bottom: 60.0,
            left: 0.0,
            right: 0.0,
        })
        .into()
}

fn confirm_quit_overlay() -> Element<'static, Message> {
    let scrim = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::scrim_style);

    let dialog = container(
        column![
            text("Quit Simple Audio Cut?")
                .size(size::TITLE)
                .font(theme::strong())
                .color(theme::TEXT),
            body("Recording, processing or unexported changes are still active."),
            gap(8.0),
            row![
                spacer(),
                button(centered(text("Keep editing").size(size::BODY)))
                    .width(116.0)
                    .height(36.0)
                    .padding(0.0)
                    .style(theme::ghost_button)
                    .on_press(Message::CancelQuit),
                button(centered(text("Quit").size(size::BODY).font(theme::medium())))
                    .width(84.0)
                    .height(36.0)
                    .padding(0.0)
                    .style(theme::danger_button)
                    .on_press(Message::ConfirmQuit),
            ]
            .spacing(8.0),
        ]
        .spacing(10.0),
    )
    .padding(28.0)
    .width(Length::Fixed(420.0))
    .style(theme::popover_style);

    stack![scrim, container(dialog).center(Length::Fill)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests;
