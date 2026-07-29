import { describe, expect, it } from "vitest";

import {
  VU_TOO_LOW_DB,
  vuDecibels,
  vuMeterPosition,
  vuNeedleAngle,
  vuPositionAtDecibels,
  vuSegmentZone,
} from "./vuMeter";

const SEGMENTS = 24;
const zones = [...Array(SEGMENTS)].map((_, index) => vuSegmentZone(index, SEGMENTS));

describe("vuSegmentZone", () => {

  it("keeps red for distortion alone — one lens, at the very end", () => {
    expect(zones.filter((zone) => zone === "clip")).toHaveLength(1);
    expect(zones[SEGMENTS - 1]).toBe("clip");
    expect(zones[SEGMENTS - 2]).toBe("safe");
  });

  it("runs cold, then working, then clipping, with no zone interleaved", () => {
    expect(zones.join(" ")).toBe(
      [...zones]
        .sort((a, b) => ["low", "safe", "clip"].indexOf(a) - ["low", "safe", "clip"].indexOf(b))
        .join(" "),
    );
  });

  it("puts the cold/working boundary where the decibel threshold says", () => {
    const boundary = zones.indexOf("safe");
    // The last cold lens must light at or below the threshold, the first
    // working one above it. That is what ties the colours to a level rather
    // than to a lens count.
    const threshold = vuPositionAtDecibels(VU_TOO_LOW_DB);
    expect(boundary / SEGMENTS).toBeLessThanOrEqual(threshold);
    expect((boundary + 1) / SEGMENTS).toBeGreaterThan(threshold);
  });

  it("holds its meaning if the meter is ever rebuilt with more lenses", () => {
    for (const count of [12, 24, 48]) {
      const rebuilt = [...Array(count)].map((_, index) => vuSegmentZone(index, count));
      expect(rebuilt.filter((zone) => zone === "clip")).toHaveLength(1);
      const share = rebuilt.filter((zone) => zone === "low").length / count;
      expect(share).toBeGreaterThan(0.15);
      expect(share).toBeLessThan(0.45);
    }
  });
});

describe("vuPositionAtDecibels", () => {
  it("agrees with the scale it is read from", () => {
    expect(vuPositionAtDecibels(-20)).toBeCloseTo(0, 8);
    expect(vuPositionAtDecibels(0)).toBeCloseTo(0.75, 8);
    expect(vuPositionAtDecibels(3)).toBeCloseTo(1, 8);
  });

  it("clamps outside the printed scale", () => {
    expect(vuPositionAtDecibels(-100)).toBe(0);
    expect(vuPositionAtDecibels(40)).toBe(1);
  });
});

describe("vuMeter", () => {
  it("places silence against the mechanical stop", () => {
    expect(vuDecibels(0)).toBe(-20);
    expect(vuNeedleAngle(0)).toBe(-48);
  });

  it("calibrates 0 VU to the nominal master level", () => {
    expect(vuDecibels(0.35)).toBeCloseTo(0, 8);
    expect(vuMeterPosition(0.35)).toBeCloseTo(0.75, 8);
    expect(vuNeedleAngle(0.35)).toBeCloseTo(24, 8);
  });

  it("moves monotonically and clamps hot signals", () => {
    expect(vuNeedleAngle(0.05)).toBeLessThan(vuNeedleAngle(0.2));
    expect(vuNeedleAngle(0.2)).toBeLessThan(vuNeedleAngle(0.8));
    expect(vuNeedleAngle(1)).toBe(48);
  });
});
