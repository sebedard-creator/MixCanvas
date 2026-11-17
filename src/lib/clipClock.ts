import type { TimelineClip } from "../timeline/types";
import type { TempoClock } from "./tempoCurve";
import { loopTurns } from "./clipTrim";

export function clipSourceBeat(clip: TimelineClip, beat: number, clock: TempoClock): number {
  if (clip.tempoMatchingEnabled !== false || !clip.bpm || !Number.isFinite(beat)) return beat;
  return clip.anchorBeat + (clock.secondsAtBeat(beat) - clock.secondsAtBeat(clip.anchorBeat)) * clip.bpm / 60;
}

export function clipProjectBeat(clip: TimelineClip, sourceBeat: number, clock: TempoClock): number {
  if (clip.tempoMatchingEnabled !== false || !clip.bpm || !Number.isFinite(sourceBeat)) return sourceBeat;
  return clock.beatAtSeconds(clock.secondsAtBeat(clip.anchorBeat) + (sourceBeat - clip.anchorBeat) * 60 / clip.bpm);
}

/** Existing trim and loop tools operate in source beats in both playback modes. */
export function sourceClip(clip: TimelineClip, clock: TempoClock): TimelineClip {
  if (clip.tempoMatchingEnabled !== false) return clip;
  return { ...clip, visualStartBeat: clipSourceBeat(clip, clip.visualStartBeat, clock), visualEndBeat: clipSourceBeat(clip, clip.visualEndBeat, clock) };
}

export function projectClip(clip: TimelineClip, clock: TempoClock): TimelineClip {
  if (clip.tempoMatchingEnabled !== false) return clip;
  return { ...clip, visualStartBeat: clipProjectBeat(clip, clip.visualStartBeat, clock), visualEndBeat: clipProjectBeat(clip, clip.visualEndBeat, clock) };
}

export function clipAtAnchor(clip: TimelineClip, anchorBeat: number, clock: TempoClock): TimelineClip {
  const source = sourceClip(clip, clock);
  const delta = anchorBeat - clip.anchorBeat;
  return projectClip({ ...source, anchorBeat, visualStartBeat: source.visualStartBeat + delta, visualEndBeat: source.visualEndBeat + delta }, clock);
}

export function minimumClipAnchor(clip: TimelineClip, clock: TempoClock): number {
  const offset = Math.max(0, clip.preRollBeats - clip.trimStartBeats + (clip.looping ? clip.loopLeadBeats : 0));
  return Math.ceil(clip.tempoMatchingEnabled === false && clip.bpm ? clock.beatAtSeconds(offset * 60 / clip.bpm) : offset);
}

export function clipLoopTurns(clip: TimelineClip, pixelsPerBeat: number, clock: TempoClock) {
  const source = sourceClip(clip, clock);
  let cursor = source.visualStartBeat;
  return loopTurns(source, pixelsPerBeat).map((turn) => {
    const sourceStartBeat = cursor;
    cursor += turn.durationBeats;
    const projectStartBeat = clipProjectBeat(clip, sourceStartBeat, clock);
    const projectEndBeat = clipProjectBeat(clip, cursor, clock);
    const left = Math.round((projectStartBeat - clip.visualStartBeat) * pixelsPerBeat);
    const right = Math.round((projectEndBeat - clip.visualStartBeat) * pixelsPerBeat);
    return { ...turn, sourceStartBeat, projectStartBeat, projectEndBeat,
      offsetPx: clip.tempoMatchingEnabled === false ? left : turn.offsetPx,
      widthPx: clip.tempoMatchingEnabled === false ? Math.max(1, right - left) : turn.widthPx };
  });
}
