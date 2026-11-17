import type { LibraryTrack } from "../library/types";
import type { TimelineClip, TimelineInsertion } from "../timeline/types";
import { BEATS_PER_MEASURE } from "./timelineSnap";
import { createTempoClock, type TempoClock } from "./tempoCurve";

export interface TimelineInsertPreview extends TimelineInsertion {
  lane: number;
  insertionBeat: number;
  startBeat: number;
  anchorBeat: number;
  endBeat: number;
  shiftBeats: number;
}

function analyzedTrack(track: LibraryTrack | undefined): track is LibraryTrack & { bpm: number; firstBeatMs: number } {
  return !!track && !track.isMissing && track.bpm !== null && track.firstBeatMs !== null
    && Number.isFinite(track.bpm) && track.bpm >= 40 && track.bpm <= 300
    && Number.isFinite(track.firstBeatMs) && Number.isFinite(track.durationMs)
    && track.firstBeatMs >= 0 && track.firstBeatMs <= track.durationMs && track.durationMs > 0;
}

/** Prefer the clip under the pointer; otherwise catch a snapped drop that would overlap one. */
export function timelineDropConflict(
  clips: readonly TimelineClip[],
  track: LibraryTrack | undefined,
  lane: number,
  pointerBeat: number,
  anchorBeat: number,
  clock?: TempoClock,
): TimelineClip | null {
  if (!Number.isFinite(pointerBeat)) return null;
  const underPointer = clips.find((clip) => clip.lane === lane
    && pointerBeat >= clip.visualStartBeat && pointerBeat < clip.visualEndBeat);
  if (underPointer) return underPointer;
  if (!analyzedTrack(track)) return null;
  const preRoll = track.firstBeatMs * track.bpm / 60_000;
  const minimum = track.tempoMatchingEnabled === false && clock ? clock.beatAtSeconds(track.firstBeatMs / 1000) : Math.ceil(preRoll);
  const anchor = Math.max(anchorBeat, Math.ceil(minimum / BEATS_PER_MEASURE) * BEATS_PER_MEASURE);
  const start = track.tempoMatchingEnabled === false && clock
    ? clock.beatAtSeconds(clock.secondsAtBeat(anchor) - track.firstBeatMs / 1000) : anchor - preRoll;
  const end = track.tempoMatchingEnabled === false && clock
    ? clock.beatAtSeconds(clock.secondsAtBeat(start) + track.durationMs / 1000) : start + track.durationMs * track.bpm / 60_000;
  return clips.filter((clip) => clip.lane === lane
    && start < clip.visualEndBeat - 0.05 && end > clip.visualStartBeat + 0.05)
    .sort((a, b) => a.visualStartBeat - b.visualStartBeat)[0] ?? null;
}

/** Mirrors insert_clip; Rust recomputes this plan from the current session on confirmation. */
export function resolveTimelineInsert(
  clips: readonly TimelineClip[],
  track: LibraryTrack | undefined,
  insertion: TimelineInsertion,
  clock?: TempoClock,
): TimelineInsertPreview | null {
  if (!analyzedTrack(track)) return null;
  const target = clips.find((clip) => clip.id === insertion.clipId);
  if (!target || target.needsAnalysis || target.visualEndBeat <= target.visualStartBeat) return null;
  const insertionBeat = insertion.position === "before" ? target.visualStartBeat : target.visualEndBeat;
  const preRoll = track.firstBeatMs * track.bpm / 60_000;
  let anchorBeat = 0, startBeat = 0, endBeat = 0, shiftBeats = 0;
  const shifted = new Set(clips.filter((clip) => clip.visualStartBeat >= insertionBeat - 1e-6).map((clip) => clip.id));
  let fits = false;
  for (let attempt = 0; attempt < 64; attempt++) {
    const future = clock ? createTempoClock(clock.projectBpm, clock.points.map((point) => ({ ...point,
      beat: point.beat + (point.clipId !== null && shifted.has(point.clipId) ? shiftBeats : 0) }))) : null;
    const free = track.tempoMatchingEnabled === false && future;
    const desiredAnchor = free ? future.beatAtSeconds(future.secondsAtBeat(insertionBeat) + track.firstBeatMs / 1000) : insertionBeat + preRoll;
    anchorBeat = Math.ceil(desiredAnchor / BEATS_PER_MEASURE) * BEATS_PER_MEASURE;
    startBeat = free ? future.beatAtSeconds(future.secondsAtBeat(anchorBeat) - track.firstBeatMs / 1000) : anchorBeat - preRoll;
    endBeat = free ? future.beatAtSeconds(future.secondsAtBeat(startBeat) + track.durationMs / 1000) : startBeat + track.durationMs * track.bpm / 60_000;
    const required = Math.ceil((endBeat - insertionBeat) / BEATS_PER_MEASURE) * BEATS_PER_MEASURE;
    if (required <= shiftBeats && shiftBeats > 0) { fits = true; break; }
    shiftBeats = required;
    if (insertionBeat + shiftBeats > 1_000_000) return null;
  }
  if (!fits) return null;
  if (endBeat > 1_000_000 || clips.some((clip) => clip.visualStartBeat >= insertionBeat - 1e-6
    && Math.max(clip.visualEndBeat, clip.anchorBeat, clip.tempoAnchorBeat) + shiftBeats > 1_000_000)) return null;
  return { ...insertion, lane: target.lane, insertionBeat, startBeat, anchorBeat, endBeat, shiftBeats };
}
