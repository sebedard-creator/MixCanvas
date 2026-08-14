//! Le delay : l'écho qui rebondit d'une oreille à l'autre, au tempo.
//!
//! **Sa longueur est musicale, pas fixe.** Un delay réglé en millisecondes se
//! décale dès que le tempo bouge; celui-ci reçoit sa longueur en échantillons à
//! chaque image, calculée depuis la carte de tempo du projet, et suit donc les
//! rampes de BPM sans qu'on ait à y penser. C'est ce que le programme sait
//! faire et qu'un plugin ordinaire ne peut pas.
//!
//! **Un vrai ping-pong** : l'envoi n'entre que dans la ligne gauche, sommé en
//! mono. La première répétition sort donc à gauche, la deuxième à droite, la
//! troisième à gauche — l'écho traverse la tête. Nourrir les deux lignes, comme
//! c'était le cas d'abord, faisait sortir chaque répétition des deux côtés à la
//! fois : il n'y avait plus d'alternance à entendre, et l'effet ressemblait à
//! une nappe de reverb plutôt qu'à un écho.
//!
//! **Ce module ne rend que le signal mouillé**, comme la reverb et le flanger.
//! L'envoi est pris sur chaque piste, le retour se somme au bus master. C'est
//! ce qui permet au geste le plus reconnaissable du métier : tenir le delay,
//! couper la piste, et laisser l'écho porter la transition. En insert, couper
//! la piste couperait aussi l'écho.

/// Le retard, en fraction de temps : une croche pointée.
///
/// Trois doubles-croches. Elle croise le rythme au lieu de le doubler, si bien
/// qu'on l'entend distinctement du morceau sans qu'elle l'encombre — c'est la
/// division du house et de la techno pour cette raison précise.
pub(crate) const DELAY_BEATS: f32 = 0.75;

/// La longueur maximale de la ligne, en secondes.
///
/// Le tempo descend jusqu'à quarante, où un temps dure une seconde et demie et
/// la croche pointée un peu plus d'une seconde. Deux secondes couvrent ce cas
/// avec de la marge, et une ligne trop longue ne coûte que sa mémoire.
const MAX_DELAY_SECONDS: f32 = 2.0;

/// Ce que chaque répétition réinjecte.
///
/// Monté par deux fois : 0,55, puis 0,62, puis 0,72. À 0,55 il ne restait
/// guère que deux répétitions franches; à 0,62 quatre, et la traîne s'arrêtait
/// encore trop net pour porter une transition — c'est pourtant le geste pour
/// lequel cet effet existe. À 0,72 on en compte sept, chacune un peu plus
/// faible que la précédente, si bien que la fin de l'écho **s'efface** au lieu
/// de s'arrêter. Plus haut, les échos s'empilent et le mix se brouille.
const FEEDBACK: f32 = 0.72;

/// L'absorption des aigus à chaque tour.
///
/// Un vrai delay assombrit ses répétitions, et c'est ce qui les fait passer
/// derrière la musique au lieu de se battre avec elle. Sans cela, la quatrième
/// répétition d'un charley est aussi brillante que la première et l'oreille ne
/// sait plus laquelle suivre.
///
/// Allégée d'un quart à un huitième : à un quart, les répétitions perdaient si
/// vite leur transitoire qu'elles se fondaient en une nappe — on entendait une
/// pièce, pas un écho. Un delay se reconnaît à ce que chaque répétition reste
/// **un événement**, et un événement a besoin de son attaque.
const DAMPING: f32 = 0.12;

/// Ce que le retour pèse dans le mix, envoi à fond.
///
/// Les répétitions doivent rester sous la source — un écho qui passe devant ce
/// qu'il répète n'est plus un écho — mais pas de beaucoup.
///
/// Monté par deux fois : 0,45, puis 0,7, puis 0,85. À 0,45 le delay était
/// **inaudible dans un mix**, et le diagnostic a été long parce que rien
/// n'était cassé — le module rendait bien son signal, il était simplement trop
/// discret. Trois choses s'additionnaient : sept décibels sous la source, une
/// seule oreille à la fois depuis le ping-pong, et des répétitions assombries.
/// Contre une reverb qui remplit tout et un flanger qui bouge, il ne restait
/// rien à remarquer. À 0,7 il s'entendait; à 0,85 il porte.
///
/// C'est le retour le plus haut des trois, et c'est cohérent : une répétition
/// est un **événement isolé**, pas une nappe. Elle n'a qu'un instant pour se
/// faire entendre, là où une queue de reverb dispose de plusieurs secondes.
const RETURN_GAIN: f32 = 0.85;

