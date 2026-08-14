import { describe, expect, it } from "vitest";

import {
  VU_RANGE_DB,
  VU_TOO_LOW_DB,
  vuDecibels,
  vuMeterPosition,
  vuPositionAtDecibels,
  vuSegmentZone,
} from "./vuMeter";

const SEGMENTS = 24;
const zones = [...Array(SEGMENTS)].map((_, index) => vuSegmentZone(index, SEGMENTS));

describe("vuSegmentZone", () => {
  it("keeps red for distortion alone — one lens, at the very end", () => {
    expect(zones.filter((zone) => zone === "clip")).toHaveLength(1);
    expect(zones[SEGMENTS - 1]).toBe("clip");
    expect(zones[SEGMENTS - 2]).toBe("safe");
  });

  it("runs cold, then working, then clipping, with no zone interleaved", () => {
    expect(zones.join(" ")).toBe(
      [...zones]
        .sort((a, b) => ["low", "safe", "clip"].indexOf(a) - ["low", "safe", "clip"].indexOf(b))
        .join(" "),
    );
  });

  it("puts the cold/working boundary where the decibel threshold says", () => {
    const boundary = zones.indexOf("safe");
    // The last cold lens must light at or below the threshold, the first
    // working one above it. That is what ties the colours to a level rather
    // than to a lens count.
    const threshold = vuPositionAtDecibels(VU_TOO_LOW_DB);
    expect(boundary / SEGMENTS).toBeLessThanOrEqual(threshold);
    expect((boundary + 1) / SEGMENTS).toBeGreaterThan(threshold);
  });

  it("holds its meaning if the meter is ever rebuilt with more lenses", () => {
    /* La part de barre en dessous du seuil ne dépend pas du nombre de diodes :
       elle vaut la position du seuil, à une diode près. C'est cette part qui a
       changé en passant au dBFS — le seuil est resté au même niveau sonore,
       mais l'échelle ne tasse plus le bas, donc on voit enfin combien de la
       plage se trouve en dessous. */
    const threshold = vuPositionAtDecibels(VU_TOO_LOW_DB);
    for (const count of [12, 24, 48]) {
      const rebuilt = [...Array(count)].map((_, index) => vuSegmentZone(index, count));
      expect(rebuilt.filter((zone) => zone === "clip")).toHaveLength(1);
      const share = rebuilt.filter((zone) => zone === "low").length / count;
      expect(Math.abs(share - threshold)).toBeLessThanOrEqual(1 / count);
    }
  });
});

describe("vuPositionAtDecibels", () => {
  it("spreads the decibels evenly along the bar", () => {
    expect(vuPositionAtDecibels(-40)).toBeCloseTo(0, 8);
    expect(vuPositionAtDecibels(-20)).toBeCloseTo(0.5, 8);
    expect(vuPositionAtDecibels(0)).toBeCloseTo(1, 8);
  });

  /* C'est **la** raison du changement : une diode vaut le même nombre de
     décibels d'un bout à l'autre. L'ancienne échelle en donnait 2,31 en bas
     pour 0,50 en haut, sans rien d'imprimé pour prévenir. */
  it("gives every lens the same number of decibels", () => {
    const perLens = (VU_RANGE_DB.max - VU_RANGE_DB.min) / SEGMENTS;
    const steps = [...Array(SEGMENTS)].map(
      (_, index) =>
        vuPositionAtDecibels(VU_RANGE_DB.min + (index + 1) * perLens)
        - vuPositionAtDecibels(VU_RANGE_DB.min + index * perLens),
    );
    for (const step of steps) expect(step).toBeCloseTo(1 / SEGMENTS, 10);
  });

  it("clamps outside the scale", () => {
    expect(vuPositionAtDecibels(-100)).toBe(0);
    expect(vuPositionAtDecibels(40)).toBe(1);
  });
});

describe("vuMeter", () => {
  it("places silence at the bottom of the scale", () => {
    expect(vuDecibels(0)).toBe(VU_RANGE_DB.min);
    expect(vuMeterPosition(0)).toBe(0);
  });

  /* Le haut de la barre est le plein niveau, et non plus −6,1 dBFS. Le mètre
     est lu avant le limiteur : les six derniers décibels avant l'écrêtage
     étaient invisibles, et c'est là que la décision se prend. */
  it("calibrates the top of the bar to full scale", () => {
    expect(vuDecibels(1)).toBeCloseTo(0, 8);
    expect(vuMeterPosition(1)).toBeCloseTo(1, 8);
  });

  it("reads a halved amplitude as six decibels down", () => {
    expect(vuDecibels(0.5)).toBeCloseTo(-6.0206, 4);
  });

  it("moves monotonically and clamps hot signals", () => {
    expect(vuMeterPosition(0.05)).toBeLessThan(vuMeterPosition(0.2));
    expect(vuMeterPosition(0.2)).toBeLessThan(vuMeterPosition(0.8));
    expect(vuMeterPosition(4)).toBe(1);
  });
});
