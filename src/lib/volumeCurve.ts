/**
 * Where a volume automation node sits vertically, and what a vertical position
 * means in decibels.
 *
 * Both directions live here because they have to agree exactly. When they did
 * not, grabbing a node read its own drawn position as a different gain and the
 * node jumped before it followed the pointer.
 */

/** Height of the automation SVG's viewBox, covering all three lane pairs. */
export const AUTOMATION_VIEWBOX_UNITS = 450;
/** Height of one lane pair: its filter sub-lane on top, its audio lane below. */
export const LANE_PAIR_UNITS = 150;
/** Filter sub-lane height, matching `.timeline-filter-lane` in the stylesheet. */
export const FILTER_LANE_UNITS = (LANE_PAIR_UNITS / 3) * (43 / 50);

/**
 * Niveau d'une voie là où l'utilisateur n'a rien décidé.
 *
 * Doit rester d'accord avec `DEFAULT_TRACK_GAIN_DB` dans
 * `src-tauri/src/timeline.rs`, qui est ce que le moteur applique réellement.
 * Le frontend en tenait sa propre copie écrite en dur, restée à −6 dB quand le
 * moteur est passé à −4 : la ligne dessinée mentait alors sur ce qu'on entend.
 */
export const DEFAULT_TRACK_GAIN_DB = -4;

export const VOLUME_MAX_DB = 12;

/**
 * Le plus bas que l'échelle descende avant de basculer au silence.
 *
 * Quarante décibels sous l'unité, et non soixante. Entre les deux, il n'y a
 * rien à entendre dans un mix : c'était de la course perdue, prise sur la
 * partie de l'enveloppe où le travail se fait réellement. Le moteur, lui,
 * accepte toujours jusqu'à −60 — un projet ancien qui porte de telles valeurs
 * les joue sans broncher, il ne peut simplement plus les redessiner.
 */
export const VOLUME_FLOOR_DB = -40;

/** Bord supérieur de la voie audio, sous la bande de filtre. */
const LANE_TOP_UNITS = FILTER_LANE_UNITS;
/** Marge en haut et en bas, pour qu'un nœud ne soit pas rogné par le bord. */
const NODE_MARGIN_UNITS = 8;

/**
 * La course occupe désormais **toute la voie**, du haut au bas.
 *
 * Elle n'en prenait que 43 % : le plafond de +12 dB tombait au tiers de la
 * hauteur, et tout le reste ne servait à rien. Un même glissé devait donc
 * résoudre le double d'amplitude par pixel.
 */
const VOLUME_TOP_UNITS = LANE_TOP_UNITS + NODE_MARGIN_UNITS;
/**
 * Le bas de la course : le plancher et le silence au même endroit.
 *
 * Les séparer ferait sauter de six unités tout nœud posé au plancher dès qu'on
 * le saisit — c'est exactement le défaut qui a fait naître ce module. Le
 * silence arrive donc « avant » par la valeur et non par la position : à −40 dB
 * plutôt qu'à −60, puisque ce qui vit en dessous ne s'entend pas dans un mix.
 * Toute la course gagnée va à la partie utile de l'enveloppe.
 */
const VOLUME_FLOOR_UNITS = LANE_PAIR_UNITS - NODE_MARGIN_UNITS;

/**
 * Where 0 dB sits inside a lane pair, measured from the pair's top.
 *
 * Au milieu de la course : la moitié haute pour douze décibels de gain, la
 * moitié basse pour quarante de coupure.
 */
const VOLUME_ZERO_UNITS = (VOLUME_TOP_UNITS + VOLUME_FLOOR_UNITS) / 2;
/** Course d'une moitié, du zéro vers l'un ou l'autre bout. */
const VOLUME_HALF_SPAN_UNITS = VOLUME_ZERO_UNITS - VOLUME_TOP_UNITS;

/**
 * La courbe de la moitié basse.
 *
 * Elle était linéaire en décibels : quarante décibels répartis également sous
 * l'unité. À l'œil c'est régulier, à l'oreille non. Un dixième de course
 * coûtait déjà quatre décibels — un mouvement de deux pixels s'entendait —
 * tandis que le dernier quart courait de − 30 à − 40 dB, où il n'y a plus rien
 * à entendre dans un mix. Le geste utile se faisait donc dans une bande
 * étroite, et le reste de la course ne servait à rien.
 *
 * La position élevée au carré redistribue ça. Un fader de console suit une loi
 * du même genre, et la comparaison est directe : à mi-course, cette courbe donne
 * − 10 dB et un fader normalisé aussi; aux trois quarts, − 22,5 contre − 30
 * environ. Près de l'unité la résolution devient dix fois plus fine — un
 * dixième de course vaut 0,4 dB au lieu de 4.
 *
 * Seul le **dessin** change. Le moteur reçoit toujours des décibels, et un
 * projet enregistré sonne exactement pareil : ses nœuds sont simplement
 * redessinés plus haut dans la voie.
 */
const VOLUME_TAPER = 2;

/** Vertical position of a gain, in viewBox units from the top of the SVG. */
export function volumeNodeY(lane: number, gainDb: number | null): number {
  const base = lane * LANE_PAIR_UNITS;
  if (gainDb === null) return base + VOLUME_FLOOR_UNITS;
  const delta =
    gainDb >= 0
      ? (Math.min(gainDb, VOLUME_MAX_DB) / VOLUME_MAX_DB) * VOLUME_HALF_SPAN_UNITS
      : -(
        (Math.max(gainDb, VOLUME_FLOOR_DB) / VOLUME_FLOOR_DB) ** (1 / VOLUME_TAPER)
        * VOLUME_HALF_SPAN_UNITS
      );
  return base + VOLUME_ZERO_UNITS - delta;
}

