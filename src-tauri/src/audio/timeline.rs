use std::{
    collections::{HashSet, VecDeque, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    num::NonZero,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source, source::SeekError};

use crate::{tempo::TempoMap, timeline::TimelineRenderPlan};

use super::bitcrush::BitCrusher;
use super::delay::{DELAY_BEATS, Delay};
use super::flanger::Flanger;
use super::metadata::{Mp3Decoder, open_mp3_decoder};
use super::reverb::Reverb;

const FALLBACK_OUTPUT_SAMPLE_RATE: u32 = 44_100;
pub(crate) const OUTPUT_CHANNELS: u16 = 2;
/// Longueur d'un grain, en images.
///
/// Quarante-six millisecondes. Un grain court multiplie les raccords — quatre-
/// vingt-six par seconde à 512 images — et chacun laisse sa trace sur un son
/// tenu. Passer à 2048 divise ce nombre par quatre et fait tomber l'énergie
/// parasite d'une nappe de 24 % à 2 %, mesuré, sans rien coûter aux attaques :
/// les deux longueurs rendent le même nombre de frappes.
const WSOLA_HOP_FRAMES: usize = 2_048;

/// Combien de temps chaque effet continue d'être calculé après son dernier
/// envoi, **en secondes**.
///
/// En secondes et non en images, et c'est une correction. Les trois budgets
/// étaient écrits en nombres d'images à 48 kHz — `5 * 48_000` et compagnie —
/// donc ils ne décrivaient la bonne durée que sur une sortie à cette fréquence.
/// À 44,1 kHz les queues traînaient neuf pour cent trop longtemps, ce qui ne
/// coûte que du calcul; mais **à 96 kHz elles étaient coupées de moitié**, et
/// celle du delay serait retombée à douze secondes et demie — sous les
/// vingt-quatre qu'il lui faut à quarante BPM. Le défaut corrigé en portant ce
/// budget de quinze à vingt-cinq secondes revenait donc intact dès qu'on
/// branchait une interface à 96 kHz, sans que rien ne le signale.
///
/// La pièce : au-delà de cinq secondes, la plus grande des trois queues est
/// éteinte bien sous le seuil d'audition, et il n'y a plus de raison de payer
/// vingt-quatre lignes à retard par échantillon.
const REVERB_TAIL_SECONDS: f32 = 5.0;

/// Le flanger : sa ligne fait quelques millisecondes et son rebouclage l'éteint
/// en une fraction de seconde.
const FLANGER_TAIL_SECONDS: f32 = 0.5;

/// Le delay, de loin la plus longue, et la seule qui ait dû être **calculée**
/// plutôt qu'estimée : elle dépend du tempo. Avec un rebouclage de 0,72 il faut
/// une vingtaine de tours pour passer sous le seuil d'audition, et à quarante
/// BPM — le plancher que le programme accepte — une croche pointée dure plus
/// d'une seconde, ce qui met la traîne à près de vingt-quatre secondes.
const DELAY_TAIL_SECONDS: f32 = 25.0;

/// Le budget d'une queue en images, pour la fréquence de sortie réelle.
fn tail_frames(seconds: f32, sample_rate: u32) -> usize {
    (seconds * sample_rate as f32).ceil() as usize
}

/// La montée de l'envoi quand on appuie : courte et fixe.
///
/// Assez longue pour ne pas claquer, assez courte pour qu'un appui sur le temps
/// tombe sur le temps. Elle ne suit pas le tempo, contrairement à la descente :
/// une attaque qui s'allonge sur un morceau lent arriverait en retard.
const REVERB_ATTACK_SECONDS: f32 = 0.010;

/// La descente, en fraction de temps — une croche pointée.
///
/// Musicale plutôt que fixe : à 128 BPM elle dure un peu plus de trois cents
/// millisecondes, et elle s'allonge d'elle-même sur un morceau plus lent. Une
/// trente-deuxième, d'abord retenue, coupait net; une croche restait sèche. Ce
/// n'est pourtant que l'envoi qui retombe, la queue déjà dans la pièce
/// continuant de sonner.
///
/// **La même valeur que la rampe de sortie écrite sur la timeline.** Ce qu'on
/// entend en jouant doit être ce qui se rejoue, sans quoi la passe enregistrée
/// ne ressemble pas au geste qui l'a produite.
const REVERB_RELEASE_BEATS: f32 = 0.75;

/// Le gain d'envoi d'une piste à l'image suivante.
///
/// Trois choses s'y rencontrent, et les séparer de la boucle de mixage permet
/// de les vérifier : la rampe du geste vivant, la passe déjà enregistrée, et la
/// gomme.
///
/// Le geste et la passe se combinent par le **maximum**, non par une somme :
/// rejouer par-dessus une passe déjà écrite doit s'entendre comme la même
/// reverb, pas comme le double.
///
/// Sous la gomme, la passe enregistrée ne compte plus. Elle est en train d'être
/// retirée, et la laisser sonner ferait entendre le contraire du geste. Le
/// geste vivant, lui, continue de passer : tenir la reverb et la gomme ensemble
/// laisse entendre ce qu'on joue, ce qui est bien ce qui restera. Et comme le
/// gain courant portait encore la valeur de l'automation à l'image précédente,
/// la coupure n'est pas franche — la descente reprend là où elle était,
/// exactement comme si l'on relâchait le bouton.
fn next_effect_send(
    current: f32,
    held: bool,
    erasing: bool,
    automation: f32,
    attack: f32,
    release: f32,
) -> f32 {
    let gesture = if held {
        (current + attack).min(1.0)
    } else {
        (current - release).max(0.0)
    };
    if erasing {
        gesture
    } else {
        gesture.max(automation)
    }
}

/// Durée du raccord entre deux grains, en images.
///
/// Le fondu couvrait autrefois **tout** le pas. Deux flux dont l'écart croît
/// linéairement étaient donc mélangés en permanence, ce qui revient à lire à
/// leur vitesse moyenne : la hauteur suivait le tempo, et un morceau ralenti de
/// sept battements sonnait un demi-ton plus bas. Le grain joue maintenant seul
/// l'essentiel du pas, et ne se croise avec le précédent que sur ce raccord.
const WSOLA_FADE_FRAMES: usize = 256;
/// Fenêtre de comparaison, en images. Cinq périodes d'un grave à 110 Hz : en
/// dessous, la corrélation ne voit pas assez de forme pour la reconnaître.
const WSOLA_CORRELATION_FRAMES: usize = 2_048;
const WSOLA_CORRELATION_STRIDE: usize = 8;
/// Étendue de la recherche de raccord.
///
/// Elle doit couvrir **au moins une période du grave**, sans quoi il n'existe
/// aucun décalage capable de remettre la forme en phase : 400 images à 110 Hz.
/// Elle était naguère déduite de la correction à faire — ±21 images pour sept
/// battements d'écart —, ce qui rendait le recalage impossible par construction.
const WSOLA_MAX_SEARCH_FRAMES: usize = 512;
/// Pas de la passe grossière, en images, et de combien elle allège la fenêtre
/// de comparaison.
///
/// La corrélation d'un son musical est lisse à l'échelle de la période qu'on
/// cherche : un point sur seize suffit à reconnaître la bonne bosse, et un
/// point sur quatre de la fenêtre suffit à la noter. Les passes fines qui
/// suivent retrouvent le sommet exact.
const WSOLA_COARSE_OFFSET_STEP: usize = 16;
const WSOLA_COARSE_DECIMATION: usize = 4;
const SOURCE_BACKTRACK_FRAMES: usize = 4_096;
const MAX_PROJECT_SECONDS: f64 = 4.0 * 60.0 * 60.0;
const MIN_STRETCH_RATIO: f64 = 0.5;
const MAX_STRETCH_RATIO: f64 = 2.0;
const OUTPUT_CEILING: f32 = 0.98;
/// La chute du mètre, en constante de temps : environ dix décibels par
/// seconde, la cadence d'un crête-mètre de studio. L'attaque, elle, n'a pas de
/// constante — voir `vu_envelope`.
const VU_RELEASE_SECONDS: f32 = 0.85;
const METER_PUBLISH_FRAMES: usize = 128;
const FILTER_SMOOTHING_SECONDS: f32 = 0.008;
const FILTER_Q: f32 = 0.707_106_77;
const FILTER_LOW_PASS_OPEN_HZ: f32 = 18_000.0;
const FILTER_LOW_PASS_CLOSED_HZ: f32 = 90.0;
const FILTER_HIGH_PASS_OPEN_HZ: f32 = 50.0;
const FILTER_HIGH_PASS_CLOSED_HZ: f32 = 12_000.0;
const FILTER_LOW_PASS_MAX_MAKEUP_DB: f32 = 6.0;
const FILTER_HIGH_PASS_MAX_MAKEUP_DB: f32 = 4.5;
/// Clip EQ gain at or below which a band is a full cut rather than an
/// attenuation. `CLIP_EQ_SILENCE_DB` in `src/lib/clipEq.ts` holds the same
/// value; the interface never sends `-Infinity`, which JSON cannot carry.
const CLIP_EQ_SILENCE_DB: f64 = -60.0;
/// Master limiter. The threshold sits on the physical output bound so the
/// limiter absorbs what the hard clamp used to shave off; the clamp stays as a
/// last resort for the brief overshoot a finite attack cannot catch.
const LIMITER_THRESHOLD: f32 = OUTPUT_CEILING;
/// Small margin above the output bound before `OL` calls it a clipped peak.
/// While the limiter holds a signal, the result sits exactly on the bound, and
/// rounding alone must not be reported as an overload.
const OVERLOAD_THRESHOLD: f32 = OUTPUT_CEILING + 0.001;
const LIMITER_ATTACK_SECONDS: f32 = 0.002;
const LIMITER_RELEASE_SECONDS: f32 = 0.12;

/// Master compressor character. See `MasterCompressor` for why each value.
/// −12 dBFS, a 6 dB knee around it, 2:1, and +3 dB of makeup.
const COMPRESSOR_THRESHOLD: f32 = 0.251_188_6;
const COMPRESSOR_KNEE_LOW: f32 = 0.177_827_94;
const COMPRESSOR_KNEE_HIGH: f32 = 0.354_813_4;
const COMPRESSOR_ATTACK_SECONDS: f32 = 0.010;
const COMPRESSOR_RELEASE_SECONDS: f32 = 0.120;
/// +2 dB. Kept low because the colour shelves below also add level.
const COMPRESSOR_MAKEUP_GAIN: f32 = 1.258_925_4;
/// The detector listens to the mix through a high pass at 120 Hz.
const COMPRESSOR_DETECTOR_HZ: f32 = 120.0;
/// Console-style tilt: a little weight under 90 Hz, a little air around 13 kHz.
///
/// Both ends used to be shelves sharing one constant, which is why the curve
/// came out as a symmetrical smile. Judged on other systems it was too much of
/// one, and more so at the top.
///
/// The top is no longer a shelf at all. A shelf rises to its full gain and
/// **stays there** to Nyquist, so it was lifting 18 kHz and above as hard as
/// the air band — a stretch nothing reproduces and no one hears, which costs
/// headroom and, on the MP3 path, costs bits. A bell does the same work where
/// the work is audible and comes back to nothing above it: +1.0 dB at 13 kHz,
/// +0.13 at 18 kHz, +0.03 at 20 kHz. A Q of 1.2 keeps it wide enough to read
/// as air rather than as a resonance.
const COLOUR_LOW_SHELF_HZ: f32 = 90.0;
const COLOUR_LOW_SHELF_DB: f32 = 1.5;
const COLOUR_AIR_HZ: f32 = 13_000.0;
const COLOUR_AIR_DB: f32 = 1.0;
const COLOUR_AIR_Q: f32 = 1.2;
/// Saturation is what actually colours a signal — shelves only move its
/// balance. It is confined to the body of the mix, below 5 kHz: the third
/// harmonic of anything in that band lands under 15 kHz and so stays inside the
/// spectrum, which is what makes oversampling unnecessary here. It is also
/// where the warmth belongs; the same curve applied to cymbals would only add
/// fizz. Everything above the split is passed through untouched.
const COLOUR_SATURATION_SPLIT_HZ: f32 = 5_000.0;
/// How much of the saturated body replaces the clean one. Depth lives here
/// rather than in a drive control: the cubic's usable range is exactly ±1,
/// which is also the sample range, so pushing harder would only bury peaks in
/// the flat part of the curve where it stops being musical and starts being a
/// clipper. Blending instead keeps the bend gentle at every level. At 0.3 a
/// body peaking near full scale loses under a decibel and gains a third
/// harmonic around −34 dB: audible as weight, not as distortion.
const COLOUR_SATURATION_MIX: f32 = 0.3;
/// Fade of the whole colour stage when COMP is switched during playback.
const COLOUR_BLEND_SECONDS: f32 = 0.008;

/// Sidechain ducking. The character is fixed, as for the other master
/// processors, and aims at the pronounced French-touch pump rather than at
/// transparent gain control.
/// The detector is low-passed at 150 Hz so a whole track can be the key and
/// still trigger on its kick. −15 dB is deep enough to be unmistakable while
/// leaving the covered material audible underneath. The gain recovers over
/// nine tenths of a beat, so it arrives back at unity just as the next kick
/// lands — that near-miss is what the ear hears as breathing.
const DUCK_DETECTOR_HZ: f32 = 150.0;
/// Profondeur nominale du pompage, clé à plein niveau. Elle s'atténue avec
/// l'enveloppe de la clé, de sorte qu'une montée de fader écrive une
/// progression plutôt qu'un interrupteur.
const DUCK_DEPTH_DB: f32 = 15.0;
/// Le même chiffre en gain linéaire, pour les tests. `powf` n'étant pas
/// constante, c'est la seule façon de l'écrire ici; le rendu, lui, le calcule
/// depuis les décibels et un test vérifie que les deux s'accordent.
#[cfg(test)]
const DUCK_FLOOR: f32 = 0.177_827_94;
const DUCK_RELEASE_BEATS: f64 = 0.9;
/// Envelopes the trigger compares, both measured on energy rather than on
/// amplitude. Measuring one as a peak and the other as a mean was the mistake
/// that let a bassline through: for a steady sine that ratio sits near 1.57
/// whatever the level, close enough to any sensible threshold to fire. Two
/// smoothed energies converge on the same value for steady material, so their
/// ratio settles at 1 and only a genuine rise can lift it.
const DUCK_FAST_SECONDS: f32 = 0.015;
const DUCK_SLOW_SECONDS: f32 = 0.300;
/// Energy ratio marking a hit: two and a half times the power, about 4 dB above
/// the level the low end has been holding. A kick quieter than the bassline
/// under it will not clear that bar — but such a kick makes a poor trigger
/// anyway, and lowering the bar further would let ordinary bass notes fire.
const DUCK_TRANSIENT_RATIO: f32 = 2.5;
/// Below this energy the low end is noise, not a kick, whatever its shape.
/// An amplitude of 0.01.
const DUCK_NOISE_FLOOR: f32 = 0.000_1;
/// A kick cannot be followed by another within half a beat, so nothing is
/// allowed to retrigger before then. This keeps a decaying kick, or a bass
/// note landing just after it, from firing the duck a second time.
const DUCK_REFRACTORY_BEATS: f64 = 0.5;
/// How often the release is re-timed against the tempo curve. A ramp does not
/// need a sample-accurate tempo, and reading the curve is not free.
const DUCK_TEMPO_REFRESH_FRAMES: usize = 2_048;

/// Master glue compressor, tuned for the electronic material this tool mixes.
///
/// The character is fixed rather than exposed as knobs: one button has to give
/// a usable result, and every value below serves that.
///
/// - **Threshold −12 dBFS.** Lanes rest at −4 dB, so a busy passage sums a few
///   dB under full scale. The compressor therefore works on the loud half of
///   the material and leaves quiet passages alone.
/// - **2:1.** Enough to be heard as density; past that a dense four-to-the-floor
///   mix stops breathing.
/// - **6 dB soft knee.** The compressor eases in instead of switching on, which
///   would be audible as a click on every kick near the threshold.
/// - **Attack 10 ms.** Deliberately not faster: the kick transient has to pass
///   through untouched, or the mix loses its punch — the one thing that must
///   not happen here.
/// - **Release 120 ms.** Shorter than a beat at any club tempo, so the gain
///   recovers between kicks. That recovery is the audible pumping.
/// - **Makeup +2 dB.** Far below the ~6 dB that would restore the theoretical
///   loss, on purpose: the point is density, not level. The limiter stays a
///   safety net rather than becoming part of the sound.
/// - **Detector high-passed at 120 Hz.** The one choice that matters most for
///   this material. Fed the full mix, the kick owns the whole gain reduction
///   and ducks the track on every beat. Listening past the low end, the
///   compressor answers to the mix as a whole and the kick keeps its weight —
///   the compression becomes frequency dependent rather than bass driven.
#[derive(Clone, Copy, Debug)]
struct MasterCompressor {
    gain: f32,
    detector_previous_input: f32,
    detector_previous_output: f32,
    detector_coefficient: f32,
    detector_sample_rate: u32,
}

impl Default for MasterCompressor {
    fn default() -> Self {
        Self {
            gain: 1.0,
            detector_previous_input: 0.0,
            detector_previous_output: 0.0,
            detector_coefficient: 0.0,
            detector_sample_rate: 0,
        }
    }
}

impl MasterCompressor {
    /// Gain to apply to the frame, derived from its stereo-linked mono sum.
    fn process(&mut self, mono: f32, sample_rate: u32, enabled: bool) -> f32 {
        let detected = self.detect(mono, sample_rate).abs();
        if !enabled {
            // Glide back to unity instead of jumping, so switching the
            // compressor off mid-playback is not heard as a step in level.
            self.gain = follow(self.gain, 1.0, COMPRESSOR_RELEASE_SECONDS, sample_rate);
            return self.gain;
        }

        let target = Self::target_gain(detected);
        let seconds = if target < self.gain {
            COMPRESSOR_ATTACK_SECONDS
        } else {
            COMPRESSOR_RELEASE_SECONDS
        };
        self.gain = follow(self.gain, target, seconds, sample_rate);
        if !self.gain.is_finite() {
            self.gain = 1.0;
        }
        self.gain * COMPRESSOR_MAKEUP_GAIN
    }

    /// One-pole high pass on the detection path only; the audio itself is
    /// never routed through it.
    fn detect(&mut self, input: f32, sample_rate: u32) -> f32 {
        if self.detector_sample_rate != sample_rate {
            let rc = 1.0 / (std::f32::consts::TAU * COMPRESSOR_DETECTOR_HZ);
            let dt = 1.0 / sample_rate as f32;
            self.detector_coefficient = rc / (rc + dt);
            self.detector_sample_rate = sample_rate;
        }
        let output = self.detector_coefficient
            * (self.detector_previous_output + input - self.detector_previous_input);
        self.detector_previous_input = input;
        self.detector_previous_output = if output.is_finite() { output } else { 0.0 };
        self.detector_previous_output
    }

    /// Static curve, before the attack and release smoothing.
    ///
    /// At 2:1 the compressed output is `sqrt(threshold * peak)`, so the gain is
    /// `sqrt(threshold / peak)` — one square root, no logarithm on the audio
    /// thread. The knee blends that in with a smoothstep.
    fn target_gain(frame_peak: f32) -> f32 {
        if frame_peak <= COMPRESSOR_KNEE_LOW {
            return 1.0;
        }
        let compressed = (COMPRESSOR_THRESHOLD / frame_peak).sqrt();
        if frame_peak >= COMPRESSOR_KNEE_HIGH {
            return compressed;
        }
        let blend =
            (frame_peak - COMPRESSOR_KNEE_LOW) / (COMPRESSOR_KNEE_HIGH - COMPRESSOR_KNEE_LOW);
        let eased = blend * blend * (3.0 - 2.0 * blend);
        1.0 + (compressed - 1.0) * eased
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.detector_previous_input = 0.0;
        self.detector_previous_output = 0.0;
    }
}

/// Gentle console tilt applied with the compressor: a shelf of weight under the
/// kick and a shelf of air on top. Small on purpose — it should read as
/// presence, not as an EQ move, and it must not eat the limiter's headroom.
#[derive(Clone, Copy, Debug, Default)]
struct MasterColour {
    low_shelf: [BiquadState; OUTPUT_CHANNELS as usize],
    high_shelf: [BiquadState; OUTPUT_CHANNELS as usize],
    saturation_split: [BiquadState; OUTPUT_CHANNELS as usize],
}

/// Cubic soft clipper, the cheapest curve with a musically useful shape: it is
/// exactly linear through the origin, bends progressively, and flattens at
/// ±2/3. Being a cubic, a sine through it produces the fundamental and a third
/// harmonic and nothing else — the harmonic content is bounded by construction,
/// which is what lets the band split above keep every product inside the
/// spectrum.
fn soft_clip(input: f32) -> f32 {
    if input <= -1.0 {
        -2.0 / 3.0
    } else if input >= 1.0 {
        2.0 / 3.0
    } else {
        input - input * input * input / 3.0
    }
}

impl MasterColour {
    /// The whole stage always runs, so its state stays warm and the `amount`
    /// crossfade can switch the colour in and out without a click.
    ///
    /// Order matters: the shelves come first so the lift under the kick drives
    /// the saturator harder than the rest, which is where the weight of a large
    /// console comes from. Saturating first and tilting afterwards would only
    /// equalise a distortion that had already happened.
    fn process(&mut self, input: f32, channel: usize, sample_rate: u32, amount: f32) -> f32 {
        let low = self.low_shelf[channel].process_shelf(
            input,
            COLOUR_LOW_SHELF_HZ,
            COLOUR_LOW_SHELF_DB,
            sample_rate,
            false,
        );
        let tilted = self.high_shelf[channel].process_peaking(
            low,
            COLOUR_AIR_HZ,
            COLOUR_AIR_DB,
            COLOUR_AIR_Q,
            sample_rate,
        );

        // Split off the body, colour that, and hand the top back untouched.
        let body = self.saturation_split[channel].process(
            tilted,
            COLOUR_SATURATION_SPLIT_HZ,
            sample_rate,
            false,
        );
        let air = tilted - body;
        let saturated = body + (soft_clip(body) - body) * COLOUR_SATURATION_MIX;

        let coloured = saturated + air;
        input + (coloured - input) * amount
    }
}

/// One-pole envelope step towards `target` over a time constant.
fn follow(current: f32, target: f32, seconds: f32, sample_rate: u32) -> f32 {
    let coefficient = (-1.0 / (sample_rate as f32 * seconds)).exp();
    target + coefficient * (current - target)
}

/// Sidechain ducker: the pumping a keyed clip imposes on everything it covers.
///
/// It fires on a *transient* in the low end, not on its level. Measuring level
/// was the obvious approach and the wrong one: a bassline occupies the same
/// band as the kick and holds a roughly constant level, so a level detector
/// reads it as one long hit and produces continuous gain reduction — audibly a
/// compressor misbehaving rather than a pump on each kick.
///
/// A transient is what separates the two. A fast envelope follows the attack of
/// a hit while a slow one tracks the level the low end has been sitting at; a
/// kick makes the first jump clear of the second, whereas a sustained bass note
/// moves them together and triggers nothing.
///
/// The tempo enters twice: it times the release, and it sets a refractory
/// window during which nothing may retrigger. It deliberately does not gate on
/// beat *positions* — that would make the effect depend on the beatgrid being
/// right, and a wrong grid would then silence real kicks.
///
/// The release is the shape of the effect, and it is linear in decibels rather
/// than exponential: multiplying the gain by a fixed factor each frame produces
/// the straight swell that reads as pumping, where a one-pole settle would
/// rush most of the way back and then crawl.
#[derive(Clone, Copy, Debug)]
struct SidechainDucker {
    gain: f32,
    detector: BiquadState,
    fast: f32,
    slow: f32,
    release_factor: f32,
    /// Durée de la remontée, en images. Le facteur par image en découle à
    /// chaque déclenchement : une profondeur plus faible doit remonter dans le
    /// même temps musical, sinon le groove change quand on baisse la clé.
    release_frames: f32,
    refractory_frames: usize,
    refractory_reset: usize,
}

impl Default for SidechainDucker {
    fn default() -> Self {
        Self {
            gain: 1.0,
            detector: BiquadState::default(),
            fast: 0.0,
            slow: 0.0,
            release_factor: 1.0,
            release_frames: 1.0,
            refractory_frames: 0,
            refractory_reset: 0,
        }
    }
}

impl SidechainDucker {
    /// Recomputes what the tempo governs: the release step and the refractory
    /// window.
    fn set_tempo(&mut self, bpm: f64, sample_rate: u32) {
        let beat_seconds = 60.0 / bpm.clamp(40.0, 300.0);
        self.release_frames =
            (beat_seconds * DUCK_RELEASE_BEATS * f64::from(sample_rate)).max(1.0) as f32;
        self.refractory_reset =
            (beat_seconds * DUCK_REFRACTORY_BEATS * f64::from(sample_rate)).max(1.0) as usize;
    }

    /// Gain to apply to everything the key covers, from the key's own frame.
    ///
    /// `depth_scale` est le gain d'enveloppe de la piste-clé, de 0 à 1. La
    /// profondeur du pompage le suit : monter graduellement le volume de la
    /// clé fait grandir le pompage, ce qui est la façon d'en écrire une
    /// progression. Une profondeur fixe ne laissait que deux états — pompage
    /// plein tant que le détecteur déclenchait, rien dès qu'il passait sous son
    /// plancher de bruit.
    fn process(&mut self, key_mono: f32, sample_rate: u32, depth_scale: f32) -> f32 {
        let low = self
            .detector
            .process(key_mono, DUCK_DETECTOR_HZ, sample_rate, false)
            .abs();

        let energy = low * low;
        self.fast = follow(self.fast, energy, DUCK_FAST_SECONDS, sample_rate);
        self.slow = follow(self.slow, energy, DUCK_SLOW_SECONDS, sample_rate);

        let waiting = self.refractory_frames > 0;
        self.refractory_frames = self.refractory_frames.saturating_sub(1);

        let hit = !waiting
            && self.fast > DUCK_NOISE_FLOOR
            && self.fast > self.slow * DUCK_TRANSIENT_RATIO;

        if hit {
            // L'attaque d'un sidechain est ce qui laisse passer le kick : la
            // descente est immédiate. Seule sa profondeur suit la clé.
            // La profondeur est mise à l'échelle en décibels et non en gain :
            // à mi-course on veut la moitié du pompage tel qu'on l'entend, ce
            // qu'une interpolation linéaire du gain ne donnerait pas.
            let scale = depth_scale.clamp(0.0, 1.0);
            let floor = 10_f32.powf(-(DUCK_DEPTH_DB * scale) / 20.0);
            self.gain = floor;
            // La remontée prend le même temps musical quelle que soit la
            // profondeur : après `release_frames` images, le gain revient
            // exactement à l'unité.
            self.release_factor = if floor > 0.0 && floor < 1.0 {
                (1.0 / floor).powf(1.0 / self.release_frames.max(1.0))
            } else {
                1.0
            };
            self.refractory_frames = self.refractory_reset;
        } else {
            self.gain = (self.gain * self.release_factor).min(1.0);
        }
        if !self.gain.is_finite() {
            self.gain = 1.0;
        }
        self.gain
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.detector = BiquadState::default();
        self.fast = 0.0;
        self.slow = 0.0;
        self.refractory_frames = 0;
    }
}

/// Stereo-linked gain reduction: both channels share one gain so limiting can
/// never shift the stereo image.
#[derive(Clone, Copy, Debug)]
struct MasterLimiter {
    gain: f32,
}

impl Default for MasterLimiter {
    fn default() -> Self {
        Self { gain: 1.0 }
    }
}

impl MasterLimiter {
    fn process(&mut self, frame_peak: f32, sample_rate: u32, enabled: bool) -> f32 {
        let target = if enabled && frame_peak > LIMITER_THRESHOLD {
            LIMITER_THRESHOLD / frame_peak
        } else {
            1.0
        };
        let seconds = if target < self.gain {
            LIMITER_ATTACK_SECONDS
        } else {
            LIMITER_RELEASE_SECONDS
        };
        let coefficient = (-1.0 / (sample_rate as f32 * seconds)).exp();
        self.gain = target + coefficient * (self.gain - target);
        if self.gain.is_finite() {
            self.gain.clamp(0.0, 1.0)
        } else {
            self.gain = 1.0;
            1.0
        }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
    }
}

#[derive(Clone, Copy, Debug)]
struct GrainBlend {
    current_position: f64,
    previous_position: Option<f64>,
    fade_in: f32,
}

#[derive(Debug, Default)]
struct StereoMeterState {
    left_bits: AtomicU32,
    right_bits: AtomicU32,
    overload: AtomicU8,
    reset_epoch: AtomicU32,
}

impl StereoMeterState {
    fn store(&self, left: f32, right: f32) {
        self.left_bits
            .store(left.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.right_bits
            .store(right.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn levels(&self) -> (f32, f32, bool) {
        (
            f32::from_bits(self.left_bits.load(Ordering::Relaxed)),
            f32::from_bits(self.right_bits.load(Ordering::Relaxed)),
            self.overload.load(Ordering::Relaxed) != 0,
        )
    }

    fn set_overload(&self, overload: bool) {
        self.overload.store(u8::from(overload), Ordering::Relaxed);
    }

    fn reset(&self) {
        self.store(0.0, 0.0);
        self.set_overload(false);
        self.reset_epoch.fetch_add(1, Ordering::Relaxed);
    }

    fn epoch(&self) -> u32 {
        self.reset_epoch.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug)]
struct VolumeFramePoint {
    frame: usize,
    gain_db: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
struct PanFramePoint {
    frame: usize,
    value: f32,
}

/// Panoramique d'une voie au fil des images. Interpolé linéairement entre les
/// nœuds, centré là où il n'y en a pas.
#[derive(Clone, Debug, Default)]
struct PanAutomation {
    points: Vec<PanFramePoint>,
}

impl PanAutomation {
    fn value_at_frame(&self, frame: usize) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        let next_index = self.points.partition_point(|point| point.frame < frame);
        match (next_index.checked_sub(1), self.points.get(next_index)) {
            (None, Some(next)) => next.value,
            (Some(previous), None) => self.points[previous].value,
            (Some(previous), Some(next)) => {
                let previous = self.points[previous];
                if next.frame <= previous.frame {
                    return next.value;
                }
                let mix = (frame - previous.frame) as f32 / (next.frame - previous.frame) as f32;
                previous.value + (next.value - previous.value) * mix
            }
            (None, None) => 0.0,
        }
    }
}

/// Gains gauche et droit pour un panoramique de −1 à +1.
///
/// Loi à puissance constante : les deux gains valent `√2/2` au centre, soit
/// −3 dB chacun, de sorte que la somme de puissance ne bouge pas d'un bout à
/// l'autre du balayage. Une loi linéaire ferait paraître le centre plus fort
/// que les extrêmes, ce qui s'entend comme une bosse au milieu d'un mouvement.
fn equal_power_pan(value: f32) -> (f32, f32) {
    let angle = (value.clamp(-1.0, 1.0) + 1.0) * 0.25 * std::f32::consts::PI;
    (angle.cos(), angle.sin())
}

/// Ce que la clé pèse dans la somme mono du bus, le centre valant un.
///
/// Un kick poussé sur un côté n'envoie plus que la moitié de lui-même dans la
/// somme, là où le centre en envoie `√2/2` de chaque côté : trois décibels de
/// moins à l'arrivée, donc un pompage d'autant plus léger. C'est ce que fait
/// une console dont le départ est pris après le panoramique.
///
/// Le rapport est ramené au centre plutôt qu'absolu, pour que le cas courant —
/// une grosse caisse au milieu — reste exactement ce qu'il était.
///
/// Le niveau envoyé au détecteur ne pouvait pas porter cette information : son
/// déclenchement compare une énergie rapide à une énergie lente, il est donc
/// insensible au niveau qu'on lui donne. C'est la profondeur qui la porte, comme
/// pour l'enveloppe de volume.
fn sidechain_pan_weight(value: f32) -> f32 {
    let (left, right) = equal_power_pan(value);
    (left + right) / std::f32::consts::SQRT_2
}

#[derive(Clone, Debug, Default)]
struct VolumeAutomation {
    points: Vec<VolumeFramePoint>,
}

impl VolumeAutomation {
    fn gain_at_frame(&self, frame: usize) -> f32 {
        if self.points.is_empty() {
            return db_to_gain(Some(crate::timeline::DEFAULT_TRACK_GAIN_DB));
        }
        let next_index = self.points.partition_point(|point| point.frame < frame);
        match (next_index.checked_sub(1), self.points.get(next_index)) {
            (None, Some(next)) => db_to_gain(next.gain_db),
            (Some(previous), None) => db_to_gain(self.points[previous].gain_db),
            (Some(previous), Some(next)) => {
                let previous = self.points[previous];
                if frame == next.frame {
                    return db_to_gain(next.gain_db);
                }
                if next.frame <= previous.frame {
                    return db_to_gain(next.gain_db);
                }
                let mix = (frame - previous.frame) as f64 / (next.frame - previous.frame) as f64;
                let previous_db = previous.gain_db.unwrap_or(-60.0);
                let next_db = next.gain_db.unwrap_or(-60.0);
                db_to_gain(Some(previous_db + (next_db - previous_db) * mix))
            }
            (None, None) => db_to_gain(Some(crate::timeline::DEFAULT_TRACK_GAIN_DB)),
        }
    }
}

fn db_to_gain(gain_db: Option<f64>) -> f32 {
    gain_db.map_or(0.0, |value| 10_f64.powf(value / 20.0) as f32)
}

#[derive(Clone, Copy, Debug)]
struct FilterFramePoint {
    frame: usize,
    value: f64,
    tension: f64,
}

#[derive(Clone, Debug, Default)]
struct FilterAutomation {
    points: Vec<FilterFramePoint>,
}

impl FilterAutomation {
    fn value_at_frame(&self, frame: usize) -> f32 {
        let next_index = self.points.partition_point(|point| point.frame <= frame);
        match (next_index.checked_sub(1), self.points.get(next_index)) {
            (None, Some(_)) => 0.0,
            (Some(previous), None) => self.points[previous].value as f32,
            (Some(previous), Some(next)) => {
                let previous = &self.points[previous];
                if next.frame <= previous.frame || frame == next.frame {
                    return next.value as f32;
                }
                let mix = (frame - previous.frame) as f64 / (next.frame - previous.frame) as f64;
                let exponent = 2_f64.powf((previous.tension * 2.0).clamp(-2.0, 2.0));
                let curved_mix = mix.powf(exponent);
                (previous.value + (next.value - previous.value) * curved_mix) as f32
            }
            (None, None) => 0.0,
        }
    }
}

/// L'envoi de reverb d'une voie au fil du temps.
///
/// Même forme que les autres lignes — des points, une interpolation linéaire —
/// mais sans tension : une passe jouée n'a pas de courbure à régler, elle monte
/// et redescend.
#[derive(Clone, Debug, Default)]
struct SendAutomation {
    points: Vec<SendFramePoint>,
}

#[derive(Clone, Copy, Debug)]
struct SendFramePoint {
    frame: usize,
    value: f32,
}

impl SendAutomation {
    /// Zéro avant le premier point et là où il n'y en a aucun : une voie sans
    /// passe enregistrée n'envoie rien.
    fn value_at_frame(&self, frame: usize) -> f32 {
        let next_index = self.points.partition_point(|point| point.frame <= frame);
        match (next_index.checked_sub(1), self.points.get(next_index)) {
            (Some(previous), None) => self.points[previous].value,
            (Some(previous), Some(next)) => {
                let previous = &self.points[previous];
                if next.frame <= previous.frame {
                    return next.value;
                }
                let mix = (frame - previous.frame) as f32 / (next.frame - previous.frame) as f32;
                previous.value + (next.value - previous.value) * mix
            }
            _ => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BiquadKind {
    LowPass,
    HighPass,
    Peaking,
    Notch,
    LowShelf,
    HighShelf,
}

/// Difference-equation coefficients, already divided by `a0`.
#[derive(Clone, Copy, Debug)]
struct BiquadCoefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Default for BiquadCoefficients {
    /// Pass-through, so a filter that has never been configured is silent-safe.
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

impl BiquadCoefficients {
    fn design(kind: BiquadKind, cutoff_hz: f32, gain_db: f32, q: f32, sample_rate: u32) -> Self {
        let omega = std::f32::consts::TAU * cutoff_hz / sample_rate as f32;
        let sin_w = omega.sin();
        let cos_w = omega.cos();

        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::HighPass => {
                let alpha = sin_w / (2.0 * q);
                (
                    (1.0 + cos_w) * 0.5,
                    -(1.0 + cos_w),
                    (1.0 + cos_w) * 0.5,
                    1.0 + alpha,
                    -2.0 * cos_w,
                    1.0 - alpha,
                )
            }
            BiquadKind::LowPass => {
                let alpha = sin_w / (2.0 * q);
                (
                    (1.0 - cos_w) * 0.5,
                    1.0 - cos_w,
                    (1.0 - cos_w) * 0.5,
                    1.0 + alpha,
                    -2.0 * cos_w,
                    1.0 - alpha,
                )
            }
            BiquadKind::Peaking => {
                let alpha = sin_w / (2.0 * q);
                let a_factor = 10.0_f32.powf(gain_db / 40.0);
                (
                    1.0 + alpha * a_factor,
                    -2.0 * cos_w,
                    1.0 - alpha * a_factor,
                    1.0 + alpha / a_factor,
                    -2.0 * cos_w,
                    1.0 - alpha / a_factor,
                )
            }
            BiquadKind::Notch => {
                let alpha = sin_w / (2.0 * q);
                (
                    1.0,
                    -2.0 * cos_w,
                    1.0,
                    1.0 + alpha,
                    -2.0 * cos_w,
                    1.0 - alpha,
                )
            }
            // Shelves use a slope of 1, which reduces their alpha to this form.
            BiquadKind::LowShelf => {
                let a = 10.0_f32.powf(gain_db / 40.0);
                let alpha = sin_w * std::f32::consts::FRAC_1_SQRT_2;
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos_w + two_sqrt_a_alpha),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w),
                    a * ((a + 1.0) - (a - 1.0) * cos_w - two_sqrt_a_alpha),
                    (a + 1.0) + (a - 1.0) * cos_w + two_sqrt_a_alpha,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos_w),
                    (a + 1.0) + (a - 1.0) * cos_w - two_sqrt_a_alpha,
                )
            }
            BiquadKind::HighShelf => {
                let a = 10.0_f32.powf(gain_db / 40.0);
                let alpha = sin_w * std::f32::consts::FRAC_1_SQRT_2;
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos_w + two_sqrt_a_alpha),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w),
                    a * ((a + 1.0) + (a - 1.0) * cos_w - two_sqrt_a_alpha),
                    (a + 1.0) - (a - 1.0) * cos_w + two_sqrt_a_alpha,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos_w),
                    (a + 1.0) - (a - 1.0) * cos_w - two_sqrt_a_alpha,
                )
            }
        };

        if a0.abs() < f32::EPSILON || !a0.is_finite() {
            return Self::default();
        }
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
}

