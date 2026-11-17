import type { WaveformPeaks } from "../timeline/types";

/** Preserve peaks while placing native-speed audio on a changing musical grid. */
export function warpWaveform(waveform: WaveformPeaks, sourceProgressAt: (progress: number) => number): WaveformPeaks {
  const count = waveform.leftMin.length;
  if (!count) return waveform;
  const keys = ["leftMin", "leftMax", "leftRms", "rightMin", "rightMax", "rightRms"] as const;
  const result = Object.fromEntries(keys.map((key) => [key, []])) as unknown as WaveformPeaks;
  for (let index = 0; index < count; index++) {
    const from = Math.max(0, Math.min(count - 1, Math.floor(sourceProgressAt(index / count) * count)));
    const to = Math.max(from + 1, Math.min(count, Math.ceil(sourceProgressAt((index + 1) / count) * count)));
    for (const key of keys) {
      let value = key.endsWith("Min") ? Infinity : key.endsWith("Max") ? -Infinity : 0;
      for (let source = from; source < to; source++) {
        const sample = waveform[key][source];
        value = key.endsWith("Min") ? Math.min(value, sample) : key.endsWith("Max") ? Math.max(value, sample) : value + sample * sample;
      }
      result[key].push(key.endsWith("Rms") ? Math.sqrt(value / (to - from)) : value);
    }
  }
  return result;
}
