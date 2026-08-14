//! Le flanger : une copie du signal qui glisse contre lui-même.
//!
//! Un retard court — quelques millisecondes — dont la longueur est promenée par
//! un oscillateur lent. Sommée au signal sec, cette copie décalée fait
//! s'annuler certaines fréquences et s'additionner d'autres : un peigne de
//! creux qui monte et descend avec l'oscillateur. C'est ce balayage qu'on
//! entend, pas le retard lui-même.
//!
//! **Ce module ne rend que le signal mouillé**, comme la reverb. Le sec ne
//! passe jamais par ici : l'envoi est pris sur chaque piste et le retour se
//! somme au bus master, où il rencontre le sec de cette même piste. C'est là
//! que le peigne se forme, et c'est aussi pourquoi le retour doit sortir près
//! du niveau qu'on lui donne — deux signaux d'amplitudes très différentes ne
//! s'annulent pas, ils se superposent, et il ne reste qu'un écho terne.

/// La longueur du retard, au creux et à la crête de l'oscillateur.
///
/// Sous la milliseconde, le peigne est si large que ses creux sortent du
/// spectre utile et il ne reste qu'une coloration. Au-delà d'une dizaine, les
/// creux se resserrent au point qu'on entend un écho distinct plutôt qu'un
/// timbre. Un à sept millisecondes est la fenêtre où le balayage s'entend comme
/// un mouvement du son et non comme une répétition.
const DELAY_MIN_MS: f32 = 1.0;
const DELAY_MAX_MS: f32 = 7.0;

/// La vitesse du balayage, en cycles par seconde.
///
/// Lent : c'est un mouvement qu'on doit sentir passer sous la musique, pas une
/// vibration. Un demi-cycle par seconde met deux secondes à faire l'aller et le
/// retour, ce qui tombe bien sur une mesure à tempo de club.
const LFO_HZ: f32 = 0.5;

/// Ce que la sortie réinjecte dans sa propre ligne à retard.
///
/// C'est le réglage qui décide si l'effet est discret ou métallique : sans
/// rebouclage le peigne n'a que des creux doux, avec trop il se met à siffler
/// sur les fréquences qu'il renforce. Aux deux tiers, les creux sont francs et
/// le sifflement reste hors de portée.
const FEEDBACK: f32 = 0.66;

/// Ce que le retour pèse dans le mix, envoi à fond.
///
/// Bien plus haut que celui de la reverb, et pour une raison de fond : la
/// reverb s'ajoute au mix, le flanger doit s'**annuler** avec lui. Deux signaux
/// d'amplitudes très différentes ne produisent pas de creux — atténuer le
/// retour reviendrait à supprimer l'effet tout en le laissant s'entendre comme
/// un écho. Un peu sous l'unité, pour que la somme ne dépasse pas.
const RETURN_GAIN: f32 = 0.9;

/// Une poussière dans la boucle de rebouclage.
///
/// Même raison que dans la reverb : une queue qui tend vers zéro traverse des
/// nombres dénormaux, et ceux-ci coûtent sur x86 des dizaines de fois le prix
/// d'un calcul normal — un ralentissement qui n'apparaît qu'**après** que le
/// son a cessé.
const DENORMAL_GUARD: f32 = 1.0e-18;

/// Le décalage de phase entre les deux oscillateurs, en tours.
///
/// Un quart de tour. Avec un seul oscillateur les deux canaux balaient
/// ensemble et l'effet reste au centre de la tête; décalés, les creux d'une
/// oreille tombent sur les bosses de l'autre et le son tourne dans l'image.
const STEREO_PHASE: f32 = 0.25;

/// Une ligne à retard à lecture fractionnaire.
///
/// La longueur du retard varie de façon continue, donc la tête de lecture tombe
/// entre deux échantillons. Une interpolation linéaire entre les deux voisins
/// suffit ici : les creux du peigne sont larges, et l'erreur qu'elle laisse est
/// bien sous ce qu'on peut entendre.
struct Line {
    buffer: Vec<f32>,
    write: usize,
}

