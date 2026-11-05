//! Gesture helpers for direct waveform editing.

use crate::region::Region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectRegionOperation {
    Delete,
    Restore,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectRegionEdit {
    pub region: Region,
    pub operation: DirectRegionOperation,
}

/// Silence applied by a left-button drag. Same sweep semantics as the
/// right-button delete gesture: forward silences, backward restores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuteRegionOperation {
    Mute,
    Restore,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MuteRegionEdit {
    pub region: Region,
    pub operation: MuteRegionOperation,
}

/// The normalized range covered by a drag plus its direction. Returns `None`
/// for a tap (and for non-finite input), so a click still seeks.
fn swept_region(start: f64, current: f64, minimum_duration: f64) -> Option<(Region, bool)> {
    let delta = current - start;
    if !delta.is_finite() || delta.abs() <= minimum_duration {
        return None;
    }
    Some((
        Region::new(start.min(current), start.max(current)),
        delta > 0.0,
    ))
}

/// Maps a right-button drag to the original delete/restore gestures: dragging
/// forward deletes, dragging backward restores.
pub fn direct_region_edit(
    start: f64,
    current: f64,
    minimum_duration: f64,
) -> Option<DirectRegionEdit> {
    let (region, forward) = swept_region(start, current, minimum_duration)?;
    Some(DirectRegionEdit {
        region,
        operation: if forward {
            DirectRegionOperation::Delete
        } else {
            DirectRegionOperation::Restore
        },
    })
}

/// Maps a left-button drag to the mute/restore gestures, mirroring the
/// delete gesture: dragging forward silences the swept range, dragging
/// backward un-silences it.
pub fn mute_region_edit(
    start: f64,
    current: f64,
    minimum_duration: f64,
) -> Option<MuteRegionEdit> {
    let (region, forward) = swept_region(start, current, minimum_duration)?;
    Some(MuteRegionEdit {
        region,
        operation: if forward {
            MuteRegionOperation::Mute
        } else {
            MuteRegionOperation::Restore
        },
    })
}

/// Returns the clip bounded by the surrounding edit points, if any.
pub fn segment_at_time(points: &[f64], time: f64, duration: f64) -> Option<Region> {
    if points.is_empty() || duration <= 0.0 {
        return None;
    }
    let mut cuts: Vec<f64> = std::iter::once(0.0)
        .chain(points.iter().copied().filter(|point| *point > 0.0 && *point < duration))
        .chain(std::iter::once(duration))
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|left, right| (*left - *right).abs() < 0.01);

    let safe_time = time.clamp(0.0, duration);
    let end_index = cuts
        .iter()
        .position(|point| *point > safe_time)
        .unwrap_or(cuts.len() - 1);
    Some(Region::new(
        cuts[end_index.saturating_sub(1)],
        cuts[end_index],
    ))
}

/// Adds an edit point, keeping the list ordered and free of near-duplicates.
pub fn add_edit_point(points: &[f64], time: f64, duration: f64) -> Vec<f64> {
    let safe_time = time.clamp(0.0, duration);
    if safe_time <= 0.001 || safe_time >= duration - 0.001 {
        return points.to_vec();
    }
    let mut next: Vec<f64> = points.to_vec();
    next.push(safe_time);
    next.sort_by(f64::total_cmp);
    next.dedup_by(|left, right| (*left - *right).abs() < 0.01);
    next
}

#[cfg(test)]
mod tests {
    use super::{
        add_edit_point, direct_region_edit, mute_region_edit, segment_at_time,
        DirectRegionOperation, MuteRegionOperation,
    };
    use crate::region::Region;

    #[test]
    fn maps_right_button_drag_direction_to_delete_and_restore() {
        let delete = direct_region_edit(2.0, 6.0, 0.01).expect("delete edit");
        assert_eq!(delete.region, Region::new(2.0, 6.0));
        assert_eq!(delete.operation, DirectRegionOperation::Delete);

        let restore = direct_region_edit(6.0, 2.0, 0.01).expect("restore edit");
        assert_eq!(restore.region, Region::new(2.0, 6.0));
        assert_eq!(restore.operation, DirectRegionOperation::Restore);

        assert!(direct_region_edit(2.0, 2.005, 0.01).is_none());
        assert!(direct_region_edit(2.0, 5.0, 12.0).is_none());
    }

    #[test]
    fn maps_left_button_drag_direction_to_mute_and_restore() {
        let mute = mute_region_edit(2.0, 6.0, 0.01).expect("mute edit");
        assert_eq!(mute.region, Region::new(2.0, 6.0));
        assert_eq!(mute.operation, MuteRegionOperation::Mute);

        let restore = mute_region_edit(6.0, 2.0, 0.01).expect("restore edit");
        assert_eq!(restore.region, Region::new(2.0, 6.0));
        assert_eq!(restore.operation, MuteRegionOperation::Restore);

        // A tap (or a sub-pixel wiggle) is a seek, not a mute.
        assert!(mute_region_edit(2.0, 2.005, 0.01).is_none());
        assert!(mute_region_edit(2.0, 2.0, 0.0).is_none());
        assert!(mute_region_edit(f64::NAN, 2.0, 0.0).is_none());
    }

    #[test]
    fn adds_ordered_edit_points_without_duplicating_nearby_cuts() {
        assert_eq!(add_edit_point(&[7.0, 2.0], 4.0, 10.0), vec![2.0, 4.0, 7.0]);
        assert_eq!(add_edit_point(&[4.0], 4.005, 10.0), vec![4.0]);
        assert!(add_edit_point(&[], 0.0, 10.0).is_empty());
        assert!(add_edit_point(&[], 10.0, 10.0).is_empty());
    }

    #[test]
    fn maps_a_click_to_the_clip_bounded_by_edit_points() {
        assert_eq!(
            segment_at_time(&[3.0, 7.0], 5.0, 10.0),
            Some(Region::new(3.0, 7.0))
        );
        assert_eq!(
            segment_at_time(&[3.0, 7.0], 7.0, 10.0),
            Some(Region::new(7.0, 10.0))
        );
        assert_eq!(segment_at_time(&[], 5.0, 10.0), None);
    }
}
