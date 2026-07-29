import { describe, expect, it } from "vitest";

import { tempoBpmAtBeat, tempoCurveGeometry, tempoSecondsAtBeat } from "./tempoCurve";

describe("tempoCurveGeometry", () => {
  it("draws a gradual ramp between tempo targets and holds after the last one", () => {
    const geometry = tempoCurveGeometry(
      [
        { beat: 0, bpm: 120, clipId: null },
        { beat: 16, bpm: 128, clipId: 2 },
      ],
      10,
      320,
    );

    expect(geometry.markers.map(({ x, bpm }) => ({ x, bpm }))).toEqual([
      { x: 0, bpm: 120 },
      { x: 160, bpm: 128 },
    ]);
    expect(geometry.path).toContain("L 160.00,5.00 L 320.00,5.00");
  });

  it("keeps a constant tempo centered when all targets match", () => {
    const geometry = tempoCurveGeometry(
      [{ beat: 0, bpm: 124, clipId: null }],
      10,
      100,
    );

    expect(geometry.markers[0].y).toBe(17);
    expect(geometry.path).toBe("M 0.00,17.00 L 100.00,17.00");
  });
});

describe("tempo readout math", () => {
  const ramp = [
    { beat: 0, bpm: 120, clipId: null },
    { beat: 16, bpm: 128, clipId: 2 },
  ];

  it("reports the current interpolated BPM", () => {
    expect(tempoBpmAtBeat(ramp, 8)).toBe(124);
    expect(tempoBpmAtBeat(ramp, 24)).toBe(128);
  });

  it("integrates the exact duration of a linear BPM ramp", () => {
    const expected = 60 / 0.5 * Math.log(128 / 120);
    expect(tempoSecondsAtBeat(ramp, 16)).toBeCloseTo(expected, 9);
  });
});
