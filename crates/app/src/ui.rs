//! View layer: pure functions from application state to iced elements.

use iced::widget::canvas;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, space, stack, text, text_input,
    Space,
};
use iced::{Alignment, Element, Font, Length, Padding, Size};

use simple_audio_cut_core::denoise::DenoiseCheckState;
use simple_audio_cut_core::region::Region;

use crate::message::Message;
use crate::model::{
    ClipStatus, ContextMenu, ContextMenuKind, DenoiseStatus, MediaClip, MediaId, Track,
};
use crate::update::App;
use crate::waveform::{canvas_height, TrackCanvas};
use crate::theme;

const LABEL_WIDTH: f32 = 176.0;
const HANDLE_WIDTH: f32 = 10.0;
/// Horizontal chrome around a waveform row: handle, label, card padding,
/// scrollbar and the timeline panel margins.
const ROW_CHROME: f32 = 60.0;

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

fn mono() -> Font {
    Font::MONOSPACE
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
    container(text(label.to_string()).size(10.0).color(theme::TEXT_SOFT))
        .padding(Padding {
            top: 1.0,
            bottom: 1.0,
            left: 5.0,
            right: 5.0,
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

pub fn view(app: &App) -> Element<'_, Message> {
    let base = column![titlebar(app), media_pool(app), timeline(app)]
        .width(Length::Fill)
        .height(Length::Fill);

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
    stack(layers).width(Length::Fill).height(Length::Fill).into()
}

fn titlebar(app: &App) -> Element<'_, Message> {
    let recording = app.session.recording.is_some();
    let busy = app.ui.recording_busy;
    let record_label = if busy {
        "Working…".into()
    } else if recording {
        format!("Stop  {}", format_time_standard(app.session.recording_seconds))
    } else {
        "● Record".into()
    };
    let record_button = button(text(record_label).size(12.0))
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
        .width(56.0)
        .size(12.0)
        .font(mono())
        .style(theme::input_style);

    let delete_track = button(text("Delete track").size(11.0))
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 10.0,
            right: 10.0,
        })
        .style(theme::danger_button)
        .on_press_maybe(
            app.session
                .selected_track
                .map(Message::RemoveTrack),
        );

    let changed = app.session.changed_tracks().len();
    let pending = app.session.pending_imports() + app.session.pending_denoise();
    let session_summary = if changed > 0 || pending > 0 {
        let mut parts = Vec::new();
        if changed > 0 {
            parts.push(format!("{changed} unexported"));
        }
        if pending > 0 {
            parts.push(format!("{pending} processing"));
        }
        parts.join(" · ")
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
        .map(|status| (status.provider_name.clone(), status.ready, status.summary.clone()))
        .unwrap_or_else(|| ("Denoising".into(), false, "Open denoising settings".into()));
    let provider_ready = provider.1;
    let provider_button = button(
        row![
            container(Space::new())
                .width(7.0)
                .height(7.0)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(if provider_ready {
                        theme::GREEN
                    } else {
                        theme::RED
                    })),
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..iced::Border::default()
                    },
                    ..container::Style::default()
                }),
            text(provider.0.clone()).size(12.0),
            text("⚙").size(12.0).color(theme::MUTED),
        ]
        .spacing(6.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 8.0,
        bottom: 8.0,
        left: 12.0,
        right: 12.0,
    })
    .style(theme::flat_button)
    .on_press(Message::OpenSettings);

    container(
        row![
            column![
                text("Simple Audio Cut").size(16.0),
                text("Local spoken-word editor").size(11.0).color(theme::MUTED),
            ]
            .spacing(1.0)
            .width(Length::Fixed(230.0)),
            row![record_button, import_button]
                .spacing(7.0)
                .align_y(Alignment::Center),
            row![
                column![
                    text("Normalization").size(11.0).color(theme::TEXT_SOFT),
                    text("LUFS target").size(9.0).color(theme::MUTED),
                ]
                .spacing(0.0),
                lufs,
            ]
            .spacing(8.0)
            .align_y(Alignment::Center),
            delete_track,
            space(),
            row![
                text(session_summary).size(11.0).color(summary_color),
                provider_button
            ]
            .spacing(14.0)
            .align_y(Alignment::Center),
        ]
        .spacing(14.0)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(60.0)
    .padding(Padding {
        top: 0.0,
        bottom: 0.0,
        left: 16.0,
        right: 10.0,
    })
    .style(theme::titlebar_style)
    .into()
}

