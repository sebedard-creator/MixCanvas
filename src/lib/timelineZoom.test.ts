import { describe, expect, it } from "vitest";

import {
  clampTimelineZoom,
  minimumTimelineZoom,
  scrollLeftCenteringBeat,
  scrollLeftFollowingBeat,
  TIMELINE_FIT_RATIO,
  timelineContentLayout,
} from "./timelineZoom";

describe("minimumTimelineZoom", () => {
  it("leaves a gap at each end so the limit is visible", () => {
    const zoom = minimumTimelineZoom(900, 600);

    // Short of a perfect fit on purpose: the last notch of zoom out has to
    // read as a limit rather than as a jammed control.
    expect(600 * zoom).toBe(900 * TIMELINE_FIT_RATIO);
    expect(600 * zoom).toBeLessThan(900);
  });

  it("keeps a practical minimum for a short project", () => {
    expect(minimumTimelineZoom(900, 64)).toBe(4);
  });

  it("still fits a multi-hour project without an arbitrary zoom floor", () => {
    const zoom = minimumTimelineZoom(900, 12_000);

    expect(12_000 * zoom).toBeLessThan(900);
  });
});

describe("timelineContentLayout", () => {
  it("centres the whole project between two gaps once it fits", () => {
    // Zoomed all the way out, shifting the content to centre the playhead
    // would push half of it off screen. It is framed instead.
    expect(timelineContentLayout(300, 1.5, 600, 900)).toEqual({
      paddingPx: 0,
      offsetPx: 150,
    });
    // Exactly as wide as the window: nothing to frame, nothing to shift.
    expect(timelineContentLayout(0, 1.5, 900, 900)).toEqual({ paddingPx: 0, offsetPx: 0 });
  });

  it("frames a fully zoomed out project with equal gaps", () => {
    const viewportWidth = 900;
    const totalBeats = 600;
    const zoom = minimumTimelineZoom(viewportWidth, totalBeats);
    const contentWidth = totalBeats * zoom;
    const { paddingPx, offsetPx } = timelineContentLayout(0, zoom, contentWidth, viewportWidth);

    const gapLeft = paddingPx + offsetPx;
    const gapRight = viewportWidth - contentWidth - gapLeft;
    expect(gapLeft).toBeGreaterThan(0);
    expect(gapLeft).toBeCloseTo(gapRight, 10);
  });

  it("pins the playhead to the centre while the content overflows", () => {
    expect(timelineContentLayout(100, 5, 4_000, 900)).toEqual({
      paddingPx: 450,
      offsetPx: -500,
    });
  });

  it("puts the playhead exactly on the centre line", () => {
    const viewportWidth = 900;
    const pixelsPerBeat = 5;
    const beat = 100;
    const { paddingPx, offsetPx } = timelineContentLayout(
      beat,
      pixelsPerBeat,
      4_000,
      viewportWidth,
    );
    // The content's left edge, plus the beat's own position inside it.
    expect(paddingPx + offsetPx + beat * pixelsPerBeat).toBe(viewportWidth / 2);
  });

  it("falls back to the left edge when the viewport is not measured yet", () => {
    expect(timelineContentLayout(10, 5, 4_000, 0)).toEqual({ paddingPx: 0, offsetPx: 0 });
    expect(timelineContentLayout(Number.NaN, 5, 4_000, 900)).toEqual({
      paddingPx: 0,
      offsetPx: 0,
    });
  });
});

describe("timeline zoom anchoring", () => {
  it("recentre le playhead pendant le zoom", () => {
    const nextScroll = scrollLeftCenteringBeat(130, 600, 5, 400);

    expect(nextScroll).toBe(350);
    expect((nextScroll + 300) / 5).toBe(130);
  });

  it("respecte les deux extrémités de la timeline", () => {
    expect(scrollLeftCenteringBeat(0, 600, 5, 400)).toBe(0);
    expect(scrollLeftCenteringBeat(400, 600, 5, 400)).toBe(1_400);
  });

  it("clamps zoom to the calculated project fit", () => {
    expect(clampTimelineZoom(0.5, 1.5)).toBe(1.5);
    expect(clampTimelineZoom(200, 1.5)).toBe(96);
  });

  it("keeps a playing beat under the centre line with virtual side space", () => {
    expect(scrollLeftFollowingBeat(0, 5, 400)).toBe(0);
    expect(scrollLeftFollowingBeat(130, 5, 400)).toBe(650);
    expect(scrollLeftFollowingBeat(999, 5, 400)).toBe(2_000);
  });
});
