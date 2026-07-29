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
  top: 200,
  height: 300,
};

describe("timeline pointer drag", () => {
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
