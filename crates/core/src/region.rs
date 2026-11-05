//! Non-destructive edit regions and volume envelope math.

use serde::{Deserialize, Serialize};

/// A half-open time range in seconds on the original media timeline.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Region {
    pub start: f64,
    pub end: f64,
}

impl Region {
    pub const fn new(start: f64, end: f64) -> Self {
        Self { start, end }
    }

    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn is_finite(&self) -> bool {
        self.start.is_finite() && self.end.is_finite()
    }

    pub fn is_valid(&self) -> bool {
        self.is_finite() && self.end > self.start
    }

    pub fn normalized(self) -> Self {
        Self {
            start: self.start.min(self.end),
            end: self.start.max(self.end),
        }
    }

    pub fn clamped(self, duration: f64) -> Self {
        if !self.is_finite() || !duration.is_finite() || duration <= 0.0 {
            return Self::new(0.0, 0.0);
        }
        Self {
            start: self.start.clamp(0.0, duration),
            end: self.end.clamp(0.0, duration),
        }
    }

    pub fn contains(&self, time: f64) -> bool {
        time >= self.start && time <= self.end
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// A volume envelope keyframe. `gain` is a linear multiplier in `0.0..=2.0`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct EnvelopePoint {
    pub id: String,
    pub time: f64,
    pub gain: f64,
}

impl EnvelopePoint {
    pub fn new(id: impl Into<String>, time: f64, gain: f64) -> Self {
        Self {
            id: id.into(),
            time,
            gain,
        }
    }
}

/// Adds `new_region` to `regions`, merging every overlap and adjacency.
pub fn merge_regions(regions: &[Region], new_region: Region) -> Vec<Region> {
    let mut sorted = regions.to_vec();
    sorted.push(new_region);
    sorted.retain(Region::is_finite);
    sorted.sort_by(|left, right| left.start.total_cmp(&right.start));
    let mut merged: Vec<Region> = Vec::with_capacity(sorted.len());
    for region in sorted {
        match merged.last_mut() {
            Some(last) if region.start <= last.end => {
                last.end = last.end.max(region.end);
            }
            _ => merged.push(region),
        }
    }
    merged
}

/// Flattens several region groups into one normalized, merged list.
pub fn combine_regions(groups: &[&[Region]]) -> Vec<Region> {
    let mut combined = Vec::new();
    for group in groups {
        for region in *group {
            combined = merge_regions(&combined, *region);
        }
    }
    combined
}

pub fn region_is_covered(regions: &[Region], target: Region) -> bool {
    if target.end <= target.start {
        return false;
    }
    combine_regions(&[regions])
        .iter()
        .any(|region| region.start <= target.start && region.end >= target.end)
}

/// Removes `subtract` from every region, splitting ranges where necessary.
pub fn subtract_region(regions: &[Region], subtract: Region) -> Vec<Region> {
    let mut result = Vec::with_capacity(regions.len() + 1);
    for region in regions {
        if subtract.end <= region.start || subtract.start >= region.end {
            result.push(*region);
            continue;
        }
        if region.start < subtract.start {
            result.push(Region::new(region.start, subtract.start));
        }
        if region.end > subtract.end {
            result.push(Region::new(subtract.end, region.end));
        }
    }
    result
}

/// Smoothstep gain at `time`, holding unity outside the user's keyframes.
pub fn envelope_gain_at_time(points: &[EnvelopePoint], duration: f64, time: f64) -> f64 {
    if points.is_empty() || duration <= 0.0 {
        return 1.0;
    }
    let mut anchors: Vec<(f64, f64)> = points
        .iter()
        .filter(|point| point.time.is_finite() && point.gain.is_finite())
        .map(|point| (point.time.clamp(0.0, duration), point.gain.clamp(0.0, 2.0)))
        .collect();
    if anchors.is_empty() {
        return 1.0;
    }
    anchors.sort_by(|left, right| left.0.total_cmp(&right.0));
    if anchors[0].0 > 0.0 {
        anchors.insert(0, (0.0, 1.0));
    }
    if anchors[anchors.len() - 1].0 < duration {
        anchors.push((duration, 1.0));
    }

    for index in 1..anchors.len() {
        let left = anchors[index - 1];
        let right = anchors[index];
        if time > right.0 && index < anchors.len() - 1 {
            continue;
        }
        if right.0 <= left.0 {
            return right.1;
        }
        let progress = ((time - left.0) / (right.0 - left.0)).clamp(0.0, 1.0);
        let smooth = progress * progress * (3.0 - 2.0 * progress);
        return left.1 + (right.1 - left.1) * smooth;
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::{
        combine_regions, envelope_gain_at_time, merge_regions, region_is_covered, subtract_region,
        EnvelopePoint, Region,
    };

    #[test]
    fn detects_whether_a_selected_range_is_already_fully_muted() {
        let regions = [Region::new(1.0, 2.0), Region::new(2.0, 3.0)];
        assert!(region_is_covered(&regions, Region::new(1.5, 2.5)));
        assert!(!region_is_covered(&regions, Region::new(0.5, 2.5)));
    }

    #[test]
    fn uses_explicit_edge_keyframes_when_they_exist() {
        let points = vec![
            EnvelopePoint::new("start", 0.0, 0.0),
            EnvelopePoint::new("rise", 2.0, 1.5),
            EnvelopePoint::new("hold", 10.0, 1.5),
            EnvelopePoint::new("end", 12.0, 0.0),
        ];
        assert_eq!(envelope_gain_at_time(&points, 12.0, 0.0), 0.0);
        assert_eq!(envelope_gain_at_time(&points, 12.0, 2.0), 1.5);
        assert_eq!(envelope_gain_at_time(&points, 12.0, 12.0), 0.0);
    }

    #[test]
    fn keeps_unity_gain_outside_user_keyframes_when_no_edge_point_exists() {
        let points = vec![EnvelopePoint::new("keyframe", 5.0, 0.5)];
        assert_eq!(envelope_gain_at_time(&points, 10.0, 0.0), 1.0);
        assert_eq!(envelope_gain_at_time(&points, 10.0, 10.0), 1.0);
    }

    #[test]
    fn merges_overlapping_and_adjacent_regions() {
        let merged = merge_regions(
            &[Region::new(1.0, 2.0), Region::new(4.0, 6.0)],
            Region::new(1.5, 3.0),
        );
        assert_eq!(
            merged,
            vec![Region::new(1.0, 3.0), Region::new(4.0, 6.0)]
        );
        assert_eq!(
            merge_regions(&[Region::new(1.0, 2.0)], Region::new(2.0, 5.0)),
            vec![Region::new(1.0, 5.0)]
        );
    }

    #[test]
    fn combines_manual_and_detected_regions() {
        let combined = combine_regions(&[
            &[Region::new(1.0, 2.0)],
            &[Region::new(1.5, 3.0), Region::new(8.0, 9.0)],
        ]);
        assert_eq!(
            combined,
            vec![Region::new(1.0, 3.0), Region::new(8.0, 9.0)]
        );
    }

    #[test]
    fn subtracts_from_the_middle_and_the_edges() {
        let regions = [Region::new(1.0, 5.0)];
        assert_eq!(
            subtract_region(&regions, Region::new(2.0, 3.0)),
            vec![Region::new(1.0, 2.0), Region::new(3.0, 5.0)]
        );
        assert_eq!(
            subtract_region(&regions, Region::new(0.0, 2.0)),
            vec![Region::new(2.0, 5.0)]
        );
        assert_eq!(
            subtract_region(&regions, Region::new(4.5, 9.0)),
            vec![Region::new(1.0, 4.5)]
        );
    }
}
