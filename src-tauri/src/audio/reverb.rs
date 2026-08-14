//! La pièce dans laquelle les pistes peuvent être envoyées.
//!
//! Un réseau de retards rebouclés — huit filtres en peigne en parallèle, puis
//! quatre passe-tout en série, par canal. C'est la structure de Freeverb, et
//! elle est retenue pour une raison simple : elle coûte vingt-quatre lectures
//! de ligne à retard par échantillon, là où une convolution en coûterait des
//! milliers. Le fil audio porte déjà la recherche WSOLA de chaque clip; ce qui
//! s'y ajoute doit être modeste.
//!
//! **Ce module ne rend que le signal mouillé.** Le signal sec ne passe jamais
//! par ici : les envois sont pris sur chaque piste et le retour se somme au bus
//! master. Une reverb en insert couperait sa propre queue au relâchement du
//! bouton, ce qui est précisément le défaut qu'un départ évite.

/// Les longueurs de Freeverb, en échantillons à 44,1 kHz.
///
/// Elles sont premières entre elles, ce qui empêche leurs échos de retomber
/// ensemble et de sonner comme un peigne plutôt que comme une pièce.
const COMB_LENGTHS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_LENGTHS: [usize; 4] = [556, 441, 341, 225];

/// Le décalage du canal droit, qui décorrèle les deux oreilles.
///
/// Sans lui les deux canaux sont identiques et la pièce s'écroule au centre.
/// Plus large que les vingt-trois échantillons de Freeverb : celui-ci sonnait
/// comme une petite salle, et l'écartement est ce qui donne de l'air avant même
/// que la queue ne s'allonge.
const STEREO_SPREAD: usize = 41;

/// La fréquence pour laquelle les longueurs ci-dessus ont été choisies.
const TUNING_SAMPLE_RATE: f32 = 44_100.0;

/// Le gain de rebouclage des passe-tout, fixe chez Freeverb comme ici : il
/// disperse les échos dans le temps sans colorer.
const ALLPASS_FEEDBACK: f32 = 0.5;

/// Une poussière ajoutée dans les boucles de rebouclage.
///
/// Une queue de reverb tend vers zéro sans jamais l'atteindre, et les nombres
/// dénormaux qu'elle traverse en chemin coûtent, sur x86, des dizaines de fois
/// le prix d'un calcul normal — un ralentissement qui apparaît **après** que le
/// son a cessé, ce qui est la pire façon de le découvrir.
const DENORMAL_GUARD: f32 = 1.0e-18;

/// L'absorption des aigus dans la boucle.
///
/// C'est elle qui décide si la queue est claire ou sourde. Descendue de douze
/// à quatre centièmes : la pièce manquait encore d'air, les aigus étant mangés
/// au bout de quelques réflexions alors qu'une grande salle claire les garde
/// bien plus longtemps. Ce qui reste absorbe juste assez pour que la queue ne
/// siffle pas.
const REVERB_DAMPING: f32 = 0.05;

/// La fréquence au-dessus de laquelle le retour est relevé, en hertz.
///
/// Baisser l'absorption allonge la vie des aigus dans la queue, mais ne crée
/// rien qui n'y soit déjà. L'impression d'air vient aussi du **haut du
/// spectre du retour lui-même** : une pièce dont on n'entend que le bas semble
/// petite, quelle que soit la longueur de sa queue.
const AIR_HZ: f32 = 3_000.0;

/// De combien le haut du spectre est relevé, en proportion.
///
/// Une demie, soit un peu plus de trois décibels. Assez pour que la pièce
/// respire, pas assez pour qu'un charley y devienne sifflant — un relèvement
/// plus franc rendrait l'effet fatigant sur une passe longue, et une passe
/// longue est précisément ce à quoi cette reverb sert.
const AIR_LIFT: f32 = 0.45;

