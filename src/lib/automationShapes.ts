/**
 * Formes d'automation pré-établies, dessinées d'un seul glissé.
 *
 * Le geste donne trois choses : l'étendue horizontale, la valeur pointée — donc
 * l'amplitude — et rien d'autre. La forme et la période viennent du bouton, ce
 * qui évite de les redemander à chaque trait.
 *
 * Volume et panoramique n'oscillent pas autour de la même chose, parce que les
 * deux grandeurs n'ont pas la même nature. Un niveau a un plafond — celui qui
 * est déjà en place — donc la forme creuse vers le bas. Un panoramique a un
 * centre, donc la forme se balance de part et d'autre.
 */

import { VOLUME_FLOOR_DB, VOLUME_MAX_DB } from "./volumeCurve";

export type ShapeKind = "step" | "sine" | "triangle";
export type ShapePeriod = 0.5 | 1 | 2 | 4;

export const SHAPE_KINDS: ShapeKind[] = ["step", "sine", "triangle"];
export const SHAPE_PERIODS: ShapePeriod[] = [0.5, 1, 2, 4];

/**
 * Plafond de nœuds pour un seul trait, dans le même esprit que les 512
 * échantillons du pinceau de filtre.
 *
 * La résolution baisse d'abord : un sinus long est plus grossier qu'un sinus
 * court, mais couvre bien toute son étendue. Elle ne peut pourtant pas baisser
 * indéfiniment — représenter mille cycles en demande au moins deux mille
 * nœuds — donc au-delà c'est la longueur qui cède, et le trait s'arrête là où
 * il cesse d'être représentable. Une limite qu'on voit vaut mieux qu'un
 * dépassement silencieux.
 */
export const MAX_SHAPE_NODES = 2_048;

/** Le grain le plus fin, et le plus grossier, de chaque forme sur un cycle. */
const SAMPLES_PER_CYCLE: Record<ShapeKind, { fine: number; coarse: number }> = {
  // Deux paires de nœuds : un palier haut, un palier bas. Les paliers ne
  // tiennent que parce que chaque transition porte deux nœuds presque
  // confondus — l'interpolation entre nœuds étant linéaire, un carré tracé
  // avec un seul nœud par palier ressortirait en triangle.
  step: { fine: 4, coarse: 4 },
  sine: { fine: 12, coarse: 4 },
  triangle: { fine: 2, coarse: 2 },
};

/** Écart entre les deux nœuds d'une transition franche, en temps. */
const STEP_EDGE_BEATS = 0.01;

export interface ShapePoint {
  beat: number;
  /** −1 au creux de la forme, +1 à sa crête. */
  unit: number;
}

/**
 * La forme en unités normalisées, de −1 à +1, sur l'étendue demandée.
 *
 * Séparer la forme de la grandeur qu'elle pilote permet d'écrire une seule fois
 * la géométrie, et de la traduire ensuite en décibels ou en panoramique.
 */
export function shapePoints(
  startBeat: number,
  endBeat: number,
  kind: ShapeKind,
  period: ShapePeriod,
): ShapePoint[] {
  const from = Math.min(startBeat, endBeat);
  const to = Math.max(startBeat, endBeat);
  const span = to - from;
  if (!(span > 0) || !Number.isFinite(span)) return [];

  const requested = Math.max(1, Math.round(span / period));
  const { fine, coarse } = SAMPLES_PER_CYCLE[kind];
  // D'abord la résolution.
  let perCycle = fine;
  while (perCycle > coarse && requested * perCycle > MAX_SHAPE_NODES) {
    perCycle -= kind === "sine" ? 2 : 1;
  }
  // Puis, seulement si elle n'a pas suffi, la longueur.
  const cycles = Math.min(requested, Math.floor(MAX_SHAPE_NODES / perCycle));
  const drawnEnd = Math.min(to, from + cycles * period);

  const points: ShapePoint[] = [];

  for (let cycle = 0; cycle < cycles; cycle += 1) {
    const cycleStart = from + cycle * period;
    if (cycleStart >= drawnEnd) break;

    if (kind === "step") {
      const half = period / 2;
      points.push({ beat: cycleStart, unit: 1 });
      points.push({ beat: cycleStart + half - STEP_EDGE_BEATS, unit: 1 });
      points.push({ beat: cycleStart + half, unit: -1 });
      points.push({ beat: cycleStart + period - STEP_EDGE_BEATS, unit: -1 });
      continue;
    }

    for (let step = 0; step < perCycle; step += 1) {
      const phase = step / perCycle;
      const beat = cycleStart + phase * period;
      const unit = kind === "sine"
        ? Math.sin(phase * Math.PI * 2)
        // Triangle : monte sur la première moitié, redescend sur la seconde.
        : phase < 0.5 ? 1 - 4 * phase : 4 * phase - 3;
      points.push({ beat, unit });
    }
  }

  // Referme la forme sur sa fin, sans quoi le dernier cycle resterait suspendu
  // à sa dernière valeur jusqu'au nœud suivant.
  points.push({ beat: drawnEnd, unit: points.length > 0 ? points[0].unit : 0 });

  // La forme est resserrée d'un centième de temps à chaque bout, pour laisser
  // la place aux ancres de repos que posent les traductions. Deux centièmes
  // pris sur toute une étendue ne s'entendent pas; une automation qui démarre
  // ailleurs qu'au repos, si.
  const inner = drawnEnd - from - 2 * STEP_EDGE_BEATS;
  const scale = inner > 0 ? inner / (drawnEnd - from) : 0;
  return points
    .filter((point) => point.beat >= from && point.beat <= drawnEnd)
    .map((point) => ({
      beat: from + STEP_EDGE_BEATS + (point.beat - from) * scale,
      unit: point.unit,
    }));
}

