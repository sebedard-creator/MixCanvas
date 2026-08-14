//! Rendu hors ligne du mix complet vers un fichier WAV.
//!
//! Le bounce réutilise `TimelineMixSource`, la source même que joue le
//! transport : time-stretch, égaliseur de clip, filtres, automation de volume,
//! sidechain, compresseur, teinte et limiteur. Un moteur de rendu séparé
//! finirait par diverger de celui qu'on entend, et un bounce qui ne ressemble
//! pas au monitoring ne sert à rien.
//!
//! Il ne tourne pas en temps réel : la source est tirée aussi vite que la
//! machine le permet, sans périphérique audio ni contrainte de latence.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use rodio::Source;

use super::timeline::{OUTPUT_CHANNELS, prepare_timeline};
use crate::timeline::TimelineRenderPlan;

/// 44,1 kHz, la fréquence des MP3 sources. Rendre à cette valeur évite une
/// conversion de fréquence que la qualité n'aurait rien à y gagner.
pub const BOUNCE_SAMPLE_RATE: u32 = 44_100;
const BOUNCE_BITS_PER_SAMPLE: u16 = 16;
/// Pleine échelle en 16 bits signés. On divise par 32767 et non par 32768 pour
/// qu'un signal à −1,0 comme à +1,0 reste symétrique après conversion.
const FULL_SCALE: f32 = 32_767.0;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BounceSummary {
    pub path: String,
    pub frames: usize,
    pub duration_seconds: f64,
    /// Silence de tête écarté, en secondes.
    pub trimmed_seconds: f64,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
}

/// Bruit de dithering triangulaire, d'un LSB de crête à crête.
///
/// Tronquer du flottant vers 16 bits corrèle l'erreur au signal : elle
/// s'entend comme une distorsion sur les fondus et les queues, là où le niveau
/// descend vers les derniers bits. Un bruit triangulaire décorrèle cette
/// erreur et rend sa variance indépendante du signal — on échange une
/// distorsion audible contre un souffle constant vingt décibels plus bas.
struct TriangularDither {
    state: u32,
}

impl TriangularDither {
    fn new() -> Self {
        Self { state: 0x2545_f491 }
    }

    /// Xorshift : suffisant pour du dithering, et déterministe, de sorte que
    /// deux bounces du même mix donnent le même fichier.
    fn uniform(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        (self.state as f32 / u32::MAX as f32) - 0.5
    }

    fn next(&mut self) -> f32 {
        self.uniform() + self.uniform()
    }
}

/// Premier instant où un clip se fait entendre.
///
/// Un projet dont le premier clip commence à la mesure trois n'a aucune raison
/// d'exporter deux mesures de silence : le bounce commence là où le son
/// commence.
fn first_audible_seconds(plan: &TimelineRenderPlan) -> f64 {
    let first_beat = plan
        .clips
        .iter()
        .map(|clip| clip.visual_start_beat)
        .fold(f64::INFINITY, f64::min);
    if !first_beat.is_finite() || first_beat <= 0.0 {
        return 0.0;
    }
    plan.tempo_map.seconds_at_beat(first_beat).max(0.0)
}

