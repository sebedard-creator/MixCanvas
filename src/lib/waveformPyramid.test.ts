import { describe, expect, it } from "vitest";

import { buildWaveformPyramid, selectWaveformLevel } from "./waveformPyramid";
import type { WaveformPeaks } from "../timeline/types";

function waveform(length: number): WaveformPeaks {
  const values = Array.from({ length }, (_, index) => index / length);
  return {
    leftMin: values.map((value) => -value),
    leftMax: values,
    leftRms: values.map((value) => value / 2),
    rightMin: values.map((value) => -value / 2),
    rightMax: values.map((value) => value / 2),
    rightRms: values.map((value) => value / 4),
  };
}

describe("waveform pyramid", () => {
  it("keeps extrema and combines RMS energy at each level", () => {
    const source: WaveformPeaks = {
      leftMin: [-0.2, -0.8],
      leftMax: [0.4, 0.7],
      leftRms: [0.3, 0.5],
      rightMin: [-0.6, -0.1],
      rightMax: [0.2, 0.9],
      rightRms: [0.4, 0.2],
    };
    const levels = buildWaveformPyramid({
      ...source,
      leftMin: [...source.leftMin, ...Array(127).fill(-0.1)],
      leftMax: [...source.leftMax, ...Array(127).fill(0.1)],
      leftRms: [...source.leftRms, ...Array(127).fill(0.05)],
      rightMin: [...source.rightMin, ...Array(127).fill(-0.1)],
      rightMax: [...source.rightMax, ...Array(127).fill(0.1)],
      rightRms: [...source.rightRms, ...Array(127).fill(0.05)],
    });

    expect(levels[1].leftMin[0]).toBe(-0.8);
    expect(levels[1].leftMax[0]).toBe(0.7);
    expect(levels[1].leftRms[0]).toBeCloseTo(Math.sqrt(0.17), 8);
    expect(levels[1].rightRms[0]).toBeCloseTo(Math.sqrt(0.1), 8);
  });

  it("chooses the lightest level that still covers the rendered width", () => {
    const levels = buildWaveformPyramid(waveform(1_024));

    expect(selectWaveformLevel(levels, 700)?.leftMin).toHaveLength(1_024);
    expect(selectWaveformLevel(levels, 300)?.leftMin).toHaveLength(512);
    expect(selectWaveformLevel(levels, 100)?.leftMin).toHaveLength(128);
  });
});
