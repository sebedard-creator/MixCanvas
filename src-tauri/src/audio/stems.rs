//! Séparation d'un morceau en deux voix : le chant et le reste.
//!
//! Le modèle ne voit jamais le signal. Il reçoit un **spectrogramme
//! d'amplitude** et rend un masque de même forme; toute la transformée vit ici.
//! C'est ce qui a décidé du choix du modèle : la branche spectrale de Demucs est
//! bâtie sur des nombres complexes, qu'ONNX ne sait pas représenter, alors
//! qu'open-unmix laisse cette partie dehors.
//!
//! L'instrumental n'est pas prédit, il est **déduit** : `mix − voix`, dans le
//! domaine temporel. Deux avantages qui valent d'être dits — le fichier du
//! modèle est deux fois plus petit puisqu'une seule cible est apprise, et les
//! deux stems se resomment exactement en l'original, ce qu'une seconde
//! prédiction ne garantirait jamais.

use std::f32::consts::PI;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rodio::Source;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

use crate::analysis::{WAVEFORM_BUCKET_COUNT, WaveformPeaks};
use crate::audio::open_mp3_decoder;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// Taille de la fenêtre d'analyse, en échantillons. Celle du modèle entraîné :
/// la changer reviendrait à lui présenter un spectre qu'il n'a jamais vu.
pub(crate) const FFT_SIZE: usize = 4096;
/// Avancement entre deux trames. Quatre trames par fenêtre, soit le recouvrement
/// qui rend la fenêtre de Hann exactement reconstructible.
pub(crate) const HOP_SIZE: usize = 1024;
/// Bandes d'un spectre réel : la moitié de la fenêtre, plus la composante
/// continue.
pub(crate) const BIN_COUNT: usize = FFT_SIZE / 2 + 1;
/// Trames par appel au modèle, figées à l'export du graphe.
pub(crate) const FRAMES_PER_CHUNK: usize = 256;

/// Fenêtre de Hann **périodique**, celle du modèle.
///
/// La variante symétrique — `cos(2πn/(N−1))` — s'en écarte d'un échantillon, ce
/// qui suffit à casser la reconstruction parfaite en recouvrement : la somme des
/// carrés cesse d'être constante et laisse une ondulation au rythme du pas
/// d'avancement.
fn hann_window() -> Vec<f32> {
    (0..FFT_SIZE)
        .map(|index| {
            let phase = 2.0 * PI * index as f32 / FFT_SIZE as f32;
            0.5 - 0.5 * phase.cos()
        })
        .collect()
}

/// Un spectrogramme : une suite de trames, chacune de `BIN_COUNT` bandes
/// complexes, pour un seul canal.
pub(crate) struct Spectrogram {
    pub(crate) frames: usize,
    /// `frames × BIN_COUNT`, en ordre trame par trame.
    pub(crate) bins: Vec<Complex32>,
}

impl Spectrogram {
    /// Les amplitudes, dans l'ordre du spectrogramme.
    ///
    /// La séparation ne passe pas par là : elle transpose vers l'ordre du
    /// modèle en lisant les normes au vol, ce qui évite un tableau
    /// intermédiaire par tranche. Cette forme-ci reste celle contre laquelle
    /// les tests sont écrits, parce qu'elle se lit.
    #[cfg(test)]
    pub(crate) fn magnitudes(&self) -> Vec<f32> {
        self.bins.iter().map(|bin| bin.norm()).collect()
    }

    /// Applique un masque d'amplitude en gardant la phase d'origine.
    ///
    /// La phase du mélange est réutilisée telle quelle. C'est l'approximation
    /// que fait toute séparation par masque, et elle s'entend surtout là où deux
    /// sources se superposent exactement; la reconstruire coûterait bien plus
    /// cher que ce qu'elle rapporte ici.
    pub(crate) fn masked(&self, magnitudes: &[f32]) -> Spectrogram {
        debug_assert_eq!(magnitudes.len(), self.bins.len());
        let bins = self
            .bins
            .iter()
            .zip(magnitudes)
            .map(|(bin, target)| {
                let magnitude = bin.norm();
                if magnitude <= f32::EPSILON {
                    Complex32::new(0.0, 0.0)
                } else {
                    // Le rapport plutôt que la phase reconstruite : une
                    // multiplication garde le signe et la phase sans repasser
                    // par un arc tangente.
                    bin * (target / magnitude)
                }
            })
            .collect();
        Spectrogram {
            frames: self.frames,
            bins,
        }
    }
}

/// La transformée et son inverse, avec leurs plans réutilisables.
///
/// Les plans coûtent cher à construire et rien à garder : une séparation en fait
/// des milliers d'appels.
pub(crate) struct StftPlan {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
}

impl StftPlan {
    pub(crate) fn new() -> Self {
        let mut planner = FftPlanner::new();
        Self {
            forward: planner.plan_fft_forward(FFT_SIZE),
            inverse: planner.plan_fft_inverse(FFT_SIZE),
            window: hann_window(),
        }
    }

    /// Nombre de trames pour un signal d'une longueur donnée.
    pub(crate) fn frames_for(&self, samples: usize) -> usize {
        // Le signal est bordé d'une demi-fenêtre de chaque côté, de sorte que
        // son premier et son dernier échantillon soient au centre d'une trame
        // et non à son bord — sans quoi les deux extrémités du morceau
        // ressortiraient atténuées.
        samples.div_ceil(HOP_SIZE) + 1
    }

