use std::{fs::File, path::PathBuf, time::Duration};

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, cpal::BufferSize};
use serde::Serialize;

use super::metadata::{metadata_from_decoder, open_mp3_decoder};

const PREVIEW_OUTPUT_BUFFER_FRAMES: u32 = 4_096;
const NORMAL_PREVIEW_SPEED: f32 = 1.0;
const SLOW_PREVIEW_SPEED: f32 = 0.5;
/// Le niveau de l'écoute, en décibels sous l'original.
///
/// Un MP3 masterisé sort à pleine échelle, et l'écoute sert à travailler —
/// taper les temps, chercher un premier temps — pas à juger un mix. Quatre
/// décibels de moins font la différence entre un outil qu'on ouvre sans y
/// penser et un qui fait sursauter.
///
/// Écrit en décibels et converti, plutôt qu'en gain linéaire : « 0,63 » ne se
/// relit pas, et personne ne saurait dire de combien il faut le bouger pour
/// gagner un décibel de plus.
const PREVIEW_GAIN_DB: f32 = -4.0;

fn preview_gain() -> f32 {
    10.0_f32.powf(PREVIEW_GAIN_DB / 20.0)
}

#[derive(Clone, Debug)]
struct LoadedTrack {
    path: PathBuf,
    file_name: String,
    duration: Duration,
    sample_rate: u32,
    channels: u16,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
enum PreviewStatus {
    #[default]
    Empty,
    Paused,
    Playing,
    Ended,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSnapshot {
    status: PreviewStatus,
    file_name: Option<String>,
    file_path: Option<String>,
    duration_ms: u64,
    position_ms: u64,
    playback_speed: f32,
    sample_rate: Option<u32>,
    channels: Option<u16>,
}

pub struct PreviewEngine {
    output: Option<MixerDeviceSink>,
    player: Option<Player>,
    track: Option<LoadedTrack>,
    stored_position: Duration,
    playback_speed: f32,
}

impl Default for PreviewEngine {
    fn default() -> Self {
        Self {
            output: None,
            player: None,
            track: None,
            stored_position: Duration::ZERO,
            playback_speed: NORMAL_PREVIEW_SPEED,
        }
    }
}

impl PreviewEngine {
    pub fn load(&mut self, raw_path: String) -> Result<PreviewSnapshot, String> {
        let path = PathBuf::from(raw_path);
        let decoder = open_mp3_decoder(&path)?;
        let metadata = metadata_from_decoder(&decoder);

        let track = LoadedTrack {
            file_name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled track".to_owned()),
            path,
            duration: metadata.duration,
            sample_rate: metadata.sample_rate,
            channels: metadata.channels,
        };

        self.playback_speed = NORMAL_PREVIEW_SPEED;
        self.ensure_output()?;
        let player = self.player_ref()?;
        player.set_speed(self.playback_speed);
        player.stop();
        player.append(decoder);
        player.pause();
        self.track = Some(track);
        self.stored_position = Duration::ZERO;

        Ok(self.snapshot())
    }

    pub fn play(&mut self) -> Result<PreviewSnapshot, String> {
        if self.track.is_none() {
            return Err("Choose an MP3 file first.".to_owned());
        }

        self.ensure_output()?;

        if self.player_ref()?.empty() {
            self.queue_loaded_track()?;
            let duration = self
                .track
                .as_ref()
                .map_or(Duration::ZERO, |track| track.duration);
            if self.stored_position >= duration {
                self.stored_position = Duration::ZERO;
            }
            if !self.stored_position.is_zero() {
                self.player_ref()?
                    .try_seek(player_position_for_source(
                        self.stored_position,
                        self.playback_speed,
                    ))
                    .map_err(|error| format!("Could not resume the preview: {error}"))?;
            }
        }

        self.player_ref()?.play();
        Ok(self.snapshot())
    }