/// Le sommet du retour, en hertz.
///
/// Le relèvement porterait jusqu'au bout du spectre si rien ne l'arrêtait, et
/// c'est là qu'un réseau rebouclé accumule le plus : sur un signal très aigu
/// soutenu, la crête du retour montait de moitié. Ce plafond la ramène, et il
/// est de toute façon plus juste musicalement — rien d'utile ne vit à vingt
/// kilohertz dans un mix, et l'y relever n'ajoutait que du souffle.
///
/// Douze kilohertz : l'air demandé vit entre trois et dix, et il en ressort
/// intact.
const TOP_HZ: f32 = 12_000.0;

/// Ce que le retour pèse dans le mix, envoi à fond.
///
/// Le réseau rend un signal au niveau de son entrée, si bien qu'un envoi à un
/// pour un noyait complètement le mix — la reverb couvrait la musique au lieu
/// de l'entourer.
///
/// Descendu par deux fois : 0,25, puis 0,17, puis 0,13 — près de dix-huit
/// décibels sous la source. Éclaircir la pièce l'avait rendue **plus présente
/// sans qu'on ait touché à son niveau** : le relèvement des aigus ajoute près
/// de la moitié de gain dans la bande où l'oreille est la plus sensible, si
/// bien qu'à réglage égal on en entendait davantage. Une pièce doit entourer ce
/// qu'elle habille, pas se placer à côté.
const RETURN_GAIN: f32 = 0.13;

/// Le rebouclage des peignes : ce qui décide de la longueur de la queue.
///
/// Trois tailles ont existé, et elles ont été retirées. Elles ne changeaient
/// que la durée, et les deux plus courtes ne servaient pas : on choisit une
/// reverb, on ne la règle pas. Une pièce, généreuse, et rien à décider.
///
/// Éclaircir la pièce a failli coûter cette valeur : **l'amortissement était ce
/// qui bridait le rebouclage dans le haut du spectre**, et le baisser a rendu
/// au réseau une marge qu'il avait perdue. Plutôt que de raccourcir la queue
/// pour compenser, c'est le sommet du spectre du retour qui est borné — voir
/// `TOP_HZ`. La pièce garde donc sa longueur.
const REVERB_FEEDBACK: f32 = 0.95;

/// Un filtre en peigne amorti : l'écho qui revient, un peu plus sourd.
struct Comb {
    buffer: Vec<f32>,
    index: usize,
    /// L'état du passe-bas à un pôle placé dans la boucle.
    damped: f32,
}

impl Comb {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            index: 0,
            damped: 0.0,
        }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let output = self.buffer[self.index];
        self.damped = output * (1.0 - damping) + self.damped * damping + DENORMAL_GUARD;
        self.buffer[self.index] = input + self.damped * feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.damped = 0.0;
    }
}

/// Un passe-tout : il ne change pas le spectre, il étale les échos.
struct Allpass {
    buffer: Vec<f32>,
    index: usize,
}

impl Allpass {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(1)],
            index: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.index];
        self.buffer[self.index] = input + buffered * ALLPASS_FEEDBACK + DENORMAL_GUARD;
        self.index = (self.index + 1) % self.buffer.len();
        buffered - input
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
    }
}

/// Le relèvement des aigus posé sur le retour.
///
/// Un passe-bas à un pôle sépare le bas du haut, et le haut est rendu une fois
/// et demie. C'est un filtre en plateau écrit en trois opérations, ce qui est
/// tout ce qu'un fil audio peut se permettre en plus de vingt-quatre lectures
/// de ligne à retard par échantillon.
struct AirShelf {
    /// L'état du passe-bas : ce qui reste du signal une fois le haut retiré.
    low: f32,
    /// Le coefficient du pôle, calculé une fois pour la fréquence de sortie.
    pole: f32,
    /// L'état du plafond posé au-dessus du relèvement.
    top: f32,
    top_pole: f32,
}