    fn padded(&self, samples: &[f32], frames: usize) -> Vec<f32> {
        let half = FFT_SIZE / 2;
        let mut padded = vec![0.0_f32; half + samples.len() + FFT_SIZE + frames * HOP_SIZE];
        padded[half..half + samples.len()].copy_from_slice(samples);
        padded
    }

    /// Le signal bordé, prêt à être découpé en trames. Rendu au lieu d'être
    /// recalculé à chaque tranche : une séparation le parcourt des centaines de
    /// fois.
    pub(crate) fn pad(&self, samples: &[f32]) -> Vec<f32> {
        let frames = self.frames_for(samples.len());
        self.padded(samples, frames)
    }

    /// Analyse une **portion** de trames d'un signal déjà bordé.
    ///
    /// C'est cette forme que la séparation emploie. Le spectrogramme complet
    /// d'un morceau de cinq minutes pèse deux cents mégaoctets par canal : le
    /// tenir en entier pour n'en regarder que deux cent cinquante-six trames à
    /// la fois serait payer quatre cents mégaoctets pour rien.
    pub(crate) fn forward_frames(
        &self,
        padded: &[f32],
        first_frame: usize,
        frames: usize,
    ) -> Spectrogram {
        let mut bins = Vec::with_capacity(frames * BIN_COUNT);
        let mut scratch = vec![Complex32::new(0.0, 0.0); FFT_SIZE];

        for frame in 0..frames {
            let start = (first_frame + frame) * HOP_SIZE;
            for (index, slot) in scratch.iter_mut().enumerate() {
                let sample = padded.get(start + index).copied().unwrap_or(0.0);
                *slot = Complex32::new(sample * self.window[index], 0.0);
            }
            self.forward.process(&mut scratch);
            bins.extend_from_slice(&scratch[..BIN_COUNT]);
        }

        Spectrogram { frames, bins }
    }

    /// Le morceau entier d'un coup.
    ///
    /// La séparation travaille par tranches — un spectrogramme complet pèse
    /// deux cents mégaoctets — mais cette forme dit ce que les tranches doivent
    /// donner, et c'est contre elle qu'elles sont vérifiées.
    #[cfg(test)]
    pub(crate) fn forward(&self, samples: &[f32]) -> Spectrogram {
        let frames = self.frames_for(samples.len());
        let padded = self.padded(samples, frames);
        self.forward_frames(&padded, 0, frames)
    }

    /// Longueur des accumulateurs pour un signal d'une longueur donnée.
    pub(crate) fn scratch_length(&self, samples: usize) -> usize {
        FFT_SIZE / 2 + samples + FFT_SIZE + self.frames_for(samples) * HOP_SIZE
    }

    /// Reconstruit une portion et l'ajoute aux accumulateurs.
    ///
    /// Signal et poids sont tenus par l'appelant, de sorte que des tranches
    /// successives se recouvrent proprement : c'est ce recouvrement qui fait la
    /// reconstruction, et il ne peut pas exister à l'intérieur d'une tranche.
    pub(crate) fn inverse_into(
        &self,
        spectrogram: &Spectrogram,
        first_frame: usize,
        signal: &mut [f32],
        weight: &mut [f32],
    ) {
        let mut scratch = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        for frame in 0..spectrogram.frames {
            let start = (first_frame + frame) * HOP_SIZE;
            if start + FFT_SIZE > signal.len() {
                break;
            }
            let offset = frame * BIN_COUNT;
            // Le spectre réel est reconstitué par symétrie hermitienne : la
            // moitié haute est le conjugué de la moitié basse, en miroir.
            for (bin, slot) in scratch.iter_mut().enumerate() {
                *slot = if bin < BIN_COUNT {
                    spectrogram.bins[offset + bin]
                } else {
                    spectrogram.bins[offset + FFT_SIZE - bin].conj()
                };
            }
            self.inverse.process(&mut scratch);
            for (index, value) in scratch.iter().enumerate() {
                let sample = value.re / FFT_SIZE as f32;
                signal[start + index] += sample * self.window[index];
                weight[start + index] += self.window[index] * self.window[index];
            }
        }
    }

    /// Ramène les accumulateurs au signal : division par le poids des fenêtres,
    /// puis retrait de la bordure.
    pub(crate) fn finish(&self, signal: Vec<f32>, weight: Vec<f32>, samples: usize) -> Vec<f32> {
        signal
            .into_iter()
            .zip(weight)
            .skip(FFT_SIZE / 2)
            .take(samples)
            .map(|(value, weight)| if weight > 1e-8 { value / weight } else { 0.0 })
            .collect()
    }

    /// Reconstruit le signal, borné à la longueur demandée.
    ///
    /// La somme des carrés de la fenêtre est divisée à la fin plutôt que
    /// supposée constante : elle l'est au milieu, elle ne l'est pas aux deux
    /// premières et dernières trames, et c'est précisément là qu'une division
    /// omise laisse un fondu que personne n'a demandé.
    #[cfg(test)]
    pub(crate) fn inverse(&self, spectrogram: &Spectrogram, samples: usize) -> Vec<f32> {
        let length = self.scratch_length(samples);
        let mut signal = vec![0.0_f32; length];
        let mut weight = vec![0.0_f32; length];
        self.inverse_into(spectrogram, 0, &mut signal, &mut weight);
        self.finish(signal, weight, samples)
    }
}

