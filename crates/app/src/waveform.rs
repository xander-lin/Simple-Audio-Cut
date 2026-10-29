//! Waveform canvas: one canvas per timeline track, internally wrapped into
//! rows the same way the original editor wrapped long audio.

use std::cell::Cell;
use std::sync::Arc;

use iced::mouse;
use iced::widget::canvas::{self, Action, Cache, Event, Frame, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use simple_audio_cut_core::analysis::{amplitude_to_dbfs, RmsFrame};
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::direct_edit::{direct_region_edit, DirectRegionOperation};
use simple_audio_cut_core::region::{envelope_gain_at_time, EnvelopePoint, Region};

use crate::message::{Message, WaveformEvent};
use crate::model::MediaId;
use crate::theme;
use crate::ui::{format_time_compact, format_time_precise};

pub const ROW_HEIGHT: f32 = 120.0;
pub const ROW_GAP: f32 = 10.0;
const EDGE_MIN: f32 = 24.0;
const HIT_RADIUS: f32 = 14.0;
const LINE_HIT_RADIUS: f32 = 8.0;
const EDIT_POINT_HIT: f32 = 6.0;

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

    fn y_at_gain(gain: f64) -> f32 {
        (ROW_HEIGHT as f64 * (1.0 - gain / 2.0)) as f32
    }

    fn gain_at_y(y: f32) -> f64 {
        (2.0 * (1.0 - y as f64 / ROW_HEIGHT as f64)).clamp(0.0, 2.0)
    }

    fn row_top(index: usize) -> f32 {
        index as f32 * (ROW_HEIGHT + ROW_GAP)
    }

    fn row_at(bounds: Rectangle, rows: &[RowLayout], position: Point) -> Option<(RowLayout, Point)> {
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

    fn draw_base(&self, frame: &mut Frame, bounds: Rectangle) {
        let rows = layout_rows(bounds.width, self.duration, self.pixels_per_second);
        for row in rows {
            let top = Self::row_top(row.index);
            let row_rect = Rectangle::new(
                Point::new(0.0, top),
                Size::new(bounds.width, ROW_HEIGHT),
            );
            frame.fill_rectangle(row_rect.position(), row_rect.size(), Color::from_rgb(0.102, 0.129, 0.122));
            // Center line.
            frame.fill_rectangle(
                Point::new(row.leading, top + ROW_HEIGHT / 2.0),
                Size::new(row.timeline_width(), 1.0),
                Color::from_rgb(0.22, 0.27, 0.25),
            );
            self.draw_rms(frame, &row, top);
            self.draw_peaks(frame, &row, top);
            if self.show_threshold {
                let radius = Self::rms_radius(
                    simple_audio_cut_core::analysis::dbfs_to_amplitude(self.threshold_db),
                );
                let color = Color::from_rgba(0.84, 0.74, 0.35, 0.70);
                frame.fill_rectangle(
                    Point::new(row.leading, top + ROW_HEIGHT / 2.0 - radius),
                    Size::new(row.timeline_width(), 1.0),
                    color,
                );
                frame.fill_rectangle(
                    Point::new(row.leading, top + ROW_HEIGHT / 2.0 + radius),
                    Size::new(row.timeline_width(), 1.0),
                    color,
                );
            }
            frame.fill_text(Text {
                content: format_time_compact(row.end),
                position: Point::new(
                    (bounds.width - 6.0f32).max(0.0),
                    top + ROW_HEIGHT - 16.0,
                ),
                color: Color::from_rgba(0.55, 0.64, 0.60, 0.8),
                size: 10.0.into(),
                ..Text::default()
            });
        }
    }

    fn rms_radius(amplitude: f64) -> f32 {
        let normalized = ((amplitude_to_dbfs(amplitude, -70.0) + 70.0) / 70.0).clamp(0.0, 1.0);
        (normalized as f32) * (ROW_HEIGHT / 2.0 - 4.0)
    }

    fn draw_rms(&self, frame: &mut Frame, row: &RowLayout, top: f32) {
        let width = row.timeline_width().ceil() as usize;
        if width == 0 || self.rms.is_empty() {
            return;
        }
        let mut upper: Vec<f32> = Vec::with_capacity(width);
        let mut lower: Vec<f32> = Vec::with_capacity(width);
        for index in 0..width {
            let time = row.start + (index as f64 / width as f64) * (row.end - row.start);
            let frame_index = self
                .rms
                .binary_search_by(|frame| frame.time.total_cmp(&time))
                .unwrap_or_else(|index| index.min(self.rms.len() - 1));
            let amplitude = self.rms[frame_index].amplitude;
            let radius = Self::rms_radius(amplitude);
            upper.push(top + ROW_HEIGHT / 2.0 - radius);
            lower.push(top + ROW_HEIGHT / 2.0 + radius);
        }
        let path = Path::new(|builder| {
            builder.move_to(Point::new(row.leading, upper[0]));
            for (index, y) in upper.iter().enumerate().skip(1) {
                builder.line_to(Point::new(row.leading + index as f32, *y));
            }
            for (index, y) in lower.iter().enumerate().rev() {
                builder.line_to(Point::new(row.leading + index as f32, *y));
            }
            builder.close();
        });
        frame.fill(&path, Color::from_rgba(0.41, 0.58, 0.67, 0.22));
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(Color::from_rgba(0.52, 0.69, 0.78, 0.55))
                .with_width(1.0),
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
                let center = top + ROW_HEIGHT / 2.0;
                let half = ROW_HEIGHT / 2.0;
                builder.move_to(Point::new(row.leading + x as f32, center + min * half));
                builder.line_to(Point::new(row.leading + x as f32, center + max * half));
            }
        });
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(Color::from_rgb(0.46, 0.68, 0.58))
                .with_width(1.0),
        );
    }

    fn draw_overlays(&self, frame: &mut Frame, bounds: Rectangle, state: &CanvasState) {
        let rows = layout_rows(bounds.width, self.duration, self.pixels_per_second);
        for row in &rows {
            let top = Self::row_top(row.index);
            let center = top + ROW_HEIGHT / 2.0;

            for region in &self.deleted {
                let start = region.start.max(row.start);
                let end = region.end.min(row.end);
                if end <= start {
                    continue;
                }
                let left = Self::x_at_time(row, start);
                let width = (Self::x_at_time(row, end) - left).max(1.0);
                frame.fill_rectangle(
                    Point::new(left, top),
                    Size::new(width, ROW_HEIGHT),
                    Color::from_rgba(0.06, 0.07, 0.07, 0.72),
                );
                if width > 58.0 {
                    frame.fill_text(Text {
                        content: "Restore".into(),
                        position: Point::new(left + 7.0, center - 7.0),
                        color: Color::from_rgba(0.85, 0.89, 0.88, 0.85),
                        size: 11.0.into(),
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
                    Point::new(left, top),
                    Size::new(width, ROW_HEIGHT),
                    Color::from_rgba(0.84, 0.74, 0.35, 0.16),
                );
                if width > 46.0 {
                    frame.fill_text(Text {
                        content: "Muted".into(),
                        position: Point::new(left + 7.0, center - 7.0),
                        color: Color::from_rgba(0.84, 0.74, 0.35, 0.85),
                        size: 11.0.into(),
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
                        Point::new(left, top),
                        Size::new(width, ROW_HEIGHT),
                        Color::from_rgba(0.41, 0.58, 0.67, 0.20),
                    );
                    frame.fill_rectangle(
                        Point::new(left, top),
                        Size::new(1.0, ROW_HEIGHT),
                        Color::from_rgba(0.70, 0.85, 0.92, 0.75),
                    );
                    frame.fill_rectangle(
                        Point::new(left + width - 1.0, top),
                        Size::new(1.0, ROW_HEIGHT),
                        Color::from_rgba(0.70, 0.85, 0.92, 0.75),
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
                            top + Self::y_at_gain(gain),
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
                        .with_color(Color::from_rgba(0.84, 0.74, 0.35, 0.9))
                        .with_width(1.6),
                );
                for point in self.visible_envelope(row) {
                    let center = Point::new(
                        Self::x_at_time(row, point.time),
                        top + Self::y_at_gain(point.gain),
                    );
                    let circle = Path::circle(center, 4.5);
                    frame.fill(&circle, Color::from_rgba(0.95, 0.86, 0.52, 0.95));
                    frame.stroke(
                        &circle,
                        Stroke::default()
                            .with_color(Color::from_rgba(0.16, 0.18, 0.16, 0.9))
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
                    Point::new(x - 0.5, top),
                    Size::new(1.0, ROW_HEIGHT),
                    Color::from_rgba(0.47, 0.72, 0.82, 0.85),
                );
                let marker = Path::new(|builder| {
                    builder.move_to(Point::new(x - 4.0, top));
                    builder.line_to(Point::new(x + 4.0, top));
                    builder.line_to(Point::new(x, top + 7.0));
                    builder.close();
                });
                frame.fill(&marker, Color::from_rgba(0.47, 0.72, 0.82, 0.95));
            }

            if let Some(drag) = &state.drag {
                if let Some(preview) = drag.preview() {
                    let start = preview.start.max(row.start);
                    let end = preview.end.min(row.end);
                    if end > start {
                        let left = Self::x_at_time(row, start);
                        let width = (Self::x_at_time(row, end) - left).max(1.0);
                        let color = match preview.operation {
                            DirectRegionOperation::Delete => {
                                Color::from_rgba(0.88, 0.46, 0.42, 0.30)
                            }
                            DirectRegionOperation::Restore => {
                                Color::from_rgba(0.35, 0.66, 0.47, 0.32)
                            }
                        };
                        frame.fill_rectangle(
                            Point::new(left, top),
                            Size::new(width, ROW_HEIGHT),
                            color,
                        );
                    }
                }
            }

            if let Some(playhead) = self.playhead {
                if playhead >= row.start && playhead <= row.end {
                    let x = Self::x_at_time(row, playhead);
                    frame.fill_rectangle(
                        Point::new(x - 1.0, top),
                        Size::new(2.0, ROW_HEIGHT),
                        theme::ACCENT,
                    );
                }
            }

            if let Some((x, y, gain)) = state.envelope_hover {
                let text_position = Point::new(
                    (x + 10.0).min(bounds.width - 70.0).max(4.0),
                    (y - 30.0).clamp(top + 4.0, top + ROW_HEIGHT - 24.0),
                );
                frame.fill_text(Text {
                    content: format_gain(gain),
                    position: text_position,
                    color: theme::ACCENT,
                    size: 11.0.into(),
                    ..Text::default()
                });
            }
        }
    }

    fn hit_envelope_point(
        &self,
        row: &RowLayout,
        local: Point,
    ) -> Option<EnvelopePoint> {
        self.visible_envelope(row)
            .into_iter()
            .find(|point| {
                let dx = Self::x_at_time(row, point.time) - local.x;
                let dy = Self::y_at_gain(point.gain) - local.y;
                (dx * dx + dy * dy).sqrt() <= HIT_RADIUS
            })
            .cloned()
    }

    fn hit_envelope_line(&self, local: Point, time: f64) -> Option<f64> {
        let gain = envelope_gain_at_time(&self.envelope, self.duration, time);
        let y = Self::y_at_gain(gain);
        (y - local.y).abs().le(&LINE_HIT_RADIUS).then_some(gain)
    }

    fn hit_edit_point(&self, row: &RowLayout, local: Point) -> Option<f64> {
        self.edit_points
            .iter()
            .copied()
            .find(|time| *time >= row.start && *time <= row.end)
            .filter(|time| (Self::x_at_time(row, *time) - local.x).abs() <= EDIT_POINT_HIT)
    }

    fn hit_deleted(&self, time: f64) -> Option<Region> {
        self.deleted.iter().copied().find(|region| region.contains(time))
    }
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
}

#[derive(Clone, Debug)]
enum Drag {
    Range {
        start: f64,
        current: f64,
    },
    Direct {
        start: f64,
        current: f64,
    },
    Envelope {
        id: String,
    },
}

#[derive(Clone, Copy, Debug)]
struct Preview {
    start: f64,
    end: f64,
    operation: DirectRegionOperation,
}

impl Drag {
    fn preview(&self) -> Option<Preview> {
        match self {
            Self::Range { start, current } => Some(Preview {
                start: start.min(*current),
                end: start.max(*current),
                operation: DirectRegionOperation::Delete,
            }),
            Self::Direct { start, current } => Some(Preview {
                start: start.min(*current),
                end: start.max(*current),
                operation: if current >= start {
                    DirectRegionOperation::Delete
                } else {
                    DirectRegionOperation::Restore
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
                        let point = EnvelopePoint::new(format!("point-{}", crate::model::new_id()), time, gain);
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
                            let gain = Self::gain_at_y(local.y);
                            state.envelope_hover = Some((local.x, local.y, gain));
                            return publish(WaveformEvent::EnvelopeMove { id, time, gain });
                        }
                    }
                }
                if self.show_envelope {
                    if let Some((row, local)) = Self::row_at(bounds, &rows, position) {
                        let time = Self::time_at_x(&row, local.x);
                        let gain = envelope_gain_at_time(&self.envelope, self.duration, time);
                        let y = Self::y_at_gain(gain);
                        state.envelope_hover = if (y - local.y).abs() <= 12.0 {
                            Some((local.x, y, gain))
                        } else {
                            None
                        };
                    }
                } else {
                    state.envelope_hover = None;
                }
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
                        if (current - start).abs() > minimum {
                            publish(WaveformEvent::Select(Some(Region::new(
                                start.min(current),
                                start.max(current),
                            ))))
                        } else {
                            publish(WaveformEvent::Seek(start))
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
        if self.cache.revision.get() != self.revision {
            self.cache.cache.clear();
            self.cache.revision.set(self.revision);
        }
        let base = self.cache.cache.draw(renderer, bounds.size(), |frame| {
            self.draw_base(frame, bounds);
        });
        let mut overlay = Frame::new(renderer, bounds.size());
        self.draw_overlays(&mut overlay, bounds, state);
        if self.selected {
            overlay.stroke_rectangle(
                Point::ORIGIN,
                bounds.size(),
                Stroke::default()
                    .with_color(Color::from_rgba(0.46, 0.68, 0.58, 0.55))
                    .with_width(1.0),
            );
        }
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

/// Formats the playhead time for the canvas tooltip area.
pub fn playhead_label(time: f64) -> String {
    format_time_precise(time)
}

#[cfg(test)]
mod tests {
    use super::{canvas_height, layout_rows, ROW_GAP, ROW_HEIGHT};

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
}
