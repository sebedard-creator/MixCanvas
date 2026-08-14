import { describe, expect, it } from "vitest";

import {
  PLAYED_EFFECTS,
  TIMELINE_LANE_COUNT,
  hatchBands,
  hatchPatternId,
  laneLetter,
  lanesPlayingAt,
  regionsAcross,
  reverbGradientId,
  reverbGradientStops,
  reverbSpans,
} from "./mixEffects";

describe("laneLetter", () => {
  it("names the three tracks the way the rest of the program does", () => {
    expect(laneLetter(0)).toBe("A");
    expect(laneLetter(1)).toBe("B");
    expect(laneLetter(2)).toBe("C");
  });
});

describe("reverbSpans", () => {
  /** Ce que `write_reverb_span` écrit côté Rust : zéro, un, un, zéro. */
  const pass = (start: number, end: number, ramp = 0.5) => [
    { beat: start, value: 0 },
    { beat: start + ramp, value: 1 },
    { beat: end - ramp, value: 1 },
    { beat: end, value: 0 },
  ];

  it("turns a played pass into one region, ramps included", () => {
    expect(reverbSpans(pass(8, 24))).toEqual([
      { startBeat: 8, endBeat: 24, peak: 1, nodes: pass(8, 24) },
    ]);
  });

  it("keeps two passes apart", () => {
    const spans = reverbSpans([...pass(4, 12), ...pass(20, 28)]);
    expect(spans.map((s) => [s.startBeat, s.endBeat])).toEqual([
      [4, 12],
      [20, 28],
    ]);
  });

  it("has nothing to tint without nodes", () => {
    expect(reverbSpans([])).toEqual([]);
  });

  it("ignores a lane left at zero", () => {
    expect(reverbSpans([{ beat: 0, value: 0 }, { beat: 16, value: 0 }])).toEqual([]);
  });

  it("still closes a pass whose last node was never brought back down", () => {
    // Un projet écrit par une version antérieure, ou un nœud effacé à la main :
    // mieux vaut teinter jusqu'au dernier point que de ne rien montrer.
    const nodes = [{ beat: 4, value: 0 }, { beat: 6, value: 1 }, { beat: 10, value: 1 }];
    expect(reverbSpans(nodes)).toEqual([
      { startBeat: 4, endBeat: 10, peak: 1, nodes },
    ]);
  });

  it("reads nodes given out of order", () => {
    const scrambled = [...pass(8, 24)].reverse();
    expect(reverbSpans(scrambled)).toEqual([
      { startBeat: 8, endBeat: 24, peak: 1, nodes: pass(8, 24) },
    ]);
  });
});

describe("reverbGradientStops", () => {
  /** Les rampes que `write_reverb_span` écrit réellement, en beats. */
  const RAMP_IN = 0.125;
  const RAMP_OUT = 0.75;
  const written = (start: number, end: number) => [
    { beat: start, value: 0 },
    { beat: start + RAMP_IN, value: 1 },
    { beat: end - RAMP_OUT, value: 1 },
    { beat: end, value: 0 },
  ];
  const spanOf = (start: number, end: number) => reverbSpans(written(start, end))[0];

  it("puts one stop on each node, at the place the node actually holds", () => {
    // Une passe de seize temps : la montée occupe 0,125 / 16 de la largeur.
    const stops = reverbGradientStops(spanOf(8, 24));
    expect(stops).toEqual([
      { offset: 0, opacity: 0 },
      { offset: 0.125 / 16, opacity: 1 },
      { offset: 15.25 / 16, opacity: 1 },
      { offset: 1, opacity: 0 },
    ]);
  });

  /** Le défaut signalé : le dégradé était fixe à douze pour cent, si bien que
   *  la teinte disait autre chose que l'automation — et l'écart changeait de
   *  sens selon la longueur de la passe. */
  it("follows the pass instead of a fixed fraction of its width", () => {
    const long = reverbGradientStops(spanOf(0, 32));
    const short = reverbGradientStops(spanOf(0, 2));
    // Sur une longue passe la montée est une petite fraction, sur une courte
    // une grande : c'est précisément ce qu'un pourcentage fixe ne peut pas dire.
    expect(long[1].offset).toBeCloseTo(0.125 / 32, 10);
    expect(short[1].offset).toBeCloseTo(0.125 / 2, 10);
    expect(long[1].offset).toBeLessThan(0.12);
    expect(short[1].offset).toBeGreaterThan(0.03);
    // Et la descente est bien plus longue que la montée, dans les deux cas.
    expect(1 - long[2].offset).toBeGreaterThan(long[1].offset);
    expect(1 - short[2].offset).toBeGreaterThan(short[1].offset);
  });

  it("keeps every stop inside the region", () => {
    for (const stop of reverbGradientStops(spanOf(4, 20))) {
      expect(stop.offset).toBeGreaterThanOrEqual(0);
      expect(stop.offset).toBeLessThanOrEqual(1);
      expect(stop.opacity).toBeGreaterThanOrEqual(0);
      expect(stop.opacity).toBeLessThanOrEqual(1);
    }
  });

  it("clamps a node that a damaged project put outside the region", () => {
    const stops = reverbGradientStops({
      startBeat: 0,
      endBeat: 4,
      peak: 1,
      nodes: [{ beat: -2, value: 1.5 }, { beat: 9, value: -0.2 }],
    });
    expect(stops).toEqual([
      { offset: 0, opacity: 1 },
      { offset: 1, opacity: 0 },
    ]);
  });

  it("still gives a colour to a region with no width", () => {
    const stops = reverbGradientStops({ startBeat: 5, endBeat: 5, peak: 0.4, nodes: [] });
    expect(stops).toEqual([{ offset: 0, opacity: 0.4 }]);
  });
});

