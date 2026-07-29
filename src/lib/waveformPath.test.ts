import { describe, expect, it } from "vitest";

import { waveformChannelPath, waveformRmsPath } from "./waveformPath";

describe("waveformChannelPath", () => {
  it("dessine le maximum vers le haut et le minimum vers le bas", () => {
    expect(waveformChannelPath([-1, -0.5], [0.5, 1], 25, 20)).toBe(
      "M0,15.00 L1,5.00 L1,35.00 L0,45.00 Z",
    );
  });

  it("borne les valeurs invalides ou hors plage", () => {
    expect(waveformChannelPath([-2], [Number.NaN], 50, 10)).toBe(
      "M0,50.00 L0,60.00 Z",
    );
  });

  it("retourne un chemin vide sans échantillon", () => {
    expect(waveformChannelPath([], [], 25, 20)).toBe("");
  });

  it("dessine le corps RMS symétriquement autour de l'axe zéro", () => {
    expect(waveformRmsPath([0.5, 1], 25, 20)).toBe(
      "M0,15.00 L1,5.00 L1,45.00 L0,35.00 Z",
    );
  });
});