/// Identifies the design a cached set of coefficients came from.
type BiquadDesign = (BiquadKind, f32, f32, f32, u32);

#[derive(Clone, Copy, Debug, Default)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    /// Coefficients used to involve `sin`, `cos` and `powf` on every single
    /// sample, on the real-time thread, for every filter of every channel of
    /// every clip. They are now designed only when the shape actually changes:
    /// a static Clip EQ never redesigns, and a moving lane sweep redesigns once
    /// per frame instead of once per sample.
    design: Option<BiquadDesign>,
    coefficients: BiquadCoefficients,
}

impl BiquadState {
    fn coefficients(
        &mut self,
        kind: BiquadKind,
        cutoff_hz: f32,
        gain_db: f32,
        q: f32,
        sample_rate: u32,
    ) -> BiquadCoefficients {
        let cutoff = cutoff_hz.clamp(20.0, sample_rate as f32 * 0.45);
        let q = q.clamp(0.1, 10.0);
        let design = (kind, cutoff, gain_db, q, sample_rate);
        if self.design != Some(design) {
            self.coefficients = BiquadCoefficients::design(kind, cutoff, gain_db, q, sample_rate);
            self.design = Some(design);
        }
        self.coefficients
    }

    fn apply(&mut self, input: f32, coefficients: BiquadCoefficients) -> f32 {
        let output =
            coefficients.b0 * input + coefficients.b1 * self.x1 + coefficients.b2 * self.x2
                - coefficients.a1 * self.y1
                - coefficients.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }

    fn process(&mut self, input: f32, cutoff_hz: f32, sample_rate: u32, high_pass: bool) -> f32 {
        let kind = if high_pass {
            BiquadKind::HighPass
        } else {
            BiquadKind::LowPass
        };
        let coefficients = self.coefficients(kind, cutoff_hz, 0.0, FILTER_Q, sample_rate);
        self.apply(input, coefficients)
    }

    fn process_peaking(
        &mut self,
        input: f32,
        center_hz: f32,
        gain_db: f32,
        q: f32,
        sample_rate: u32,
    ) -> f32 {
        if gain_db.abs() < 0.01 {
            return input;
        }
        let coefficients =
            self.coefficients(BiquadKind::Peaking, center_hz, gain_db, q, sample_rate);
        self.apply(input, coefficients)
    }

    fn process_shelf(
        &mut self,
        input: f32,
        corner_hz: f32,
        gain_db: f32,
        sample_rate: u32,
        high: bool,
    ) -> f32 {
        let kind = if high {
            BiquadKind::HighShelf
        } else {
            BiquadKind::LowShelf
        };
        let coefficients = self.coefficients(kind, corner_hz, gain_db, 1.0, sample_rate);
        self.apply(input, coefficients)
    }

    fn process_notch(&mut self, input: f32, center_hz: f32, q: f32, sample_rate: u32) -> f32 {
        let coefficients = self.coefficients(BiquadKind::Notch, center_hz, 0.0, q, sample_rate);
        self.apply(input, coefficients)
    }
}

#[derive(Clone, Debug, Default)]
struct ClipEqState {
    high_pass: [BiquadState; 2],
    low_pass: [BiquadState; 2],
    peaking: [BiquadState; 2],
}

#[derive(Clone, Debug, Default)]
struct LaneFilterState {
    low_pass: [BiquadState; 2],
    high_pass: [BiquadState; 2],
}

fn filter_cutoff_hz(value: f32) -> f32 {
    if value >= 0.0 {
        FILTER_HIGH_PASS_OPEN_HZ
            * (FILTER_HIGH_PASS_CLOSED_HZ / FILTER_HIGH_PASS_OPEN_HZ).powf(value)
    } else {
        FILTER_LOW_PASS_OPEN_HZ * (FILTER_LOW_PASS_CLOSED_HZ / FILTER_LOW_PASS_OPEN_HZ).powf(-value)
    }
}

/// The ear perceives level approximately logarithmically. A linear ramp in dB
/// therefore keeps a filter sweep substantially more even than a linear
/// amplitude multiplier while keeping the maximum boost predictable.
fn filter_makeup_gain(value: f32) -> f32 {
    let amount = value.abs().clamp(0.0, 1.0);
    let maximum_db = if value >= 0.0 {
        FILTER_HIGH_PASS_MAX_MAKEUP_DB
    } else {
        FILTER_LOW_PASS_MAX_MAKEUP_DB
    };
    10_f32.powf((maximum_db * amount) / 20.0)
}

struct PcmWindow {
    decoder: Mp3Decoder,
    source_sample_rate: u32,
    source_channels: usize,
    samples: VecDeque<f32>,
    window_start_frame: usize,
    decoded_end_frame: usize,
    exhausted: bool,
    positioned: bool,
}

impl PcmWindow {
    fn open(path: &Path) -> Result<Self, String> {
        let decoder = open_mp3_decoder(path)?;
        let source_sample_rate = decoder.sample_rate().get();
        let source_channels = usize::from(decoder.channels().get());
        Ok(Self {
            decoder,
            source_sample_rate,
            source_channels,
            samples: VecDeque::with_capacity(
                (SOURCE_BACKTRACK_FRAMES
                    + WSOLA_MAX_SEARCH_FRAMES
                    + WSOLA_CORRELATION_FRAMES
                    + WSOLA_HOP_FRAMES * 2)
                    * OUTPUT_CHANNELS as usize,
            ),
            window_start_frame: 0,
            decoded_end_frame: 0,
            exhausted: false,
            positioned: false,
        })
    }

    fn blended_sample(&mut self, blend: GrainBlend, channel: usize) -> f32 {
        let minimum_position = blend
            .previous_position
            .map_or(blend.current_position, |previous| {
                previous.min(blend.current_position)
            });
        if !self.positioned
            && self
                .position_near(minimum_position.floor() as usize)
                .is_err()
        {
            self.exhausted = true;
            return 0.0;
        }

        let current = self.interpolated_sample(blend.current_position, channel);
        let output = blend
            .previous_position
            .map_or(current, |previous_position| {
                let previous = self.interpolated_sample(previous_position, channel);
                let fade_in = smooth_crossfade(blend.fade_in);
                previous * (1.0 - fade_in) + current * fade_in
            });
        self.discard_before(
            (minimum_position.floor() as usize).saturating_sub(SOURCE_BACKTRACK_FRAMES),
        );
        output
    }

    fn position_near(&mut self, source_frame: usize) -> Result<(), String> {
        self.decoder
            .try_seek(Duration::from_secs_f64(
                source_frame as f64 / f64::from(self.source_sample_rate),
            ))
            .map_err(|error| format!("Could not position the MP3 decoder: {error}"))?;
        self.samples.clear();
        self.window_start_frame = source_frame;
        self.decoded_end_frame = source_frame;
        self.exhausted = false;
        self.positioned = true;
        Ok(())
    }

    fn interpolated_sample(&mut self, source_position: f64, channel: usize) -> f32 {
        let first_frame = source_position.floor() as usize;
        self.ensure_decoded_through(first_frame.saturating_add(2));
        let fraction = (source_position - first_frame as f64) as f32;
        let before = self.buffered_sample(first_frame.saturating_sub(1), channel);
        let first = self.buffered_sample(first_frame, channel);
        let second = self.buffered_sample(first_frame.saturating_add(1), channel);
        let after = self.buffered_sample(first_frame.saturating_add(2), channel);
        cubic_interpolate(before, first, second, after, fraction)
    }

    fn align_to_previous(&mut self, reference_position: f64, nominal_position: f64) -> f64 {
        let required_correction = nominal_position - reference_position;
        if required_correction.abs() < 0.5 {
            return nominal_position;
        }

        // Pleine étendue, quelle que soit la correction : c'est la période du
        // son à recaler qui commande, pas la distance à rattraper.
        let search_radius = WSOLA_MAX_SEARCH_FRAMES;
        let nominal_frame = nominal_position.floor() as usize;
        let earliest_candidate = nominal_frame.saturating_sub(search_radius + 2);
        let earliest_reference = (reference_position.floor() as usize).saturating_sub(2);
        let required_start = earliest_candidate.min(earliest_reference);
        let latest_candidate = nominal_frame
            .saturating_add(search_radius)
            .saturating_add(WSOLA_CORRELATION_FRAMES)
            .saturating_add(2);
        let latest_reference = (reference_position.ceil() as usize)
            .saturating_add(WSOLA_CORRELATION_FRAMES)
            .saturating_add(2);

        if (!self.positioned || required_start < self.window_start_frame)
            && self.position_near(required_start).is_err()
        {
            return nominal_position;
        }
        self.ensure_decoded_through(latest_candidate.max(latest_reference));

        let offset = best_wsola_offset(
            reference_position,
            nominal_position,
            search_radius,
            |position, channel| self.buffered_interpolated_sample(position, channel),
        );
        nominal_position + offset as f64
    }

