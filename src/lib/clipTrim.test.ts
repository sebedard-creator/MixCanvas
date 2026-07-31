import { describe, expect, it } from "vitest";

import {
  MIN_CLIP_BEATS,
  clipTrimLimits,
  clipWithTrim,
  minimumAnchorBeat,
  trimEdgeAtPointer,
  trimForEdge,
  untrimmedBounds,
} from "./clipTrim";

/** A clip whose source runs 0..64, currently untrimmed. */
const whole = {
  visualStartBeat: 0,
  visualEndBeat: 64,
  trimStartBeats: 0,
  trimEndBeats: 0,
};

/** The same source, with 8 beats hidden at the head and 4 at the tail. */
const trimmed = {
  visualStartBeat: 8,
  visualEndBeat: 60,
  trimStartBeats: 8,
  trimEndBeats: 4,
};

const OPEN = { limitStartBeat: 0, limitEndBeat: Number.POSITIVE_INFINITY };

describe("untrimmedBounds", () => {
  it("recovers where the source begins and ends", () => {
    expect(untrimmedBounds(whole)).toEqual({ startBeat: 0, endBeat: 64 });
    expect(untrimmedBounds(trimmed)).toEqual({ startBeat: 0, endBeat: 64 });
  });
});

describe("clipWithTrim", () => {
  it("keeps the box and the waveform window describing the same clip", () => {
    // The bug this exists to prevent: the clip was drawn at the dragged width
    // while the waveform was still sliced for the committed trim, so a fixed
    // stretch of audio was squeezed into a shrinking box — a time-stretch, not
    // a trim. The two have to come from one calculation.
    const live = clipWithTrim(whole, { trimStartBeats: 16, trimEndBeats: 8 });
    expect(live.visualStartBeat).toBe(16);
    expect(live.visualEndBeat).toBe(56);

    // The source length is what the waveform slice divides by, and it must not
    // change as the edges move.
    const total = live.trimStartBeats
      + (live.visualEndBeat - live.visualStartBeat)
      + live.trimEndBeats;
    expect(total).toBe(64);
  });

  it("works from an already trimmed clip", () => {
    const live = clipWithTrim(trimmed, { trimStartBeats: 0, trimEndBeats: 0 });
    expect(live.visualStartBeat).toBe(0);
    expect(live.visualEndBeat).toBe(64);
  });

  it("hands the clip straight back when nothing is being dragged", () => {
    expect(clipWithTrim(trimmed, undefined)).toBe(trimmed);
  });
});

describe("trimEdgeAtPointer", () => {
  it("grabs an edge from either side of it", () => {
    expect(trimEdgeAtPointer(whole, 0.1, 20)).toBe("start");
    expect(trimEdgeAtPointer(whole, -0.2, 20)).toBe("start");
    expect(trimEdgeAtPointer(whole, 63.9, 20)).toBe("end");
    expect(trimEdgeAtPointer(whole, 64.2, 20)).toBe("end");
  });

  it("leaves the body of the clip to the move gesture", () => {
    expect(trimEdgeAtPointer(whole, 32, 20)).toBeNull();
  });

  it("keeps the same reach under the hand at any zoom", () => {
    // 7 px is a third of a beat at 21 px/beat and a tenth at 70.
    expect(trimEdgeAtPointer(whole, 0.3, 21)).toBe("start");
    expect(trimEdgeAtPointer(whole, 0.3, 70)).toBeNull();
  });

  it("never lets both edges claim the middle of a short clip", () => {
    const short = { ...whole, visualEndBeat: 1.5 };
    // At this zoom the raw reach would be 3.5 beats, swallowing the clip.
    expect(trimEdgeAtPointer(short, 0.75, 2)).toBeNull();
  });
});

