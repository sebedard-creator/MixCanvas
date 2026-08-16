//! Le limiteur de mastering du bounce.
//!
//! Il n'a rien à voir avec celui du transport, et c'est délibéré. Celui du
//! moteur est un **garde-fou** : il protège l'écoute d'un dépassement, avec
//! deux millisecondes d'attaque et un écrêtage franc au bout. Celui-ci est un
//! **outil** : il monte le niveau du mix et garantit un plafond, ce qui n'est
//! pas le même métier et ne se règle pas de la même façon.
//!
//! Il tourne hors ligne, ce qui change tout : la latence ne coûte rien. Un
//! limiteur temps réel doit décider du gain à l'instant où l'échantillon
//! arrive, donc il réagit toujours trop tard et rattrape en écrêtant. Ici,
//! l'échantillon est retenu quelques millisecondes avant d'être écrit, si bien
//! que le gain est **déjà** descendu quand la crête se présente. Le
//! dépassement devient impossible plutôt qu'improbable.

/// Le programme sort en stéréo; la borne laisse de la marge sans ouvrir la
/// porte à une allocation par image.
const MAX_CHANNELS: usize = 8;

/// De combien le limiteur voit venir, en millisecondes.
///
/// C'est aussi son retard, et donc ce qu'il faut vider en fin de rendu. Trois
/// millisecondes suffisent à laisser la baisse de gain s'étaler sans qu'on
/// l'entende, et restent bien en deçà de ce qui décalerait un transitoire à
/// l'oreille.
const LOOKAHEAD_MS: f32 = 3.0;

/// Les bornes du relâchement automatique.
///
/// Court sur une pointe isolée, pour que le gain revienne avant qu'on
/// n'entende le creux. Long sur un passage dense, pour qu'il cesse de pomper
/// au rythme de la grosse caisse.
const AUTO_RELEASE_FAST_MS: f32 = 40.0;
const AUTO_RELEASE_SLOW_MS: f32 = 900.0;

/// La fenêtre sur laquelle se juge « dense ou isolé ».
const AUTO_RELEASE_SENSE_MS: f32 = 300.0;

/// Le réglage du limiteur, tel que l'interface le pose.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasteringSettings {
    /// Le seuil, en décibels sous la pleine échelle. Toujours négatif.
    pub threshold_db: f32,
    /// Le plafond de sortie, en décibels sous la pleine échelle.
    pub ceiling_db: f32,
    /// Le relâchement en millisecondes, ignoré quand l'automatique est actif.
    pub release_ms: f32,
    pub auto_release: bool,
}

impl Default for MasteringSettings {
    fn default() -> Self {
        Self {
            threshold_db: -3.7,
            ceiling_db: -0.1,
            release_ms: 1.0,
            auto_release: true,
        }
    }
}

