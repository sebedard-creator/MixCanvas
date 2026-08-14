//! Le bitcrush : le son tel qu'un convertisseur bon marché le rendrait.
//!
//! Deux dégradations, et il faut les deux. **Quantifier l'amplitude** sur peu
//! de niveaux ajoute un bruit de marches d'escalier, corrélé au signal — c'est
//! le grain. **Tenir chaque échantillon** plusieurs images de suite replie tout
//! ce qui passe au-dessus de la moitié de la fréquence tenue vers le bas du
//! spectre — c'est le repliement, et c'est lui qui donne le côté « console de
//! salon » plutôt que « bande sale ». Séparément, l'une sonne comme du souffle
//! et l'autre comme un filtre; ensemble, elles sonnent huit bits.
//!
//! **Contrairement à la reverb et au flanger, ce module est un insert.** Les
//! deux autres se somment au mix : leur retour s'ajoute au signal sec, et c'est
//! bien ce qu'on veut d'une pièce ou d'un peigne. Un bitcrush sommé au sec
//! n'aurait aucun effet audible sur le sec — on entendrait le son propre, avec
//! du grain par-dessus. Or ce qu'on demande à cet effet est précisément de
//! **remplacer** le son propre. Il travaille donc sur la contribution de la
//! piste, en fondu entre le sec et le broyé, et les envois de reverb et de
//! flanger sont pris **après** lui : on entend la pièce du son broyé, ce qui
//! est l'ordre d'une chaîne réelle.

/// Le nombre de bits que garde la quantification.
///
/// Huit, comme demandé, et c'est aussi la valeur où l'effet s'entend sans
/// devenir illisible : à quatre bits il ne reste plus de musique, à douze on
/// n'entend presque rien sur un mix déjà dense.
const CRUSH_BITS: u32 = 8;

/// La fréquence à laquelle les échantillons sont retenus, en hertz.
///
/// Un sixième de 44,1 kHz, la fréquence des vieux échantillonneurs. C'est le
/// repliement qu'elle provoque qu'on reconnaît : les aigus reviennent en bas du
/// spectre sous forme de sifflements qui ne suivent pas la mélodie.
///
/// Fixe en hertz et non en nombre d'images tenues, pour que l'effet sonne
/// pareil à 44,1 et à 48 kHz — un nombre d'images fixe décrirait deux
/// fréquences différentes.
const HOLD_RATE_HZ: f32 = 7_350.0;

/// Ce que le signal broyé pèse en sortie.
///
/// Trois décibels sous l'unité, et la raison n'est **pas** celle qu'on croirait.
/// Mesuré sur un mix dense, le broyage ne change pas l'énergie du signal : zéro
/// décibel de large, la crête ne bouge pas. Ce qu'il fait, c'est déplacer un peu
/// moins d'un décibel vers le haut du spectre — le repliement dépose là ce qu'il
/// replie, et c'est la bande où l'oreille est la plus sensible.
///
/// L'écart vient surtout d'ailleurs : **c'est le seul effet du panneau qui
/// travaille à l'unité.** Les trois autres sont des retours atténués, mêlés
/// *sous* le sec; le bitcrush, lui, est un insert et remplace le sec à plein
/// niveau. À geste égal il paraissait donc plus fort que ses voisins, ce qui
/// n'était l'effet de personne.
const CRUSH_TRIM: f32 = 0.72;

/// Les paliers de la quantification, de part et d'autre de zéro.
///
/// Huit bits signés couvrent 256 valeurs, donc 128 de chaque côté.
fn levels() -> f32 {
    (1_u32 << (CRUSH_BITS - 1)) as f32
}

/// Le broyeur d'une piste : son horloge de maintien et ce qu'elle retient.
///
/// Un par piste, et non un partagé : c'est un insert, il travaille sur le
/// signal de **cette** piste. Les trois pourraient partager leur horloge, mais
/// pas les échantillons retenus, et séparer l'un sans l'autre ne simplifierait
/// rien.
pub(crate) struct BitCrusher {
    /// Où en est l'horloge de maintien, de zéro à un.
    phase: f32,
    /// Son avance par image, calculée une fois.
    step: f32,
    /// Le dernier échantillon retenu, par canal.
    held: [f32; 2],
}