impl Line {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(2)],
            write: 0,
        }
    }

    fn push(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write = (self.write + 1) % self.buffer.len();
    }

    /// Le signal tel qu'il était il y a `delay` échantillons.
    fn read(&self, delay: f32) -> f32 {
        let length = self.buffer.len();
        let clamped = delay.clamp(1.0, (length - 1) as f32);
        // `write` pointe déjà sur la case suivante, donc l'échantillon qu'on
        // vient d'écrire est à une case en arrière.
        let back = clamped + 1.0;
        let whole = back.floor();
        let fraction = back - whole;
        let first = (self.write + length - whole as usize) % length;
        let second = (first + length - 1) % length;
        self.buffer[first] * (1.0 - fraction) + self.buffer[second] * fraction
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
    }
}

/// Un canal : sa ligne à retard, et la phase de son oscillateur.
struct Channel {
    line: Line,
    phase: f32,
}

impl Channel {
    fn new(length: usize, phase: f32) -> Self {
        Self {
            line: Line::new(length),
            phase,
        }
    }

    fn process(&mut self, input: f32, delay: f32) -> f32 {
        let delayed = self.line.read(delay);
        self.line.push(input + delayed * FEEDBACK + DENORMAL_GUARD);
        delayed
    }

    fn reset(&mut self, phase: f32) {
        self.line.reset();
        self.phase = phase;
    }
}

/// Le flanger partagé par les trois pistes.
///
/// Un seul pour tout le mix, comme la pièce de reverb : trois pistes envoyées
/// dans trois balayages désaccordés ne sonneraient pas comme un même effet, et
/// c'est trois fois moins cher.
pub(crate) struct Flanger {
    left: Channel,
    right: Channel,
    /// L'avance de phase par échantillon, calculée une fois.
    step: f32,
    min_delay: f32,
    max_delay: f32,
}

