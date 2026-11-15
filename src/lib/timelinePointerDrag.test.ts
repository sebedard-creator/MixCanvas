import { describe, expect, it } from "vitest";

import {
  pointerMovedEnoughToDrag,
  resolveTimelinePointerDrop,
  type TimelineDropGeometry,
} from "./timelinePointerDrag";

const GEOMETRY: TimelineDropGeometry = {
  contentLeft: -300,
  viewportLeft: 100,
  viewportRight: 900,
  viewportTop: 200,
  viewportBottom: 500,
  top: 200,
  height: 400,
};

describe("timeline pointer drag", () => {
  it("targets D after vertical scrolling and rejects hidden tracks", () => {
    const scrolled = { ...GEOMETRY, top: -200, height: 720, viewportTop: 100 };
    expect(resolveTimelinePointerDrop(500, 450, scrolled, 20)).toEqual({ anchorBeat: 40, lane: 3 });
    expect(resolveTimelinePointerDrop(500, 99, scrolled, 20)).toBeNull();
  });
  it("keeps a short pointer movement as a normal click", () => {
    expect(pointerMovedEnoughToDrag(10, 10, 14, 13)).toBe(false);
    expect(pointerMovedEnoughToDrag(10, 10, 16, 10)).toBe(true);
  });

  it("resolves the lane and snapped measure under the pointer", () => {
    expect(resolveTimelinePointerDrop(500, 350, GEOMETRY, 20)).toEqual({
      anchorBeat: 40,
      lane: 1,
    });
  });

  it("rejects points outside the visible timeline viewport", () => {
    expect(resolveTimelinePointerDrop(950, 350, GEOMETRY, 20)).toBeNull();
    expect(resolveTimelinePointerDrop(500, 550, GEOMETRY, 20)).toBeNull();
  });
});
