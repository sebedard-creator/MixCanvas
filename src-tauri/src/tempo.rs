use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

const MIN_BPM: f64 = 40.0;
const MAX_BPM: f64 = 300.0;
const EPSILON: f64 = 1.0e-9;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoPoint {
    pub beat: f64,
    pub bpm: f64,
    pub clip_id: Option<i64>,
}

impl TempoPoint {
    pub fn project_start(bpm: f64) -> Self {
        Self {
            beat: 0.0,
            bpm,
            clip_id: None,
        }
    }

    pub fn clip_target(beat: f64, bpm: f64, clip_id: i64) -> Self {
        Self {
            beat,
            bpm,
            clip_id: Some(clip_id),
        }
    }
}

/// Combien de temps entiers la table cumulative accepte de couvrir.
///
/// La limite de sécurité du projet est de quatre heures; à 300 BPM cela fait
/// soixante-douze mille temps. Cette borne est au-dessus, et n'existe que pour
/// qu'une carte de tempo aberrante ne demande jamais un tableau démesuré.
const MAX_TABLE_BEATS: usize = 100_000;

/// La carte de tempo du projet.
///
/// **Le tempo est constant à l'intérieur d'un temps et ne change qu'à sa
/// frontière.** Il l'a longtemps été de façon continue — une rampe linéaire
/// entre deux ancres, évaluée à chaque instant — et cela s'entendait : un
/// changement de tempo au milieu d'un temps est à nu, alors qu'un changement à
/// la frontière d'un temps est masqué par la transitoire qui s'y trouve. Sur un
/// seul morceau on entendait la vitesse glisser; sur deux clips superposés, les
/// transitoires dérivaient l'une contre l'autre au lieu de rester verrouillées,
/// ce qui est le contraire de ce qu'on cherche en mixant.
///
/// Une ancre posée sur un temps entier est donc honorée exactement. Une ancre
/// posée entre deux temps voit son changement prendre effet à la frontière
/// suivante — ce qui est précisément le comportement demandé, et non une
/// approximation subie.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    points: Vec<TempoPoint>,
    /// Secondes écoulées au **début** du temps `n`, pour `n` allant de zéro à
    /// la dernière ancre.
    ///
    /// La durée d'un temps vaut `60 / bpm`, mais ce BPM change d'un temps à
    /// l'autre le long d'une rampe : la somme n'a pas de forme close, il faut
    /// l'accumuler. Elle est accumulée **une fois par édition**, et non à
    /// chaque appel, parce que `beat_at_seconds` est interrogée depuis le
    /// chemin audio — une fois par grain WSOLA. Une boucle sur les trente mille
    /// temps d'un mix y coûterait ce que la recherche de WSOLA a déjà coûté une
    /// fois cette semaine.
    beat_seconds: Vec<f64>,
}

impl TempoMap {
    pub fn new(fallback_bpm: f64, mut points: Vec<TempoPoint>) -> Result<Self, String> {
        validate_bpm(fallback_bpm)?;

        if points.iter().any(|point| {
            !point.beat.is_finite() || point.beat < 0.0 || validate_bpm(point.bpm).is_err()
        }) {
            return Err("The tempo map contains an invalid point.".to_owned());
        }

        points.push(TempoPoint::project_start(fallback_bpm));
        points.sort_by(|left, right| {
            left.beat.total_cmp(&right.beat).then_with(|| {
                left.clip_id
                    .unwrap_or(i64::MIN)
                    .cmp(&right.clip_id.unwrap_or(i64::MIN))
            })
        });

        let mut deduplicated: Vec<TempoPoint> = Vec::with_capacity(points.len());
        for point in points {
            if let Some(previous) = deduplicated.last_mut()
                && (previous.beat - point.beat).abs() <= EPSILON
            {
                *previous = point;
                continue;
            }
            deduplicated.push(point);
        }

        let mut map = Self {
            points: deduplicated,
            beat_seconds: Vec::new(),
        };
        map.beat_seconds = map.accumulate_beat_seconds();
        Ok(map)
    }

    /// Les secondes écoulées au début de chaque temps entier, jusqu'à la
    /// dernière ancre. Au-delà le tempo ne bouge plus et une multiplication
    /// suffit.
    fn accumulate_beat_seconds(&self) -> Vec<f64> {
        let last_anchor_beat = self
            .points
            .last()
            .map_or(0.0, |point| point.beat)
            .max(0.0)
            .ceil();
        let covered = (last_anchor_beat as usize).min(MAX_TABLE_BEATS);

        let mut table = Vec::with_capacity(covered + 1);
        let mut elapsed = 0.0;
        table.push(elapsed);
        for beat in 0..covered {
            elapsed += 60.0 / self.ramp_bpm_at(beat as f64);
            table.push(elapsed);
        }
        table
    }

