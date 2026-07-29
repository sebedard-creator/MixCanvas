export type FilterBubbleShape = "ramp_up" | "ramp_down" | "triangle";

export const FILTER_BUBBLE_MIN_WIDTH_BEATS = 2;
/**
 * 1 024 measures, roughly half an hour at 128 BPM. Matches
 * `FILTER_BUBBLE_MAX_WIDTH_BEATS` in `src-tauri/src/timeline.rs`, which clamps
 * the width again before writing. Rust widens the spacing between persisted
 * samples past a few hundred beats, so a long sweep is no more expensive to
 * store than a short one.
 */
export const FILTER_BUBBLE_MAX_WIDTH_BEATS = 4_096;
export const FILTER_BUBBLE_DEFAULT_WIDTH_BEATS = 8;

/**
 * Beats between the edge of a bubble and the bypass sample that closes it.
 * Rust writes that sample at the same distance in `draw_filter_bubble`, so the
 * drawn preview and the persisted automation describe the same curve.
 */
export const FILTER_BUBBLE_BYPASS_EPSILON_BEATS = 0.01;

const FILTER_BUBBLE_SAMPLES = 32;

/**
 * Le nombre de quarts de temps qu'un seul trait peut peindre, tel que Rust
 * l'accepte (`FILTER_BUBBLE_MAX_SAMPLES`).
 */
export const FILTER_STROKE_MAX_SAMPLES = 512;

/**
 * Le trait libre, prêt à être dessiné comme à être écrit.
 *
 * Les quarts peints sont remis dans l'ordre du temps — la main peut revenir en
 * arrière, corriger, repasser — et le trait est refermé par deux échantillons
 * au bypass, un centième de temps de part et d'autre. C'est la convention de
 * cette bande depuis le pinceau : sans eux la courbe rampe depuis le nœud
 * d'avant, à des mesures de là, et repart vers le suivant.
 *
 * Une seule fonction produit l'aperçu et la charge envoyée, de sorte que la
 * courbe montrée pendant le geste est exactement celle qui sera jouée.
 */
export function filterStrokeNodes(painted: Map<number, number>): [number, number][] {
  const beats = [...painted.keys()].sort((left, right) => left - right);
  if (beats.length === 0) return [];

  const nodes: [number, number][] = [];
  const first = beats[0];
  if (first >= FILTER_BUBBLE_BYPASS_EPSILON_BEATS) {
    nodes.push([first - FILTER_BUBBLE_BYPASS_EPSILON_BEATS, 0]);
  }
  for (const beat of beats.slice(0, FILTER_STROKE_MAX_SAMPLES - 2)) {
    nodes.push([beat, painted.get(beat) ?? 0]);
  }
  nodes.push([nodes[nodes.length - 1][0] + FILTER_BUBBLE_BYPASS_EPSILON_BEATS, 0]);
  return nodes;
}

export interface FilterBubbleShapeSource {
  startBeat: number;
  widthBeats: number;
  value: number;
  shape?: FilterBubbleShape;
}

export interface FilterBubbleSample {
  beat: number;
  value: number;
}

/**
 * Fraction of the bubble value reached at `progress` along its width.
 * Mirrors the `shape_multiplier` match in `draw_filter_bubble` (Rust).
 */
export function filterShapeMultiplier(shape: FilterBubbleShape, progress: number): number {
  if (shape === "ramp_down") {
    return 1 - progress;
  }
  if (shape === "triangle") {
    return 1 - Math.abs(2 * progress - 1);
  }
  return progress;
}

/** Samples a Filter Brush bubble the way Rust persists it. */
export function filterBubblePoints(source: FilterBubbleShapeSource): FilterBubbleSample[] {
  const startBeat = source.startBeat;
  const endBeat = source.startBeat + source.widthBeats;
  const shape = source.shape ?? "ramp_up";

  const points: FilterBubbleSample[] = Array.from(
    { length: FILTER_BUBBLE_SAMPLES + 1 },
    (_, index) => {
      const progress = index / FILTER_BUBBLE_SAMPLES;
      return {
        beat: startBeat + (endBeat - startBeat) * progress,
        value: source.value * filterShapeMultiplier(shape, progress),
      };
    },
  );

  // A ramp ends away from bypass, so it needs an explicit sample bringing the
  // band back. A triangle already returns to zero on both of its own edges.
  if (shape === "ramp_up") {
    points.push({ beat: endBeat + FILTER_BUBBLE_BYPASS_EPSILON_BEATS, value: 0 });
  } else if (shape === "ramp_down") {
    points.unshift({
      beat: Math.max(0, startBeat - FILTER_BUBBLE_BYPASS_EPSILON_BEATS),
      value: 0,
    });
  }

  return points;
}
