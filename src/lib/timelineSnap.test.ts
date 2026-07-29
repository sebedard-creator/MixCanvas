import { describe, expect, it } from "vitest";

import { snapTimelineBeat } from "./timelineSnap";

describe("snapTimelineBeat", () => {
  it("snaps the source downbeat to four-beat measure boundaries", () => {
    expect(snapTimelineBeat(5.9)).toBe(4);
    expect(snapTimelineBeat(6)).toBe(8);
    expect(snapTimelineBeat(13.2)).toBe(12);
  });

  it("moves to the next complete measure when the pre-roll needs room", () => {
    expect(snapTimelineBeat(0.2, 0.8)).toBe(4);
    expect(snapTimelineBeat(-12, 8.1)).toBe(12);
  });

  it("handles invalid pointer values without escaping the timeline", () => {
    expect(snapTimelineBeat(Number.NaN, 1)).toBe(4);
  });
});
