import { describe, expect, it } from "vitest";

import { SPLIT_EDGE_MARGIN_BEATS, clipToSplit, type LaneClip } from "./laneTarget";
import { resolveLaneShortcut, resolveViewShortcut } from "./timelineShortcut";

const clips: LaneClip[] = [
  { id: 10, lane: 1, visualStartBeat: 0, visualEndBeat: 64 },
  { id: 11, lane: 0, visualStartBeat: 0, visualEndBeat: 64 },
  { id: 12, lane: 2, visualStartBeat: 32, visualEndBeat: 96 },
  { id: 13, lane: 0, visualStartBeat: 80, visualEndBeat: 128 },
];

describe("clipToSplit", () => {
  it("cuts the lane the user pointed at, not whichever clip came back first", () => {
    // The regression: at beat 40 all three lanes are playing, and the snapshot
    // happens to list lane B first. Every lane must still cut its own clip.
    expect(clipToSplit(clips, 0, 40)?.id).toBe(11);
    expect(clipToSplit(clips, 1, 40)?.id).toBe(10);
    expect(clipToSplit(clips, 2, 40)?.id).toBe(12);
  });

  it("finds nothing in a lane that is empty at the playhead", () => {
    expect(clipToSplit(clips, 1, 90)).toBeNull();
    expect(clipToSplit(clips, 2, 10)).toBeNull();
    // Lane A has a gap between its two clips.
    expect(clipToSplit(clips, 0, 72)).toBeNull();
  });

  it("picks the right clip when a lane holds several", () => {
    expect(clipToSplit(clips, 0, 100)?.id).toBe(13);
  });

  it("refuses the edges, where a cut would leave a piece of no length", () => {
    expect(clipToSplit(clips, 0, 0)).toBeNull();
    expect(clipToSplit(clips, 0, 64)).toBeNull();
    expect(clipToSplit(clips, 0, SPLIT_EDGE_MARGIN_BEATS / 2)).toBeNull();
    expect(clipToSplit(clips, 0, SPLIT_EDGE_MARGIN_BEATS * 2)?.id).toBe(11);
  });
});

describe("resolveLaneShortcut", () => {
  const plain = { shift: false, ctrl: false, alt: false, meta: false };
  const shift = { ...plain, shift: true };

  it("maps the five lane actions", () => {
    expect(resolveLaneShortcut("b", plain)).toBe("split");
    expect(resolveLaneShortcut("B", shift)).toBe("split");
    expect(resolveLaneShortcut("v", plain)).toBe("volume");
    expect(resolveLaneShortcut("p", plain)).toBe("pan");
    expect(resolveLaneShortcut("S", shift)).toBe("solo");
    expect(resolveLaneShortcut("M", shift)).toBe("mute");
  });

  it("leaves the unshifted letters alone, so they stay typeable", () => {
    expect(resolveLaneShortcut("s", plain)).toBeNull();
    expect(resolveLaneShortcut("m", plain)).toBeNull();
  });

  it("gives the key back when another modifier is held", () => {
    for (const held of ["ctrl", "alt", "meta"] as const) {
      expect(resolveLaneShortcut("b", { ...plain, [held]: true })).toBeNull();
      expect(resolveLaneShortcut("v", { ...plain, [held]: true })).toBeNull();
      expect(resolveLaneShortcut("s", { ...shift, [held]: true })).toBeNull();
      expect(resolveLaneShortcut("m", { ...shift, [held]: true })).toBeNull();
    }
  });

  it("never fires while text is being typed", () => {
    expect(resolveLaneShortcut("b", plain, "INPUT")).toBeNull();
    expect(resolveLaneShortcut("v", plain, "INPUT")).toBeNull();
    expect(resolveLaneShortcut("S", shift, "TEXTAREA")).toBeNull();
    expect(resolveLaneShortcut("M", shift, undefined, true)).toBeNull();
  });

  it("ignores keys it does not own", () => {
    expect(resolveLaneShortcut("k", shift)).toBeNull();
    expect(resolveLaneShortcut("Enter", plain)).toBeNull();
  });
});

describe("resolveViewShortcut", () => {
  const plain = { shift: false, ctrl: false, alt: false, meta: false };
  const shift = { ...plain, shift: true };

  it("maps the three keycaps of the view rail", () => {
    expect(resolveViewShortcut("e", plain)).toBe("view");
    expect(resolveViewShortcut("s", plain)).toBe("shape");
    expect(resolveViewShortcut("d", plain)).toBe("period");
    expect(resolveViewShortcut("E", plain)).toBe("view");
  });

  it("leaves Shift+S to the solo of a track", () => {
    // Deux actions sur une frappe seraient impossibles à défaire de tête.
    expect(resolveViewShortcut("s", shift)).toBeNull();
    expect(resolveLaneShortcut("s", plain)).toBeNull();
  });

  it("gives the key back when another modifier is held", () => {
    for (const held of ["ctrl", "alt", "meta"] as const) {
      expect(resolveViewShortcut("e", { ...plain, [held]: true })).toBeNull();
      expect(resolveViewShortcut("d", { ...plain, [held]: true })).toBeNull();
    }
  });

  it("never fires while text is being typed", () => {
    expect(resolveViewShortcut("e", plain, "INPUT")).toBeNull();
    expect(resolveViewShortcut("s", plain, "TEXTAREA")).toBeNull();
    expect(resolveViewShortcut("d", plain, undefined, true)).toBeNull();
  });

  it("ignores keys it does not own", () => {
    expect(resolveViewShortcut("k", plain)).toBeNull();
    expect(resolveViewShortcut("Escape", plain)).toBeNull();
  });
});