    fn buffered_interpolated_sample(&self, source_position: f64, channel: usize) -> f32 {
        let first_frame = source_position.floor() as usize;
        let fraction = (source_position - first_frame as f64) as f32;
        cubic_interpolate(
            self.buffered_sample(first_frame.saturating_sub(1), channel),
            self.buffered_sample(first_frame, channel),
            self.buffered_sample(first_frame.saturating_add(1), channel),
            self.buffered_sample(first_frame.saturating_add(2), channel),
            fraction,
        )
    }

    fn ensure_decoded_through(&mut self, requested_frame: usize) {
        while !self.exhausted && self.decoded_end_frame <= requested_frame {
            let Some([left, right]) = self.decode_next_stereo_frame() else {
                self.exhausted = true;
                break;
            };
            self.samples.push_back(left);
            self.samples.push_back(right);
            self.decoded_end_frame = self.decoded_end_frame.saturating_add(1);
        }
    }

    fn decode_next_stereo_frame(&mut self) -> Option<[f32; 2]> {
        let mut left = 0.0_f32;
        let mut right = 0.0_f32;
        for channel in 0..self.source_channels {
            let sample = self.decoder.next()?;
            match channel {
                0 => {
                    left = sample;
                    right = sample;
                }
                1 => right = sample,
                _ => {}
            }
        }
        Some([left, right])
    }

    fn buffered_sample(&self, source_frame: usize, channel: usize) -> f32 {
        let Some(relative_frame) = source_frame.checked_sub(self.window_start_frame) else {
            return 0.0;
        };
        self.samples
            .get(
                relative_frame
                    .saturating_mul(OUTPUT_CHANNELS as usize)
                    .saturating_add(channel),
            )
            .copied()
            .unwrap_or(0.0)
    }

    fn discard_before(&mut self, source_frame: usize) {
        let frames_to_discard = source_frame
            .saturating_sub(self.window_start_frame)
            .min(self.samples.len() / OUTPUT_CHANNELS as usize);
        for _ in 0..frames_to_discard.saturating_mul(OUTPUT_CHANNELS as usize) {
            self.samples.pop_front();
        }
        self.window_start_frame = self.window_start_frame.saturating_add(frames_to_discard);
    }
}

fn cubic_interpolate(before: f32, first: f32, second: f32, after: f32, mix: f32) -> f32 {
    let a = -0.5 * before + 1.5 * first - 1.5 * second + 0.5 * after;
    let b = before - 2.5 * first + 2.0 * second - 0.5 * after;
    let c = -0.5 * before + 0.5 * second;
    ((a * mix + b) * mix + c) * mix + first
}

fn smooth_crossfade(mix: f32) -> f32 {
    0.5 - 0.5 * (std::f32::consts::PI * mix.clamp(0.0, 1.0)).cos()
}

fn best_wsola_offset(
    reference_position: f64,
    nominal_position: f64,
    search_radius: usize,
    mut sample_at: impl FnMut(f64, usize) -> f32,
) -> isize {
    let radius = isize::try_from(search_radius).unwrap_or(isize::MAX);
    const CORRELATION_SAMPLES: usize = WSOLA_CORRELATION_FRAMES / WSOLA_CORRELATION_STRIDE;
    let mut reference_samples = [0.0_f64; CORRELATION_SAMPLES];
    let mut reference_energy = 0.0_f64;
    for (index, frame) in (0..WSOLA_CORRELATION_FRAMES)
        .step_by(WSOLA_CORRELATION_STRIDE)
        .enumerate()
    {
        let frame = frame as f64;
        let reference = 0.5
            * f64::from(
                sample_at(reference_position + frame, 0) + sample_at(reference_position + frame, 1),
            );
        reference_samples[index] = reference;
        reference_energy += reference * reference;
    }

    // L'énergie de la référence pour la passe grossière, qui ne lit qu'un point
    // sur quatre de la série ci-dessus : la comparaison doit porter sur le même
    // sous-ensemble des deux côtés, sinon la normalisation ment.
    let coarse_reference_energy: f64 = reference_samples
        .iter()
        .step_by(WSOLA_COARSE_DECIMATION)
        .map(|value| value * value)
        .sum();

    // Recherche grossière puis fine, plutôt qu'un balayage plein.
    //
    // Le rayon doit couvrir une période du grave — c'est ce qui rend le
    // recalage possible — mais il n'a jamais fallu **noter** mille candidats à
    // pleine résolution pour le trouver. En le faisant, chaque grain coûtait
    // cent trente-cinq mille interpolations : sur le fil audio, avec deux clips
    // étirés qui se superposent, cela suffit à faire craquer la sortie. La
    // corrélation est lisse à cette échelle : une passe à gros pas trouve la
    // bonne bosse, deux passes serrées trouvent son sommet.
    let mut score_offset = |offset: isize, decimation: usize| {
        let candidate_position = nominal_position + offset as f64;
        if candidate_position < 0.0 {
            return f64::NEG_INFINITY;
        }
        let mut correlation = 0.0_f64;
        let mut candidate_energy = 0.0_f64;

        for index in (0..CORRELATION_SAMPLES).step_by(decimation) {
            let frame = (index * WSOLA_CORRELATION_STRIDE) as f64;
            let candidate = 0.5
                * f64::from(
                    sample_at(candidate_position + frame, 0)
                        + sample_at(candidate_position + frame, 1),
                );
            correlation += reference_samples[index] * candidate;
            candidate_energy += candidate * candidate;
        }

        let reference_energy = if decimation == 1 {
            reference_energy
        } else {
            coarse_reference_energy
        };
        let normalization = (reference_energy * candidate_energy).sqrt();
        let similarity = if normalization > 1.0e-12 {
            correlation / normalization
        } else {
            0.0
        };
        let distance = offset as f64 / search_radius.max(1) as f64;
        similarity - distance * distance * 0.025
    };

    let mut best_offset = 0_isize;
    let mut best_score = f64::NEG_INFINITY;
    for offset in (-radius..=radius).step_by(WSOLA_COARSE_OFFSET_STEP) {
        let score = score_offset(offset, WSOLA_COARSE_DECIMATION);
        if score > best_score {
            best_score = score;
            best_offset = offset;
        }
    }

    // Les deux passes suivantes reprennent à pleine résolution : la grossière a
    // dit *quelle* bosse, elles disent où en est le sommet. Le voisinage couvre
    // un pas entier de chaque côté, pour que la bonne réponse reste atteignable
    // quand la grossière s'est arrêtée juste à côté.
    let mut refine = |centre: isize, span: isize, step: usize, best: &mut (isize, f64)| {
        let from = (centre - span).max(-radius);
        let to = (centre + span).min(radius);
        for offset in (from..=to).step_by(step) {
            let score = score_offset(offset, 1);
            if score > best.1 {
                *best = (offset, score);
            }
        }
    };

    // Le score grossier et le score fin ne sont pas comparables — ils ne
    // portent pas sur le même nombre de points. La finesse repart donc d'un
    // score neuf.
    let mut best = (best_offset, f64::NEG_INFINITY);
    refine(best_offset, WSOLA_COARSE_OFFSET_STEP as isize, 4, &mut best);
    refine(best.0, 3, 1, &mut best);
    best.0
}

#[derive(Clone, Copy, Debug)]
struct GrainStartCache {
    grain_index: usize,
    current_position: f64,
    previous_position: Option<f64>,
}

struct PlacedClip {
    file_path: String,
    lane: usize,
    start_frame: usize,
    output_frames: usize,
    visual_start_beat: f64,
    source_bpm: f64,
    trim_start_beats: f64,
    trim_end_beats: f64,
    is_sidechain_key: bool,
    eq_settings: Option<crate::timeline::ClipEqSettings>,
    eq_state: ClipEqState,
    grain_cache: Option<GrainStartCache>,
    reader: Option<PcmWindow>,
    failed: bool,
}

impl Clone for PlacedClip {
    fn clone(&self) -> Self {
        Self {
            file_path: self.file_path.clone(),
            lane: self.lane,
            start_frame: self.start_frame,
            output_frames: self.output_frames,
            visual_start_beat: self.visual_start_beat,
            source_bpm: self.source_bpm,
            trim_start_beats: self.trim_start_beats,
            trim_end_beats: self.trim_end_beats,
            is_sidechain_key: self.is_sidechain_key,
            eq_settings: self.eq_settings.clone(),
            eq_state: ClipEqState::default(),
            grain_cache: None,
            reader: None,
            failed: false,
        }
    }
}

impl PlacedClip {
    fn end_frame(&self) -> usize {
        self.start_frame.saturating_add(self.output_frames)
    }

    fn sample_at(
        &mut self,
        timeline_frame: usize,
        channel: usize,
        output_sample_rate: u32,
        tempo_map: &TempoMap,
    ) -> f32 {
        let Some(output_frame) = timeline_frame.checked_sub(self.start_frame) else {
            return 0.0;
        };
        if output_frame >= self.output_frames || self.failed {
            return 0.0;
        }
        if self.reader.is_none() {
            match PcmWindow::open(Path::new(&self.file_path)) {
                Ok(reader) => self.reader = Some(reader),
                Err(_) => {
                    self.failed = true;
                    return 0.0;
                }
            }
        }
        let source_sample_rate = self
            .reader
            .as_ref()
            .map_or(output_sample_rate, |reader| reader.source_sample_rate);
        let blend = self.grain_blend(
            output_frame,
            source_sample_rate,
            output_sample_rate,
            tempo_map,
        );
        let raw_sample = self
            .reader
            .as_mut()
            .map_or(0.0, |reader| reader.blended_sample(blend, channel));

        if let Some(eq) = &self.eq_settings
            && eq.enabled.unwrap_or(true)
        {
            let ch = channel % 2;
            let mut s = raw_sample;

            // 1. High Pass Filter (HPF)
            if eq.high_pass_hz > 20.5 {
                s = self.eq_state.high_pass[ch].process(
                    s,
                    eq.high_pass_hz as f32,
                    output_sample_rate,
                    true,
                );
            }

            // 2. Low Pass Filter (LPF)
            if eq.low_pass_hz < 19950.0 {
                s = self.eq_state.low_pass[ch].process(
                    s,
                    eq.low_pass_hz as f32,
                    output_sample_rate,
                    false,
                );
            }

            // 3. 3rd Parametric EQ (Bell / Peaking or Notch Cut)
            if let (Some(peak_hz), Some(peak_gain_db)) = (eq.peak_hz, eq.peak_gain_db) {
                let q = eq.peak_q.unwrap_or(1.0) as f32;
                if peak_gain_db <= CLIP_EQ_SILENCE_DB {
                    s = self.eq_state.peaking[ch].process_notch(
                        s,
                        peak_hz as f32,
                        q,
                        output_sample_rate,
                    );
                } else if peak_gain_db.abs() > 0.05 {
                    s = self.eq_state.peaking[ch].process_peaking(
                        s,
                        peak_hz as f32,
                        peak_gain_db as f32,
                        q,
                        output_sample_rate,
                    );
                }
            }

            // 4. Overall Clip Gain (-inf dB to +12 dB)
            if let Some(gain_db) = eq.gain_db {
                if gain_db <= CLIP_EQ_SILENCE_DB {
                    return 0.0;
                }
                if gain_db.abs() > 0.01 {
                    let linear_gain = 10.0_f32.powf((gain_db as f32) / 20.0);
                    s *= linear_gain;
                }
            }

            return s;
        }

        raw_sample
    }

    fn grain_blend(
        &mut self,
        output_frame: usize,
        source_sample_rate: u32,
        output_sample_rate: u32,
        tempo_map: &TempoMap,
    ) -> GrainBlend {
        let grain_index = output_frame / WSOLA_HOP_FRAMES;
        let phase = output_frame % WSOLA_HOP_FRAMES;
        let cache = match self.grain_cache {
            Some(cache) if cache.grain_index == grain_index => cache,
            _ => {
                let current_frame = self
                    .start_frame
                    .saturating_add(grain_index.saturating_mul(WSOLA_HOP_FRAMES));
                let nominal_position = self.source_position_at_timeline_frame(
                    current_frame,
                    source_sample_rate,
                    output_sample_rate,
                    tempo_map,
                );
                let source_frames_per_output_frame =
                    f64::from(source_sample_rate) / f64::from(output_sample_rate);
                let previous_position = self.grain_cache.map(|previous| {
                    previous.current_position
                        + WSOLA_HOP_FRAMES as f64 * source_frames_per_output_frame
                });
                let current_position = match (previous_position, self.reader.as_mut()) {
                    (Some(reference), Some(reader)) => {
                        reader.align_to_previous(reference, nominal_position)
                    }
                    _ => nominal_position,
                };
                let cache = GrainStartCache {
                    grain_index,
                    current_position,
                    previous_position,
                };
                self.grain_cache = Some(cache);
                cache
            }
        };
        let source_frames_per_output_frame =
            f64::from(source_sample_rate) / f64::from(output_sample_rate);
        GrainBlend {
            current_position: cache.current_position
                + phase as f64 * source_frames_per_output_frame,
            previous_position: cache
                .previous_position
                .map(|position| position + phase as f64 * source_frames_per_output_frame),
            fade_in: if grain_index == 0 {
                1.0
            } else {
                (phase as f32 / WSOLA_FADE_FRAMES as f32).min(1.0)
            },
        }
    }

    fn source_position_at_timeline_frame(
        &self,
        timeline_frame: usize,
        source_sample_rate: u32,
        output_sample_rate: u32,
        tempo_map: &TempoMap,
    ) -> f64 {
        let timeline_seconds = timeline_frame as f64 / f64::from(output_sample_rate);
        let timeline_beat = tempo_map.beat_at_seconds(timeline_seconds);
        let source_beat = (timeline_beat - self.visual_start_beat) + self.trim_start_beats;
        let source_seconds = (source_beat * 60.0 / self.source_bpm).max(0.0);
        source_seconds * f64::from(source_sample_rate)
    }

    fn reset_reader(&mut self) {
        self.reader = None;
        self.failed = false;
        self.grain_cache = None;
    }
}

/// Ce que les effets **à queue** gardent entre deux images, réuni à part.
///
/// Il ne vit pas dans la source, mais à côté d'elle, et c'est la correction
/// d'un défaut de fond. Écrire une passe reconstruit le plan, donc une nouvelle
/// source, donc — jusqu'ici — une pièce vide et une ligne à retard vide : la
/// queue mourait à l'instant précis où l'on relâchait le bouton, c'est-à-dire
/// à l'instant où elle devait commencer. Le même écueil que la taille de pièce
/// en son temps, et pour la même raison : **l'état d'un effet n'appartient pas
/// au plan.**
///
/// Le bitcrush n'y est pas : c'est un insert, son seul état est un échantillon
/// retenu quelques dizaines de microsecondes, et le perdre ne s'entend pas. Le
/// garder ici aurait obligé à prendre le verrou dans la boucle par échantillon
/// plutôt qu'une fois par image.
pub(crate) struct EffectTails {
    reverb: Reverb,
    flanger: Flanger,
    delay: Delay,
    /// Combien d'images il reste à calculer chaque effet une fois son envoi
    /// retombé. Sans ce décompte, leurs lignes tourneraient sur du silence
    /// pendant tout le mix.
    reverb_frames: usize,
    flanger_frames: usize,
    delay_frames: usize,
    /// Les budgets pleins, calculés une fois pour la fréquence de sortie. Ils
    /// vivent ici parce que c'est ici qu'on les recharge, et qu'un budget
    /// recalculé ailleurs finirait par décrire une autre durée.
    reverb_budget: usize,
    flanger_budget: usize,
    delay_budget: usize,
}

impl EffectTails {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            reverb: Reverb::new(sample_rate),
            flanger: Flanger::new(sample_rate),
            delay: Delay::new(sample_rate),
            reverb_frames: 0,
            flanger_frames: 0,
            delay_frames: 0,
            reverb_budget: tail_frames(REVERB_TAIL_SECONDS, sample_rate),
            flanger_budget: tail_frames(FLANGER_TAIL_SECONDS, sample_rate),
            delay_budget: tail_frames(DELAY_TAIL_SECONDS, sample_rate),
        }
    }

    /// Vide tout. Appelé sur un déplacement **voulu** de la tête de lecture, et
    /// sur lui seul : la queue de l'endroit qu'on quitte n'a rien à faire à
    /// l'endroit où l'on arrive, et on l'entendrait très bien. Une édition qui
    /// reconstruit le plan, elle, ne déplace personne et ne doit rien vider.
    pub(crate) fn reset(&mut self) {
        self.reverb.reset();
        self.flanger.reset();
        self.delay.reset();
        self.reverb_frames = 0;
        self.flanger_frames = 0;
        self.delay_frames = 0;
    }
}

pub(crate) struct TimelineMixSource {
    clips: Vec<PlacedClip>,
    total_frames: usize,
    position_sample: usize,
    next_start_index: usize,
    active_indices: Vec<usize>,
    volume_automation: [VolumeAutomation; 3],
    pan_automation: [PanAutomation; 3],
    filter_automation: [FilterAutomation; 3],
    reverb_automation: [SendAutomation; 3],
    flanger_automation: [SendAutomation; 3],
    bitcrush_automation: [SendAutomation; 3],
    delay_automation: [SendAutomation; 3],
    filter_states: [LaneFilterState; 3],
    filter_values: [f32; 3],
    audible_lane_mask: Arc<AtomicU8>,
    limiter_enabled: Arc<AtomicBool>,
    compressor_enabled: Arc<AtomicBool>,
    compressor: MasterCompressor,
    ducker: SidechainDucker,
    colour: MasterColour,
    colour_amount: f32,
    /// Les effets à queue, **partagés** avec les sources qui suivront.
    ///
    /// Leurs envois sont pris **après** le filtre, le volume et le panoramique
    /// de la piste — des départs post-fader, donc baisser une piste baisse ses
    /// effets — et leurs retours se somment au master après le compresseur,
    /// avant le VU et le limiteur. Les queues échappent ainsi au pompage du
    /// sidechain, et le limiteur les voit : une longue queue ne peut pas
    /// pousser la sortie sans que rien ne la retienne.
    ///
    /// Le verrou est pris **une fois par image**, jamais par échantillon, et il
    /// n'est jamais disputé : une seule source tire à la fois.
    tails: Arc<Mutex<EffectTails>>,
    /// Quelles pistes ont leur bouton enfoncé — un bit par piste.
    ///
    /// Partagé atomiquement avec la source déjà en file, comme Mute et Solo :
    /// appuyer s'entend tout de suite, sans reconstruire le plan.
    reverb_keys: Arc<AtomicU8>,
    /// Les mêmes bits, pour le flanger.
    flanger_keys: Arc<AtomicU8>,
    /// Et pour le bitcrush.
    bitcrush_keys: Arc<AtomicU8>,
    /// Et pour le delay.
    delay_keys: Arc<AtomicU8>,
    /// Quelles pistes ont la gomme tenue au-dessus d'elles.
    ///
    /// La gomme n'écrit dans la base qu'au relâchement, si bien que le plan en
    /// cours contient encore la passe qu'on est en train de retirer : sans ce
    /// masque, on entendait la reverb continuer sous la gomme et l'on ne savait
    /// pas ce qu'on avait effacé avant de lever le doigt. Effacer doit
    /// s'entendre pendant le geste, comme jouer.
    ///
    /// Il ne coupe que la passe **enregistrée**. Tenir la reverb et la gomme
    /// ensemble sur une piste laisse donc entendre ce qu'on joue, ce qui est
    /// bien ce qui restera : réécrire par-dessus est le seul geste qui survit à
    /// l'effacement.
    effect_erase: Arc<AtomicU8>,
    /// Vrai tant que `FFWD` est tenu.
    ///
    /// Le gain d'envoi réellement appliqué, qui rejoint la consigne par une
    /// rampe.
    ///
    /// Un créneau franc claque. La montée est courte et fixe — assez pour
    /// tomber sur le temps —, la descente suit le tempo, si bien qu'elle
    /// respire avec le morceau au lieu d'être une constante arbitraire.
    reverb_sends: [f32; 3],
    flanger_sends: [f32; 3],
    /// Le dosage du bitcrush par voie.
    ///
    /// « Envoi » est un abus de langage ici : rien n'est envoyé nulle part, ce
    /// nombre est la position d'un fondu entre le son sec et le son broyé. Il
    /// suit pourtant exactement la même rampe que les deux autres, puisque
    /// c'est le même geste qui le pilote.
    bitcrush_sends: [f32; 3],
    /// Un broyeur par voie : c'est un insert, il travaille sur le signal de
    /// **cette** piste.
    bitcrushers: [BitCrusher; 3],
    delay_sends: [f32; 3],
    limiter: MasterLimiter,
    /// Both channels of the current frame, already limited. Rendered together
    /// so the limiter can link them.
    pending_frame: [f32; OUTPUT_CHANNELS as usize],
    tempo_map: TempoMap,
    output_sample_rate: u32,
    meter: Arc<StereoMeterState>,
    meter_epoch: u32,
    meter_left_envelope: f32,
    meter_right_envelope: f32,
    meter_pending_left: f32,
    /// Le coefficient de chute du mètre, calculé une fois.
    ///
    /// Il ne dépend que de la fréquence de sortie et d'une constante, et il
    /// était pourtant recalculé à chaque échantillon — une exponentielle sur
    /// le fil temps réel, quarante-huit mille fois par seconde et par canal,
    /// pour toujours rendre le même nombre.
    meter_release: f32,
    overload_hold_frames: usize,
}

impl TimelineMixSource {
    /// Reprend les queues d'une source précédente plutôt que les siennes.
    ///
    /// C'est ce qui les fait survivre à une reconstruction du plan. Rien
    /// d'autre n'est repris : les clips, l'automation et le tempo viennent bien
    /// du plan neuf.
    fn share_tails(&mut self, tails: Arc<Mutex<EffectTails>>) {
        self.tails = tails;
    }
}

impl Clone for TimelineMixSource {
    fn clone(&self) -> Self {
        let mut source = Self {
            clips: self.clips.clone(),
            total_frames: self.total_frames,
            position_sample: self.position_sample,
            next_start_index: 0,
            active_indices: Vec::new(),
            volume_automation: self.volume_automation.clone(),
            pan_automation: self.pan_automation.clone(),
            filter_automation: self.filter_automation.clone(),
            reverb_automation: self.reverb_automation.clone(),
            flanger_automation: self.flanger_automation.clone(),
            bitcrush_automation: self.bitcrush_automation.clone(),
            delay_automation: self.delay_automation.clone(),
            filter_states: std::array::from_fn(|_| LaneFilterState::default()),
            filter_values: [0.0; 3],
            audible_lane_mask: Arc::clone(&self.audible_lane_mask),
            limiter_enabled: Arc::clone(&self.limiter_enabled),
            compressor_enabled: Arc::clone(&self.compressor_enabled),
            compressor: MasterCompressor::default(),
            ducker: SidechainDucker::default(),
            colour: MasterColour::default(),
            colour_amount: 0.0,
            tails: Arc::clone(&self.tails),
            bitcrushers: std::array::from_fn(|_| BitCrusher::new(self.output_sample_rate)),
            reverb_keys: Arc::clone(&self.reverb_keys),
            flanger_keys: Arc::clone(&self.flanger_keys),
            bitcrush_keys: Arc::clone(&self.bitcrush_keys),
            delay_keys: Arc::clone(&self.delay_keys),
            effect_erase: Arc::clone(&self.effect_erase),
            reverb_sends: [0.0; 3],
            flanger_sends: [0.0; 3],
            bitcrush_sends: [0.0; 3],
            delay_sends: [0.0; 3],
            limiter: MasterLimiter::default(),
            pending_frame: [0.0; OUTPUT_CHANNELS as usize],
            tempo_map: self.tempo_map.clone(),
            output_sample_rate: self.output_sample_rate,
            meter: Arc::clone(&self.meter),
            meter_epoch: self.meter.epoch(),
            meter_left_envelope: 0.0,
            meter_right_envelope: 0.0,
            meter_pending_left: 0.0,
            meter_release: meter_release_coefficient(self.output_sample_rate),
            overload_hold_frames: 0,
        };
        source.rebuild_active_clips(source.position_sample / OUTPUT_CHANNELS as usize);
        source
    }
}

