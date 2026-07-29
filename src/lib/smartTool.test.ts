import { describe, expect, it } from "vitest";

import { smartToolAt, type SmartToolContext } from "./smartTool";
import { TRIM_GRAB_PX } from "./clipTrim";

const CLIP = {
  visualStartBeat: 16,
  visualEndBeat: 32,
  trimStartBeats: 4,
  trimEndBeats: 4,
};

const HEADING = 18;

function at(overrides: Partial<SmartToolContext>): SmartToolContext {
  return {
    beat: 24,
    offsetY: 30,
    headingHeight: HEADING,
    pixelsPerBeat: 16,
    canDraw: true,
    ...overrides,
  };
}

describe("smartToolAt", () => {
  it("draws in the body and moves from the heading", () => {
    expect(smartToolAt(CLIP, at({ offsetY: HEADING }))).toBe("draw");
    expect(smartToolAt(CLIP, at({ offsetY: HEADING - 1 }))).toBe("move");
    expect(smartToolAt(CLIP, at({ offsetY: 0 }))).toBe("move");
  });

  it("gives the edges to the trim tool", () => {
    expect(smartToolAt(CLIP, at({ beat: CLIP.visualStartBeat }))).toBe("trim-start");
    expect(smartToolAt(CLIP, at({ beat: CLIP.visualEndBeat }))).toBe("trim-end");
  });

  it("keeps the edge an edge even up in the heading", () => {
    // Sinon les sept pixels de prise du rognage disparaîtraient sur toute la
    // hauteur du titre, et il n'y aurait plus qu'un ruban pour les attraper.
    expect(smartToolAt(CLIP, at({ beat: CLIP.visualStartBeat, offsetY: 2 }))).toBe("trim-start");
    expect(smartToolAt(CLIP, at({ beat: CLIP.visualEndBeat, offsetY: 2 }))).toBe("trim-end");
  });

  it("falls back to moving when no automation line is shown", () => {
    // Un crayon sans ligne où écrire promettrait un geste impossible.
    expect(smartToolAt(CLIP, at({ canDraw: false }))).toBe("move");
    // Mais les bords, eux, ne dépendent d'aucune ligne.
    expect(smartToolAt(CLIP, at({ beat: CLIP.visualStartBeat, canDraw: false }))).toBe(
      "trim-start",
    );
  });

  it("stops offering the trim once the pointer has left the grab band", () => {
    const justInside = CLIP.visualStartBeat + (TRIM_GRAB_PX + 1) / 16;
    expect(smartToolAt(CLIP, at({ beat: justInside }))).toBe("draw");
  });

  it("draws right down to the bottom of the clip", () => {
    // Le corps entier moins la barre : c'est le choix qui a été fait contre une
    // coupe à mi-hauteur, pour maximiser la surface de dessin.
    expect(smartToolAt(CLIP, at({ offsetY: 999 }))).toBe("draw");
  });
});
