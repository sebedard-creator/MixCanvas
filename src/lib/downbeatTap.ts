export const MINIMUM_DOWNBEAT_TAPS = 4;
export const RECOMMENDED_DOWNBEAT_TAPS = 8;
export const MAXIMUM_DOWNBEAT_TAPS = 16;

export interface DownbeatTapEstimate {
  bpm: number;
  firstBeatMs: number;
  rmsErrorMs: number;
  tapCount: number;
}

export function hasExcellentTapAccuracy(tapCount: number, rmsErrorMs: number): boolean {
  return (
    tapCount >= MINIMUM_DOWNBEAT_TAPS
    && Number.isFinite(rmsErrorMs)
    && rmsErrorMs < 20
  );
}

/**
 * Adds a source-audio position to a series of consecutive bar downbeats.
 *
 * Seeking backwards starts a new series automatically. Sixteen taps bound the
 * UI state while retaining a long enough span for a very accurate
 * constant-tempo estimate; later presses leave that completed series intact.
 */
export function appendDownbeatTap(current: number[], positionMs: number): number[] {
  if (!Number.isFinite(positionMs) || positionMs < 0) {
    return current;
  }

  const last = current.at(-1);
  if (last !== undefined && positionMs <= last) {
    return [positionMs];
  }

  if (current.length >= MAXIMUM_DOWNBEAT_TAPS) {
    return current;
  }

  return [...current, positionMs];
}

/**
 * Fits one rigid 4/4 grid through bar-one taps recorded in source time.
 *
 * Tap i represents beat i × 4. Linear regression uses every tap instead of
 * trusting only two clicks: timing errors are shared across the whole span,
 * and the intercept refines the first downbeat as well as the BPM.
 */
export function estimateGridFromDownbeatTaps(timestampsMs: number[]): DownbeatTapEstimate | null {
  if (timestampsMs.length < MINIMUM_DOWNBEAT_TAPS) {
    return null;
  }

  const intervals = timestampsMs.slice(1).map((timestamp, index) => timestamp - timestampsMs[index]);
  if (intervals.some((interval) => !Number.isFinite(interval) || interval <= 0)) {
    return null;
  }

  // A missed bar is almost exactly twice the other intervals. Refuse that
  // ambiguous series instead of silently fitting the wrong BPM.
  const sortedIntervals = [...intervals].sort((left, right) => left - right);
  const middle = Math.floor(sortedIntervals.length / 2);
  const medianInterval =
    sortedIntervals.length % 2 === 0
      ? (sortedIntervals[middle - 1] + sortedIntervals[middle]) / 2
      : sortedIntervals[middle];
  if (intervals.some((interval) => interval < medianInterval * 0.6 || interval > medianInterval * 1.6)) {
    return null;
  }

  const count = timestampsMs.length;
  const meanBar = (count - 1) / 2;
  const meanTime = timestampsMs.reduce((sum, timestamp) => sum + timestamp, 0) / count;
  let covariance = 0;
  let barVariance = 0;
  for (let index = 0; index < count; index += 1) {
    const centeredBar = index - meanBar;
    covariance += centeredBar * (timestampsMs[index] - meanTime);
    barVariance += centeredBar * centeredBar;
  }

  const millisecondsPerBar = covariance / barVariance;
  const bpm = 240_000 / millisecondsPerBar;
  if (!Number.isFinite(bpm) || bpm < 40 || bpm > 300) {
    return null;
  }

  const firstBeatMs = meanTime - millisecondsPerBar * meanBar;
  if (!Number.isFinite(firstBeatMs) || firstBeatMs < 0) {
    return null;
  }

  const squaredError = timestampsMs.reduce((sum, timestamp, index) => {
    const fitted = firstBeatMs + millisecondsPerBar * index;
    return sum + (timestamp - fitted) ** 2;
  }, 0);

  return {
    bpm: Math.round(bpm * 1_000) / 1_000,
    firstBeatMs: Math.round(firstBeatMs),
    rmsErrorMs: Math.round(Math.sqrt(squaredError / count)),
    tapCount: count,
  };
}