/// Which master dynamics processors the project has switched on.
#[derive(Clone, Copy, Debug)]
struct MasterDynamics {
    compressor_enabled: bool,
    limiter_enabled: bool,
}

/// The per-lane automation curves, which always travel together.
#[derive(Clone, Debug)]
struct LaneAutomation {
    volume: [VolumeAutomation; 3],
    pan: [PanAutomation; 3],
    filter: [FilterAutomation; 3],
    reverb: [SendAutomation; 3],
    flanger: [SendAutomation; 3],
    bitcrush: [SendAutomation; 3],
    delay: [SendAutomation; 3],
}

impl TimelineMixSource {
    fn new(
        mut clips: Vec<PlacedClip>,
        total_frames: usize,
        audible_lane_mask: u8,
        dynamics: MasterDynamics,
        tempo_map: TempoMap,
        output_sample_rate: u32,
        automation: LaneAutomation,
    ) -> Self {
        clips.sort_by_key(|clip| clip.start_frame);
        let meter = Arc::new(StereoMeterState::default());
        Self {
            clips,
            total_frames,
            position_sample: 0,
            next_start_index: 0,
            active_indices: Vec::new(),
            volume_automation: automation.volume,
            pan_automation: automation.pan,
            filter_automation: automation.filter,
            reverb_automation: automation.reverb,
            flanger_automation: automation.flanger,
            bitcrush_automation: automation.bitcrush,
            delay_automation: automation.delay,
            filter_states: std::array::from_fn(|_| LaneFilterState::default()),
            filter_values: [0.0; 3],
            audible_lane_mask: Arc::new(AtomicU8::new(audible_lane_mask)),
            limiter_enabled: Arc::new(AtomicBool::new(dynamics.limiter_enabled)),
            compressor_enabled: Arc::new(AtomicBool::new(dynamics.compressor_enabled)),
            compressor: MasterCompressor::default(),
            ducker: SidechainDucker::default(),
            colour: MasterColour::default(),
            colour_amount: 0.0,
            tails: Arc::new(Mutex::new(EffectTails::new(output_sample_rate))),

            bitcrushers: std::array::from_fn(|_| BitCrusher::new(output_sample_rate)),
            reverb_keys: Arc::new(AtomicU8::new(0)),
            flanger_keys: Arc::new(AtomicU8::new(0)),
            bitcrush_keys: Arc::new(AtomicU8::new(0)),
            delay_keys: Arc::new(AtomicU8::new(0)),
            effect_erase: Arc::new(AtomicU8::new(0)),
            reverb_sends: [0.0; 3],
            flanger_sends: [0.0; 3],
            bitcrush_sends: [0.0; 3],
            delay_sends: [0.0; 3],
            limiter: MasterLimiter::default(),
            pending_frame: [0.0; OUTPUT_CHANNELS as usize],
            tempo_map,
            output_sample_rate,
            meter,
            meter_epoch: 0,
            meter_left_envelope: 0.0,
            meter_right_envelope: 0.0,
            meter_pending_left: 0.0,
            meter_release: meter_release_coefficient(output_sample_rate),
            overload_hold_frames: 0,
        }
    }

    fn refresh_active_clips(&mut self, frame: usize) {
        self.active_indices
            .retain(|index| self.clips[*index].end_frame() > frame);
        while self
            .clips
            .get(self.next_start_index)
            .is_some_and(|clip| clip.start_frame <= frame)
        {
            if self.clips[self.next_start_index].end_frame() > frame {
                self.active_indices.push(self.next_start_index);
            }
            self.next_start_index += 1;
        }
    }

    fn rebuild_active_clips(&mut self, frame: usize) {
        for clip in &mut self.clips {
            clip.reset_reader();
        }
        self.next_start_index = self.clips.partition_point(|clip| clip.start_frame <= frame);
        self.active_indices = (0..self.next_start_index)
            .filter(|index| self.clips[*index].end_frame() > frame)
            .collect();
        self.compressor.reset();
        self.ducker.reset();
        self.colour = MasterColour::default();
        self.limiter.reset();
        self.pending_frame = [0.0; OUTPUT_CHANNELS as usize];
        self.reset_meter_envelopes();
    }

    fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.total_frames as f64 / f64::from(self.output_sample_rate))
    }

    fn set_audible_lane_mask(&self, audible_lane_mask: u8) {
        self.audible_lane_mask
            .store(audible_lane_mask, Ordering::Relaxed);
    }

    fn set_limiter_enabled(&self, limiter_enabled: bool) {
        self.limiter_enabled
            .store(limiter_enabled, Ordering::Relaxed);
    }

    /// Quelles pistes tiennent leur bouton de reverb enfoncé.
    fn set_reverb_keys(&self, keys: u8) {
        self.reverb_keys.store(keys, Ordering::Relaxed);
    }

    /// Quelles pistes tiennent leur bouton de flanger enfoncé.
    fn set_flanger_keys(&self, keys: u8) {
        self.flanger_keys.store(keys, Ordering::Relaxed);
    }

    /// Quelles pistes tiennent leur bouton de bitcrush enfoncé.
    fn set_bitcrush_keys(&self, keys: u8) {
        self.bitcrush_keys.store(keys, Ordering::Relaxed);
    }

    /// Quelles pistes tiennent leur bouton de delay enfoncé.
    fn set_delay_keys(&self, keys: u8) {
        self.delay_keys.store(keys, Ordering::Relaxed);
    }

    /// Quelles pistes ont la gomme tenue au-dessus d'elles.
    fn set_effect_erase(&self, lanes: u8) {
        self.effect_erase.store(lanes, Ordering::Relaxed);
    }

    fn set_compressor_enabled(&self, compressor_enabled: bool) {
        self.compressor_enabled
            .store(compressor_enabled, Ordering::Relaxed);
    }

    fn meter_levels(&self) -> (f32, f32, bool) {
        self.meter.levels()
    }

    fn reset_meter(&self) {
        self.meter.reset();
    }

    fn reset_meter_envelopes(&mut self) {
        self.meter.reset();
        self.meter_epoch = self.meter.epoch();
        self.meter_left_envelope = 0.0;
        self.meter_right_envelope = 0.0;
        self.meter_pending_left = 0.0;
        self.overload_hold_frames = 0;
    }

    fn update_meter(&mut self, frame: usize, channel: usize, sample: f32) {
        let current_epoch = self.meter.epoch();
        if current_epoch != self.meter_epoch {
            self.meter_epoch = current_epoch;
            self.meter_left_envelope = 0.0;
            self.meter_right_envelope = 0.0;
            self.meter_pending_left = 0.0;
            self.overload_hold_frames = 0;
        }

        if channel == 0 {
            self.meter_pending_left = sample.abs();
            return;
        }

        self.meter_left_envelope = vu_envelope(
            self.meter_left_envelope,
            self.meter_pending_left,
            self.meter_release,
        );
        self.meter_right_envelope =
            vu_envelope(self.meter_right_envelope, sample.abs(), self.meter_release);
        if frame.is_multiple_of(METER_PUBLISH_FRAMES) {
            self.meter
                .store(self.meter_left_envelope, self.meter_right_envelope);
        }
    }

    /// `OL` reports an overshoot the output actually suffered: it is measured
    /// after the limiter, on the value the hard bound had to shave off. With
    /// the limiter engaged it therefore stays dark unless a transient outruns
    /// the attack; with the limiter bypassed it lights on every clipped peak.
    fn update_overload(&mut self, clipped: bool) {
        if clipped {
            self.overload_hold_frames = self.output_sample_rate as usize * 3 / 4;
        } else {
            self.overload_hold_frames = self.overload_hold_frames.saturating_sub(1);
        }
        self.meter.set_overload(self.overload_hold_frames > 0);
    }

    fn update_filter_values(&mut self, frame: usize) {
        let coefficient =
            (-1.0 / (self.output_sample_rate as f32 * FILTER_SMOOTHING_SECONDS)).exp();
        for lane in 0..self.filter_values.len() {
            let target = self.filter_automation[lane].value_at_frame(frame);
            self.filter_values[lane] = target + coefficient * (self.filter_values[lane] - target);
        }
    }

    fn filter_lane_sample(&mut self, lane: usize, channel: usize, input: f32) -> f32 {
        let value = self.filter_values[lane];
        let amount = value.abs();
        if amount <= 0.000_1 {
            return input;
        }
        let wet = if value > 0.0 {
            self.filter_states[lane].high_pass[channel].process(
                input,
                filter_cutoff_hz(value),
                self.output_sample_rate,
                true,
            )
        } else {
            self.filter_states[lane].low_pass[channel].process(
                input,
                filter_cutoff_hz(value),
                self.output_sample_rate,
                false,
            )
        };
        (input + (wet - input) * amount) * filter_makeup_gain(value)
    }

    /// Renders both channels of one frame into `pending_frame`.
    ///
    /// The two channels are produced together so the limiter can derive a
    /// single gain from the frame peak: a per-channel gain would move the
    /// stereo image whenever one side is louder than the other.
    fn render_frame(&mut self, frame: usize) {
        let audible = self.audible_lane_mask.load(Ordering::Relaxed);
        let mut master = [0.0_f32; OUTPUT_CHANNELS as usize];

        // The key clip is silent only where it actually covers something. On
        // its own it plays like any other clip, which is what lets a whole
        // track serve as the key rather than a muted trigger loop.
        let key_position = self.active_indices.iter().position(|index| {
            let clip = &self.clips[*index];
            clip.is_sidechain_key && audible & (1_u8 << clip.lane) != 0
        });
        let key_covers_something = key_position.is_some_and(|_| {
            self.active_indices
                .iter()
                .filter(|index| {
                    let clip = &self.clips[**index];
                    !clip.is_sidechain_key && audible & (1_u8 << clip.lane) != 0
                })
                .count()
                > 0
        });
        let keying = key_covers_something;
        let mut key_frame = [0.0_f32; OUTPUT_CHANNELS as usize];
        let mut reverb_send = [0.0_f32; OUTPUT_CHANNELS as usize];
        let mut flanger_send = [0.0_f32; OUTPUT_CHANNELS as usize];
        let mut delay_send = [0.0_f32; OUTPUT_CHANNELS as usize];

        // Les envois rejoignent leur consigne par une rampe, calculée une fois
        // par image et non par échantillon : les deux canaux d'une même image
        // doivent partager exactement le même gain, sans quoi l'image stéréo
        // bouge pendant la montée.

        let reverb_keys = self.reverb_keys.load(Ordering::Relaxed);
        let flanger_keys = self.flanger_keys.load(Ordering::Relaxed);
        let bitcrush_keys = self.bitcrush_keys.load(Ordering::Relaxed);
        let delay_keys = self.delay_keys.load(Ordering::Relaxed);
        // Un seul masque de gomme pour les deux effets : la gomme est un seul
        // bouton, et ce qu'elle balaie, elle l'emporte en entier.
        let erasing = self.effect_erase.load(Ordering::Relaxed);
        let attack = 1.0 / (REVERB_ATTACK_SECONDS * self.output_sample_rate as f32).max(1.0);
        let beat_seconds = 60.0
            / self.tempo_map.bpm_at_beat(
                self.tempo_map
                    .beat_at_seconds(frame as f64 / f64::from(self.output_sample_rate)),
            ) as f32;
        let release =
            1.0 / (beat_seconds * REVERB_RELEASE_BEATS * self.output_sample_rate as f32).max(1.0);
        for lane in 0..self.reverb_sends.len() {
            let bit = 1_u8 << lane;
            let erased = erasing & bit != 0;
            self.reverb_sends[lane] = next_effect_send(
                self.reverb_sends[lane],
                reverb_keys & bit != 0,
                erased,
                self.reverb_automation[lane].value_at_frame(frame),
                attack,
                release,
            );
            self.flanger_sends[lane] = next_effect_send(
                self.flanger_sends[lane],
                flanger_keys & bit != 0,
                erased,
                self.flanger_automation[lane].value_at_frame(frame),
                attack,
                release,
            );
            self.bitcrush_sends[lane] = next_effect_send(
                self.bitcrush_sends[lane],
                bitcrush_keys & bit != 0,
                erased,
                self.bitcrush_automation[lane].value_at_frame(frame),
                attack,
                release,
            );
            self.delay_sends[lane] = next_effect_send(
                self.delay_sends[lane],
                delay_keys & bit != 0,
                erased,
                self.delay_automation[lane].value_at_frame(frame),
                attack,
                release,
            );
        }

        // La longueur de l'écho, en échantillons, recalculée à chaque image
        // depuis le tempo de cette image — `beat_seconds` est déjà là pour la
        // descente des envois, donc cela ne coûte rien de plus. C'est ce qui
        // fait tenir l'écho sur le temps même pendant une rampe de BPM, là où
        // un delay réglé en millisecondes se décalerait.
        let delay_samples = beat_seconds * DELAY_BEATS * self.output_sample_rate as f32;

        // L'horloge de maintien du bitcrush avance **une fois par image**, avant
        // les canaux : les deux canaux doivent être retenus et relâchés
        // ensemble, sans quoi leur maintien glisserait de l'un à l'autre et
        // l'image stéréo se déchirerait.
        let mut crush_latch = [false; 3];
        for (lane, latch) in crush_latch.iter_mut().enumerate() {
            *latch = self.bitcrushers[lane].tick();
        }

        for (channel, master_sample) in master.iter_mut().enumerate() {
            let mut lane_mix = [0.0_f32; 3];
            for active_position in 0..self.active_indices.len() {
                let clip_index = self.active_indices[active_position];
                let lane = self.clips[clip_index].lane;
                if audible & (1_u8 << lane) == 0 {
                    continue;
                }
                // The key is always rendered — it feeds the detector — but its
                // output is withheld from the mix while it is keying.
                let sample = self.clips[clip_index].sample_at(
                    frame,
                    channel,
                    self.output_sample_rate,
                    &self.tempo_map,
                );
                if keying && Some(active_position) == key_position {
                    // Après l'enveloppe de la piste, pas avant : le détecteur
                    // doit entendre la clé au niveau où elle est réglée, sans
                    // quoi son fader ne changerait rien au pompage.
                    key_frame[channel] = sample * self.volume_automation[lane].gain_at_frame(frame);
                    continue;
                }
                lane_mix[lane] += sample;
            }

            let mut mixed = 0.0_f32;
            for (lane, sample) in lane_mix.into_iter().enumerate() {
                // Le panoramique agit par voie, après son volume et avant la
                // sommation : c'est la place d'un panoramique de tranche.
                let (left, right) =
                    equal_power_pan(self.pan_automation[lane].value_at_frame(frame));
                let side = if channel == 0 { left } else { right };
                let dry = self.filter_lane_sample(lane, channel, sample)
                    * self.volume_automation[lane].gain_at_frame(frame)
                    * side;
                // Le bitcrush est un **insert** : il remplace le signal au lieu
                // de s'y ajouter. Sommé au sec comme la reverb, on entendrait le
                // son propre avec du grain par-dessus, alors que ce qu'on
                // demande à cet effet est justement de remplacer le son propre.
                //
                // Il agit avant les deux départs, si bien qu'on entend la pièce
                // et le peigne **du son broyé** — l'ordre d'une chaîne réelle.
                let contribution = self.bitcrushers[lane].process(
                    dry,
                    channel,
                    crush_latch[lane],
                    self.bitcrush_sends[lane],
                );
                mixed += contribution;
                // La prise de l'envoi : après tout ce que la piste subit, donc
                // ce qu'on entend d'elle est bien ce qui part dans la pièce.
                reverb_send[channel] += contribution * self.reverb_sends[lane];
                flanger_send[channel] += contribution * self.flanger_sends[lane];
                delay_send[channel] += contribution * self.delay_sends[lane];
            }
            *master_sample = if mixed.is_finite() { mixed } else { 0.0 };
        }

        // Duck, then compressor, then limiter: the console order, the sidechain
        // belonging to the mix and the master processors to what leaves it.
        // Worth knowing while mixing: a compressor lifts what the duck just
        // pushed down, so switching COMP on softens the pump a little.
        if keying {
            if frame.is_multiple_of(DUCK_TEMPO_REFRESH_FRAMES) {
                let seconds = frame as f64 / f64::from(self.output_sample_rate);
                let beat = self.tempo_map.beat_at_seconds(seconds);
                self.ducker
                    .set_tempo(self.tempo_map.bpm_at_beat(beat), self.output_sample_rate);
            }
            // La profondeur suit ce que la clé pèse réellement dans le mix :
            // son niveau réglé, ce qui permet d'écrire une progression de
            // pompage en montant son enveloppe, et sa place dans le champ
            // stéréo, un kick sur un côté ne poussant pas la somme comme un
            // kick au milieu.
            let key_gain = key_position
                .map(|position| {
                    let lane = self.clips[self.active_indices[position]].lane;
                    self.volume_automation[lane].gain_at_frame(frame)
                        * sidechain_pan_weight(self.pan_automation[lane].value_at_frame(frame))
                })
                .unwrap_or(1.0);
            let duck = self.ducker.process(
                (key_frame[0] + key_frame[1]) * 0.5,
                self.output_sample_rate,
                key_gain,
            );
            for sample in &mut master {
                *sample *= duck;
            }
        } else {
            self.ducker.reset();
        }

        // Master chain: compressor, colour, meters, limiter, then the bound.
        // The compressor and the colour shape the mix and belong to its sound,
        // so the VU shows their result; the limiter is protection, so the VU
        // stays ahead of it and `OL` is measured behind it.
        let compressor_enabled = self.compressor_enabled.load(Ordering::Relaxed);
        let compressor_gain = self.compressor.process(
            (master[0] + master[1]) * 0.5,
            self.output_sample_rate,
            compressor_enabled,
        );
        self.colour_amount = follow(
            self.colour_amount,
            if compressor_enabled { 1.0 } else { 0.0 },
            COLOUR_BLEND_SECONDS,
            self.output_sample_rate,
        );
        for (channel, sample) in master.iter_mut().enumerate() {
            *sample = self.colour.process(
                *sample * compressor_gain,
                channel,
                self.output_sample_rate,
                self.colour_amount,
            );
        }

        // Les trois retours, après le compresseur et la teinte, avant le VU et
        // le limiteur. Un effet que plus personne n'alimente finit de sonner
        // puis s'arrête d'être calculé : sans ces décomptes, leurs lignes
        // tourneraient sur du silence pendant tout le mix.
        //
        // L'Arc est cloné avant la prise du verrou : `self` doit rester libre,
        // et un incrément atomique par image ne se mesure pas.
        let tails = Arc::clone(&self.tails);
        let mut tails = tails
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let feeding = reverb_send[0] != 0.0 || reverb_send[1] != 0.0;
        if feeding {
            tails.reverb_frames = tails.reverb_budget;
        }
        if tails.reverb_frames > 0 {
            let (wet_left, wet_right) = tails.reverb.process(reverb_send[0], reverb_send[1]);
            master[0] += wet_left;
            master[1] += wet_right;
            if !feeding {
                tails.reverb_frames -= 1;
            }
        }

        // La queue du flanger est bien plus courte que celle de la pièce : sa
        // ligne fait quelques millisecondes, et son rebouclage l'éteint en une
        // fraction de seconde.
        let flanging = flanger_send[0] != 0.0 || flanger_send[1] != 0.0;
        if flanging {
            tails.flanger_frames = tails.flanger_budget;
        }
        if tails.flanger_frames > 0 {
            let (wet_left, wet_right) = tails.flanger.process(flanger_send[0], flanger_send[1]);
            master[0] += wet_left;
            master[1] += wet_right;
            if !flanging {
                tails.flanger_frames -= 1;
            }
        }

        // L'écho, au même endroit de la chaîne. Sa traîne est la plus longue des
        // trois : chaque tour ne perd qu'un peu plus du quart, et à tempo lent
        // un tour dure plus d'une seconde.
        let echoing = delay_send[0] != 0.0 || delay_send[1] != 0.0;
        if echoing {
            tails.delay_frames = tails.delay_budget;
        }
        if tails.delay_frames > 0 {
            let (wet_left, wet_right) =
                tails
                    .delay
                    .process(delay_send[0], delay_send[1], delay_samples);
            master[0] += wet_left;
            master[1] += wet_right;
            if !echoing {
                tails.delay_frames -= 1;
            }
        }
        drop(tails);

        self.update_meter(frame, 0, master[0]);
        self.update_meter(frame, 1, master[1]);

        let frame_peak = master[0].abs().max(master[1].abs());
        let gain = self.limiter.process(
            frame_peak,
            self.output_sample_rate,
            self.limiter_enabled.load(Ordering::Relaxed),
        );

        // `OL` is measured here, after the limiter and before the hard bound:
        // it lights only when the bound truly had to shave the signal.
        let mut clipped = false;
        for (output, sample) in self.pending_frame.iter_mut().zip(master) {
            let limited = sample * gain;
            if !limited.is_finite() {
                clipped = true;
                *output = 0.0;
                continue;
            }
            clipped |= limited.abs() > OVERLOAD_THRESHOLD;
            *output = limited.clamp(-OUTPUT_CEILING, OUTPUT_CEILING);
        }
        self.update_overload(clipped);
    }
}

impl Iterator for TimelineMixSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let total_samples = self.total_frames.saturating_mul(OUTPUT_CHANNELS as usize);
        if self.position_sample >= total_samples {
            self.reset_meter();
            return None;
        }
        let frame = self.position_sample / OUTPUT_CHANNELS as usize;
        let channel = self.position_sample % OUTPUT_CHANNELS as usize;
        if channel == 0 {
            self.refresh_active_clips(frame);
            self.update_filter_values(frame);
            self.render_frame(frame);
        }

        let output = self.pending_frame[channel];
        self.position_sample += 1;
        Some(output)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let total_samples = self.total_frames.saturating_mul(OUTPUT_CHANNELS as usize);
        let remaining = total_samples.saturating_sub(self.position_sample);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for TimelineMixSource {}

impl Source for TimelineMixSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.position_sample >= self.total_frames.saturating_mul(OUTPUT_CHANNELS as usize) {
            Some(0)
        } else {
            Some(self.total_frames.saturating_mul(OUTPUT_CHANNELS as usize))
        }
    }

    fn channels(&self) -> NonZero<u16> {
        NonZero::new(OUTPUT_CHANNELS).expect("stereo has two channels")
    }

    fn sample_rate(&self) -> NonZero<u32> {
        NonZero::new(self.output_sample_rate).expect("sample rate is non-zero")
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(self.duration())
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        let frame = duration_to_frame(position, self.output_sample_rate).min(self.total_frames);
        self.position_sample = frame.saturating_mul(OUTPUT_CHANNELS as usize);
        self.rebuild_active_clips(frame);
        // Les queues ne sont **pas** vidées ici. Ce `try_seek` sert aussi bien
        // à un déplacement voulu qu'au repositionnement qui suit une édition —
        // et dans ce second cas, vider tuerait la queue de la passe qu'on vient
        // tout juste d'écrire. C'est au moteur, qui sait pourquoi il déplace,
        // d'appeler `EffectTails::reset`.
        for crusher in &mut self.bitcrushers {
            crusher.reset();
        }
        Ok(())
    }
}

#[derive(Clone)]
struct CachedTimeline {
    signature: u64,
    tempo_signature: u64,
    end_beat: f64,
    duration: Duration,
    source: TimelineMixSource,
}

#[derive(Default)]
pub struct TimelinePlaybackEngine {
    output: Option<MixerDeviceSink>,
    player: Option<Player>,
    cached: Option<CachedTimeline>,
    /// Les queues des effets, **conservées d'une source à l'autre**.
    ///
    /// Elles vivent ici et non dans la source parce qu'une source ne dure que
    /// jusqu'à la prochaine édition : écrire une passe reconstruit le plan, et
    /// une pièce reconstruite est une pièce vide. La queue mourait donc à
    /// l'instant où l'on relâchait le bouton — l'instant précis où elle devait
    /// commencer.
    ///
    /// La fréquence de sortie est retenue avec elles : les longueurs de ligne
    /// en dépendent, donc changer de périphérique demande de tout refaire.
    tails: Option<(u32, Arc<Mutex<EffectTails>>)>,
}

