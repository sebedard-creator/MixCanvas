import type { WaveformPeaks } from "../timeline/types";

const MIN_WAVEFORM_LEVEL_BUCKETS = 128;

function reduceExtrema(values: number[], choose: (left: number, right: number) => number) {
  const reduced: number[] = [];
  for (let index = 0; index < values.length; index += 2) {
    const left = Number.isFinite(values[index]) ? values[index] : 0;
    const rightValue = values[index + 1];
    const right = Number.isFinite(rightValue) ? rightValue : left;
    reduced.push(choose(left, right));
  }
  return reduced;
}

function reduceRms(values: number[]) {
  const reduced: number[] = [];
  for (let index = 0; index < values.length; index += 2) {
    const left = Number.isFinite(values[index]) ? values[index] : 0;
    const hasRight = index + 1 < values.length;
    const rightValue = values[index + 1];
    const right = Number.isFinite(rightValue) ? rightValue : left;
    const divisor = hasRight ? 2 : 1;
    reduced.push(Math.sqrt((left * left + (hasRight ? right * right : 0)) / divisor));
  }
  return reduced;
}

function reduceWaveformLevel(level: WaveformPeaks): WaveformPeaks {
  return {
    leftMin: reduceExtrema(level.leftMin, Math.min),
    leftMax: reduceExtrema(level.leftMax, Math.max),
    leftRms: reduceRms(level.leftRms),
    rightMin: reduceExtrema(level.rightMin, Math.min),
    rightMax: reduceExtrema(level.rightMax, Math.max),
    rightRms: reduceRms(level.rightRms),
  };
}

export function buildWaveformPyramid(waveform: WaveformPeaks): WaveformPeaks[] {
  const levels = [waveform];
  let current = waveform;
  while (current.leftMin.length > MIN_WAVEFORM_LEVEL_BUCKETS) {
    current = reduceWaveformLevel(current);
    levels.push(current);
  }
  return levels;
}

export function selectWaveformLevel(
  levels: WaveformPeaks[],
  targetColumns: number,
): WaveformPeaks | null {
  if (levels.length === 0) {
    return null;
  }
  const target = Math.max(1, Math.ceil(targetColumns));
  let selected = levels[0];
  for (const level of levels) {
    if (level.leftMin.length < target) {
      break;
    }
    selected = level;
  }
  return selected;
}