/// Rend le mix, en signalant sa progression.
///
/// `on_progress` reçoit une fraction de 0 à 1. Elle n'est appelée qu'au
/// changement de point de pourcentage : un rendu émet donc cent messages au
/// plus, quelle que soit la longueur du mix. Inonder l'interface d'événements
/// la ralentirait précisément pendant qu'elle doit rester réactive.
pub fn bounce_timeline(
    plan: &TimelineRenderPlan,
    path: &Path,
    on_progress: &mut dyn FnMut(f64),
) -> Result<BounceSummary, String> {
    if plan.clips.is_empty() {
        return Err("Add at least one clip before bouncing the mix.".to_owned());
    }

    let mut source = prepare_timeline(plan, true, BOUNCE_SAMPLE_RATE)?;
    let trimmed_seconds = first_audible_seconds(plan);
    if trimmed_seconds > 0.0 {
        source
            .try_seek(Duration::from_secs_f64(trimmed_seconds))
            .map_err(|error| format!("The mix could not be positioned for bouncing: {error}"))?;
    }

    let file =
        File::create(path).map_err(|error| format!("This file could not be created: {error}"))?;
    let mut writer = BufWriter::new(file);
    write_placeholder_header(&mut writer)?;

    // La source sait exactement combien d'échantillons il lui reste après le
    // repositionnement : la progression est mesurée, pas estimée.
    let expected_samples = source.len().max(1);
    let mut last_percent = usize::MAX;
    on_progress(0.0);

    // Écrit au fil de l'eau plutôt qu'accumulé : un mix d'une heure fait
    // 635 Mo en 16 bits stéréo, qu'il n'y a aucune raison de tenir en mémoire.
    let mut dither = TriangularDither::new();
    let mut samples = 0_usize;
    let mut block = Vec::with_capacity(8_192);
    for sample in source.by_ref() {
        let scaled = sample.clamp(-1.0, 1.0) * FULL_SCALE + dither.next();
        let quantised = scaled.round().clamp(-FULL_SCALE, FULL_SCALE) as i16;
        block.extend_from_slice(&quantised.to_le_bytes());
        samples += 1;
        if block.len() >= 8_192 {
            writer
                .write_all(&block)
                .map_err(|error| format!("Writing the mix failed: {error}"))?;
            block.clear();

            let percent = samples * 100 / expected_samples;
            if percent != last_percent {
                last_percent = percent;
                on_progress(samples as f64 / expected_samples as f64);
            }
        }
    }
    if !block.is_empty() {
        writer
            .write_all(&block)
            .map_err(|error| format!("Writing the mix failed: {error}"))?;
    }

    let data_bytes = samples * 2;
    finish_header(&mut writer, data_bytes)?;
    writer
        .flush()
        .map_err(|error| format!("Writing the mix failed: {error}"))?;

    on_progress(1.0);

    let frames = samples / OUTPUT_CHANNELS as usize;
    Ok(BounceSummary {
        path: path.to_string_lossy().into_owned(),
        frames,
        duration_seconds: frames as f64 / f64::from(BOUNCE_SAMPLE_RATE),
        trimmed_seconds,
        sample_rate: BOUNCE_SAMPLE_RATE,
        bits_per_sample: BOUNCE_BITS_PER_SAMPLE,
    })
}

/// En-tête RIFF avec des tailles provisoires.
///
/// Elles ne sont connues qu'une fois le rendu terminé, et les deviner
/// obligerait à garder tout l'audio en mémoire pour le compter.
fn write_placeholder_header(writer: &mut BufWriter<File>) -> Result<(), String> {
    let channels = OUTPUT_CHANNELS;
    let byte_rate =
        BOUNCE_SAMPLE_RATE * u32::from(channels) * u32::from(BOUNCE_BITS_PER_SAMPLE) / 8;
    let block_align = channels * BOUNCE_BITS_PER_SAMPLE / 8;

    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&0_u32.to_le_bytes()); // taille du bloc, à corriger
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes()); // PCM entier
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&BOUNCE_SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&BOUNCE_BITS_PER_SAMPLE.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&0_u32.to_le_bytes()); // taille des données, à corriger

    writer
        .write_all(&header)
        .map_err(|error| format!("Writing the mix failed: {error}"))
}

