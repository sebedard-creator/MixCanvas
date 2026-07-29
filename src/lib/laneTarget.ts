/**
 * Which clip a lane-scoped keyboard action applies to.
 *
 * Splitting used to take the first clip in the snapshot that happened to
 * straddle the playhead. With three lanes running at once that is whichever
 * clip the database returned first — arbitrary from where the user sits. The
 * lane the user last pointed at decides instead, so the same keypress in the
 * same place always does the same thing.
 */

export interface LaneClip {
  id: number;
  lane: number;
  visualStartBeat: number;
  visualEndBeat: number;
}

/**
 * How far inside a clip the playhead has to be before it can be cut.
 *
 * Splitting exactly on an edge would produce a zero-length piece, and the
 * playhead lands on an edge often, since clips are snapped to the grid and so
 * is the transport.
 */
export const SPLIT_EDGE_MARGIN_BEATS = 0.01;

/** The clip in `lane` that the playhead is inside, if there is one. */
export function clipToSplit(
  clips: readonly LaneClip[],
  lane: number,
  playheadBeat: number,
): LaneClip | null {
  return (
    clips.find(
      (clip) =>
        clip.lane === lane &&
        playheadBeat > clip.visualStartBeat + SPLIT_EDGE_MARGIN_BEATS &&
        playheadBeat < clip.visualEndBeat - SPLIT_EDGE_MARGIN_BEATS,
    ) ?? null
  );
}
