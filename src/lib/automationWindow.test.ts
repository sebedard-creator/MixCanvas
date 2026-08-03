import { describe, expect, it } from "vitest";

import { nodesAcross, visibleBeatRange } from "./automationWindow";

const nodes = [0, 4, 8, 12, 16, 20, 24].map((beat) => ({ beat }));

describe("nodesAcross", () => {
  it("keeps one node on each side of the window", () => {
    const kept = nodesAcross(nodes, 9, 15);

    // 8 et 16 sont hors champ, mais ce sont eux qui donnent la pente des
    // segments qui entrent et qui sortent : les couper deplacerait la ligne.
    expect(kept.map((node) => node.beat)).toEqual([8, 12, 16]);
  });

  it("gives back everything when the window covers the whole mix", () => {
    expect(nodesAcross(nodes, -100, 100)).toBe(nodes);
  });

  it("keeps the neighbour alone when the window sits past the last node", () => {
    expect(nodesAcross(nodes, 40, 60).map((node) => node.beat)).toEqual([24]);
  });

  it("keeps the neighbour alone when the window sits before the first node", () => {
    expect(nodesAcross(nodes, -20, -10).map((node) => node.beat)).toEqual([0]);
  });

  it("has nothing to cut from an empty curve", () => {
    expect(nodesAcross([], 0, 10)).toEqual([]);
  });

  it("refuses to cut on a window it cannot trust", () => {
    // Une largeur nulle ou un zoom pas encore mesure ne doit pas faire
    // disparaitre la courbe : mieux vaut tout tracer que tracer faux.
    expect(nodesAcross(nodes, Number.NaN, 10)).toBe(nodes);
    expect(nodesAcross(nodes, 20, 5)).toBe(nodes);
  });

  it("still spans the window when a node sits exactly on each edge", () => {
    const kept = nodesAcross(nodes, 8, 16);
    expect(kept[0].beat).toBeLessThanOrEqual(8);
    expect(kept[kept.length - 1].beat).toBeGreaterThanOrEqual(16);
  });
});

describe("visibleBeatRange", () => {
  it("centres the window on the view and adds the margin", () => {
    const { fromBeat, toBeat } = visibleBeatRange(100, 10, 900, 200);

    // 450 px de demi-fenetre plus 200 px de marge, a dix pixels le beat.
    expect(fromBeat).toBeCloseTo(100 - 65, 6);
    expect(toBeat).toBeCloseTo(100 + 65, 6);
  });

  it("gives up rather than guess before the panel has been measured", () => {
    const { fromBeat, toBeat } = visibleBeatRange(100, 0, 0, 200);
    expect(fromBeat).toBe(Number.NEGATIVE_INFINITY);
    expect(toBeat).toBe(Number.POSITIVE_INFINITY);
  });
});
