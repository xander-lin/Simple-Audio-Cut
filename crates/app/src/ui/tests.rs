//! Unit tests for pure view helpers and canvas interactions.

use super::{
    format_ruler_time, format_time_compact, format_time_precise, format_time_standard, layout_for,
    Layout,
};

#[test]
fn full_screen_uses_columns_and_half_screen_stacks() {
    // Half of a 4K monitor is ~1920px: still wider than any naive
    // breakpoint, but there is no room for three columns.
    assert_eq!(layout_for(1920.0, Some(3840.0)), Layout::Stacked);
    assert_eq!(layout_for(3840.0, Some(3840.0)), Layout::Columns);
    // A 1920 monitor: full screen keeps columns, half stacks.
    assert_eq!(layout_for(1920.0, Some(1920.0)), Layout::Columns);
    assert_eq!(layout_for(960.0, Some(1920.0)), Layout::Stacked);
    // Narrow terminals always stack, even when maximised.
    assert_eq!(layout_for(900.0, Some(1920.0)), Layout::Stacked);
}

#[test]
fn layout_falls_back_to_an_absolute_width_without_monitor_info() {
    assert_eq!(layout_for(1400.0, None), Layout::Columns);
    assert_eq!(layout_for(1100.0, None), Layout::Stacked);
}

#[test]
fn the_scale_strip_ignores_clicks_outside_its_own_bounds() {
    use super::ScaleHandle;
    use crate::message::Message;
    use crate::theme;
    use iced::widget::canvas;
    use iced::{mouse, Point, Rectangle};

    let strip = ScaleHandle {
        track_id: 7,
        color: theme::GREEN,
    };
    let bounds = Rectangle::new(Point::new(0.0, 0.0), iced::Size::new(10.0, 120.0));
    let pressed = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));

    // iced delivers every canvas event to the program; a click elsewhere in
    // the window must not select this track. This is what made playback
    // always use the last track.
    let outside = mouse::Cursor::Available(Point::new(600.0, 400.0));
    let action = canvas::Program::<Message>::update(&strip, &mut (), &pressed, bounds, outside);
    assert!(action.is_none(), "a click outside must be ignored");

    let inside = mouse::Cursor::Available(Point::new(4.0, 40.0));
    let action = canvas::Program::<Message>::update(&strip, &mut (), &pressed, bounds, inside);
    let action = action.expect("a click inside must select the track");
    assert!(
        matches!(action.into_inner().0, Some(Message::SelectTrack(7))),
        "a click inside must select this track"
    );
}

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

#[test]
fn formats_ruler_labels_according_to_the_tick_interval() {
    assert_eq!(format_ruler_time(0.0, 0.5), "0:00.0");
    assert_eq!(format_ruler_time(61.25, 0.25), "1:01.2");
    assert_eq!(format_ruler_time(61.25, 5.0), "1:01");
    assert_eq!(format_ruler_time(3_665.0, 30.0), "1:01:05");
}
