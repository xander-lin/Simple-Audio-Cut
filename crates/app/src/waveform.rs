//! Waveform canvas: one canvas per timeline track, internally wrapped into
//! rows the same way the original editor wrapped long audio.
//!
//! Each row is split into a 16px time ruler and the waveform body below it.
//! The ruler makes navigation precise without hovering, and gives the playhead
//! a handle to grab visually.

use std::cell::Cell;
use std::sync::Arc;

use iced::mouse;
use iced::widget::canvas::{self, Action, Cache, Event, Frame, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use simple_audio_cut_core::analysis::{amplitude_to_dbfs, RmsFrame};
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::direct_edit::{direct_region_edit, mute_region_edit};
use simple_audio_cut_core::region::{envelope_gain_at_time, EnvelopePoint, Region};

use crate::message::{Message, WaveformEvent};
use crate::model::MediaId;
use crate::theme;
use crate::ui::{format_ruler_time, format_time_precise};

pub const ROW_HEIGHT: f32 = 184.0;
pub const ROW_GAP: f32 = 10.0;
/// Height of the time ruler at the top of every row.
pub const RULER_HEIGHT: f32 = 22.0;
const EDGE_MIN: f32 = 24.0;
const HIT_RADIUS: f32 = 14.0;
const LINE_HIT_RADIUS: f32 = 8.0;
const EDIT_POINT_HIT: f32 = 6.0;
/// Minimum horizontal distance between ruler labels.
const RULER_MIN_LABEL_GAP: f32 = 84.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowLayout {
    pub index: usize,
    pub start: f64,
    pub end: f64,
    pub leading: f32,
    pub trailing: f32,
    pub width: f32,
}

impl RowLayout {
    pub fn timeline_width(&self) -> f32 {
        (self.width - self.leading - self.trailing).max(1.0)
    }

    pub fn height(&self) -> f32 {
        ROW_HEIGHT
    }
}

/// Splits the visible width into wrapped rows for `duration`.
pub fn layout_rows(width: f32, duration: f64, pixels_per_second: f32) -> Vec<RowLayout> {
    let mut rows = Vec::new();
    if width < 8.0 || duration <= 0.0 || !pixels_per_second.is_finite() {
        return rows;
    }
    let pps = pixels_per_second.max(0.25);
    let edge = (pps * 0.3).max(EDGE_MIN).min((width * 0.25).max(EDGE_MIN));
    let mut start = 0.0_f64;
    let mut index = 0usize;
    while start < duration && index < 512 {
        let leading = if index == 0 { edge } else { 0.0 };
        let remaining = duration - start;
        let final_width = leading + remaining as f32 * pps + edge;
        let is_final = final_width <= width;
        let trailing = if is_final { edge } else { 0.0 };
        let audio_width = (width - leading - trailing).max(1.0);
        let row_duration = if is_final {
            remaining
        } else {
            (audio_width / pps) as f64
        };
        let end = (start + row_duration).min(duration);
        if end <= start {
            break;
        }
        let rendered_width = if is_final {
            leading + (end - start) as f32 * pps + trailing
        } else {
            width
        };
        rows.push(RowLayout {
            index,
            start,
            end,
            leading,
            trailing,
            width: rendered_width.min(width).max(1.0),
        });
        start = end;
        index += 1;
    }
    rows
}

pub fn canvas_height(width: f32, duration: f64, pixels_per_second: f32) -> f32 {
    let rows = layout_rows(width, duration, pixels_per_second).len().max(1);
    rows as f32 * ROW_HEIGHT + (rows.saturating_sub(1)) as f32 * ROW_GAP
}

/// Persistent drawing cache owned by the application state, one per track.
pub struct WaveformCache {
    cache: Cache,
    revision: Cell<u64>,
}

impl Default for WaveformCache {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveformCache {
    pub fn new() -> Self {
        Self {
            cache: Cache::new(),
            revision: Cell::new(u64::MAX),
        }
    }
}

/// Everything the waveform needs to draw itself.
pub struct TrackCanvas<'a> {
    pub track_id: MediaId,
    pub audio: Arc<AudioBuffer>,
    pub rms: Arc<Vec<RmsFrame>>,
    pub duration: f64,
    pub pixels_per_second: f32,
    pub deleted: Vec<Region>,
    pub muted: Vec<Region>,
    pub envelope: Vec<EnvelopePoint>,
    pub edit_points: Vec<f64>,
    pub selected_range: Option<Region>,
    pub playhead: Option<f64>,
    pub blade: bool,
    pub show_envelope: bool,
    pub show_threshold: bool,
    pub threshold_db: f64,
    pub revision: u64,
    pub selected: bool,
    cache: &'a WaveformCache,
}

impl std::fmt::Debug for TrackCanvas<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrackCanvas")
            .field("track_id", &self.track_id)
            .field("duration", &self.duration)
            .finish()
    }
}

