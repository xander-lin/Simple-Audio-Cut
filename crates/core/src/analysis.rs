//! Short-term RMS analysis and silence detection.

use crate::buffer::AudioBuffer;
use crate::region::Region;

pub const DEFAULT_RMS_WINDOW_DURATION: f64 = 0.005;
pub const DEFAULT_RMS_HOP_DURATION: f64 = 0.0025;
pub const DEFAULT_SILENCE_MIN_DURATION: f64 = 0.2;
pub const DEFAULT_SILENCE_THRESHOLD_DBFS: f64 = -36.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RmsFrame {
    pub time: f64,
    pub amplitude: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RmsOptions {
    pub window_duration: f64,
    pub hop_duration: f64,
}

impl Default for RmsOptions {
    fn default() -> Self {
        Self {
            window_duration: DEFAULT_RMS_WINDOW_DURATION,
            hop_duration: DEFAULT_RMS_HOP_DURATION,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SilenceOptions {
    /// Linear amplitude threshold (`10^(dBFS / 20)`).
    pub threshold: f64,
    pub min_duration: f64,
    pub padding: f64,
}

impl Default for SilenceOptions {
    fn default() -> Self {
        Self {
            threshold: dbfs_to_amplitude(DEFAULT_SILENCE_THRESHOLD_DBFS),
            min_duration: DEFAULT_SILENCE_MIN_DURATION,
            padding: 0.0,
        }
    }
}

pub fn dbfs_to_amplitude(dbfs: f64) -> f64 {
    10_f64.powf(dbfs / 20.0)
}

pub fn amplitude_to_dbfs(amplitude: f64, floor_db: f64) -> f64 {
    if amplitude <= 0.0 || !amplitude.is_finite() {
        return floor_db;
    }
    (20.0 * amplitude.log10()).max(floor_db)
}

pub fn analyze_rms(buffer: &AudioBuffer, options: RmsOptions) -> Vec<RmsFrame> {
    analyze_rms_from_channels(buffer.channels(), buffer.sample_rate(), options)
}

pub fn analyze_rms_from_channels(
    channels: &[Vec<f32>],
    sample_rate: u32,
    options: RmsOptions,
) -> Vec<RmsFrame> {
    if channels.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let frame_count = channels.iter().map(Vec::len).min().unwrap_or(0);
    if frame_count == 0 {
        return Vec::new();
    }
    let window_frames = ((sample_rate as f64 * options.window_duration.max(0.0)).round() as usize).max(1);
    let hop_frames = ((sample_rate as f64 * options.hop_duration.max(0.0)).round() as usize).max(1);
    let mut frames = Vec::with_capacity(frame_count / hop_frames + 1);

    let mut frame_start = 0;
    while frame_start < frame_count {
        let frame_end = (frame_start + window_frames).min(frame_count);
        let samples_in_window = frame_end - frame_start;
        let mut loudest_channel_rms: f64 = 0.0;
        for channel in channels {
            let mut sum_squares = 0.0;
            for sample in &channel[frame_start..frame_end] {
                sum_squares += (*sample as f64) * (*sample as f64);
            }
            loudest_channel_rms = loudest_channel_rms
                .max((sum_squares / samples_in_window as f64).sqrt());
        }
        frames.push(RmsFrame {
            time: (frame_start as f64 + samples_in_window as f64 / 2.0) / sample_rate as f64,
            amplitude: loudest_channel_rms,
        });
        frame_start += hop_frames;
    }
    frames
}

pub fn detect_silence(buffer: &AudioBuffer, options: SilenceOptions) -> Vec<Region> {
    detect_silence_from_analysis(&analyze_rms(buffer, RmsOptions::default()), buffer.duration(), options)
}

pub fn detect_silence_from_channels(
    channels: &[Vec<f32>],
    sample_rate: u32,
    options: SilenceOptions,
) -> Vec<Region> {
    if channels.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let frame_count = channels.iter().map(Vec::len).min().unwrap_or(0);
    let frames = analyze_rms_from_channels(channels, sample_rate, RmsOptions::default());
    detect_silence_from_analysis(
        &frames,
        frame_count as f64 / sample_rate as f64,
        options,
    )
}

pub fn detect_silence_from_analysis(
    frames: &[RmsFrame],
    duration: f64,
    options: SilenceOptions,
) -> Vec<Region> {
    if frames.is_empty() || duration <= 0.0 {
        return Vec::new();
    }
    let threshold = options.threshold.max(0.0);
    let min_duration = options.min_duration.max(0.0);
    let padding = options.padding.max(0.0);
    let mut regions = Vec::new();
    let mut silence_start: Option<f64> = None;
    let mut previous_frame: Option<RmsFrame> = None;

    let threshold_crossing = |left: RmsFrame, right: RmsFrame| {
        let delta = right.amplitude - left.amplitude;
        if delta.abs() < f64::EPSILON {
            return (left.time + right.time) / 2.0;
        }
        let progress = ((threshold - left.amplitude) / delta).clamp(0.0, 1.0);
        left.time + (right.time - left.time) * progress
    };

    let finish_silence = |start: Option<f64>, end: f64, regions: &mut Vec<Region>| {
        let Some(start) = start else { return };
        if end - start > min_duration && min_duration > 0.0 {
            let center = (start + end) / 2.0;
            let padded_start = center - min_duration / 2.0 + padding;
            let padded_end = center + min_duration / 2.0 - padding;
            if padded_end > padded_start {
                regions.push(Region::new(padded_start, padded_end));
            }
        }
    };

    for frame in frames {
        if frame.amplitude < threshold {
            if silence_start.is_none() {
                silence_start = Some(match previous_frame {
                    Some(previous) if previous.amplitude >= threshold => {
                        threshold_crossing(previous, *frame)
                    }
                    _ => 0.0,
                });
            }
        } else {
            let end = match previous_frame {
                Some(previous) if previous.amplitude < threshold => {
                    threshold_crossing(previous, *frame)
                }
                _ => frame.time,
            };
            finish_silence(silence_start, end, &mut regions);
            silence_start = None;
        }
        previous_frame = Some(*frame);
    }

    finish_silence(silence_start, duration, &mut regions);
    regions
}

#[cfg(test)]
mod tests {
    use super::{
        amplitude_to_dbfs, analyze_rms_from_channels, dbfs_to_amplitude,
        detect_silence_from_channels, RmsOptions, SilenceOptions,
    };
    use crate::region::Region;

    const SAMPLE_RATE: u32 = 1_000;

    fn detect(channels: &[Vec<f32>], options: SilenceOptions) -> Vec<Region> {
        detect_silence_from_channels(channels, SAMPLE_RATE, options)
    }

    fn default_silence() -> SilenceOptions {
        SilenceOptions {
            threshold: dbfs_to_amplitude(-30.0),
            ..SilenceOptions::default()
        }
    }

    fn assert_region(region: Region, start: f64, end: f64) {
        assert!(
            (region.start - start).abs() <= 0.011,
            "{} != {start}",
            region.start
        );
        assert!(
            (region.end - end).abs() <= 0.011,
            "{} != {end}",
            region.end
        );
    }

    #[test]
    fn uses_the_loudest_channel_when_classifying_stereo_audio() {
        let left = vec![0.0; 3_000];
        let mut right = vec![0.0; 3_000];
        right[500..1_500].fill(0.2);

        let regions = detect(&[left, right], default_silence());

        assert_eq!(regions.len(), 2);
        assert_region(regions[0], 0.145, 0.345);
        assert_region(regions[1], 2.15, 2.35);
    }

    #[test]
    fn protects_a_quiet_gap_shorter_than_the_default_duration() {
        let mut audio = vec![0.2; 1_000];
        audio[400..500].fill(0.0);
        assert!(detect(&[audio], default_silence()).is_empty());
    }

    #[test]
    fn uses_the_actual_sample_count_in_the_final_window() {
        let audio = vec![0.2; 1_005];
        assert!(detect(&[audio], default_silence()).is_empty());
    }

    #[test]
    fn reclassifies_regions_when_the_threshold_moves_in_either_direction() {
        let audio = vec![0.02; 1_000];
        let quiet = detect(std::slice::from_ref(&audio), default_silence());
        assert_eq!(quiet.len(), 1);
        assert_region(quiet[0], 0.4, 0.6);

        let louder = detect(
            &[audio],
            SilenceOptions {
                threshold: dbfs_to_amplitude(-40.0),
                ..default_silence()
            },
        );
        assert!(louder.is_empty());
    }

    #[test]
    fn exposes_the_same_multichannel_frames_used_by_silence_detection() {
        let left = vec![0.1; 100];
        let right = vec![0.25; 100];
        let frames = analyze_rms_from_channels(
            &[left, right],
            SAMPLE_RATE,
            RmsOptions {
                window_duration: 0.02,
                hop_duration: 0.01,
            },
        );
        assert_eq!(frames.len(), 10);
        assert!((frames[0].amplitude - 0.25).abs() < 1e-6);
        assert!((amplitude_to_dbfs(frames[0].amplitude, -70.0) + 12.0412).abs() < 0.001);
    }

    #[test]
    fn detects_a_quiet_gap_without_duration_gating() {
        let mut audio = vec![0.2; 1_000];
        audio[400..615].fill(0.0);
        let regions = detect(&[audio], default_silence());
        assert_eq!(regions.len(), 1);
        assert_region(regions[0], 0.4075, 0.6075);
    }

    #[test]
    fn detects_a_gap_shorter_than_the_configured_minimum() {
        let mut audio = vec![0.2; 1_000];
        audio[400..590].fill(0.0);
        let regions = detect(
            &[audio],
            SilenceOptions {
                min_duration: 0.15,
                ..default_silence()
            },
        );
        assert_eq!(regions.len(), 1);
        assert_region(regions[0], 0.42, 0.57);
    }

    #[test]
    fn does_not_create_zero_duration_silence_cuts() {
        let mut audio = vec![0.2; 1_000];
        audio[400..410].fill(0.0);
        assert!(detect(
            &[audio],
            SilenceOptions {
                min_duration: 0.0,
                ..default_silence()
            }
        )
        .is_empty());
    }

    #[test]
    fn does_not_let_a_short_loud_section_hide_neighbouring_quiet_sections() {
        let mut audio = vec![0.0; 1_000];
        audio[495..505].fill(0.2);
        let regions = detect(&[audio], default_silence());
        assert_eq!(regions.len(), 2);
        assert_region(regions[0], 0.1475, 0.3475);
        assert_region(regions[1], 0.6525, 0.8525);
    }
}