impl Flanger {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let rate = sample_rate as f32;
        let max_delay = DELAY_MAX_MS * 0.001 * rate;
        Self {
            left: Channel::new(max_delay as usize + 4, 0.0),
            right: Channel::new(max_delay as usize + 4, STEREO_PHASE),
            step: LFO_HZ / rate,
            min_delay: DELAY_MIN_MS * 0.001 * rate,
            max_delay,
        }
    }

    /// La longueur du retard pour une phase donnée.
    ///
    /// Un cosinus plutôt qu'une dent de scie : le balayage doit ralentir à ses
    /// extrémités et repartir dans l'autre sens. Une dent de scie reviendrait
    /// d'un coup à son point de départ, et ce saut s'entend comme un clic.
    fn delay_at(&self, phase: f32) -> f32 {
        let sweep = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
        self.min_delay + (self.max_delay - self.min_delay) * sweep
    }

    /// Le signal mouillé, déjà dosé pour être sommé au bus master.
    pub(crate) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let left_delay = self.delay_at(self.left.phase);
        let right_delay = self.delay_at(self.right.phase);
        let wet_left = self.left.process(left, left_delay) * RETURN_GAIN;
        let wet_right = self.right.process(right, right_delay) * RETURN_GAIN;

        // Les phases avancent ensemble et restent dans un tour : les laisser
        // croître indéfiniment ferait perdre au cosinus sa précision au bout de
        // quelques heures de lecture.
        self.left.phase = (self.left.phase + self.step).fract();
        self.right.phase = (self.right.phase + self.step).fract();
        (wet_left, wet_right)
    }

    /// Vide les lignes et remet les oscillateurs à leur départ.
    ///
    /// À appeler sur un Seek, comme la reverb : la copie de l'endroit qu'on
    /// quitte n'a rien à faire à l'endroit où l'on arrive. Remettre la phase à
    /// zéro rend aussi la lecture **reproductible** — repartir du même endroit
    /// doit donner le même balayage, sans quoi un bounce ne ressemblerait pas à
    /// ce qu'on vient d'écouter.
    pub(crate) fn reset(&mut self) {
        self.left.reset(0.0);
        self.right.reset(STEREO_PHASE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    #[test]
    fn silence_stays_silent() {
        let mut flanger = Flanger::new(RATE);
        for _ in 0..RATE {
            let (left, right) = flanger.process(0.0, 0.0);
            assert!(left.abs() < 1.0e-6 && right.abs() < 1.0e-6);
        }
    }

    /// Le rebouclage est ce qui peut faire diverger l'effet, et cela ne
    /// s'entendrait qu'au bout de plusieurs secondes de musique soutenue.
    #[test]
    fn a_loud_sustained_input_never_blows_up() {
        let mut flanger = Flanger::new(RATE);
        let mut peak: f32 = 0.0;
        for frame in 0..(RATE * 4) {
            let input = if frame % 2 == 0 { 1.0 } else { -1.0 };
            let (left, right) = flanger.process(input, input);
            assert!(left.is_finite() && right.is_finite());
            peak = peak.max(left.abs()).max(right.abs());
        }
        assert!(peak < 6.0, "le rebouclage diverge : crête à {peak}");
    }

    /// Un flanger doit rendre son entrée à un niveau comparable, faute de quoi
    /// il ne peut pas former de creux en se sommant au sec.
    #[test]
    fn the_return_comes_back_near_the_level_it_was_fed() {
        let mut flanger = Flanger::new(RATE);
        let mut peak: f32 = 0.0;
        for frame in 0..RATE {
            // Une sinusoïde à 440 Hz, largement dans la bande où le peigne
            // travaille.
            let input = (frame as f32 * std::f32::consts::TAU * 440.0 / RATE as f32).sin() * 0.5;
            let (left, right) = flanger.process(input, input);
            if frame > RATE / 2 {
                peak = peak.max(left.abs()).max(right.abs());
            }
        }
        assert!(
            peak > 0.25,
            "un retour trop faible ne creuse rien : crête à {peak}"
        );
    }

    /// C'est le balayage qui fait l'effet : sans mouvement du retard, il ne
    /// reste qu'une coloration fixe.
    #[test]
    fn the_delay_sweeps_between_its_two_bounds() {
        let flanger = Flanger::new(RATE);
        let trough = flanger.delay_at(0.0);
        let crest = flanger.delay_at(0.5);
        assert!((trough - flanger.min_delay).abs() < 1.0e-3);
        assert!((crest - flanger.max_delay).abs() < 1.0e-3);
        // Et il revient sur ses pas plutôt que de sauter.
        assert!((flanger.delay_at(1.0) - trough).abs() < 1.0e-3);
    }

    /// Sans décalage entre les deux oscillateurs, l'effet reste collé au
    /// centre de l'image.
    #[test]
    fn the_two_channels_do_not_sweep_together() {
        let mut flanger = Flanger::new(RATE);
        let mut difference = 0.0_f32;
        for frame in 0..RATE {
            let input = (frame as f32 * std::f32::consts::TAU * 330.0 / RATE as f32).sin() * 0.5;
            let (left, right) = flanger.process(input, input);
            difference += (left - right).abs();
        }
        assert!(difference > 1.0, "les deux canaux balaient ensemble");
    }

    /// Repartir du même endroit doit donner le même balayage : sans cela un
    /// bounce ne ressemblerait pas à ce qu'on vient d'écouter.
    #[test]
    fn a_reset_makes_the_sweep_repeatable() {
        let run = || {
            let mut flanger = Flanger::new(RATE);
            flanger.reset();
            let mut tail = Vec::new();
            for frame in 0..2_000 {
                let input = if frame == 0 { 1.0 } else { 0.0 };
                tail.push(flanger.process(input, input));
            }
            tail
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_reset_empties_the_line() {
        let mut flanger = Flanger::new(RATE);
        for frame in 0..1_000 {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            flanger.process(input, input);
        }
        flanger.reset();
        let (left, right) = flanger.process(0.0, 0.0);
        assert_eq!((left, right), (0.0, 0.0));
    }

    /// La fenêtre de retard est décrite en millisecondes : elle doit valoir la
    /// même durée à toute fréquence d'échantillonnage.
    #[test]
    fn the_sweep_lasts_the_same_time_at_another_sample_rate() {
        for rate in [44_100_u32, 48_000] {
            let flanger = Flanger::new(rate);
            let min_ms = flanger.min_delay / rate as f32 * 1000.0;
            let max_ms = flanger.max_delay / rate as f32 * 1000.0;
            assert!(
                (min_ms - DELAY_MIN_MS).abs() < 0.01,
                "{rate} Hz : {min_ms} ms"
            );
            assert!(
                (max_ms - DELAY_MAX_MS).abs() < 0.01,
                "{rate} Hz : {max_ms} ms"
            );
        }
    }
}
