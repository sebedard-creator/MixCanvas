/**
 * Trimming a clip's head or tail by dragging its edge.
 *
 * The anchor never moves. Trimming changes how much of the source is heard, not
 * where the clip sits, so the audio under the untrimmed part stays exactly where
 * it was on the timeline — which is the whole reason to reach for the tool
 * rather than move the clip.
 *
 * A trim is stored as how many beats are hidden at each end. Dragging an edge
 * back out is the same gesture with a smaller number, down to zero where the
 * whole source is heard again.
 */

/** How close to an edge the pointer has to be, in pixels, to grab it. */
export const TRIM_GRAB_PX = 7;
/** A clip has to keep at least this much of itself, or it would vanish. */
export const MIN_CLIP_BEATS = 0.5;

export type TrimEdge = "start" | "end";

export interface TrimmableClip {
  visualStartBeat: number;
  visualEndBeat: number;
  trimStartBeats: number;
  trimEndBeats: number;
}

export interface ClipTrim {
  trimStartBeats: number;
  trimEndBeats: number;
}

/** Where the clip would begin and end with nothing trimmed away. */
export function untrimmedBounds(clip: TrimmableClip): { startBeat: number; endBeat: number } {
  return {
    startBeat: clip.visualStartBeat - clip.trimStartBeats,
    endBeat: clip.visualEndBeat + clip.trimEndBeats,
  };
}

/**
 * A clip as it looks mid-drag, before the edit reaches the backend.
 *
 * Everything downstream has to read the same geometry — the box the clip is
 * drawn in, and the window of the waveform shown inside it. Feeding the drag's
 * width to one and the committed trim to the other squeezes a fixed slice of
 * audio into a shrinking box, which looks exactly like a time-stretch.
 */
export function clipWithTrim<T extends TrimmableClip>(clip: T, trim: ClipTrim | undefined): T {
  if (!trim) return clip;
  const bounds = untrimmedBounds(clip);
  return {
    ...clip,
    trimStartBeats: trim.trimStartBeats,
    trimEndBeats: trim.trimEndBeats,
    visualStartBeat: bounds.startBeat + trim.trimStartBeats,
    visualEndBeat: bounds.endBeat - trim.trimEndBeats,
  };
}

/**
 * Which edge the pointer is over, if either.
 *
 * The grab zone is measured in pixels rather than beats so it stays the same
 * size under the hand at every zoom level. It is also clamped to a third of the
 * clip, so the two edges of a very short clip cannot both claim the middle.
 */
export function trimEdgeAtPointer(
  clip: TrimmableClip,
  pointerBeat: number,
  pixelsPerBeat: number,
): TrimEdge | null {
  if (!(pixelsPerBeat > 0)) return null;
  const width = clip.visualEndBeat - clip.visualStartBeat;
  if (width <= 0) return null;

  const reach = Math.min(TRIM_GRAB_PX / pixelsPerBeat, width / 3);
  if (pointerBeat >= clip.visualStartBeat - reach && pointerBeat <= clip.visualStartBeat + reach) {
    return "start";
  }
  if (pointerBeat >= clip.visualEndBeat - reach && pointerBeat <= clip.visualEndBeat + reach) {
    return "end";
  }
  return null;
}

/** Snaps a beat onto the quarter-beat grid, as the rest of the timeline does. */
export function snapTrimBeat(beat: number): number {
  return Math.round(beat * 4) / 4;
}

/**
 * The trim that results from dragging `edge` to `pointerBeat`.
 *
 * `limitStartBeat` and `limitEndBeat` are how far the clip may grow before it
 * would run into a neighbour on the same lane; extending is as constrained as
 * moving, since the space either exists or it does not.
 */
export function trimForEdge(
  clip: TrimmableClip,
  edge: TrimEdge,
  pointerBeat: number,
  limits: { limitStartBeat: number; limitEndBeat: number },
): ClipTrim {
  const bounds = untrimmedBounds(clip);
  const snapped = snapTrimBeat(pointerBeat);

  if (edge === "start") {
    // Never past the source's own beginning, never into the clip before it,
    // and never so far right that nothing of the clip is left.
    const earliest = Math.max(bounds.startBeat, limits.limitStartBeat);
    const latest = clip.visualEndBeat - MIN_CLIP_BEATS;
    const startBeat = Math.min(Math.max(snapped, earliest), latest);
    return {
      trimStartBeats: Math.max(0, startBeat - bounds.startBeat),
      trimEndBeats: clip.trimEndBeats,
    };
  }

  const latest = Math.min(bounds.endBeat, limits.limitEndBeat);
  const earliest = clip.visualStartBeat + MIN_CLIP_BEATS;
  const endBeat = Math.max(Math.min(snapped, latest), earliest);
  return {
    trimStartBeats: clip.trimStartBeats,
    trimEndBeats: Math.max(0, bounds.endBeat - endBeat),
  };
}

/**
 * How far a clip may grow in each direction before meeting a neighbour.
 *
 * Only clips on the same lane can be in the way, and only the nearest one on
 * each side matters.
 */
export function clipTrimLimits(
  clips: readonly { id: number; lane: number; visualStartBeat: number; visualEndBeat: number }[],
  clip: { id: number; lane: number; visualStartBeat: number; visualEndBeat: number },
): { limitStartBeat: number; limitEndBeat: number } {
  let limitStartBeat = 0;
  let limitEndBeat = Number.POSITIVE_INFINITY;

  for (const other of clips) {
    if (other.id === clip.id || other.lane !== clip.lane) continue;
    if (other.visualEndBeat <= clip.visualStartBeat) {
      limitStartBeat = Math.max(limitStartBeat, other.visualEndBeat);
    } else if (other.visualStartBeat >= clip.visualEndBeat) {
      limitEndBeat = Math.min(limitEndBeat, other.visualStartBeat);
    }
  }

  return { limitStartBeat, limitEndBeat };
}

/**
 * Le temps le plus à gauche où l'ancre d'un clip peut se poser.
 *
 * Miroir de `minimum_anchor_beat` dans `src-tauri/src/timeline.rs`, et il doit
 * le rester : le serveur refuserait un déplacement que l'interface aurait
 * autorisé, et le clip reviendrait en arrière sous le curseur.
 *
 * L'ancre porte le **premier temps**, pas le début du clip. Ce qu'on protège,
 * c'est que la partie *visible* ne commence pas avant le temps zéro — donc
 * `ancre − pré-roll + rognage ≥ 0`. Le rognage manquait à ce calcul : un clip
 * dont on avait coupé la tête restait retenu par une part qu'il ne fait plus
 * entendre.
 */
export function minimumAnchorBeat(preRollBeats: number, trimStartBeats: number): number {
  const usable = Number.isFinite(preRollBeats) && Number.isFinite(trimStartBeats);
  if (!usable) return 0;
  return Math.max(0, Math.ceil(preRollBeats - trimStartBeats));
}