describe("reverbGradientId", () => {
  /** Un identifiant que `url(#…)` ne retrouve pas laisse la région sans
   *  couleur, et rien dans le rendu ne le signale. */
  it("stays a usable id whatever the beat looks like", () => {
    for (const beat of [0, 8, 30.125, 1e-7, 1234.5678, -4.5]) {
      for (const lane of [0, 1, 2]) {
        for (const effect of PLAYED_EFFECTS) {
          expect(reverbGradientId(effect, lane, beat)).toMatch(/^[A-Za-z][-A-Za-z0-9_]*$/);
        }
      }
    }
  });

  it("gives each region its own", () => {
    const ids = new Set([
      reverbGradientId("reverb", 0, 8),
      reverbGradientId("flanger", 0, 8),
      reverbGradientId("reverb", 1, 8),
      reverbGradientId("reverb", 0, 8.125),
      reverbGradientId("reverb", 0, 24),
    ]);
    expect(ids.size).toBe(5);
  });
});

describe("hatchBands", () => {
  const region = (
    effect: "reverb" | "flanger" | "bitcrush",
    lane: number,
    startBeat: number,
    endBeat: number,
  ) => ({ effect, lane, startBeat, endBeat, peak: 1, nodes: [] });

  it("finds nothing when one effect plays alone", () => {
    expect(hatchBands([region("reverb", 0, 0, 16)])).toEqual([]);
  });

  it("finds nothing when two effects sit on different tracks", () => {
    // Se recouvrir dans le temps ne suffit pas : deux voies différentes
    // occupent deux bandes différentes, et rien ne se superpose à l'écran.
    expect(hatchBands([region("reverb", 0, 0, 16), region("flanger", 1, 0, 16)])).toEqual([]);
  });

  it("finds nothing when two effects on one track never meet", () => {
    expect(hatchBands([region("reverb", 0, 0, 8), region("flanger", 0, 12, 20)])).toEqual([]);
  });

  it("marks the part where two effects share a track", () => {
    const bands = hatchBands([region("reverb", 0, 0, 16), region("flanger", 0, 8, 24)]);
    expect(bands).toEqual([
      { lane: 0, startBeat: 8, endBeat: 16, effects: ["reverb", "flanger"] },
    ]);
  });

  it("keeps the effects in a stable order whatever order they came in", () => {
    // Sinon deux recouvrements identiques donneraient deux motifs, l'un rayé
    // mauve-vert et l'autre vert-mauve.
    const one = hatchBands([region("reverb", 0, 0, 16), region("flanger", 0, 8, 24)]);
    const other = hatchBands([region("flanger", 0, 8, 24), region("reverb", 0, 0, 16)]);
    expect(one).toEqual(other);
  });

  it("covers a track fully wrapped by another effect", () => {
    const bands = hatchBands([region("reverb", 0, 4, 8), region("flanger", 0, 0, 16)]);
    expect(bands).toEqual([
      { lane: 0, startBeat: 4, endBeat: 8, effects: ["reverb", "flanger"] },
    ]);
  });

  it("joins two touching slices that hold the same effects", () => {
    // Deux passes de reverb bout à bout sous un seul flanger : une seule bande,
    // sans couture là où rien ne change.
    const bands = hatchBands([
      region("reverb", 0, 0, 8),
      region("reverb", 0, 8, 16),
      region("flanger", 0, 0, 16),
    ]);
    expect(bands).toEqual([
      { lane: 0, startBeat: 0, endBeat: 16, effects: ["reverb", "flanger"] },
    ]);
  });

  it("hatches all three when all three overlap", () => {
    const bands = hatchBands([
      region("reverb", 1, 0, 16),
      region("flanger", 1, 0, 16),
      region("bitcrush", 1, 0, 16),
    ]);
    expect(bands).toEqual([
      { lane: 1, startBeat: 0, endBeat: 16, effects: ["reverb", "flanger", "bitcrush"] },
    ]);
  });

  it("splits where the set of effects changes", () => {
    // Reverb tout du long, flanger sur la première moitié, bitcrush sur la
    // seconde : trois bandes, dont deux hachurées différemment.
    const bands = hatchBands([
      region("reverb", 0, 0, 16),
      region("flanger", 0, 0, 8),
      region("bitcrush", 0, 8, 16),
    ]);
    expect(bands).toEqual([
      { lane: 0, startBeat: 0, endBeat: 8, effects: ["reverb", "flanger"] },
      { lane: 0, startBeat: 8, endBeat: 16, effects: ["reverb", "bitcrush"] },
    ]);
  });

  it("reports each track separately", () => {
    const bands = hatchBands([
      region("reverb", 2, 0, 16),
      region("flanger", 2, 8, 24),
      region("reverb", 0, 0, 16),
      region("flanger", 0, 8, 24),
    ]);
    expect(bands.map((band) => band.lane)).toEqual([0, 2]);
  });
});

