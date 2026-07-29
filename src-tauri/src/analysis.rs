use std::{collections::BTreeMap, path::Path, time::Duration};

use beat_this::{BeatThis, RtenRuntime};
use rodio::Source;

use crate::audio::open_mp3_decoder;

const ENVELOPE_RATE_HZ: f64 = 100.0;
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 190.0;
const MIN_ANALYSIS_SECONDS: f64 = 8.0;
/// Combien la recherche peut s'écarter d'un tempo tapé, en proportion.
///
/// Taper huit fois et prendre la médiane des intervalles place la plupart des
/// gens à deux ou trois pour cent près. Huit pour cent absorbe une main peu
/// sûre sans jamais approcher les voisins en demi-temps et en double-temps,
/// vers lesquels une fenêtre plus large finirait par glisser.
const TEMPO_HINT_TOLERANCE: f64 = 0.08;
/// Corner of the kick-band envelope used to place the downbeat, in hertz.
/// Two poles, so a clap around 2 kHz arrives some 50 dB below a 60 Hz kick.
const KICK_BAND_HZ: f64 = 120.0;
/// Bumped whenever the grid an existing cache would hold is no longer the grid
/// this code produces. `ANALYSIS_ALGORITHM_VERSION` in `src/library/types.ts`
/// carries the same number, and the app re-analyses older caches once.
pub const ANALYSIS_ALGORITHM_VERSION: u32 = 3;
pub const WAVEFORM_BUCKET_COUNT: usize = 16_384;

#[derive(Clone, Debug)]
pub struct BeatModelPaths {
    pub mel: std::path::PathBuf,
    pub beats: std::path::PathBuf,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveformPeaks {
    pub left_min: Vec<f32>,
    pub left_max: Vec<f32>,
    pub left_rms: Vec<f32>,
    pub right_min: Vec<f32>,
    pub right_max: Vec<f32>,
    pub right_rms: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct BeatAnalysis {
    pub bpm: f64,
    pub confidence: f64,
    pub first_beat_ms: u64,
    pub beats_ms: Vec<u64>,
    pub waveform: WaveformPeaks,
}

/// Bornes de recherche du tempo, en BPM.
///
/// Sans indice, toute la plage utile. Avec un indice, une fenêtre étroite
/// autour de lui — mais toujours à l'intérieur des bornes globales, pour qu'un
/// tap aberrant ne fasse pas chercher un tempo que le reste du programme
/// refuserait ensuite.
fn tempo_search_window(hinted_bpm: Option<f64>) -> (f64, f64) {
    match hinted_bpm {
        Some(bpm) if bpm.is_finite() && bpm > 0.0 => {
            let low = (bpm * (1.0 - TEMPO_HINT_TOLERANCE)).max(MIN_BPM);
            let high = (bpm * (1.0 + TEMPO_HINT_TOLERANCE)).min(MAX_BPM);
            if low < high {
                (low, high)
            } else {
                // L'indice tombe hors de la plage utile; on l'ignore plutôt que
                // de chercher dans une fenêtre vide.
                (MIN_BPM, MAX_BPM)
            }
        }
        _ => (MIN_BPM, MAX_BPM),
    }
}

/// Analyse en cherchant le tempo autour d'une valeur tapée à la main.
///
/// L'analyse automatique se trompe surtout de *période*; les attaques qu'elle
/// détecte restent bonnes. Un tap donne la période approximative, la corrélation
/// la précise sur les kicks, et le placement du premier temps suit sans
/// changer — c'est lui qui recale la phase.
pub fn analyze_mp3_near(
    path: &Path,
    hinted_bpm: f64,
    models: &BeatModelPaths,
) -> Result<BeatAnalysis, String> {
    analyze_with_hint(path, Some(hinted_bpm), models)
}

pub fn analyze_mp3(path: &Path, models: &BeatModelPaths) -> Result<BeatAnalysis, String> {
    analyze_with_hint(path, None, models)
}

fn analyze_with_hint(
    path: &Path,
    hinted_bpm: Option<f64>,
    models: &BeatModelPaths,
) -> Result<BeatAnalysis, String> {
    let decoder = open_mp3_decoder(path)?;
    let duration = decoder.total_duration().unwrap_or_default();
    let sample_rate = decoder.sample_rate().get();
    let channels = decoder.channels().get();
    let features = collect_audio_features(
        decoder,
        sample_rate,
        channels,
        duration,
        WAVEFORM_BUCKET_COUNT,
    );
    let onset = onset_envelope(&features.energy);
    let kick_onset = onset_envelope(&features.kick_energy);
    let analyzed_duration = if duration.is_zero() {
        Duration::from_secs_f64(features.energy.len() as f64 / ENVELOPE_RATE_HZ)
    } else {
        duration
    };

    let mut analysis = estimate_model_beat_grid(
        path,
        models,
        &features.kick_energy,
        analyzed_duration,
        hinted_bpm,
    )
    .or_else(|model_error| {
        estimate_beat_grid(
            &onset,
            &kick_onset,
            &features.kick_energy,
            ENVELOPE_RATE_HZ,
            analyzed_duration,
            hinted_bpm,
        )
        .map_err(|legacy_error| {
            format!(
                "The learned beat tracker failed ({model_error}); the fallback analyzer also failed ({legacy_error})."
            )
        })
    })?;
    analysis.waveform = features.waveform;
    Ok(analysis)
}

pub fn analyze_waveform(path: &Path) -> Result<WaveformPeaks, String> {
    let decoder = open_mp3_decoder(path)?;
    let duration = decoder.total_duration().unwrap_or_default();
    let sample_rate = decoder.sample_rate().get();
    let channels = decoder.channels().get();
    Ok(collect_audio_features(
        decoder,
        sample_rate,
        channels,
        duration,
        WAVEFORM_BUCKET_COUNT,
    )
    .waveform)
}

/// Everything one decoding pass produces.
struct AudioFeatures {
    /// Broadband RMS per envelope frame, used to find the tempo.
    energy: Vec<f32>,
    /// Low-band RMS per envelope frame, used to place the downbeat.
    kick_energy: Vec<f32>,
    waveform: WaveformPeaks,
}

/// Two cascaded one-pole low passes, run on the mono sum.
#[derive(Default)]
struct KickBandFilter {
    coefficient: f64,
    first: f64,
    second: f64,
}

impl KickBandFilter {
    fn new(sample_rate: u32) -> Self {
        let dt = 1.0 / f64::from(sample_rate.max(1));
        let rc = 1.0 / (std::f64::consts::TAU * KICK_BAND_HZ);
        Self {
            coefficient: dt / (rc + dt),
            first: 0.0,
            second: 0.0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        self.first += self.coefficient * (input - self.first);
        self.second += self.coefficient * (self.first - self.second);
        self.second
    }
}

fn collect_audio_features(
    decoder: impl Iterator<Item = f32>,
    sample_rate: u32,
    channels: u16,
    duration: Duration,
    waveform_bucket_count: usize,
) -> AudioFeatures {
    let channel_count = usize::from(channels).max(1);
    let samples_per_channel = ((f64::from(sample_rate) / ENVELOPE_RATE_HZ).round() as usize).max(1);
    let samples_per_frame = samples_per_channel.saturating_mul(channel_count);
    let estimated_frames = (duration.as_secs_f64() * f64::from(sample_rate)).ceil() as usize;
    let frames_per_waveform_bucket = if waveform_bucket_count == 0 {
        usize::MAX
    } else if estimated_frames == 0 {
        (sample_rate as usize / 20).max(1)
    } else {
        estimated_frames
            .max(1)
            .div_ceil(waveform_bucket_count)
            .max(1)
    };
    let mut envelope = Vec::new();
    let mut squared_sum = 0.0_f64;
    let mut sample_count = 0_usize;
    let mut frame_channel = 0_usize;
    let mut frame_left = 0.0_f32;
    let mut frame_right = 0.0_f32;
    let mut waveform = WaveformAccumulator::default();

    // The kick band is filtered on the mono sum of a completed frame, never on
    // the interleaved stream, which would mix the two channels into one filter.
    let mut kick_filter = KickBandFilter::new(sample_rate);
    let mut kick_envelope = Vec::new();
    let mut kick_squared_sum = 0.0_f64;
    let mut kick_frame_count = 0_usize;

    for sample in decoder {
        let finite_sample = if sample.is_finite() { sample } else { 0.0 };
        squared_sum += f64::from(finite_sample * finite_sample);
        sample_count += 1;

        if sample_count >= samples_per_frame {
            envelope.push((squared_sum / sample_count as f64).sqrt() as f32);
            squared_sum = 0.0;
            sample_count = 0;
        }

        if frame_channel == 0 {
            frame_left = finite_sample;
            frame_right = finite_sample;
        } else if frame_channel == 1 {
            frame_right = finite_sample;
        }
        frame_channel += 1;

        if frame_channel >= channel_count {
            waveform.push_frame(frame_left, frame_right, frames_per_waveform_bucket);
            frame_channel = 0;

            let mono = (f64::from(frame_left) + f64::from(frame_right)) * 0.5;
            let low = kick_filter.process(mono);
            kick_squared_sum += low * low;
            kick_frame_count += 1;
            if kick_frame_count >= samples_per_channel {
                kick_envelope.push((kick_squared_sum / kick_frame_count as f64).sqrt() as f32);
                kick_squared_sum = 0.0;
                kick_frame_count = 0;
            }
        }
    }

    if sample_count > 0 {
        envelope.push((squared_sum / sample_count as f64).sqrt() as f32);
    }

    if frame_channel > 0 {
        waveform.push_frame(frame_left, frame_right, frames_per_waveform_bucket);
    }

    if kick_frame_count > 0 {
        kick_envelope.push((kick_squared_sum / kick_frame_count as f64).sqrt() as f32);
    }

    AudioFeatures {
        energy: envelope,
        kick_energy: kick_envelope,
        waveform: waveform.finish(waveform_bucket_count),
    }
}

#[derive(Default)]
struct WaveformAccumulator {
    left_min: Vec<f32>,
    left_max: Vec<f32>,
    left_rms: Vec<f32>,
    right_min: Vec<f32>,
    right_max: Vec<f32>,
    right_rms: Vec<f32>,
    bucket_left_min: f32,
    bucket_left_max: f32,
    bucket_left_squared_sum: f64,
    bucket_right_min: f32,
    bucket_right_max: f32,
    bucket_right_squared_sum: f64,
    bucket_frames: usize,
}

impl WaveformAccumulator {
    fn push_frame(&mut self, left: f32, right: f32, frames_per_bucket: usize) {
        if self.bucket_frames == 0 {
            self.bucket_left_min = left;
            self.bucket_left_max = left;
            self.bucket_right_min = right;
            self.bucket_right_max = right;
        } else {
            self.bucket_left_min = self.bucket_left_min.min(left);
            self.bucket_left_max = self.bucket_left_max.max(left);
            self.bucket_right_min = self.bucket_right_min.min(right);
            self.bucket_right_max = self.bucket_right_max.max(right);
        }
        self.bucket_left_squared_sum += f64::from(left) * f64::from(left);
        self.bucket_right_squared_sum += f64::from(right) * f64::from(right);
        self.bucket_frames += 1;

        if self.bucket_frames >= frames_per_bucket {
            self.flush_bucket();
        }
    }

    fn flush_bucket(&mut self) {
        if self.bucket_frames == 0 {
            return;
        }
        self.left_min.push(self.bucket_left_min);
        self.left_max.push(self.bucket_left_max);
        self.left_rms
            .push((self.bucket_left_squared_sum / self.bucket_frames as f64).sqrt() as f32);
        self.right_min.push(self.bucket_right_min);
        self.right_max.push(self.bucket_right_max);
        self.right_rms
            .push((self.bucket_right_squared_sum / self.bucket_frames as f64).sqrt() as f32);
        self.bucket_left_squared_sum = 0.0;
        self.bucket_right_squared_sum = 0.0;
        self.bucket_frames = 0;
    }

    fn finish(mut self, requested_bucket_count: usize) -> WaveformPeaks {
        self.flush_bucket();
        let mut waveform = WaveformPeaks {
            left_min: self.left_min,
            left_max: self.left_max,
            left_rms: self.left_rms,
            right_min: self.right_min,
            right_max: self.right_max,
            right_rms: self.right_rms,
        };
        if waveform.left_min.len() > requested_bucket_count && requested_bucket_count > 0 {
            waveform = reduce_waveform(&waveform, requested_bucket_count);
        }
        normalize_waveform(&mut waveform);
        waveform
    }
}

fn reduce_waveform(source: &WaveformPeaks, bucket_count: usize) -> WaveformPeaks {
    let source_count = source.left_min.len();
    let mut reduced = WaveformPeaks {
        left_min: Vec::with_capacity(bucket_count),
        left_max: Vec::with_capacity(bucket_count),
        left_rms: Vec::with_capacity(bucket_count),
        right_min: Vec::with_capacity(bucket_count),
        right_max: Vec::with_capacity(bucket_count),
        right_rms: Vec::with_capacity(bucket_count),
    };

    for bucket in 0..bucket_count {
        let start = bucket * source_count / bucket_count;
        let end = ((bucket + 1) * source_count / bucket_count).max(start + 1);
        reduced.left_min.push(
            source.left_min[start..end]
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min),
        );
        reduced.left_max.push(
            source.left_max[start..end]
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max),
        );
        reduced.left_rms.push(
            (source.left_rms[start..end]
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                / (end - start) as f32)
                .sqrt(),
        );
        reduced.right_min.push(
            source.right_min[start..end]
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min),
        );
        reduced.right_max.push(
            source.right_max[start..end]
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max),
        );
        reduced.right_rms.push(
            (source.right_rms[start..end]
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                / (end - start) as f32)
                .sqrt(),
        );
    }

    reduced
}

