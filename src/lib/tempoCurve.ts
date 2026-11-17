import type { TimelineTempoPoint } from "../timeline/types";

export interface TempoCurveMarker extends TimelineTempoPoint {
  x: number;
  y: number;
}

export interface TempoCurveGeometry {
  path: string;
  markers: TempoCurveMarker[];
}

export interface TempoClock {
  projectBpm: number;
  points: TimelineTempoPoint[];
  secondsAtBeat: (beat: number) => number;
  beatAtSeconds: (seconds: number) => number;
}

/** Same beat-stepped clock as Rust TempoMap, including fractional beat positions. */
export function createTempoClock(projectBpm: number, points: readonly TimelineTempoPoint[]): TempoClock {
  const byBeat = new Map<number, TimelineTempoPoint>([[0, { beat: 0, bpm: projectBpm, clipId: null }]]);
  for (const point of points) byBeat.set(point.beat, point);
  const sorted = [...byBeat.values()].sort((a, b) => a.beat - b.beat);
  const bpmAt = (beat: number): number => {
    let low = 0, high = sorted.length;
    while (low < high) { const mid = (low + high) >>> 1; if (sorted[mid].beat <= beat) low = mid + 1; else high = mid; }
    const left = sorted[Math.max(0, low - 1)], right = sorted[low];
    return right ? left.bpm + (right.bpm - left.bpm) * (beat - left.beat) / (right.beat - left.beat) : left.bpm;
  };
  const elapsed = [0];
  const last = Math.min(100_000, Math.ceil(sorted[sorted.length - 1].beat));
  for (let beat = 0; beat < last; beat++) elapsed.push(elapsed[beat] + 60 / bpmAt(beat));
  return {
    projectBpm,
    points: sorted,
    secondsAtBeat(beat) {
      if (!Number.isFinite(beat)) return beat;
      if (beat < 0) return beat * 60 / bpmAt(0);
      const index = Math.min(Math.floor(beat), elapsed.length - 1);
      return elapsed[index] + (beat - index) * 60 / bpmAt(index);
    },
    beatAtSeconds(seconds) {
      if (!Number.isFinite(seconds)) return seconds;
      if (seconds < 0) return seconds * bpmAt(0) / 60;
      let low = 0, high = elapsed.length;
      while (low < high) { const mid = (low + high) >>> 1; if (elapsed[mid] <= seconds) low = mid + 1; else high = mid; }
      const index = Math.max(0, low - 1);
      return index + (seconds - elapsed[index]) * bpmAt(index) / 60;
    },
  };
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
