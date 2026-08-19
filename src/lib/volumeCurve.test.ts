import { describe, expect, it } from "vitest";

import {
  DEFAULT_TRACK_GAIN_DB,
  FILTER_LANE_UNITS,
  volumeDbAtBeat,
  panCentreY,
  panLabel,
  panNodeValue,
  panNodeY,
  LANE_PAIR_UNITS,
  VOLUME_FLOOR_DB,
  VOLUME_MAX_DB,
  automationUnitsAtPointer,
  gainLabel,
  volumeNodeGainDb,
  volumeNodeY,
} from "./volumeCurve";

describe("volumeCurve", () => {
  it("reads back the gain a node is drawn at", () => {
    // The regression that motivated this module: grabbing a node used to
    // resolve its own drawn position to a different gain, so the node jumped
    // to silence before it started following the pointer.
    for (const lane of [0, 1, 2]) {
      for (const gainDb of [VOLUME_MAX_DB, 6, 0.5, 0, -3, -6, -24, -39.9]) {
        const y = volumeNodeY(lane, gainDb);
        expect(volumeNodeGainDb(lane, y)).toBeCloseTo(gainDb, 1);
      }
    }
  });

  it("keeps silence at the bottom of the travel, where it is drawn", () => {
    for (const lane of [0, 1, 2]) {
      const y = volumeNodeY(lane, null);
      expect(volumeNodeGainDb(lane, y)).toBeNull();
      // A node flipping to silence must not move.
      expect(y).toBe(volumeNodeY(lane, VOLUME_FLOOR_DB));
    }
  });

  it("uses the whole lane, so a pixel is worth half what it was", () => {
    // La course occupait 43 % de la voie : le plafond de +12 dB tombait au
    // tiers de la hauteur et tout le bas ne servait à rien. Un même glissé
    // devait donc résoudre le double d'amplitude par pixel.
    const haut = volumeNodeY(0, VOLUME_MAX_DB);
    const bas = volumeNodeY(0, null);
    const course = bas - haut;
    const voie = LANE_PAIR_UNITS - FILTER_LANE_UNITS;
    expect(course / voie).toBeGreaterThan(0.8);
    // Et elle reste dans la voie, marge comprise.
    expect(haut).toBeGreaterThan(FILTER_LANE_UNITS);
    expect(bas).toBeLessThan(LANE_PAIR_UNITS);
  });

  it("puts silence at -40 dB, where a mix stops hearing anything", () => {
    // « Tricher » un peu : le silence arrive par la valeur, pas par la
    // position. Tout ce qui vivait entre −40 et −60 était de la course perdue.
    expect(VOLUME_FLOOR_DB).toBe(-40);
    expect(volumeNodeGainDb(0, volumeNodeY(0, -39.9))).toBeCloseTo(-39.9, 1);
    // Un cran plus bas, c'est le silence — et il se dessine au même endroit,
    // donc saisir un nœud posé au plancher ne le fait pas sauter.
    expect(volumeNodeGainDb(0, volumeNodeY(0, null))).toBeNull();
    expect(volumeNodeY(0, null)).toBe(volumeNodeY(0, VOLUME_FLOOR_DB));
  });

  it("clamps a pointer dragged past either end", () => {
    expect(volumeNodeGainDb(0, -500)).toBe(VOLUME_MAX_DB);
    expect(volumeNodeGainDb(0, 500)).toBeNull();
  });

  it("gives boost and cut the same room, since automation lives near unity", () => {
    const top = volumeNodeY(0, VOLUME_MAX_DB);
    const unity = volumeNodeY(0, 0);
    const bottom = volumeNodeY(0, VOLUME_FLOOR_DB);
    expect(unity - top).toBeCloseTo(bottom - unity, 6);
    expect(top).toBeLessThan(unity);
    expect(unity).toBeLessThan(bottom);
  });

  it("stays inside the audio lane, clear of the filter sub-lane above it", () => {
    for (const lane of [0, 1, 2]) {
      const laneTop = lane * LANE_PAIR_UNITS;
      const highest = volumeNodeY(lane, VOLUME_MAX_DB) - laneTop;
      const lowest = volumeNodeY(lane, null) - laneTop;
      expect(highest).toBeGreaterThan(FILTER_LANE_UNITS);
      expect(lowest).toBeLessThan(LANE_PAIR_UNITS);
    }
  });

  it("maps a pointer through the viewBox rather than through pixels", () => {
    // Half way down a 300 px tall element is half way down the 450 unit box.
    expect(automationUnitsAtPointer(200, 50, 300)).toBeCloseTo(225, 6);
    expect(automationUnitsAtPointer(50, 50, 300)).toBe(0);
    expect(automationUnitsAtPointer(100, 0, 0)).toBe(0);
  });

  it("labels silence and signs the boosts", () => {
    expect(gainLabel(null)).toBe("−∞ dB");
    expect(gainLabel(0)).toBe("0.0 dB");
    expect(gainLabel(3)).toBe("+3.0 dB");
    expect(gainLabel(-6)).toBe("-6.0 dB");
  });
});

