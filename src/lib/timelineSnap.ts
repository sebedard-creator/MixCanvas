export const BEATS_PER_MEASURE = 4;

export function snapTimelineBeat(requestedBeat: number, minimumBeat = 0): number {
  const safeMinimum = Number.isFinite(minimumBeat) ? Math.max(0, minimumBeat) : 0;
  const minimumMeasure = Math.ceil(safeMinimum / BEATS_PER_MEASURE) * BEATS_PER_MEASURE;
  if (!Number.isFinite(requestedBeat)) {
    return minimumMeasure;
  }

  const nearestMeasure =
    Math.round(Math.max(0, requestedBeat) / BEATS_PER_MEASURE) * BEATS_PER_MEASURE;
  return Math.max(minimumMeasure, nearestMeasure);
}
