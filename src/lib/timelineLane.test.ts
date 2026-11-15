import { describe, expect, it } from "vitest";

import { timelineLaneFromPointer } from "./timelineLane";

describe("timelineLaneFromPointer", () => {
  it("sélectionne chacune des quatre pistes", () => {
    expect(timelineLaneFromPointer(110, 100, 400, 4)).toBe(0);
    expect(timelineLaneFromPointer(250, 100, 400, 4)).toBe(1);
    expect(timelineLaneFromPointer(390, 100, 400, 4)).toBe(2);
    expect(timelineLaneFromPointer(490, 100, 400, 4)).toBe(3);
  });

  it("borne le pointeur avant ou après les pistes", () => {
    expect(timelineLaneFromPointer(20, 100, 400, 4)).toBe(0);
    expect(timelineLaneFromPointer(800, 100, 400, 4)).toBe(3);
  });

  it("retourne la première piste pour une géométrie invalide", () => {
    expect(timelineLaneFromPointer(100, 100, 0, 3)).toBe(0);
    expect(timelineLaneFromPointer(Number.NaN, 100, 400, 4)).toBe(0);
  });
});