fn media_pool(app: &App) -> Element<'_, Message> {
    let cards: Vec<Element<'_, Message>> = app
        .session
        .media
        .iter()
        .map(|clip| media_card(app, clip))
        .collect();
    let content: Element<'_, Message> = if cards.is_empty() {
        container(
            row![
                text("No source media").size(12.0).color(theme::TEXT_SOFT),
                text("Record or import audio to begin")
                    .size(11.0)
                    .color(theme::MUTED),
            ]
            .spacing(8.0)
            .align_y(Alignment::Center),
        )
        .padding(14.0)
        .into()
    } else {
        scrollable(row(cards).spacing(8.0).padding(8.0))
            .direction(Direction::Horizontal(Scrollbar::new().width(2.0).scroller_width(2.0)))
            .width(Length::Fill)
            .height(Length::Shrink)
            .style(theme::scrollable_style)
            .into()
    };
    container(
        column![
            row![
                section_kicker("Media pool"),
                text(format!("{} source item(s)", app.session.media.len()))
                    .size(10.0)
                    .color(theme::MUTED)
            ]
            .spacing(10.0)
            .align_y(Alignment::Center)
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
    .style(theme::panel_style)
    .into()
}

fn media_card<'a>(app: &'a App, clip: &'a MediaClip) -> Element<'a, Message> {
    let ready = clip.ready();
    let selected = app.session.selected_media == Some(clip.id);
    let status_color = clip_color(clip);
    let duration = if ready {
        format_time_standard(clip.duration)
    } else {
        "--:--".into()
    };
    let lufs = clip
        .lufs
        .map(|value| format!("{value:.1} LUFS"))
        .unwrap_or_else(|| "-- LUFS".into());

    let name = text_input(clip.name.as_str(), clip.name.as_str())
        .on_input(move |value| Message::RenameMedia(clip.id, value))
        .size(12.0)
        .padding(4.0)
        .width(Length::Fixed(168.0))
        .style(theme::input_style);

    let add = button(text("+").size(14.0))
        .padding(Padding {
            top: 2.0,
            bottom: 2.0,
            left: 9.0,
            right: 9.0,
        })
        .style(theme::tool_button)
        .on_press_maybe(ready.then_some(Message::AddToTimeline(clip.id)));

    let remove = button(text("–").size(12.0))
        .padding(Padding {
            top: 2.0,
            bottom: 2.0,
            left: 9.0,
            right: 9.0,
        })
        .style(theme::danger_button)
        .on_press(Message::DeleteMedia(clip.id));

    let retry: Option<Element<'_, Message>> = clip
        .source_path
        .as_ref()
        .filter(|_| matches!(clip.status, ClipStatus::Failed(_)))
        .map(|_| {
            button(text("Retry").size(10.0))
                .padding(3.0)
                .style(theme::tool_button)
                .on_press(Message::RetryImport(clip.id))
                .into()
        });

    let mut footer = row![add, remove].spacing(4.0).align_y(Alignment::Center);
    if let Some(retry) = retry {
        footer = footer.push(retry);
    }

    let mut body = column![
        row![name, footer].spacing(6.0).align_y(Alignment::Center),
        row![
            text(duration).size(10.0).color(theme::MUTED).font(mono()),
            text(lufs).size(10.0).color(theme::MUTED).font(mono()),
            space(),
            text(clip.status_label().to_string())
                .size(10.0)
                .color(status_color),
        ]
        .spacing(10.0)
        .align_y(Alignment::Center),
    ]
    .spacing(5.0);

    if let Some(error) = clip.error() {
        body = body.push(text(error.to_string()).size(10.0).color(theme::RED));
    }

    container(body)
        .padding(9.0)
        .width(Length::Fixed(248.0))
        .style(move |theme| {
            if selected {
                theme::selected_card(theme)
            } else if matches!(clip.status, ClipStatus::Failed(_)) {
                theme::error_card(theme)
            } else {
                theme::raised_card(theme)
            }
        })
        .into()
}

fn timeline(app: &App) -> Element<'_, Message> {
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

    let play = button(text(if playing { "⏸" } else { "▶" }).size(15.0))
        .padding(Padding {
            top: 5.0,
            bottom: 5.0,
            left: 13.0,
            right: 13.0,
        })
        .style(theme::tool_button)
        .on_press_maybe(selected.is_some().then_some(Message::PlayPause));

    let export = button(
        row![
            text(if app.session.exporting.is_empty() {
                "Export changed".into()
            } else {
                "Exporting…".to_string()
            })
            .size(11.0),
            if changed > 0 {
                text(changed.to_string()).size(10.0).color(theme::ACCENT)
            } else {
                text("").size(10.0)
            },
        ]
        .spacing(6.0)
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

    let header = container(
        row![
            column![
                section_kicker("Timeline"),
                text(title).size(17.0),
            ]
            .spacing(1.0)
            .width(Length::Fill),
            row![
                play,
                column![
                    text(format_time_precise(app.session.playhead)).size(13.0).font(mono()),
                    text(format!("kept / {}", format_time_precise(duration)))
                        .size(10.0)
                        .font(mono())
                        .color(theme::MUTED),
                ]
                .spacing(0.0),
            ]
            .spacing(10.0)
            .align_y(Alignment::Center),
            column![
                text(format!(
                    "{} track(s)",
                    app.session.tracks.len()
                ))
                .size(10.0)
                .color(theme::MUTED),
                text(format!("{changed} changed"))
                    .size(10.0)
                    .color(if changed > 0 { theme::ACCENT } else { theme::MUTED }),
            ]
            .spacing(2.0)
            .align_x(Alignment::End),
            export,
        ]
        .spacing(16.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 10.0,
        bottom: 10.0,
        left: 14.0,
        right: 14.0,
    });

    let inspector = inspector(app);

    let body: Element<'_, Message> = if app.session.tracks.is_empty() {
        container(
            column![
                text("Build your timeline").size(20.0),
                text(
                    "Add a ready item from the media pool. Each take stays independent and exports as its own WAV file."
                )
                .size(12.0)
                .color(theme::MUTED),
                row![
                    keycap("Drag"), text("Select range").size(10.0).color(theme::MUTED),
                    keycap("Right-drag →"), text("Delete").size(10.0).color(theme::MUTED),
                    keycap("Space"), text("Play").size(10.0).color(theme::MUTED),
                ]
                .spacing(10.0),
            ]
            .spacing(12.0)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    } else {
        let rows: Vec<Element<'_, Message>> = app
            .session
            .tracks
            .iter()
            .map(|track| track_row(app, track))
            .collect();
        scrollable(column(rows).spacing(6.0).padding(Padding {
            top: 4.0,
            bottom: 10.0,
            left: 10.0,
            right: 10.0,
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::scrollable_style)
        .into()
    };

    container(
        column![header, inspector, body, status_bar(app)]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::panel_style)
    .into()
}

fn inspector(app: &App) -> Element<'_, Message> {
    let Some(track) = app.session.selected_track() else {
        return container(
            text("Select a track to edit silence, loudness and export state.")
                .size(10.0)
                .color(theme::MUTED),
        )
        .padding(8.0)
        .into();
    };

    let threshold = text_input("", &app.ui.silence_threshold_input)
        .on_input(Message::SilenceThresholdInput)
        .on_submit(Message::SilenceSettingsSubmit)
        .width(56.0)
        .size(11.0)
        .font(mono())
        .style(theme::input_style);
    let minimum = text_input("", &app.ui.min_silence_input)
        .on_input(Message::MinSilenceInput)
        .on_submit(Message::SilenceSettingsSubmit)
        .width(62.0)
        .size(11.0)
        .font(mono())
        .style(theme::input_style);
    let detect = button(
        text(if track.silence_detection {
            "Silence cuts on"
        } else {
            "Detect silence"
        })
        .size(10.0),
    )
    .padding(5.0)
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
                .size(10.0),
            )
            .padding(5.0)
            .style(if muted {
                theme::primary_button
            } else {
                theme::tool_button
            })
            .on_press(Message::MuteRange)
            .into()
        }
        _ => text("").size(10.0).into(),
    };

    container(
        row![
            text("Silence").size(10.0).color(theme::MUTED),
            text("Threshold").size(10.0).color(theme::TEXT_SOFT),
            threshold,
            text("dBFS").size(9.0).color(theme::MUTED),
            text("Minimum").size(10.0).color(theme::TEXT_SOFT),
            minimum,
            text("ms").size(9.0).color(theme::MUTED),
            detect,
            selection_action,
            space(),
            text(if track.collapsed {
                "Collapsed view · right-drag restore is disabled"
            } else if app.ui.blade_mode {
                "Blade active · click the waveform to cut"
            } else {
                "Right-drag right: delete · left: restore · click the yellow curve to add a point · right-click a point to remove"
            })
            .size(9.0)
            .color(theme::MUTED),
            button(text("•••").size(11.0))
                .padding(4.0)
                .style(theme::flat_button)
                .on_press(Message::ContextMenu {
                    track: track.id(),
                    kind: ContextMenuKind::Actions,
                    x: (app.window_size.width - 220.0).max(4.0),
                    y: 140.0,
                }),
        ]
        .spacing(7.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 6.0,
        bottom: 6.0,
        left: 14.0,
        right: 14.0,
    })
    .width(Length::Fill)
    .into()
}

