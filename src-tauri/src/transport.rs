use std::time::Instant;

use serde::Serialize;

use crate::tempo::TempoMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TimelineTransportStatus {
    Paused,
    Playing,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineTransportSnapshot {
    status: TimelineTransportStatus,
    position_beat: f64,
    meter_left: f32,
    meter_right: f32,
    meter_overload: bool,
}

impl TimelineTransportSnapshot {
    pub fn with_meter(mut self, left: f32, right: f32, overload: bool) -> Self {
        self.meter_left = left.clamp(0.0, 1.0);
        self.meter_right = right.clamp(0.0, 1.0);
        self.meter_overload = overload;
        self
    }
}

#[derive(Debug)]
pub struct TimelineTransport {
    base_position_beat: f64,
    started_at: Option<Instant>,
}

impl Default for TimelineTransport {
    fn default() -> Self {
        Self {
            base_position_beat: 0.0,
            started_at: None,
        }
    }
}

impl TimelineTransport {
    pub fn position_beat(&mut self, tempo_map: &TempoMap, end_beat: f64) -> Result<f64, String> {
        Ok(self.snapshot(tempo_map, end_beat)?.position_beat)
    }

    pub fn synchronize_audio(
        &mut self,
        position_beat: f64,
        playing: bool,
        end_beat: f64,
    ) -> Result<TimelineTransportSnapshot, String> {
        if !position_beat.is_finite() {
            return Err("The timeline audio position is not valid.".to_owned());
        }
        self.base_position_beat = position_beat.clamp(0.0, end_beat.max(0.0));
        self.started_at = playing.then(Instant::now);
        Ok(self.as_snapshot())
    }

    pub fn snapshot(
        &mut self,
        tempo_map: &TempoMap,
        end_beat: f64,
    ) -> Result<TimelineTransportSnapshot, String> {
        self.snapshot_at(tempo_map, end_beat, Instant::now())
    }

    pub fn pause(
        &mut self,
        tempo_map: &TempoMap,
        end_beat: f64,
    ) -> Result<TimelineTransportSnapshot, String> {
        let now = Instant::now();
        self.base_position_beat = self
            .position_at(now, tempo_map)
            .clamp(0.0, end_beat.max(0.0));
        self.started_at = None;
        Ok(self.as_snapshot())
    }

    pub fn seek(
        &mut self,
        position_beat: f64,
        end_beat: f64,
    ) -> Result<TimelineTransportSnapshot, String> {
        if !position_beat.is_finite() {
            return Err("The playhead position is not valid.".to_owned());
        }
        let now = Instant::now();
        self.base_position_beat = position_beat.clamp(0.0, end_beat.max(0.0));
        if self.started_at.is_some() {
            self.started_at = Some(now);
        }
        Ok(self.as_snapshot())
    }

    fn snapshot_at(
        &mut self,
        tempo_map: &TempoMap,
        end_beat: f64,
        now: Instant,
    ) -> Result<TimelineTransportSnapshot, String> {
        let position_beat = self
            .position_at(now, tempo_map)
            .clamp(0.0, end_beat.max(0.0));
        if self.started_at.is_some() && position_beat >= end_beat.max(0.0) {
            self.base_position_beat = position_beat;
            self.started_at = None;
        }

        Ok(TimelineTransportSnapshot {
            status: self.status(),
            position_beat,
            meter_left: 0.0,
            meter_right: 0.0,
            meter_overload: false,
        })
    }

    fn position_at(&self, now: Instant, tempo_map: &TempoMap) -> f64 {
        self.started_at
            .map_or(self.base_position_beat, |started_at| {
                let start_seconds = tempo_map.seconds_at_beat(self.base_position_beat);
                tempo_map.beat_at_seconds(
                    start_seconds + now.saturating_duration_since(started_at).as_secs_f64(),
                )
            })
    }

    fn status(&self) -> TimelineTransportStatus {
        if self.started_at.is_some() {
            TimelineTransportStatus::Playing
        } else {
            TimelineTransportStatus::Paused
        }
    }

    fn as_snapshot(&self) -> TimelineTransportSnapshot {
        TimelineTransportSnapshot {
            status: self.status(),
            position_beat: self.base_position_beat,
            meter_left: 0.0,
            meter_right: 0.0,
            meter_overload: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TimelineTransport, TimelineTransportStatus};
    use crate::tempo::{TempoMap, TempoPoint};
    use std::time::{Duration, Instant};

    fn ramp() -> TempoMap {
        TempoMap::new(120.0, vec![TempoPoint::clip_target(16.0, 128.0, 1)])
            .expect("valid tempo map")
    }

    #[test]
    fn transport_pauses_automatically_at_project_end() {
        let start = Instant::now();
        let mut transport = TimelineTransport {
            base_position_beat: 7.0,
            started_at: Some(start),
        };

        let snapshot = transport
            .snapshot_at(&ramp(), 8.0, start + Duration::from_secs(1))
            .expect("snapshot should succeed");

        assert_eq!(snapshot.status, TimelineTransportStatus::Paused);
        assert!((snapshot.position_beat - 8.0).abs() < 1e-9);
    }

    #[test]
    fn audio_position_replaces_the_estimated_clock() {
        let mut transport = TimelineTransport::default();
        let snapshot = transport
            .synchronize_audio(12.5, true, 64.0)
            .expect("audio synchronization should succeed");

        assert_eq!(snapshot.status, TimelineTransportStatus::Playing);
        assert!((snapshot.position_beat - 12.5).abs() < 1e-9);
    }

    #[test]
    fn estimated_clock_follows_the_tempo_ramp() {
        let start = Instant::now();
        let tempo_map = ramp();
        let mut transport = TimelineTransport {
            base_position_beat: 0.0,
            started_at: Some(start),
        };
        let elapsed = tempo_map.seconds_at_beat(8.0);

        let snapshot = transport
            .snapshot_at(&tempo_map, 64.0, start + Duration::from_secs_f64(elapsed))
            .expect("snapshot should succeed");

        assert!((snapshot.position_beat - 8.0).abs() < 1.0e-8);
    }
}