fn normalize_waveform(waveform: &mut WaveformPeaks) {
    let peak = waveform
        .left_min
        .iter()
        .chain(&waveform.left_max)
        .chain(&waveform.right_min)
        .chain(&waveform.right_max)
        .copied()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);

    if peak > f32::EPSILON {
        for value in waveform
            .left_min
            .iter_mut()
            .chain(&mut waveform.left_max)
            .chain(&mut waveform.left_rms)
            .chain(&mut waveform.right_min)
            .chain(&mut waveform.right_max)
            .chain(&mut waveform.right_rms)
        {
            *value /= peak;
        }
    }
}

fn onset_envelope(energy: &[f32]) -> Vec<f32> {
    if energy.is_empty() {
        return Vec::new();
    }

    let log_energy = energy
        .iter()
        .map(|value| (f64::from(*value) + 1.0e-7).ln())
        .collect::<Vec<_>>();
    let mut positive_difference = vec![0.0_f64; energy.len()];

    for index in 1..log_energy.len() {
        positive_difference[index] = (log_energy[index] - log_energy[index - 1]).max(0.0);
    }

    let threshold_window = ENVELOPE_RATE_HZ.round() as usize;
    let mut onset = vec![0.0_f32; energy.len()];
    let mut recent_sum = 0.0_f64;

    for index in 0..positive_difference.len() {
        let window_start = index.saturating_sub(threshold_window);
        if index > 0 {
            recent_sum += positive_difference[index - 1];
        }
        if window_start > 0 {
            recent_sum -= positive_difference[window_start - 1];
        }

        let populated = index.min(threshold_window).max(1);
        let adaptive_threshold = recent_sum / populated as f64 * 1.35;
        onset[index] = (positive_difference[index] - adaptive_threshold).max(0.0) as f32;
    }

    let peak = onset.iter().copied().fold(0.0_f32, f32::max);
    if peak > 0.0 {
        for value in &mut onset {
            *value /= peak;
        }
    }

    onset
}

#[derive(Clone, Copy, Debug)]
struct RigidGrid {
    period_seconds: f64,
    origin_seconds: f64,
    matched_events: usize,
    residual_rms_seconds: f64,
}