/// Une poussière dans la boucle, contre les nombres dénormaux — même raison
/// que dans la reverb : ils coûtent cher, et seulement **après** que le son a
/// cessé.
const DENORMAL_GUARD: f32 = 1.0e-18;

/// Une ligne à retard, lue à une distance fractionnaire.
///
/// Fractionnaire parce que la longueur suit le tempo : à 128 BPM une croche
/// pointée fait 15 501,5 échantillons, et arrondir ferait dériver l'écho du
/// temps sur une longue passe. Quand le tempo rampe, la distance de lecture
/// glisse, et les répétitions changent légèrement de hauteur — exactement ce
/// que fait une bande dont on change la vitesse.
struct Line {
    buffer: Vec<f32>,
    write: usize,
    /// L'état du passe-bas placé dans la boucle.
    damped: f32,
}

impl Line {
    fn new(length: usize) -> Self {
        Self {
            buffer: vec![0.0; length.max(2)],
            write: 0,
            damped: 0.0,
        }
    }

    fn read(&self, delay: f32) -> f32 {
        let length = self.buffer.len();
        let clamped = delay.clamp(1.0, (length - 2) as f32);
        let whole = clamped.floor();
        let fraction = clamped - whole;
        let first = (self.write + length - whole as usize) % length;
        let second = (first + length - 1) % length;
        self.buffer[first] * (1.0 - fraction) + self.buffer[second] * fraction
    }

    fn push(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write = (self.write + 1) % self.buffer.len();
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.damped = 0.0;
    }
}

/// Le delay partagé par les trois pistes.
///
/// Un seul pour tout le mix, comme la pièce et le peigne : trois pistes
/// renvoyées par trois échos désynchronisés ne sonneraient pas comme un même
/// effet.
pub(crate) struct Delay {
    left: Line,
    right: Line,
}