    /// Le tempo que la rampe atteint à cet endroit, sans quantification.
    ///
    /// C'est l'ancienne interpolation continue, devenue interne : elle ne sert
    /// plus qu'à donner son tempo à un temps entier, jamais à décrire ce que le
    /// moteur joue entre deux temps.
    fn ramp_bpm_at(&self, beat: f64) -> f64 {
        let first = self.points[0];
        if beat <= first.beat {
            return first.bpm;
        }

        for window in self.points.windows(2) {
            let start = window[0];
            let end = window[1];
            if beat <= end.beat {
                let progress = (beat - start.beat) / (end.beat - start.beat);
                return start.bpm + (end.bpm - start.bpm) * progress;
            }
        }

        self.points.last().map_or(first.bpm, |point| point.bpm)
    }

    pub fn points(&self) -> &[TempoPoint] {
        &self.points
    }

    /// Le tempo en vigueur à cet endroit — celui du **temps** qui le contient.
    pub fn bpm_at_beat(&self, beat: f64) -> f64 {
        self.ramp_bpm_at(beat.max(0.0).floor())
    }

    pub fn seconds_at_beat(&self, beat: f64) -> f64 {
        let beat = beat.max(0.0);
        let whole = beat.floor();
        let index = (whole as usize).min(self.beat_seconds.len() - 1);
        let anchor_beat = index as f64;

        // Un temps dure `60 / bpm`, et ce BPM tient d'un bout à l'autre : la
        // part entamée du temps courant est donc une simple proportion.
        self.beat_seconds[index] + (beat - anchor_beat) * 60.0 / self.ramp_bpm_at(anchor_beat)
    }

    pub fn beat_at_seconds(&self, seconds: f64) -> f64 {
        let seconds = seconds.max(0.0);
        // Le tableau est croissant par construction — un temps dure toujours
        // une durée strictement positive —, donc il se cherche par dichotomie.
        let index = match self
            .beat_seconds
            .binary_search_by(|elapsed| elapsed.total_cmp(&seconds))
        {
            Ok(found) => return found as f64,
            Err(0) => 0,
            Err(insertion) => insertion - 1,
        };

        let anchor_beat = index as f64;
        let bpm = self.ramp_bpm_at(anchor_beat);
        anchor_beat + (seconds - self.beat_seconds[index]) * bpm / 60.0
    }

    pub fn bpm_extrema_between(&self, start_beat: f64, end_beat: f64) -> (f64, f64) {
        let start_beat = start_beat.max(0.0);
        let end_beat = end_beat.max(start_beat);
        let mut minimum = self.bpm_at_beat(start_beat);
        let mut maximum = minimum;

        let end_bpm = self.bpm_at_beat(end_beat);
        minimum = minimum.min(end_bpm);
        maximum = maximum.max(end_bpm);

        for point in &self.points {
            if point.beat > start_beat && point.beat < end_beat {
                minimum = minimum.min(point.bpm);
                maximum = maximum.max(point.bpm);
            }
        }

        (minimum, maximum)
    }

    pub fn signature(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for point in &self.points {
            point.beat.to_bits().hash(&mut hasher);
            point.bpm.to_bits().hash(&mut hasher);
            point.clip_id.hash(&mut hasher);
        }
        hasher.finish()
    }
}

fn validate_bpm(bpm: f64) -> Result<(), String> {
    if bpm.is_finite() && (MIN_BPM..=MAX_BPM).contains(&bpm) {
        Ok(())
    } else {
        Err(format!(
            "The BPM has to be between {MIN_BPM:.0} and {MAX_BPM:.0}."
        ))
    }
}