/// Fits a constant DJ grid to imperfect beat observations.
///
/// Beat trackers deliberately report musical events, not a transport clock:
/// an event can be absent in an intro, duplicated by a subdivision, or missed
/// under a breakdown. Treating the event's array index as the beat number makes
/// one such mistake shift every beat that follows. Here each observation gets
/// its own nearest integer grid index; missing and extra events therefore stay
/// local, while the period is measured over the whole track.
fn fit_rigid_grid(observations: &[f32], seed_bpm: f64) -> Result<RigidGrid, String> {
    let times = observations
        .iter()
        .copied()
        .map(f64::from)
        .filter(|time| time.is_finite() && *time >= 0.0)
        .collect::<Vec<_>>();
    if times.len() < 16 || !seed_bpm.is_finite() || seed_bpm <= 0.0 {
        return Err("The beat tracker did not return enough usable beat evidence.".to_owned());
    }

    let seed_period = 60.0 / seed_bpm;
    let mut period_votes = Vec::new();
    for (left_index, left) in times.iter().enumerate() {
        for right in times.iter().skip(left_index + 1) {
            let span = right - left;
            if span < 2.0 {
                continue;
            }
            if span > 16.0 {
                break;
            }
            let beat_span = (span / seed_period).round();
            if beat_span < 4.0 {
                continue;
            }
            let period = span / beat_span;
            if ((period / seed_period) - 1.0).abs() <= TEMPO_HINT_TOLERANCE {
                period_votes.push((period, beat_span.min(32.0)));
            }
        }
    }
    let mut period = weighted_median(&mut period_votes)
        .ok_or_else(|| "The detected beats do not support a stable tempo.".to_owned())?;

    // Circular kernel density over t mod period. A 1 ms grid is finer than the
    // source MP3 timing and avoids making the later regression inherit a
    // coarse 10 ms envelope frame.
    const PHASE_STEP_SECONDS: f64 = 0.001;
    const PHASE_SIGMA_SECONDS: f64 = 0.018;
    let phase_bins = (period / PHASE_STEP_SECONDS).round().max(1.0) as usize;
    let phase_radius = (3.0 * PHASE_SIGMA_SECONDS / PHASE_STEP_SECONDS).round() as isize;
    let mut phase_density = vec![0.0_f64; phase_bins];
    for time in &times {
        let center = ((time.rem_euclid(period) / PHASE_STEP_SECONDS).round() as usize) % phase_bins;
        for distance in -phase_radius..=phase_radius {
            let bin = (center as isize + distance).rem_euclid(phase_bins as isize) as usize;
            let seconds = distance as f64 * PHASE_STEP_SECONDS;
            phase_density[bin] += (-0.5 * (seconds / PHASE_SIGMA_SECONDS).powi(2)).exp();
        }
    }
    let best_phase_bin = phase_density
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(index, _)| index)
        .unwrap_or_default();
    let mut origin = best_phase_bin as f64 * PHASE_STEP_SECONDS;
    let mut final_points = Vec::new();

    // Reassign and refit until period and phase stop moving. For every integer
    // beat only the closest observation is retained, so a duplicated onset
    // cannot pull the regression twice.
    for _ in 0..8 {
        let mut closest: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
        for time in &times {
            let beat = ((time - origin) / period).round() as i64;
            let error = (time - (origin + beat as f64 * period)).abs();
            if error > 0.080 {
                continue;
            }
            let entry = closest.entry(beat).or_insert((error, *time));
            if error < entry.0 {
                *entry = (error, *time);
            }
        }
        let mut points = closest
            .into_iter()
            .map(|(beat, (_, time))| (beat, time))
            .collect::<Vec<_>>();
        if points.len() < 16 {
            return Err("Too few detected beats agree with one rigid grid.".to_owned());
        }

        let mut fitted = None;
        for cutoff in [0.050_f64, 0.030, 0.020] {
            let Some((next_period, next_origin)) = linear_grid_fit(&points) else {
                break;
            };
            fitted = Some((next_period, next_origin));
            let retained = points
                .iter()
                .copied()
                .filter(|(beat, time)| {
                    (time - (next_origin + *beat as f64 * next_period)).abs() <= cutoff
                })
                .collect::<Vec<_>>();
            if retained.len() >= 16 && retained.len() * 2 >= points.len() {
                points = retained;
            }
        }
        let Some((next_period, next_origin)) = fitted else {
            return Err("The rigid beat grid could not be fitted.".to_owned());
        };
        let movement = (next_period - period).abs();
        period = next_period;
        origin = next_origin;
        final_points = points;
        if movement < 1.0e-10 {
            break;
        }
    }

    if !(60.0 / MAX_BPM..=60.0 / MIN_BPM).contains(&period) {
        return Err("The fitted tempo falls outside the supported BPM range.".to_owned());
    }
    let residual_rms_seconds = (final_points
        .iter()
        .map(|(beat, time)| (time - (origin + *beat as f64 * period)).powi(2))
        .sum::<f64>()
        / final_points.len().max(1) as f64)
        .sqrt();

    Ok(RigidGrid {
        period_seconds: period,
        origin_seconds: origin,
        matched_events: final_points.len(),
        residual_rms_seconds,
    })
}

fn weighted_median(values: &mut [(f64, f64)]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|left, right| left.0.total_cmp(&right.0));
    let total_weight = values.iter().map(|(_, weight)| *weight).sum::<f64>();
    let mut accumulated = 0.0;
    for (value, weight) in values.iter() {
        accumulated += *weight;
        if accumulated >= total_weight * 0.5 {
            return Some(*value);
        }
    }
    values.last().map(|(value, _)| *value)
}

fn linear_grid_fit(points: &[(i64, f64)]) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let count = points.len() as f64;
    let sum_beats = points.iter().map(|(beat, _)| *beat as f64).sum::<f64>();
    let sum_times = points.iter().map(|(_, time)| *time).sum::<f64>();
    let sum_square_beats = points
        .iter()
        .map(|(beat, _)| (*beat as f64).powi(2))
        .sum::<f64>();
    let sum_products = points
        .iter()
        .map(|(beat, time)| *beat as f64 * time)
        .sum::<f64>();
    let denominator = count * sum_square_beats - sum_beats.powi(2);
    if denominator.abs() <= f64::EPSILON {
        return None;
    }
    let period = (count * sum_products - sum_beats * sum_times) / denominator;
    let origin = (sum_times - period * sum_beats) / count;
    (period.is_finite() && period > 0.0 && origin.is_finite()).then_some((period, origin))
}

