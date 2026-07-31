import { describe, expect, it } from "vitest";

import {
  MAXIMUM_DOWNBEAT_TAPS,
  appendDownbeatTap,
  estimateGridFromDownbeatTaps,
  hasExcellentTapAccuracy,
} from "./downbeatTap";

describe("estimateGridFromDownbeatTaps", () => {
  it("waits for four consecutive bar ones", () => {
    expect(estimateGridFromDownbeatTaps([1_000, 3_000, 5_000])).toBeNull();
  });

  it("derives BPM and the first downbeat from consecutive measures", () => {
    expect(estimateGridFromDownbeatTaps([1_000, 3_000, 5_000, 7_000])).toEqual({
      bpm: 120,
      firstBeatMs: 1_000,
      rmsErrorMs: 0,
      tapCount: 4,
    });
  });

  it("uses the whole span to absorb human timing errors", () => {
    const estimate = estimateGridFromDownbeatTaps([
      1_018, 2_887, 4_821, 6_706, 8_632, 10_516, 12_437, 14_326,
    ]);

    expect(estimate?.bpm).toBeCloseTo(126, 0);
    expect(estimate?.firstBeatMs).toBeCloseTo(1_000, -2);
    expect(estimate?.rmsErrorMs).toBeLessThan(30);
  });

  it("refuses a series where a whole measure was skipped", () => {
    expect(estimateGridFromDownbeatTaps([1_000, 3_000, 7_000, 9_000])).toBeNull();
  });
});

describe("appendDownbeatTap", () => {
  it("starts again after seeking backwards", () => {
    expect(appendDownbeatTap([1_000, 3_000, 5_000], 2_000)).toEqual([2_000]);
  });

  it("keeps a bounded long-baseline series", () => {
    const taps = Array.from({ length: MAXIMUM_DOWNBEAT_TAPS }, (_, index) => index * 2_000);
    expect(appendDownbeatTap(taps, MAXIMUM_DOWNBEAT_TAPS * 2_000)).toEqual(taps);
  });
});

describe("hasExcellentTapAccuracy", () => {
  it("turns green only below 20 ms and after the fourth measurement", () => {
    expect(hasExcellentTapAccuracy(3, 10)).toBe(false);
    expect(hasExcellentTapAccuracy(4, 19)).toBe(true);
    expect(hasExcellentTapAccuracy(4, 20)).toBe(false);
  });
});