/**
 * Les bornes d'une forme dessinée sont celles de la ligne elle-même.
 *
 * Elles vivaient ici en double, écrites en dur : le plancher est resté à −60
 * quand l'échelle est montée à −40, et une forme pouvait descendre sous ce que
 * la ligne sait redessiner.
 */
export const VOLUME_SHAPE_FLOOR_DB = VOLUME_FLOOR_DB;
export const VOLUME_SHAPE_CEILING_DB = VOLUME_MAX_DB;

/**
 * La forme traduite en décibels.
 *
 * Elle va du niveau déjà en place à la hauteur pointée : glisser vers le bas
 * creuse, ce qui est le geste d'un gate ou d'un trémolo. La crête reste au
 * niveau réglé plutôt que de le dépasser, faute de quoi un trémolo mangerait la
 * réserve du limiteur à chaque montée.
 */
export function volumeShapeNodes(
  startBeat: number,
  endBeat: number,
  restDb: number,
  pointedDb: number,
  kind: ShapeKind,
  period: ShapePeriod,
): { beat: number; gainDb: number }[] {
  const top = Math.max(restDb, pointedDb);
  const bottom = Math.min(restDb, pointedDb);
  const shaped = shapePoints(startBeat, endBeat, kind, period).map((point) => ({
    beat: point.beat,
    // `unit` va de −1 à +1; la forme occupe la bande entre les deux valeurs.
    gainDb: clamp(
      bottom + ((point.unit + 1) / 2) * (top - bottom),
      VOLUME_SHAPE_FLOOR_DB,
      VOLUME_SHAPE_CEILING_DB,
    ),
  }));
  // L'ancre subit les mêmes bornes que la forme : un niveau de repos aberrant
  // ne doit pas passer par la porte que la forme, elle, a franchie bornée.
  const anchor = clamp(restDb, VOLUME_SHAPE_FLOOR_DB, VOLUME_SHAPE_CEILING_DB);
  return anchorAtRest(shaped, startBeat, endBeat, anchor, (beat, gainDb) => ({ beat, gainDb }));
}

/**
 * La forme traduite en panoramique.
 *
 * Symétrique autour du centre : pointer L60 donne un balancement L60 ↔ R60,
 * ce qu'on attend d'un auto-pan. Partir du centre vers un seul côté obligerait
 * à commencer le geste à un extrême pour balayer tout le champ.
 */
export function panShapeNodes(
  startBeat: number,
  endBeat: number,
  amplitude: number,
  kind: ShapeKind,
  period: ShapePeriod,
  restValue = 0,
): { beat: number; value: number }[] {
  const reach = clamp(Math.abs(amplitude), 0, 1);
  const shaped = shapePoints(startBeat, endBeat, kind, period).map((point) => ({
    beat: point.beat,
    value: clamp(point.unit * reach, -1, 1),
  }));
  const anchor = clamp(restValue, -1, 1);
  return anchorAtRest(shaped, startBeat, endBeat, anchor, (beat, value) => ({ beat, value }));
}

/**
 * Encadre un tracé par deux nœuds à la valeur de repos.
 *
 * Sans eux, la ligne rampe depuis le dernier nœud d'avant le trait jusqu'à la
 * première valeur de la forme, et repart de sa dernière valeur vers le nœud
 * suivant — de l'automation créée *vers* le dessin, que personne n'a demandée.
 * Selon la forme, ce premier point tombe sur une crête ou sur un creux :
 * l'ancrage ne peut donc pas se déduire de la forme, il doit être posé.
 */
function anchorAtRest<T extends { beat: number }>(
  shaped: T[],
  startBeat: number,
  endBeat: number,
  rest: number,
  make: (beat: number, value: number) => T,
): T[] {
  if (shaped.length === 0) return shaped;
  const from = Math.min(startBeat, endBeat);
  // Borné au trait : l'accumulation des flottants sur des centaines de cycles
  // suffit à faire sortir la dernière ancre de quelques millionièmes de temps,
  // et le serveur refuse un nœud hors de l'étendue annoncée.
  const last = Math.min(
    Math.max(startBeat, endBeat),
    shaped[shaped.length - 1].beat + STEP_EDGE_BEATS,
  );
  return [make(from, rest), ...shaped, make(last, rest)];
}

function clamp(value: number, low: number, high: number): number {
  return Math.max(low, Math.min(high, value));
}
