import { describe, expect, it } from "vitest";

import { calculateTapTempo, nextTapSeries } from "./tapTempo";

describe("calculateTapTempo", () => {
  it("detects a steady 120 BPM tap", () => {
    expect(calculateTapTempo([0, 500, 1_000, 1_500, 2_000])).toBe(120);
  });

  it("uses the median to resist small timing errors", () => {
    expect(calculateTapTempo([0, 510, 995, 1_508, 2_000])).toBeCloseTo(119.76, 2);
  });

  it("waits for at least two taps", () => {
    expect(calculateTapTempo([100])).toBeNull();
  });
});

describe("nextTapSeries", () => {
  it("resets after a long pause", () => {
    expect(nextTapSeries([0, 500, 1_000], 3_500)).toEqual([3_500]);
  });
});