describe("trimForEdge", () => {
  it("hides the head without moving the tail", () => {
    const result = trimForEdge(whole, "start", 12, OPEN);
    expect(result).toEqual({ trimStartBeats: 12, trimEndBeats: 0 });
  });

  it("hides the tail without moving the head", () => {
    const result = trimForEdge(whole, "end", 50, OPEN);
    expect(result).toEqual({ trimStartBeats: 0, trimEndBeats: 14 });
  });

  it("gives a trimmed clip its material back", () => {
    expect(trimForEdge(trimmed, "start", 2, OPEN).trimStartBeats).toBe(2);
    expect(trimForEdge(trimmed, "end", 62, OPEN).trimEndBeats).toBe(2);
  });

  it("stops at the source's own ends rather than inventing audio", () => {
    expect(trimForEdge(trimmed, "start", -20, OPEN).trimStartBeats).toBe(0);
    expect(trimForEdge(trimmed, "end", 400, OPEN).trimEndBeats).toBe(0);
  });

  it("always leaves something of the clip behind", () => {
    const fromStart = trimForEdge(whole, "start", 999, OPEN);
    expect(64 - fromStart.trimStartBeats).toBeCloseTo(MIN_CLIP_BEATS, 6);
    const fromEnd = trimForEdge(whole, "end", -999, OPEN);
    expect(64 - fromEnd.trimEndBeats).toBeCloseTo(MIN_CLIP_BEATS, 6);
  });

  it("will not grow into a neighbour", () => {
    const limits = { limitStartBeat: 4, limitEndBeat: 58 };
    expect(trimForEdge(trimmed, "start", 0, limits).trimStartBeats).toBe(4);
    expect(trimForEdge(trimmed, "end", 64, limits).trimEndBeats).toBe(6);
  });

  it("snaps to the quarter beat", () => {
    expect(trimForEdge(whole, "start", 12.1, OPEN).trimStartBeats).toBe(12);
    expect(trimForEdge(whole, "start", 12.2, OPEN).trimStartBeats).toBe(12.25);
  });
});

describe("clipTrimLimits", () => {
  const lane = [
    { id: 1, lane: 0, visualStartBeat: 0, visualEndBeat: 16 },
    { id: 2, lane: 0, visualStartBeat: 24, visualEndBeat: 40 },
    { id: 3, lane: 0, visualStartBeat: 48, visualEndBeat: 64 },
    { id: 4, lane: 1, visualStartBeat: 20, visualEndBeat: 44 },
  ];

  it("takes the nearest neighbour on each side", () => {
    expect(clipTrimLimits(lane, lane[1])).toEqual({ limitStartBeat: 16, limitEndBeat: 48 });
  });

  it("opens out where there is no neighbour", () => {
    expect(clipTrimLimits(lane, lane[0])).toEqual({ limitStartBeat: 0, limitEndBeat: 24 });
    expect(clipTrimLimits(lane, lane[2])).toEqual({
      limitStartBeat: 40,
      limitEndBeat: Number.POSITIVE_INFINITY,
    });
  });

  it("ignores clips on other lanes, which cannot be in the way", () => {
    expect(clipTrimLimits(lane, lane[3])).toEqual({
      limitStartBeat: 0,
      limitEndBeat: Number.POSITIVE_INFINITY,
    });
  });
});

describe("minimumAnchorBeat", () => {
  it("protects the pre-roll of an untrimmed clip", () => {
    expect(minimumAnchorBeat(0, 0)).toBe(0);
    expect(minimumAnchorBeat(0.2, 0)).toBe(1);
    expect(minimumAnchorBeat(8.4, 0)).toBe(9);
  });

  it("gives back exactly what the head trim removed", () => {
    // Le défaut rapporté : le premier clip d'une timeline refusait de reculer
    // parce que la butée protégeait encore la tête qu'on venait de couper.
    expect(minimumAnchorBeat(8, 2)).toBe(6);
    expect(minimumAnchorBeat(8, 8)).toBe(0);
  });

  it("never asks for a negative anchor", () => {
    // Le schéma l'interdit; une butée négative ferait refuser côté serveur un
    // déplacement que l'interface venait d'autoriser.
    expect(minimumAnchorBeat(8, 40)).toBe(0);
    expect(minimumAnchorBeat(Number.NaN, 0)).toBe(0);
  });
});
