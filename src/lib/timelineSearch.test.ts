import { describe, expect, it } from "vitest";
import type { TimelineClip } from "../timeline/types";
import type { LibraryTrack } from "../library/types";
import { searchTimeline } from "./timelineSearch";

const tracks = [
  { id: 1, artist: "Björk", title: "Jóga", fileName: "Bjork_Joga-remix.flac", filePath: "C:/music/Joga.flac" },
  { id: 2, artist: "Unused", title: "Library only", fileName: "Unused.wav", filePath: "C:/music/Unused.wav" },
] as LibraryTrack[];
const clips = [
  { id: 20, libraryTrackId: 1, anchorBeat: 1024, visualStartBeat: 1104, lane: 3, fileName: "Joga.flac", filePath: "C:/music/Joga.flac" },
  { id: 10, libraryTrackId: 1, anchorBeat: 4, visualStartBeat: 3, lane: 0, fileName: "Joga.flac", filePath: "C:/music/Joga.flac" },
  { id: 30, libraryTrackId: 3, anchorBeat: 400, visualStartBeat: 399, lane: 2, fileName: "Lost song.wav", filePath: "C:/missing/Lost song.wav" },
] as TimelineClip[];

describe("timeline song search", () => {
  it("finds artist, title and filename without accents or case", () => {
    for (const query of ["BJORK JOGA", "joga", "remix flac"]) expect(searchTimeline(clips, tracks, query).map(result => result.clip.id)).toEqual([10, 20]);
  });
  it("keeps repeated uses and offscreen clips with their actual sequence numbers", () => {
    expect(searchTimeline(clips, tracks, "joga").map(({ clip, sequence }) => [clip.id, sequence, clip.lane])).toEqual([[10, 1, 0], [20, 3, 3]]);
  });
  it("excludes songs that are only in the library and handles missing metadata", () => {
    expect(searchTimeline(clips, tracks, "unused")).toEqual([]);
    expect(searchTimeline(clips, tracks, "lost")[0].name).toBe("Lost song.wav");
    expect(searchTimeline(clips, tracks, "no match")).toEqual([]);
  });
  it("shows the full chronological mix for an empty query", () => {
    expect(searchTimeline(clips, tracks, "  ").map(result => result.clip.id)).toEqual([10, 30, 20]);
  });
  it("reports the visible bar of a trimmed clip instead of its original downbeat anchor", () => {
    expect(searchTimeline(clips, tracks, "joga").map(({ clip, bar }) => [clip.id, bar])).toEqual([[10, 1], [20, 277]]);
  });
});
