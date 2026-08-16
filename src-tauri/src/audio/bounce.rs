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

use mp3lame_encoder::{
    Bitrate, Builder, DualPcm, FlushNoGap, Mode, Quality, max_required_buffer_size,
};

use super::mastering::{MasteringLimiter, MasteringSettings};
use super::timeline::{OUTPUT_CHANNELS, prepare_timeline};
use crate::timeline::TimelineRenderPlan;

/// 44,1 kHz, la fréquence des MP3 sources. Rendre à cette valeur évite une
/// conversion de fréquence que la qualité n'aurait rien à y gagner.
pub const BOUNCE_SAMPLE_RATE: u32 = 44_100;
const BOUNCE_BITS_PER_SAMPLE: u16 = 16;
/// Pleine échelle en 16 bits signés. On divise par 32767 et non par 32768 pour
/// qu'un signal à −1,0 comme à +1,0 reste symétrique après conversion.
const FULL_SCALE: f32 = 32_767.0;

/// Ce qu'on écrit au bout.
///
/// Le WAV reste le défaut : c'est un master, et un master se garde sans perte.
/// Le MP3 est là pour ce qu'on envoie, et il n'a de sens qu'au débit et à la
/// qualité les plus hauts que LAME sache produire — un mix compressé deux fois
/// ne se rattrape pas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BounceFormat {
    #[default]
    Wav,
    Mp3,
}

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
    pub format: BounceFormat,
}

/// Là où finissent les images, une fois limitées.
///
/// Deux sorties très différentes — un PCM entrelacé qu'il faut quantifier, et
/// un encodeur qui préfère du flottant par canal — mais un seul chemin en
/// amont. Sans cette frontière, la boucle du rendu porterait deux fois la
/// gestion du limiteur et de la progression.
trait BounceSink {
    fn write_frame(&mut self, frame: &[f32]) -> Result<(), String>;
    fn finish(&mut self, frames: usize) -> Result<(), String>;
}

/// Le WAV : quantification en 16 bits, avec dither.
struct WavSink<W: Write + Seek> {
    writer: W,
    dither: TriangularDither,
    block: Vec<u8>,
}

impl<W: Write + Seek> BounceSink for WavSink<W> {
    fn write_frame(&mut self, frame: &[f32]) -> Result<(), String> {
        for sample in frame {
            let scaled = sample.clamp(-1.0, 1.0) * FULL_SCALE + self.dither.next();
            let quantised = scaled.round().clamp(-FULL_SCALE, FULL_SCALE) as i16;
            self.block.extend_from_slice(&quantised.to_le_bytes());
        }
        if self.block.len() >= 8_192 {
            self.writer
                .write_all(&self.block)
                .map_err(|error| format!("Writing the mix failed: {error}"))?;
            self.block.clear();
        }
        Ok(())
    }

    fn finish(&mut self, frames: usize) -> Result<(), String> {
        if !self.block.is_empty() {
            self.writer
                .write_all(&self.block)
                .map_err(|error| format!("Writing the mix failed: {error}"))?;
            self.block.clear();
        }
        finish_header(&mut self.writer, frames * OUTPUT_CHANNELS as usize * 2)?;
        self.writer
            .flush()
            .map_err(|error| format!("Writing the mix failed: {error}"))
    }
}

/// Le MP3 : LAME, au débit et à la qualité les plus hauts.
///
/// Le mix entre en **flottant**, par `lame_encode_buffer_ieee_float`, sans
/// passer par 16 bits. Quantifier avant de compresser ajouterait un bruit que
/// le codeur devrait ensuite dépenser des bits à conserver.
struct Mp3Sink<W: Write> {
    writer: W,
    encoder: mp3lame_encoder::Encoder,
    left: Vec<f32>,
    right: Vec<f32>,
    encoded: Vec<u8>,
}

/// Combien d'images sont accumulées avant d'appeler l'encodeur.
const MP3_BLOCK_FRAMES: usize = 4_096;

