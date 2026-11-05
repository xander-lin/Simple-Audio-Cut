//! Unit tests for waveform geometry, RMS bins and canvas gestures.

use super::{
    canvas_height, deleted_at_time, layout_rows, rms_columns, ruler_step, PreviewOperation,
    TrackCanvas, WaveformCache, ROW_GAP, ROW_HEIGHT,
};
use crate::message::{Message, WaveformEvent};
use crate::model::MediaId;
use iced::widget::canvas;
use iced::{mouse, Point, Rectangle};
use simple_audio_cut_core::analysis::RmsFrame;
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::region::Region;
use std::sync::Arc;

/// Four seconds at 100 px/s: one row whose left/right edge is 30px.
const BOUNDS: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 600.0,
    height: ROW_HEIGHT,
};

fn test_canvas<'a>(cache: &'a WaveformCache) -> TrackCanvas<'a> {
    TrackCanvas::new(
        cache,
        7 as MediaId,
        Arc::new(AudioBuffer::new(1_000, vec![vec![0.0; 4_000]])),
        Arc::new(Vec::<RmsFrame>::new()),
        4.0,
        100.0,
    )
}

fn cursor(x: f32, y: f32) -> mouse::Cursor {
    mouse::Cursor::Available(Point::new(x, y))
}

fn left_press() -> canvas::Event {
    canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
}

fn right_press() -> canvas::Event {
    canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
}

fn release(button: mouse::Button) -> canvas::Event {
    canvas::Event::Mouse(mouse::Event::ButtonReleased(button))
}

fn moved() -> canvas::Event {
    canvas::Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(0.0, 0.0),
    })
}

fn published(action: Option<canvas::Action<Message>>) -> Option<Message> {
    action.and_then(|action| action.into_inner().0)
}

fn drag(
    canvas: &TrackCanvas<'_>,
    state: &mut super::CanvasState,
    button: mouse::Button,
    from: f32,
    to: f32,
) -> Option<Message> {
    let press = if button == mouse::Button::Left {
        left_press()
    } else {
        right_press()
    };
    let _ = canvas::Program::<Message>::update(canvas, state, &press, BOUNDS, cursor(from, 80.0));
    let _ = canvas::Program::<Message>::update(canvas, state, &moved(), BOUNDS, cursor(to, 80.0));
    published(canvas::Program::<Message>::update(
        canvas,
        state,
        &release(button),
        BOUNDS,
        cursor(to, 80.0),
    ))
}

fn message_event(message: Option<Message>) -> Option<WaveformEvent> {
    match message {
        Some(Message::Waveform { event, .. }) => Some(event),
        _ => None,
    }
}

#[test]
fn wraps_long_audio_into_multiple_rows() {
    let rows = layout_rows(500.0, 30.0, 48.0);
    assert!(rows.len() >= 3);
    assert_eq!(rows[0].start, 0.0);
    assert!((rows.last().unwrap().end - 30.0).abs() < 1e-6);
    for pair in rows.windows(2) {
        assert!((pair[0].end - pair[1].start).abs() < 1e-6);
    }
}

#[test]
fn short_audio_keeps_edge_padding_on_both_sides() {
    let rows = layout_rows(800.0, 1.0, 48.0);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].leading > 0.0);
    assert!(rows[0].trailing > 0.0);
    assert!(rows[0].width < 800.0);
}

#[test]
fn canvas_height_matches_the_rows_it_returns() {
    assert_eq!(canvas_height(400.0, 0.0, 48.0), ROW_HEIGHT);
    let rows = layout_rows(400.0, 200.0, 48.0).len();
    assert_eq!(
        canvas_height(400.0, 200.0, 48.0),
        rows as f32 * ROW_HEIGHT + (rows.saturating_sub(1)) as f32 * ROW_GAP
    );
}

#[test]
fn rms_columns_keep_the_quietest_and_loudest_frame_per_pixel() {
    let rms = vec![
        RmsFrame {
            time: 0.05,
            amplitude: 0.2,
        },
        RmsFrame {
            time: 0.08,
            amplitude: 0.8,
        },
        RmsFrame {
            time: 0.15,
            amplitude: 0.4,
        },
    ];
    let columns = rms_columns(&rms, 0.0, 0.3, 3);

    assert_eq!(columns.len(), 3);
    // The first column holds both low and high frames.
    assert!((columns[0].min - 0.2).abs() < 1e-9);
    assert!((columns[0].max - 0.8).abs() < 1e-9);
    assert!((columns[1].min - 0.4).abs() < 1e-9);
    assert!((columns[1].max - 0.4).abs() < 1e-9);
    // Columns with no frames reuse the previous amplitude.
    assert!((columns[2].min - 0.4).abs() < 1e-9);
    assert!((columns[2].max - 0.4).abs() < 1e-9);
}

#[test]
fn rms_columns_are_empty_without_analysis_or_duration() {
    assert!(rms_columns(&[], 0.0, 1.0, 35).is_empty());
    assert!(rms_columns(
        &[RmsFrame {
            time: 0.0,
            amplitude: 0.1
        }],
        1.0,
        1.0,
        10
    )
    .is_empty());
    assert!(rms_columns(
        &[RmsFrame {
            time: 0.0,
            amplitude: 0.1
        }],
        0.0,
        1.0,
        0
    )
    .is_empty());
}

