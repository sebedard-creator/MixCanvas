import { describe, expect, it } from "vitest";
import { canBeSidechainKey, clipsCoveredByKey } from "./sidechainKey";
import type { TimelineClip } from "../timeline/types";

function clip(id: number, lane: number, startBeat: number, endBeat: number): TimelineClip {
  return {
    id,
    libraryTrackId: id,
    fileName: `clip-${id}.mp3`,
    filePath: `C:/music/clip-${id}.mp3`,
    lane,
    anchorBeat: startBeat,
    tempoAnchorBeat: startBeat,
    bpm: 128,
    firstBeatMs: 0,
    preRollBeats: 0,
    durationBeats: endBeat - startBeat,
    visualStartBeat: startBeat,
    visualEndBeat: endBeat,
    trimStartBeats: 0,
    trimEndBeats: 0,
    isSidechainKey: false,
    stem: "full",
    hasStems: false,
  isBaked: false,
    isMissing: false,
    needsAnalysis: false,
    waveform: null,
  };
}

describe("canBeSidechainKey", () => {
  it("refuses a clip that covers nothing", () => {
    const alone = clip(1, 0, 0, 32);
    expect(canBeSidechainKey(alone, [alone])).toBe(false);
    expect(canBeSidechainKey(alone, [alone, clip(2, 1, 64, 96)])).toBe(false);
  });

  it("accepts a clip that overlaps another lane", () => {
    const key = clip(1, 0, 0, 32);
    const covered = clip(2, 1, 16, 48);
    expect(canBeSidechainKey(key, [key, covered])).toBe(true);
    // The relation is symmetric: either of the two can be the key.
    expect(canBeSidechainKey(covered, [key, covered])).toBe(true);
  });

  it("does not count clips that merely touch", () => {
    const first = clip(1, 0, 0, 32);
    const second = clip(2, 1, 32, 64);
    expect(canBeSidechainKey(first, [first, second])).toBe(false);
  });

  it("accepts a clip fully contained in another", () => {
    const short = clip(1, 0, 20, 24);
    const long = clip(2, 1, 0, 64);
    expect(canBeSidechainKey(short, [short, long])).toBe(true);
  });
});

describe("clipsCoveredByKey", () => {
  it("lists only what the key actually overlaps", () => {
    const key = clip(1, 0, 0, 32);
    const covered = clip(2, 1, 16, 48);
    const elsewhere = clip(3, 2, 64, 96);

    const result = clipsCoveredByKey(key, [key, covered, elsewhere]);
    expect(result.map((c) => c.id)).toEqual([2]);
  });

  it("is empty when the key stands alone", () => {
    const key = clip(1, 0, 0, 32);
    expect(clipsCoveredByKey(key, [key])).toEqual([]);
  });
});
