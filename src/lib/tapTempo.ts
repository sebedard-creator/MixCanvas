const TAP_RESET_DELAY_MS = 2_000;
const MAX_TAPS = 9;

export function nextTapSeries(current: number[], timestampMs: number): number[] {
  const lastTap = current.at(-1);
  if (lastTap === undefined || timestampMs - lastTap > TAP_RESET_DELAY_MS) {
    return [timestampMs];
  }

  return [...current, timestampMs].slice(-MAX_TAPS);
}

export function calculateTapTempo(timestampsMs: number[]): number | null {
  if (timestampsMs.length < 2) {
    return null;
  }

  const intervals = timestampsMs
    .slice(1)
    .map((timestamp, index) => timestamp - timestampsMs[index])
    .filter((interval) => interval >= 150 && interval <= TAP_RESET_DELAY_MS)
    .sort((left, right) => left - right);

  if (intervals.length === 0) {
    return null;
  }

  const middle = Math.floor(intervals.length / 2);
  const medianInterval =
    intervals.length % 2 === 0
      ? (intervals[middle - 1] + intervals[middle]) / 2
      : intervals[middle];

  return Math.round((60_000 / medianInterval) * 100) / 100;
}
