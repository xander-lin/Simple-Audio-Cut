//! Timeline rendering: kept regions, time mapping and the edited audio buffer.

use crate::buffer::AudioBuffer;
use crate::region::{envelope_gain_at_time, EnvelopePoint, Region};

/// Time ranges of the source that survive the deletions, in source order.
pub fn kept_regions(deleted_regions: &[Region], total_duration: f64) -> Vec<Region> {
    if !total_duration.is_finite() || total_duration <= 0.0 {
        return Vec::new();
    }
    let mut sorted: Vec<Region> = deleted_regions
        .iter()
        .filter(|region| region.is_finite())
        .map(|region| region.clamped(total_duration))
        .filter(Region::is_valid)
        .collect();
    sorted.sort_by(|left, right| left.start.total_cmp(&right.start));

    let mut kept = Vec::new();
    let mut current_time = 0.0_f64;
    for region in sorted {
        if current_time < region.start {
            kept.push(Region::new(current_time, region.start));
        }
        current_time = current_time.max(region.end);
    }
    if current_time < total_duration {
        kept.push(Region::new(current_time, total_duration));
    }
    kept
}

/// Maps a source-timeline position to the edited timeline.
pub fn edited_offset_at_original_time(time: f64, kept: &[Region]) -> f64 {
    let mut offset = 0.0;
    for region in kept {
        if time <= region.start {
            return offset;
        }
        if time < region.end {
            return offset + time - region.start;
        }
        offset += region.duration();
    }
    offset
}

/// Maps an edited-timeline position back to the source timeline.
pub fn original_time_at_edited_offset(offset: f64, kept: &[Region]) -> f64 {
    let mut remaining = offset;
    for region in kept {
        let duration = region.duration();
        if remaining <= duration {
            return region.start + remaining;
        }
        remaining -= duration;
    }
    kept.last().map(|region| region.end).unwrap_or(0.0)
}

pub fn edited_duration(kept: &[Region]) -> f64 {
    kept.iter().map(Region::duration).sum()
}

/// Builds the audio that playback and collapsing operate on: deletions
/// removed, muted ranges silenced, volume envelope applied. The source buffer
/// is never modified.
pub fn create_edited_buffer(
    buffer: &AudioBuffer,
    deleted_regions: &[Region],
    envelope_points: &[EnvelopePoint],
    muted_regions: &[Region],
) -> AudioBuffer {
    let kept = kept_regions(deleted_regions, buffer.duration());
    let sample_rate = buffer.sample_rate();
    let ranges: Vec<(usize, usize)> = kept
        .iter()
        .map(|region| {
            (
                buffer.frame_at(region.start),
                buffer.frame_at(region.end),
            )
        })
        .collect();
    let total_frames: usize = ranges.iter().map(|(start, end)| end.saturating_sub(*start)).sum();

    let mut channels = Vec::with_capacity(buffer.channel_count());
    for channel in buffer.channels() {
        let mut output = Vec::with_capacity(total_frames);
        for (start, end) in &ranges {
            let mut chunk = channel[*start..*end].to_vec();
            apply_envelope(
                &mut chunk,
                *start,
                sample_rate,
                buffer.duration(),
                envelope_points,
            );
            apply_muted_regions(&mut chunk, *start, sample_rate, buffer.duration(), muted_regions);
            output.extend_from_slice(&chunk);
        }
        channels.push(output);
    }
    AudioBuffer::new(sample_rate, channels)
}

fn apply_muted_regions(
    samples: &mut [f32],
    source_start_frame: usize,
    sample_rate: u32,
    duration: f64,
    regions: &[Region],
) {
    for region in regions {
        if !region.is_finite() {
            continue;
        }
        let start = region.start.clamp(0.0, duration);
        let end = region.end.clamp(0.0, duration);
        if end <= start {
            continue;
        }
        let local_start = (start * sample_rate as f64).floor() as usize;
        let local_end = (end * sample_rate as f64).ceil() as usize;
        let local_start = local_start.saturating_sub(source_start_frame).min(samples.len());
        let local_end = local_end
            .saturating_sub(source_start_frame)
            .min(samples.len());
        if local_end > local_start {
            samples[local_start..local_end].fill(0.0);
        }
    }
}

fn apply_envelope(
    samples: &mut [f32],
    source_start_frame: usize,
    sample_rate: u32,
    duration: f64,
    points: &[EnvelopePoint],
) {
    if points.is_empty() || sample_rate == 0 {
        return;
    }
    for (index, sample) in samples.iter_mut().enumerate() {
        let time = (source_start_frame + index) as f64 / sample_rate as f64;
        *sample *= envelope_gain_at_time(points, duration, time) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        create_edited_buffer, edited_duration, edited_offset_at_original_time, kept_regions,
        original_time_at_edited_offset,
    };
    use crate::buffer::AudioBuffer;
    use crate::region::Region;

    #[test]
    fn normalizes_overlapping_and_out_of_bounds_deleted_regions() {
        let kept = kept_regions(
            &[
                Region::new(8.0, 20.0),
                Region::new(-2.0, 2.0),
                Region::new(1.0, 4.0),
                Region::new(6.0, 5.0),
                Region::new(f64::NAN, 7.0),
            ],
            10.0,
        );
        assert_eq!(kept, vec![Region::new(4.0, 8.0)]);
    }

    #[test]
    fn returns_the_complete_source_when_no_valid_deletion_exists() {
        assert_eq!(
            kept_regions(&[Region::new(4.0, 4.0)], 10.0),
            vec![Region::new(0.0, 10.0)]
        );
        assert!(kept_regions(&[], 0.0).is_empty());
    }

    #[test]
    fn maps_between_source_and_edited_time() {
        let kept = vec![Region::new(0.0, 2.0), Region::new(4.0, 6.0)];
        assert_eq!(edited_duration(&kept), 4.0);
        assert_eq!(edited_offset_at_original_time(1.0, &kept), 1.0);
        assert_eq!(edited_offset_at_original_time(2.0, &kept), 2.0);
        assert_eq!(edited_offset_at_original_time(5.0, &kept), 3.0);
        assert_eq!(original_time_at_edited_offset(3.0, &kept), 5.0);
        assert_eq!(original_time_at_edited_offset(9.0, &kept), 6.0);
    }

    #[test]
    fn replaces_muted_ranges_with_silence_without_changing_duration() {
        let source = AudioBuffer::new(10, vec![vec![1.0; 10]]);
        let result = create_edited_buffer(
            &source,
            &[],
            &[],
            &[Region::new(0.2, 0.5)],
        );
        assert_eq!(result.frames(), 10);
        assert_eq!(
            result.channel(0),
            &[1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0]
        );
    }

    #[test]
    fn concatenates_kept_regions_and_applies_the_envelope() {
        // 10 frames at 10 Hz => one second of audio.
        let source = AudioBuffer::new(10, vec![vec![1.0; 10]]);
        let result = create_edited_buffer(
            &source,
            &[Region::new(0.2, 0.4), Region::new(0.6, 0.8)],
            &[],
            &[Region::new(0.5, 0.6)],
        );
        assert_eq!(result.frames(), 6);
        assert_eq!(result.duration(), 0.6);
        assert_eq!(
            result.channel(0),
            &[1.0, 1.0, 1.0, 0.0, 1.0, 1.0]
        );
    }
}