fn model_downbeat_phase(downbeats: &[f32], grid: RigidGrid) -> (usize, f64) {
    let mut scores = [0.0_f64; BEATS_PER_MEASURE];
    for downbeat in downbeats.iter().copied().map(f64::from) {
        if !downbeat.is_finite() {
            continue;
        }
        let beat = ((downbeat - grid.origin_seconds) / grid.period_seconds).round() as i64;
        let error = (downbeat - (grid.origin_seconds + beat as f64 * grid.period_seconds)).abs();
        let weight = (-0.5 * (error / 0.025).powi(2)).exp();
        scores[beat.rem_euclid(BEATS_PER_MEASURE as i64) as usize] += weight;
    }
    let (winner, best_score) = scores
        .iter()
        .copied()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .unwrap_or((0, 0.0));
    let second_score = scores
        .iter()
        .copied()
        .enumerate()
        .filter(|(phase, _)| *phase != winner)
        .map(|(_, score)| score)
        .fold(0.0_f64, f64::max);
    let separation = if best_score > 0.0 {
        ((best_score - second_score) / best_score).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (winner, separation)
}

fn first_model_downbeat(kick_energy: &[f32], grid: RigidGrid, downbeat_phase: usize) -> f64 {
    let origin_frame = grid.origin_seconds * ENVELOPE_RATE_HZ;
    let period_frames = grid.period_seconds * ENVELOPE_RATE_HZ;
    let groove_seconds = first_grooving_beat(kick_energy, origin_frame, period_frames)
        .map(|frame| frame / ENVELOPE_RATE_HZ)
        .unwrap_or_else(|| grid.origin_seconds.max(0.0));
    let groove_beat = ((groove_seconds - grid.origin_seconds) / grid.period_seconds).round() as i64;
    // The low-band detector can cross its threshold on beat 2 even though the
    // kick entered on beat 1 immediately before it. Always advancing to the
    // next bar then skipped a complete musical measure. Choose the nearest
    // beat carrying the model's bar phase; ties prefer the preceding downbeat.
    let downbeat_beat = nearest_beat_with_phase(groove_beat, downbeat_phase);
    let mut first = grid.origin_seconds + downbeat_beat as f64 * grid.period_seconds;
    while first < 0.0 {
        first += grid.period_seconds * BEATS_PER_MEASURE as f64;
    }
    first
}

fn nearest_beat_with_phase(beat: i64, phase: usize) -> i64 {
    let beats_per_measure = BEATS_PER_MEASURE as i64;
    let lower = beat - (beat - phase as i64).rem_euclid(beats_per_measure);
    let upper = lower + beats_per_measure;
    if beat - lower <= upper - beat {
        lower
    } else {
        upper
    }
}

fn estimate_model_beat_grid(
    path: &Path,
    models: &BeatModelPaths,
    kick_energy: &[f32],
    duration: Duration,
    hinted_bpm: Option<f64>,
) -> Result<BeatAnalysis, String> {
    let mut tracker = BeatThis::new(&RtenRuntime, &models.mel, &models.beats)
        .map_err(|error| format!("the beat models could not be loaded: {error}"))?;
    let detected = tracker
        .analyze_file(path)
        .map_err(|error| format!("the learned beat analysis failed: {error}"))?;
    let detected_bpm = beat_this::calculate_bpm(&detected)
        .map(f64::from)
        .ok_or_else(|| "the learned tracker found too few beats".to_owned())?;
    let seed_bpm = hinted_bpm
        .filter(|bpm| bpm.is_finite() && (MIN_BPM..=MAX_BPM).contains(bpm))
        .unwrap_or(detected_bpm);
    let grid = fit_rigid_grid(&detected.beats, seed_bpm)?;
    let bpm = 60.0 / grid.period_seconds;
    let (downbeat_phase, phase_confidence) = model_downbeat_phase(&detected.downbeats, grid);
    let first_downbeat = first_model_downbeat(kick_energy, grid, downbeat_phase);
    let duration_seconds = duration.as_secs_f64();
    let beat_frames = uniform_beat_frames(first_downbeat, grid.period_seconds, duration_seconds);
    let beats_ms = beat_frames
        .into_iter()
        .map(seconds_to_millis)
        .collect::<Vec<_>>();

    let observation_precision = grid.matched_events as f64 / detected.beats.len().max(1) as f64;
    let expected_beats = duration_seconds / grid.period_seconds;
    let grid_coverage = grid.matched_events as f64 / expected_beats.max(1.0);
    let residual_quality = (1.0 - grid.residual_rms_seconds / 0.040).clamp(0.0, 1.0);
    let confidence = (observation_precision.clamp(0.0, 1.0) * 0.35
        + grid_coverage.clamp(0.0, 1.0) * 0.25
        + residual_quality * 0.30
        + phase_confidence * 0.10)
        .clamp(0.0, 1.0);

    Ok(BeatAnalysis {
        // A hundredth of a BPM can accumulate into an audible offset over a
        // long mix. Keep a millibeat resolution in storage and presentation.
        bpm: (bpm * 1_000.0).round() / 1_000.0,
        confidence: (confidence * 1_000.0).round() / 1_000.0,
        first_beat_ms: seconds_to_millis(first_downbeat),
        beats_ms,
        waveform: WaveformPeaks {
            left_min: Vec::new(),
            left_max: Vec::new(),
            left_rms: Vec::new(),
            right_min: Vec::new(),
            right_max: Vec::new(),
            right_rms: Vec::new(),
        },
    })
}

fn estimate_beat_grid(
    onset: &[f32],
    kick_onset: &[f32],
    kick_energy: &[f32],
    envelope_rate_hz: f64,
    duration: Duration,
    hinted_bpm: Option<f64>,
) -> Result<BeatAnalysis, String> {
    if onset.len() as f64 / envelope_rate_hz < MIN_ANALYSIS_SECONDS {
        return Err("This track is too short for a reliable BPM analysis.".to_owned());
    }

    let onset_peak = onset.iter().copied().fold(0.0_f32, f32::max);
    if onset_peak < 1.0e-4 {
        return Err("No clear enough pulse was found in this track.".to_owned());
    }

    // Un tempo tapé ne dit pas quelle est la période, il dit où la chercher.
    // La corrélation tranche ensuite sur les kicks réels, ce qui corrige
    // l'imprécision de la main sans jamais s'en éloigner assez pour attraper
    // une autre pulsation.
    let (search_min_bpm, search_max_bpm) = tempo_search_window(hinted_bpm);
    let minimum_lag = (60.0 * envelope_rate_hz / search_max_bpm).floor().max(1.0) as usize;
    let maximum_lag = (60.0 * envelope_rate_hz / search_min_bpm).ceil() as usize;
    let correlation_limit = (maximum_lag * 4).min(onset.len().saturating_sub(1));
    let mut correlations = vec![0.0_f64; correlation_limit + 1];

    for (lag, correlation) in correlations.iter_mut().enumerate().skip(1) {
        *correlation = normalized_correlation(onset, lag);
    }

    let mut candidates = (minimum_lag..=maximum_lag)
        .filter(|lag| *lag < correlations.len())
        .map(|lag| {
            let harmonic_two = correlations.get(lag * 2).copied().unwrap_or_default();
            let harmonic_four = correlations.get(lag * 4).copied().unwrap_or_default();
            let bpm = 60.0 * envelope_rate_hz / lag as f64;
            // Ce a priori départage deux hypothèses également plausibles. Un
            // tempo tapé n'en est pas une : l'utilisateur a déjà tranché.
            let tempo_prior = if hinted_bpm.is_none() && bpm > 175.0 {
                0.94
            } else {
                1.0
            };
            let score =
                (correlations[lag] + harmonic_two * 0.35 + harmonic_four * 0.12) * tempo_prior;
            (lag, score)
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    let (best_lag, best_score) = candidates
        .first()
        .copied()
        .ok_or_else(|| "The BPM could not be estimated.".to_owned())?;

    if best_score <= 0.0 {
        return Err("No reliable musical period was found.".to_owned());
    }

    let short_period = refine_period_from_harmonics(best_lag, &correlations);
    let (beat_period, long_range_correlation) = refine_period_across_track(onset, short_period);
    let second_score = candidates
        .iter()
        .find(|(lag, _)| lag.abs_diff(best_lag) > 2)
        .map(|(_, score)| *score)
        .unwrap_or_default();
    let separation = ((best_score - second_score) / best_score).clamp(0.0, 1.0);
    let confidence = (correlations[best_lag].clamp(0.0, 1.0) * 0.62
        + long_range_correlation.clamp(0.0, 1.0) * 0.28
        + separation * 0.10)
        .clamp(0.0, 1.0);
    // The grid is anchored where the beat actually starts, not on the first
    // transient of a beatless intro. The coarse pass only has to land somewhere
    // inside the grooving part, so that the phase is fitted on real beats.
    let coarse_groove_frame = detect_groove_start(kick_energy, envelope_rate_hz);
    let anchor_seed_frame = if coarse_groove_frame > 0.0 {
        coarse_groove_frame
    } else {
        first_significant_onset(onset, onset_peak)
    };
    let pulse_origin_frame = optimize_pulse_origin(onset, anchor_seed_frame, beat_period);
    let duration_frames = duration.as_secs_f64() * envelope_rate_hz;

    // When the drums enter after an intro, they enter on beat one. That is a
    // far stronger cue than comparing the four phases against one another: with
    // a kick on every beat, beat one and beat three carry the same low-end
    // accent and no vote can tell them apart. The four-phase comparison stays
    // for tracks that groove from their first beat, where no entry exists.
    let first_beat_frame = first_grooving_beat(kick_energy, pulse_origin_frame, beat_period)
        .unwrap_or_else(|| {
            let phase = detect_downbeat_phase(kick_onset, onset, pulse_origin_frame, beat_period);
            pulse_origin_frame + phase as f64 * beat_period
        });
    let beat_frames = uniform_beat_frames(first_beat_frame, beat_period, duration_frames);
    let bpm = 60.0 * envelope_rate_hz / beat_period;
    let first_beat_seconds = first_beat_frame / envelope_rate_hz;
    let beats_ms = beat_frames
        .into_iter()
        .map(|frame| seconds_to_millis(frame / envelope_rate_hz))
        .collect();

    Ok(BeatAnalysis {
        bpm: (bpm * 100.0).round() / 100.0,
        confidence: (confidence * 1_000.0).round() / 1_000.0,
        first_beat_ms: seconds_to_millis(first_beat_seconds),
        beats_ms,
        waveform: WaveformPeaks {
            left_min: Vec::new(),
            left_max: Vec::new(),
            left_rms: Vec::new(),
            right_min: Vec::new(),
            right_max: Vec::new(),
            right_rms: Vec::new(),
        },
    })
}

fn normalized_correlation(values: &[f32], lag: usize) -> f64 {
    if lag == 0 || lag >= values.len() {
        return 0.0;
    }

    let mut dot_product = 0.0_f64;
    let mut left_energy = 0.0_f64;
    let mut right_energy = 0.0_f64;

    for index in lag..values.len() {
        let left = f64::from(values[index]);
        let right = f64::from(values[index - lag]);
        dot_product += left * right;
        left_energy += left * left;
        right_energy += right * right;
    }

    let denominator = (left_energy * right_energy).sqrt();
    if denominator > f64::EPSILON {
        dot_product / denominator
    } else {
        0.0
    }
}

fn refine_lag(lag: usize, correlations: &[f64]) -> f64 {
    if lag == 0 || lag + 1 >= correlations.len() {
        return lag as f64;
    }

    let left = correlations[lag - 1];
    let center = correlations[lag];
    let right = correlations[lag + 1];
    let denominator = left - 2.0 * center + right;

    if denominator.abs() <= f64::EPSILON {
        lag as f64
    } else {
        let offset = (0.5 * (left - right) / denominator).clamp(-0.5, 0.5);
        lag as f64 + offset
    }
}

fn refine_period_from_harmonics(lag: usize, correlations: &[f64]) -> f64 {
    let (weighted_sum, weight_sum) = [(1_usize, 1.0_f64), (2, 0.6), (4, 0.35)]
        .into_iter()
        .filter_map(|(multiple, weight)| {
            let center = lag.checked_mul(multiple)?;
            let start = center.saturating_sub(multiple).max(1);
            let end = center
                .saturating_add(multiple)
                .min(correlations.len().saturating_sub(2));
            (start <= end).then(|| {
                let harmonic_lag = (start..=end)
                    .max_by(|left, right| correlations[*left].total_cmp(&correlations[*right]))
                    .unwrap_or(center);
                (
                    refine_lag(harmonic_lag, correlations) / multiple as f64,
                    weight,
                )
            })
        })
        .fold((0.0, 0.0), |(sum, weights), (period, weight)| {
            (sum + period * weight, weights + weight)
        });
    weighted_sum / weight_sum
}

fn refine_period_across_track(onset: &[f32], initial_period: f64) -> (f64, f64) {
    let mut period = initial_period;
    let mut strongest_correlation = 0.0_f64;

    for beat_span in [8_usize, 16, 32, 64] {
        let predicted_lag = period * beat_span as f64;
        if predicted_lag * 2.0 >= onset.len() as f64 {
            break;
        }
        let center = predicted_lag.round() as usize;
        let radius = (period * 0.45).floor().max(2.0) as usize;
        let start = center.saturating_sub(radius).max(1);
        let end = center
            .saturating_add(radius)
            .min(onset.len().saturating_sub(2));
        if start > end {
            continue;
        }

        let Some((best_lag, best_correlation)) = (start..=end)
            .map(|lag| (lag, normalized_correlation(onset, lag)))
            .max_by(|left, right| left.1.total_cmp(&right.1))
        else {
            continue;
        };
        let left = normalized_correlation(onset, best_lag.saturating_sub(1));
        let right = normalized_correlation(onset, best_lag.saturating_add(1));
        let refined_lag = refine_peak(best_lag, left, best_correlation, right);
        let candidate_period = refined_lag / beat_span as f64;

        if ((candidate_period - period) / period).abs() <= 0.02 {
            period = candidate_period;
            strongest_correlation = strongest_correlation.max(best_correlation);
        }
    }

    (period, strongest_correlation)
}

fn refine_peak(index: usize, left: f64, center: f64, right: f64) -> f64 {
    let denominator = left - 2.0 * center + right;
    if denominator.abs() <= f64::EPSILON {
        index as f64
    } else {
        index as f64 + (0.5 * (left - right) / denominator).clamp(-0.5, 0.5)
    }
}

/// Frame where the beat-driven part of the track begins.
///
/// An electronic track often opens with a minute of pads, noise or a riser
/// before the drums enter. Anchoring the grid on a point extrapolated back into
/// that intro gives a first beat where nothing is playing, which is of no use
/// for beatmatching. The kick band answers the question directly: the groove
/// starts where low-end energy rises to a real fraction of the level the track
/// holds later, and stays there.
///
/// Returns 0 when no such step exists, which covers both a track that grooves
/// from its first second and one that never does.
fn detect_groove_start(kick_energy: &[f32], envelope_rate_hz: f64) -> f64 {
    let window = (envelope_rate_hz * 2.0).round() as usize;
    if kick_energy.len() <= window * 2 {
        return 0.0;
    }

    let mut smoothed = Vec::with_capacity(kick_energy.len() - window);
    let mut sum: f64 = kick_energy[..window].iter().map(|v| f64::from(*v)).sum();
    smoothed.push((sum / window as f64) as f32);
    for index in window..kick_energy.len() {
        sum += f64::from(kick_energy[index]) - f64::from(kick_energy[index - window]);
        smoothed.push((sum / window as f64) as f32);
    }

    // A high percentile rather than the maximum, so one loud bar cannot set
    // the reference and a fade-out cannot lower it.
    let mut ranked = smoothed.clone();
    ranked.sort_by(f32::total_cmp);
    let reference = ranked[ranked.len() * 4 / 5];
    if reference <= 1.0e-5 {
        return 0.0;
    }

    let threshold = reference * 0.35;
    let hold = window;
    let mut candidate: Option<usize> = None;
    let mut coarse_start = None;
    for (index, level) in smoothed.iter().enumerate() {
        if *level >= threshold {
            let start = *candidate.get_or_insert(index);
            if index - start >= hold {
                coarse_start = Some(start);
                break;
            }
        } else {
            candidate = None;
        }
    }

    let Some(coarse_start) = coarse_start else {
        return 0.0;
    };

    // `smoothed[k]` averages the window that *follows* k, so it crosses the
    // threshold before the material does. The coarse index is therefore only a
    // lower bound; the first frame that genuinely carries the kick is found by
    // walking forward over the raw envelope.
    let entry_threshold = reference * 0.5;
    kick_energy
        .iter()
        .enumerate()
        .skip(coarse_start)
        .find(|(_, level)| **level >= entry_threshold)
        .map(|(frame, _)| frame as f64)
        .unwrap_or(coarse_start as f64)
}

/// Frame of the first beat the kick actually plays on.
///
/// Measured on the beat grid rather than on raw level, which is what makes it
/// find the *first* kick instead of the loudest section: an intro whose kick is
/// well below the drop still lands on the grid, so it counts, while a beatless
/// intro has nothing on the grid at all and does not.
///
/// Returns `None` when the kick never settles into the grid — an ambient piece,
/// or a track with no usable low end — leaving the decision to the phase vote.
fn first_grooving_beat(kick_energy: &[f32], pulse_origin: f64, period_frames: f64) -> Option<f64> {
    if kick_energy.is_empty() || period_frames <= 0.0 {
        return None;
    }

    // Walk the grid from its earliest point inside the file, so a quiet kick
    // before the coarse groove estimate is still seen.
    let first_grid = pulse_origin - (pulse_origin / period_frames).floor() * period_frames;
    let radius = (period_frames * 0.08).clamp(1.0, 4.0);
    let mut strengths = Vec::new();
    let mut beat_index = 0_usize;
    loop {
        let position = first_grid + beat_index as f64 * period_frames;
        if position >= kick_energy.len() as f64 {
            break;
        }
        let start = (position - radius).max(0.0).floor() as usize;
        let end = (position + radius)
            .ceil()
            .min(kick_energy.len().saturating_sub(1) as f64) as usize;
        strengths.push(
            kick_energy[start..=end]
                .iter()
                .copied()
                .fold(0.0_f32, f32::max),
        );
        beat_index += 1;
    }

    const RUN_BEATS: usize = 8;
    if strengths.len() < RUN_BEATS * 2 {
        return None;
    }

    // Average over two bars, so a single stray thump in the intro cannot pass
    // for a groove and one missing kick cannot break one.
    let smoothed = strengths
        .windows(RUN_BEATS)
        .map(|run| run.iter().sum::<f32>() / RUN_BEATS as f32)
        .collect::<Vec<_>>();
    let mut ranked = smoothed.clone();
    ranked.sort_by(f32::total_cmp);
    let reference = ranked[ranked.len() * 4 / 5];
    if reference <= 1.0e-5 {
        return None;
    }

    // The one knob of this whole decision. It is deliberately set against the
    // level the track holds later rather than against its noise floor: builds
    // routinely carry a filtered kick well under the drop, and a floor-relative
    // threshold anchors on that instead of on the beat a DJ would count from.
    // A kick at a third of the later level counts; one much quieter reads as
    // part of the intro.
    const GROOVE_LEVEL_RATIO: f32 = 0.3;
    let threshold = reference * GROOVE_LEVEL_RATIO;
    let start_beat = smoothed.iter().position(|value| *value >= threshold)?;

    // The window opens on the first grooving beat, but the kick may land a beat
    // or two into it; take the first beat of the window that actually hits.
    let beat_threshold = threshold;
    let offset = strengths[start_beat..(start_beat + RUN_BEATS).min(strengths.len())]
        .iter()
        .position(|value| *value >= beat_threshold)
        .unwrap_or(0);

    Some(first_grid + (start_beat + offset) as f64 * period_frames)
}

fn first_significant_onset(onset: &[f32], peak: f32) -> f64 {
    let threshold = peak * 0.2;
    onset
        .iter()
        .position(|value| *value >= threshold)
        .unwrap_or_default() as f64
}

/// Finds the phase of the beat grid that best explains the whole track.
///
/// The phase is periodic with the beat, so the search covers a full period.
/// It used to look only ±18 % of a period around the first significant onset,
/// which silently assumed that onset was itself on a beat. In an electronic
/// intro it often is not — a riser, a pad swell, vinyl noise or an upbeat lands
/// first — and the correct phase was then unreachable, offsetting the entire
/// grid. `initial_origin` now only decides which period the answer sits in.
fn optimize_pulse_origin(onset: &[f32], initial_origin: f64, period_frames: f64) -> f64 {
    let step = 0.125_f64;
    let sample_radius = (period_frames * 0.06).clamp(1.0, 3.0);
    let search_start = (initial_origin - period_frames).max(0.0);
    let steps = (period_frames / step).ceil() as usize;
    let mut best_origin = initial_origin;
    let mut best_score = f64::NEG_INFINITY;

    for step_index in 0..=steps {
        let origin = search_start + step_index as f64 * step;
        let mut score = 0.0_f64;
        let mut count = 0_u64;
        let mut beat_index = 0_u64;

        loop {
            let position = origin + beat_index as f64 * period_frames;
            if position >= onset.len() as f64 {
                break;
            }
            let start = (position - sample_radius).max(0.0).floor() as usize;
            let end = (position + sample_radius)
                .ceil()
                .min(onset.len().saturating_sub(1) as f64) as usize;
            let strength = onset[start..=end]
                .iter()
                .enumerate()
                .map(|(offset, strength)| {
                    let frame = (start + offset) as f64;
                    let distance = (frame - position).abs();
                    let weight = (1.0 - distance / (sample_radius + 1.0)).max(0.0);
                    f64::from(*strength).powi(2) * weight
                })
                .sum::<f64>();
            score += strength;
            count += 1;
            beat_index += 1;
        }

        let average_score = if count > 0 { score / count as f64 } else { 0.0 };
        if average_score > best_score {
            best_score = average_score;
            best_origin = origin;
        }
    }

    best_origin
}

fn uniform_beat_frames(first_beat: f64, period_frames: f64, duration_frames: f64) -> Vec<f64> {
    let mut beats = Vec::new();
    let mut beat_index = 0_u64;

    loop {
        let position = first_beat + beat_index as f64 * period_frames;
        if position > duration_frames + 1.0e-6 {
            break;
        }
        beats.push(position);
        beat_index += 1;
    }

    beats
}

const BEATS_PER_MEASURE: usize = 4;

/// Chooses which of the four beats of the bar is beat one.
///
/// It reads the kick band rather than the full mix. In electronic music the
/// downbeat is the kick, but a clap or snare on beats two and four produces a
/// far larger broadband onset than the kick does — brighter, wider spectrum,
/// bigger jump in log energy. Deciding on the full mix therefore locked onto
/// the backbeat again and again, which is what put the red line half a bar off.
///
/// Evidence is combined from two views so that one loud passage cannot decide
/// the whole track: the average accent of each phase, and how many individual
/// bars that phase wins. A phase that wins bar after bar is a downbeat; one
/// that wins on the average alone may just contain a single loud event.
fn detect_downbeat_phase(
    kick_onset: &[f32],
    onset: &[f32],
    pulse_origin: f64,
    period_frames: f64,
) -> usize {
    // A track with no low end at all — ambient, a thin sample — leaves nothing
    // to read in the kick band, so the full mix stays the best evidence there.
    let kick_peak = kick_onset.iter().copied().fold(0.0_f32, f32::max);
    let accents = if kick_onset.len() >= onset.len() / 2 && kick_peak > 1.0e-3 {
        kick_onset
    } else {
        onset
    };

    let radius = (period_frames * 0.08).clamp(1.0, 4.0);
    let mut energy = [0.0_f64; BEATS_PER_MEASURE];
    let mut counts = [0_u64; BEATS_PER_MEASURE];
    let mut votes = [0_u64; BEATS_PER_MEASURE];
    let mut bar = [0.0_f64; BEATS_PER_MEASURE];
    let mut beat_index = 0_usize;

    loop {
        let position = pulse_origin + beat_index as f64 * period_frames;
        if position >= accents.len() as f64 {
            break;
        }
        let start = (position - radius).max(0.0).floor() as usize;
        let end = (position + radius)
            .ceil()
            .min(accents.len().saturating_sub(1) as f64) as usize;
        let strength = f64::from(accents[start..=end].iter().copied().fold(0.0_f32, f32::max));

        let phase = beat_index % BEATS_PER_MEASURE;
        energy[phase] += strength * strength;
        counts[phase] += 1;
        bar[phase] = strength;

        if phase == BEATS_PER_MEASURE - 1 {
            let winner = (1..BEATS_PER_MEASURE).fold(0, |best, candidate| {
                if bar[candidate] > bar[best] {
                    candidate
                } else {
                    best
                }
            });
            // A flat bar names no winner; only a real accent votes.
            if bar[winner] > 0.0 {
                votes[winner] += 1;
            }
            bar = [0.0; BEATS_PER_MEASURE];
        }
        beat_index += 1;
    }

    for (value, count) in energy.iter_mut().zip(counts) {
        if count > 0 {
            *value /= count as f64;
        }
    }

    let energy_peak = energy.iter().copied().fold(0.0_f64, f64::max);
    let vote_peak = votes.iter().copied().max().unwrap_or_default() as f64;
    if energy_peak <= 0.0 && vote_peak <= 0.0 {
        return 0;
    }

    let mut best_phase = 0;
    let mut best_score = f64::NEG_INFINITY;
    for phase in 0..BEATS_PER_MEASURE {
        let normalized_energy = if energy_peak > 0.0 {
            energy[phase] / energy_peak
        } else {
            0.0
        };
        let normalized_votes = if vote_peak > 0.0 {
            votes[phase] as f64 / vote_peak
        } else {
            0.0
        };
        let score = normalized_energy * 0.6 + normalized_votes * 0.4;
        if score > best_score {
            best_score = score;
            best_phase = phase;
        }
    }

    best_phase
}

fn seconds_to_millis(seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        0
    } else {
        (seconds * 1_000.0).round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ENVELOPE_RATE_HZ, MAX_BPM, MIN_BPM, collect_audio_features, estimate_beat_grid,
        first_grooving_beat, fit_rigid_grid, model_downbeat_phase, nearest_beat_with_phase,
        optimize_pulse_origin, refine_period_across_track, tempo_search_window,
    };
    use std::time::Duration;

    #[test]
    fn without_a_tap_the_search_covers_the_whole_useful_range() {
        assert_eq!(tempo_search_window(None), (MIN_BPM, MAX_BPM));
    }

    #[test]
    fn a_tap_narrows_the_search_around_itself() {
        let (low, high) = tempo_search_window(Some(128.0));
        assert!(low < 128.0 && high > 128.0);
        // Assez large pour une main peu sûre, assez étroit pour ne jamais
        // atteindre le demi-temps ni le double-temps.
        assert!(
            low > 64.0 * 1.5,
            "la fenêtre approche le demi-temps : {low}"
        );
        assert!(
            high < 256.0 * 0.7,
            "la fenêtre approche le double-temps : {high}"
        );
        assert!((high - low) / 128.0 < 0.2);
    }

    #[test]
    fn rigid_grid_ignores_missing_and_duplicate_model_events() {
        let bpm = 126.0;
        let period = 60.0 / bpm;
        let mut observations = Vec::new();
        for beat in 0..1_000 {
            if beat % 23 == 7 {
                continue;
            }
            let jitter = ((beat as f64 * 1.713).sin() * 0.009) as f32;
            let time = (0.137 + beat as f64 * period) as f32 + jitter;
            observations.push(time);
            if beat % 31 == 4 {
                observations.push(time + 0.067);
            }
        }

        let grid = fit_rigid_grid(&observations, 125.0).expect("a rigid grid should fit");

        assert!((60.0 / grid.period_seconds - bpm).abs() < 0.01);
        assert!(grid.residual_rms_seconds < 0.012);
        assert!(grid.matched_events > 900);
    }

    #[test]
    fn model_downbeats_vote_for_one_bar_phase_despite_false_peaks() {
        let grid = super::RigidGrid {
            period_seconds: 0.5,
            origin_seconds: 0.02,
            matched_events: 200,
            residual_rms_seconds: 0.004,
        };
        let mut downbeats = (0..60)
            .map(|bar| (grid.origin_seconds + (bar * 4 + 2) as f64 * 0.5) as f32)
            .collect::<Vec<_>>();
        downbeats
            .extend((0..12).map(|bar| (grid.origin_seconds + (bar * 4 + 1) as f64 * 0.5) as f32));

        let (phase, separation) = model_downbeat_phase(&downbeats, grid);

        assert_eq!(phase, 2);
        assert!(separation > 0.7);
    }

    #[test]
    fn groove_threshold_on_beat_two_keeps_the_preceding_downbeat() {
        assert_eq!(nearest_beat_with_phase(25, 0), 24);
        assert_eq!(nearest_beat_with_phase(26, 0), 24);
        assert_eq!(nearest_beat_with_phase(27, 0), 28);
    }

    #[test]
    fn a_tap_never_leaves_the_bounds_the_rest_of_the_program_accepts() {
        let (slow_low, slow_high) = tempo_search_window(Some(MIN_BPM + 1.0));
        assert!(
            slow_low >= MIN_BPM,
            "la fenêtre descend sous la borne basse"
        );
        assert!(slow_low < slow_high);

        let (fast_low, fast_high) = tempo_search_window(Some(MAX_BPM - 1.0));
        assert!(fast_high <= MAX_BPM, "la fenêtre dépasse la borne haute");
        assert!(fast_low < fast_high);
    }

    #[test]
    fn an_absurd_tap_falls_back_to_the_full_range() {
        // Hors plage utile dans les deux sens, et les valeurs qu'un calcul raté
        // peut produire.
        for bpm in [1.0, 10_000.0, f64::NAN, f64::INFINITY, -120.0, 0.0] {
            assert_eq!(
                tempo_search_window(Some(bpm)),
                (MIN_BPM, MAX_BPM),
                "un tap de {bpm} devrait être ignoré"
            );
        }
    }

    #[test]
    fn a_shaky_tap_is_pulled_onto_the_tempo_the_kicks_actually_carry() {
        // La détection automatique se trompe surtout de période; les attaques
        // restent bonnes. Un tap à 3 % près doit donc être ramené sur la vraie
        // valeur par la corrélation, pas conservé tel quel.
        let onset = synthetic_onset(128.0, 45.0, 0.4);
        for tapped in [124.2, 126.0, 130.5, 131.8] {
            let analysis = estimate_beat_grid(
                &onset,
                &[],
                &[],
                ENVELOPE_RATE_HZ,
                Duration::from_secs(45),
                Some(tapped),
            )
            .expect("a hinted analysis should succeed");
            assert!(
                (analysis.bpm - 128.0).abs() < 0.5,
                "un tap à {tapped} devrait retomber sur 128, obtenu {}",
                analysis.bpm
            );
        }
    }

    #[test]
    fn detects_a_steady_120_bpm_grid() {
        let onset = synthetic_onset(120.0, 45.0, 0.4);
        let analysis = estimate_beat_grid(
            &onset,
            &[],
            &[],
            ENVELOPE_RATE_HZ,
            Duration::from_secs(45),
            None,
        )
        .expect("steady pulse should be analyzed");

        assert!(
            (analysis.bpm - 120.0).abs() < 0.25,
            "detected {} BPM",
            analysis.bpm
        );
        assert!(analysis.first_beat_ms.abs_diff(400) <= 20);
        assert!(analysis.beats_ms.len() >= 89);
        assert!(analysis.confidence > 0.7);
    }

    #[test]
    fn detects_a_fractional_128_bpm_grid() {
        let onset = synthetic_onset(128.0, 60.0, 1.0);
        let analysis = estimate_beat_grid(
            &onset,
            &[],
            &[],
            ENVELOPE_RATE_HZ,
            Duration::from_secs(60),
            None,
        )
        .expect("fractional pulse should be analyzed");

        assert!(
            (analysis.bpm - 128.0).abs() < 0.75,
            "detected {} BPM",
            analysis.bpm
        );
        assert!(
            analysis.first_beat_ms.abs_diff(1_000) <= 30,
            "first beat was {} ms at {} BPM",
            analysis.first_beat_ms,
            analysis.bpm
        );
        assert!(analysis.beats_ms.len() >= 125);
    }

    #[test]
    fn identifies_the_accented_first_beat_of_a_four_beat_measure() {
        let onset = synthetic_accented_onset(120.0, 45.0, 0.4, 3);
        let analysis = estimate_beat_grid(
            &onset,
            &[],
            &[],
            ENVELOPE_RATE_HZ,
            Duration::from_secs(45),
            None,
        )
        .expect("accented four-beat pulse should be analyzed");

        assert!(
            analysis.first_beat_ms.abs_diff(1_900) <= 25,
            "downbeat was {} ms",
            analysis.first_beat_ms
        );
    }

    #[test]
    fn persisted_beat_grid_uses_one_stable_period_without_local_warping() {
        let onset = synthetic_onset(128.0, 60.0, 1.0);
        let analysis = estimate_beat_grid(
            &onset,
            &[],
            &[],
            ENVELOPE_RATE_HZ,
            Duration::from_secs(60),
            None,
        )
        .expect("steady pulse should be analyzed");
        let expected_period_ms = 60_000.0 / analysis.bpm;

        assert!(
            analysis
                .beats_ms
                .windows(2)
                .all(|beats| { ((beats[1] - beats[0]) as f64 - expected_period_ms).abs() <= 1.1 })
        );
    }

    #[test]
    fn long_range_refinement_corrects_a_small_bpm_error_before_it_can_accumulate() {
        let onset = synthetic_onset(128.0, 360.0, 0.4);
        let imprecise_period = 60.0 * ENVELOPE_RATE_HZ / 127.6;
        let (refined_period, correlation) = refine_period_across_track(&onset, imprecise_period);
        let refined_bpm = 60.0 * ENVELOPE_RATE_HZ / refined_period;

        assert!(
            (refined_bpm - 128.0).abs() < 0.02,
            "refined {refined_bpm} BPM"
        );
        assert!(correlation > 0.95);
    }

    #[test]
    fn global_phase_optimization_recovers_the_pulse_origin() {
        let onset = synthetic_onset(120.0, 90.0, 0.44);
        let period = 60.0 * ENVELOPE_RATE_HZ / 120.0;
        let origin = optimize_pulse_origin(&onset, 0.50 * ENVELOPE_RATE_HZ, period);

        assert!((origin / ENVELOPE_RATE_HZ - 0.44).abs() < 0.01);
    }

    #[test]
    fn rejects_audio_that_is_too_short() {
        let onset = synthetic_onset(120.0, 4.0, 0.0);
        let result = estimate_beat_grid(
            &onset,
            &[],
            &[],
            ENVELOPE_RATE_HZ,
            Duration::from_secs(4),
            None,
        );

        assert!(result.is_err());
    }

    #[test]
    fn builds_normalized_stereo_waveform_peaks() {
        let stereo_samples = vec![-0.5, 0.25, 0.25, -1.0, -0.25, 0.5, 1.0, -0.5];
        let waveform =
            collect_audio_features(stereo_samples.into_iter(), 4, 2, Duration::from_secs(1), 2)
                .waveform;

        assert_eq!(waveform.left_min, vec![-0.5, -0.25]);
        assert_eq!(waveform.left_max, vec![0.25, 1.0]);
        assert_eq!(waveform.left_rms.len(), 2);
        assert_eq!(waveform.right_min, vec![-1.0, -0.5]);
        assert_eq!(waveform.right_max, vec![0.25, 0.5]);
        assert_eq!(waveform.right_rms.len(), 2);
        assert!((waveform.left_rms[0] - 0.395_284_7).abs() < 1.0e-6);
        assert!((waveform.right_rms[0] - 0.728_869).abs() < 1.0e-6);
    }

    #[test]
    fn duplicates_mono_samples_on_both_channels() {
        let waveform =
            collect_audio_features(vec![-0.5, 1.0].into_iter(), 2, 1, Duration::from_secs(1), 1)
                .waveform;

        assert_eq!(waveform.left_min, waveform.right_min);
        assert_eq!(waveform.left_max, waveform.right_max);
        assert_eq!(waveform.left_rms, waveform.right_rms);
    }

    /// A long beatless intro followed by the drums entering. The grid used to
    /// be anchored on the first transient of the intro, where nothing is
    /// playing; the anchor now falls on the first beat the kick lands on.
    #[test]
    fn the_first_beat_is_the_first_kick_after_a_beatless_intro() {
        let bpm = 126.0;
        let period = 60.0 * ENVELOPE_RATE_HZ / bpm;
        let seconds = 180.0;
        let silent_beats = 130;
        let groove_frame = 4.0 + silent_beats as f64 * period;

        let mut kick_energy = vec![0.002_f32; (seconds * ENVELOPE_RATE_HZ) as usize];
        let mut beat = 0;
        loop {
            let position = 4.0 + beat as f64 * period;
            if position >= kick_energy.len() as f64 {
                break;
            }
            if beat >= silent_beats {
                let index = position.round() as usize;
                for frame in index..(index + 8).min(kick_energy.len()) {
                    kick_energy[frame] = 0.5;
                }
            }
            beat += 1;
        }

        let found = first_grooving_beat(&kick_energy, 4.0, period)
            .expect("a groove that starts partway in should be found");
        assert!(
            (found - groove_frame).abs() < period * 0.5,
            "groove placed at {found} frames, expected {groove_frame}"
        );
    }

    /// An intro kick counts as the groove as long as it is a real fraction of
    /// the level the track settles at. A build that carries a heavily filtered
    /// kick far below the drop is deliberately read as intro instead: on the
    /// reference recordings that is the beat a DJ counts from.
    #[test]
    fn an_intro_kick_counts_once_it_is_a_real_fraction_of_the_later_level() {
        let period = 47.0;
        let mut kick_energy = vec![0.001_f32; 12_000];
        let quiet_from = 16;
        let loud_from = 120;
        let mut beat = 0;
        loop {
            let position = 10.0 + beat as f64 * period;
            if position >= kick_energy.len() as f64 {
                break;
            }
            if beat >= quiet_from {
                let index = position.round() as usize;
                // The intro kick is 40 % of the level the drop reaches.
                let level = if beat >= loud_from { 0.5 } else { 0.2 };
                for frame in index..(index + 8).min(kick_energy.len()) {
                    kick_energy[frame] = level;
                }
            }
            beat += 1;
        }

        let found = first_grooving_beat(&kick_energy, 10.0, period)
            .expect("the quiet groove should be found");
        let expected = 10.0 + quiet_from as f64 * period;
        assert!(
            (found - expected).abs() < period * 0.5,
            "groove placed at {found} frames, expected {expected}"
        );
    }

    #[test]
    fn a_track_with_no_kick_at_all_reports_no_groove() {
        assert!(first_grooving_beat(&vec![0.0_f32; 12_000], 10.0, 47.0).is_none());
        assert!(first_grooving_beat(&[], 0.0, 47.0).is_none());
    }

    /// A four-to-the-floor bar as it really reaches the analyser: a kick on
    /// every beat, and a clap on two and four whose broadband onset is larger
    /// than the kick's. This is the case that used to place the downbeat on the
    /// backbeat.
    #[test]
    fn the_kick_band_decides_the_downbeat_when_the_clap_is_louder() {
        let bpm = 128.0;
        let seconds = 60.0;
        let origin = 0.5;
        let downbeat_phase = 2;

        let full = synthetic_layered_onset(bpm, seconds, origin, |phase| {
            // The clap sits two beats from the downbeat and dominates the mix.
            if phase == (downbeat_phase + 1) % 4 || phase == (downbeat_phase + 3) % 4 {
                1.0
            } else {
                0.45
            }
        });
        let kick = synthetic_layered_onset(bpm, seconds, origin, |phase| {
            if phase == downbeat_phase { 1.0 } else { 0.5 }
        });

        let expected_ms = ((origin + downbeat_phase as f64 * 60.0 / bpm) * 1_000.0).round() as u64;

        let with_kick = estimate_beat_grid(
            &full,
            &kick,
            &[],
            ENVELOPE_RATE_HZ,
            Duration::from_secs(60),
            None,
        )
        .expect("layered pulse should be analyzed");
        assert!(
            with_kick.first_beat_ms.abs_diff(expected_ms) <= 30,
            "kick band placed the downbeat at {} ms, expected {expected_ms} ms",
            with_kick.first_beat_ms
        );

        // Reading the full mix alone is what fails: it follows the clap.
        let without_kick = estimate_beat_grid(
            &full,
            &[],
            &[],
            ENVELOPE_RATE_HZ,
            Duration::from_secs(60),
            None,
        )
        .expect("layered pulse should be analyzed");
        assert!(
            without_kick.first_beat_ms.abs_diff(expected_ms) > 30,
            "the broadband envelope was expected to be misled here"
        );
    }

    #[test]
    fn a_track_without_low_end_still_uses_the_full_mix() {
        let onset = synthetic_accented_onset(120.0, 45.0, 0.4, 3);
        let silent_kick = vec![0.0_f32; onset.len()];
        let analysis = estimate_beat_grid(
            &onset,
            &silent_kick,
            &silent_kick,
            ENVELOPE_RATE_HZ,
            Duration::from_secs(45),
            None,
        )
        .expect("accented pulse should be analyzed");

        assert!(
            analysis.first_beat_ms.abs_diff(1_900) <= 25,
            "downbeat was {} ms",
            analysis.first_beat_ms
        );
    }

    /// The phase of a grid repeats every beat, so a seed that is not itself on
    /// a beat must not stop the search from finding the right one.
    #[test]
    fn the_pulse_origin_is_found_even_when_the_first_onset_is_not_a_beat() {
        let bpm = 128.0;
        let period = 60.0 * ENVELOPE_RATE_HZ / bpm;
        let mut onset = synthetic_onset(bpm, 60.0, 1.0);
        // A riser lands well off the grid, two thirds of a beat early.
        let stray = (1.0 * ENVELOPE_RATE_HZ - period * 0.66).round() as usize;
        onset[stray] = 1.0;

        let origin = optimize_pulse_origin(&onset, stray as f64, period);
        let distance_to_grid = {
            let beats = (origin - 1.0 * ENVELOPE_RATE_HZ) / period;
            (beats - beats.round()).abs() * period
        };
        assert!(
            distance_to_grid < 1.0,
            "origin {origin} sits {distance_to_grid} frames off the true grid"
        );
    }

    fn synthetic_layered_onset(
        bpm: f64,
        duration_seconds: f64,
        first_pulse_seconds: f64,
        strength_for_phase: impl Fn(usize) -> f32,
    ) -> Vec<f32> {
        let mut onset = vec![0.0; (duration_seconds * ENVELOPE_RATE_HZ) as usize];
        let period = 60.0 * ENVELOPE_RATE_HZ / bpm;
        let mut position = first_pulse_seconds * ENVELOPE_RATE_HZ;
        let mut beat_index = 0_usize;

        while position < onset.len() as f64 {
            let index = position.round() as usize;
            if let Some(value) = onset.get_mut(index) {
                *value = strength_for_phase(beat_index % 4);
            }
            position += period;
            beat_index += 1;
        }

        onset
    }

    fn synthetic_onset(bpm: f64, duration_seconds: f64, first_beat_seconds: f64) -> Vec<f32> {
        let mut onset = vec![0.0; (duration_seconds * ENVELOPE_RATE_HZ) as usize];
        let period = 60.0 * ENVELOPE_RATE_HZ / bpm;
        let mut position = first_beat_seconds * ENVELOPE_RATE_HZ;

        while position < onset.len() as f64 {
            let index = position.round() as usize;
            if let Some(value) = onset.get_mut(index) {
                *value = 1.0;
            }
            position += period;
        }

        onset
    }

    fn synthetic_accented_onset(
        bpm: f64,
        duration_seconds: f64,
        first_pulse_seconds: f64,
        downbeat_phase: usize,
    ) -> Vec<f32> {
        let mut onset = vec![0.0; (duration_seconds * ENVELOPE_RATE_HZ) as usize];
        let period = 60.0 * ENVELOPE_RATE_HZ / bpm;
        let mut position = first_pulse_seconds * ENVELOPE_RATE_HZ;
        let mut beat_index = 0_usize;

        while position < onset.len() as f64 {
            let index = position.round() as usize;
            if let Some(value) = onset.get_mut(index) {
                *value = if beat_index % 4 == downbeat_phase {
                    1.0
                } else {
                    0.35
                };
            }
            position += period;
            beat_index += 1;
        }

        onset
    }
}
