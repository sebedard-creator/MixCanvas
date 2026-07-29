use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

const MIN_BPM: f64 = 40.0;
const MAX_BPM: f64 = 300.0;
const EPSILON: f64 = 1.0e-9;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoPoint {
    pub beat: f64,
    pub bpm: f64,
    pub clip_id: Option<i64>,
}

impl TempoPoint {
    pub fn project_start(bpm: f64) -> Self {
        Self {
            beat: 0.0,
            bpm,
            clip_id: None,
        }
    }

    pub fn clip_target(beat: f64, bpm: f64, clip_id: i64) -> Self {
        Self {
            beat,
            bpm,
            clip_id: Some(clip_id),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    points: Vec<TempoPoint>,
}

impl TempoMap {
    pub fn new(fallback_bpm: f64, mut points: Vec<TempoPoint>) -> Result<Self, String> {
        validate_bpm(fallback_bpm)?;

        if points.iter().any(|point| {
            !point.beat.is_finite() || point.beat < 0.0 || validate_bpm(point.bpm).is_err()
        }) {
            return Err("The tempo map contains an invalid point.".to_owned());
        }

        points.push(TempoPoint::project_start(fallback_bpm));
        points.sort_by(|left, right| {
            left.beat.total_cmp(&right.beat).then_with(|| {
                left.clip_id
                    .unwrap_or(i64::MIN)
                    .cmp(&right.clip_id.unwrap_or(i64::MIN))
            })
        });

        let mut deduplicated: Vec<TempoPoint> = Vec::with_capacity(points.len());
        for point in points {
            if let Some(previous) = deduplicated.last_mut()
                && (previous.beat - point.beat).abs() <= EPSILON
            {
                *previous = point;
                continue;
            }
            deduplicated.push(point);
        }

        Ok(Self {
            points: deduplicated,
        })
    }

    pub fn points(&self) -> &[TempoPoint] {
        &self.points
    }

    pub fn bpm_at_beat(&self, beat: f64) -> f64 {
        let beat = beat.max(0.0);
        let first = self.points[0];
        if beat <= first.beat {
            return first.bpm;
        }

        for window in self.points.windows(2) {
            let start = window[0];
            let end = window[1];
            if beat <= end.beat {
                let progress = (beat - start.beat) / (end.beat - start.beat);
                return start.bpm + (end.bpm - start.bpm) * progress;
            }
        }

        self.points.last().map_or(first.bpm, |point| point.bpm)
    }

    pub fn seconds_at_beat(&self, beat: f64) -> f64 {
        let beat = beat.max(0.0);
        let mut elapsed_seconds = 0.0;

        for window in self.points.windows(2) {
            let start = window[0];
            let end = window[1];
            if beat <= start.beat {
                return elapsed_seconds;
            }

            let segment_end = beat.min(end.beat);
            elapsed_seconds +=
                linear_segment_seconds(start.beat, start.bpm, end.beat, end.bpm, segment_end);

            if beat <= end.beat {
                return elapsed_seconds;
            }
        }

        let last = *self
            .points
            .last()
            .expect("a tempo map always holds at least one point");
        elapsed_seconds + ((beat - last.beat).max(0.0) * 60.0 / last.bpm)
    }

    pub fn beat_at_seconds(&self, seconds: f64) -> f64 {
        let mut remaining_seconds = seconds.max(0.0);

        for window in self.points.windows(2) {
            let start = window[0];
            let end = window[1];
            let segment_seconds =
                linear_segment_seconds(start.beat, start.bpm, end.beat, end.bpm, end.beat);

            if remaining_seconds <= segment_seconds {
                return inverse_linear_segment_beat(start, end, remaining_seconds);
            }
            remaining_seconds -= segment_seconds;
        }

        let last = *self
            .points
            .last()
            .expect("a tempo map always holds at least one point");
        last.beat + remaining_seconds * last.bpm / 60.0
    }

    pub fn bpm_extrema_between(&self, start_beat: f64, end_beat: f64) -> (f64, f64) {
        let start_beat = start_beat.max(0.0);
        let end_beat = end_beat.max(start_beat);
        let mut minimum = self.bpm_at_beat(start_beat);
        let mut maximum = minimum;

        let end_bpm = self.bpm_at_beat(end_beat);
        minimum = minimum.min(end_bpm);
        maximum = maximum.max(end_bpm);

        for point in &self.points {
            if point.beat > start_beat && point.beat < end_beat {
                minimum = minimum.min(point.bpm);
                maximum = maximum.max(point.bpm);
            }
        }

        (minimum, maximum)
    }

    pub fn signature(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for point in &self.points {
            point.beat.to_bits().hash(&mut hasher);
            point.bpm.to_bits().hash(&mut hasher);
            point.clip_id.hash(&mut hasher);
        }
        hasher.finish()
    }
}

fn validate_bpm(bpm: f64) -> Result<(), String> {
    if bpm.is_finite() && (MIN_BPM..=MAX_BPM).contains(&bpm) {
        Ok(())
    } else {
        Err(format!(
            "The BPM has to be between {MIN_BPM:.0} and {MAX_BPM:.0}."
        ))
    }
}

fn linear_segment_seconds(
    start_beat: f64,
    start_bpm: f64,
    end_beat: f64,
    end_bpm: f64,
    target_beat: f64,
) -> f64 {
    let beat_delta = (target_beat - start_beat).clamp(0.0, end_beat - start_beat);
    let slope = (end_bpm - start_bpm) / (end_beat - start_beat);
    if slope.abs() <= EPSILON {
        beat_delta * 60.0 / start_bpm
    } else {
        let target_bpm = start_bpm + slope * beat_delta;
        60.0 / slope * (target_bpm / start_bpm).ln()
    }
}

fn inverse_linear_segment_beat(start: TempoPoint, end: TempoPoint, seconds: f64) -> f64 {
    let slope = (end.bpm - start.bpm) / (end.beat - start.beat);
    if slope.abs() <= EPSILON {
        start.beat + seconds * start.bpm / 60.0
    } else {
        start.beat + start.bpm / slope * ((seconds * slope / 60.0).exp() - 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> TempoMap {
        TempoMap::new(
            120.0,
            vec![
                TempoPoint::clip_target(0.0, 120.0, 1),
                TempoPoint::clip_target(16.0, 128.0, 2),
            ],
        )
        .expect("valid tempo map")
    }

    #[test]
    fn tempo_is_linearly_interpolated_in_beat_space() {
        let map = ramp();
        assert!((map.bpm_at_beat(8.0) - 124.0).abs() < 1.0e-9);
        assert!((map.bpm_at_beat(20.0) - 128.0).abs() < 1.0e-9);
    }

    #[test]
    fn beat_and_seconds_round_trip_across_a_ramp() {
        let map = ramp();
        for beat in [0.0, 2.0, 8.0, 15.5, 16.0, 24.0, 64.0] {
            let reconstructed = map.beat_at_seconds(map.seconds_at_beat(beat));
            assert!((reconstructed - beat).abs() < 1.0e-8, "beat {beat}");
        }
    }

    #[test]
    fn latest_clip_wins_when_targets_share_an_anchor() {
        let map = TempoMap::new(
            120.0,
            vec![
                TempoPoint::clip_target(8.0, 124.0, 10),
                TempoPoint::clip_target(8.0, 126.0, 12),
                TempoPoint::clip_target(8.0, 125.0, 11),
            ],
        )
        .expect("valid tempo map");

        assert_eq!(map.points().len(), 2);
        assert!((map.bpm_at_beat(8.0) - 126.0).abs() < 1.0e-9);
    }

    #[test]
    fn extrema_include_targets_inside_the_requested_range() {
        let map = TempoMap::new(
            120.0,
            vec![
                TempoPoint::clip_target(8.0, 130.0, 1),
                TempoPoint::clip_target(16.0, 110.0, 2),
            ],
        )
        .expect("valid tempo map");

        assert_eq!(map.bpm_extrema_between(4.0, 20.0), (110.0, 130.0));
    }
}