impl TimelinePlaybackEngine {
    pub fn prepare_and_play(
        &mut self,
        plan: &TimelineRenderPlan,
        position_beat: f64,
    ) -> Result<Duration, String> {
        let signature = playback_signature(plan);
        if let Some(cache) = &self.cached {
            cache.source.set_audible_lane_mask(plan.audible_lane_mask);
            cache.source.set_limiter_enabled(plan.limiter_enabled);
            cache.source.set_compressor_enabled(plan.compressor_enabled);
        }
        if self.cached.as_ref().map(|cache| cache.signature) != Some(signature) {
            self.replace_cached_timeline(plan, true)?;
        } else {
            self.ensure_output()?;
            if self.player_ref()?.empty() {
                self.queue_cached()?;
            }
        }

        let cache = self.cached_ref()?;
        let target = Duration::from_secs_f64(plan.tempo_map.seconds_at_beat(position_beat))
            .min(cache.duration);
        self.player_ref()?.pause();
        self.player_ref()?
            .try_seek(target)
            .map_err(|error| format!("Could not position the mix: {error}"))?;
        self.player_ref()?.play();
        Ok(target)
    }

    pub fn refresh_while_playing(
        &mut self,
        plan: &TimelineRenderPlan,
        previous_tempo_map: &TempoMap,
        previous_end_beat: f64,
    ) -> Result<Option<Duration>, String> {
        let Some((position, true)) = self.transport_position(previous_tempo_map, previous_end_beat)
        else {
            return Ok(None);
        };
        let position_beat = previous_tempo_map.beat_at_seconds(position.as_secs_f64());

        self.replace_cached_timeline(plan, false)?;
        let target = Duration::from_secs_f64(plan.tempo_map.seconds_at_beat(position_beat))
            .min(self.cached_ref()?.duration);
        self.player_ref()?
            .try_seek(target)
            .map_err(|error| format!("Could not position the mix: {error}"))?;
        self.player_ref()?.play();
        Ok(Some(target))
    }

    pub fn pause(&self) -> Option<Duration> {
        let player = self.player.as_ref()?;
        let position = self.player_position(player);
        player.pause();
        if let Some(cache) = &self.cached {
            cache.source.reset_meter();
        }
        Some(position)
    }

    pub fn pause_if_playing(&self) -> Option<Duration> {
        let player = self.player.as_ref()?;
        if player.empty() || player.is_paused() {
            if let Some(cache) = &self.cached {
                cache.source.reset_meter();
            }
            return Some(self.player_position(player));
        }
        let position = self.player_position(player);
        player.pause();
        if let Some(cache) = &self.cached {
            cache.source.reset_meter();
        }
        Some(position)
    }

    pub fn release_output(&mut self) {
        if let Some(player) = &self.player {
            player.stop();
        }
        if let Some(cache) = &self.cached {
            cache.source.reset_meter();
        }
        self.player = None;
        self.output = None;
    }

    pub fn meter_levels(&self) -> (f32, f32, bool) {
        self.cached
            .as_ref()
            .map_or((0.0, 0.0, false), |cache| cache.source.meter_levels())
    }

    pub fn set_audible_lane_mask(&self, audible_lane_mask: u8) {
        if let Some(cache) = &self.cached {
            cache.source.set_audible_lane_mask(audible_lane_mask);
        }
    }

    pub fn set_limiter_enabled(&self, limiter_enabled: bool) {
        if let Some(cache) = &self.cached {
            cache.source.set_limiter_enabled(limiter_enabled);
        }
    }

    pub fn set_compressor_enabled(&self, compressor_enabled: bool) {
        if let Some(cache) = &self.cached {
            cache.source.set_compressor_enabled(compressor_enabled);
        }
    }

    /// Le masque des boutons de reverb enfoncés, un bit par piste.
    ///
    /// Partagé atomiquement comme Mute et Solo : un appui s'entend sans que le
    /// plan soit reconstruit, ce qui est indispensable pour un geste joué.
    pub fn set_reverb_keys(&self, keys: u8) {
        if let Some(cache) = &self.cached {
            cache.source.set_reverb_keys(keys);
        }
    }

    /// Les mêmes bits, pour le flanger.
    pub fn set_flanger_keys(&self, keys: u8) {
        if let Some(cache) = &self.cached {
            cache.source.set_flanger_keys(keys);
        }
    }

    /// Et pour le bitcrush.
    pub fn set_bitcrush_keys(&self, keys: u8) {
        if let Some(cache) = &self.cached {
            cache.source.set_bitcrush_keys(keys);
        }
    }

    /// Et pour le delay.
    pub fn set_delay_keys(&self, keys: u8) {
        if let Some(cache) = &self.cached {
            cache.source.set_delay_keys(keys);
        }
    }

    /// Le masque des pistes sous la gomme, un bit par piste.
    ///
    /// Il fait taire la passe **enregistrée** de ces pistes le temps du geste.
    /// Sans lui, la gomme effaçait bel et bien en base, mais on continuait
    /// d'entendre la reverb pendant qu'on l'effaçait : le seul moment où l'on
    /// veut savoir ce qu'on retire est justement celui où l'on appuie.
    pub fn set_effect_erase(&self, lanes: u8) {
        if let Some(cache) = &self.cached {
            cache.source.set_effect_erase(lanes);
        }
    }

    pub fn seek_if_current(
        &mut self,
        position_beat: f64,
        tempo_map: &TempoMap,
        end_beat: f64,
    ) -> Result<Option<Duration>, String> {
        if !self.matches_timing(tempo_map, end_beat) {
            return Ok(None);
        }
        if self.player.is_none() {
            return Ok(None);
        }
        if self.player.as_ref().is_some_and(Player::empty) {
            self.queue_cached()?;
        }
        let player = self
            .player
            .as_ref()
            .ok_or_else(|| "The timeline audio output is not running.".to_owned())?;
        let target = Duration::from_secs_f64(tempo_map.seconds_at_beat(position_beat))
            .min(self.cached_ref()?.duration);
        player
            .try_seek(target)
            .map_err(|error| format!("Could not position the mix: {error}"))?;
        // Ici, et seulement ici, les queues sont vidées : c'est le seul
        // déplacement que l'utilisateur a **voulu**. La queue de l'endroit
        // qu'on quitte n'a rien à faire à l'endroit où l'on arrive, et on
        // l'entendrait très bien. Le repositionnement qui suit une édition, lui,
        // ne déplace personne et laisse les queues sonner.
        self.reset_effect_tails();
        Ok(Some(target))
    }

    /// Vide les queues des effets. Réservé aux déplacements voulus.
    fn reset_effect_tails(&self) {
        if let Some((_, tails)) = &self.tails {
            tails
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .reset();
        }
    }

    pub fn transport_position(
        &self,
        tempo_map: &TempoMap,
        end_beat: f64,
    ) -> Option<(Duration, bool)> {
        if !self.matches_timing(tempo_map, end_beat) {
            return None;
        }
        let player = self.player.as_ref()?;
        let playing = !player.empty() && !player.is_paused();
        Some((self.player_position(player), playing))
    }

    fn matches_timing(&self, tempo_map: &TempoMap, end_beat: f64) -> bool {
        self.cached.as_ref().is_some_and(|cache| {
            cache.tempo_signature == tempo_map.signature()
                && (cache.end_beat - end_beat).abs() < 1e-6
        })
    }

    fn player_position(&self, player: &Player) -> Duration {
        if player.empty() {
            self.cached
                .as_ref()
                .map_or(Duration::ZERO, |cache| cache.duration)
        } else {
            player.get_pos()
        }
    }

    fn replace_cached_timeline(
        &mut self,
        plan: &TimelineRenderPlan,
        verify_files: bool,
    ) -> Result<(), String> {
        self.ensure_output()?;
        let output_sample_rate = self
            .output
            .as_ref()
            .map(|output| output.config().sample_rate().get())
            .unwrap_or(FALLBACK_OUTPUT_SAMPLE_RATE);
        let mut source = prepare_timeline(plan, verify_files, output_sample_rate)?;
        // Les queues survivent à la reconstruction. Elles ne sont refaites que
        // si la fréquence de sortie a changé, auquel cas les longueurs de ligne
        // ne voudraient plus rien dire.
        let tails = match self.tails.take() {
            Some((rate, tails)) if rate == output_sample_rate => tails,
            _ => Arc::new(Mutex::new(EffectTails::new(output_sample_rate))),
        };
        source.share_tails(Arc::clone(&tails));
        self.tails = Some((output_sample_rate, tails));
        self.cached = Some(CachedTimeline {
            signature: playback_signature(plan),
            tempo_signature: plan.tempo_map.signature(),
            end_beat: plan.end_beat,
            duration: source.duration(),
            source,
        });
        self.queue_cached()
    }

    fn queue_cached(&mut self) -> Result<(), String> {
        self.ensure_output()?;
        let source = self.cached_ref()?.source.clone();
        let player = self.player_ref()?;
        player.stop();
        player.append(source);
        player.pause();
        Ok(())
    }

    fn ensure_output(&mut self) -> Result<(), String> {
        if self.output.is_some() && self.player.is_some() {
            return Ok(());
        }
        let output = DeviceSinkBuilder::open_default_sink()
            .map_err(|error| format!("Could not open the default audio output: {error}"))?;
        let player = Player::connect_new(output.mixer());
        self.output = Some(output);
        self.player = Some(player);
        Ok(())
    }

    fn player_ref(&self) -> Result<&Player, String> {
        self.player
            .as_ref()
            .ok_or_else(|| "The timeline audio output is not running.".to_owned())
    }

    fn cached_ref(&self) -> Result<&CachedTimeline, String> {
        self.cached
            .as_ref()
            .ok_or_else(|| "The timeline has not been prepared yet.".to_owned())
    }
}

pub(crate) fn prepare_timeline(
    plan: &TimelineRenderPlan,
    verify_files: bool,
    output_sample_rate: u32,
) -> Result<TimelineMixSource, String> {
    validate_plan(plan)?;
    let mut verified_paths = HashSet::new();
    let mut clips = Vec::with_capacity(plan.clips.len());
    let mut total_frames = seconds_to_frames(
        plan.tempo_map.seconds_at_beat(plan.end_beat),
        output_sample_rate,
    )?;

    for clip in &plan.clips {
        let clip_end_beat = clip.visual_start_beat + clip.duration_beats;
        let (minimum_bpm, maximum_bpm) = plan
            .tempo_map
            .bpm_extrema_between(clip.visual_start_beat, clip_end_beat);
        validate_stretch_ratio(
            stretch_duration_ratio(clip.source_bpm, minimum_bpm),
            &clip.file_path,
        )?;
        validate_stretch_ratio(
            stretch_duration_ratio(clip.source_bpm, maximum_bpm),
            &clip.file_path,
        )?;
        if verify_files && verified_paths.insert(clip.file_path.clone()) {
            open_mp3_decoder(Path::new(&clip.file_path))?;
        }
        let start_seconds = plan.tempo_map.seconds_at_beat(clip.visual_start_beat);
        let start_frame = seconds_to_frames(start_seconds, output_sample_rate)?;
        let end_seconds = plan.tempo_map.seconds_at_beat(clip_end_beat);
        let end_frame = seconds_to_frames(end_seconds, output_sample_rate)?;
        let output_frames = end_frame.saturating_sub(start_frame).max(1);
        total_frames = total_frames.max(start_frame.saturating_add(output_frames));
        let lane = usize::try_from(clip.lane)
            .map_err(|_| "This clip's audio track is not valid.".to_owned())?;
        if lane >= 3 {
            return Err("This clip's audio track is not valid.".to_owned());
        }
        clips.push(PlacedClip {
            file_path: clip.file_path.clone(),
            lane,
            start_frame,
            output_frames,
            visual_start_beat: clip.visual_start_beat,
            source_bpm: clip.source_bpm,
            trim_start_beats: clip.trim_start_beats,
            trim_end_beats: clip.trim_end_beats,
            is_sidechain_key: clip.is_sidechain_key,
            eq_settings: clip.eq_settings.clone(),
            eq_state: ClipEqState::default(),
            grain_cache: None,
            reader: None,
            failed: false,
        });
    }

    let mut volume_automation: [VolumeAutomation; 3] =
        std::array::from_fn(|_| VolumeAutomation::default());
    for node in &plan.volume_nodes {
        let lane = usize::try_from(node.lane)
            .map_err(|_| "The Volume Node lane is invalid.".to_owned())?;
        if lane >= volume_automation.len() {
            return Err("The Volume Node lane is invalid.".to_owned());
        }
        volume_automation[lane].points.push(VolumeFramePoint {
            frame: seconds_to_frames(
                plan.tempo_map.seconds_at_beat(node.beat),
                output_sample_rate,
            )?,
            gain_db: node.gain_db,
        });
    }

    let mut pan_automation: [PanAutomation; 3] = std::array::from_fn(|_| PanAutomation::default());
    for node in &plan.pan_nodes {
        let lane =
            usize::try_from(node.lane).map_err(|_| "The Pan Node lane is invalid.".to_owned())?;
        if lane >= pan_automation.len() {
            return Err("The Pan Node lane is invalid.".to_owned());
        }
        pan_automation[lane].points.push(PanFramePoint {
            frame: seconds_to_frames(
                plan.tempo_map.seconds_at_beat(node.beat),
                output_sample_rate,
            )?,
            value: node.value as f32,
        });
    }

    let mut filter_automation: [FilterAutomation; 3] =
        std::array::from_fn(|_| FilterAutomation::default());
    for node in &plan.filter_nodes {
        let lane = usize::try_from(node.lane)
            .map_err(|_| "The Filter Node lane is invalid.".to_owned())?;
        if lane >= filter_automation.len() {
            return Err("The Filter Node lane is invalid.".to_owned());
        }
        filter_automation[lane].points.push(FilterFramePoint {
            frame: seconds_to_frames(
                plan.tempo_map.seconds_at_beat(node.beat),
                output_sample_rate,
            )?,
            value: node.value,
            tension: node.tension,
        });
    }

    // Les deux effets joués se convertissent de la même façon : mêmes nœuds,
    // même passage des beats aux images. Une boucle plutôt que deux blocs
    // jumeaux, pour qu'ils ne puissent pas diverger.
    let mut played_automation: [[SendAutomation; 3]; 4] =
        std::array::from_fn(|_| std::array::from_fn(|_| SendAutomation::default()));
    for (slot, nodes) in [
        &plan.reverb_nodes,
        &plan.flanger_nodes,
        &plan.bitcrush_nodes,
        &plan.delay_nodes,
    ]
    .into_iter()
    .enumerate()
    {
        for node in nodes {
            let lane = usize::try_from(node.lane)
                .map_err(|_| "The effect node lane is invalid.".to_owned())?;
            if lane >= played_automation[slot].len() {
                return Err("The effect node lane is invalid.".to_owned());
            }
            played_automation[slot][lane].points.push(SendFramePoint {
                frame: seconds_to_frames(
                    plan.tempo_map.seconds_at_beat(node.beat),
                    output_sample_rate,
                )?,
                value: node.value as f32,
            });
        }
    }
    let [
        reverb_automation,
        flanger_automation,
        bitcrush_automation,
        delay_automation,
    ] = played_automation;

    Ok(TimelineMixSource::new(
        clips,
        total_frames,
        plan.audible_lane_mask,
        MasterDynamics {
            compressor_enabled: plan.compressor_enabled,
            limiter_enabled: plan.limiter_enabled,
        },
        plan.tempo_map.clone(),
        output_sample_rate,
        LaneAutomation {
            volume: volume_automation,
            pan: pan_automation,
            filter: filter_automation,
            reverb: reverb_automation,
            flanger: flanger_automation,
            bitcrush: bitcrush_automation,
            delay: delay_automation,
        },
    ))
}

fn stretch_duration_ratio(source_bpm: f64, project_bpm: f64) -> f64 {
    source_bpm / project_bpm
}

/// Le suiveur du VU-mètre : **attaque instantanée**, chute lente.
///
/// L'attaque avait une constante de 65 ms, et c'était une erreur de nature et
/// pas seulement de réglage. Un filtre à un pôle appliqué à `|x|` ne converge
/// pas vers le maximum du signal mais vers sa **moyenne** : il ne mesurait
/// donc pas des crêtes, il les moyennait. Un morceau masterisé, dont les
/// crêtes dépassent la moyenne d'une douzaine de décibels, touchait le plein
/// niveau en n'affichant que les deux tiers de la barre — le mètre annonçait
/// un niveau confortable pendant que le limiteur travaillait.
///
/// Une crête est maintenant prise telle quelle, à l'échantillon. Ce que la
/// barre montre est le plus haut niveau atteint récemment, ce qui est la seule
/// chose qu'on lui demande avant l'écrêtage.
fn meter_release_coefficient(sample_rate: u32) -> f32 {
    (-1.0 / (sample_rate as f32 * VU_RELEASE_SECONDS)).exp()
}

fn vu_envelope(current: f32, input: f32, release_coefficient: f32) -> f32 {
    if input >= current {
        return input;
    }
    input + release_coefficient * (current - input)
}

fn validate_plan(plan: &TimelineRenderPlan) -> Result<(), String> {
    if !plan.project_bpm.is_finite() || !(40.0..=300.0).contains(&plan.project_bpm) {
        return Err("The project BPM is not valid.".to_owned());
    }
    if !plan.end_beat.is_finite() || plan.end_beat <= 0.0 || plan.clips.is_empty() {
        return Err("Add at least one clip before starting the timeline.".to_owned());
    }
    let seconds = plan.tempo_map.seconds_at_beat(plan.end_beat);
    if seconds > MAX_PROJECT_SECONDS {
        return Err("The project is longer than the current four-hour safety limit.".to_owned());
    }
    Ok(())
}

fn validate_stretch_ratio(ratio: f64, file_path: &str) -> Result<(), String> {
    if ratio.is_finite() && (MIN_STRETCH_RATIO..=MAX_STRETCH_RATIO).contains(&ratio) {
        return Ok(());
    }
    let name = Path::new(file_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_path.to_owned());
    Err(format!(
        "{name} would need a time-stretch outside the supported 0.5x to 2x range."
    ))
}

