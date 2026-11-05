//! Playback position bookkeeping shared by every GUI frontend.

use crate::region::Region;
use crate::timeline::original_time_at_edited_offset;

#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackTiming {
    pub context_time: f64,
    pub started_at: f64,
    pub edited_offset: f64,
    pub kept_regions: Vec<Region>,
}

/// Computes the source position to resume from when playback is stopped.
pub fn saved_playback_position(has_active_source: bool, timing: &PlaybackTiming) -> Option<f64> {
    if !has_active_source {
        return None;
    }
    let duration: f64 = timing.kept_regions.iter().map(Region::duration).sum();
    let elapsed = (timing.context_time - timing.started_at).max(0.0);
    let edited_time = (timing.edited_offset + elapsed).clamp(0.0, duration);
    Some(original_time_at_edited_offset(
        edited_time,
        &timing.kept_regions,
    ))
}

#[cfg(test)]
mod tests {
    use super::{saved_playback_position, PlaybackTiming};
    use crate::region::Region;

    fn timing() -> PlaybackTiming {
        PlaybackTiming {
            context_time: 12.0,
            started_at: 10.0,
            edited_offset: 1.0,
            kept_regions: vec![Region::new(0.0, 5.0), Region::new(8.0, 11.0)],
        }
    }

    #[test]
    fn does_not_derive_a_position_without_an_active_source() {
        assert_eq!(saved_playback_position(false, &timing()), None);
    }

    #[test]
    fn maps_an_edited_position_back_to_source_time() {
        assert_eq!(saved_playback_position(true, &timing()), Some(3.0));
    }

    #[test]
    fn clamps_a_position_past_the_end_to_the_final_kept_region() {
        let mut timing = timing();
        timing.context_time = 40.0;
        assert_eq!(saved_playback_position(true, &timing), Some(11.0));
    }
}
