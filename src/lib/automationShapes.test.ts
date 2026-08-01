import { describe, expect, it } from "vitest";

import {
  MAX_SHAPE_NODES,
  SHAPE_EDGE_NODES,
  SHAPE_KINDS,
  SHAPE_PERIODS,
  panShapeNodes,
  shapePoints,
  volumeShapeNodes,
} from "./automationShapes";

describe("shapePoints", () => {
  it("stays inside the span it was drawn over", () => {
    for (const kind of SHAPE_KINDS) {
      for (const period of SHAPE_PERIODS) {
        const points = shapePoints(8, 24, kind, period);
        expect(points.length).toBeGreaterThan(0);
        for (const point of points) {
          expect(point.beat).toBeGreaterThanOrEqual(8);
          expect(point.beat).toBeLessThanOrEqual(24);
          expect(point.unit).toBeGreaterThanOrEqual(-1);
          expect(point.unit).toBeLessThanOrEqual(1);
        }
      }
    }
  });

  it("reads the drag in either direction", () => {
    expect(shapePoints(24, 8, "sine", 2)).toEqual(shapePoints(8, 24, "sine", 2));
  });

  it("draws nothing for a drag with no width", () => {
    expect(shapePoints(8, 8, "sine", 1)).toEqual([]);
    expect(shapePoints(8, Number.NaN, "sine", 1)).toEqual([]);
  });

  it("gives a step its doubled nodes, or it would come out a triangle", () => {
    // L'interpolation entre nœuds est linéaire : un palier n'existe que si deux
    // nœuds le tiennent, et une transition franche que si deux nœuds la serrent.
    const points = shapePoints(0, 4, "step", 2);
    const first = points[0];
    const beforeEdge = points[1];
    expect(first.unit).toBe(beforeEdge.unit);
    expect(beforeEdge.beat).toBeGreaterThan(first.beat);
    expect(points[2].unit).toBe(-first.unit);
    // La transition tient en un centième de temps, donc elle se lit comme
    // verticale à tout zoom utile.
    expect(points[2].beat - beforeEdge.beat).toBeLessThan(0.02);
  });

  it("drops resolution before it drops length", () => {
    // Un sinus sur cent vingt-huit mesures doit couvrir toute son étendue,
    // quitte à être plus grossier qu'un sinus sur deux.
    const long = shapePoints(0, 512, "sine", 1);
    expect(long.length).toBeLessThanOrEqual(MAX_SHAPE_NODES + 1);
    // Resserré d'un centième de temps pour laisser place aux ancres de repos.
    expect(long[long.length - 1].beat).toBeCloseTo(512, 1);

    const short = shapePoints(0, 8, "sine", 1);
    const perCycleLong = (long.length - 1) / 512;
    const perCycleShort = (short.length - 1) / 8;
    expect(perCycleLong).toBeLessThan(perCycleShort);
  });

  it("stops where the shape stops being representable, rather than silently thinning", () => {
    // Mille cycles de sinus demandent au moins deux mille nœuds : au-delà du
    // plafond, le trait s'arrête et cela se voit.
    const absurd = shapePoints(0, 4_096, "sine", 1);
    expect(absurd.length).toBeLessThanOrEqual(MAX_SHAPE_NODES + 1);
    const drawnEnd = absurd[absurd.length - 1].beat;
    expect(drawnEnd).toBeLessThan(4_096);
    expect(drawnEnd).toBeGreaterThan(0);
  });

  it("fits two cycles in a beat at the half period", () => {
    // La demi-période sert au trémolo en croches : elle doit donner exactement
    // deux fois plus de cycles que la période d'un temps sur la même étendue.
    const half = shapePoints(0, 8, "step", 0.5);
    const whole = shapePoints(0, 8, "step", 1);
    expect(half.length).toBe(whole.length * 2 - 1);
    expect(half[half.length - 1].beat).toBeCloseTo(8 - 0.01, 6);
  });

  it("sweeps a full cycle per period", () => {
    // Quatre temps à une période de un : quatre cycles, donc quatre crêtes.
    const points = shapePoints(0, 4, "triangle", 1);
    const peaks = points.filter((point) => point.unit > 0.99);
    expect(peaks.length).toBeGreaterThanOrEqual(4);
  });
});

describe("volumeShapeNodes", () => {
  it("hangs from the level already in place and digs downwards", () => {
    const nodes = volumeShapeNodes(0, 8, -4, -16, "triangle", 1);
    const levels = nodes.map((node) => node.gainDb);
    expect(Math.max(...levels)).toBeCloseTo(-4, 6);
    expect(Math.min(...levels)).toBeCloseTo(-16, 6);
    // La crête ne dépasse jamais le niveau réglé : un trémolo qui monterait
    // mangerait la réserve du limiteur à chaque cycle.
    expect(Math.max(...levels)).toBeLessThanOrEqual(-4);
  });

  it("reads a drag above the resting level as an upward shape", () => {
    const nodes = volumeShapeNodes(0, 8, -12, -2, "triangle", 1);
    const levels = nodes.map((node) => node.gainDb);
    expect(Math.max(...levels)).toBeCloseTo(-2, 6);
    expect(Math.min(...levels)).toBeCloseTo(-12, 6);
  });

  it("never leaves the range the engine accepts", () => {
    const nodes = volumeShapeNodes(0, 8, 40, -200, "sine", 1);
    for (const node of nodes) {
      expect(node.gainDb).toBeGreaterThanOrEqual(-60);
      expect(node.gainDb).toBeLessThanOrEqual(12);
    }
  });
});