fn finish_header(writer: &mut BufWriter<File>, data_bytes: usize) -> Result<(), String> {
    let data_size = u32::try_from(data_bytes).map_err(|_| {
        "This mix is too long for a WAV file, which stops at four gigabytes.".to_owned()
    })?;
    let riff_size = data_size
        .checked_add(36)
        .ok_or_else(|| "This mix is too long for a WAV file.".to_owned())?;

    writer
        .seek(SeekFrom::Start(4))
        .map_err(|error| format!("Writing the mix failed: {error}"))?;
    writer
        .write_all(&riff_size.to_le_bytes())
        .map_err(|error| format!("Writing the mix failed: {error}"))?;
    writer
        .seek(SeekFrom::Start(40))
        .map_err(|error| format!("Writing the mix failed: {error}"))?;
    writer
        .write_all(&data_size.to_le_bytes())
        .map_err(|error| format!("Writing the mix failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dither_is_triangular_and_bounded_to_one_lsb() {
        let mut dither = TriangularDither::new();
        let mut lowest = f32::INFINITY;
        let mut highest = f32::NEG_INFINITY;
        let mut total = 0.0_f64;
        for _ in 0..200_000 {
            let value = dither.next();
            lowest = lowest.min(value);
            highest = highest.max(value);
            total += f64::from(value);
        }
        // Somme de deux uniformes sur [−0,5, 0,5) : bornée à ±1 LSB, centrée.
        assert!(lowest >= -1.0 && highest <= 1.0, "{lowest} .. {highest}");
        assert!(
            lowest < -0.7 && highest > 0.7,
            "la plage devrait être exploitée"
        );
        assert!(
            (total / 200_000.0).abs() < 0.01,
            "le bruit doit être centré"
        );
    }

    fn scratch_wav(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mixcanvas-{name}-{}.wav", std::process::id()))
    }

    #[test]
    fn the_wav_header_says_16_bit_44_1_khz_interleaved_stereo() {
        // Un décalage d'octet dans l'en-tête donne un fichier illisible, ou pire
        // un fichier lu au mauvais format. Les positions sont donc vérifiées une
        // à une plutôt que supposées.
        let path = scratch_wav("header");
        {
            let mut writer = BufWriter::new(File::create(&path).expect("file"));
            write_placeholder_header(&mut writer).expect("header");
            writer
                .write_all(&[0_u8; 8])
                .expect("four frames of silence");
            finish_header(&mut writer, 8).expect("sizes");
            writer.flush().expect("flush");
        }
        let bytes = std::fs::read(&path).expect("read back");
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32_at(16), 16, "PCM entier a un bloc fmt de 16 octets");
        assert_eq!(u16_at(20), 1, "format 1 = PCM entier");
        assert_eq!(u16_at(22), 2, "stéréo");
        assert_eq!(u32_at(24), 44_100);
        assert_eq!(u32_at(28), 44_100 * 4, "débit : 2 voies × 2 octets");
        assert_eq!(u16_at(32), 4, "alignement de bloc : une trame stéréo");
        assert_eq!(u16_at(34), 16, "16 bits");
        // Les deux tailles ne sont écrites qu'à la fin, par rembobinage.
        assert_eq!(u32_at(40), 8, "taille des données");
        assert_eq!(u32_at(4), 8 + 36, "taille RIFF = données + en-tête");
        assert_eq!(bytes.len(), 44 + 8);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_mix_that_starts_late_is_bounced_from_its_first_sound() {
        use crate::tempo::TempoMap;
        use crate::timeline::{TimelineRenderClip, TimelineRenderPlan};

        let plan = |first_beat: f64| TimelineRenderPlan {
            project_bpm: 120.0,
            tempo_map: TempoMap::new(120.0, Vec::new()).expect("a flat tempo map"),
            end_beat: 64.0,
            audible_lane_mask: 0b111,
            limiter_enabled: true,
            compressor_enabled: false,
            clips: vec![TimelineRenderClip {
                id: 1,
                lane: 0,
                file_path: String::new(),
                source_bpm: 120.0,
                first_beat_ms: 0,
                anchor_beat: first_beat,
                visual_start_beat: first_beat,
                duration_beats: 32.0,
                trim_start_beats: 0.0,
                trim_end_beats: 0.0,
                is_sidechain_key: false,
                eq_settings: None,
            }],
            volume_nodes: Vec::new(),
            pan_nodes: Vec::new(),
            filter_nodes: Vec::new(),
            reverb_nodes: Vec::new(),
            flanger_nodes: Vec::new(),
            bitcrush_nodes: Vec::new(),
            delay_nodes: Vec::new(),
        };

        // Huit temps à 120 BPM font quatre secondes : c'est ce que le bounce
        // doit écarter plutôt que d'exporter du silence.
        assert!((first_audible_seconds(&plan(8.0)) - 4.0).abs() < 1.0e-9);
        // Un mix qui commence au premier temps n'a rien à écarter.
        assert_eq!(first_audible_seconds(&plan(0.0)), 0.0);
    }

    #[test]
    fn the_dither_repeats_so_two_bounces_of_one_mix_match() {
        let first: Vec<f32> = {
            let mut d = TriangularDither::new();
            (0..64).map(|_| d.next()).collect()
        };
        let second: Vec<f32> = {
            let mut d = TriangularDither::new();
            (0..64).map(|_| d.next()).collect()
        };
        assert_eq!(first, second);
    }
}