    pub fn pause(&mut self) -> Result<PreviewSnapshot, String> {
        if self.track.is_none() {
            return Err("No track is loaded.".to_owned());
        }

        self.player_ref()?.pause();
        self.stored_position = self.current_source_position();
        Ok(self.snapshot())
    }

    pub fn release_output(&mut self) -> PreviewSnapshot {
        self.stored_position = self.current_source_position();
        if let Some(player) = &self.player {
            player.stop();
        }
        self.player = None;
        self.output = None;
        self.playback_speed = NORMAL_PREVIEW_SPEED;
        self.snapshot()
    }

    pub fn stop(&mut self) -> Result<PreviewSnapshot, String> {
        if self.track.is_none() {
            return Ok(self.snapshot());
        }

        self.player_ref()?.stop();
        self.queue_loaded_track()?;
        self.player_ref()?.pause();
        self.stored_position = Duration::ZERO;
        Ok(self.snapshot())
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<PreviewSnapshot, String> {
        let duration = self
            .track
            .as_ref()
            .ok_or_else(|| "No track is loaded.".to_owned())?
            .duration;
        let target = clamp_seek_position(position_ms, duration);

        self.ensure_output()?;

        let was_playing = {
            let player = self.player_ref()?;
            !player.empty() && !player.is_paused()
        };

        if self.player_ref()?.empty() {
            self.player_ref()?.pause();
            self.queue_loaded_track()?;
        }

        self.player_ref()?
            .try_seek(player_position_for_source(target, self.playback_speed))
            .map_err(|error| format!("Could not seek within the MP3: {error}"))?;

        if was_playing {
            self.player_ref()?.play();
        } else {
            self.player_ref()?.pause();
        }
        self.stored_position = target;

        Ok(self.snapshot())
    }

    pub fn set_speed(&mut self, speed: f32) -> Result<PreviewSnapshot, String> {
        if !is_supported_preview_speed(speed) {
            return Err("Preview speed must be normal or half speed.".to_owned());
        }

        if (self.playback_speed - speed).abs() < f32::EPSILON {
            return Ok(self.snapshot());
        }

        let source_position = self.current_source_position();
        self.playback_speed = speed;
        if let Some(player) = &self.player {
            player.set_speed(speed);
            if !player.empty() {
                player
                    .try_seek(player_position_for_source(source_position, speed))
                    .map_err(|error| {
                        format!(
                            "Could not preserve the preview position after changing speed: {error}"
                        )
                    })?;
            }
        }
        self.stored_position = source_position;
        Ok(self.snapshot())
    }

    pub fn snapshot(&self) -> PreviewSnapshot {
        let Some(track) = &self.track else {
            return PreviewSnapshot {
                status: PreviewStatus::Empty,
                file_name: None,
                file_path: None,
                duration_ms: 0,
                position_ms: 0,
                playback_speed: self.playback_speed,
                sample_rate: None,
                channels: None,
            };
        };

        let (status, position) = match &self.player {
            Some(player) if player.empty() => (PreviewStatus::Ended, track.duration),
            Some(player) if player.is_paused() => (
                PreviewStatus::Paused,
                source_position_from_player(player.get_pos(), self.playback_speed),
            ),
            Some(player) => (
                PreviewStatus::Playing,
                source_position_from_player(player.get_pos(), self.playback_speed),
            ),
            None => (
                PreviewStatus::Paused,
                self.stored_position.min(track.duration),
            ),
        };

        PreviewSnapshot {
            status,
            file_name: Some(track.file_name.clone()),
            file_path: Some(track.path.to_string_lossy().into_owned()),
            duration_ms: duration_millis(track.duration),
            position_ms: duration_millis(position.min(track.duration)),
            playback_speed: self.playback_speed,
            sample_rate: Some(track.sample_rate),
            channels: Some(track.channels),
        }
    }

    fn ensure_output(&mut self) -> Result<(), String> {
        if self.output.is_some() && self.player.is_some() {
            return Ok(());
        }

        let output = DeviceSinkBuilder::from_default_device()
            .and_then(|builder| {
                builder
                    .with_buffer_size(BufferSize::Fixed(PREVIEW_OUTPUT_BUFFER_FRAMES))
                    .open_stream()
            })
            .or_else(|_| DeviceSinkBuilder::open_default_sink())
            .map_err(|error| format!("Could not open the default audio output: {error}"))?;
        let player = Player::connect_new(output.mixer());
        player.set_speed(self.playback_speed);
        // Posé une fois, à la création : ni `stop` ni `append` n'y touchent,
        // donc le niveau tient d'un morceau à l'autre.
        player.set_volume(preview_gain());

        self.output = Some(output);
        self.player = Some(player);
        Ok(())
    }

    fn queue_loaded_track(&self) -> Result<(), String> {
        let track = self
            .track
            .as_ref()
            .ok_or_else(|| "No track is loaded.".to_owned())?;
        let file = File::open(&track.path)
            .map_err(|error| format!("Could not reopen the MP3: {error}"))?;
        let decoder = Decoder::try_from(file)
            .map_err(|error| format!("Could not re-decode the MP3: {error}"))?;

        self.player_ref()?.append(decoder);
        Ok(())
    }

    fn player_ref(&self) -> Result<&Player, String> {
        self.player
            .as_ref()
            .ok_or_else(|| "The audio output is not running.".to_owned())
    }

    fn current_source_position(&self) -> Duration {
        let Some(track) = &self.track else {
            return Duration::ZERO;
        };

        match &self.player {
            Some(player) if player.empty() => track.duration,
            Some(player) => source_position_from_player(player.get_pos(), self.playback_speed)
                .min(track.duration),
            None => self.stored_position.min(track.duration),
        }
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn clamp_seek_position(position_ms: u64, duration: Duration) -> Duration {
    Duration::from_millis(position_ms).min(duration)
}

fn source_position_from_player(player_position: Duration, playback_speed: f32) -> Duration {
    player_position.mul_f32(playback_speed)
}

fn player_position_for_source(source_position: Duration, playback_speed: f32) -> Duration {
    source_position.div_f32(playback_speed)
}

fn is_supported_preview_speed(speed: f32) -> bool {
    speed.is_finite()
        && ((speed - NORMAL_PREVIEW_SPEED).abs() < f32::EPSILON
            || (speed - SLOW_PREVIEW_SPEED).abs() < f32::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::{
        PreviewEngine, clamp_seek_position, duration_millis, player_position_for_source,
        source_position_from_player,
    };
    use std::time::Duration;

    #[test]
    fn duration_conversion_is_stable() {
        assert_eq!(duration_millis(Duration::from_millis(12_345)), 12_345);
    }

    #[test]
    fn seek_position_is_clamped_to_track_duration() {
        let duration = Duration::from_secs(180);

        assert_eq!(
            clamp_seek_position(42_500, duration),
            Duration::from_millis(42_500)
        );
        assert_eq!(clamp_seek_position(250_000, duration), duration);
    }

    #[test]
    fn preview_speed_accepts_only_normal_and_half_speed() {
        let mut engine = PreviewEngine::default();

        assert_eq!(
            engine
                .set_speed(0.5)
                .expect("half speed should be accepted")
                .playback_speed,
            0.5
        );
        assert_eq!(
            engine
                .set_speed(1.0)
                .expect("normal speed should be accepted")
                .playback_speed,
            1.0
        );
        assert!(engine.set_speed(0.75).is_err());
        assert!(engine.set_speed(f32::NAN).is_err());
    }

    #[test]
    fn half_speed_positions_are_converted_to_source_time() {
        let source_position = Duration::from_secs(12);
        let player_position = player_position_for_source(source_position, 0.5);

        assert_eq!(player_position, Duration::from_secs(24));
        assert_eq!(
            source_position_from_player(player_position, 0.5),
            source_position
        );
    }
}
