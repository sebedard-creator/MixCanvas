import type { TimelineClip } from "../timeline/types";

/** Same tolerance the backend uses, so both agree on what touching means. */
const OVERLAP_EPSILON_BEATS = 0.05;

function overlaps(a: TimelineClip, b: TimelineClip): boolean {
  return (
    a.visualStartBeat < b.visualEndBeat - OVERLAP_EPSILON_BEATS &&
    a.visualEndBeat > b.visualStartBeat + OVERLAP_EPSILON_BEATS
  );
}

/**
 * True when a clip covers, or is covered by, something on another lane.
 *
 * The key is only meaningful over material it can duck: on its own it would
 * mute nothing and pump nothing, so the control stays unavailable. Two clips on
 * the same lane can never overlap — the backend refuses it — so this only ever
 * finds neighbours across lanes.
 */
export function canBeSidechainKey(clip: TimelineClip, clips: TimelineClip[]): boolean {
  return clips.some((other) => other.id !== clip.id && overlaps(clip, other));
}

/**
 * Clips whose audio a key currently ducks: everything it overlaps.
 * Used to explain the effect in the control's tooltip.
 */
export function clipsCoveredByKey(key: TimelineClip, clips: TimelineClip[]): TimelineClip[] {
  return clips.filter((other) => other.id !== key.id && overlaps(key, other));
}
