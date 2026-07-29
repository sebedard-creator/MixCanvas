import {
  FILTER_BUBBLE_MAX_WIDTH_BEATS,
  FILTER_BUBBLE_MIN_WIDTH_BEATS,
} from "./filterShape";

export interface FilterCurveBounds {
  startBeat: number;
  endBeat: number;
}

export interface FilterResizeLimits {
  /** Earliest beat the left edge may reach, set by the previous curve. */
  limitStartBeat: number;
  /** Latest beat the right edge may reach, set by the next curve. */
  limitEndBeat: number;
}

/**
 * New bounds for a curve whose right edge is being dragged to `pointerBeat`,
 * with its left edge held at `startBeat`.
 *
 * The result never inverts, never falls under the minimum width, never exceeds
 * the maximum, and never crosses into the next curve.
 */
export function resizeFilterCurveEnd(
  startBeat: number,
  pointerBeat: number,
  limits: FilterResizeLimits,
): FilterCurveBounds {
  const maximumEnd = Math.min(startBeat + FILTER_BUBBLE_MAX_WIDTH_BEATS, limits.limitEndBeat);
  const endBeat = Math.min(
    maximumEnd,
    Math.max(startBeat + FILTER_BUBBLE_MIN_WIDTH_BEATS, pointerBeat),
  );
  return { startBeat, endBeat };
}

/**
 * New bounds for a curve whose left edge is being dragged to `pointerBeat`,
 * with its right edge held at `endBeat`. Same guarantees, mirrored.
 */
export function resizeFilterCurveStart(
  endBeat: number,
  pointerBeat: number,
  limits: FilterResizeLimits,
): FilterCurveBounds {
  const minimumStart = Math.max(
    endBeat - FILTER_BUBBLE_MAX_WIDTH_BEATS,
    limits.limitStartBeat,
    0,
  );
  const startBeat = Math.max(
    minimumStart,
    Math.min(endBeat - FILTER_BUBBLE_MIN_WIDTH_BEATS, pointerBeat),
  );
  return { startBeat, endBeat };
}

/**
 * Limits a curve's edges from its neighbours on the same lane.
 * `curves` must be sorted by position, as the lane scan produces them.
 */
export function filterResizeLimits(
  curves: FilterCurveBounds[],
  index: number,
): FilterResizeLimits {
  return {
    limitStartBeat: index > 0 ? curves[index - 1].endBeat : 0,
    limitEndBeat: index < curves.length - 1 ? curves[index + 1].startBeat : Number.POSITIVE_INFINITY,
  };
}

/** Snaps a beat onto the quarter-beat grid the engine persists. */
export function snapFilterBeat(beat: number): number {
  return Math.max(0, Math.round(beat * 4) / 4);
}