impl<'a> TrackCanvas<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cache: &'a WaveformCache,
        track_id: MediaId,
        audio: Arc<AudioBuffer>,
        rms: Arc<Vec<RmsFrame>>,
        duration: f64,
        pixels_per_second: f32,
    ) -> Self {
        Self {
            track_id,
            audio,
            rms,
            duration,
            pixels_per_second,
            deleted: Vec::new(),
            muted: Vec::new(),
            envelope: Vec::new(),
            edit_points: Vec::new(),
            selected_range: None,
            playhead: None,
            blade: false,
            show_envelope: false,
            show_threshold: false,
            threshold_db: -36.0,
            revision: 0,
            selected: false,
            cache,
        }
    }

    fn content_height() -> f32 {
        ROW_HEIGHT - RULER_HEIGHT
    }

    fn content_top(top: f32) -> f32 {
        top + RULER_HEIGHT
    }

    fn content_center(top: f32) -> f32 {
        Self::content_top(top) + Self::content_height() / 2.0
    }

    fn x_at_time(row: &RowLayout, time: f64) -> f32 {
        let progress = if row.end > row.start {
            ((time - row.start) / (row.end - row.start)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        row.leading + progress as f32 * row.timeline_width()
    }

    fn time_at_x(row: &RowLayout, x: f32) -> f64 {
        let progress = ((x - row.leading) / row.timeline_width()).clamp(0.0, 1.0);
        row.start + progress as f64 * (row.end - row.start)
    }

    /// Vertical position of `gain` inside the waveform body of a row.
    fn y_at_gain(gain: f64) -> f32 {
        let height = Self::content_height() as f64;
        (height * (1.0 - gain / 2.0)) as f32
    }

    fn gain_at_y(y: f32) -> f64 {
        let height = Self::content_height() as f64;
        (2.0 * (1.0 - y as f64 / height)).clamp(0.0, 2.0)
    }

    fn row_top(index: usize) -> f32 {
        index as f32 * (ROW_HEIGHT + ROW_GAP)
    }

    fn row_at(
        bounds: Rectangle,
        rows: &[RowLayout],
        position: Point,
    ) -> Option<(RowLayout, Point)> {
        for (index, row) in rows.iter().enumerate() {
            let top = Self::row_top(index);
            if position.y >= top && position.y <= top + ROW_HEIGHT {
                let local = Point::new(position.x, position.y - top);
                return Some((*row, local));
            }
        }
        let _ = bounds;
        None
    }

    fn visible_envelope(&self, row: &RowLayout) -> Vec<&EnvelopePoint> {
        self.envelope
            .iter()
            .filter(|point| point.time >= row.start - 0.001 && point.time <= row.end + 0.001)
            .collect()
    }

    /// Cache key for the static layer. Selection and threshold rendering live
    /// in the cached frame, so they must invalidate it too — otherwise
    /// selecting a track would keep the previous track's guidelines.
    fn cache_key(&self) -> u64 {
        let mut key = self.revision;
        key = key.wrapping_mul(31).wrapping_add(self.selected as u64);
        key = key
            .wrapping_mul(31)
            .wrapping_add(self.show_threshold as u64);
        key = key
            .wrapping_mul(31)
            .wrapping_add((self.threshold_db * 100.0) as i64 as u64);
        key
    }

    fn draw_base(&self, frame: &mut Frame, bounds: Rectangle) {
        let rows = layout_rows(bounds.width, self.duration, self.pixels_per_second);
        let background = if self.selected {
            theme::WAVE_SELECTED_BG
        } else {
            theme::WAVE_BG
        };
        for row in rows {
            let top = Self::row_top(row.index);
            let row_rect =
                Rectangle::new(Point::new(0.0, top), Size::new(bounds.width, ROW_HEIGHT));
            frame.fill_rectangle(row_rect.position(), row_rect.size(), background);

            self.draw_ruler(frame, &row, top, bounds.width);

            let center = Self::content_center(top);
            // Center line.
            frame.fill_rectangle(
                Point::new(row.leading, center),
                Size::new(row.timeline_width(), 1.0),
                Color { a: 0.06, ..theme::TEXT },
            );
            self.draw_rms(frame, &row, top);
            self.draw_peaks(frame, &row, top);
            if self.show_threshold {
                let radius = Self::rms_radius(simple_audio_cut_core::analysis::dbfs_to_amplitude(
                    self.threshold_db,
                ));
                let color = Color { a: 0.28, ..theme::ACCENT };
                // The original editor drew the threshold as a dashed guide.
                draw_dashed_rule(
                    frame,
                    row.leading,
                    center - radius,
                    row.timeline_width(),
                    color,
                );
                draw_dashed_rule(
                    frame,
                    row.leading,
                    center + radius,
                    row.timeline_width(),
                    color,
                );
            }
        }
    }

    /// 16px ruler: background, tick marks, faint grid in the body and labels.
    fn draw_ruler(&self, frame: &mut Frame, row: &RowLayout, top: f32, width: f32) {
        frame.fill_rectangle(
            Point::new(0.0, top + RULER_HEIGHT - 1.0),
            Size::new(width, 1.0),
            theme::RULER_BG,
        );
        let step = ruler_step(self.pixels_per_second);
        let first = (row.start / step).ceil() * step;
        let mut time = first;
        let mut guard = 0;
        while time <= row.end && guard < 4096 {
            guard += 1;
            let x = Self::x_at_time(row, time);
            let tick_top = top + RULER_HEIGHT - 6.0;
            frame.fill_rectangle(
                Point::new(x, tick_top),
                Size::new(1.0, 5.0),
                theme::RULER_TICK,
            );
            // Faint grid line through the waveform body, so time is readable
            // without counting ticks.
            frame.fill_rectangle(
                Point::new(x, Self::content_top(top)),
                Size::new(1.0, Self::content_height()),
                theme::GRID,
            );
            if self.ruler_label_fits(row, x) {
                frame.fill_text(Text {
                    content: format_ruler_time(time, step),
                    position: Point::new(x + 5.0, top + 4.0),
                    color: theme::RULER_TEXT,
                    size: 10.0.into(),
                    ..Text::default()
                });
            }
            time += step;
        }
        // Row end label, right-aligned in the ruler.
        frame.fill_text(Text {
            content: format_ruler_time(row.end, step),
            position: Point::new((width - 44.0).max(0.0), top + 4.0),
            color: theme::RULER_TEXT,
            size: 10.0.into(),
            ..Text::default()
        });
    }

    fn ruler_label_fits(&self, row: &RowLayout, x: f32) -> bool {
        let end = x + 34.0;
        end < row.leading + row.timeline_width() + row.trailing - 44.0
    }

    fn rms_radius(amplitude: f64) -> f32 {
        let normalized = ((amplitude_to_dbfs(amplitude, -70.0) + 70.0) / 70.0).clamp(0.0, 1.0);
        (normalized as f32) * (Self::content_height() / 2.0 - 4.0)
    }

    fn draw_rms(&self, frame: &mut Frame, row: &RowLayout, top: f32) {
        let width = row.timeline_width().ceil() as usize;
        if width == 0 || self.rms.is_empty() {
            return;
        }
        let columns = rms_columns(&self.rms, row.start, row.end, width);
        if columns.is_empty() {
            return;
        }
        let center = Self::content_center(top);
        let max_upper: Vec<f32> = columns
            .iter()
            .map(|column| center - Self::rms_radius(column.max))
            .collect();
        let max_lower: Vec<f32> = columns
            .iter()
            .map(|column| center + Self::rms_radius(column.max))
            .collect();
        // The RMS band is a quiet underlay: peaks above it stay the focus.
        let band = Path::new(|builder| {
            builder.move_to(Point::new(row.leading, max_upper[0]));
            for (index, y) in max_upper.iter().enumerate().skip(1) {
                builder.line_to(Point::new(row.leading + index as f32, *y));
            }
            for (index, y) in max_lower.iter().enumerate().rev() {
                builder.line_to(Point::new(row.leading + index as f32, *y));
            }
            builder.close();
        });
        frame.fill(
            &band,
            Color {
                a: if self.selected { 0.14 } else { 0.08 },
                ..theme::RMS_FILL
            },
        );
    }

    fn draw_peaks(&self, frame: &mut Frame, row: &RowLayout, top: f32) {
        let width = row.timeline_width().ceil() as usize;
        if width == 0 {
            return;
        }
        let first = self.audio.frame_at(row.start);
        let last = self.audio.frame_at(row.end).max(first + 1);
        let span = last - first;
        let channels = self.audio.channel_count().max(1);
        let stride = (span / (width * 64)).max(1);
        let center = Self::content_center(top);
        let half = Self::content_height() / 2.0;
        let color = if self.selected {
            Color {
                a: 0.95,
                ..theme::WAVE_FILL
            }
        } else {
            Color {
                a: 0.85,
                ..theme::WAVE_FILL_DIM
            }
        };
        let path = Path::new(|builder| {
            for x in 0..width {
                let bin_start = first + span * x / width;
                let bin_end = (first + span * (x + 1) / width)
                    .max(bin_start + 1)
                    .min(last);
                let mut min = 1.0_f32;
                let mut max = -1.0_f32;
                for channel in 0..channels {
                    let samples = self.audio.channel(channel);
                    let mut sample = bin_start;
                    while sample < bin_end && sample < samples.len() {
                        let value = samples[sample];
                        min = min.min(value);
                        max = max.max(value);
                        sample += stride;
                    }
                }
                if max < min {
                    min = 0.0;
                    max = 0.0;
                }
                builder.move_to(Point::new(row.leading + x as f32, center + min * half));
                builder.line_to(Point::new(row.leading + x as f32, center + max * half));
            }
        });
        frame.stroke(&path, Stroke::default().with_color(color).with_width(1.0));
    }

    fn draw_overlays(&self, frame: &mut Frame, bounds: Rectangle, state: &CanvasState) {
        let rows = layout_rows(bounds.width, self.duration, self.pixels_per_second);
        for row in &rows {
            let top = Self::row_top(row.index);
            let content_top = Self::content_top(top);
            let center = Self::content_center(top);

            for region in &self.deleted {
                let start = region.start.max(row.start);
                let end = region.end.min(row.end);
                if end <= start {
                    continue;
                }
                let left = Self::x_at_time(row, start);
                let width = (Self::x_at_time(row, end) - left).max(1.0);
                frame.fill_rectangle(
                    Point::new(left, content_top),
                    Size::new(width, Self::content_height()),
                    Color { a: 0.82, ..theme::BG },
                );
                draw_hatch(
                    frame,
                    left,
                    content_top,
                    width,
                    Self::content_height(),
                    Color::from_rgba(0.72, 0.78, 0.79, 0.16),
                );
                // A small restore glyph only: full labels on every cut turned
                // the waveform into a wall of text.
                if width > 26.0 {
                    frame.fill_text(Text {
                        content: "↺".into(),
                        position: Point::new(left + 6.0, center - 8.0),
                        color: Color::from_rgba(0.82, 0.88, 0.87, 0.55),
                        size: 12.0.into(),
                        ..Text::default()
                    });
                }
            }

            for region in &self.muted {
                let start = region.start.max(row.start);
                let end = region.end.min(row.end);
                if end <= start {
                    continue;
                }
                let left = Self::x_at_time(row, start);
                let width = (Self::x_at_time(row, end) - left).max(1.0);
                frame.fill_rectangle(
                    Point::new(left, content_top),
                    Size::new(width, Self::content_height()),
                    Color { a: 0.10, ..theme::ACCENT },
                );
                frame.fill_rectangle(
                    Point::new(left, content_top),
                    Size::new(1.0, Self::content_height()),
                    Color { a: 0.45, ..theme::ACCENT },
                );
                frame.fill_rectangle(
                    Point::new(left + width - 1.0, content_top),
                    Size::new(1.0, Self::content_height()),
                    Color { a: 0.45, ..theme::ACCENT },
                );
                if width > 46.0 {
                    frame.fill_text(Text {
                        content: "Muted".into(),
                        position: Point::new(left + 8.0, center - 7.0),
                        color: Color { a: 0.9, ..theme::ACCENT },
                        size: 10.0.into(),
                        ..Text::default()
                    });
                }
            }

            if let Some(selected) = self.selected_range {
                let start = selected.start.max(row.start);
                let end = selected.end.min(row.end);
                if end > start {
                    let left = Self::x_at_time(row, start);
                    let width = (Self::x_at_time(row, end) - left).max(1.0);
                    frame.fill_rectangle(
                        Point::new(left, content_top),
                        Size::new(width, Self::content_height()),
                        Color { a: 0.10, ..theme::SELECTION },
                    );
                    // Solid edges plus a bar under the ruler make the range
                    // readable even when it sits on top of a deleted region.
                    frame.fill_rectangle(
                        Point::new(left, content_top),
                        Size::new(1.5, Self::content_height()),
                        theme::SELECTION,
                    );
                    frame.fill_rectangle(
                        Point::new(left + width - 1.5, content_top),
                        Size::new(1.5, Self::content_height()),
                        theme::SELECTION,
                    );
                    frame.fill_rectangle(
                        Point::new(left, top + RULER_HEIGHT - 2.0),
                        Size::new(width, 2.0),
                        theme::SELECTION,
                    );
                }
            }

            if self.show_envelope || !self.envelope.is_empty() {
                let samples = ((row.timeline_width() / 10.0).ceil() as usize).max(8) + 1;
                let path = Path::new(|builder| {
                    for index in 0..samples {
                        let time = row.start
                            + (index as f64 / (samples - 1) as f64) * (row.end - row.start);
                        let gain = envelope_gain_at_time(&self.envelope, self.duration, time);
                        let point = Point::new(
                            Self::x_at_time(row, time),
                            content_top + Self::y_at_gain(gain),
                        );
                        if index == 0 {
                            builder.move_to(point);
                        } else {
                            builder.line_to(point);
                        }
                    }
                });
                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(Color {
                            a: if self.envelope.is_empty() {
                                0.22
                            } else if self.show_envelope {
                                0.9
                            } else {
                                0.4
                            },
                            ..theme::ACCENT
                        })
                        .with_width(1.6),
                );
                for point in self.visible_envelope(row) {
                    let center = Point::new(
                        Self::x_at_time(row, point.time),
                        content_top + Self::y_at_gain(point.gain),
                    );
                    let circle = Path::circle(center, 4.5);
                    frame.fill(&circle, theme::ACCENT);
                    frame.stroke(
                        &circle,
                        Stroke::default()
                            .with_color(theme::BG)
                            .with_width(1.0),
                    );
                }
            }

            for edit_point in &self.edit_points {
                if *edit_point < row.start || *edit_point > row.end {
                    continue;
                }
                let x = Self::x_at_time(row, *edit_point);
                frame.fill_rectangle(
                    Point::new(x - 0.5, content_top),
                    Size::new(1.0, Self::content_height()),
                    Color { a: 0.55, ..theme::SELECTION },
                );
                let marker = Path::new(|builder| {
                    builder.move_to(Point::new(x - 4.0, top + RULER_HEIGHT - 8.0));
                    builder.line_to(Point::new(x + 4.0, top + RULER_HEIGHT - 8.0));
                    builder.line_to(Point::new(x, top + RULER_HEIGHT));
                    builder.close();
                });
                frame.fill(&marker, Color { a: 0.8, ..theme::SELECTION });
            }

            if let Some(drag) = &state.drag {
                if let Some(preview) = drag.preview() {
                    let start = preview.start.max(row.start);
                    let end = preview.end.min(row.end);
                    if end > start {
                        let left = Self::x_at_time(row, start);
                        let width = (Self::x_at_time(row, end) - left).max(1.0);
                        let color = match preview.operation {
                            PreviewOperation::Mute => Color::from_rgba(0.94, 0.72, 0.30, 0.28),
                            PreviewOperation::Delete => Color::from_rgba(0.88, 0.46, 0.42, 0.30),
                            PreviewOperation::Restore => Color::from_rgba(0.35, 0.66, 0.47, 0.32),
                        };
                        frame.fill_rectangle(
                            Point::new(left, content_top),
                            Size::new(width, Self::content_height()),
                            color,
                        );
                    }
                }
            }

            if let Some(hover) = state.hover_time {
                if hover >= row.start && hover <= row.end {
                    let x = Self::x_at_time(row, hover);
                    frame.fill_rectangle(
                        Point::new(x, content_top),
                        Size::new(1.0, Self::content_height()),
                        Color::from_rgba(0.85, 0.90, 0.92, 0.18),
                    );
                }
            }

            if let Some(playhead) = self.playhead {
                if playhead >= row.start && playhead <= row.end {
                    let x = Self::x_at_time(row, playhead);
                    frame.fill_rectangle(
                        Point::new(x - 1.0, top),
                        Size::new(2.0, ROW_HEIGHT),
                        theme::PLAYHEAD,
                    );
                    let handle = Path::new(|builder| {
                        builder.move_to(Point::new(x - 5.0, top));
                        builder.line_to(Point::new(x + 5.0, top));
                        builder.line_to(Point::new(x, top + 7.0));
                        builder.close();
                    });
                    frame.fill(&handle, theme::PLAYHEAD);
                }
            }
        }

        // Drawn once in canvas coordinates: the hover used to be painted
        // inside every row, which duplicated the label on wrapped tracks.
        if let Some((x, y, gain)) = state.envelope_hover {
            let text_position = Point::new(
                (x + 10.0).min(bounds.width - 70.0).max(4.0),
                (y - 26.0).clamp(4.0, (bounds.height - 20.0).max(4.0)),
            );
            frame.fill_text(Text {
                content: format_gain(gain),
                position: text_position,
                color: theme::ACCENT,
                size: 10.0.into(),
                ..Text::default()
            });
        }
        if let Some(hover) = state.hover_time {
            let rows = layout_rows(bounds.width, self.duration, self.pixels_per_second);
            if let Some(row) = rows
                .iter()
                .find(|row| hover >= row.start && hover <= row.end)
            {
                let x = Self::x_at_time(row, hover);
                let label = format_time_precise(hover);
                let width = (label.len() as f32 * 5.6 + 10.0).max(38.0);
                let left = (x + 6.0).min(bounds.width - width - 4.0).max(4.0);
                frame.fill_rectangle(
                    Point::new(left, bounds.height - 16.0),
                    Size::new(width, 14.0),
                    theme::RAISED,
                );
                frame.fill_text(Text {
                    content: label,
                    position: Point::new(left + 5.0, bounds.height - 15.0),
                    color: theme::TEXT_SOFT,
                    size: 10.0.into(),
                    ..Text::default()
                });
            }
        }
    }

    fn hit_envelope_point(&self, row: &RowLayout, local: Point) -> Option<EnvelopePoint> {
        let y = local.y - RULER_HEIGHT;
        self.visible_envelope(row)
            .into_iter()
            .find(|point| {
                let dx = Self::x_at_time(row, point.time) - local.x;
                let dy = Self::y_at_gain(point.gain) - y;
                (dx * dx + dy * dy).sqrt() <= HIT_RADIUS
            })
            .cloned()
    }

    fn hit_envelope_line(&self, local: Point, time: f64) -> Option<f64> {
        let gain = envelope_gain_at_time(&self.envelope, self.duration, time);
        let y = Self::y_at_gain(gain);
        (y - (local.y - RULER_HEIGHT))
            .abs()
            .le(&LINE_HIT_RADIUS)
            .then_some(gain)
    }

    fn hit_edit_point(&self, row: &RowLayout, local: Point) -> Option<f64> {
        self.edit_points
            .iter()
            .copied()
            .find(|time| *time >= row.start && *time <= row.end)
            .filter(|time| (Self::x_at_time(row, *time) - local.x).abs() <= EDIT_POINT_HIT)
    }

    fn hit_deleted(&self, time: f64) -> Option<Region> {
        deleted_at_time(&self.deleted, time)
    }
}