fn from_decibels(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn one_pole(milliseconds: f32, sample_rate: u32) -> f32 {
    let seconds = (milliseconds.max(0.01)) / 1_000.0;
    (-1.0 / (sample_rate as f32 * seconds)).exp()
}

/// Le minimum glissant d'une fenêtre, en temps constant.
///
/// Le gain à appliquer est celui que réclame la crête **la plus forte à
/// venir** dans la fenêtre d'anticipation. Le relire à chaque image coûterait
/// une centaine de comparaisons par échantillon; cette file ne garde que les
/// candidats encore capables de devenir le minimum, et chacun n'y entre et
/// n'en sort qu'une fois.
struct SlidingMinimum {
    /// Les candidats, par valeur croissante : le premier est le minimum.
    values: std::collections::VecDeque<(usize, f32)>,
    window: usize,
    index: usize,
}

impl SlidingMinimum {
    fn new(window: usize) -> Self {
        Self {
            values: std::collections::VecDeque::with_capacity(window.max(1)),
            window: window.max(1),
            index: 0,
        }
    }

    fn push(&mut self, value: f32) {
        while self.values.back().is_some_and(|&(_, held)| held >= value) {
            self.values.pop_back();
        }
        self.values.push_back((self.index, value));
        // Ce qui est sorti de la fenêtre ne peut plus rien réclamer.
        while self
            .values
            .front()
            .is_some_and(|&(at, _)| at + self.window <= self.index)
        {
            self.values.pop_front();
        }
        self.index += 1;
    }

    fn minimum(&self) -> f32 {
        self.values.front().map_or(1.0, |&(_, value)| value)
    }
}

/// Le limiteur, sur un flux entrelacé.
pub struct MasteringLimiter {
    channels: usize,
    /// La ligne à retard, entrelacée comme le flux.
    delay: Vec<f32>,
    write: usize,
    lookahead_frames: usize,
    required: SlidingMinimum,
    makeup: f32,
    ceiling: f32,
    gain: f32,
    release_coefficient: f32,
    auto_release: bool,
    /// La profondeur de réduction moyenne, qui décide de la vitesse de retour.
    sensed_depth: f32,
    sense_coefficient: f32,
    fast_coefficient: f32,
    slow_coefficient: f32,
    /// Combien d'images restent dans la ligne à retard.
    pending: usize,
}

impl MasteringLimiter {
    pub fn new(settings: MasteringSettings, sample_rate: u32, channels: usize) -> Self {
        let channels = channels.clamp(1, MAX_CHANNELS);
        let lookahead_frames =
            ((LOOKAHEAD_MS / 1_000.0) * sample_rate as f32).round().max(1.0) as usize;
        let ceiling = from_decibels(settings.ceiling_db.min(0.0));
        // Le seuil **remonte** le niveau jusqu'au plafond, comme sur les
        // limiteurs de mastering dont il porte le nom : descendre le seuil de
        // 3,7 dB sous un plafond de 0,1 revient à demander 3,6 dB de gain. Un
        // seuil qui ne ferait que déclencher la limitation donnerait un bouton
        // dont on ne s'expliquerait pas qu'il ne change rien.
        let makeup = from_decibels((settings.ceiling_db - settings.threshold_db).max(0.0));
        Self {
            channels,
            delay: vec![0.0; lookahead_frames * channels],
            write: 0,
            lookahead_frames,
            /* `+ 1`, et il compte. L'image qui sort à l'instant `n` est
               celle entrée en `n − L`; une fenêtre de `L` ne couvrirait que
               `n − L + 1 .. n`, c'est-à-dire tout ce qui la suit **sauf
               elle-même**. Une crête isolée suivie de silence échappait ainsi
               à sa propre exigence et sortait deux millièmes de décibel
               au-dessus du plafond. */
            required: SlidingMinimum::new(lookahead_frames + 1),
            makeup,
            ceiling,
            gain: 1.0,
            release_coefficient: one_pole(settings.release_ms, sample_rate),
            auto_release: settings.auto_release,
            sensed_depth: 0.0,
            sense_coefficient: one_pole(AUTO_RELEASE_SENSE_MS, sample_rate),
            fast_coefficient: one_pole(AUTO_RELEASE_FAST_MS, sample_rate),
            slow_coefficient: one_pole(AUTO_RELEASE_SLOW_MS, sample_rate),
            pending: 0,
        }
    }

    /// Passe une image et rend celle qui sort, ou `None` tant que la ligne à
    /// retard n'est pas pleine.
    ///
    /// `frame` est écrit en place par la sortie quand il y en a une.
    pub fn process(&mut self, frame: &mut [f32]) -> bool {
        debug_assert_eq!(frame.len(), self.channels);

        let mut peak = 0.0_f32;
        for sample in frame.iter_mut() {
            *sample *= self.makeup;
            peak = peak.max(sample.abs());
        }
        // Ce que cette image réclamera comme gain quand elle sortira.
        let required = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        self.required.push(required);

        // Sur la pile, et non dans un `Vec` : cette fonction est appelée une
        // fois par image, soit cent millions de fois sur un mix de quarante
        // minutes. Une allocation par appel s'y verrait.
        let mut outgoing = [0.0_f32; MAX_CHANNELS];
        let base = self.write * self.channels;
        outgoing[..self.channels].copy_from_slice(&self.delay[base..base + self.channels]);
        self.delay[base..base + self.channels].copy_from_slice(frame);
        self.write = (self.write + 1) % self.lookahead_frames;

        let target = self.required.minimum();
        self.advance_gain(target);

        if self.pending < self.lookahead_frames {
            self.pending += 1;
            return false;
        }
        for (slot, value) in frame.iter_mut().zip(outgoing) {
            *slot = value * self.gain;
        }
        true
    }

    /// Le gain descend d'un coup et remonte doucement.
    ///
    /// La descente n'a pas besoin d'être lissée : elle a lieu au plus tard
    /// une fenêtre d'anticipation **avant** la crête, donc sur du signal plus
    /// faible, là où une marche de gain ne s'entend pas. C'est exactement ce
    /// que le look-ahead achète, et c'est aussi ce qui rend le dépassement
    /// impossible : le gain courant ne dépasse jamais le minimum réclamé par
    /// ce qui vient.
    fn advance_gain(&mut self, target: f32) {
        let depth = 1.0 - target;
        self.sensed_depth =
            depth + self.sense_coefficient * (self.sensed_depth - depth);

        if target < self.gain {
            self.gain = target;
            return;
        }
        let coefficient = if self.auto_release {
            let blend = self.sensed_depth.clamp(0.0, 1.0);
            self.fast_coefficient + (self.slow_coefficient - self.fast_coefficient) * blend
        } else {
            self.release_coefficient
        };
        self.gain = target + coefficient * (self.gain - target);
    }

    /// Vide la ligne à retard en fin de rendu.
    ///
    /// Sans cela le bounce perdrait ses trois dernières millisecondes — une
    /// queue de reverb coupée net, et personne pour dire pourquoi.
    pub fn flush(&mut self) -> Vec<f32> {
        let mut tail = Vec::with_capacity(self.pending * self.channels);
        let mut silence = vec![0.0; self.channels];
        for _ in 0..self.lookahead_frames {
            silence.iter_mut().for_each(|value| *value = 0.0);
            if self.process(&mut silence) {
                tail.extend_from_slice(&silence);
            }
        }
        tail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn run(settings: MasteringSettings, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let mut limiter = MasteringLimiter::new(settings, RATE, 2);
        let mut out = Vec::with_capacity(input.len());
        for frame in input {
            let mut work = *frame;
            if limiter.process(&mut work) {
                out.push(work);
            }
        }
        for chunk in limiter.flush().chunks_exact(2) {
            out.push([chunk[0], chunk[1]]);
        }
        out
    }

    /// La promesse du limiteur, et la seule qui ne se négocie pas.
    #[test]
    fn nothing_ever_comes_out_above_the_ceiling() {
        let settings = MasteringSettings::default();
        let ceiling = from_decibels(settings.ceiling_db);

        // Du silence, puis une salve pleine échelle sans le moindre fondu :
        // c'est le cas qu'un limiteur sans anticipation rate toujours.
        let mut input = vec![[0.0, 0.0]; 2_000];
        input.extend(std::iter::repeat_n([1.0, -1.0], 500));
        input.extend(std::iter::repeat_n([0.05, 0.05], 4_000));
        input.extend(std::iter::repeat_n([0.9, 0.9], 2_000));

        for frame in run(settings, &input) {
            for sample in frame {
                assert!(
                    sample.abs() <= ceiling + 1.0e-6,
                    "{sample} dépasse le plafond {ceiling}"
                );
            }
        }
    }

    /// Le seuil remonte le niveau : c'est ce qu'on attend du bouton.
    #[test]
    fn a_quiet_mix_comes_back_louder_by_the_threshold() {
        let settings = MasteringSettings {
            threshold_db: -3.7,
            ceiling_db: -0.1,
            ..MasteringSettings::default()
        };
        // Bien en dessous du seuil : rien à limiter, donc le gain seul agit.
        let input = vec![[0.02, 0.02]; 8_000];
        let output = run(settings, &input);

        let expected = 0.02 * from_decibels(-0.1 - -3.7);
        let settled = output[output.len() / 2][0];
        assert!(
            (settled - expected).abs() < 1.0e-4,
            "attendu {expected}, obtenu {settled}"
        );
    }

    /// Le rendu ne doit pas perdre sa fin.
    ///
    /// Le limiteur retient trois millisecondes; sans vidange, une queue de
    /// reverb serait coupée net à la fin du bounce.
    #[test]
    fn the_render_keeps_every_frame_it_was_given() {
        let settings = MasteringSettings::default();
        let input = vec![[0.1, 0.1]; 5_000];
        assert_eq!(run(settings, &input).len(), input.len());
    }

    /// Le relâchement automatique doit vraiment distinguer les deux cas.
    #[test]
    fn a_lone_peak_recovers_faster_than_a_dense_passage() {
        let settings = MasteringSettings::default();
        let quiet = || std::iter::repeat_n([0.1_f32, 0.1], 20_000);

        let mut lone = vec![[0.1, 0.1]; 1_000];
        lone.extend(std::iter::repeat_n([1.0, 1.0], 50));
        lone.extend(quiet());

        let mut dense = vec![[0.1, 0.1]; 1_000];
        dense.extend(std::iter::repeat_n([1.0, 1.0], 20_000));
        dense.extend(quiet());

        // Combien d'images après la fin de la partie forte avant que le niveau
        // ne soit revenu à ce que le seul gain de compensation donnerait.
        let recovery = |frames: Vec<[f32; 2]>, after: usize| {
            let settled = 0.1 * from_decibels(-0.1 - -3.7);
            frames
                .iter()
                .skip(after)
                .position(|frame| frame[0] >= settled * 0.99)
                .unwrap_or(usize::MAX)
        };

        let lone_recovery = recovery(run(settings, &lone), 1_050);
        let dense_recovery = recovery(run(settings, &dense), 21_000);
        assert!(
            lone_recovery < dense_recovery,
            "pointe isolée : {lone_recovery} images, passage dense : {dense_recovery}"
        );
    }
}