impl BitCrusher {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            phase: 1.0,
            step: HOLD_RATE_HZ / sample_rate as f32,
            held: [0.0; 2],
        }
    }

    /// Avance l'horloge d'une image et dit s'il faut prendre un échantillon.
    ///
    /// Appelée **une fois par image**, avant les canaux : les deux canaux
    /// doivent être retenus et relâchés ensemble, sans quoi leur maintien
    /// glisserait l'un par rapport à l'autre et l'image stéréo se déchirerait.
    pub(crate) fn tick(&mut self) -> bool {
        self.phase += self.step;
        if self.phase < 1.0 {
            return false;
        }
        self.phase -= 1.0;
        true
    }

    /// Le signal broyé, mêlé au sec selon `amount`.
    ///
    /// `amount` va de zéro — le signal intact — à un — entièrement broyé. Entre
    /// les deux, c'est un fondu : c'est ce qui donne à la montée et à la
    /// descente du geste la même forme que pour les deux autres effets.
    pub(crate) fn process(&mut self, input: f32, channel: usize, latch: bool, amount: f32) -> f32 {
        let slot = channel.min(1);
        if latch {
            // Le pas de quantification est relatif à la pleine échelle, si bien
            // qu'un passage discret use moins de paliers qu'un passage fort.
            // C'est exactement ce que fait un convertisseur, et c'est pourquoi
            // les fins de phrase grésillent plus que les refrains.
            let steps = levels();
            self.held[slot] = (input * steps).round() / steps;
        }
        // Le trim porte sur le broyé seul : à dosage nul, le signal doit
        // ressortir **exactement** tel qu'il est entré, et une atténuation
        // appliquée après le fondu le rabaisserait aussi.
        let crushed = self.held[slot] * CRUSH_TRIM;
        input + (crushed - input) * amount.clamp(0.0, 1.0)
    }

    /// Oublie ce qui était retenu. À appeler sur un Seek, comme les autres :
    /// l'échantillon de l'endroit qu'on quitte n'a rien à faire ici.
    pub(crate) fn reset(&mut self) {
        self.phase = 1.0;
        self.held = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// Un fondu à zéro doit rendre le signal **exactement** tel qu'il est
    /// entré : une piste sans passe de bitcrush ne doit rien subir.
    #[test]
    fn nothing_happens_at_zero() {
        let mut crusher = BitCrusher::new(RATE);
        for frame in 0..RATE {
            let latch = crusher.tick();
            let input = (frame as f32 * 0.01).sin() * 0.7;
            assert_eq!(crusher.process(input, 0, latch, 0.0), input);
        }
    }

    #[test]
    fn silence_stays_silent() {
        let mut crusher = BitCrusher::new(RATE);
        for _ in 0..RATE {
            let latch = crusher.tick();
            assert_eq!(crusher.process(0.0, 0, latch, 1.0), 0.0);
        }
    }

    /// Le grain : à fond, la sortie ne prend qu'un petit nombre de valeurs
    /// distinctes là où l'entrée en prenait des milliers.
    #[test]
    fn the_output_lands_on_a_coarse_ladder_of_values() {
        let mut crusher = BitCrusher::new(RATE);
        let mut seen = std::collections::BTreeSet::new();
        for frame in 0..RATE {
            let latch = crusher.tick();
            let input = (frame as f32 * std::f32::consts::TAU * 220.0 / RATE as f32).sin() * 0.9;
            let out = crusher.process(input, 0, latch, 1.0);
            seen.insert((out * levels()).round() as i32);
        }
        assert!(
            seen.len() <= (1 << CRUSH_BITS),
            "plus de paliers que huit bits n'en offrent : {}",
            seen.len()
        );
        assert!(
            seen.len() > 4,
            "un seul palier ne serait plus de la musique"
        );
    }

    /// Chaque valeur rendue est bien sur la grille, et non entre deux barreaux.
    ///
    /// La grille est **mise à l'échelle du trim** : celui-ci change le niveau,
    /// pas le nombre de paliers. Diviser avant de vérifier dit exactement cela,
    /// et le test échouerait encore si la quantification cessait d'opérer.
    #[test]
    fn every_value_sits_on_the_ladder() {
        let mut crusher = BitCrusher::new(RATE);
        for frame in 0..2_000 {
            let latch = crusher.tick();
            let input = (frame as f32 * 0.03).sin();
            let out = crusher.process(input, 0, latch, 1.0);
            let steps = out / CRUSH_TRIM * levels();
            assert!(
                (steps - steps.round()).abs() < 1.0e-4,
                "sortie hors grille : {out}"
            );
        }
    }

    /// Le trim est ce qui remet le bitcrush au niveau de ses voisins. S'il
    /// disparaissait, l'effet redeviendrait le plus fort du panneau sans que
    /// rien d'autre ne change.
    #[test]
    fn the_crushed_signal_comes_back_under_the_dry() {
        let mut crusher = BitCrusher::new(RATE);
        let mut dry_energy = 0.0_f64;
        let mut wet_energy = 0.0_f64;
        for frame in 0..RATE {
            let latch = crusher.tick();
            let input = (frame as f32 * std::f32::consts::TAU * 220.0 / RATE as f32).sin() * 0.6;
            let out = crusher.process(input, 0, latch, 1.0);
            dry_energy += f64::from(input) * f64::from(input);
            wet_energy += f64::from(out) * f64::from(out);
        }
        let ratio = (wet_energy / dry_energy).sqrt();
        assert!(
            (0.6..0.85).contains(&ratio),
            "le broyé devrait sortir nettement sous le sec, sans disparaître : {ratio}"
        );
    }

    /// Le repliement vient du maintien : la sortie doit rester constante entre
    /// deux prises, sinon il n'y a pas de sous-échantillonnage du tout.
    #[test]
    fn the_signal_is_held_between_two_samples() {
        let mut crusher = BitCrusher::new(RATE);
        let mut holds = 0_usize;
        let mut latches = 0_usize;
        let mut previous = None;
        for frame in 0..RATE {
            let latch = crusher.tick();
            if latch {
                latches += 1;
            }
            let input = (frame as f32 * std::f32::consts::TAU * 3_000.0 / RATE as f32).sin();
            let out = crusher.process(input, 0, latch, 1.0);
            if !latch && previous == Some(out.to_bits()) {
                holds += 1;
            }
            previous = Some(out.to_bits());
        }
        assert!(holds > RATE as usize / 2, "le signal n'est pas tenu");
        // Une seconde de musique doit donner à peu près `HOLD_RATE_HZ` prises.
        let expected = HOLD_RATE_HZ as usize;
        assert!(
            latches.abs_diff(expected) < expected / 20,
            "{latches} prises pour {expected} attendues"
        );
    }

    /// La fenêtre est décrite en hertz : elle doit valoir la même fréquence à
    /// toute fréquence d'échantillonnage.
    #[test]
    fn the_hold_rate_is_the_same_at_another_sample_rate() {
        for rate in [44_100_u32, 48_000] {
            let mut crusher = BitCrusher::new(rate);
            let latches = (0..rate).filter(|_| crusher.tick()).count();
            let expected = HOLD_RATE_HZ as usize;
            assert!(
                latches.abs_diff(expected) < expected / 20,
                "{rate} Hz : {latches} prises pour {expected} attendues"
            );
        }
    }

    /// Les deux canaux sont retenus par la **même** horloge : si l'un pouvait
    /// se rafraîchir sans l'autre, l'image stéréo se déchirerait.
    #[test]
    fn both_channels_are_held_by_one_clock() {
        let mut crusher = BitCrusher::new(RATE);
        for frame in 0..1_000 {
            let latch = crusher.tick();
            let left = crusher.process(0.5, 0, latch, 1.0);
            let right = crusher.process(0.5, 1, latch, 1.0);
            assert_eq!(left, right, "les deux canaux divergent à l'image {frame}");
        }
    }

    /// Un fondu à mi-chemin doit tomber entre le sec et le broyé, sans jamais
    /// les dépasser : c'est ce qui rend la montée du geste continue.
    #[test]
    fn a_half_fade_lands_between_the_two() {
        let mut crusher = BitCrusher::new(RATE);
        let mut dry = BitCrusher::new(RATE);
        for frame in 0..500 {
            let input = (frame as f32 * 0.07).sin() * 0.8;
            let latch = crusher.tick();
            dry.tick();
            let full = dry.process(input, 0, latch, 1.0);
            let half = crusher.process(input, 0, latch, 0.5);
            let low = input.min(full);
            let high = input.max(full);
            assert!(half >= low - 1.0e-6 && half <= high + 1.0e-6);
        }
    }

    #[test]
    fn a_reset_forgets_what_was_held() {
        let mut crusher = BitCrusher::new(RATE);
        for _ in 0..100 {
            let latch = crusher.tick();
            crusher.process(0.9, 0, latch, 1.0);
        }
        crusher.reset();
        assert_eq!(crusher.process(0.0, 0, false, 1.0), 0.0);
    }

    /// Un signal déjà au maximum ne doit pas sortir plus fort que lui : la
    /// quantification arrondit, et un arrondi vers le haut à pleine échelle
    /// ferait travailler le limiteur pour rien.
    #[test]
    fn a_full_scale_input_never_comes_back_louder() {
        let mut crusher = BitCrusher::new(RATE);
        for frame in 0..2_000 {
            let latch = crusher.tick();
            let input = if frame % 2 == 0 { 1.0 } else { -1.0 };
            let out = crusher.process(input, 0, latch, 1.0);
            assert!(out.abs() <= 1.0 + 1.0e-6, "sortie à {out}");
        }
    }
}