#[test]
fn deleted_regions_use_a_half_open_right_edge() {
    let regions = vec![Region::new(1.0, 2.0)];
    assert_eq!(deleted_at_time(&regions, 1.0), Some(Region::new(1.0, 2.0)));
    assert_eq!(deleted_at_time(&regions, 1.5), Some(Region::new(1.0, 2.0)));
    assert_eq!(deleted_at_time(&regions, 2.0), None);
}

#[test]
fn ruler_step_keeps_labels_readable_at_every_zoom_level() {
    for pps in [0.25_f32, 4.0, 12.0, 48.0, 120.0, 400.0] {
        let step = ruler_step(pps) as f32;
        assert!(
            step * pps >= 64.0,
            "labels would collide at {pps} px/s (step {step})"
        );
    }
    // Zooming in must never make the interval coarser.
    assert!(ruler_step(400.0) <= ruler_step(48.0));
    assert!(ruler_step(48.0) <= ruler_step(1.0));
}

#[test]
fn left_drag_mutes_the_range_instead_of_deleting_it() {
    let cache = WaveformCache::new();
    let canvas = test_canvas(&cache);
    let mut state = super::CanvasState::default();

    // x=130 is t=1.0s, x=330 is t=3.0s in the single row.
    let message = drag(&canvas, &mut state, mouse::Button::Left, 130.0, 330.0);

    match message_event(message) {
        Some(WaveformEvent::MuteRange(region)) => {
            assert!((region.start - 1.0).abs() < 1e-6, "{region:?}");
            assert!((region.end - 3.0).abs() < 1e-6, "{region:?}");
        }
        other => panic!("left-drag must publish a mute, got {other:?}"),
    }
}

#[test]
fn left_drag_previews_amber_mute_and_green_restore() {
    let cache = WaveformCache::new();
    let canvas = test_canvas(&cache);
    let mut state = super::CanvasState::default();

    let _ = canvas::Program::<Message>::update(
        &canvas,
        &mut state,
        &left_press(),
        BOUNDS,
        cursor(130.0, 80.0),
    );
    let _ = canvas::Program::<Message>::update(
        &canvas,
        &mut state,
        &moved(),
        BOUNDS,
        cursor(330.0, 80.0),
    );
    let drag = state.drag.as_ref().expect("drag started");
    let preview = drag.preview(&[]).expect("preview");
    assert_eq!(preview.operation, PreviewOperation::Mute);
    let preview = drag
        .preview(&[Region::new(1.0, 3.0)])
        .expect("preview over muted audio");
    assert_eq!(preview.operation, PreviewOperation::Restore);
}

#[test]
fn right_drag_still_deletes_forward_and_restores_backward() {
    let cache = WaveformCache::new();
    let canvas = test_canvas(&cache);
    let mut state = super::CanvasState::default();

    let delete = message_event(drag(
        &canvas,
        &mut state,
        mouse::Button::Right,
        130.0,
        330.0,
    ));
    match delete {
        Some(WaveformEvent::DirectEdit { region, operation }) => {
            assert_eq!(region, Region::new(1.0, 3.0));
            assert_eq!(
                operation,
                simple_audio_cut_core::direct_edit::DirectRegionOperation::Delete
            );
        }
        other => panic!("right-drag forward must delete, got {other:?}"),
    }

    let restore = message_event(drag(
        &canvas,
        &mut state,
        mouse::Button::Right,
        330.0,
        130.0,
    ));
    match restore {
        Some(WaveformEvent::DirectEdit { region, operation }) => {
            assert_eq!(region, Region::new(1.0, 3.0));
            assert_eq!(
                operation,
                simple_audio_cut_core::direct_edit::DirectRegionOperation::Restore
            );
        }
        other => panic!("right-drag backward must restore, got {other:?}"),
    }
}

#[test]
fn a_short_left_press_seeks_and_a_ruler_click_never_selects() {
    let cache = WaveformCache::new();
    let canvas = test_canvas(&cache);
    let mut state = super::CanvasState::default();

    let seek = message_event(drag(&canvas, &mut state, mouse::Button::Left, 130.0, 131.0));
    assert_eq!(seek, Some(WaveformEvent::Seek(1.0)));

    let message = published(canvas::Program::<Message>::update(
        &canvas,
        &mut state,
        &left_press(),
        BOUNDS,
        cursor(230.0, 5.0),
    ));
    assert!(matches!(
        message_event(message),
        Some(WaveformEvent::Seek(time)) if (time - 2.0).abs() < 1e-6
    ));
    // The ruler must not leave a range drag behind.
    assert!(state.drag.is_none());
}

#[test]
fn events_outside_the_canvas_never_start_a_gesture() {
    let cache = WaveformCache::new();
    let canvas = test_canvas(&cache);
    let mut state = super::CanvasState::default();

    let action = canvas::Program::<Message>::update(
        &canvas,
        &mut state,
        &left_press(),
        BOUNDS,
        cursor(900.0, 80.0),
    );
    assert!(action.is_none(), "a click outside must be ignored");
    assert!(state.drag.is_none());
}
