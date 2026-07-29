import { describe, expect, it } from "vitest";

import { timelineLaneFromPointer } from "./timelineLane";

describe("timelineLaneFromPointer", () => {
  it("sélectionne chacune des trois pistes", () => {
    expect(timelineLaneFromPointer(110, 100, 300, 3)).toBe(0);
    expect(timelineLaneFromPointer(250, 100, 300, 3)).toBe(1);
    expect(timelineLaneFromPointer(390, 100, 300, 3)).toBe(2);
  });

  it("borne le pointeur avant ou après les pistes", () => {
    expect(timelineLaneFromPointer(20, 100, 300, 3)).toBe(0);
    expect(timelineLaneFromPointer(800, 100, 300, 3)).toBe(2);
  });

  it("retourne la première piste pour une géométrie invalide", () => {
    expect(timelineLaneFromPointer(100, 100, 0, 3)).toBe(0);
    expect(timelineLaneFromPointer(Number.NaN, 100, 300, 3)).toBe(0);
  });
});
