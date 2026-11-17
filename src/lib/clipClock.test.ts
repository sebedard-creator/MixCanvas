import { describe, expect, it } from "vitest";
import { createTempoClock } from "./tempoCurve";
import { clipAtAnchor, clipLoopTurns, clipProjectBeat, clipSourceBeat, minimumClipAnchor, projectClip, sourceClip } from "./clipClock";
import { clipWithTrim, trimForEdge } from "./clipTrim";
import type { TimelineClip } from "../timeline/types";

const clip = { id: 1, anchorBeat: 16, bpm: 100, tempoMatchingEnabled: false, preRollBeats: 0,
  trimStartBeats: 0, trimEndBeats: 0, durationBeats: 8, visualStartBeat: 16, visualEndBeat: 25.6,
  looping: false, loopLeadBeats: 0, loopTailBeats: 0 } as TimelineClip;

describe("native-speed clip clock", () => {
  it("round-trips the beat-stepped Rust clock across a tempo ramp", () => {
    const clock = createTempoClock(120, [{ beat: 8, bpm: 180, clipId: 2 }]);
    expect(clock.secondsAtBeat(1)).toBe(0.5);
    expect(clock.secondsAtBeat(0.5)).toBe(0.25);
    for (const beat of [-1, 0, 0.75, 4.25, 8, 20]) expect(clock.beatAtSeconds(clock.secondsAtBeat(beat))).toBeCloseTo(beat, 10);
    const end = clipProjectBeat(clip, 24, clock);
    expect(clock.secondsAtBeat(end) - clock.secondsAtBeat(16)).toBeCloseTo(4.8, 10);
    expect(clipSourceBeat(clip, end, clock)).toBeCloseTo(24, 10);
  });

  it("keeps trims and minimum anchors in source time", () => {
    const clock = createTempoClock(120, []);
    const source = sourceClip(clip, clock);
    expect(source.visualEndBeat).toBeCloseTo(24);
    const trim = trimForEdge(source, "end", clipSourceBeat(clip, 23.2, clock), { limitStartBeat: 0, limitEndBeat: Infinity });
    expect(trim.trimEndBeats).toBeCloseTo(2);
    expect(projectClip(clipWithTrim(source, trim), clock).visualEndBeat).toBeCloseTo(23.2);
    expect(minimumClipAnchor({ ...clip, preRollBeats: 2 }, clock)).toBe(3);
  });

  it("moves free audio without changing its elapsed duration and projects each loop turn", () => {
    const clock = createTempoClock(120, [{ beat: 32, bpm: 180, clipId: 2 }]);
    const original = projectClip({ ...clip, visualEndBeat: 24 }, clock);
    const moved = clipAtAnchor(original, 40, clock);
    expect(clock.secondsAtBeat(moved.visualEndBeat) - clock.secondsAtBeat(moved.visualStartBeat)).toBeCloseTo(4.8);
    const looped = projectClip({ ...clip, looping: true, loopTailBeats: 8, visualEndBeat: 32 }, clock);
    const turns = clipLoopTurns(looped, 16, clock);
    expect(turns).toHaveLength(2);
    for (const turn of turns) expect(clock.secondsAtBeat(turn.projectEndBeat) - clock.secondsAtBeat(turn.projectStartBeat)).toBeCloseTo(4.8);
  });
});
