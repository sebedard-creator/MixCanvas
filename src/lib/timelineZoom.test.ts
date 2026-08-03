import { describe, expect, it } from "vitest";

import {
  clampTimelineZoom,
  minimumTimelineZoom,
  scrollLeftCenteringBeat,
  scrollLeftFollowingBeat,
  timelineSeekBeat,
  TIMELINE_FIT_RATIO,
  timelineContentLayout,
  visibleMeasures,
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

describe("timeline click seeking", () => {
  it("uses the lane's local coordinate in an overflowing, centred timeline", () => {
    // At beat 100, `scrollLeft` is 500 px and virtual padding is 450 px, so
    // the lane starts at -50 px in the viewport. Its local x 500 is beat 100.
    expect(timelineSeekBeat(450, -50, 5)).toBe(100);
  });

  it("uses ordinary local coordinates while the full project fits", () => {
    expect(timelineSeekBeat(300, 100, 5)).toBe(40);
  });
});

describe("visibleMeasures", () => {
  it("renders every marker while the project fits the window", () => {
    const measures = visibleMeasures(0, 4, 1280, 64, 1, 256);
    expect(measures[0]).toBe(0);
    expect(measures[measures.length - 1]).toBe(16);
    expect(measures).toHaveLength(17);
  });

  it("renders a window, not the whole two-hour set", () => {
    // 18 000 temps ≈ deux heures à 128 BPM : 4 500 mesures, dont une trentaine
    // tombent dans la fenêtre. Le reste était mis en page pour rien, à chaque
    // rendu — le gros du coût qui faisait strober le zoom.
    const measures = visibleMeasures(9_000, 16, 1_280, 18_000, 1, 288_000);
    expect(measures.length).toBeLessThan(30);
    const beats = measures.map((measure) => measure * 4);
    expect(Math.min(...beats)).toBeLessThanOrEqual(9_000 - 40);
    expect(Math.max(...beats)).toBeGreaterThanOrEqual(9_000 + 40);
    expect(Math.min(...beats)).toBeGreaterThan(8_800);
    expect(Math.max(...beats)).toBeLessThan(9_200);
  });

  it("clamps at both ends and keeps stride multiples through a slide", () => {
    const atStart = visibleMeasures(0, 16, 1_280, 18_000, 4, 288_000);
    expect(atStart[0]).toBe(0);
    for (const measure of atStart) {
      expect(measure % 4).toBe(0);
    }
    // Une fenêtre qui glisse d'un temps montre les mêmes marqueurs alignés,
    // pas des marqueurs décalés d'un cran qui scintilleraient.
    const slid = visibleMeasures(1, 16, 1_280, 18_000, 4, 288_000);
    for (const measure of slid) {
      expect(measure % 4).toBe(0);
    }
    const atEnd = visibleMeasures(18_000, 16, 1_280, 18_000, 1, 288_000);
    expect(atEnd[atEnd.length - 1]).toBe(4_500);
  });
});