fn playback_signature(plan: &TimelineRenderPlan) -> u64 {
    let mut hasher = DefaultHasher::new();
    plan.project_bpm.to_bits().hash(&mut hasher);
    plan.tempo_map.signature().hash(&mut hasher);
    plan.end_beat.to_bits().hash(&mut hasher);
    for clip in &plan.clips {
        clip.id.hash(&mut hasher);
        clip.lane.hash(&mut hasher);
        clip.file_path.hash(&mut hasher);
        clip.source_bpm.to_bits().hash(&mut hasher);
        clip.first_beat_ms.hash(&mut hasher);
        clip.anchor_beat.to_bits().hash(&mut hasher);
        clip.visual_start_beat.to_bits().hash(&mut hasher);
        clip.duration_beats.to_bits().hash(&mut hasher);
        clip.trim_start_beats.to_bits().hash(&mut hasher);
        clip.trim_end_beats.to_bits().hash(&mut hasher);
        // The key decides which clip falls silent, so it belongs to the mix the
        // engine renders. Leaving it out let a cached mix built without a key
        // survive being given one: nothing was muted and nothing pumped.
        clip.is_sidechain_key.hash(&mut hasher);
        if let Some(eq) = &clip.eq_settings {
            eq.high_pass_hz.to_bits().hash(&mut hasher);
            eq.low_pass_hz.to_bits().hash(&mut hasher);
            eq.peak_hz.map(f64::to_bits).hash(&mut hasher);
            eq.peak_gain_db.map(f64::to_bits).hash(&mut hasher);
            eq.peak_q.map(f64::to_bits).hash(&mut hasher);
            eq.gain_db.map(f64::to_bits).hash(&mut hasher);
            eq.enabled.hash(&mut hasher);
        }
        if let Ok(metadata) = std::fs::metadata(&clip.file_path) {
            metadata.len().hash(&mut hasher);
            if let Ok(modified) = metadata.modified()
                && let Ok(elapsed) = modified.duration_since(UNIX_EPOCH)
            {
                elapsed.as_nanos().hash(&mut hasher);
            }
        }
    }
    for node in &plan.volume_nodes {
        node.id.hash(&mut hasher);
        node.lane.hash(&mut hasher);
        node.beat.to_bits().hash(&mut hasher);
        node.gain_db.map(f64::to_bits).hash(&mut hasher);
    }
    // Le panoramique change ce qui sort du moteur, donc il appartient à la
    // signature. L'omettre laisserait un mix en cache survivre à l'édition
    // d'une courbe — le défaut qui avait rendu la clé de sidechain inopérante.
    for node in &plan.pan_nodes {
        node.id.hash(&mut hasher);
        node.lane.hash(&mut hasher);
        node.beat.to_bits().hash(&mut hasher);
        node.value.to_bits().hash(&mut hasher);
    }
    for node in &plan.filter_nodes {
        node.id.hash(&mut hasher);
        node.lane.hash(&mut hasher);
        node.beat.to_bits().hash(&mut hasher);
        node.value.to_bits().hash(&mut hasher);
        node.tension.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

fn seconds_to_frames(seconds: f64, sample_rate: u32) -> Result<usize, String> {
    if !seconds.is_finite() || !(0.0..=MAX_PROJECT_SECONDS).contains(&seconds) {
        return Err(
            "The computed timeline length is invalid, or longer than four hours.".to_owned(),
        );
    }
    Ok((seconds * f64::from(sample_rate)).ceil() as usize)
}

fn duration_to_frame(duration: Duration, sample_rate: u32) -> usize {
    (duration.as_secs_f64() * f64::from(sample_rate)).round() as usize
}

#[cfg(test)]
mod tests {
    use super::{
        BiquadKind, BiquadState, COLOUR_AIR_DB, COLOUR_LOW_SHELF_DB, COMPRESSOR_MAKEUP_GAIN,
        CachedTimeline, ClipEqState, DELAY_TAIL_SECONDS, DUCK_DEPTH_DB, DUCK_FLOOR, EffectTails,
        FALLBACK_OUTPUT_SAMPLE_RATE, FILTER_HIGH_PASS_CLOSED_HZ, FILTER_HIGH_PASS_OPEN_HZ,
        FILTER_LOW_PASS_CLOSED_HZ, FILTER_Q, FLANGER_TAIL_SECONDS, FilterAutomation,
        FilterFramePoint, LaneAutomation, METER_PUBLISH_FRAMES, MasterColour, MasterCompressor,
        MasterDynamics, MasterLimiter, OUTPUT_CEILING, OVERLOAD_THRESHOLD, PanAutomation,
        PanFramePoint, PlacedClip, REVERB_TAIL_SECONDS, SendAutomation, SidechainDucker,
        TimelineMixSource, TimelinePlaybackEngine, VolumeAutomation, VolumeFramePoint,
        WSOLA_MAX_SEARCH_FRAMES, best_wsola_offset, cubic_interpolate, equal_power_pan,
        filter_cutoff_hz, filter_makeup_gain, meter_release_coefficient, playback_signature,
        prepare_timeline, sidechain_pan_weight, smooth_crossfade, stretch_duration_ratio,
        vu_envelope,
    };
    use crate::{
        tempo::{TempoMap, TempoPoint},
        timeline::{TimelineRenderClip, TimelineRenderPlan, TimelineVolumeNode},
    };
    use rodio::Source;
    use std::sync::{Arc, Mutex};
    use std::{f64::consts::TAU, sync::atomic::Ordering, time::Duration};

    fn stereo_sine(frequency: f64, seconds: f64, sample_rate: u32) -> Vec<f32> {
        let frames = (seconds * f64::from(sample_rate)) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for frame in 0..frames {
            let value = (TAU * frequency * frame as f64 / f64::from(sample_rate)).sin() as f32;
            samples.push(value);
            samples.push(value * 0.5);
        }
        samples
    }

    fn interpolated_sample(input: &[f32], position: f64, channel: usize) -> f32 {
        let first = position.floor() as usize;
        let fraction = (position - first as f64) as f32;
        let get = |frame: usize| input.get(frame * 2 + channel).copied().unwrap_or(0.0);
        cubic_interpolate(
            get(first.saturating_sub(1)),
            get(first),
            get(first.saturating_add(1)),
            get(first.saturating_add(2)),
            fraction,
        )
    }

    fn placed_clip(start_frame: usize, output_frames: usize) -> PlacedClip {
        PlacedClip {
            file_path: "test.mp3".to_owned(),
            lane: 0,
            start_frame,
            output_frames,
            visual_start_beat: 0.0,
            source_bpm: 120.0,
            trim_start_beats: 0.0,
            trim_end_beats: 0.0,
            is_sidechain_key: false,
            eq_settings: None,
            eq_state: ClipEqState::default(),
            grain_cache: None,
            reader: None,
            failed: false,
        }
    }

    fn constant_tempo() -> TempoMap {
        TempoMap::new(120.0, Vec::new()).expect("valid constant tempo")
    }

    fn unity_automation() -> [VolumeAutomation; 3] {
        std::array::from_fn(|_| VolumeAutomation::default())
    }

    #[test]
    fn the_pan_law_holds_its_power_across_the_sweep() {
        // Une loi linéaire ferait paraître le centre plus fort que les
        // extrêmes : c'est la somme des carrés qui doit rester constante.
        for value in [-1.0, -0.5, -0.25, 0.0, 0.25, 0.5, 1.0_f32] {
            let (left, right) = equal_power_pan(value);
            let power = left * left + right * right;
            assert!(
                (power - 1.0).abs() < 1.0e-5,
                "à {value} la puissance vaut {power}"
            );
        }

        let (left, right) = equal_power_pan(0.0);
        assert!(
            (left - right).abs() < 1.0e-6,
            "le centre doit être symétrique"
        );
        assert!(
            (left - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-6,
            "soit −3 dB de chaque côté"
        );

        let (hard_left_l, hard_left_r) = equal_power_pan(-1.0);
        assert!(
            hard_left_l > 0.999 && hard_left_r < 0.001,
            "−1 est à gauche"
        );
        let (hard_right_l, hard_right_r) = equal_power_pan(1.0);
        assert!(
            hard_right_r > 0.999 && hard_right_l < 0.001,
            "+1 est à droite"
        );
    }

    #[test]
    fn a_lane_without_pan_nodes_stays_centred() {
        let automation = PanAutomation::default();
        assert_eq!(automation.value_at_frame(0), 0.0);
        assert_eq!(automation.value_at_frame(100_000), 0.0);
    }

    #[test]
    fn pan_interpolates_between_its_nodes() {
        let automation = PanAutomation {
            points: vec![
                PanFramePoint {
                    frame: 0,
                    value: -1.0,
                },
                PanFramePoint {
                    frame: 100,
                    value: 1.0,
                },
            ],
        };
        assert!((automation.value_at_frame(50) - 0.0).abs() < 1.0e-6);
        assert!((automation.value_at_frame(25) + 0.5).abs() < 1.0e-6);
        // Au-delà du dernier nœud, la valeur tient plutôt que de revenir au
        // centre : un mouvement écrit doit rester où on l'a laissé.
        assert!((automation.value_at_frame(500) - 1.0).abs() < 1.0e-6);
    }

    /// Panoramique au centre : ce que produit une piste sans nœud.
    fn centred_pan_automation() -> [PanAutomation; 3] {
        std::array::from_fn(|_| PanAutomation::default())
    }

    fn bypass_filter_automation() -> [FilterAutomation; 3] {
        std::array::from_fn(|_| FilterAutomation::default())
    }

    /// Une voie sans nœud doit sonner comme une voie dont le nœud porte la
    /// valeur par défaut. Les deux chiffres ont vécu séparément — l'un dans la
    /// base, l'autre en dur ici — et changer le premier aurait laissé le second
    /// derrière.
    #[test]
    fn a_lane_without_nodes_uses_the_same_default_the_database_writes() {
        let default_db = crate::timeline::DEFAULT_TRACK_GAIN_DB;
        let silent = VolumeAutomation::default().gain_at_frame(0);
        assert!((silent - 10_f32.powf(default_db as f32 / 20.0)).abs() < 1.0e-6);
    }

    /// Response of a filter to a sine, as a fraction of the input amplitude.
    fn biquad_response(
        mut filter: BiquadState,
        frequency: f64,
        high_pass: bool,
        cutoff: f32,
    ) -> f32 {
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let mut peak = 0.0_f32;
        let frames = sample_rate as usize / 4;
        for frame in 0..frames {
            let input = (TAU * frequency * frame as f64 / f64::from(sample_rate)).sin() as f32;
            let output = filter.process(input, cutoff, sample_rate, high_pass);
            // Ignore the settling transient.
            if frame > frames / 2 {
                peak = peak.max(output.abs());
            }
        }
        peak
    }

    #[test]
    fn cached_coefficients_still_filter_the_expected_bands() {
        // A high pass at 8 kHz must cut a 100 Hz tone and pass a 12 kHz one.
        assert!(biquad_response(BiquadState::default(), 100.0, true, 8_000.0) < 0.05);
        assert!(biquad_response(BiquadState::default(), 12_000.0, true, 8_000.0) > 0.7);
        // A low pass at 200 Hz does the opposite.
        assert!(biquad_response(BiquadState::default(), 100.0, false, 200.0) > 0.7);
        assert!(biquad_response(BiquadState::default(), 8_000.0, false, 200.0) < 0.05);
    }

    #[test]
    fn coefficients_are_reused_until_the_shape_changes() {
        let mut filter = BiquadState::default();
        let first = filter.coefficients(BiquadKind::LowPass, 1_000.0, 0.0, FILTER_Q, 48_000);
        let design = filter.design;
        let repeated = filter.coefficients(BiquadKind::LowPass, 1_000.0, 0.0, FILTER_Q, 48_000);
        assert_eq!(
            filter.design, design,
            "an unchanged shape must not redesign"
        );
        assert_eq!(first.b0, repeated.b0);

        // A different cutoff, kind or sample rate has to invalidate the cache.
        let moved = filter.coefficients(BiquadKind::LowPass, 4_000.0, 0.0, FILTER_Q, 48_000);
        assert_ne!(first.b0, moved.b0);
        let switched = filter.coefficients(BiquadKind::HighPass, 4_000.0, 0.0, FILTER_Q, 48_000);
        assert_ne!(moved.b0, switched.b0);
        let resampled = filter.coefficients(BiquadKind::HighPass, 4_000.0, 0.0, FILTER_Q, 44_100);
        assert_ne!(switched.b0, resampled.b0);

        // The cutoff is clamped before it becomes the cache key, so values that
        // land on the same rail share one design.
        let below = filter.coefficients(BiquadKind::LowPass, 1.0, 0.0, FILTER_Q, 48_000);
        let also_below = filter.coefficients(BiquadKind::LowPass, 5.0, 0.0, FILTER_Q, 48_000);
        assert_eq!(below.b0, also_below.b0);
    }

    /// Feeds a steady sine at `amplitude` through the compressor and returns
    /// the settled gain it applies.
    fn settled_compressor_gain(amplitude: f32, frequency: f64, enabled: bool) -> f32 {
        let mut compressor = MasterCompressor::default();
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let mut gain = 1.0;
        for frame in 0..sample_rate as usize {
            let sample =
                amplitude * (TAU * frequency * frame as f64 / f64::from(sample_rate)).sin() as f32;
            gain = compressor.process(sample, sample_rate, enabled);
        }
        gain
    }

    #[test]
    fn the_compressor_leaves_quiet_material_alone_and_holds_loud_material_back() {
        // Under the knee nothing happens beyond the makeup gain.
        let quiet = settled_compressor_gain(0.05, 1_000.0, true);
        assert!(
            (quiet - COMPRESSOR_MAKEUP_GAIN).abs() < 0.02,
            "a quiet signal should only receive the makeup, got {quiet}"
        );

        // Well over the threshold it pulls back, but never past 2:1.
        let loud = settled_compressor_gain(0.7, 1_000.0, true);
        assert!(loud < quiet, "a loud signal should be held back");
        assert!(
            0.7 * loud < 0.7 * quiet,
            "the compressed peak must sit below the uncompressed one"
        );
        // 2:1 halves every dB over the threshold, so the output rises with the
        // square root of the input rather than in step with it.
        let louder = settled_compressor_gain(1.4, 1_000.0, true);
        assert!(
            1.4 * louder < 2.0 * (0.7 * loud),
            "doubling the input must not double the output"
        );
    }

    #[test]
    fn switched_off_the_compressor_settles_at_unity() {
        let gain = settled_compressor_gain(0.9, 1_000.0, false);
        assert!(
            (gain - 1.0).abs() < 1.0e-3,
            "a bypassed compressor should apply no gain, got {gain}"
        );
    }

    #[test]
    fn the_detector_listens_past_the_low_end_so_the_kick_does_not_duck_the_mix() {
        // The same amplitude, once as a kick-range tone and once in the mids.
        let low = settled_compressor_gain(0.6, 50.0, true);
        let mid = settled_compressor_gain(0.6, 1_000.0, true);
        assert!(
            low > mid,
            "a 50 Hz tone must drive far less gain reduction than a 1 kHz one: {low} vs {mid}"
        );
        assert!(
            (low - COMPRESSOR_MAKEUP_GAIN).abs() < 0.05,
            "the low tone should barely move the compressor at all, got {low}"
        );
    }

    #[test]
    fn the_colour_stage_lifts_the_ends_and_leaves_the_middle_and_the_top_octave_alone() {
        // Measured well below the saturation knee, so this reads the shelves
        // alone rather than the curve that follows them.
        const AMPLITUDE: f32 = 0.05;
        let response = |frequency: f64| {
            let mut colour = MasterColour::default();
            let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
            let frames = sample_rate as usize / 2;
            let mut peak = 0.0_f32;
            for frame in 0..frames {
                let input = (TAU * frequency * frame as f64 / f64::from(sample_rate)).sin() as f32
                    * AMPLITUDE;
                let output = colour.process(input, 0, sample_rate, 1.0);
                if frame > frames / 2 {
                    peak = peak.max(output.abs());
                }
            }
            peak / AMPLITUDE
        };

        let low = response(40.0);
        let middle = response(1_000.0);
        let air = response(13_000.0);
        // Ce que la cloche remplace : un plateau tenait ce niveau jusqu'à
        // Nyquist. Elle doit être retombée ici, sur une bande que rien ne
        // reproduit et que le MP3 paierait en bits.
        let ultrasound = response(19_000.0);

        assert!(low > 1.05, "the low shelf should add weight, got {low}");
        assert!(air > 1.05, "the air bell should lift its centre, got {air}");
        assert!(
            (middle - 1.0).abs() < 0.05,
            "the midrange must stay untouched, got {middle}"
        );
        assert!(
            ultrasound < 1.02,
            "the top octave must come back to nothing, got {ultrasound}"
        );
        // Small on purpose: this is presence, not an EQ move. The two ends are
        // dosed separately, so each is judged against its own ceiling.
        assert!(low < 10_f32.powf((COLOUR_LOW_SHELF_DB + 0.5) / 20.0));
        assert!(air < 10_f32.powf((COLOUR_AIR_DB + 0.5) / 20.0));
    }

    /// Amplitude of one frequency component of a signal, by quadrature
    /// correlation. Enough to weigh a single harmonic without a full transform.
    fn component_amplitude(signal: &[f32], frequency: f64, sample_rate: u32) -> f32 {
        let mut real = 0.0_f64;
        let mut imaginary = 0.0_f64;
        for (frame, sample) in signal.iter().enumerate() {
            let phase = TAU * frequency * frame as f64 / f64::from(sample_rate);
            real += f64::from(*sample) * phase.cos();
            imaginary += f64::from(*sample) * phase.sin();
        }
        let length = signal.len() as f64;
        (2.0 * (real * real + imaginary * imaginary).sqrt() / length) as f32
    }

    fn colour_response(frequency: f64, amplitude: f32) -> Vec<f32> {
        let mut colour = MasterColour::default();
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let frames = sample_rate as usize / 2;
        let mut output = Vec::with_capacity(frames);
        for frame in 0..frames {
            let input =
                (TAU * frequency * frame as f64 / f64::from(sample_rate)).sin() as f32 * amplitude;
            output.push(colour.process(input, 0, sample_rate, 1.0));
        }
        // Drop the filters' settling time so only the steady state is weighed.
        output.split_off(frames / 2)
    }

    #[test]
    fn saturation_adds_a_third_harmonic_to_the_body_without_squashing_it() {
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let output = colour_response(200.0, 0.8);

        let fundamental = component_amplitude(&output, 200.0, sample_rate);
        let third = component_amplitude(&output, 600.0, sample_rate);

        assert!(
            third > fundamental * 0.005,
            "the curve should colour the body, got a third harmonic of {third} against {fundamental}"
        );
        assert!(
            third < fundamental * 0.1,
            "this is a console stage, not a distortion: {third} against {fundamental}"
        );
        // The shelves lift 200 Hz a little and the curve gives some back; the
        // net must stay within a decibel either way.
        let level_db = 20.0 * (fundamental / 0.8).log10();
        assert!(
            level_db.abs() < 1.0,
            "the fundamental should survive intact, got {level_db} dB"
        );
    }

    #[test]
    fn saturation_leaves_the_top_of_the_spectrum_clean() {
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        // Above the split. Were this tone saturated, its third harmonic at
        // 36 kHz would fold back to 8.1 kHz as an inharmonic whistle — the very
        // artefact the band split exists to prevent.
        let output = colour_response(12_000.0, 0.8);

        let fundamental = component_amplitude(&output, 12_000.0, sample_rate);
        let alias = component_amplitude(
            &output,
            f64::from(3 * 12_000 - sample_rate as i32).abs(),
            sample_rate,
        );

        assert!(
            alias < fundamental * 0.001,
            "the air path must stay linear, got an alias of {alias} against {fundamental}"
        );
    }

    #[test]
    fn the_colour_stage_is_transparent_when_blended_out() {
        let mut colour = MasterColour::default();
        for frame in 0..1_000 {
            let input = (TAU * 200.0 * frame as f64 / 44_100.0).sin() as f32;
            let output = colour.process(input, 0, FALLBACK_OUTPUT_SAMPLE_RATE, 0.0);
            assert!((output - input).abs() < 1.0e-6);
        }
    }

    /// Feeds a four-to-the-floor kick to the ducker and returns the gain it
    /// applies across one beat, sampled from just after a kick to just before
    /// the next one.
    fn duck_gain_across_a_beat(bpm: f64) -> Vec<f32> {
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let beat_frames = (60.0 / bpm * f64::from(sample_rate)) as usize;
        let mut ducker = SidechainDucker::default();
        ducker.set_tempo(bpm, sample_rate);

        let mut gains = Vec::new();
        for frame in 0..beat_frames * 6 {
            let into_beat = frame % beat_frames;
            // A 60 Hz thump on every beat, silence between them. Its length is
            // a fraction of the beat, not a fixed number of milliseconds, so
            // two tempos are compared on musically identical material.
            let key = if into_beat < beat_frames / 16 {
                0.9 * (TAU * 60.0 * into_beat as f64 / f64::from(sample_rate)).sin() as f32
            } else {
                0.0
            };
            let gain = ducker.process(key, sample_rate, 1.0);
            if frame >= beat_frames * 5 {
                gains.push(gain);
            }
        }
        gains
    }

    #[test]
    fn the_ducker_slams_on_the_kick_and_swells_back_before_the_next_one() {
        let gains = duck_gain_across_a_beat(128.0);

        let deepest = gains.iter().copied().fold(1.0_f32, f32::min);
        assert!(
            deepest <= DUCK_FLOOR + 0.02,
            "the kick should push the gain to the floor, reached {deepest}"
        );

        // By the end of the beat the swell is home, which is what the ear reads
        // as breathing rather than as a gate chattering.
        let last = *gains.last().expect("a beat of gains");
        assert!(last > 0.97, "the gain had only recovered to {last}");

        // And it climbs, rather than jumping back.
        let quarter = gains[gains.len() / 4];
        let half = gains[gains.len() / 2];
        assert!(
            deepest < quarter && quarter < half && half < last,
            "the release should rise steadily: {deepest} {quarter} {half} {last}"
        );
    }

    #[test]
    fn the_release_follows_the_tempo() {
        // At half the tempo the beat is twice as long, so the swell must take
        // twice as long too or the pump stops matching the music.
        let fast = duck_gain_across_a_beat(160.0);
        let slow = duck_gain_across_a_beat(80.0);
        let midpoint = |gains: &[f32]| gains[gains.len() / 2];
        assert!(
            (midpoint(&fast) - midpoint(&slow)).abs() < 0.05,
            "the same point of the beat should sit at the same gain: {} vs {}",
            midpoint(&fast),
            midpoint(&slow)
        );
    }

    #[test]
    fn a_silent_key_leaves_everything_alone() {
        let mut ducker = SidechainDucker::default();
        ducker.set_tempo(128.0, FALLBACK_OUTPUT_SAMPLE_RATE);
        for _ in 0..FALLBACK_OUTPUT_SAMPLE_RATE {
            assert_eq!(ducker.process(0.0, FALLBACK_OUTPUT_SAMPLE_RATE, 1.0), 1.0);
        }
    }

    #[test]
    fn the_detector_answers_to_the_kick_rather_than_to_the_hats() {
        // The key can be a whole track, so a bright transient must not pump it.
        // Both are struck rather than held: a held tone is a bassline, and the
        // test below covers that separately.
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let struck = |frequency: f64| {
            let mut ducker = SidechainDucker::default();
            ducker.set_tempo(128.0, sample_rate);
            let mut lowest = 1.0_f32;
            let burst = sample_rate as usize / 40;
            for frame in 0..sample_rate as usize / 4 {
                let key = if frame < burst {
                    0.9 * (TAU * frequency * frame as f64 / f64::from(sample_rate)).sin() as f32
                } else {
                    0.0
                };
                lowest = lowest.min(ducker.process(key, sample_rate, 1.0));
            }
            lowest
        };

        let kick = struck(55.0);
        let hat = struck(9_000.0);
        assert!(kick <= DUCK_FLOOR + 0.01, "a 55 Hz hit should duck: {kick}");
        assert!(hat > 0.9, "a 9 kHz hit should barely duck at all: {hat}");
    }

    /// Une clé poussée sur un côté ne pousse plus la somme mono comme une clé au
    /// milieu : trois décibels de moins à l'arrivée, donc un pompage plus léger.
    /// C'est ce que fait une console dont le départ est pris après le
    /// panoramique.
    #[test]
    fn a_key_pushed_to_one_side_pumps_less_than_one_in_the_middle() {
        // Le centre est la référence : le cas courant ne doit rien changer par
        // rapport à ce qui existait avant que le panoramique compte.
        assert!((sidechain_pan_weight(0.0) - 1.0).abs() < 1e-6);

        let hard_left = sidechain_pan_weight(-1.0);
        let hard_right = sidechain_pan_weight(1.0);
        assert!(
            (hard_left - hard_right).abs() < 1e-6,
            "les deux côtés pèsent pareil"
        );
        assert!((hard_left - 1.0 / std::f32::consts::SQRT_2).abs() < 1e-6);
        let half = sidechain_pan_weight(-0.5);
        assert!(
            hard_left < half && half < 1.0,
            "le poids doit varier continûment: {half}"
        );

        let centred = lowest_duck_gain_at_depth(128.0, 0.2, 0.9, 1.0);
        let sided = lowest_duck_gain_at_depth(128.0, 0.2, 0.9, hard_left);
        assert!(
            sided > centred + 0.01,
            "une clé sur le côté doit creuser moins: {sided} contre {centred}"
        );
        assert!(sided < 0.95, "elle doit tout de même pomper: {sided}");
    }

    /// Lowest gain reached over a few beats of a key made of a sustained
    /// bassline plus, optionally, a kick on each beat.
    fn lowest_duck_gain(bpm: f64, bass: f32, kick: f32) -> f32 {
        lowest_duck_gain_at_depth(bpm, bass, kick, 1.0)
    }

    fn lowest_duck_gain_at_depth(bpm: f64, bass: f32, kick: f32, depth: f32) -> f32 {
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let beat_frames = (60.0 / bpm * f64::from(sample_rate)) as usize;
        let mut ducker = SidechainDucker::default();
        ducker.set_tempo(bpm, sample_rate);

        let mut lowest = 1.0_f32;
        for frame in 0..beat_frames * 5 {
            let phase = frame as f64 / f64::from(sample_rate);
            // A bassline sits in the kick's own band and never stops.
            let mut key = bass * (TAU * 80.0 * phase).sin() as f32;
            let into_beat = frame % beat_frames;
            if into_beat < beat_frames / 16 {
                key += kick * (TAU * 55.0 * into_beat as f64 / f64::from(sample_rate)).sin() as f32;
            }
            let gain = ducker.process(key, sample_rate, depth);
            // Skip the first beat, while the slow envelope is still filling.
            if frame >= beat_frames {
                lowest = lowest.min(gain);
            }
        }
        lowest
    }

    /// The failure this detector exists to avoid: reading a bassline as one
    /// long hit, which reduces gain continuously instead of pumping per kick.
    #[test]
    fn a_bassline_alone_never_triggers_the_duck() {
        let lowest = lowest_duck_gain(128.0, 0.6, 0.0);
        assert!(
            lowest > 0.99,
            "a sustained bassline must leave the gain alone, it reached {lowest}"
        );
    }

    #[test]
    fn a_kick_over_a_bassline_still_triggers_the_duck() {
        let lowest = lowest_duck_gain(128.0, 0.6, 0.9);
        assert!(
            lowest <= DUCK_FLOOR + 0.01,
            "a kick riding a bassline should duck fully, it reached {lowest}"
        );
    }

    /// The detector has to hold across the range a mix actually presents: a
    /// loud bassline under a modest kick, and any club tempo.
    #[test]
    fn the_two_ways_of_writing_the_duck_depth_agree() {
        assert!(
            (DUCK_FLOOR - 10_f32.powf(-DUCK_DEPTH_DB / 20.0)).abs() < 1.0e-6,
            "DUCK_FLOOR devrait valoir 10^(−DUCK_DEPTH_DB/20), soit {}",
            10_f32.powf(-DUCK_DEPTH_DB / 20.0)
        );
    }

    /// L'envoi de reverb : ce que le geste, la passe écrite et la gomme font
    /// ensemble. La règle vit hors de la boucle de mixage précisément pour être
    /// vérifiable ici.
    mod reverb_send {
        use crate::audio::timeline::next_effect_send;

        /// Assez grossiers pour que chaque pas se lise à l'œil.
        const ATTACK: f32 = 0.25;
        const RELEASE: f32 = 0.10;

        #[test]
        fn a_held_pad_climbs_to_full_and_stops_there() {
            let mut send = 0.0;
            for _ in 0..8 {
                send = next_effect_send(send, true, false, 0.0, ATTACK, RELEASE);
            }
            assert_eq!(send, 1.0);
        }

        #[test]
        fn a_released_pad_comes_back_down_to_nothing() {
            let mut send = 1.0;
            for _ in 0..20 {
                send = next_effect_send(send, false, false, 0.0, ATTACK, RELEASE);
            }
            assert_eq!(send, 0.0);
        }

        /// Rejouer par-dessus une passe écrite doit s'entendre comme la même
        /// reverb, pas comme le double.
        #[test]
        fn the_pass_and_the_gesture_combine_by_the_larger_of_the_two() {
            assert_eq!(
                next_effect_send(1.0, true, false, 1.0, ATTACK, RELEASE),
                1.0
            );
            // La passe seule porte l'envoi, geste au repos.
            assert_eq!(
                next_effect_send(0.0, false, false, 0.6, ATTACK, RELEASE),
                0.6
            );
        }

        /// Le défaut signalé : la gomme effaçait bien en base, mais on
        /// continuait d'entendre la passe pendant qu'on l'effaçait.
        #[test]
        fn the_eraser_silences_the_recorded_pass_while_it_is_held() {
            assert_eq!(
                next_effect_send(0.0, false, true, 1.0, ATTACK, RELEASE),
                0.0
            );
        }

        /// Elle ne coupe pas net : le gain courant porte encore la valeur de
        /// l'automation, et la descente reprend là où elle était.
        #[test]
        fn the_eraser_lets_the_send_fall_rather_than_cutting_it() {
            let under_the_eraser = next_effect_send(1.0, false, true, 1.0, ATTACK, RELEASE);
            assert_eq!(under_the_eraser, 1.0 - RELEASE);
            let released = next_effect_send(1.0, false, false, 0.0, ATTACK, RELEASE);
            assert_eq!(
                under_the_eraser, released,
                "effacer doit retomber exactement comme relâcher"
            );
        }

        /// Tenir la reverb et la gomme ensemble laisse entendre ce qu'on joue —
        /// c'est bien ce qui restera, puisque réécrire est le seul geste qui
        /// survit à l'effacement.
        #[test]
        fn playing_over_the_eraser_is_still_heard() {
            let send = next_effect_send(0.5, true, true, 1.0, ATTACK, RELEASE);
            assert_eq!(send, 0.75);
        }

        #[test]
        fn the_send_never_leaves_its_bounds() {
            for held in [false, true] {
                for erasing in [false, true] {
                    let mut send = 0.5;
                    for _ in 0..100 {
                        send = next_effect_send(send, held, erasing, 1.0, ATTACK, RELEASE);
                        assert!((0.0..=1.0).contains(&send), "envoi hors bornes : {send}");
                    }
                }
            }
        }
    }

    /// Le défaut signalé : la profondeur était fixe, si bien que monter le
    /// volume de la piste-clé ne produisait aucune progression — pompage plein
    /// tant que le détecteur déclenchait, rien dès qu'il passait sous son
    /// plancher.
    #[test]
    fn the_pump_deepens_as_the_key_is_faded_in() {
        let depths: Vec<f32> = [0.0, 0.25, 0.5, 0.75, 1.0]
            .into_iter()
            .map(|scale| lowest_duck_gain_at_depth(128.0, 0.05, 0.9, scale))
            .collect();

        for pair in depths.windows(2) {
            assert!(
                pair[1] < pair[0] - 0.02,
                "monter la clé doit creuser le pompage, obtenu {pair:?}"
            );
        }
        assert!(
            depths[0] > 0.99,
            "clé à zéro : aucun pompage, got {}",
            depths[0]
        );
        assert!(
            (depths[4] - DUCK_FLOOR).abs() < 0.01,
            "clé à plein : la profondeur nominale, got {}",
            depths[4]
        );
    }

    /// Une profondeur plus faible ne doit pas remonter plus vite : le groove
    /// changerait pendant qu'on monte le fader.
    #[test]
    fn the_pump_recovers_in_the_same_musical_time_at_any_depth() {
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let bpm = 128.0;
        let recovery_frames = |depth: f32| {
            let mut ducker = SidechainDucker::default();
            ducker.set_tempo(bpm, sample_rate);
            // Un coup unique, puis du silence : on compte la remontée.
            ducker.fast = 1.0;
            ducker.slow = 0.0;
            ducker.process(1.0, sample_rate, depth);
            let mut frames = 0_usize;
            while ducker.process(0.0, sample_rate, depth) < 0.999 && frames < sample_rate as usize {
                frames += 1;
            }
            frames
        };

        let shallow = recovery_frames(0.35);
        let full = recovery_frames(1.0);
        let tolerance = (full as f64 * 0.1) as usize;
        assert!(
            shallow.abs_diff(full) <= tolerance,
            "les deux profondeurs doivent remonter dans le même temps : {shallow} contre {full}"
        );
    }

    #[test]
    fn the_trigger_survives_a_loud_bassline_and_any_tempo() {
        for bpm in [90.0, 110.0, 128.0, 150.0, 174.0] {
            let quiet = lowest_duck_gain(bpm, 0.7, 0.0);
            assert!(
                quiet > 0.99,
                "a bassline at {bpm} BPM triggered the duck, reaching {quiet}"
            );

            // A kick standing above the bassline, as one does in a mix.
            let struck = lowest_duck_gain(bpm, 0.7, 0.9);
            assert!(
                struck <= DUCK_FLOOR + 0.01,
                "a kick at {bpm} BPM failed to duck, reaching {struck}"
            );
        }
    }

    #[test]
    fn a_kick_pumps_once_per_beat_rather_than_chattering() {
        // Counts how many times the gain is driven back to the floor: one hit
        // per beat, not several as a decaying kick rings out.
        let sample_rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let bpm = 128.0;
        let beat_frames = (60.0 / bpm * f64::from(sample_rate)) as usize;
        let mut ducker = SidechainDucker::default();
        ducker.set_tempo(bpm, sample_rate);

        let mut hits = 0;
        let mut previous = 1.0_f32;
        let beats = 8;
        for frame in 0..beat_frames * beats {
            let into_beat = frame % beat_frames;
            let key = if into_beat < beat_frames / 16 {
                0.9 * (TAU * 55.0 * into_beat as f64 / f64::from(sample_rate)).sin() as f32
            } else {
                0.0
            };
            let gain = ducker.process(key, sample_rate, 1.0);
            if gain < previous - 0.01 {
                hits += 1;
            }
            previous = gain;
        }

        assert_eq!(hits, beats, "expected one duck per beat, counted {hits}");
    }

    #[test]
    fn the_limiter_pulls_a_hot_signal_under_the_ceiling_then_recovers() {
        let mut limiter = MasterLimiter::default();
        assert_eq!(limiter.process(0.5, FALLBACK_OUTPUT_SAMPLE_RATE, true), 1.0);

        // A signal well over the ceiling is brought down within a few ms.
        let attack_samples = (FALLBACK_OUTPUT_SAMPLE_RATE as f32 * 0.02) as usize;
        for _ in 0..attack_samples {
            limiter.process(2.0, FALLBACK_OUTPUT_SAMPLE_RATE, true);
        }
        let reduced = limiter.process(2.0, FALLBACK_OUTPUT_SAMPLE_RATE, true);
        assert!(
            2.0 * reduced <= OUTPUT_CEILING + 1.0e-3,
            "limited peak was {}",
            2.0 * reduced
        );

        // Once the signal drops back, the gain returns towards unity.
        let release_samples = (FALLBACK_OUTPUT_SAMPLE_RATE as f32 * 1.0) as usize;
        for _ in 0..release_samples {
            limiter.process(0.1, FALLBACK_OUTPUT_SAMPLE_RATE, true);
        }
        assert!(
            limiter.process(0.1, FALLBACK_OUTPUT_SAMPLE_RATE, true) > 0.99,
            "the limiter should let a quiet signal through untouched"
        );
    }

    #[test]
    fn a_disabled_limiter_never_touches_the_gain() {
        let mut limiter = MasterLimiter::default();
        for _ in 0..1_000 {
            assert_eq!(
                limiter.process(4.0, FALLBACK_OUTPUT_SAMPLE_RATE, false),
                1.0
            );
        }
    }

    #[test]
    fn the_limiter_gain_is_shared_by_both_channels() {
        // A frame loud on one side only must keep its balance: the gain comes
        // from the frame peak, so the ratio between the channels is preserved.
        let mut limiter = MasterLimiter::default();
        let left = 2.0_f32;
        let right = 0.5_f32;
        let mut gain = 1.0;
        for _ in 0..(FALLBACK_OUTPUT_SAMPLE_RATE as f32 * 0.02) as usize {
            gain = limiter.process(left.max(right), FALLBACK_OUTPUT_SAMPLE_RATE, true);
        }
        assert!(((left * gain) / (right * gain) - left / right).abs() < 1.0e-4);
    }

    #[test]
    fn smooth_crossfade_is_complementary_and_has_soft_edges() {
        assert_eq!(smooth_crossfade(0.0), 0.0);
        assert_eq!(smooth_crossfade(1.0), 1.0);
        assert!((smooth_crossfade(0.5) - 0.5).abs() < 1.0e-6);
        assert!(smooth_crossfade(0.01) < 0.001);
    }

    #[test]
    fn wsola_search_finds_a_stereo_linked_waveform_match() {
        let input = stereo_sine(437.0, 1.0, FALLBACK_OUTPUT_SAMPLE_RATE);
        let reference = 8_000.0;
        let nominal = reference + 41.0;
        let offset = best_wsola_offset(reference, nominal, 36, |position, channel| {
            interpolated_sample(&input, position, channel)
        });
        let aligned = nominal + offset as f64;
        let period = f64::from(FALLBACK_OUTPUT_SAMPLE_RATE) / 437.0;
        let phase_error = ((aligned - reference) / period).round() * period;
        let aligned_error = ((aligned - reference) - phase_error).abs();
        let nominal_error = nominal - reference;
        assert!(aligned_error < 8.0, "aligned error was {aligned_error}");
        assert!(aligned_error < nominal_error.abs());
    }

    /// Un accord tenu doit ressortir tenu, pas haché.
    ///
    /// C'est le cas que le sinus ne représentait pas. Une seule périodicité se
    /// recale parfaitement; un accord en a plusieurs à la fois, et un unique
    /// décalage temporel ne peut pas les mettre toutes en phase. Ce qui reste
    /// s'annule partiellement au fondu entre grains, et s'entend comme un
    /// grésillement dissonant sur les nappes.
    ///
    /// Ignoré par défaut : il écrit deux fichiers et fait tourner le moteur
    /// complet. `cargo test pads_survive -- --ignored --nocapture`.
    #[test]
    #[ignore = "mesure de qualité, longue"]
    fn pads_survive_a_small_tempo_change() {
        use crate::audio::stems::write_stereo_wav;
        use crate::timeline::{TimelineRenderClip, TimelineRenderPlan};

        let dossier = std::env::temp_dir().join(format!("mixcanvas-pad-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier de travail");
        let source = dossier.join("pad.wav");
        let rendu = dossier.join("rendu.wav");

        // Une nappe : un accord mineur tenu, avec ses harmoniques. Rien de
        // percussif, aucune attaque — ce sur quoi le défaut s'entend.
        let taux = f64::from(FALLBACK_OUTPUT_SAMPLE_RATE);
        let secondes = 8.0_f64;
        let echantillons = (taux * secondes) as usize;
        let partiels = [110.0_f64, 130.81, 164.81, 220.0, 261.63, 329.63];
        let signal: Vec<f32> = (0..echantillons)
            .map(|index| {
                let t = index as f64 / taux;
                let somme: f64 = partiels
                    .iter()
                    .map(|hz| (2.0 * std::f64::consts::PI * hz * t).sin())
                    .sum();
                (somme / partiels.len() as f64 * 0.6) as f32
            })
            .collect();
        write_stereo_wav(&source, &signal, &signal).expect("la nappe devrait s'écrire");

        /// Part de l'énergie qui n'est plus sur les partiels d'origine.
        ///
        /// Un étirement temporel ne change pas les hauteurs : tout ce qui sort
        /// des raies de départ est une invention du moteur, et c'est ce qu'on
        /// entend comme dissonance. L'enveloppe seule ne la voit pas — un
        /// peigne peut recreuser le spectre sans faire varier le niveau moyen.
        fn hors_partiels(entrelace: &[f32], partiels: &[f64], taux: f64) -> f64 {
            use rustfft::num_complex::Complex32;
            const N: usize = 16_384;
            let mono: Vec<f32> = entrelace.chunks(2).map(|paire| paire[0]).collect();
            let debut = mono.len() / 2;
            let mut tampon: Vec<Complex32> = (0..N)
                .map(|index| {
                    let fenetre =
                        0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / N as f32).cos();
                    Complex32::new(
                        mono.get(debut + index).copied().unwrap_or(0.0) * fenetre,
                        0.0,
                    )
                })
                .collect();
            rustfft::FftPlanner::new()
                .plan_fft_forward(N)
                .process(&mut tampon);

            let resolution = taux / N as f64;
            let mut totale = 0.0_f64;
            let mut sur_raies = 0.0_f64;
            for (bin, valeur) in tampon.iter().take(N / 2).enumerate() {
                let energie = f64::from(valeur.norm_sqr());
                totale += energie;
                let hz = bin as f64 * resolution;
                if partiels
                    .iter()
                    .any(|partiel| (hz - partiel).abs() <= 3.0 * resolution)
                {
                    sur_raies += energie;
                }
            }
            if totale <= 0.0 {
                return 0.0;
            }
            10.0 * ((totale - sur_raies) / totale).log10()
        }

        /// Les six raies les plus fortes, pour savoir si la hauteur a bougé.
        ///
        /// Un étirement qui déplacerait les fréquences ne serait pas un défaut
        /// de grain mais un rééchantillonnage déguisé — un tout autre défaut,
        /// et une tout autre correction.
        fn raies_dominantes(entrelace: &[f32], taux: f64) -> Vec<f64> {
            use rustfft::num_complex::Complex32;
            const N: usize = 16_384;
            let mono: Vec<f32> = entrelace.chunks(2).map(|paire| paire[0]).collect();
            let debut = mono.len() / 2;
            let mut tampon: Vec<Complex32> = (0..N)
                .map(|index| {
                    let fenetre =
                        0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / N as f32).cos();
                    Complex32::new(
                        mono.get(debut + index).copied().unwrap_or(0.0) * fenetre,
                        0.0,
                    )
                })
                .collect();
            rustfft::FftPlanner::new()
                .plan_fft_forward(N)
                .process(&mut tampon);
            let resolution = taux / N as f64;
            let mut pics: Vec<(f64, f64)> = tampon
                .iter()
                .take(N / 2)
                .enumerate()
                .map(|(bin, valeur)| (bin as f64 * resolution, f64::from(valeur.norm_sqr())))
                .collect();
            pics.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let mut gardees: Vec<f64> = Vec::new();
            for (hz, _) in pics {
                if gardees.iter().all(|garde: &f64| (garde - hz).abs() > 12.0) {
                    gardees.push(hz);
                }
                if gardees.len() == 6 {
                    break;
                }
            }
            gardees.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            gardees
        }

        /// Creux et bosse de l'enveloppe, en décibels autour de sa moyenne.
        fn enveloppe(entrelace: &[f32]) -> (f64, f64) {
            let bloc = 1_024_usize;
            let debut = entrelace.len() / 8;
            let fin = entrelace.len() * 7 / 8;
            let mut niveaux = Vec::new();
            let mut index = debut;
            while index + bloc < fin {
                let somme: f64 = entrelace[index..index + bloc]
                    .iter()
                    .map(|v| f64::from(*v) * f64::from(*v))
                    .sum();
                niveaux.push((somme / bloc as f64).sqrt());
                index += bloc;
            }
            let moyenne = niveaux.iter().sum::<f64>() / niveaux.len() as f64;
            let plus_bas = niveaux.iter().cloned().fold(f64::INFINITY, f64::min);
            let plus_haut = niveaux.iter().cloned().fold(0.0_f64, f64::max);
            (
                20.0 * (plus_bas / moyenne).log10(),
                20.0 * (plus_haut / moyenne).log10(),
            )
        }

        let source_bpm = 130.0_f64;
        let mut mesures = Vec::new();
        for project_bpm in [130.0_f64, 123.0] {
            let temps = secondes / 60.0 * source_bpm;
            let plan = TimelineRenderPlan {
                project_bpm,
                tempo_map: TempoMap::new(project_bpm, Vec::new()).expect("tempo constant"),
                end_beat: temps,
                audible_lane_mask: 0b111,
                limiter_enabled: false,
                compressor_enabled: false,
                clips: vec![TimelineRenderClip {
                    id: 1,
                    lane: 0,
                    file_path: source.to_string_lossy().into_owned(),
                    source_bpm,
                    first_beat_ms: 0,
                    anchor_beat: 0.0,
                    visual_start_beat: 0.0,
                    duration_beats: temps,
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
            crate::audio::bounce_timeline(
                &plan,
                &rendu,
                crate::audio::BounceFormat::Wav,
                None,
                &mut |_| {},
            )
            .expect("le rendu devrait aboutir");

            let decodeur = super::open_mp3_decoder(&rendu).expect("le rendu devrait se relire");
            let sortie: Vec<f32> = decodeur.collect();
            let (creux, bosse) = enveloppe(&sortie);
            let parasite = hors_partiels(&sortie, &partiels, taux);
            println!(
                "{source_bpm} → {project_bpm} BPM : creux {creux:>6.2} dB, bosse {bosse:>5.2} dB, hors raies {parasite:>6.2} dB"
            );
            let raies = raies_dominantes(&sortie, taux);
            println!(
                "                     raies : {}",
                raies
                    .iter()
                    .map(|hz| format!("{hz:.0}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            );
            mesures.push((project_bpm, creux, bosse));
        }

        // La référence : le même calcul sur la nappe d'origine. Un accord de
        // partiels non harmoniques bat de lui-même, et le comparer à une
        // platitude théorique accuserait le moteur de ce que le signal fait
        // tout seul.
        let mono: Vec<f32> = signal.iter().flat_map(|v| [*v, *v]).collect();
        let (creux_source, bosse_source) = enveloppe(&mono);
        let parasite_source = hors_partiels(&mono, &partiels, taux);
        println!(
            "           source : creux {creux_source:>6.2} dB, bosse {bosse_source:>5.2} dB, hors raies {parasite_source:>6.2} dB"
        );

        for (bpm, creux, bosse) in &mesures {
            let aggravation = creux_source - creux;
            println!(
                "           {bpm} BPM : {aggravation:>5.2} dB de creux en plus qu'à la source"
            );
            if (*bpm - source_bpm).abs() < 1.0e-9 {
                // Sans étirement, le moteur ne doit rien ajouter du tout.
                assert!(
                    aggravation < 1.0,
                    "à taux 1, le rendu creuse {aggravation:.2} dB de plus que la source"
                );
            }
            let _ = bosse;
        }

        let _ = std::fs::remove_dir_all(&dossier);
    }

    /// Une frappe doit rester une frappe : ni doublée, ni escamotée.
    ///
    /// L'autre côté du compromis. Un grain long flatte les nappes — moins de
    /// raccords par seconde — mais peut répéter ou sauter une attaque, ce qui
    /// s'entend comme un battement sur la grosse caisse. Régler la granulation
    /// sur les seules nappes reviendrait à échanger un défaut contre un autre.
    #[test]
    #[ignore = "mesure de qualité, longue"]
    fn transients_are_neither_doubled_nor_dropped() {
        use crate::audio::stems::write_stereo_wav;
        use crate::timeline::{TimelineRenderClip, TimelineRenderPlan};

        let dossier = std::env::temp_dir().join(format!("mixcanvas-clic-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier de travail");
        let source = dossier.join("clics.wav");
        let rendu = dossier.join("rendu.wav");

        // Une frappe sèche tous les demi-temps, sur du silence : le pire cas
        // pour un grain, et le plus facile à compter.
        let taux = f64::from(FALLBACK_OUTPUT_SAMPLE_RATE);
        let source_bpm = 130.0_f64;
        let secondes = 8.0_f64;
        let periode = (taux * 60.0 / source_bpm / 2.0) as usize;
        let echantillons = (taux * secondes) as usize;
        let signal: Vec<f32> = (0..echantillons)
            .map(|index| {
                let depuis = index % periode;
                if depuis < 220 {
                    let enveloppe = 1.0 - depuis as f32 / 220.0;
                    (2.0 * std::f32::consts::PI * 1_800.0 * depuis as f32 / taux as f32).sin()
                        * enveloppe
                        * 0.8
                } else {
                    0.0
                }
            })
            .collect();
        write_stereo_wav(&source, &signal, &signal).expect("les clics devraient s'écrire");

        let temps = secondes / 60.0 * source_bpm;
        let plan = TimelineRenderPlan {
            project_bpm: 123.0,
            tempo_map: TempoMap::new(123.0, Vec::new()).expect("tempo constant"),
            end_beat: temps,
            audible_lane_mask: 0b111,
            limiter_enabled: false,
            compressor_enabled: false,
            clips: vec![TimelineRenderClip {
                id: 1,
                lane: 0,
                file_path: source.to_string_lossy().into_owned(),
                source_bpm,
                first_beat_ms: 0,
                anchor_beat: 0.0,
                visual_start_beat: 0.0,
                duration_beats: temps,
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
        crate::audio::bounce_timeline(
            &plan,
            &rendu,
            crate::audio::BounceFormat::Wav,
            None,
            &mut |_| {},
        )
        .expect("le rendu devrait aboutir");

        let sortie: Vec<f32> = super::open_mp3_decoder(&rendu)
            .expect("le rendu devrait se relire")
            .collect();
        let mono: Vec<f32> = sortie.chunks(2).map(|paire| paire[0]).collect();

        // Compte les attaques : un franchissement de seuil après du silence.
        let seuil = 0.25_f32;
        let mut attaques = 0_usize;
        let mut armee = true;
        let mut sous_seuil = 0_usize;
        for valeur in &mono {
            if valeur.abs() > seuil {
                if armee {
                    attaques += 1;
                    armee = false;
                }
                sous_seuil = 0;
            } else {
                sous_seuil += 1;
                if sous_seuil > 400 {
                    armee = true;
                }
            }
        }

        // Le rendu dure plus longtemps que la source, mais porte le même nombre
        // de frappes : c'est tout l'intérêt d'un étirement.
        let attendues = (secondes * source_bpm / 60.0 * 2.0).floor() as usize;
        println!("frappes : {attaques} rendues pour {attendues} jouées");
        let ecart = attaques.abs_diff(attendues);
        assert!(
            ecart <= attendues / 10,
            "{attaques} frappes rendues pour {attendues} : le grain en double ou en escamote"
        );

        let _ = std::fs::remove_dir_all(&dossier);
    }

    /// Le recalage doit rester en phase sur toute la bande utile.
    ///
    /// Écrit d'abord comme diagnostic, en cherchant l'origine d'un rendu
    /// « granuleux » : la corrélation ne lit qu'un échantillon sur huit, et
    /// l'on pouvait croire qu'elle s'égarait dans l'aigu, où une période ne
    /// fait plus que quinze échantillons. Mesure faite, elle ne s'égare pas —
    /// moins de dix degrés partout. L'hypothèse était fausse, et le test reste
    /// pour que la réponse ne se reperde pas : un sous-échantillonnage plus
    /// grossier, ou une recherche plus courte, le feraient tomber.
    #[test]
    fn wsola_alignment_stays_in_phase_across_the_band() {
        for hz in [110.0_f64, 220.0, 437.0, 880.0, 1_500.0, 3_000.0] {
            let input = stereo_sine(hz, 1.0, FALLBACK_OUTPUT_SAMPLE_RATE);
            let period = f64::from(FALLBACK_OUTPUT_SAMPLE_RATE) / hz;
            let mut pire = 0.0_f64;
            for decalage in [7.0_f64, 19.0, 41.0, 63.0] {
                let reference = 8_000.0;
                let nominal = reference + decalage;
                let offset = best_wsola_offset(reference, nominal, 96, |position, channel| {
                    interpolated_sample(&input, position, channel)
                });
                let aligned = nominal + offset as f64;
                let ecart = aligned - reference;
                let erreur = (ecart - (ecart / period).round() * period).abs();
                pire = pire.max(erreur);
            }
            let degres = pire / period * 360.0;
            println!("{hz:>7.0} Hz — période {period:>6.1} éch. — {degres:>5.1}° d'erreur");
            // Quinze degrés : au-delà, le fondu entre deux grains commence à
            // creuser un peigne dans le spectre, ce qui s'entend.
            assert!(
                degres < 15.0,
                "à {hz} Hz, le recalage se trompe de {degres:.1}°"
            );
        }
    }

    /// Ce qu'un grain coûte au fil audio, compté et borné.
    ///
    /// La correction du time-stretch avait rendu le rayon de recherche toujours
    /// maximal — juste — mais en notant chaque candidat à pleine résolution :
    /// cent trente-cinq mille interpolations par grain, dix-huit fois le coût
    /// d'avant. Un clip passait; deux clips étirés qui se superposent faisaient
    /// craquer la sortie. Rien ne mesurait ce coût, donc rien ne l'a signalé.
    ///
    /// Le plafond est celui d'une recherche hiérarchique avec de la marge. Il
    /// n'est pas là pour être joli : il est là pour tomber si quelqu'un rend la
    /// recherche exhaustive une seconde fois.
    #[test]
    fn one_grain_of_search_stays_within_its_budget() {
        use std::cell::Cell;

        let calls = Cell::new(0_usize);
        let offset = best_wsola_offset(0.0, 4_096.0, WSOLA_MAX_SEARCH_FRAMES, |position, _| {
            calls.set(calls.get() + 1);
            (position * 0.01).sin() as f32
        });

        let per_grain = calls.get();
        assert!(
            per_grain < 30_000,
            "{per_grain} interpolations par grain : à vingt-et-un grains par seconde et par              clip, deux clips superposés dépasseraient ce que le fil audio peut tenir"
        );
        // Et la recherche cherche encore : un plafond qu'on tiendrait en ne
        // faisant rien ne prouverait rien.
        assert!(
            per_grain > 2_000,
            "la recherche ne regarde plus assez de candidats"
        );
        assert!(
            offset.unsigned_abs() <= WSOLA_MAX_SEARCH_FRAMES,
            "le décalage sort du rayon annoncé"
        );
    }

    #[test]
    fn source_tempo_is_converted_to_the_project_tempo() {
        for (source_bpm, project_bpm) in [(125.0, 120.0), (120.0, 125.0)] {
            let duration_ratio = stretch_duration_ratio(source_bpm, project_bpm);
            let source_beat_seconds = 60.0 / source_bpm;
            let output_beat_seconds = source_beat_seconds * duration_ratio;

            assert!((output_beat_seconds - 60.0 / project_bpm).abs() < 1.0e-12);
            assert!((source_bpm / duration_ratio - project_bpm).abs() < 1.0e-12);
        }
    }

    #[test]
    fn vu_ballistics_catch_a_peak_at_once_then_fall_slowly() {
        // Une seule crête suffit : c'est ce que « attaque instantanée » veut
        // dire, et c'est ce qui manquait.
        let release = meter_release_coefficient(FALLBACK_OUTPUT_SAMPLE_RATE);
        let envelope = vu_envelope(0.0, 1.0, release);
        assert_eq!(envelope, 1.0);

        // Puis environ dix décibels par seconde.
        let mut falling = envelope;
        for _ in 0..FALLBACK_OUTPUT_SAMPLE_RATE {
            falling = vu_envelope(falling, 0.0, release);
        }
        let decibels = 20.0 * falling.log10();
        assert!(
            (-11.0..-9.5).contains(&decibels),
            "la chute vaut {decibels} dB par seconde"
        );
    }

    /// Le défaut que l'attaque instantanée corrige, écrit comme un test.
    ///
    /// Un signal dont les crêtes touchent le plein niveau doit **afficher** le
    /// plein niveau, quelle que soit sa moyenne. Avec l'ancienne attaque de
    /// 65 ms, une sinusoïde pleine échelle se lisait à sa moyenne — 2/π, soit
    /// près de 4 dB trop bas — et un morceau réel bien davantage.
    #[test]
    fn a_signal_that_touches_full_scale_reads_as_full_scale() {
        let rate = FALLBACK_OUTPUT_SAMPLE_RATE;
        let release = meter_release_coefficient(rate);
        let mut envelope = 0.0_f32;
        for index in 0..rate {
            let phase = std::f32::consts::TAU * 1_000.0 * index as f32 / rate as f32;
            envelope = vu_envelope(envelope, phase.sin().abs(), release);
        }
        assert!(
            envelope > 0.99,
            "une sinusoïde pleine échelle se lit {envelope}"
        );

        // La moyenne de |sin| vaut 2/pi : c'est ce que l'ancien suiveur
        // affichait, et le nouveau doit s'en écarter franchement.
        assert!(envelope > 2.0 / std::f32::consts::PI + 0.3);
    }

    /// Les budgets de queue doivent valoir la **même durée** à toute fréquence
    /// de sortie.
    ///
    /// Ils étaient écrits en nombres d'images à 48 kHz, si bien qu'à 96 kHz ils
    /// tombaient de moitié : celui du delay serait passé à douze secondes et
    /// demie, sous les vingt-quatre qu'il lui faut à quarante BPM. Le défaut
    /// corrigé en portant ce budget de quinze à vingt-cinq secondes revenait
    /// donc intact dès qu'on branchait une autre interface, et **rien ne
    /// l'aurait signalé** — c'est exactement le genre d'écart qu'une unité
    /// implicite laisse passer.
    #[test]
    fn the_tail_budgets_last_the_same_time_at_any_sample_rate() {
        for rate in [44_100_u32, 48_000, 96_000] {
            let tails = EffectTails::new(rate);
            let seconds = |frames: usize| frames as f32 / rate as f32;
            for (name, frames, expected) in [
                ("reverb", tails.reverb_budget, REVERB_TAIL_SECONDS),
                ("flanger", tails.flanger_budget, FLANGER_TAIL_SECONDS),
                ("delay", tails.delay_budget, DELAY_TAIL_SECONDS),
            ] {
                let measured = seconds(frames);
                assert!(
                    (measured - expected).abs() < 0.001,
                    "à {rate} Hz la queue du {name} dure {measured} s pour {expected} s attendues"
                );
            }
        }
    }

    /// Écrire une passe reconstruit le plan, donc la source. Tant que les
    /// queues vivaient **dans** la source, elles mouraient à cet instant précis
    /// — celui où l'on relâche le bouton, c'est-à-dire celui où la queue doit
    /// commencer. Elles sont maintenant remises à la source neuve, et seul un
    /// déplacement voulu les vide.
    #[test]
    fn a_rebuilt_source_keeps_the_tail_it_was_handed() {
        let tails = Arc::new(Mutex::new(EffectTails::new(FALLBACK_OUTPUT_SAMPLE_RATE)));
        let ringing = |rack: &mut EffectTails| {
            let (left, right) = rack.reverb.process(0.0, 0.0);
            left.abs().max(right.abs())
        };

        // De l'énergie dans la pièce, comme une passe qu'on vient de jouer.
        {
            let mut rack = tails.lock().expect("le rack devrait se prendre");
            for frame in 0..2_000 {
                let input = if frame == 0 { 1.0 } else { 0.0 };
                rack.reverb.process(input, input);
            }
            let before = ringing(&mut rack);
            assert!(before > 1.0e-5, "la pièce devrait sonner : {before}");
        }

        // Une source neuve reprend les queues, exactement comme après une
        // édition, puis se repositionne.
        let mut source = TimelineMixSource::new(
            Vec::new(),
            44_100,
            0b111,
            MasterDynamics {
                compressor_enabled: false,
                limiter_enabled: true,
            },
            constant_tempo(),
            FALLBACK_OUTPUT_SAMPLE_RATE,
            LaneAutomation {
                volume: unity_automation(),
                pan: centred_pan_automation(),
                filter: bypass_filter_automation(),
                reverb: std::array::from_fn(|_| SendAutomation::default()),
                flanger: std::array::from_fn(|_| SendAutomation::default()),
                bitcrush: std::array::from_fn(|_| SendAutomation::default()),
                delay: std::array::from_fn(|_| SendAutomation::default()),
            },
        );
        source.share_tails(Arc::clone(&tails));
        source
            .try_seek(Duration::from_millis(500))
            .expect("le repositionnement devrait réussir");

        let after = ringing(&mut tails.lock().expect("le rack devrait se prendre"));
        assert!(
            after > 1.0e-5,
            "la reconstruction a coupé la queue : {after}"
        );

        // Un déplacement voulu, lui, vide bien.
        tails.lock().expect("le rack devrait se prendre").reset();
        let emptied = ringing(&mut tails.lock().expect("le rack devrait se prendre"));
        assert_eq!(emptied, 0.0, "un seek voulu doit vider la pièce");
    }

    #[test]
    fn master_meter_preserves_independent_stereo_levels() {
        let mut source = TimelineMixSource::new(
            Vec::new(),
            44_100,
            0b111,
            MasterDynamics {
                compressor_enabled: false,
                limiter_enabled: true,
            },
            constant_tempo(),
            FALLBACK_OUTPUT_SAMPLE_RATE,
            LaneAutomation {
                volume: unity_automation(),
                pan: centred_pan_automation(),
                filter: bypass_filter_automation(),
                reverb: std::array::from_fn(|_| SendAutomation::default()),
                flanger: std::array::from_fn(|_| SendAutomation::default()),
                bitcrush: std::array::from_fn(|_| SendAutomation::default()),
                delay: std::array::from_fn(|_| SendAutomation::default()),
            },
        );
        for frame in 0..44_100 {
            source.update_meter(frame, 0, 1.0);
            source.update_meter(frame, 1, 0.25);
        }
        let (left, right, overload) = source.meter_levels();

        assert!(left > 0.98);
        assert!((0.24..=0.25).contains(&right));
        // The needles follow the master, but `OL` is not their business: it is
        // raised further down the chain, once the output bound has acted.
        assert!(!overload);

        source.reset_meter();
        assert_eq!(source.meter_levels(), (0.0, 0.0, false));
    }

    #[test]
    fn overload_reports_what_the_output_bound_had_to_shave() {
        let mut source = TimelineMixSource::new(
            Vec::new(),
            44_100,
            0b111,
            MasterDynamics {
                compressor_enabled: false,
                limiter_enabled: true,
            },
            constant_tempo(),
            FALLBACK_OUTPUT_SAMPLE_RATE,
            LaneAutomation {
                volume: unity_automation(),
                pan: centred_pan_automation(),
                filter: bypass_filter_automation(),
                reverb: std::array::from_fn(|_| SendAutomation::default()),
                flanger: std::array::from_fn(|_| SendAutomation::default()),
                bitcrush: std::array::from_fn(|_| SendAutomation::default()),
                delay: std::array::from_fn(|_| SendAutomation::default()),
            },
        );

        source.update_overload(true);
        assert!(source.meter_levels().2);

        // The lamp holds for a moment so a brief clip stays visible.
        for _ in 0..(FALLBACK_OUTPUT_SAMPLE_RATE as usize / 2) {
            source.update_overload(false);
        }
        assert!(source.meter_levels().2);
        for _ in 0..FALLBACK_OUTPUT_SAMPLE_RATE as usize {
            source.update_overload(false);
        }
        assert!(!source.meter_levels().2);
    }

    #[test]
    fn a_limited_peak_never_raises_overload_but_an_unlimited_one_does() {
        let hot = 4.0_f32;

        let mut limited = MasterLimiter::default();
        // Once the limiter has settled, the output sits on the bound, so `OL`
        // must stay dark: nothing was shaved.
        for _ in 0..FALLBACK_OUTPUT_SAMPLE_RATE as usize {
            limited.process(hot, FALLBACK_OUTPUT_SAMPLE_RATE, true);
        }
        let gain = limited.process(hot, FALLBACK_OUTPUT_SAMPLE_RATE, true);
        assert!(
            hot * gain <= OVERLOAD_THRESHOLD,
            "a held peak reached {}",
            hot * gain
        );

        // Bypassed, the same peak reaches the bound untouched and lights `OL`.
        let mut bypassed = MasterLimiter::default();
        let bypass_gain = bypassed.process(hot, FALLBACK_OUTPUT_SAMPLE_RATE, false);
        assert!(hot * bypass_gain > OVERLOAD_THRESHOLD);
    }

    #[test]
    fn queued_clone_publishes_levels_to_the_cached_timeline() {
        let source = TimelineMixSource::new(
            Vec::new(),
            44_100,
            0b111,
            MasterDynamics {
                compressor_enabled: false,
                limiter_enabled: true,
            },
            constant_tempo(),
            FALLBACK_OUTPUT_SAMPLE_RATE,
            LaneAutomation {
                volume: unity_automation(),
                pan: centred_pan_automation(),
                filter: bypass_filter_automation(),
                reverb: std::array::from_fn(|_| SendAutomation::default()),
                flanger: std::array::from_fn(|_| SendAutomation::default()),
                bitcrush: std::array::from_fn(|_| SendAutomation::default()),
                delay: std::array::from_fn(|_| SendAutomation::default()),
            },
        );
        let mut queued = source.clone();
        for frame in 0..=METER_PUBLISH_FRAMES {
            queued.update_meter(frame, 0, 0.5);
            queued.update_meter(frame, 1, 0.25);
        }

        let (left, right, overload) = source.meter_levels();
        assert!(left > 0.0);
        assert!(right > 0.0);
        assert!(!overload);
    }

    #[test]
    fn clip_source_position_follows_the_intermediate_tempo_ramp() {
        let tempo_map = TempoMap::new(120.0, vec![TempoPoint::clip_target(16.0, 128.0, 2)])
            .expect("valid tempo ramp");
        let mut clip = placed_clip(0, 1_000_000);
        clip.source_bpm = 128.0;
        let timeline_frame = (tempo_map.seconds_at_beat(8.0)
            * f64::from(FALLBACK_OUTPUT_SAMPLE_RATE))
        .round() as usize;

        let source_position = clip.source_position_at_timeline_frame(
            timeline_frame,
            FALLBACK_OUTPUT_SAMPLE_RATE,
            FALLBACK_OUTPUT_SAMPLE_RATE,
            &tempo_map,
        );
        let expected = 8.0 * 60.0 / 128.0 * f64::from(FALLBACK_OUTPUT_SAMPLE_RATE);

        assert!((source_position - expected).abs() < 2.0);
        assert!((tempo_map.bpm_at_beat(8.0) - 124.0).abs() < 1.0e-9);
    }

    #[test]
    fn lane_audibility_changes_are_shared_with_queued_sources() {
        let source = TimelineMixSource::new(
            Vec::new(),
            44_100,
            0b111,
            MasterDynamics {
                compressor_enabled: false,
                limiter_enabled: true,
            },
            constant_tempo(),
            FALLBACK_OUTPUT_SAMPLE_RATE,
            LaneAutomation {
                volume: unity_automation(),
                pan: centred_pan_automation(),
                filter: bypass_filter_automation(),
                reverb: std::array::from_fn(|_| SendAutomation::default()),
                flanger: std::array::from_fn(|_| SendAutomation::default()),
                bitcrush: std::array::from_fn(|_| SendAutomation::default()),
                delay: std::array::from_fn(|_| SendAutomation::default()),
            },
        );
        let queued = source.clone();

        source.set_audible_lane_mask(0b010);

        assert_eq!(queued.audible_lane_mask.load(Ordering::Relaxed), 0b010);
    }

    #[test]
    fn volume_automation_interpolates_in_decibels() {
        let automation = VolumeAutomation {
            points: vec![
                VolumeFramePoint {
                    frame: 0,
                    gain_db: Some(0.0),
                },
                VolumeFramePoint {
                    frame: 100,
                    gain_db: Some(-12.0),
                },
            ],
        };
        // Mi-chemin entre 0 et −12 dB : −6 dB, parce que l'interpolation se
        // fait en décibels et non en amplitude. Ce chiffre n'a rien à voir
        // avec le niveau par défaut d'une piste.
        let midpoint = automation.gain_at_frame(50);
        assert!((midpoint - 10_f32.powf(-6.0 / 20.0)).abs() < 1.0e-6);
    }

    #[test]
    fn filter_automation_interpolates_the_bipolar_value() {
        let automation = FilterAutomation {
            points: vec![
                FilterFramePoint {
                    frame: 0,
                    value: -1.0,
                    tension: 0.0,
                },
                FilterFramePoint {
                    frame: 100,
                    value: 1.0,
                    tension: 0.0,
                },
            ],
        };
        assert!((automation.value_at_frame(0) + 1.0).abs() < f32::EPSILON);
        assert!((automation.value_at_frame(50) - 0.0).abs() < f32::EPSILON);
        assert!((automation.value_at_frame(75) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn filter_automation_stays_bypassed_before_its_first_node() {
        let automation = FilterAutomation {
            points: vec![FilterFramePoint {
                frame: 20,
                value: 0.75,
                tension: 0.0,
            }],
        };
        assert!((automation.value_at_frame(0) - 0.0).abs() < f32::EPSILON);
        assert!((automation.value_at_frame(20) - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn filter_automation_applies_the_previous_node_curve_tension() {
        let automation = FilterAutomation {
            points: vec![
                FilterFramePoint {
                    frame: 0,
                    value: 0.0,
                    tension: 1.0,
                },
                FilterFramePoint {
                    frame: 100,
                    value: 1.0,
                    tension: 0.0,
                },
            ],
        };
        assert!((automation.value_at_frame(50) - 0.0625).abs() < f32::EPSILON);
    }

    #[test]
    fn filter_cutoffs_keep_the_extreme_sweeps_musical() {
        assert!((filter_cutoff_hz(-1.0) - FILTER_LOW_PASS_CLOSED_HZ).abs() < f32::EPSILON);
        assert!((filter_cutoff_hz(-0.0) - FILTER_HIGH_PASS_OPEN_HZ).abs() < f32::EPSILON);
        assert!((filter_cutoff_hz(1.0) - FILTER_HIGH_PASS_CLOSED_HZ).abs() < f32::EPSILON);
    }

    #[test]
    fn filter_makeup_gain_is_a_linear_db_ramp_per_direction() {
        assert!((filter_makeup_gain(0.0) - 1.0).abs() < f32::EPSILON);
        assert!((filter_makeup_gain(-1.0) - 10_f32.powf(6.0 / 20.0)).abs() < 1.0e-6);
        assert!((filter_makeup_gain(1.0) - 10_f32.powf(4.5 / 20.0)).abs() < 1.0e-6);
    }

    #[test]
    fn negative_infinity_volume_node_is_exact_silence() {
        let automation = VolumeAutomation {
            points: vec![
                VolumeFramePoint {
                    frame: 0,
                    gain_db: Some(0.0),
                },
                VolumeFramePoint {
                    frame: 100,
                    gain_db: None,
                },
            ],
        };
        assert_eq!(automation.gain_at_frame(100), 0.0);
        assert_eq!(automation.gain_at_frame(200), 0.0);
    }

    #[test]
    fn timeline_seek_resets_streaming_readers_without_prior_decoding() {
        let clip = placed_clip(0, 44_100);
        let mut source = TimelineMixSource::new(
            vec![clip],
            44_100,
            0b111,
            MasterDynamics {
                compressor_enabled: false,
                limiter_enabled: true,
            },
            constant_tempo(),
            FALLBACK_OUTPUT_SAMPLE_RATE,
            LaneAutomation {
                volume: unity_automation(),
                pan: centred_pan_automation(),
                filter: bypass_filter_automation(),
                reverb: std::array::from_fn(|_| SendAutomation::default()),
                flanger: std::array::from_fn(|_| SendAutomation::default()),
                bitcrush: std::array::from_fn(|_| SendAutomation::default()),
                delay: std::array::from_fn(|_| SendAutomation::default()),
            },
        );
        source
            .try_seek(Duration::from_millis(500))
            .expect("timeline seek should succeed");
        assert_eq!(source.position_sample, 44_100);
        assert!(source.clips[0].reader.is_none());
    }

    #[test]
    fn timeline_seek_ignores_a_cached_mix_without_an_output_player() {
        let tempo_map = constant_tempo();
        let source = TimelineMixSource::new(
            Vec::new(),
            44_100,
            0b111,
            MasterDynamics {
                compressor_enabled: false,
                limiter_enabled: true,
            },
            constant_tempo(),
            FALLBACK_OUTPUT_SAMPLE_RATE,
            LaneAutomation {
                volume: unity_automation(),
                pan: centred_pan_automation(),
                filter: bypass_filter_automation(),
                reverb: std::array::from_fn(|_| SendAutomation::default()),
                flanger: std::array::from_fn(|_| SendAutomation::default()),
                bitcrush: std::array::from_fn(|_| SendAutomation::default()),
                delay: std::array::from_fn(|_| SendAutomation::default()),
            },
        );
        let mut engine = TimelinePlaybackEngine {
            output: None,
            player: None,
            tails: None,
            cached: Some(CachedTimeline {
                signature: 1,
                tempo_signature: tempo_map.signature(),
                end_beat: 16.0,
                duration: Duration::from_secs(8),
                source,
            }),
        };

        assert!(
            engine
                .seek_if_current(4.0, &tempo_map, 16.0)
                .expect("a released output should not be seeked")
                .is_none()
        );
    }

    /// Le bounce hors ligne passe par `prepare_timeline`, comme le transport.
    /// C'est ce qui lui fait hériter des interrupteurs master — encore faut-il
    /// que le plan les porte jusqu'à la source, ce que ce test verrouille.
    #[test]
    fn a_prepared_mix_carries_the_master_switches_the_project_saved() {
        let plan_with = |limiter: bool, compressor: bool| TimelineRenderPlan {
            project_bpm: 120.0,
            tempo_map: TempoMap::new(120.0, Vec::new()).expect("valid tempo map"),
            end_beat: 16.0,
            audible_lane_mask: 0b111,
            limiter_enabled: limiter,
            compressor_enabled: compressor,
            clips: vec![TimelineRenderClip {
                id: 1,
                lane: 0,
                file_path: "definitely-missing-switch-test.mp3".to_owned(),
                source_bpm: 120.0,
                first_beat_ms: 0,
                anchor_beat: 0.0,
                visual_start_beat: 0.0,
                duration_beats: 16.0,
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

        for (limiter, compressor) in [(true, true), (true, false), (false, true), (false, false)] {
            let source = prepare_timeline(&plan_with(limiter, compressor), false, 44_100)
                .expect("the plan should prepare");
            assert_eq!(
                source.limiter_enabled.load(Ordering::Relaxed),
                limiter,
                "LIMIT should reach the render as the project saved it"
            );
            assert_eq!(
                source.compressor_enabled.load(Ordering::Relaxed),
                compressor,
                "COMP should reach the render as the project saved it"
            );
        }
    }

    #[test]
    fn live_edit_refresh_builds_relationships_without_opening_mp3_files() {
        let plan = TimelineRenderPlan {
            project_bpm: 120.0,
            tempo_map: TempoMap::new(120.0, vec![TempoPoint::clip_target(0.0, 125.0, 1)])
                .expect("valid tempo map"),
            end_beat: 16.0,
            audible_lane_mask: 0b111,
            limiter_enabled: true,
            compressor_enabled: false,
            clips: vec![TimelineRenderClip {
                id: 1,
                lane: 0,
                file_path: "definitely-missing-live-edit-test.mp3".to_owned(),
                source_bpm: 125.0,
                first_beat_ms: 0,
                anchor_beat: 0.0,
                visual_start_beat: 0.0,
                duration_beats: 16.0,
                trim_start_beats: 0.0,
                trim_end_beats: 0.0,
                is_sidechain_key: false,
                eq_settings: None,
            }],
            volume_nodes: vec![TimelineVolumeNode {
                id: 1,
                lane: 0,
                beat: 8.0,
                gain_db: Some(-6.0),
                draw_group_id: None,
            }],
            pan_nodes: Vec::new(),
            filter_nodes: Vec::new(),
            reverb_nodes: Vec::new(),
            flanger_nodes: Vec::new(),
            bitcrush_nodes: Vec::new(),
            delay_nodes: Vec::new(),
        };

        let source = prepare_timeline(&plan, false, FALLBACK_OUTPUT_SAMPLE_RATE)
            .expect("a live edit should only rebuild compact playback relationships");
        assert!(source.clips[0].reader.is_none());
        assert!(prepare_timeline(&plan, true, FALLBACK_OUTPUT_SAMPLE_RATE).is_err());

        // The signature decides whether a cached mix may be reused. Anything
        // that changes what is rendered has to move it, or the engine plays on
        // with a mix that no longer matches the project.
        let mut keyed = plan.clone();
        keyed.clips[0].is_sidechain_key = true;
        assert_ne!(
            playback_signature(&plan),
            playback_signature(&keyed),
            "naming a sidechain key must invalidate the cached mix"
        );

        // Whereas the master switches are shared atomically with the queued
        // source, so they deliberately leave the signature alone.
        let mut compressed = plan.clone();
        compressed.compressor_enabled = true;
        assert_eq!(playback_signature(&plan), playback_signature(&compressed));
    }
}
