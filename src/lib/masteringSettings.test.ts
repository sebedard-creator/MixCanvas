import { describe, expect, it } from "vitest";

import {
  DEFAULT_BOUNCE_FORMAT,
  DEFAULT_MASTERING_SETTINGS,
  MASTERING_LIMITS,
  ceilingForFormat,
  masteringGainDb,
  parseBounceFormat,
  parseMasteringSettings,
  serializeMasteringSettings,
} from "./masteringSettings";

describe("parseMasteringSettings", () => {
  it("survives a round trip", () => {
    const settings = { thresholdDb: -6, ceilingDb: -0.3, releaseMs: 120, autoRelease: false };
    expect(parseMasteringSettings(serializeMasteringSettings(settings))).toEqual(settings);
  });

  it("falls back to the defaults on anything unreadable", () => {
    for (const raw of [null, undefined, "", "not json", "42", "[]"]) {
      expect(parseMasteringSettings(raw)).toEqual(DEFAULT_MASTERING_SETTINGS);
    }
  });

  /* Champ par champ, et c'est le point : une préférence écrite par une version
     plus ancienne, ou tronquée, ne doit pas emporter tout le réglage avec
     elle. */
  it("keeps the fields it can read and defaults only the rest", () => {
    const parsed = parseMasteringSettings(JSON.stringify({ ceilingDb: -0.5 }));
    expect(parsed.ceilingDb).toBe(-0.5);
    expect(parsed.thresholdDb).toBe(DEFAULT_MASTERING_SETTINGS.thresholdDb);
    expect(parsed.autoRelease).toBe(DEFAULT_MASTERING_SETTINGS.autoRelease);
  });

  it("clamps a value that would ask the limiter for nonsense", () => {
    const parsed = parseMasteringSettings(
      JSON.stringify({ thresholdDb: -400, ceilingDb: 12, releaseMs: -3 }),
    );
    expect(parsed.thresholdDb).toBe(MASTERING_LIMITS.thresholdDb.min);
    expect(parsed.ceilingDb).toBe(MASTERING_LIMITS.ceilingDb.max);
    expect(parsed.releaseMs).toBe(MASTERING_LIMITS.releaseMs.min);
  });
});

describe("masteringGainDb", () => {
  /* Deux nombres négatifs ne disent pas qu'on demande du gain, et c'est
     exactement ce que la boîte doit montrer avant de lancer un rendu. */
  it("reads the lift the two thresholds are asking for", () => {
    expect(masteringGainDb({ ...DEFAULT_MASTERING_SETTINGS })).toBeCloseTo(3.6, 10);
    expect(
      masteringGainDb({ ...DEFAULT_MASTERING_SETTINGS, thresholdDb: -12, ceilingDb: -1 }),
    ).toBeCloseTo(11, 10);
  });

  it("never reports a negative lift", () => {
    expect(
      masteringGainDb({ ...DEFAULT_MASTERING_SETTINGS, thresholdDb: -0.1, ceilingDb: -3 }),
    ).toBe(0);
  });
});

describe("parseBounceFormat", () => {
  it("reads back what it was given", () => {
    expect(parseBounceFormat("mp3")).toBe("mp3");
    expect(parseBounceFormat("wav")).toBe("wav");
  });

  /* Le sens du défaut compte : croire qu'on garde un WAV et n'avoir qu'un MP3
     ne se rattrape pas, l'inverse se réencode. */
  it("falls back to lossless on anything it cannot read", () => {
    for (const raw of [null, undefined, "", "flac", "MP3 "]) {
      expect(parseBounceFormat(raw)).toBe(DEFAULT_BOUNCE_FORMAT);
    }
    expect(DEFAULT_BOUNCE_FORMAT).toBe("wav");
  });
});

describe("ceilingForFormat", () => {
  /* Mesuré sur un mix réel : le WAV s'arrête à −0,0997 dB, le MP3 décodé monte
     à +0,385. La marge n'est pas une précaution de principe. */
  it("leaves a decibel of room for a lossy codec", () => {
    expect(ceilingForFormat("wav")).toBe(-0.1);
    expect(ceilingForFormat("mp3")).toBe(-1.0);
    expect(ceilingForFormat("mp3")).toBeLessThan(ceilingForFormat("wav"));
  });
});
