import type { LibraryTrack } from "../library/types";
import type { TimelineClip } from "../timeline/types";
import { libraryDisplayName } from "./libraryDisplayName";
import { libraryMatchesSearch } from "./librarySearch";
import { BEATS_PER_MEASURE } from "./timelineSnap";

export interface TimelineSearchResult {
  clip: TimelineClip;
  name: string;
  sequence: number;
  bar: number;
}

/** Search the whole mix, including clips outside the rendered viewport. */
export function searchTimeline(clips: readonly TimelineClip[], tracks: readonly LibraryTrack[], query: string): TimelineSearchResult[] {
  const byId = new Map(tracks.map((track) => [track.id, track]));
  return [...clips].sort((a, b) => a.anchorBeat - b.anchorBeat || a.id - b.id)
    .flatMap((clip, index) => {
      const track = byId.get(clip.libraryTrackId) ?? { artist: null, title: null, fileName: clip.fileName, filePath: clip.filePath };
      return libraryMatchesSearch(track, query) ? [{ clip, name: libraryDisplayName(track), sequence: index + 1,
        bar: Math.floor(Math.max(0, clip.visualStartBeat) / BEATS_PER_MEASURE) + 1 }] : [];
    });
}
