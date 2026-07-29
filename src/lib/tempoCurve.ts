import type { TimelineTempoPoint } from "../timeline/types";

export interface TempoCurveMarker extends TimelineTempoPoint {
  x: number;
  y: number;
}

export interface TempoCurveGeometry {
  path: string;
  markers: TempoCurveMarker[];
}

export function tempoBpmAtBeat(points: TimelineTempoPoint[], beat: number): number {
  const sorted = [...points].sort((left, right) => left.beat - right.beat);
  if (sorted.length === 0) return 120;
  const target = Math.max(0, beat);
  if (target <= sorted[0].beat) return sorted[0].bpm;
  for (let index = 0; index < sorted.length - 1; index += 1) {
    const start = sorted[index];
    const end = sorted[index + 1];
    if (target <= end.beat) {
      const progress = (target - start.beat) / Math.max(Number.EPSILON, end.beat - start.beat);
      return start.bpm + (end.bpm - start.bpm) * progress;
    }
  }
  return sorted[sorted.length - 1].bpm;
}

export function tempoSecondsAtBeat(points: TimelineTempoPoint[], beat: number): number {
  const sorted = [...points].sort((left, right) => left.beat - right.beat);
  if (sorted.length === 0) return Math.max(0, beat) * 0.5;
  const target = Math.max(0, beat);
  let seconds = 0;
  for (let index = 0; index < sorted.length - 1; index += 1) {
    const start = sorted[index];
    const end = sorted[index + 1];
    if (target <= start.beat) return seconds;
    const segmentEnd = Math.min(target, end.beat);
    const delta = segmentEnd - start.beat;
    const slope = (end.bpm - start.bpm) / Math.max(Number.EPSILON, end.beat - start.beat);
    seconds += Math.abs(slope) < 1e-9
      ? delta * 60 / start.bpm
      : 60 / slope * Math.log((start.bpm + slope * delta) / start.bpm);
    if (target <= end.beat) return seconds;
  }
  const last = sorted[sorted.length - 1];
  return seconds + Math.max(0, target - last.beat) * 60 / last.bpm;
}

export function tempoCurveGeometry(
  points: TimelineTempoPoint[],
  pixelsPerBeat: number,
  contentWidth: number,
  height = 34,
): TempoCurveGeometry {
  if (points.length === 0 || pixelsPerBeat <= 0 || contentWidth <= 0) {
    return { path: "", markers: [] };
  }

  const sorted = [...points].sort((left, right) => left.beat - right.beat);
  const minimum = Math.min(...sorted.map((point) => point.bpm));
  const maximum = Math.max(...sorted.map((point) => point.bpm));
  const range = Math.max(4, maximum - minimum);
  const middle = (minimum + maximum) / 2;
  const lower = middle - range / 2;
  const topPadding = 5;
  const bottomPadding = 5;
  const plotHeight = Math.max(1, height - topPadding - bottomPadding);
  const yForBpm = (bpm: number) => (
    topPadding + (1 - (bpm - lower) / range) * plotHeight
  );

  const markers = sorted.map((point) => ({
    ...point,
    x: Math.max(0, Math.min(contentWidth, point.beat * pixelsPerBeat)),
    y: yForBpm(point.bpm),
  }));
  const last = markers[markers.length - 1];
  const pathPoints = [
    ...markers.map((marker) => `${marker.x.toFixed(2)},${marker.y.toFixed(2)}`),
    `${contentWidth.toFixed(2)},${last.y.toFixed(2)}`,
  ];

  return {
    path: `M ${pathPoints.join(" L ")}`,
    markers,
  };
}