/// Finds the deleted region under `time`. The right edge is exclusive so a
/// click exactly at a boundary belongs to the audio after the cut.
fn deleted_at_time(regions: &[Region], time: f64) -> Option<Region> {
    regions
        .iter()
        .copied()
        .find(|region| time >= region.start && time < region.end)
}

/// Chooses the ruler interval that keeps labels at least
/// [`RULER_MIN_LABEL_GAP`] pixels apart.
pub fn ruler_step(pixels_per_second: f32) -> f64 {
    const STEPS: [f64; 15] = [
        0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
    ];
    let pps = pixels_per_second.max(0.25) as f64;
    for step in STEPS {
        if step * pps >= RULER_MIN_LABEL_GAP as f64 {
            return step;
        }
    }
    STEPS[STEPS.len() - 1]
}

fn format_gain(gain: f64) -> String {
    if gain <= 0.000_1 {
        "−∞ dB".into()
    } else {
        let db = 20.0 * gain.log10();
        format!("{}{db:.1} dB", if db >= 0.0 { "+" } else { "" })
    }
}

#[derive(Clone, Debug, Default)]
pub struct CanvasState {
    drag: Option<Drag>,
    envelope_hover: Option<(f32, f32, f64)>,
    hover_time: Option<f64>,
}

#[derive(Clone, Debug)]
enum Drag {
    Range { start: f64, current: f64 },
    Direct { start: f64, current: f64 },
    Envelope { id: String },
}

