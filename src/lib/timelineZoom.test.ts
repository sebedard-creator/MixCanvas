import { describe, expect, it } from "vitest";

import {
  clampTimelineZoom,
  minimumTimelineZoom,
  scrollLeftCenteringBeat,
  scrollLeftFollowingBeat,
  TIMELINE_FIT_RATIO,
  timelineContentLayout,
  isZoomGestureBurst,
  timelineZoomAnchorPx,
  visibleMeasures,
  zoomPreviewNeedsCommit,
  zoomPreviewScale,
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

describe("isZoomGestureBurst", () => {
  it("treats a lone notch as its own gesture, rendered at once", () => {
    // C'est le cas rapporté : « un cran » — il ne doit jamais passer par la
    // paire étirer-puis-poser, donc jamais par ses artefacts.
    expect(isZoomGestureBurst(1_000, 0, false)).toBe(false);
    expect(isZoomGestureBurst(1_000, 900, false)).toBe(true);
    // Un aperçu encore en vol garde le geste ouvert, quel que soit l'écart.
    expect(isZoomGestureBurst(9_999, 0, true)).toBe(true);
  });
});

describe("zoom preview", () => {
  it("stretches within bounds and commits beyond them", () => {
    expect(zoomPreviewScale(16, 20)).toBeCloseTo(1.25);
    expect(zoomPreviewNeedsCommit(zoomPreviewScale(16, 20))).toBe(false);
    // Au-delà du double ou de la moitié, l'étirement se verrait trop : on rend.
    expect(zoomPreviewNeedsCommit(2.01)).toBe(true);
    expect(zoomPreviewNeedsCommit(0.49)).toBe(true);
    expect(zoomPreviewNeedsCommit(2)).toBe(false);
  });

  it("keeps the anchored beat exactly where the settled render will put it", () => {
    // C'est l'invariant qui empêche l'image de sauter au moment du rendu net :
    // le point fixe de l'étirement doit être celui que la mise en page nette
    // garde immobile — le temps affiché, épinglé au centre de la fenêtre.
    const viewport = 1280;
    for (const [displayBeat, committed, pending] of [
      [64, 16, 24],
      [512, 8, 4.31],
      [3, 96, 55],
    ] as const) {
      const contentWidth = 4_096 * committed;
      const layout = timelineContentLayout(displayBeat, committed, contentWidth, viewport);
      const anchor = timelineZoomAnchorPx(displayBeat, committed, contentWidth, viewport);
      const scale = zoomPreviewScale(committed, pending);
      const translate = layout.paddingPx + layout.offsetPx;
      // Où l'ancre tombe à l'écran pendant l'étirement…
      const previewX = translate + anchor + scale * (anchor - anchor);
      // …et où le même temps tombera une fois rendu net au zoom visé.
      const settled = timelineContentLayout(displayBeat, pending, 4_096 * pending, viewport);
      const settledX = settled.paddingPx + settled.offsetPx + displayBeat * pending;
      expect(previewX).toBeCloseTo(settledX, 6);
      expect(previewX).toBeCloseTo(viewport / 2, 6);
    }
  });

  it("anchors a project that fits on its middle", () => {
    expect(timelineZoomAnchorPx(10, 4, 800, 1280)).toBe(400);
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