describe("les ancres de repos", () => {
  // Sans elles, la ligne rampe depuis le nœud d'avant le trait jusqu'à la
  // première valeur de la forme : de l'automation créée *vers* le dessin.
  it("starts and ends a volume shape at the level already in place", () => {
    for (const kind of SHAPE_KINDS) {
      const nodes = volumeShapeNodes(4, 20, -4, -18, kind, 1);
      expect(nodes[0].gainDb).toBeCloseTo(-4, 6);
      expect(nodes[nodes.length - 1].gainDb).toBeCloseTo(-4, 6);
      expect(nodes[0].beat).toBeCloseTo(4, 6);
    }
  });

  it("starts and ends a pan shape at centre by default", () => {
    for (const kind of SHAPE_KINDS) {
      const nodes = panShapeNodes(4, 20, 0.8, kind, 2);
      expect(nodes[0].value).toBe(0);
      expect(nodes[nodes.length - 1].value).toBe(0);
    }
  });

  it("takes whatever pan was in place, not only centre", () => {
    const nodes = panShapeNodes(4, 20, 0.8, "sine", 1, -0.3);
    expect(nodes[0].value).toBeCloseTo(-0.3, 6);
    expect(nodes[nodes.length - 1].value).toBeCloseTo(-0.3, 6);
  });

  it("keeps every node inside the stroke", () => {
    const nodes = volumeShapeNodes(4, 20, -4, -18, "sine", 1);
    for (const node of nodes) {
      expect(node.beat).toBeGreaterThanOrEqual(4);
      expect(node.beat).toBeLessThanOrEqual(20);
    }
  });
});

describe("panShapeNodes", () => {
  it("swings either side of centre", () => {
    const nodes = panShapeNodes(0, 8, 0.6, "triangle", 1);
    const values = nodes.map((node) => node.value);
    expect(Math.max(...values)).toBeCloseTo(0.6, 6);
    expect(Math.min(...values)).toBeCloseTo(-0.6, 6);
  });

  it("takes the reach from the pointer, whichever side it was on", () => {
    // Pointer à gauche ou à droite décrit la même largeur de balancement.
    expect(panShapeNodes(0, 8, -0.6, "sine", 2)).toEqual(
      panShapeNodes(0, 8, 0.6, "sine", 2),
    );
  });

  it("never leaves the stereo field", () => {
    for (const node of panShapeNodes(0, 8, 5, "sine", 1)) {
      expect(node.value).toBeGreaterThanOrEqual(-1);
      expect(node.value).toBeLessThanOrEqual(1);
    }
  });
});

describe("the eight-beat period", () => {
  it("sweeps one cycle over two bars", () => {
    // Le cran le plus long : deux mesures par cycle, pour un balayage qui
    // respire là où quatre temps donnaient encore un motif.
    const points = shapePoints(0, 8, "sine", 8);
    // La forme est resserrée d'un centième de temps à chaque bout, pour laisser
    // la place aux ancres de repos : elle ne part donc pas exactement de zéro.
    expect(points[0].beat).toBeCloseTo(0.01, 6);
    expect(points[points.length - 1].beat).toBeLessThanOrEqual(8);
    // Une seule remontée par le zéro dans le sens montant : un cycle, pas deux.
    let risingZeros = 0;
    for (let i = 1; i < points.length; i++) {
      if (points[i - 1].unit < 0 && points[i].unit >= 0) risingZeros += 1;
    }
    expect(risingZeros).toBe(1);
  });

  it("stays inside the node budget over a long stroke", () => {
    // Le plafond protège la base : un trait de deux cents mesures ne doit pas
    // écrire plus de nœuds qu'on n'en accepte, quelle que soit la période.
    // Ce qui compte est ce que le **serveur** reçoit : la forme, son nœud de
    // fermeture et les deux ancres. Le budget interne les ignorait, et un long
    // trait à la période la plus courte se faisait refuser.
    for (const kind of SHAPE_KINDS) {
      for (const period of SHAPE_PERIODS) {
        for (const span of [100, 400, 800, 4_000]) {
          const budget = MAX_SHAPE_NODES + SHAPE_EDGE_NODES;
          expect(volumeShapeNodes(0, span, -4, -20, kind, period).length)
            .toBeLessThanOrEqual(budget);
          expect(panShapeNodes(0, span, 0.8, kind, period).length)
            .toBeLessThanOrEqual(budget);
        }
      }
    }
  });
});