impl Delay {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let length = (MAX_DELAY_SECONDS * sample_rate as f32) as usize + 4;
        Self {
            left: Line::new(length),
            right: Line::new(length),
        }
    }

    /// Le signal mouillé, pour un retard donné **en échantillons**.
    ///
    /// L'appelant fournit la longueur parce que c'est lui qui connaît le tempo
    /// à cette image; ce module ne sait rien de la carte de tempo et n'a pas à
    /// la connaître.
    ///
    /// L'envoi entre dans la ligne gauche seule, et les deux lignes se
    /// nourrissent ensuite **en croix** : ce qui sort à gauche revient à droite
    /// et inversement. L'écho traverse donc la tête d'une répétition à l'autre,
    /// ce qui lui laisse la place de se faire entendre sans couvrir le centre
    /// du mix.
    pub(crate) fn process(&mut self, left: f32, right: f32, delay_samples: f32) -> (f32, f32) {
        let out_left = self.left.read(delay_samples);
        let out_right = self.right.read(delay_samples);

        self.left.damped = out_right * (1.0 - DAMPING) + self.left.damped * DAMPING;
        self.right.damped = out_left * (1.0 - DAMPING) + self.right.damped * DAMPING;

        // **L'entrée ne va que dans la ligne gauche.** C'était là le défaut :
        // nourrir les deux lignes faisait sortir la première répétition dans
        // les deux oreilles en même temps, et le rebouclage croisé ne faisait
        // ensuite qu'épaissir ce qui était déjà partout. On entendait une
        // nappe — une reverb — au lieu d'un écho qui rebondit.
        //
        // Sommée en mono, parce qu'un ping-pong alterne les oreilles : garder
        // le canal droit à part le ferait entrer directement à droite, ce qui
        // casserait l'alternance dès le premier tour.
        let feed = (left + right) * 0.5;
        self.left
            .push(feed + self.left.damped * FEEDBACK + DENORMAL_GUARD);
        self.right
            .push(self.right.damped * FEEDBACK + DENORMAL_GUARD);

        (out_left * RETURN_GAIN, out_right * RETURN_GAIN)
    }

    /// Vide les lignes. À appeler sur un Seek, comme les autres : l'écho de
    /// l'endroit qu'on quitte n'a rien à faire à l'endroit où l'on arrive.
    pub(crate) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// Le retard d'une croche pointée à ce tempo, en échantillons.
    fn samples_at(bpm: f32) -> f32 {
        60.0 / bpm * DELAY_BEATS * RATE as f32
    }

    #[test]
    fn silence_stays_silent() {
        let mut delay = Delay::new(RATE);
        let step = samples_at(128.0);
        for _ in 0..RATE {
            let (left, right) = delay.process(0.0, 0.0, step);
            assert!(left.abs() < 1.0e-6 && right.abs() < 1.0e-6);
        }
    }

    /// Une impulsion doit ressortir **au bon moment**, et pas ailleurs : c'est
    /// tout l'intérêt d'un delay calé sur le tempo.
    #[test]
    fn the_first_repeat_lands_one_dotted_eighth_later() {
        let mut delay = Delay::new(RATE);
        let step = samples_at(128.0);
        let mut loudest = 0;
        let mut peak = 0.0_f32;
        for frame in 0..(RATE / 2) {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let (left, _) = delay.process(input, 0.0, step);
            if left.abs() > peak {
                peak = left.abs();
                loudest = frame;
            }
        }
        let expected = step.round() as u32;
        assert!(
            loudest.abs_diff(expected) <= 2,
            "premier écho à {loudest} images pour {expected} attendues"
        );
    }

    /// Le tempo décide de la longueur : deux fois plus lent, deux fois plus
    /// tard. Sans cela le delay ne serait pas « en fonction du tempo ».
    #[test]
    fn a_slower_tempo_puts_the_repeat_further_out() {
        let first_repeat = |bpm: f32| {
            let mut delay = Delay::new(RATE);
            let step = samples_at(bpm);
            let mut loudest = 0;
            let mut peak = 0.0_f32;
            for frame in 0..(RATE * 2) {
                let input = if frame == 0 { 1.0 } else { 0.0 };
                let (left, _) = delay.process(input, 0.0, step);
                if left.abs() > peak {
                    peak = left.abs();
                    loudest = frame;
                }
            }
            loudest
        };
        let fast = first_repeat(128.0);
        let slow = first_repeat(64.0);
        assert!(
            slow.abs_diff(fast * 2) <= 4,
            "à moitié tempo l'écho devrait tomber deux fois plus loin : {fast} puis {slow}"
        );
    }

    /// Le ping-pong, et c'est **le** défaut que ce test doit attraper.
    ///
    /// La première version nourrissait les deux lignes, si bien que chaque
    /// répétition sortait des deux côtés à la fois : il n'y avait aucune
    /// alternance à entendre, et l'effet ressemblait à une nappe de reverb.
    /// L'ancienne version de ce test ne vérifiait que la présence d'énergie de
    /// chaque côté — ce qui était vrai des deux montages, et ne distinguait
    /// donc pas celui qui marche de celui qui ne marche pas. Il faut mesurer
    /// que le premier tour est **absent** de l'oreille opposée.
    #[test]
    fn an_echo_bounces_from_one_ear_to_the_other() {
        let mut delay = Delay::new(RATE);
        let step = samples_at(128.0);
        let window = step.round() as u32;
        let mut first = (0.0_f32, 0.0_f32);
        let mut second = (0.0_f32, 0.0_f32);
        // Mesuré **serré autour de chaque tour**, et non sur une période
        // entière. Une répétition n'occupe que quelques dizaines
        // d'échantillons : compter la période complète faisait tomber le début
        // du second tour dans la fenêtre du premier — la lecture fractionnaire
        // et l'amortissement l'étalent deux échantillons avant la borne — et le
        // test accusait une fuite qui n'en était pas une.
        let near = |frame: u32, centre: u32| frame.abs_diff(centre) < 128;
        // Une entrée stéréo, comme celle que le bus envoie réellement : c'est
        // le cas où l'ancien montage échouait.
        for frame in 0..(window * 3) {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let (left, right) = delay.process(input, input, step);
            if near(frame, window) {
                first.0 += left.abs();
                first.1 += right.abs();
            }
            if near(frame, window * 2) {
                second.0 += left.abs();
                second.1 += right.abs();
            }
        }
        assert!(first.0 > 0.1, "le premier tour manque à gauche : {first:?}");
        assert!(
            first.1 < first.0 * 0.02,
            "le premier tour ne doit pas sortir à droite : {first:?}"
        );
        assert!(
            second.1 > second.0 * 4.0,
            "le second tour doit avoir traversé : {second:?}"
        );
    }

    /// Les répétitions doivent s'éteindre. Un rebouclage mal réglé s'emballe, et
    /// cela ne s'entend qu'une fois la musique arrêtée.
    #[test]
    fn the_repeats_die_away() {
        let mut delay = Delay::new(RATE);
        let step = samples_at(128.0);
        let mut peak = 0.0_f32;
        for frame in 0..(RATE * 20) {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let (left, right) = delay.process(input, 0.0, step);
            if frame > RATE * 15 {
                peak = peak.max(left.abs()).max(right.abs());
            }
        }
        assert!(peak < 1.0e-3, "les échos sonnent encore : crête à {peak}");
    }

    /// Le moteur cesse de calculer l'écho au bout d'un budget fixe. Ce budget
    /// doit couvrir la traîne **au tempo le plus lent**, faute de quoi il
    /// couperait exactement ce que le rebouclage vient d'allonger — et il le
    /// couperait net, ce qui est le défaut qu'on cherchait à corriger.
    #[test]
    fn the_tail_budget_covers_the_slowest_tempo() {
        let seconds_to_quiet = |bpm: f32| {
            let mut delay = Delay::new(RATE);
            let step = samples_at(bpm);
            let mut last_loud = 0;
            for frame in 0..(RATE * 30) {
                let input = if frame == 0 { 1.0 } else { 0.0 };
                let (left, right) = delay.process(input, 0.0, step);
                if left.abs().max(right.abs()) > 1.0e-4 {
                    last_loud = frame;
                }
            }
            last_loud as f32 / RATE as f32
        };
        // La même valeur que `DELAY_TAIL_FRAMES` côté moteur, en secondes.
        const BUDGET_SECONDS: f32 = 25.0;
        let slowest = seconds_to_quiet(40.0);
        assert!(
            slowest < BUDGET_SECONDS,
            "à quarante BPM la traîne dure {slowest} s pour un budget de {BUDGET_SECONDS} s"
        );
    }

    /// Chaque tour doit être plus sombre que le précédent : c'est ce qui fait
    /// passer les échos derrière la musique au lieu de se battre avec elle.
    #[test]
    fn each_repeat_comes_back_darker() {
        let mut delay = Delay::new(RATE);
        let step = samples_at(128.0);
        let window = step.round() as usize;
        // Une salve d'aigus : ce qui reste après quelques tours doit avoir
        // perdu bien plus que le rebouclage seul ne l'expliquerait.
        let mut first = 0.0_f32;
        let mut later = 0.0_f32;
        for frame in 0..(window * 5) {
            let input = if frame < 32 {
                if frame % 2 == 0 { 0.8 } else { -0.8 }
            } else {
                0.0
            };
            let (left, _) = delay.process(input, 0.0, step);
            if (window..window + 64).contains(&frame) {
                first += left.abs();
            }
            if (window * 3..window * 3 + 64).contains(&frame) {
                later += left.abs();
            }
        }
        assert!(first > 0.0, "le premier tour doit exister");
        assert!(
            later < first * FEEDBACK * FEEDBACK,
            "le quatrième tour n'est pas plus sombre : {first} puis {later}"
        );
    }

    /// Un envoi soutenu et fort ne doit pas faire diverger la boucle.
    #[test]
    fn a_loud_sustained_send_never_blows_up() {
        let mut delay = Delay::new(RATE);
        let step = samples_at(128.0);
        let mut peak = 0.0_f32;
        for frame in 0..(RATE * 4) {
            let input = if frame % 2 == 0 { 1.0 } else { -1.0 };
            let (left, right) = delay.process(input, input, step);
            assert!(left.is_finite() && right.is_finite());
            peak = peak.max(left.abs()).max(right.abs());
        }
        assert!(peak < 3.0, "la boucle diverge : crête à {peak}");
    }

    /// Le retour reste sous ce qu'on lui donne — un écho qui passe devant ce
    /// qu'il répète n'est plus un écho — mais il doit rester franc.
    ///
    /// Borné des **deux côtés** : la borne haute seule laissait passer un retour
    /// si faible qu'on ne l'entendait pas, ce qui a été le défaut signalé.
    #[test]
    fn the_return_stays_under_the_source_without_disappearing() {
        let mut delay = Delay::new(RATE);
        let step = samples_at(128.0);
        let mut peak = 0.0_f32;
        for frame in 0..RATE {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            let (left, right) = delay.process(input, input, step);
            peak = peak.max(left.abs()).max(right.abs());
        }
        assert!(
            (0.5..1.0).contains(&peak),
            "le premier écho doit s'entendre sans couvrir sa source : {peak}"
        );
    }

    /// Le tempo le plus lent que le programme accepte doit tenir dans la ligne,
    /// sinon l'écho reviendrait trop tôt sans que rien ne le signale.
    #[test]
    fn the_slowest_tempo_still_fits_in_the_line() {
        // Quarante BPM, le plancher de `validate_bpm`.
        let needed = samples_at(40.0);
        let available = MAX_DELAY_SECONDS * RATE as f32;
        assert!(
            needed < available,
            "{needed} échantillons demandés pour {available} disponibles"
        );
    }

    #[test]
    fn a_reset_empties_the_lines() {
        let mut delay = Delay::new(RATE);
        let step = samples_at(128.0);
        for frame in 0..1_000 {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            delay.process(input, input, step);
        }
        delay.reset();
        assert_eq!(delay.process(0.0, 0.0, step), (0.0, 0.0));
    }
}