/// Ce que la séparation a produit.
pub struct StemFiles {
    pub vocals: PathBuf,
    pub instrumental: PathBuf,
    /// L'instant de la source où ces fichiers commencent. Le plan de rendu
    /// décale le premier temps d'autant, faute de quoi le clip jouerait à côté
    /// de sa grille.
    pub source_from_ms: f64,
    /// Les crêtes de chaque stem, **à l'échelle du morceau entier**.
    ///
    /// Un clip place sa forme d'onde à partir de la géométrie de la source. Des
    /// crêtes calculées sur la seule fenêtre séparée s'y étaleraient comme si
    /// elles couvraient tout le morceau, et le dessin ne correspondrait plus à
    /// ce qu'on entend. Elles sont donc rangées aux mêmes cases que celles de la
    /// source, silencieuses hors de la fenêtre — ce qui est honnête, puisque
    /// c'est exactement ce que le clip jouerait si on l'allongeait.
    pub vocals_waveform: WaveformPeaks,
    pub instrumental_waveform: WaveformPeaks,
}

/// Range des échantillons dans les cases d'une forme d'onde du morceau entier.
///
/// `first_sample` est l'endroit de la source où la fenêtre commence, et
/// `source_samples` la longueur du morceau : les cases hors fenêtre restent à
/// zéro.
fn peaks_over_source(
    left: &[f32],
    right: &[f32],
    first_sample: usize,
    source_samples: usize,
) -> WaveformPeaks {
    let buckets = WAVEFORM_BUCKET_COUNT;
    let mut peaks = WaveformPeaks {
        left_min: vec![0.0; buckets],
        left_max: vec![0.0; buckets],
        left_rms: vec![0.0; buckets],
        right_min: vec![0.0; buckets],
        right_max: vec![0.0; buckets],
        right_rms: vec![0.0; buckets],
    };
    let total = source_samples.max(1);
    let per_bucket = (total as f64 / buckets as f64).max(1.0);

    for bucket in 0..buckets {
        let from = (bucket as f64 * per_bucket) as usize;
        let to = (((bucket + 1) as f64 * per_bucket) as usize).min(total);
        // La part de cette case qui tombe dans la fenêtre séparée.
        let start = from.saturating_sub(first_sample);
        let end = to.saturating_sub(first_sample);
        if to <= first_sample || start >= left.len() {
            continue;
        }
        let end = end.min(left.len());
        if start >= end {
            continue;
        }

        let mut sum_left = 0.0_f64;
        let mut sum_right = 0.0_f64;
        for index in start..end {
            let value = left[index];
            peaks.left_min[bucket] = peaks.left_min[bucket].min(value);
            peaks.left_max[bucket] = peaks.left_max[bucket].max(value);
            sum_left += f64::from(value) * f64::from(value);
            let value = right[index];
            peaks.right_min[bucket] = peaks.right_min[bucket].min(value);
            peaks.right_max[bucket] = peaks.right_max[bucket].max(value);
            sum_right += f64::from(value) * f64::from(value);
        }
        let count = (end - start) as f64;
        peaks.left_rms[bucket] = (sum_left / count).sqrt() as f32;
        peaks.right_rms[bucket] = (sum_right / count).sqrt() as f32;
    }
    peaks
}

/// Marge prise de chaque côté de la fenêtre d'un clip.
///
/// Deux raisons : le modèle décide mieux avec du contexte autour de ce qu'il
/// sépare, et un rognage repoussé de quelques mesures après coup ne doit pas
/// retomber dans le silence.
const STEM_MARGIN_MS: f64 = 4_000.0;

/// Part de la barre réservée au décodage.
///
/// Une barre qui reste à zéro pendant une minute puis file en dix secondes ne
/// renseigne sur rien. Le décodage est une vraie partie du travail : il occupe
/// donc une vraie partie de la barre.
const DECODE_SHARE: f64 = 0.25;

/// La fréquence à laquelle le modèle a été entraîné.
///
/// Un spectre analysé à une autre fréquence range les mêmes sons dans d'autres
/// bandes : le modèle chercherait une voix là où il n'y en a pas. Un morceau
/// qui arrive à une autre fréquence est donc **amené** à celle-ci, et non plus
/// refusé — la timeline accepte le 48 kHz, la séparation devait suivre.
const REQUIRED_SAMPLE_RATE: u32 = 44_100;