/* `linear_segment_seconds` et son inverse vivaient ici. Elles intégraient une
rampe **continue** — un logarithme et une exponentielle, la forme close de
l'intégrale de `1/bpm` quand le BPM varie linéairement. Le tempo étant
désormais constant sur chaque temps, cette intégrale n'a plus d'objet : la
durée d'un temps vaut `60 / bpm` et la somme est accumulée une fois pour
toutes dans `beat_seconds`. */

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> TempoMap {
        TempoMap::new(
            120.0,
            vec![
                TempoPoint::clip_target(0.0, 120.0, 1),
                TempoPoint::clip_target(16.0, 128.0, 2),
            ],
        )
        .expect("valid tempo map")
    }

    #[test]
    fn the_ramp_reaches_its_targets_on_the_beats() {
        let map = ramp();
        assert!((map.bpm_at_beat(8.0) - 124.0).abs() < 1.0e-9);
        assert!((map.bpm_at_beat(20.0) - 128.0).abs() < 1.0e-9);
    }

    /// Le défaut que ce modèle corrige : le tempo bougeait **à l'intérieur**
    /// d'un temps, et cela s'entendait. Il doit désormais tenir d'un bout à
    /// l'autre du temps et ne changer qu'à sa frontière.
    #[test]
    fn the_tempo_holds_inside_a_beat_and_steps_at_its_edge() {
        let map = ramp();
        let start = map.bpm_at_beat(8.0);

        for offset in [0.01, 0.25, 0.5, 0.75, 0.99] {
            assert!(
                (map.bpm_at_beat(8.0 + offset) - start).abs() < 1.0e-12,
                "le tempo a bougé à {offset} d'un temps"
            );
        }

        // Et il change bien au temps suivant, sinon on aurait simplement figé
        // la rampe.
        assert!((map.bpm_at_beat(9.0) - start).abs() > 0.4);
    }

    /// Un temps dure exactement `60 / bpm` : c'est ce qui rend la frontière
    /// franche, et c'est ce que tout le reste du moteur suppose.
    #[test]
    fn a_beat_lasts_exactly_what_its_tempo_says() {
        let map = ramp();
        for beat in 0..16 {
            let expected = 60.0 / map.bpm_at_beat(beat as f64);
            let measured =
                map.seconds_at_beat(beat as f64 + 1.0) - map.seconds_at_beat(beat as f64);
            assert!(
                (measured - expected).abs() < 1.0e-9,
                "temps {beat} : {measured} au lieu de {expected}"
            );
        }
    }

    #[test]
    fn time_never_runs_backwards_across_a_ramp() {
        let map = ramp();
        let mut previous = -1.0;
        let mut beat = 0.0;
        while beat <= 24.0 {
            let seconds = map.seconds_at_beat(beat);
            assert!(seconds > previous, "recul au beat {beat}");
            previous = seconds;
            beat += 0.125;
        }
    }

    /// La table cumulative est construite une fois par édition, et la lecture
    /// se fait par dichotomie : une carte longue doit rester juste, et le rester
    /// sans que personne n'ait envie de compter les temps à chaque appel.
    #[test]
    fn a_long_map_stays_exact_from_end_to_end() {
        let map = TempoMap::new(
            120.0,
            vec![
                TempoPoint::clip_target(0.0, 120.0, 1),
                TempoPoint::clip_target(20_000.0, 180.0, 2),
            ],
        )
        .expect("valid tempo map");

        for beat in [0.0, 1.0, 999.5, 10_000.0, 19_999.75, 20_000.0, 25_000.0] {
            let reconstructed = map.beat_at_seconds(map.seconds_at_beat(beat));
            assert!((reconstructed - beat).abs() < 1.0e-6, "beat {beat}");
        }
    }

    #[test]
    fn beat_and_seconds_round_trip_across_a_ramp() {
        let map = ramp();
        for beat in [0.0, 2.0, 8.0, 15.5, 16.0, 24.0, 64.0] {
            let reconstructed = map.beat_at_seconds(map.seconds_at_beat(beat));
            assert!((reconstructed - beat).abs() < 1.0e-8, "beat {beat}");
        }
    }

    #[test]
    fn latest_clip_wins_when_targets_share_an_anchor() {
        let map = TempoMap::new(
            120.0,
            vec![
                TempoPoint::clip_target(8.0, 124.0, 10),
                TempoPoint::clip_target(8.0, 126.0, 12),
                TempoPoint::clip_target(8.0, 125.0, 11),
            ],
        )
        .expect("valid tempo map");

        assert_eq!(map.points().len(), 2);
        assert!((map.bpm_at_beat(8.0) - 126.0).abs() < 1.0e-9);
    }

    #[test]
    fn extrema_include_targets_inside_the_requested_range() {
        let map = TempoMap::new(
            120.0,
            vec![
                TempoPoint::clip_target(8.0, 130.0, 1),
                TempoPoint::clip_target(16.0, 110.0, 2),
            ],
        )
        .expect("valid tempo map");

        assert_eq!(map.bpm_extrema_between(4.0, 20.0), (110.0, 130.0));
    }
}