fn track_row<'a>(app: &'a App, track: &'a Track) -> Element<'a, Message> {
    let id = track.id();
    let Some(cache) = app.session.cache(id) else {
        return container(text("Waveform cache unavailable").size(11.0).color(theme::MUTED))
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
    let pps = track.pixels_per_second;
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
        Some(original_display_time(app.session.playhead, &kept, collapsed))
    } else {
        None
    };
    let revision = track.revision;
    let threshold = track.silence_threshold_db;
    let blade = app.ui.blade_mode && !collapsed;
    let show_envelope = selected && !collapsed;
    let show_threshold = selected;

    let row_width = canvas_width(app.window_size.width);
    let canvas_element = {
        let width = row_width;
        let height = canvas_height(width, duration, pps);
        eprintln!("DEBUG track {id} window {:?} width {width} height {height} rows {:?}", app.window_size, crate::waveform::layout_rows(width, duration, pps).len());
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
        canvas(program).width(width).height(height)
    };

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
            text(track.clip.name.clone()).size(12.0),
            text(track.clip.status_label().to_string())
                .size(10.0)
                .color(denoise_color(track.clip.denoise.status)),
            row![
                text(if collapsed { "Collapsed" } else { "Full timeline" })
                    .size(9.0)
                    .color(theme::MUTED),
                text(format!("{:.2}×", pps / 48.0))
                    .size(9.0)
                    .color(theme::MUTED)
                    .font(mono()),
                text(export_label).size(9.0).color(export_color),
            ]
            .spacing(8.0),
        ]
        .spacing(3.0),
    )
    .width(Length::Fixed(LABEL_WIDTH))
    .padding(Padding {
        top: 8.0,
        bottom: 8.0,
        left: 8.0,
        right: 8.0,
    });

    container(
        row![handle, label, canvas_element]
            .spacing(2.0)
            .align_y(Alignment::Start),
    )
    .padding(2.0)
    .width(Length::Fill)
    .style(move |_theme| {
        if selected {
            container::Style {
                background: Some(iced::Background::Color(theme::CYAN_SOFT)),
                border: iced::Border {
                    color: theme::CYAN,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..container::Style::default()
            }
        } else {
            container::Style {
                background: Some(iced::Background::Color(theme::PANEL_SOFT)),
                border: iced::Border {
                    color: theme::LINE,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..container::Style::default()
            }
        }
    })
    .into()
}

fn original_display_time(time: f64, kept: &[Region], collapsed: bool) -> f64 {
    if collapsed {
        simple_audio_cut_core::timeline::edited_offset_at_original_time(time, kept)
    } else {
        time
    }
}

fn status_bar(app: &App) -> Element<'_, Message> {
    let pending = app.session.pending_imports() + app.session.pending_denoise();
    container(
        row![
            text(app.ui.message.clone())
                .size(10.0)
                .color(theme::TEXT_SOFT)
                .width(Length::Fill),
            if pending > 0 {
                text(format!("{pending} processing")).size(10.0).color(theme::ACCENT)
            } else {
                text("").size(10.0)
            },
            row![
                keycap("A"),
                text("Direct").size(9.0).color(theme::MUTED),
                keycap("B"),
                text("Blade").size(9.0).color(theme::MUTED),
                keycap("Space"),
                text("Play").size(9.0).color(theme::MUTED),
                keycap("M"),
                text("Mute range").size(9.0).color(theme::MUTED),
            ]
            .spacing(5.0)
            .align_y(Alignment::Center),
        ]
        .spacing(12.0)
        .align_y(Alignment::Center),
    )
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
                let position = cursor.position_in(bounds).unwrap_or_default();
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
            iced::Color::from_rgba(0.10, 0.12, 0.11, 0.9),
        );
        frame.fill_rectangle(
            iced::Point::new(2.0, 0.0),
            Size::new((bounds.width - 4.0).max(2.0), bounds.height),
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

fn context_menu_overlay<'a>(app: &'a App, menu: &'a ContextMenu) -> Element<'a, Message> {
    let width = 210.0_f32;
    let height = 96.0_f32;
    let x = menu.x.min((app.window_size.width - width - 6.0).max(4.0)).max(4.0);
    let y = menu.y.min((app.window_size.height - height - 6.0).max(4.0)).max(4.0);

    let mut items: Vec<Element<'_, Message>> = Vec::new();
    if menu.kind == ContextMenuKind::Export {
        let can_export = app
            .session
            .track(menu.track_id)
            .is_some_and(|track| track.clip.ready());
        items.push(
            button(text("Force export track").size(11.0))
                .width(Length::Fill)
                .padding(6.0)
                .style(theme::flat_button)
                .on_press_maybe(can_export.then_some(Message::ExportTrack(menu.track_id)))
                .into(),
        );
    } else if app.session.selected_track == Some(menu.track_id) {
        let deleted = app
            .session
            .track(menu.track_id)
            .map(|track| track.deleted().len())
            .unwrap_or(0);
        let collapsed = app
            .session
            .track(menu.track_id)
            .is_some_and(|track| track.collapsed);
        items.push(
            button(
                text(if collapsed {
                    "Show all edits"
                } else {
                    "Collapse deleted audio"
                })
                .size(11.0),
            )
            .width(Length::Fill)
            .padding(6.0)
            .style(theme::flat_button)
            .on_press_maybe(
                (deleted > 0).then_some(Message::ToggleCollapse(menu.track_id)),
            )
            .into(),
        );
        items.push(
            button(text("Move to media pool").size(11.0))
                .width(Length::Fill)
                .padding(6.0)
                .style(theme::flat_button)
                .on_press(Message::ReturnTrackToPool(menu.track_id))
                .into(),
        );
    }

    let panel = container(column(items).spacing(2.0))
        .padding(4.0)
        .width(width)
        .style(theme::menu_style);

    let dismiss = iced::widget::mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::scrim_style),
    )
    .on_press(Message::CloseContextMenu);

    let positioned = column![
        Space::new().height(y),
        row![Space::new().width(x), panel],
    ];

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
    .on_press(if busy {
        Message::Noop
    } else {
        Message::CloseSettings
    });

    let header = container(
        row![
            column![
                section_kicker("Processing provider"),
                text("Denoising").size(20.0),
            ]
            .spacing(2.0)
            .width(Length::Fill),
            button(text("×").size(18.0))
                .padding(4.0)
                .style(theme::flat_button)
                .on_press_maybe((!busy).then_some(Message::CloseSettings)),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 12.0,
        bottom: 12.0,
        left: 16.0,
        right: 16.0,
    });

    let mut body = column![].spacing(14.0);

    if let Some(status) = &status {
        let tone = if status.ready { theme::GREEN } else { theme::RED };
        body = body.push(
            container(
                column![
                    row![
                        text(if status.ready { "✓" } else { "!" })
                            .size(14.0)
                            .color(tone),
                        text(status.provider_name.clone()).size(13.0),
                        text(if status.ready { "Ready" } else { "Needs setup" })
                            .size(10.0)
                            .color(tone),
                    ]
                    .spacing(8.0)
                    .align_y(Alignment::Center),
                    text(status.summary.clone()).size(11.0).color(theme::MUTED),
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
                .label("Automatic denoising (new media stays usable while this runs)")
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
        let model_summary = if models.is_empty() {
            "No models discovered yet".to_string()
        } else {
            format!(
                "{} of {} discovered models are complete",
                ready_models.len(),
                models.len()
            )
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
                        text("44.1 / 48 kHz audio").size(10.0).color(theme::TEXT_SOFT),
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
                    pick_list(
                        device_choices,
                        device_selected,
                        |choice: Choice| Message::SettingsDevice(choice.value),
                    )
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
                            text("Launch overrides are active").size(11.0).color(theme::ACCENT),
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
                        text(if matches!(
                            check.state,
                            DenoiseCheckState::Ready
                        ) {
                            "✓"
                        } else {
                            "!"
                        })
                        .size(11.0)
                        .color(tone),
                        column![
                            text(check.label.clone()).size(11.0),
                            text(check.detail.clone()).size(10.0).color(theme::MUTED),
                        ]
                        .spacing(1.0),
                    ]
                    .spacing(8.0),
                );
            }
            body = body.push(
                column![
                    field_label("Validation", "Checks run locally and do not download or upload anything."),
                    checks
                ]
                .spacing(8.0),
            );
        }
    } else if busy {
        body = body.push(text("Inspecting the local environment…").size(11.0).color(theme::MUTED));
    }

    if let Some(error) = &app.ui.settings.error {
        body = body.push(
            container(text(error.clone()).size(11.0).color(theme::RED))
                .padding(9.0)
                .width(Length::Fill)
                .style(theme::error_card),
        );
    }

    let footer = container(
        row![
            space(),
            button(text(if busy { "Checking…" } else { "Check setup" }).size(11.0))
                .padding(7.0)
                .style(theme::tool_button)
                .on_press_maybe(
                    (!busy && draft.is_some()).then_some(Message::SettingsCheck)
                ),
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
            scrollable(body.padding(Padding {
                top: 0.0,
                bottom: 10.0,
                left: 16.0,
                right: 16.0,
            }))
            .height(Length::Fill)
            .style(theme::scrollable_style),
            footer,
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(500.0))
    .height(Length::Fill)
    .style(theme::drawer_style);

    stack![
        dismiss,
        row![space(), drawer]
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

fn confirm_quit_overlay(app: &App) -> Element<'_, Message> {
    let _ = app;
    let scrim = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::scrim_style);

    let dialog = container(
        column![
            text("Quit Simple Audio Cut?").size(18.0),
            text("Recording, processing, or unexported changes are still active. Quit anyway?")
                .size(11.0)
                .color(theme::MUTED),
            row![
                space(),
                button(text("Keep editing").size(11.0))
                    .padding(5.0)
                    .style(theme::tool_button)
                    .on_press(Message::CancelQuit),
                button(text("Quit anyway").size(11.0))
                    .padding(5.0)
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
mod tests {
    use super::{format_time_compact, format_time_precise, format_time_standard};

    #[test]
    fn formats_standard_timecodes() {
        assert_eq!(format_time_standard(0.0), "0:00");
        assert_eq!(format_time_standard(65.4), "1:05");
        assert_eq!(format_time_standard(3_665.0), "1:01:05");
    }

    #[test]
    fn formats_compact_timecodes() {
        assert_eq!(format_time_compact(0.0), "0s");
        assert_eq!(format_time_compact(125.0), "2m5s");
    }

    #[test]
    fn formats_precise_playhead_timecodes() {
        assert_eq!(format_time_precise(0.0), "0:00.00");
        assert_eq!(format_time_precise(61.25), "1:01.25");
        assert_eq!(format_time_precise(f64::NAN), "0:00.00");
    }

}
