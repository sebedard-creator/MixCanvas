import { describe, expect, it } from "vitest";
import {
  filterResizeLimits,
  resizeFilterCurveEnd,
  resizeFilterCurveStart,
  snapFilterBeat,
} from "./filterResize";
import {
  FILTER_BUBBLE_MAX_WIDTH_BEATS,
  FILTER_BUBBLE_MIN_WIDTH_BEATS,
} from "./filterShape";

const OPEN = { limitStartBeat: 0, limitEndBeat: Number.POSITIVE_INFINITY };

describe("snapFilterBeat", () => {
  it("locks onto the quarter-beat grid the engine persists", () => {
    expect(snapFilterBeat(8.1)).toBe(8);
    expect(snapFilterBeat(8.2)).toBe(8.25);
    expect(snapFilterBeat(8.99)).toBe(9);
    expect(snapFilterBeat(-4)).toBe(0);
  });
});

describe("resizeFilterCurveEnd", () => {
  const curve = { startBeat: 16, endBeat: 32 };

  it("moves the right edge and leaves the left one alone", () => {
    expect(resizeFilterCurveEnd(curve.startBeat, 48, OPEN)).toEqual({ startBeat: 16, endBeat: 48 });
    expect(resizeFilterCurveEnd(curve.startBeat, 20, OPEN)).toEqual({ startBeat: 16, endBeat: 20 });
  });

  it("never lets the curve invert or fall under the minimum width", () => {
    expect(resizeFilterCurveEnd(curve.startBeat, 16, OPEN).endBeat).toBe(
      16 + FILTER_BUBBLE_MIN_WIDTH_BEATS,
    );
    expect(resizeFilterCurveEnd(curve.startBeat, -100, OPEN).endBeat).toBe(
      16 + FILTER_BUBBLE_MIN_WIDTH_BEATS,
    );
  });

  it("stops at the next curve instead of overwriting it", () => {
    const limits = { limitStartBeat: 0, limitEndBeat: 40 };
    expect(resizeFilterCurveEnd(curve.startBeat, 500, limits).endBeat).toBe(40);
  });

  it("respects the maximum width", () => {
    expect(resizeFilterCurveEnd(curve.startBeat, 1_000_000, OPEN).endBeat).toBe(
      16 + FILTER_BUBBLE_MAX_WIDTH_BEATS,
    );
  });
});

describe("resizeFilterCurveStart", () => {
  const curve = { startBeat: 16, endBeat: 32 };

  it("moves the left edge and leaves the right one alone", () => {
    expect(resizeFilterCurveStart(curve.endBeat, 4, OPEN)).toEqual({ startBeat: 4, endBeat: 32 });
    expect(resizeFilterCurveStart(curve.endBeat, 24, OPEN)).toEqual({ startBeat: 24, endBeat: 32 });
  });

  it("never lets the curve invert or fall under the minimum width", () => {
    expect(resizeFilterCurveStart(curve.endBeat, 32, OPEN).startBeat).toBe(
      32 - FILTER_BUBBLE_MIN_WIDTH_BEATS,
    );
    expect(resizeFilterCurveStart(curve.endBeat, 999, OPEN).startBeat).toBe(
      32 - FILTER_BUBBLE_MIN_WIDTH_BEATS,
    );
  });

  it("stops at the previous curve and at the start of the project", () => {
    expect(resizeFilterCurveStart(curve.endBeat, 0, { ...OPEN, limitStartBeat: 8 }).startBeat).toBe(8);
    expect(resizeFilterCurveStart(curve.endBeat, -50, OPEN).startBeat).toBe(0);
  });

  it("respects the maximum width", () => {
    const long = { startBeat: 0, endBeat: FILTER_BUBBLE_MAX_WIDTH_BEATS + 100 };
    expect(resizeFilterCurveStart(long.endBeat, 0, OPEN).startBeat).toBe(100);
  });
});

describe("filterResizeLimits", () => {
  const curves = [
    { startBeat: 0, endBeat: 8 },
    { startBeat: 16, endBeat: 24 },
    { startBeat: 32, endBeat: 40 },
  ];

  it("bounds a middle curve by both of its neighbours", () => {
    expect(filterResizeLimits(curves, 1)).toEqual({ limitStartBeat: 8, limitEndBeat: 32 });
  });

  it("leaves the outer edges of the first and last curves free", () => {
    expect(filterResizeLimits(curves, 0)).toEqual({ limitStartBeat: 0, limitEndBeat: 16 });
    expect(filterResizeLimits(curves, 2)).toEqual({
      limitStartBeat: 24,
      limitEndBeat: Number.POSITIVE_INFINITY,
    });
  });

  it("leaves a lone curve unbounded", () => {
    expect(filterResizeLimits([curves[0]], 0)).toEqual({
      limitStartBeat: 0,
      limitEndBeat: Number.POSITIVE_INFINITY,
    });
  });
});