/// What the drag would do if it were released now. The colour of the preview
/// follows this, so a left-drag no longer looks like a destructive edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewOperation {
    Mute,
    Restore,
    Delete,
}

#[derive(Clone, Copy, Debug)]
struct Preview {
    start: f64,
    end: f64,
    operation: PreviewOperation,
}

impl Drag {
    fn preview(&self) -> Option<Preview> {
        match self {
            Self::Range { start, current } => {
                let region = Region::new(start.min(*current), start.max(*current));
                Some(Preview {
                    start: region.start,
                    end: region.end,
                    // Same sweep semantics as the destructive right-drag:
                    // forward silences, backward restores.
                    operation: if current >= start {
                        PreviewOperation::Mute
                    } else {
                        PreviewOperation::Restore
                    },
                })
            }
            Self::Direct { start, current } => Some(Preview {
                start: start.min(*current),
                end: start.max(*current),
                operation: if current >= start {
                    PreviewOperation::Delete
                } else {
                    PreviewOperation::Restore
                },
            }),
            Self::Envelope { .. } => None,
        }
    }
}

impl canvas::Program<Message> for TrackCanvas<'_> {
    type State = CanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let rows = layout_rows(bounds.width, self.duration, self.pixels_per_second);
        let publish = |event: WaveformEvent| {
            Some(Action::publish(Message::Waveform {
                track: self.track_id,
                event,
            }))
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let position = cursor.position_in(bounds)?;
                let (row, local) = Self::row_at(bounds, &rows, position)?;
                let time = Self::time_at_x(&row, local.x);

                if *button == mouse::Button::Right {
                    if self.show_envelope {
                        if let Some(point) = self.hit_envelope_point(&row, local) {
                            return publish(WaveformEvent::EnvelopeRemove(point.id));
                        }
                    }
                    state.drag = Some(Drag::Direct {
                        start: time,
                        current: time,
                    });
                    return Some(Action::capture());
                }
                if *button != mouse::Button::Left {
                    return None;
                }
                // The ruler is a scrub strip: clicking it seeks without ever
                // starting a range selection.
                if local.y < RULER_HEIGHT {
                    state.drag = None;
                    return publish(WaveformEvent::Seek(time));
                }
                if self.blade {
                    state.drag = None;
                    return publish(WaveformEvent::Blade(time));
                }
                if let Some(edit_point) = self.hit_edit_point(&row, local) {
                    state.drag = None;
                    return publish(WaveformEvent::RemoveEditPoint(edit_point));
                }
                if self.show_envelope {
                    if let Some(point) = self.hit_envelope_point(&row, local) {
                        state.drag = Some(Drag::Envelope { id: point.id });
                        return Some(Action::capture());
                    }
                    if let Some(gain) = self.hit_envelope_line(local, time) {
                        let point = EnvelopePoint::new(
                            format!("point-{}", crate::model::new_id()),
                            time,
                            gain,
                        );
                        state.drag = Some(Drag::Envelope {
                            id: point.id.clone(),
                        });
                        return publish(WaveformEvent::EnvelopeAdd(point));
                    }
                }
                if let Some(region) = self.hit_deleted(time) {
                    state.drag = None;
                    return publish(WaveformEvent::Restore(region));
                }
                state.drag = Some(Drag::Range {
                    start: time,
                    current: time,
                });
                state.envelope_hover = None;
                Some(Action::capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(position) = cursor.position_in(bounds) else {
                    state.envelope_hover = None;
                    state.hover_time = None;
                    return None;
                };
                if let Some(drag) = state.drag.clone() {
                    match drag {
                        Drag::Range { start, .. } => {
                            let (row, local) = Self::row_at(bounds, &rows, position)?;
                            let time = Self::time_at_x(&row, local.x);
                            state.drag = Some(Drag::Range {
                                start,
                                current: time,
                            });
                            return Some(Action::request_redraw());
                        }
                        Drag::Direct { start, .. } => {
                            let (row, local) = Self::row_at(bounds, &rows, position)?;
                            let time = Self::time_at_x(&row, local.x);
                            state.drag = Some(Drag::Direct {
                                start,
                                current: time,
                            });
                            return Some(Action::request_redraw());
                        }
                        Drag::Envelope { id } => {
                            let (row, local) = Self::row_at(bounds, &rows, position)?;
                            let time = Self::time_at_x(&row, local.x);
                            let gain = Self::gain_at_y(local.y - RULER_HEIGHT);
                            state.envelope_hover =
                                Some((local.x, Self::row_top(row.index) + local.y, gain));
                            return publish(WaveformEvent::EnvelopeMove { id, time, gain });
                        }
                    }
                }
                if let Some((row, local)) = Self::row_at(bounds, &rows, position) {
                    let time = Self::time_at_x(&row, local.x);
                    state.hover_time = Some(time);
                    if self.show_envelope {
                        let gain = envelope_gain_at_time(&self.envelope, self.duration, time);
                        let y = Self::y_at_gain(gain);
                        state.envelope_hover = if (y - (local.y - RULER_HEIGHT)).abs() <= 12.0 {
                            Some((local.x, Self::row_top(row.index) + RULER_HEIGHT + y, gain))
                        } else {
                            None
                        };
                    } else {
                        state.envelope_hover = None;
                    }
                    return Some(Action::request_redraw());
                }
                state.hover_time = None;
                state.envelope_hover = None;
                None
            }
            Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                let drag = state.drag.take()?;
                let position = cursor.position_in(bounds);
                match (drag, button) {
                    (Drag::Range { start, current }, mouse::Button::Left) => {
                        let current = position
                            .and_then(|position| Self::row_at(bounds, &rows, position))
                            .map(|(row, local)| Self::time_at_x(&row, local.x))
                            .unwrap_or(current);
                        let minimum = (3.0 / self.pixels_per_second).max(0.01) as f64;
                        match mute_region_edit(start, current, minimum) {
                            // Left-drag mirrors the delete gesture: forward
                            // silences the swept range, backward restores it.
                            // The selection is kept so M can toggle it and
                            // Delete can cut it out.
                            Some(edit) => publish(WaveformEvent::MuteEdit {
                                region: edit.region,
                                operation: edit.operation,
                            }),
                            None => publish(WaveformEvent::Seek(start)),
                        }
                    }
                    (Drag::Direct { start, current }, mouse::Button::Right) => {
                        let current = position
                            .and_then(|position| Self::row_at(bounds, &rows, position))
                            .map(|(row, local)| Self::time_at_x(&row, local.x))
                            .unwrap_or(current);
                        let minimum = (3.0 / self.pixels_per_second).max(0.01) as f64;
                        match direct_region_edit(start, current, minimum) {
                            Some(edit) => publish(WaveformEvent::DirectEdit {
                                region: edit.region,
                                operation: edit.operation,
                            }),
                            None => Some(Action::request_redraw()),
                        }
                    }
                    (Drag::Envelope { .. }, _) => Some(Action::request_redraw()),
                    _ => None,
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.envelope_hover = None;
                state.hover_time = None;
                Some(Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let key = self.cache_key();
        if self.cache.revision.get() != key {
            self.cache.cache.clear();
            self.cache.revision.set(key);
        }
        let base = self.cache.cache.draw(renderer, bounds.size(), |frame| {
            self.draw_base(frame, bounds);
        });
        let mut overlay = Frame::new(renderer, bounds.size());
        self.draw_overlays(&mut overlay, bounds, state);
        vec![base, overlay.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(_bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

/// One waveform pixel column reduced to the quietest and loudest RMS frame it
/// covers.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RmsColumn {
    min: f64,
    max: f64,
}

/// Buckets RMS frames into pixel columns, the way the original canvas did.
fn rms_columns(rms: &[RmsFrame], start: f64, end: f64, width: usize) -> Vec<RmsColumn> {
    let mut columns = Vec::with_capacity(width);
    if width == 0 || rms.is_empty() || end <= start {
        return columns;
    }
    let mut index = rms
        .partition_point(|frame| frame.time < start)
        .saturating_sub(1);
    let mut previous = rms[index].amplitude;
    let span = end - start;
    for column in 0..width {
        let bin_end = start + span * (column + 1) as f64 / width as f64;
        let mut min = f64::INFINITY;
        let mut max = 0.0_f64;
        while index < rms.len() && rms[index].time < bin_end && rms[index].time <= end {
            min = min.min(rms[index].amplitude);
            max = max.max(rms[index].amplitude);
            previous = rms[index].amplitude;
            index += 1;
        }
        if !min.is_finite() {
            min = previous;
        }
        if max == 0.0 {
            max = previous;
        }
        columns.push(RmsColumn { min, max });
    }
    columns
}

/// Draws a dashed horizontal rule (5px dash, 4px gap) at `y`.
fn draw_dashed_rule(frame: &mut Frame, x: f32, y: f32, length: f32, color: Color) {
    const DASH: f32 = 5.0;
    const GAP: f32 = 4.0;
    let mut offset = 0.0;
    while offset < length {
        let segment = DASH.min(length - offset);
        frame.fill_rectangle(Point::new(x + offset, y), Size::new(segment, 1.0), color);
        offset += DASH + GAP;
    }
}

/// Diagonal hatching that marks audio removed from the output.
fn draw_hatch(frame: &mut Frame, left: f32, top: f32, width: f32, height: f32, color: Color) {
    const SPACING: f32 = 9.0;
    let mut offset = -height;
    while offset < width {
        let start_x = left + offset.max(0.0);
        let start_y = top + (-offset).max(0.0);
        let visible = (width - offset.max(0.0)).min(height + offset);
        if visible > 0.0 {
            let path = Path::new(|builder| {
                builder.move_to(Point::new(start_x, start_y));
                builder.line_to(Point::new(
                    start_x + visible.min(height - (-offset).max(0.0)),
                    start_y + visible.min(height - (-offset).max(0.0)),
                ));
            });
            frame.stroke(&path, Stroke::default().with_color(color).with_width(1.0));
        }
        offset += SPACING;
    }
}

#[cfg(test)]
mod tests;