impl AirShelf {
    fn new(sample_rate: u32) -> Self {
        let pole_at = |hz: f32| (-std::f32::consts::TAU * hz / sample_rate as f32).exp();
        Self {
            low: 0.0,
            pole: pole_at(AIR_HZ),
            top: 0.0,
            top_pole: pole_at(TOP_HZ),
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        self.low = input * (1.0 - self.pole) + self.low * self.pole;
        let lifted = input + (input - self.low) * AIR_LIFT;
        // Le plafond vient **après** le relèvement, sinon il n'y aurait rien à
        // relever au-dessus de lui.
        self.top = lifted * (1.0 - self.top_pole) + self.top * self.top_pole;
        self.top
    }

    fn reset(&mut self) {
        self.low = 0.0;
        self.top = 0.0;
    }
}

/// Un canal complet : les peignes en parallèle, les passe-tout en série, puis
/// le relèvement des aigus.
struct Channel {
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
    air: AirShelf,
}

impl Channel {
    fn new(sample_rate: u32, offset: usize) -> Self {
        // Les longueurs sont mises à l'échelle de la fréquence réelle : à
        // 48 kHz, les mêmes nombres d'échantillons décriraient une pièce plus
        // petite.
        let scale = |length: usize| {
            ((length + offset) as f32 * sample_rate as f32 / TUNING_SAMPLE_RATE).round() as usize
        };
        Self {
            combs: COMB_LENGTHS.iter().map(|&l| Comb::new(scale(l))).collect(),
            allpasses: ALLPASS_LENGTHS
                .iter()
                .map(|&l| Allpass::new(scale(l)))
                .collect(),
            air: AirShelf::new(sample_rate),
        }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        // Les peignes sont sommés, donc leur somme est divisée par leur nombre
        // pour que la taille de la pièce ne change pas le niveau.
        let mut wet = 0.0;
        for comb in &mut self.combs {
            wet += comb.process(input, feedback, damping);
        }
        wet /= self.combs.len() as f32;

        for allpass in &mut self.allpasses {
            wet = allpass.process(wet);
        }
        // En dernier, sur le mouillé seul : relever avant les peignes ferait
        // remonter le haut à chaque tour de boucle et la queue finirait par
        // siffler.
        self.air.process(wet)
    }