describe("regionsAcross", () => {
  const span = (startBeat: number, endBeat: number) => ({ startBeat, endBeat });

  it("keeps what sits inside the window", () => {
    expect(regionsAcross([span(10, 14)], 8, 20)).toEqual([span(10, 14)]);
  });

  it("drops what is far away on either side", () => {
    expect(regionsAcross([span(0, 4), span(40, 48)], 8, 20)).toEqual([]);
  });

  /** Une passe plus longue que l'écran doit rester : elle est visible de part
   *  en part, et la couper la ferait disparaître au moment où on la regarde. */
  it("keeps a region that spans the whole window", () => {
    expect(regionsAcross([span(0, 100)], 8, 20)).toEqual([span(0, 100)]);
  });

  it("keeps a region that only enters from one side", () => {
    expect(regionsAcross([span(4, 12)], 8, 20)).toEqual([span(4, 12)]);
    expect(regionsAcross([span(16, 30)], 8, 20)).toEqual([span(16, 30)]);
  });

  /** Les bords comptent comme dedans : une région qui touche exactement le bord
   *  a un pixel à l'écran, et le perdre se verrait comme un clignotement au
   *  défilement. */
  it("counts a region touching either edge as visible", () => {
    expect(regionsAcross([span(2, 8)], 8, 20)).toEqual([span(2, 8)]);
    expect(regionsAcross([span(20, 26)], 8, 20)).toEqual([span(20, 26)]);
  });

  /** La fenêtre est infinie tant que la vue n'est pas mesurable — rien ne doit
   *  disparaître avant le premier rendu. */
  it("keeps everything when the window is unbounded", () => {
    const all = [span(0, 4), span(1000, 1004)];
    expect(regionsAcross(all, Number.NEGATIVE_INFINITY, Number.POSITIVE_INFINITY)).toEqual(all);
  });

  it("carries the rest of each region through untouched", () => {
    const region = { startBeat: 10, endBeat: 14, lane: 2, effect: "delay" as const };
    expect(regionsAcross([region], 8, 20)[0]).toBe(region);
  });
});

describe("hatchPatternId", () => {
  it("is one id per combination, not per band", () => {
    expect(hatchPatternId(["reverb", "flanger"])).toBe(hatchPatternId(["reverb", "flanger"]));
    expect(hatchPatternId(["reverb", "flanger"])).toMatch(/^[A-Za-z][-A-Za-z0-9_]*$/);
  });
});

describe("lanesPlayingAt", () => {
  const spans = [
    [{ startBeat: 8, endBeat: 24, peak: 1, nodes: [] }],
    [],
    [{ startBeat: 0, endBeat: 4, peak: 1, nodes: [] }, { startBeat: 30, endBeat: 40, peak: 1, nodes: [] }],
  ];

  it("lights the lanes whose pass covers the playhead", () => {
    expect(lanesPlayingAt(spans, 2)).toBe(0b100);
    expect(lanesPlayingAt(spans, 12)).toBe(0b001);
    expect(lanesPlayingAt(spans, 35)).toBe(0b100);
  });

  it("lights nothing between passes", () => {
    expect(lanesPlayingAt(spans, 6)).toBe(0);
    expect(lanesPlayingAt(spans, 100)).toBe(0);
  });

  it("counts the edges as inside", () => {
    // Le bord est ce qu'on entend arriver : l'éteindre là ferait clignoter la
    // pastille juste au moment où la reverb s'ouvre.
    expect(lanesPlayingAt(spans, 8)).toBe(0b001);
    expect(lanesPlayingAt(spans, 24)).toBe(0b001);
  });

  it("can light two lanes at once", () => {
    const overlapping = [
      [{ startBeat: 0, endBeat: 16, peak: 1, nodes: [] }],
      [{ startBeat: 8, endBeat: 24, peak: 1, nodes: [] }],
      [],
    ];
    expect(lanesPlayingAt(overlapping, 12)).toBe(0b011);
  });

  it("lights nothing on a position it cannot trust", () => {
    expect(lanesPlayingAt(spans, Number.NaN)).toBe(0);
  });
});