/// Ce que la fermeture de LAME réclame au minimum pour écrire sa dernière
/// trame et son remplissage.
const MP3_FLUSH_BYTES: usize = 7_200;

impl<W: Write> Mp3Sink<W> {
    fn drain(&mut self) -> Result<(), String> {
        if self.left.is_empty() {
            return Ok(());
        }
        self.encoded.clear();
        /* `encode_to_vec` écrit dans la **capacité libre** du vecteur et n'en
           réserve aucune : sans cette ligne, LAME reçoit un tampon de taille
           nulle. C'est ce qui faisait planter le rendu MP3 à la première
           image. */
        self.encoded.reserve(max_required_buffer_size(self.left.len()));
        self.encoder
            .encode_to_vec(
                DualPcm {
                    left: &self.left,
                    right: &self.right,
                },
                &mut self.encoded,
            )
            .map_err(|error| format!("The MP3 encoder failed: {error}"))?;
        self.writer
            .write_all(&self.encoded)
            .map_err(|error| format!("Writing the mix failed: {error}"))?;
        self.left.clear();
        self.right.clear();
        Ok(())
    }
}

impl<W: Write> BounceSink for Mp3Sink<W> {
    fn write_frame(&mut self, frame: &[f32]) -> Result<(), String> {
        self.left.push(frame[0].clamp(-1.0, 1.0));
        self.right.push(frame[frame.len().min(2) - 1].clamp(-1.0, 1.0));
        if self.left.len() >= MP3_BLOCK_FRAMES {
            self.drain()?;
        }
        Ok(())
    }

    fn finish(&mut self, _frames: usize) -> Result<(), String> {
        self.drain()?;
        self.encoded.clear();
        // La dernière trame plus le remplissage : le module en demande 7200.
        self.encoded.reserve(MP3_FLUSH_BYTES);
        self.encoder
            .flush_to_vec::<FlushNoGap>(&mut self.encoded)
            .map_err(|error| format!("The MP3 encoder failed to close: {error}"))?;
        self.writer
            .write_all(&self.encoded)
            .map_err(|error| format!("Writing the mix failed: {error}"))?;
        self.writer
            .flush()
            .map_err(|error| format!("Writing the mix failed: {error}"))
    }
}

