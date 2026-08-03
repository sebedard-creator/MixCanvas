import { describe, expect, it } from "vitest";

import {
  WAVEFORM_WINDOW_QUANTUM_PX,
  waveformWindow,
  windowBucketRange,
} from "./waveformWindow";

describe("waveformWindow", () => {
  it("returns nothing for a clip entirely off screen", () => {
    // Le second gain après la réduction : un clip hors champ ne produit aucune
    // géométrie du tout.
    expect(waveformWindow(4_000, 6_000, 900)).toBeNull();
    expect(waveformWindow(4_000, -9_000, 900)).toBeNull();
  });

  it("covers the visible span with a margin on each side", () => {
    const window = waveformWindow(12_288, 4_000, 900);
    expect(window).not.toBeNull();
    expect(window!.offsetPx).toBeLessThanOrEqual(4_000);
    expect(window!.offsetPx + window!.widthPx).toBeGreaterThanOrEqual(4_900);
  });

  it("never reaches past the clip", () => {
    const atStart = waveformWindow(12_288, -300, 900)!;
    expect(atStart.offsetPx).toBe(0);

    const atEnd = waveformWindow(1_000, 400, 900)!;
    expect(atEnd.offsetPx + atEnd.widthPx).toBeLessThanOrEqual(1_000);
  });

  it("changes about once per step, not once per pixel", () => {
    // C'est ce qui empêche d'avoir troqué un gros calcul rare contre un petit
    // calcul permanent : la lecture avance d'un pixel par image, et la tranche
    // ne doit pas se refaire à chaque fois.
    //
    // La fenêtre s'aligne sur une grille fixe, donc elle bascule au passage
    // d'une ligne — pas un pas après un point arbitraire. Ce qui se vérifie
    // est donc la **fréquence** des bascules, pas leur date.
    const travel = 2_000;
    const seen = new Set<string>();
    for (let creep = 0; creep <= travel; creep += 1) {
      seen.add(JSON.stringify(waveformWindow(12_288, 4_000 + creep, 900)));
    }
    const expected = travel / WAVEFORM_WINDOW_QUANTUM_PX + 1;
    expect(seen.size).toBeLessThanOrEqual(Math.ceil(expected) + 1);
    expect(seen.size).toBeGreaterThan(1);

    // Et entre deux lignes de grille, elle ne bouge pas d'un cheveu.
    const settled = waveformWindow(12_288, 4_096, 900)!;
    for (let creep = 1; creep < WAVEFORM_WINDOW_QUANTUM_PX; creep += 17) {
      expect(waveformWindow(12_288, 4_096 + creep, 900)).toEqual(settled);
    }
  });

  it("asks for a slice, not the whole clip", () => {
    // Le cœur du gain, mesuré sur le cas réel : douze mille pixels de clip pour
    // neuf cents visibles.
    const window = waveformWindow(12_288, 4_000, 900)!;
    expect(window.widthPx).toBeLessThan(12_288 / 5);
  });

  it("gives the whole clip when the whole clip fits", () => {
    const window = waveformWindow(600, 0, 900)!;
    expect(window.offsetPx).toBe(0);
    expect(window.widthPx).toBe(600);
  });

  it("refuses nonsense rather than producing a broken slice", () => {
    expect(waveformWindow(0, 0, 900)).toBeNull();
    expect(waveformWindow(1_000, Number.NaN, 900)).toBeNull();
    expect(waveformWindow(1_000, 0, 0)).toBeNull();
  });
});

describe("windowBucketRange", () => {
  it("maps the slice onto the sample series", () => {
    const range = windowBucketRange({ offsetPx: 0, widthPx: 500 }, 1_000, 16_384);
    expect(range.from).toBe(0);
    expect(range.to).toBe(8_192);
  });

  it("widens outwards so a straddling column is still whole", () => {
    // Sans cet élargissement, chaque frontière de tranche laisse une marche
    // d'un pixel.
    const range = windowBucketRange({ offsetPx: 333, widthPx: 334 }, 1_000, 3_000);
    expect(range.from).toBeLessThanOrEqual(999);
    expect(range.to).toBeGreaterThanOrEqual(2_001);
  });

  it("always yields at least one column", () => {
    const range = windowBucketRange({ offsetPx: 0, widthPx: 1 }, 1_000_000, 8);
    expect(range.to).toBeGreaterThan(range.from);
  });
});
