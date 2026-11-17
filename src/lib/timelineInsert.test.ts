import { describe, expect, it } from "vitest";
import type { LibraryTrack } from "../library/types";
import type { TimelineClip } from "../timeline/types";
import { resolveTimelineInsert, timelineDropConflict } from "./timelineInsert";

const track = { bpm: 120, firstBeatMs: 500, durationMs: 60_000, isMissing: false } as LibraryTrack;
const clips = [
  { id: 1, lane: 0, anchorBeat: 4, tempoAnchorBeat: 4, visualStartBeat: 3, visualEndBeat: 123 },
  { id: 2, lane: 0, anchorBeat: 128, tempoAnchorBeat: 132, visualStartBeat: 127, visualEndBeat: 247 },
  { id: 3, lane: 3, anchorBeat: 260, tempoAnchorBeat: 264, visualStartBeat: 259, visualEndBeat: 379 },
] as TimelineClip[];

describe("timeline insertion preview", () => {
  it("detects the whole occupied footprint, even when a snapped drop overlaps another clip", () => {
    for (const beat of [127, 180, 246.99]) {
      expect(timelineDropConflict(clips, track, 0, beat, Math.round(beat / 4) * 4)?.id).toBe(2);
    }
    expect(timelineDropConflict(clips, track, 0, 125, 124)?.id).toBe(2);
    expect(timelineDropConflict(clips, track, 1, 180, 180)).toBeNull();
  });

  it("keeps free space as a normal drop", () => {
    expect(timelineDropConflict(clips, track, 0, 400, 400)).toBeNull();
  });

  it("calculates before and after separately from the selected clip's footprint", () => {
    expect(resolveTimelineInsert(clips, track, { clipId: 2, position: "before" })).toMatchObject({
      clipId: 2, insertionBeat: 127, startBeat: 127, anchorBeat: 128, endBeat: 247, shiftBeats: 120,
    });
    expect(resolveTimelineInsert(clips, track, { clipId: 2, position: "after" })).toMatchObject({
      clipId: 2, insertionBeat: 247, startBeat: 247, anchorBeat: 248, endBeat: 367, shiftBeats: 120,
    });
  });

  it("reserves pre-roll and partial bars before a trimmed or looped clip", () => {
    const trimmed = [{ ...clips[1], visualStartBeat: 129.25 }];
    expect(resolveTimelineInsert(trimmed, track, { clipId: 2, position: "before" })).toMatchObject({
      insertionBeat: 129.25, startBeat: 131, anchorBeat: 132, endBeat: 251, shiftBeats: 124,
    });
  });

  it("rejects missing, unanalyzed and out-of-bounds insertions", () => {
    for (const source of [undefined, { ...track, isMissing: true }, { ...track, bpm: null }]) {
      expect(resolveTimelineInsert(clips, source, { clipId: 2, position: "before" })).toBeNull();
    }
    expect(resolveTimelineInsert([{ ...clips[1], visualEndBeat: 999_999 }], track, { clipId: 2, position: "before" })).toBeNull();
    expect(resolveTimelineInsert(clips, track, { clipId: -1, position: "after" })).toBeNull();
  });
});