    fn reset(&mut self) {
        for comb in &mut self.combs {
            comb.reset();
        }
        for allpass in &mut self.allpasses {
            allpass.reset();
        }
        self.air.reset();
    }
}

/// Une pièce, partagée par les trois pistes.
///
/// Une seule pour tout le mix, et non une par piste : c'est trois fois moins
/// cher, et musicalement plus juste — trois pistes envoyées dans trois pièces
/// différentes ne sonnent pas comme un même lieu.
pub(crate) struct Reverb {
    left: Channel,
    right: Channel,
}

impl Reverb {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            left: Channel::new(sample_rate, 0),
            right: Channel::new(sample_rate, STEREO_SPREAD),
        }
    }

    /// Le signal mouillé, pour une image de la somme des envois.
    ///
    /// Déjà atténué : l'appelant ajoute ce qu'il reçoit au bus sans avoir à
    /// connaître le bon dosage, et le niveau ne peut pas diverger d'un endroit
    /// à l'autre du programme.
    pub(crate) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        (
            self.left.process(left, REVERB_FEEDBACK, REVERB_DAMPING) * RETURN_GAIN,
            self.right.process(right, REVERB_FEEDBACK, REVERB_DAMPING) * RETURN_GAIN,
        )
    }

    /// Vide la pièce. À appeler sur un Seek : la queue de l'endroit qu'on
    /// quitte n'a rien à faire à l'endroit où l'on arrive.
    pub(crate) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    #[test]
    fn silence_stays_silent() {
        let mut reverb = Reverb::new(RATE);
        for _ in 0..RATE {
            let (left, right) = reverb.process(0.0, 0.0);
            assert!(left.abs() < 1.0e-6 && right.abs() < 1.0e-6);
        }
    }

    #[test]
    fn a_tail_decays_instead_of_ringing_forever() {
        // Un réseau rebouclé mal réglé oscille au lieu de s'éteindre, et cela
        // ne s'entend qu'une fois la musique arrêtée.
        let mut reverb = Reverb::new(RATE);
        let mut early = 0.0_f32;
        let mut peak: f32 = 0.0;
        for frame in 0..(RATE * 4) {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let (left, right) = reverb.process(input, input);
            if frame < RATE / 4 {
                early += left * left + right * right;
            }
            if frame > RATE * 3 {
                peak = peak.max(left.abs()).max(right.abs());
            }
        }
        assert!(early > 0.0, "une impulsion doit produire quelque chose");
        assert!(
            peak < 1.0e-3,
            "la queue sonne encore à quatre secondes : {peak}"
        );
    }

    #[test]
    fn the_two_channels_do_not_collapse_to_the_middle() {
        let mut reverb = Reverb::new(RATE);
        let mut difference = 0.0_f32;
        for frame in 0..RATE {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let (left, right) = reverb.process(input, input);
            difference += (left - right).abs();
        }
        assert!(difference > 0.01, "sans décalage, la pièce est mono");
    }

    #[test]
    fn a_loud_sustained_input_never_blows_up() {
        let mut reverb = Reverb::new(RATE);
        let mut peak: f32 = 0.0;
        for frame in 0..(RATE * 2) {
            // Plus dur que tout ce qu'un envoi peut porter en pratique.
            let input = if frame % 2 == 0 { 1.0 } else { -1.0 };
            let (left, right) = reverb.process(input, input);
            assert!(left.is_finite() && right.is_finite());
            peak = peak.max(left.abs()).max(right.abs());
        }
        assert!(peak < 4.0, "le réseau diverge : crête à {peak}");
    }

    /// Le retour est déjà atténué à la sortie du module : c'est ce qui empêche
    /// la reverb de noyer le mix, et l'appelant n'a pas à le savoir.
    #[test]
    fn the_return_stays_well_under_what_it_is_fed() {
        let mut reverb = Reverb::new(RATE);
        let mut peak: f32 = 0.0;
        for frame in 0..RATE {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let (left, right) = reverb.process(input, input);
            peak = peak.max(left.abs()).max(right.abs());
        }
        assert!(peak < 0.5, "le retour est trop fort : {peak}");
    }

    #[test]
    fn a_reset_empties_the_room() {
        let mut reverb = Reverb::new(RATE);
        for frame in 0..1_000 {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            reverb.process(input, input);
        }
        reverb.reset();
        let (left, right) = reverb.process(0.0, 0.0);
        assert_eq!((left, right), (0.0, 0.0));
    }

    /// La mise à l'échelle doit décrire la **même** pièce à toute fréquence.
    #[test]
    fn the_room_keeps_its_size_at_another_sample_rate() {
        let seconds_to_quiet = |rate: u32| {
            let mut reverb = Reverb::new(rate);
            let mut last_loud = 0;
            for frame in 0..(rate * 4) {
                let input = if frame == 0 { 1.0 } else { 0.0 };
                let (left, right) = reverb.process(input, input);
                if left.abs().max(right.abs()) > 1.0e-4 {
                    last_loud = frame;
                }
            }
            last_loud as f32 / rate as f32
        };

        let at_44 = seconds_to_quiet(44_100);
        let at_48 = seconds_to_quiet(48_000);
        // Comparé **en proportion** et non en secondes. Les longueurs de peigne
        // sont arrondies à l'échantillon près, si bien qu'à 48 kHz elles ne
        // sont plus tout à fait premières entre elles et la queue s'éteint un
        // peu différemment. Cet écart est relatif à la durée de la queue : une
        // tolérance en secondes se serait révélée trop étroite dès qu'on
        // allonge la pièce, ce qui n'est pas ce qu'on cherche à vérifier ici.
        let drift = (at_44 - at_48).abs() / at_44.max(at_48);
        assert!(
            drift < 0.12,
            "la pièce change de taille avec la fréquence : {at_44} s contre {at_48} s"
        );
    }
}
