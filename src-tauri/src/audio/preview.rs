use std::{fs::File, path::PathBuf, time::Duration};

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, cpal::BufferSize};
use serde::Serialize;

use super::metadata::{metadata_from_decoder, open_mp3_decoder};

const PREVIEW_OUTPUT_BUFFER_FRAMES: u32 = 4_096;

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
    sample_rate: Option<u32>,
    channels: Option<u16>,
}

#[derive(Default)]
pub struct PreviewEngine {
    output: Option<MixerDeviceSink>,
    player: Option<Player>,
    track: Option<LoadedTrack>,
    stored_position: Duration,
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

        self.ensure_output()?;
        let player = self.player_ref()?;
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
                    .try_seek(self.stored_position)
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
        self.stored_position = self.player_ref()?.get_pos();
        Ok(self.snapshot())
    }

    pub fn release_output(&mut self) -> PreviewSnapshot {
        if let Some(player) = &self.player {
            self.stored_position = self.track.as_ref().map_or(Duration::ZERO, |track| {
                if player.empty() {
                    track.duration
                } else {
                    player.get_pos().min(track.duration)
                }
            });
            player.stop();
        }
        self.player = None;
        self.output = None;
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
            .try_seek(target)
            .map_err(|error| format!("Could not seek within the MP3: {error}"))?;

        if was_playing {
            self.player_ref()?.play();
        } else {
            self.player_ref()?.pause();
        }
        self.stored_position = target;

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
                sample_rate: None,
                channels: None,
            };
        };

        let (status, position) = match &self.player {
            Some(player) if player.empty() => (PreviewStatus::Ended, track.duration),
            Some(player) if player.is_paused() => (PreviewStatus::Paused, player.get_pos()),
            Some(player) => (PreviewStatus::Playing, player.get_pos()),
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
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn clamp_seek_position(position_ms: u64, duration: Duration) -> Duration {
    Duration::from_millis(position_ms).min(duration)
}

#[cfg(test)]
mod tests {
    use super::{clamp_seek_position, duration_millis};
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
}