/// L'encodeur, réglé une fois pour toutes.
///
/// 320 kbit/s constants et `Quality::Best`, c'est-à-dire `-q 0` : la recherche
/// psychoacoustique la plus poussée que LAME propose. Elle coûte du temps de
/// calcul, ce qui n'a aucune importance dans un rendu hors ligne.
///
/// `Mode::Stereo` plutôt que le stéréo joint que LAME choisirait seul : à
/// 320 kbit/s il n'y a aucune pression de débit qui justifierait de mutualiser
/// les deux canaux, et les passes d'effets de ce programme travaillent
/// justement l'image stéréo.
fn build_mp3_encoder() -> Result<mp3lame_encoder::Encoder, String> {
    let mut builder = Builder::new().ok_or_else(|| "The MP3 encoder could not start.".to_owned())?;
    fn refused(what: &'static str) -> impl Fn(mp3lame_encoder::BuildError) -> String {
        move |error| format!("The MP3 encoder refused {what}: {error:?}")
    }
    builder
        .set_num_channels(OUTPUT_CHANNELS as u8)
        .map_err(refused("the channel count"))?;
    builder
        .set_sample_rate(BOUNCE_SAMPLE_RATE)
        .map_err(refused("the sample rate"))?;
    builder
        .set_brate(Bitrate::Kbps320)
        .map_err(refused("the bitrate"))?;
    builder
        .set_mode(Mode::Stereo)
        .map_err(refused("stereo mode"))?;
    builder
        .set_quality(Quality::Best)
        .map_err(refused("the quality setting"))?;
    builder
        .build()
        .map_err(|error| format!("The MP3 encoder could not start: {error:?}"))
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
/// `mastering` remplace le garde-fou du moteur par un limiteur d'outil.
///
/// Les deux ne peuvent pas coexister dans ce rendu : le garde-fou borne à
/// 0,98, soit −0,18 dBFS, donc **en dessous** d'un plafond de mastering
/// ordinaire. Il serait le limiteur effectif, et son écrêtage franc
/// trancherait les transitoires avant que l'autre ne les voie. Le plan est
/// donc recopié sans lui — ce qui ne le retire pas du programme : il continue
/// de protéger l'écoute, qui est son métier.
pub fn bounce_timeline(
    plan: &TimelineRenderPlan,
    path: &Path,
    format: BounceFormat,
    mastering: Option<MasteringSettings>,
    on_progress: &mut dyn FnMut(f64),
) -> Result<BounceSummary, String> {
    if plan.clips.is_empty() {
        return Err("Add at least one clip before bouncing the mix.".to_owned());
    }

    let unlimited;
    let plan = if mastering.is_some() {
        unlimited = TimelineRenderPlan {
            limiter_enabled: false,
            ..plan.clone()
        };
        &unlimited
    } else {
        plan
    };
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
    // Seul le WAV a un en-tête à réserver puis à corriger une fois la taille
    // connue; un flux MP3 est une suite de trames et n'en a pas.
    if format == BounceFormat::Wav {
        write_placeholder_header(&mut writer)?;
    }

    // La source sait exactement combien d'échantillons il lui reste après le
    // repositionnement : la progression est mesurée, pas estimée.
    let expected_samples = source.len().max(1);
    let mut last_percent = usize::MAX;
    on_progress(0.0);

    // Écrit au fil de l'eau plutôt qu'accumulé : un mix d'une heure fait
    // 635 Mo en 16 bits stéréo, qu'il n'y a aucune raison de tenir en mémoire.
    let channels = OUTPUT_CHANNELS as usize;
    let mut limiter =
        mastering.map(|settings| MasteringLimiter::new(settings, BOUNCE_SAMPLE_RATE, channels));
    let mut sink: Box<dyn BounceSink> = match format {
        BounceFormat::Wav => Box::new(WavSink {
            writer,
            dither: TriangularDither::new(),
            block: Vec::with_capacity(8_192),
        }),
        BounceFormat::Mp3 => Box::new(Mp3Sink {
            writer,
            encoder: build_mp3_encoder()?,
            left: Vec::with_capacity(MP3_BLOCK_FRAMES),
            right: Vec::with_capacity(MP3_BLOCK_FRAMES),
            encoded: Vec::new(),
        }),
    };

    let mut frames = 0_usize;
    let mut frame = vec![0.0_f32; channels];
    let mut filled = 0_usize;
    let expected_frames = (expected_samples / channels).max(1);

    let mut push = |sink: &mut Box<dyn BounceSink>,
                    frames: &mut usize,
                    frame: &[f32]|
     -> Result<(), String> {
        sink.write_frame(frame)?;
        *frames += 1;
        let percent = *frames * 100 / expected_frames;
        if percent != last_percent {
            last_percent = percent;
            on_progress(*frames as f64 / expected_frames as f64);
        }
        Ok(())
    };

    for sample in source.by_ref() {
        frame[filled] = sample;
        filled += 1;
        if filled < channels {
            continue;
        }
        filled = 0;
        match limiter.as_mut() {
            // Par images entières : le limiteur décide d'un gain pour la crête
            // des deux canaux, faute de quoi une crête sur la gauche seule
            // ferait pivoter l'image stéréo.
            Some(limiter) => {
                if limiter.process(&mut frame) {
                    push(&mut sink, &mut frames, &frame)?;
                }
            }
            None => push(&mut sink, &mut frames, &frame)?,
        }
    }
    // Ce que la ligne à retard tient encore. Sans cette vidange, le mix
    // perdrait ses trois dernières millisecondes.
    if let Some(limiter) = limiter.as_mut() {
        for tail in limiter.flush().chunks_exact(channels) {
            push(&mut sink, &mut frames, tail)?;
        }
    }

    sink.finish(frames)?;
    on_progress(1.0);

    Ok(BounceSummary {
        path: path.to_string_lossy().into_owned(),
        frames,
        duration_seconds: frames as f64 / f64::from(BOUNCE_SAMPLE_RATE),
        trimmed_seconds,
        sample_rate: BOUNCE_SAMPLE_RATE,
        // Un MP3 n'a pas de profondeur : il ne stocke pas d'échantillons mais
        // des coefficients. Zéro le dit mieux que seize.
        bits_per_sample: match format {
            BounceFormat::Wav => BOUNCE_BITS_PER_SAMPLE,
            BounceFormat::Mp3 => 0,
        },
        format,
    })
}

/// En-tête RIFF avec des tailles provisoires.
///
/// Elles ne sont connues qu'une fois le rendu terminé, et les deviner
/// obligerait à garder tout l'audio en mémoire pour le compter.
fn write_placeholder_header<W: Write>(writer: &mut W) -> Result<(), String> {
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

fn finish_header<W: Write + Seek>(writer: &mut W, data_bytes: usize) -> Result<(), String> {
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

#[cfg(test)]
mod mp3_tests {
    use super::*;

    /// Ce que l'en-tête d'une trame MP3 annonce réellement.
    ///
    /// Quatre octets, et ils suffisent à démentir ou confirmer les réglages :
    /// un débit, une fréquence et un mode qui ne seraient pas ceux demandés
    /// expliqueraient à eux seuls une perte audible.
    #[derive(Debug, PartialEq)]
    struct FrameHeader {
        bitrate_kbps: u32,
        sample_rate: u32,
        channel_mode: &'static str,
    }

    fn first_frame_header(bytes: &[u8]) -> Option<FrameHeader> {
        const BITRATES: [u32; 15] = [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ];
        const RATES: [u32; 3] = [44_100, 48_000, 32_000];
        let mut index = 0;
        while index + 4 <= bytes.len() {
            let head = &bytes[index..index + 4];
            // Synchronisation, MPEG-1, Layer III.
            let synced = head[0] == 0xFF && (head[1] & 0xE0) == 0xE0;
            if synced && (head[1] & 0x18) == 0x18 && (head[1] & 0x06) == 0x02 {
                let bitrate = BITRATES[usize::from(head[2] >> 4)];
                let rate = RATES[usize::from((head[2] >> 2) & 0b11).min(2)];
                let mode = match (head[3] >> 6) & 0b11 {
                    0 => "stereo",
                    1 => "joint",
                    2 => "dual",
                    _ => "mono",
                };
                if bitrate > 0 {
                    return Some(FrameHeader {
                        bitrate_kbps: bitrate,
                        sample_rate: rate,
                        channel_mode: mode,
                    });
                }
            }
            index += 1;
        }
        None
    }

    fn encode_seconds(seconds: f32) -> Vec<u8> {
        let frames = (BOUNCE_SAMPLE_RATE as f32 * seconds) as usize;
        let mut out: Vec<u8> = Vec::new();
        let mut sink = Mp3Sink {
            writer: &mut out,
            encoder: build_mp3_encoder().expect("the encoder should start"),
            left: Vec::with_capacity(MP3_BLOCK_FRAMES),
            right: Vec::with_capacity(MP3_BLOCK_FRAMES),
            encoded: Vec::new(),
        };
        for index in 0..frames {
            let t = index as f32 / BOUNCE_SAMPLE_RATE as f32;
            // Un grave franc et un aigu discret, pour que les deux se voient.
            let low = (std::f32::consts::TAU * 60.0 * t).sin() * 0.6;
            let high = (std::f32::consts::TAU * 9_000.0 * t).sin() * 0.15;
            sink.write_frame(&[low + high, low + high])
                .expect("a frame should encode");
        }
        sink.finish(frames).expect("the stream should close");
        out
    }

    /// Encode un ton pur, le redécode, et rend son niveau efficace.
    ///
    /// La mesure saute le début : encodeur et décodeur ajoutent chacun leur
    /// retard, et les premières dizaines de millisecondes ne sont pas du
    /// signal mais du remplissage.
    fn round_trip_rms(hz: f32, quality: Quality) -> f32 {
        use std::io::Cursor;

        let frames = BOUNCE_SAMPLE_RATE as usize * 2;
        let mut bytes: Vec<u8> = Vec::new();
        {
            let mut builder = Builder::new().expect("a builder");
            builder.set_num_channels(2).unwrap();
            builder.set_sample_rate(BOUNCE_SAMPLE_RATE).unwrap();
            builder.set_brate(Bitrate::Kbps320).unwrap();
            builder.set_mode(Mode::Stereo).unwrap();
            builder.set_quality(quality).unwrap();
            let mut sink = Mp3Sink {
                writer: &mut bytes,
                encoder: builder.build().expect("an encoder"),
                left: Vec::with_capacity(MP3_BLOCK_FRAMES),
                right: Vec::with_capacity(MP3_BLOCK_FRAMES),
                encoded: Vec::new(),
            };
            for index in 0..frames {
                let t = index as f32 / BOUNCE_SAMPLE_RATE as f32;
                let value = (std::f32::consts::TAU * hz * t).sin() * 0.5;
                sink.write_frame(&[value, value]).expect("a frame");
            }
            sink.finish(frames).expect("a closed stream");
        }

        let decoder = rodio::Decoder::new(Cursor::new(bytes)).expect("the MP3 should decode");
        let decoded: Vec<f32> = decoder.collect();
        let skip = BOUNCE_SAMPLE_RATE as usize / 2;
        let window: Vec<f32> = decoded
            .iter()
            .skip(skip)
            .take(BOUNCE_SAMPLE_RATE as usize)
            .copied()
            .collect();
        assert!(!window.is_empty(), "rien n'est ressorti du décodeur");
        (window.iter().map(|v| v * v).sum::<f32>() / window.len() as f32).sqrt()
    }

    /// Un 320 CBR doit rendre un ton pur à son niveau, grave comme aigu.
    ///
    /// Le niveau efficace d'une sinusoïde d'amplitude 0,5 vaut 0,3536. Une
    /// perte franche à 60 Hz serait le symptôme que Sébastien décrit, et elle
    /// ne s'expliquerait pas par la compression.
    /// Du matériel large bande, plus proche d'un mix qu'un ton pur.
    fn programme_material(frames: usize) -> Vec<[f32; 2]> {
        let mut state = 0x1234_5678_u32;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        (0..frames)
            .map(|index| {
                let t = index as f32 / BOUNCE_SAMPLE_RATE as f32;
                // Un grave soutenu, une frappe périodique, et du bruit.
                let bass = (std::f32::consts::TAU * 55.0 * t).sin() * 0.35;
                let beat = if index % 22_050 < 400 { noise() * 0.5 } else { 0.0 };
                let air = noise() * 0.05;
                let left = (bass + beat + air).clamp(-1.0, 1.0);
                let right = (bass + beat + air * 0.8).clamp(-1.0, 1.0);
                [left, right]
            })
            .collect()
    }

    /// L'énergie sous 200 Hz et au-dessus de 8 kHz, par filtrage à un pôle.
    fn band_energies(samples: &[f32]) -> (f32, f32) {
        let rate = BOUNCE_SAMPLE_RATE as f32;
        let coefficient = |hz: f32| (-std::f32::consts::TAU * hz / rate).exp();
        let (low_c, high_c) = (coefficient(200.0), coefficient(8_000.0));
        let (mut low, mut high) = (0.0_f32, 0.0_f32);
        let (mut low_sum, mut high_sum) = (0.0_f64, 0.0_f64);
        for &sample in samples {
            low = sample * (1.0 - low_c) + low * low_c;
            high = sample * (1.0 - high_c) + high * high_c;
            low_sum += f64::from(low * low);
            high_sum += f64::from((sample - high) * (sample - high));
        }
        let n = samples.len().max(1) as f64;
        (
            (low_sum / n).sqrt() as f32,
            (high_sum / n).sqrt() as f32,
        )
    }

    /// La comparaison que fait l'oreille : le même mix, en WAV et en MP3.
    ///
    /// Le grave est ce que Sébastien dit perdre. S'il manque ici, le défaut
    /// est chez nous; s'il est là, c'est ailleurs qu'il faut chercher.
    #[test]
    fn the_mp3_keeps_the_same_low_and_high_energy_as_the_wav() {
        use std::io::Cursor;

        let frames = BOUNCE_SAMPLE_RATE as usize * 3;
        let material = programme_material(frames);

        // La voie WAV, sans quantifier : c'est la référence.
        let reference: Vec<f32> = material.iter().flat_map(|f| [f[0], f[1]]).collect();

        let mut bytes: Vec<u8> = Vec::new();
        {
            let mut sink = Mp3Sink {
                writer: &mut bytes,
                encoder: build_mp3_encoder().expect("an encoder"),
                left: Vec::with_capacity(MP3_BLOCK_FRAMES),
                right: Vec::with_capacity(MP3_BLOCK_FRAMES),
                encoded: Vec::new(),
            };
            for frame in &material {
                sink.write_frame(frame).expect("a frame");
            }
            sink.finish(frames).expect("a closed stream");
        }
        let decoder = rodio::Decoder::new(Cursor::new(bytes)).expect("the MP3 should decode");
        let decoded: Vec<f32> = decoder.collect();

        let skip = BOUNCE_SAMPLE_RATE as usize;
        let take = BOUNCE_SAMPLE_RATE as usize;
        let cut = |data: &[f32]| data.iter().skip(skip).take(take).copied().collect::<Vec<_>>();
        let (wav_low, wav_high) = band_energies(&cut(&reference));
        let (mp3_low, mp3_high) = band_energies(&cut(&decoded));

        let low_db = 20.0 * (mp3_low / wav_low).log10();
        let high_db = 20.0 * (mp3_high / wav_high).log10();
        println!("grave : {low_db:+.2} dB   aigu : {high_db:+.2} dB");
        assert!(
            low_db.abs() < 1.0,
            "le grave du MP3 s'écarte de {low_db:+.2} dB du WAV"
        );
        assert!(
            high_db.abs() < 3.0,
            "l'aigu du MP3 s'écarte de {high_db:+.2} dB du WAV"
        );
    }

    #[test]
    fn a_pure_tone_survives_the_round_trip_at_every_frequency() {
        let expected = 0.5 / std::f32::consts::SQRT_2;
        for hz in [40.0, 60.0, 120.0, 1_000.0, 9_000.0, 14_000.0] {
            let rms = round_trip_rms(hz, Quality::Best);
            let db = 20.0 * (rms / expected).log10();
            assert!(
                db > -1.5,
                "{hz} Hz ressort {db:.2} dB sous son niveau (rms {rms:.4})"
            );
        }
    }

    #[test]
    fn the_encoder_writes_what_the_dialog_promises() {
        let bytes = encode_seconds(0.5);
        assert!(!bytes.is_empty(), "l'encodeur n'a rien écrit");
        let header = first_frame_header(&bytes).expect("a readable MPEG-1 Layer III frame");
        assert_eq!(
            header,
            FrameHeader {
                bitrate_kbps: 320,
                sample_rate: 44_100,
                channel_mode: "stereo",
            }
        );
    }

    /// Un demi-seconde à 320 kbit/s pèse environ vingt kilo-octets.
    ///
    /// Bien moins voudrait dire que des blocs se perdent en route — ce qui
    /// s'entendrait comme une perte de matière, pas comme de la compression.
    #[test]
    fn the_stream_weighs_what_its_bitrate_says() {
        let bytes = encode_seconds(1.0);
        let expected = 320_000 / 8;
        let ratio = bytes.len() as f64 / f64::from(expected);
        assert!(
            (0.9..1.1).contains(&ratio),
            "une seconde pèse {} octets, soit {ratio:.2} fois les {expected} attendus",
            bytes.len()
        );
    }
}