/// Amène un canal à la fréquence du modèle.
///
/// Interpolation sinc plutôt qu'une interpolation linéaire écrite ici : le
/// résultat s'écoute, et un rééchantillonneur maison se paierait en repliement
/// dans l'aigu. Les paramètres sont ceux dont `beat-this` se sert déjà avec la
/// même version de la caisse.
///
/// Chaque canal passe séparément. Deux instances identiques appliquent le même
/// retard, donc l'image stéréo ne bouge pas — et c'est plus simple que
/// d'entrelacer pour désentrelacer ensuite.
fn resample_channel(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>, String> {
    if from == to {
        return Ok(samples.to_vec());
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let parameters = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let frames = samples.len();
    let mut resampler = Async::<f32>::new_sinc(
        f64::from(to) / f64::from(from),
        2.0,
        &parameters,
        frames,
        1,
        FixedAsync::Input,
    )
    .map_err(|error| format!("The resampler could not be built: {error}"))?;
    let input = InterleavedSlice::new(samples, 1, frames)
        .map_err(|error| format!("The resampler rejected the audio: {error:?}"))?;
    let output = resampler
        .process(&input, 0, None)
        .map_err(|error| format!("The audio could not be resampled: {error}"))?;
    Ok(output.take_data())
}

/// Sépare un morceau en deux fichiers : la voix, et tout le reste.
///
/// `on_progress` reçoit une fraction de 0 à 1. Un rendu de plusieurs minutes
/// sans retour visible passe pour un gel.
pub fn separate_track(
    source: &Path,
    runtime: &Path,
    model: &Path,
    output_dir: &Path,
    window: Option<(f64, f64)>,
    name: &str,
    mut on_progress: impl FnMut(f64),
) -> Result<StemFiles, String> {
    // Les deux gardes d'abord : `ort` charge sa bibliothèque à la première
    // fonction appelée et **panique** si elle manque, ce qui tuerait le
    // programme au lieu de renvoyer une erreur.
    if !runtime.is_file() {
        return Err(
            "The ONNX Runtime library is missing from this install — reinstall MixCanvas."
                .to_owned(),
        );
    }
    if !model.is_file() {
        return Err(
            "The separation model is missing from this install — reinstall MixCanvas.".to_owned(),
        );
    }
    unsafe { std::env::set_var("ORT_DYLIB_PATH", runtime) };

    // Le décodage est ce qui gelait la barre : il traverse le MP3 entier avant
    // que la première tranche n'existe, et sur un morceau de six minutes cela
    // dure plus longtemps que la séparation elle-même. Il rapporte donc son
    // avancement, et il s'arrête à la fin de la fenêtre au lieu de lire la
    // queue du fichier pour rien.
    let stop_after_ms = window.map(|(_, end_ms)| end_ms + STEM_MARGIN_MS);
    let (mut channels, decoded_rate, source_ms) =
        decode_stereo(source, stop_after_ms, &mut |fraction| {
            on_progress(fraction * DECODE_SHARE)
        })?;
    // Tout ce qui suit compte en échantillons **du modèle**. Le
    // rééchantillonnage a donc lieu ici, avant la moindre conversion de
    // millisecondes en indices : plus bas, une seule de ces conversions restée
    // à l'ancienne fréquence décalerait la fenêtre du clip.
    for channel in &mut channels {
        *channel = resample_channel(channel, decoded_rate, REQUIRED_SAMPLE_RATE)?;
    }
    let sample_rate = REQUIRED_SAMPLE_RATE;
    // La fenêtre du clip, pas le morceau entier : séparer six minutes pour huit
    // mesures utilisées coûterait vingt fois le travail nécessaire. Une marge
    // est prise de chaque côté — le modèle a besoin de contexte pour décider, et
    // un rognage repoussé de quelques mesures ne doit pas retomber dans le vide.
    let source_samples = source_ms / 1000.0 * sample_rate as f64;
    let source_samples = if source_samples > 0.0 {
        source_samples as usize
    } else {
        channels[0].len()
    };
    let mut first_sample = 0_usize;
    let mut from_ms = 0.0_f64;
    if let Some((start_ms, end_ms)) = window {
        let total = channels[0].len();
        let first = ((start_ms - STEM_MARGIN_MS).max(0.0) / 1000.0 * sample_rate as f64) as usize;
        let last = (((end_ms + STEM_MARGIN_MS) / 1000.0 * sample_rate as f64) as usize).min(total);
        if first >= last {
            return Err("This clip has no audio to separate.".to_owned());
        }
        for channel in &mut channels {
            *channel = channel[first..last].to_vec();
        }
        first_sample = first;
        from_ms = first as f64 * 1000.0 / sample_rate as f64;
    }

    let samples = channels[0].len();
    if samples == 0 {
        return Err("This track has no audio to separate.".to_owned());
    }

    let plan = StftPlan::new();
    let frames = plan.frames_for(samples);
    let padded: Vec<Vec<f32>> = channels.iter().map(|channel| plan.pad(channel)).collect();
    let length = plan.scratch_length(samples);
    let mut signal = vec![vec![0.0_f32; length], vec![0.0_f32; length]];
    let mut weight = vec![vec![0.0_f32; length], vec![0.0_f32; length]];

    let mut session = ort::session::Session::builder()
        .and_then(|mut builder| builder.commit_from_file(model))
        .map_err(|error| format!("The separation model could not be loaded: {error}"))?;

    let chunks = frames.div_ceil(FRAMES_PER_CHUNK);
    let mut input = vec![0.0_f32; 2 * BIN_COUNT * FRAMES_PER_CHUNK];
    for chunk in 0..chunks {
        let first_frame = chunk * FRAMES_PER_CHUNK;
        let count = FRAMES_PER_CHUNK.min(frames - first_frame);

        let spectra: Vec<Spectrogram> = padded
            .iter()
            .map(|channel| plan.forward_frames(channel, first_frame, FRAMES_PER_CHUNK))
            .collect();

        // Le modèle veut (lot, canal, bande, trame); le spectrogramme est rangé
        // trame par trame. La transposition se fait ici, une fois, plutôt que
        // dans la boucle qui lit le masque.
        input.fill(0.0);
        for (channel, spectrum) in spectra.iter().enumerate() {
            for frame in 0..FRAMES_PER_CHUNK {
                for bin in 0..BIN_COUNT {
                    let source = frame * BIN_COUNT + bin;
                    let target = (channel * BIN_COUNT + bin) * FRAMES_PER_CHUNK + frame;
                    input[target] = spectrum.bins[source].norm();
                }
            }
        }

        let shape = [1_usize, 2, BIN_COUNT, FRAMES_PER_CHUNK];
        let tensor = ort::value::TensorRef::from_array_view((shape, input.as_slice()))
            .map_err(|error| format!("The separation input was rejected: {error}"))?;
        let outputs = session
            .run(ort::inputs![tensor])
            .map_err(|error| format!("The separation failed: {error}"))?;
        let (_, mask) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|error| format!("The separation output could not be read: {error}"))?;

        for (channel, spectrum) in spectra.iter().enumerate() {
            let mut magnitudes = vec![0.0_f32; FRAMES_PER_CHUNK * BIN_COUNT];
            for frame in 0..FRAMES_PER_CHUNK {
                for bin in 0..BIN_COUNT {
                    let source = (channel * BIN_COUNT + bin) * FRAMES_PER_CHUNK + frame;
                    magnitudes[frame * BIN_COUNT + bin] = mask[source].max(0.0);
                }
            }
            plan.inverse_into(
                &spectrum.masked(&magnitudes),
                first_frame,
                &mut signal[channel],
                &mut weight[channel],
            );
        }

        let _ = count;
        on_progress(DECODE_SHARE + (1.0 - DECODE_SHARE) * (chunk + 1) as f64 / chunks as f64);
    }

    let vocals: Vec<Vec<f32>> = signal
        .into_iter()
        .zip(weight)
        .map(|(signal, weight)| plan.finish(signal, weight, samples))
        .collect();

    let vocals_path = output_dir.join(format!("{name} [vocals].wav"));
    let instrumental_path = output_dir.join(format!("{name} [instrumental].wav"));

    std::fs::create_dir_all(output_dir)
        .map_err(|error| format!("The stems folder could not be created: {error}"))?;
    write_stereo_wav(&vocals_path, &vocals[0], &vocals[1])?;

    // L'instrumental est ce qui reste, littéralement. Les deux fichiers se
    // resomment donc exactement en l'original.
    let left: Vec<f32> = channels[0]
        .iter()
        .zip(&vocals[0])
        .map(|(mix, vocals)| mix - vocals)
        .collect();
    let right: Vec<f32> = channels[1]
        .iter()
        .zip(&vocals[1])
        .map(|(mix, vocals)| mix - vocals)
        .collect();
    write_stereo_wav(&instrumental_path, &left, &right)?;

    Ok(StemFiles {
        vocals_waveform: peaks_over_source(&vocals[0], &vocals[1], first_sample, source_samples),
        instrumental_waveform: peaks_over_source(&left, &right, first_sample, source_samples),
        vocals: vocals_path,
        instrumental: instrumental_path,
        source_from_ms: from_ms,
    })
}

/// Écrit un WAV entier 16 bits à 44,1 kHz, entrelacé.
///
/// Le même format que le bounce, et pour la même raison : c'est ce que toute
/// application sait lire, et ce que la bibliothèque saura relire pour rejouer
/// le stem.
pub(crate) fn write_stereo_wav(path: &Path, left: &[f32], right: &[f32]) -> Result<(), String> {
    let file =
        File::create(path).map_err(|error| format!("The stem could not be written: {error}"))?;
    let mut writer = BufWriter::new(file);

    let frames = left.len().min(right.len());
    let data_bytes = frames * 2 * 2;
    let byte_rate = REQUIRED_SAMPLE_RATE * 2 * 2;

    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&((data_bytes + 36) as u32).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes()); // PCM entier
    header.extend_from_slice(&2_u16.to_le_bytes());
    header.extend_from_slice(&REQUIRED_SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&4_u16.to_le_bytes());
    header.extend_from_slice(&16_u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    writer
        .write_all(&header)
        .map_err(|error| format!("The stem could not be written: {error}"))?;

    let mut block = Vec::with_capacity(4 * 4096);
    for frame in 0..frames {
        for sample in [left[frame], right[frame]] {
            let clamped = sample.clamp(-1.0, 1.0);
            let value = (clamped * i16::MAX as f32).round() as i16;
            block.extend_from_slice(&value.to_le_bytes());
        }
        if block.len() >= 4 * 4096 {
            writer
                .write_all(&block)
                .map_err(|error| format!("The stem could not be written: {error}"))?;
            block.clear();
        }
    }
    writer
        .write_all(&block)
        .map_err(|error| format!("The stem could not be written: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("The stem could not be written: {error}"))
}

/// Décode un fichier en deux canaux flottants.
///
/// Un mono est dédoublé plutôt que refusé : le modèle attend deux canaux, et
/// une voix centrée reste une voix.
fn decode_stereo(
    path: &Path,
    stop_after_ms: Option<f64>,
    on_progress: &mut dyn FnMut(f64),
) -> Result<([Vec<f32>; 2], u32, f64), String> {
    let decoder = open_mp3_decoder(path)?;
    let sample_rate = decoder.sample_rate().get();
    let channel_count = usize::from(decoder.channels().get()).max(1);
    let total = decoder.total_duration().unwrap_or_default().as_secs_f64() * 1000.0;
    let wanted_ms =
        stop_after_ms
            .unwrap_or(f64::INFINITY)
            .min(if total > 0.0 { total } else { f64::INFINITY });
    let limit = if wanted_ms.is_finite() {
        (wanted_ms / 1000.0 * sample_rate as f64) as usize * channel_count
    } else {
        usize::MAX
    };

    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut pending = [0.0_f32; 2];
    let mut position = 0_usize;
    for sample in decoder {
        let channel = position % channel_count;
        if channel < 2 {
            pending[channel] = sample;
        }
        position += 1;
        if channel == channel_count - 1 {
            left.push(pending[0]);
            right.push(if channel_count > 1 {
                pending[1]
            } else {
                pending[0]
            });
            // Une fois par seconde de musique environ : assez pour que la barre
            // bouge, assez rare pour ne rien coûter.
            if left.len().is_multiple_of(sample_rate as usize) && limit != usize::MAX {
                on_progress((position as f64 / limit as f64).clamp(0.0, 1.0));
            }
        }
        if position >= limit {
            break;
        }
    }
    on_progress(1.0);
    Ok(([left, right], sample_rate, total))
}

#[cfg(test)]
mod resample_tests {
    use super::{REQUIRED_SAMPLE_RATE, resample_channel};

    /// À fréquence identique, on ne touche à rien — pas même d'un millième.
    #[test]
    fn matching_rates_pass_the_audio_through_untouched() {
        let samples: Vec<f32> = (0..1_000).map(|n| (n as f32 * 0.01).sin()).collect();
        let out = resample_channel(&samples, REQUIRED_SAMPLE_RATE, REQUIRED_SAMPLE_RATE)
            .expect("passing through should work");
        assert_eq!(out, samples);
    }

    /// Un morceau à 48 kHz doit sortir à 44,1 kHz **de la même durée**.
    ///
    /// C'est ce qui compte pour la suite : la fenêtre du clip, le décalage de
    /// source et la grille se comptent en millisecondes. Une durée qui glisse
    /// décalerait le stem sous la grille sans que rien ne le signale.
    #[test]
    fn a_48k_second_becomes_a_44k1_second() {
        let seconds = 2.0_f64;
        let frames = (48_000.0 * seconds) as usize;
        let samples: Vec<f32> = (0..frames)
            .map(|n| (n as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU).sin())
            .collect();

        let out = resample_channel(&samples, 48_000, REQUIRED_SAMPLE_RATE)
            .expect("resampling should work");

        let produced = out.len() as f64 / f64::from(REQUIRED_SAMPLE_RATE);
        assert!(
            (produced - seconds).abs() < 0.01,
            "deux secondes sont devenues {produced:.3} s"
        );
    }

    /// Le signal reste le signal : ni évanoui, ni saturé.
    #[test]
    fn a_sine_keeps_its_level_across_the_conversion() {
        let frames = 48_000;
        let samples: Vec<f32> = (0..frames)
            .map(|n| (n as f32 / 48_000.0 * 1_000.0 * std::f32::consts::TAU).sin() * 0.5)
            .collect();

        let out = resample_channel(&samples, 48_000, REQUIRED_SAMPLE_RATE)
            .expect("resampling should work");

        // La moitié centrale, pour ignorer la montée et la descente du filtre.
        let middle = &out[out.len() / 4..out.len() * 3 / 4];
        let rms = (middle.iter().map(|s| s * s).sum::<f32>() / middle.len() as f32).sqrt();
        let expected = 0.5 / std::f32::consts::SQRT_2;
        assert!(
            (rms - expected).abs() < 0.02,
            "niveau attendu {expected:.3}, obtenu {rms:.3}"
        );
        assert!(
            out.iter().all(|s| s.abs() <= 1.0),
            "le rééchantillonnage a fait saturer le signal"
        );
    }

    #[test]
    fn an_empty_channel_is_not_an_error() {
        assert!(
            resample_channel(&[], 48_000, REQUIRED_SAMPLE_RATE)
                .expect("an empty channel should be allowed")
                .is_empty()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chirp(samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|index| {
                let time = index as f32 / 44_100.0;
                (0.4 * (2.0 * PI * (110.0 + 900.0 * time) * time).sin())
                    + 0.2 * (2.0 * PI * 3_000.0 * time).sin()
            })
            .collect()
    }

    /// Sans transformation entre les deux, le signal doit revenir tel quel.
    ///
    /// C'est le seul test qui compte vraiment ici : un masque appliqué sur une
    /// analyse qui ne se réinverse pas proprement laisse des ondulations au
    /// rythme du pas d'avancement, ce qui s'entend comme un scintillement et se
    /// confond avec un défaut du modèle.
    #[test]
    fn the_transform_and_its_inverse_give_the_signal_back() {
        let plan = StftPlan::new();
        let signal = chirp(44_100);
        let spectrogram = plan.forward(&signal);
        let restored = plan.inverse(&spectrogram, signal.len());

        assert_eq!(restored.len(), signal.len());
        let worst = signal
            .iter()
            .zip(&restored)
            .map(|(original, restored)| (original - restored).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 1e-4, "écart maximal de {worst}");
    }

    /// Sépare pour de vrai, avec la bibliothèque et le modèle du dépôt.
    ///
    /// Ignoré par défaut : `check` doit passer sur une machine qui n'a pas
    /// encore récupéré la DLL. À lancer à la main —
    /// `cargo test separates_a_real_file -- --ignored --nocapture` — c'est le
    /// seul test qui prouve que la chaîne entière tient, du décodage au fichier
    /// écrit, et il attrape ce qu'aucun test unitaire ne verrait : une
    /// bibliothèque absente, un modèle qui refuse la forme qu'on lui donne, une
    /// sortie transposée à l'envers.
    #[test]
    #[ignore = "demande onnxruntime.dll et le modèle dans src-tauri/resources"]
    fn separates_a_real_file() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let runtime = resources.join("onnxruntime.dll");
        let model = resources.join("models").join("open-unmix-vocals-fp16.onnx");
        let temporary =
            std::env::temp_dir().join(format!("mixcanvas-stems-{}", std::process::id()));
        std::fs::create_dir_all(&temporary).expect("le dossier de travail devrait être créé");

        // Quatre secondes : une basse continue et un souffle, de quoi donner au
        // modèle quelque chose à trier sans attendre une minute.
        let samples = 44_100 * 4;
        let mix = chirp(samples);
        let source = temporary.join("mix.wav");
        write_stereo_wav(&source, &mix, &mix).expect("l'entrée devrait s'écrire");

        let mut seen = Vec::new();
        let files = separate_track(
            &source,
            &runtime,
            &model,
            &temporary,
            None,
            "mix",
            |fraction| seen.push(fraction),
        )
        .expect("la séparation devrait aboutir");

        assert!(files.vocals.is_file(), "la voix devrait exister");
        assert!(
            files.instrumental.is_file(),
            "l'instrumental devrait exister"
        );
        assert!(!seen.is_empty(), "la progression devrait être rapportée");
        assert!(
            seen.last().is_some_and(|last| (last - 1.0).abs() < 1e-9),
            "la progression devrait finir à un : {seen:?}"
        );
        // Deux fichiers de même durée que l'entrée, et non deux fichiers vides.
        for path in [&files.vocals, &files.instrumental] {
            let written = std::fs::metadata(path)
                .expect("le stem devrait être lisible")
                .len();
            let expected = 44 + samples as u64 * 4;
            assert!(
                written.abs_diff(expected) < 1_000,
                "{path:?} fait {written} octets, attendu environ {expected}"
            );
        }

        let _ = std::fs::remove_dir_all(&temporary);
    }

    /// Deux stems séparés doivent être **deux** signaux, et couvrir toute la
    /// fenêtre demandée.
    ///
    /// Ignoré par défaut, comme l'autre : il demande la bibliothèque et le
    /// modèle. Il existe parce qu'un stem muet à partir d'un certain point, ou
    /// deux stems au dessin identique, se voient à l'usage sans qu'aucun test
    /// unitaire ne les attrape.
    #[test]
    #[ignore = "demande onnxruntime.dll et le modèle dans src-tauri/resources"]
    fn a_windowed_separation_covers_its_window_and_splits_in_two() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let runtime = resources.join("onnxruntime.dll");
        let model = resources.join("models").join("open-unmix-vocals-fp16.onnx");
        let temporary =
            std::env::temp_dir().join(format!("mixcanvas-window-{}", std::process::id()));
        std::fs::create_dir_all(&temporary).expect("le dossier de travail devrait être créé");

        // Vingt secondes, dont on ne sépare que la tranche 8 s → 16 s.
        let samples = 44_100 * 20;
        let mix = chirp(samples);
        let source = temporary.join("mix.wav");
        write_stereo_wav(&source, &mix, &mix).expect("l'entrée devrait s'écrire");

        let files = separate_track(
            &source,
            &runtime,
            &model,
            &temporary,
            Some((8_000.0, 16_000.0)),
            "window",
            |_| {},
        )
        .expect("la séparation devrait aboutir");

        println!("décalage de source : {} ms", files.source_from_ms);

        let lire = |path: &Path| -> Vec<f32> {
            let decoder = open_mp3_decoder(path).expect("le stem devrait se relire");
            decoder.collect()
        };
        let voix = lire(&files.vocals);
        let instrumental = lire(&files.instrumental);
        println!(
            "voix : {} échantillons, instrumental : {}",
            voix.len(),
            instrumental.len()
        );

        // La fenêtre demandée fait 8 s, plus 4 s de marge de chaque côté.
        let attendu = 44_100 * 16 * 2;
        assert!(
            voix.len().abs_diff(attendu) < 44_100,
            "la voix devrait couvrir la fenêtre et ses marges : {} contre {attendu}",
            voix.len()
        );

        // Aucun des deux ne doit s'éteindre en cours de route.
        for (nom, signal) in [("voix", &voix), ("instrumental", &instrumental)] {
            let quarts = 8;
            let par_quart = signal.len() / quarts;
            for quart in 0..quarts {
                let tranche = &signal[quart * par_quart..(quart + 1) * par_quart];
                let crete = tranche.iter().fold(0.0_f32, |pire, v| pire.max(v.abs()));
                println!("{nom} — huitième {quart} : crête {crete:.4}");
                assert!(
                    crete > 1e-4,
                    "{nom} est muet sur son huitième {quart} : crête {crete}"
                );
            }
        }

        // Et ce sont bien deux signaux différents.
        let ecart: f32 = voix
            .iter()
            .zip(&instrumental)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        println!("écart maximal entre les deux stems : {ecart:.4}");
        assert!(ecart > 0.01, "les deux stems sont identiques");

        let _ = std::fs::remove_dir_all(&temporary);
    }

    /// Découper en tranches ne doit rien changer au résultat.
    ///
    /// C'est l'hypothèse sur laquelle repose toute la séparation : le modèle
    /// travaille par blocs de 256 trames, et si le raccord entre deux blocs
    /// laissait la moindre marche, elle s'entendrait toutes les six secondes.
    #[test]
    fn working_in_slices_gives_the_same_signal_as_working_in_one_go() {
        let plan = StftPlan::new();
        let signal = chirp(120_000);
        let whole = plan.inverse(&plan.forward(&signal), signal.len());

        let padded = plan.pad(&signal);
        let frames = plan.frames_for(signal.len());
        let length = plan.scratch_length(signal.len());
        let mut accumulated = vec![0.0_f32; length];
        let mut weight = vec![0.0_f32; length];
        let mut first = 0;
        while first < frames {
            let count = FRAMES_PER_CHUNK.min(frames - first);
            let slice = plan.forward_frames(&padded, first, count);
            plan.inverse_into(&slice, first, &mut accumulated, &mut weight);
            first += count;
        }
        let sliced = plan.finish(accumulated, weight, signal.len());

        let worst = whole
            .iter()
            .zip(&sliced)
            .map(|(whole, sliced)| (whole - sliced).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 1e-5, "écart maximal de {worst} au raccord");
    }

    /// Les bords comptent autant que le milieu : un morceau commence souvent sur
    /// un temps fort, et une première trame atténuée s'entendrait.
    #[test]
    fn the_edges_come_back_at_full_level() {
        let plan = StftPlan::new();
        let signal = vec![0.5_f32; 20_000];
        let restored = plan.inverse(&plan.forward(&signal), signal.len());

        for (index, value) in restored.iter().enumerate() {
            assert!(
                (value - 0.5).abs() < 1e-3,
                "échantillon {index} revenu à {value}"
            );
        }
    }

    /// Un masque à un laisse le signal intact, un masque à zéro le fait taire.
    /// Entre les deux, le rapport s'applique sans toucher à la phase.
    #[test]
    fn a_mask_scales_the_magnitude_and_keeps_the_phase() {
        let plan = StftPlan::new();
        let signal = chirp(8_192);
        let spectrogram = plan.forward(&signal);

        let untouched = spectrogram.masked(&spectrogram.magnitudes());
        for (before, after) in spectrogram.bins.iter().zip(&untouched.bins) {
            assert!((before - after).norm() < 1e-3);
        }

        let silenced = spectrogram.masked(&vec![0.0; spectrogram.bins.len()]);
        let restored = plan.inverse(&silenced, signal.len());
        assert!(restored.iter().all(|value| value.abs() < 1e-6));

        let halved: Vec<f32> = spectrogram
            .magnitudes()
            .iter()
            .map(|magnitude| magnitude * 0.5)
            .collect();
        let quiet = spectrogram.masked(&halved);
        for (before, after) in spectrogram.bins.iter().zip(&quiet.bins) {
            assert!((after.norm() - before.norm() * 0.5).abs() < 1e-3);
            if before.norm() > 1e-3 {
                // Même direction dans le plan complexe : la phase n'a pas bougé.
                assert!((before.arg() - after.arg()).abs() < 1e-3);
            }
        }
    }

    /// La somme des deux stems doit rendre le mélange, à l'échantillon près.
    /// C'est ce que garantit la soustraction dans le domaine temporel, et c'est
    /// la raison pour laquelle l'instrumental n'est pas prédit.
    #[test]
    fn the_two_stems_add_back_up_to_the_mix() {
        let plan = StftPlan::new();
        let mix = chirp(30_000);
        let spectrogram = plan.forward(&mix);
        // Un masque quelconque, comme le modèle en produirait.
        let mask: Vec<f32> = spectrogram
            .magnitudes()
            .iter()
            .enumerate()
            .map(|(index, magnitude)| magnitude * if index % 3 == 0 { 0.8 } else { 0.1 })
            .collect();
        let vocals = plan.inverse(&spectrogram.masked(&mask), mix.len());
        let instrumental: Vec<f32> = mix
            .iter()
            .zip(&vocals)
            .map(|(mix, vocals)| mix - vocals)
            .collect();

        for (index, sample) in mix.iter().enumerate() {
            let sum = vocals[index] + instrumental[index];
            assert!((sum - sample).abs() < 1e-6, "à l'échantillon {index}");
        }
    }
}