/**
 * The inverse: the gain a vertical position stands for, rounded to the tenth of
 * a decibel that the interface displays.
 *
 * Le silence est le plancher, et non une zone à part : atteindre le bas de la
 * course vaut −∞, et `volumeNodeY` y dessine ce silence, de sorte qu'un nœud
 * qui y bascule ne bouge pas.
 */
export function volumeNodeGainDb(lane: number, y: number): number | null {
  const local = Math.max(y - lane * LANE_PAIR_UNITS, VOLUME_TOP_UNITS);
  const delta = VOLUME_ZERO_UNITS - local;
  if (delta >= 0) {
    const db = (delta / VOLUME_HALF_SPAN_UNITS) * VOLUME_MAX_DB;
    return Math.round(Math.min(VOLUME_MAX_DB, db) * 10) / 10;
  }
  const travel = Math.min(1, -delta / VOLUME_HALF_SPAN_UNITS);
  const db = VOLUME_FLOOR_DB * travel ** VOLUME_TAPER;
  if (db <= VOLUME_FLOOR_DB) return null;
  return Math.round(db * 10) / 10;
}

/**
 * Vertical travel of the pan line either side of centre.
 *
 * Nearly the whole audio lane, less the room a node needs not to be clipped at
 * the edges. A short travel cost precision: the same drag had to resolve the
 * full stereo field, so a pixel was worth several percent of pan. The two
 * lines cross more often for it, but colour and shape already tell them apart —
 * and the view toggle can hide either one.
 */
const PAN_HALF_SPAN_UNITS = 46;
/**
 * Le niveau en vigueur à un beat donné, interpolé entre les nœuds voisins.
 * Une voie sans nœud vaut le niveau par défaut.
 */
export function volumeDbAtBeat(
  nodes: readonly { lane: number; beat: number; gainDb: number | null }[],
  lane: number,
  beat: number,
): number {
  const laneNodes = nodes
    .filter((node) => node.lane === lane)
    .sort((left, right) => left.beat - right.beat);
  if (laneNodes.length === 0) return DEFAULT_TRACK_GAIN_DB;

  const level = (node: { gainDb: number | null }) => node.gainDb ?? VOLUME_FLOOR_DB;
  const previous = [...laneNodes].reverse().find((node) => node.beat <= beat);
  const next = laneNodes.find((node) => node.beat >= beat);
  if (previous && next) {
    if (next.beat - previous.beat < 1e-9) return level(next);
    const mix = (beat - previous.beat) / (next.beat - previous.beat);
    return level(previous) + (level(next) - level(previous)) * mix;
  }
  return level(previous ?? next!);
}

/** Where a centred pan sits inside a lane pair — the middle of the audio lane. */
const PAN_CENTRE_UNITS = FILTER_LANE_UNITS + (LANE_PAIR_UNITS - FILTER_LANE_UNITS) / 2;

/**
 * Vertical position of a pan value, from −1 (hard left) to +1 (hard right).
 *
 * Left is up. Nothing about a stereo field says which side belongs at the top,
 * so the convention has to be stated once and held everywhere: a node lifted
 * above the centre line sends the track left.
 */
export function panNodeY(lane: number, value: number): number {
  const clamped = Math.max(-1, Math.min(1, value));
  return lane * LANE_PAIR_UNITS + PAN_CENTRE_UNITS + clamped * PAN_HALF_SPAN_UNITS;
}

/** The inverse: the pan a vertical position stands for, rounded to the hundredth. */
export function panNodeValue(lane: number, y: number): number {
  const offset = y - lane * LANE_PAIR_UNITS - PAN_CENTRE_UNITS;
  const value = Math.max(-1, Math.min(1, offset / PAN_HALF_SPAN_UNITS));
  return Math.round(value * 100) / 100;
}

/**
 * Le panoramique en vigueur à un beat donné, interpolé entre les nœuds voisins.
 * Une voie sans nœud est au centre.
 */
export function panValueAtBeat(
  nodes: readonly { lane: number; beat: number; value: number }[],
  lane: number,
  beat: number,
): number {
  const laneNodes = nodes
    .filter((node) => node.lane === lane)
    .sort((left, right) => left.beat - right.beat);
  if (laneNodes.length === 0) return 0;
  const previous = [...laneNodes].reverse().find((node) => node.beat <= beat);
  const next = laneNodes.find((node) => node.beat >= beat);
  if (previous && next) {
    if (next.beat - previous.beat < 1e-9) return next.value;
    const mix = (beat - previous.beat) / (next.beat - previous.beat);
    return previous.value + (next.value - previous.value) * mix;
  }
  return (previous ?? next!).value;
}

/** Where the centre line of a lane's pan sits, with no node anywhere. */
export function panCentreY(lane: number): number {
  return panNodeY(lane, 0);
}

export function panLabel(value: number): string {
  const rounded = Math.round(Math.abs(value) * 100);
  if (rounded === 0) return "C";
  return `${value < 0 ? "L" : "R"}${rounded}`;
}

/** Converts a pointer position into the viewBox units the mapping works in. */
export function automationUnitsAtPointer(
  clientY: number,
  boundsTop: number,
  boundsHeight: number,
): number {
  if (boundsHeight <= 0) return 0;
  return ((clientY - boundsTop) / boundsHeight) * AUTOMATION_VIEWBOX_UNITS;
}

export function gainLabel(gainDb: number | null): string {
  if (gainDb === null) return "−∞ dB";
  return `${gainDb > 0 ? "+" : ""}${gainDb.toFixed(1)} dB`;
}