describe("pan geometry", () => {
  it("puts left at the top and right at the bottom", () => {
    // Rien dans une image stéréo ne désigne un haut : la convention doit être
    // écrite une fois et tenue partout.
    for (const lane of [0, 1, 2]) {
      expect(panNodeY(lane, -1)).toBeLessThan(panCentreY(lane));
      expect(panNodeY(lane, 1)).toBeGreaterThan(panCentreY(lane));
    }
  });

  it("reads back the value a node is drawn at", () => {
    for (const lane of [0, 1, 2]) {
      for (const value of [-1, -0.5, -0.25, 0, 0.33, 0.75, 1]) {
        expect(panNodeValue(lane, panNodeY(lane, value))).toBeCloseTo(value, 2);
      }
    }
  });

  it("clamps a pointer dragged past either extreme", () => {
    expect(panNodeValue(0, -500)).toBe(-1);
    expect(panNodeValue(0, 500)).toBe(1);
  });

  it("sits inside the audio lane, clear of the filter sub-lane", () => {
    for (const lane of [0, 1, 2]) {
      const top = panNodeY(lane, -1) - lane * LANE_PAIR_UNITS;
      const bottom = panNodeY(lane, 1) - lane * LANE_PAIR_UNITS;
      expect(top).toBeGreaterThan(FILTER_LANE_UNITS);
      expect(bottom).toBeLessThan(LANE_PAIR_UNITS);
    }
  });

  it("labels the sides the way a mixer does", () => {
    expect(panLabel(0)).toBe("C");
    expect(panLabel(-1)).toBe("L100");
    expect(panLabel(0.5)).toBe("R50");
  });
});

describe("volumeDbAtBeat", () => {
  const nodes = [
    { lane: 0, beat: 0, gainDb: -4 },
    { lane: 0, beat: 8, gainDb: -16 },
    { lane: 1, beat: 0, gainDb: 0 },
  ];

  it("falls back to the level the engine applies where nothing is written", () => {
    expect(volumeDbAtBeat([], 0, 4)).toBe(DEFAULT_TRACK_GAIN_DB);
    expect(volumeDbAtBeat(nodes, 2, 4)).toBe(DEFAULT_TRACK_GAIN_DB);
  });

  it("interpolates between the nodes either side", () => {
    expect(volumeDbAtBeat(nodes, 0, 0)).toBe(-4);
    expect(volumeDbAtBeat(nodes, 0, 8)).toBe(-16);
    expect(volumeDbAtBeat(nodes, 0, 4)).toBeCloseTo(-10, 6);
  });

  it("holds the outermost value beyond the ends", () => {
    expect(volumeDbAtBeat(nodes, 0, -5)).toBe(-4);
    expect(volumeDbAtBeat(nodes, 0, 100)).toBe(-16);
  });

  it("reads silence as the floor rather than as nothing", () => {
    expect(volumeDbAtBeat([{ lane: 0, beat: 0, gainDb: null }], 0, 4)).toBe(VOLUME_FLOOR_DB);
  });
});

describe("la courbe de la moitié basse", () => {
  /* Où tombe une position donnée de la course sous l'unité, en fraction. */
  const dbAtTravel = (travel: number) => {
    const top = volumeNodeY(0, 0);
    const bottom = volumeNodeY(0, VOLUME_FLOOR_DB);
    return volumeNodeGainDb(0, top + (bottom - top) * travel);
  };

  /* Le défaut rapporté : à mi-course on entendait −20 dB, c'est-à-dire un
     dixième de l'amplitude, là où l'oreille attend « moitié moins fort ». */
  it("donne à mi-course ce que donne un fader de console", () => {
    expect(dbAtTravel(0.5)).toBeCloseTo(-10, 1);
  });

  /* L'autre moitié du même défaut : deux pixels sous l'unité valaient quatre
     décibels, donc la retouche fine était impossible. */
  it("laisse de la place au travail fin près de l'unité", () => {
    expect(dbAtTravel(0.1)).toBeCloseTo(-0.4, 1);
    expect(dbAtTravel(0.2)).toBeCloseTo(-1.6, 1);
  });

  /* Et le dernier quart cesse d'être perdu : il couvre une vraie descente
     plutôt que la zone où plus rien ne s'entend. */
  it("réserve le dernier quart à la fin de la descente", () => {
    expect(dbAtTravel(0.75)).toBeCloseTo(-22.5, 1);
    expect(dbAtTravel(1)).toBe(null);
  });

  it("descend sans jamais remonter", () => {
    let previous = 0;
    for (let step = 1; step <= 40; step += 1) {
      const db = dbAtTravel(step / 41) ?? VOLUME_FLOOR_DB;
      expect(db).toBeLessThanOrEqual(previous);
      previous = db;
    }
  });

  /* Un cran d'unité, qui vient de l'affichage et non de la courbe : sous
     l'unité la résolution est désormais plus fine que le dixième de décibel
     montré à l'écran, si bien que les tout premiers pour cent lisent 0,0. Deux
     pixels, et plutôt utiles : on peut revenir à l'unité sans viser. Ce qui
     compte est que ça s'arrête là. */
  it("ne garde qu'un cran d'unité de deux pour cent", () => {
    expect(dbAtTravel(0.02)).toBe(-0);
    expect(dbAtTravel(0.06)).toBeLessThanOrEqual(-0.1);
  });

  /* Les deux sens doivent rester d'accord au centième, sinon un nœud saisi
     saute avant de suivre le pointeur — le défaut qui a fait naître ce module. */
  it("fait l'aller-retour sur toute la course basse", () => {
    for (const db of [-0.1, -1, -2.5, -6, -10, -18, -22.5, -30, -39.9]) {
      expect(volumeNodeGainDb(0, volumeNodeY(0, db))).toBeCloseTo(db, 1);
    }
  });
});
