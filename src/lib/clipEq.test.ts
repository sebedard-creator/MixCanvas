import { describe, expect, it } from "vitest";
import {
  CLIP_EQ_GAIN_MAX_DB,
  CLIP_EQ_PEAK_MAX_DB,
  CLIP_EQ_SILENCE_DB,
  DEFAULT_CLIP_EQ,
  graphPointToViewBox,
  isClipEqActive,
  isClipEqSilent,
  parseClipEqGainDb,
  sanitizeClipEq,
} from "./clipEq";

describe("clipEq sanitize utility", () => {
  it("returns default EQ settings when input is missing", () => {
    expect(sanitizeClipEq(undefined)).toEqual(DEFAULT_CLIP_EQ);
    expect(sanitizeClipEq(null)).toEqual(DEFAULT_CLIP_EQ);
  });

  it("clamps high pass and low pass frequencies within 20Hz - 20000Hz range", () => {
    const sanitized = sanitizeClipEq({ highPassHz: 1, lowPassHz: 30000 });
    expect(sanitized.highPassHz).toBe(20);
    expect(sanitized.lowPassHz).toBe(20000);
  });

  it("ensures low pass frequency is never below high pass frequency", () => {
    const sanitized = sanitizeClipEq({ highPassHz: 1000, lowPassHz: 500 });
    expect(sanitized.highPassHz).toBe(1000);
    expect(sanitized.lowPassHz).toBe(1000);
  });

  it("clamps gains to the ranges the engine accepts", () => {
    expect(sanitizeClipEq({ peakGainDb: 12 }).peakGainDb).toBe(CLIP_EQ_PEAK_MAX_DB);
    expect(sanitizeClipEq({ gainDb: 40 }).gainDb).toBe(CLIP_EQ_GAIN_MAX_DB);
    expect(sanitizeClipEq({ peakGainDb: -200 }).peakGainDb).toBe(CLIP_EQ_SILENCE_DB);
    expect(sanitizeClipEq({ peakQ: 50 }).peakQ).toBe(10);
    expect(sanitizeClipEq({ peakQ: 0 }).peakQ).toBe(0.1);
  });

  it("never lets a non-finite gain reach the IPC, since JSON turns it into null", () => {
    // JSON.stringify(-Infinity) is "null", which Rust reads as "no gain set"
    // and skips entirely: the cut would silently do nothing.
    const sanitized = sanitizeClipEq({ peakGainDb: -Infinity, gainDb: Number.NaN });
    expect(sanitized.peakGainDb).toBe(CLIP_EQ_SILENCE_DB);
    expect(sanitized.gainDb).toBe(0);

    for (const value of Object.values(sanitized)) {
      if (typeof value === "number") {
        expect(Number.isFinite(value)).toBe(true);
      }
    }
    expect(JSON.parse(JSON.stringify(sanitized))).toEqual(sanitized);
  });

  it("marks a gain at or below the silence floor as a full cut", () => {
    expect(isClipEqSilent(CLIP_EQ_SILENCE_DB)).toBe(true);
    expect(isClipEqSilent(CLIP_EQ_SILENCE_DB - 1)).toBe(true);
    expect(isClipEqSilent(-36)).toBe(false);
    expect(isClipEqSilent(0)).toBe(false);
  });

  it("detects when EQ is active vs neutral", () => {
    expect(isClipEqActive(undefined)).toBe(false);
    expect(isClipEqActive(DEFAULT_CLIP_EQ)).toBe(false);
    expect(isClipEqActive({ enabled: false, highPassHz: 200 })).toBe(false);
    expect(isClipEqActive({ highPassHz: 200 })).toBe(true);
    expect(isClipEqActive({ lowPassHz: 8000 })).toBe(true);
    expect(isClipEqActive({ peakGainDb: 3 })).toBe(true);
    expect(isClipEqActive({ gainDb: CLIP_EQ_SILENCE_DB })).toBe(true);
  });
});

describe("parseClipEqGainDb", () => {
  it("lit les formes qu'on tape naturellement", () => {
    expect(parseClipEqGainDb("-6")).toBe(-6);
    expect(parseClipEqGainDb("+3")).toBe(3);
    expect(parseClipEqGainDb("3.5")).toBe(3.5);
    expect(parseClipEqGainDb("  0 ")).toBe(0);
  });

  it("accepte la virgule décimale et l'unité recopiée", () => {
    // Un clavier français met une virgule; l'affichage montre « dB », et on le
    // recopie volontiers avec.
    expect(parseClipEqGainDb("3,5")).toBe(3.5);
    expect(parseClipEqGainDb("-6 dB")).toBe(-6);
    expect(parseClipEqGainDb("-6dB")).toBe(-6);
  });

  it("accepte le signe moins d'Unicode", () => {
    // Celui que colle un traitement de texte, et qui n'est pas le trait d'union
    // du clavier : sans cela la saisie était refusée sans qu'on comprenne.
    expect(parseClipEqGainDb("−6")).toBe(-6);
  });

  it("écrit le silence en toutes lettres", () => {
    for (const forme of ["-inf", "-∞", "inf", "-infinity", "-INF"]) {
      expect(parseClipEqGainDb(forme)).toBe(CLIP_EQ_SILENCE_DB);
    }
  });

  it("borne aux limites du réglage", () => {
    expect(parseClipEqGainDb("99")).toBe(CLIP_EQ_GAIN_MAX_DB);
    expect(parseClipEqGainDb("-500")).toBe(CLIP_EQ_SILENCE_DB);
  });

  it("rend null plutôt que d'inventer une valeur", () => {
    // Un champ qui répondrait zéro à une frappe malheureuse couperait le clip
    // sans prévenir.
    for (const absurde of ["", "   ", "abc", "3 dB 4", "--6", "+"]) {
      expect(parseClipEqGainDb(absurde)).toBeNull();
    }
  });
});

describe("graphPointToViewBox", () => {
  const rect = { left: 100, top: 50, width: 290, height: 110 };

  it("scales screen pixels into the drawing's own units", () => {
    // Le graphe fait 580 × 220 unités pour 290 × 110 pixels : chaque pixel
    // vaut deux unités. Lu sans conversion, le milieu tombait au quart.
    expect(graphPointToViewBox(100, 50, rect, 580, 220)).toEqual({ x: 0, y: 0 });
    expect(graphPointToViewBox(245, 105, rect, 580, 220)).toEqual({ x: 290, y: 110 });
    expect(graphPointToViewBox(390, 160, rect, 580, 220)).toEqual({ x: 580, y: 220 });
  });

  it("puts the handle exactly under the pointer, at any scale", () => {
    // L'invariant qui compte : la fraction parcourue à l'écran est la fraction
    // parcourue dans le dessin. Sans elle, la poignée traîne d'autant plus
    // qu'on s'éloigne du bord — l'effet élastique rapporté.
    for (const width of [200, 290, 580, 900]) {
      const scaled = { left: 0, top: 0, width, height: width / 2 };
      for (const fraction of [0, 0.25, 0.5, 0.75, 1]) {
        const point = graphPointToViewBox(width * fraction, 0, scaled, 580, 220);
        expect(point.x / 580).toBeCloseTo(fraction, 10);
      }
    }
  });

  it("returns the origin rather than infinity for a collapsed element", () => {
    expect(graphPointToViewBox(10, 10, { left: 0, top: 0, width: 0, height: 0 }, 580, 220))
      .toEqual({ x: 0, y: 0 });
  });
});
