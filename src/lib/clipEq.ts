import { ClipEqSettings } from "../timeline/types";

/**
 * Gain at or below which a Clip EQ band is treated as a full cut ("−∞ dB").
 *
 * `-Infinity` cannot cross the Tauri IPC: `JSON.stringify(-Infinity)` produces
 * `null`, which the Rust side reads as "no gain set" and silently ignores. The
 * cut is therefore expressed as a finite floor, and `CLIP_EQ_SILENCE_DB` in
 * `src-tauri/src/audio/timeline.rs` holds the same value so the interface and
 * the engine agree on where silence begins. −60 dB is also the floor used by
 * the Volume Node automation.
 */
export const CLIP_EQ_SILENCE_DB = -60;
export const CLIP_EQ_MIN_FREQ_HZ = 20;
export const CLIP_EQ_MAX_FREQ_HZ = 20000;
export const CLIP_EQ_PEAK_MAX_DB = 6;
export const CLIP_EQ_GAIN_MAX_DB = 12;

export const DEFAULT_CLIP_EQ: ClipEqSettings = {
  highPassHz: CLIP_EQ_MIN_FREQ_HZ,
  lowPassHz: CLIP_EQ_MAX_FREQ_HZ,
  peakHz: 1000,
  peakGainDb: 0,
  peakQ: 1.0,
  gainDb: 0,
  enabled: true,
};

/** True when this gain means a complete cut rather than an attenuation. */
/**
 * Ce qu'un utilisateur peut taper dans la case de gain, et ce que ça vaut.
 *
 * Rend `null` quand la saisie ne veut rien dire : l'appelant garde alors la
 * valeur précédente plutôt que d'en inventer une. Un champ qui répond zéro à
 * une frappe malheureuse coupe un clip sans prévenir.
 *
 * Ce qui est accepté, et pourquoi :
 *   `-6` `+3` `3.5`   les formes évidentes, le plus signé compris;
 *   `3,5`             la virgule décimale, que tape un clavier français;
 *   `-6 dB`           l'unité, qu'on recopie volontiers depuis l'affichage;
 *   `-inf` `-∞`       le silence, qui n'a pas d'écriture numérique;
 *   `−6`              le vrai signe moins d'Unicode, que colle un traitement
 *                     de texte et qui n'est pas le trait d'union du clavier.
 */
export function parseClipEqGainDb(raw: string): number | null {
  const text = raw
    .trim()
    .toLowerCase()
    .replace(/−/g, "-")
    .replace(/\s*db$/, "")
    .trim();
  if (text === "") return null;
  if (/^-?\s*(inf|infinity|∞)$/.test(text)) return CLIP_EQ_SILENCE_DB;

  // La forme est vérifiée avant d'être lue : `Number("")` vaut zéro, si bien
  // qu'un « + » seul coupait le clip de douze décibels au lieu d'être refusé.
  const numeric = text.replace(",", ".");
  if (!/^[+-]?\d+(\.\d+)?$/.test(numeric)) return null;
  const value = Number(numeric);
  if (!Number.isFinite(value)) return null;
  const rounded = Math.round(value * 10) / 10;
  return Math.max(CLIP_EQ_SILENCE_DB, Math.min(CLIP_EQ_GAIN_MAX_DB, rounded));
}

export function isClipEqSilent(gainDb: number): boolean {
  return gainDb <= CLIP_EQ_SILENCE_DB;
}

function clampDb(value: number | undefined, maximum: number): number {
  if (value === undefined || !Number.isFinite(value)) {
    return value !== undefined && value === -Infinity ? CLIP_EQ_SILENCE_DB : 0;
  }
  return Math.max(CLIP_EQ_SILENCE_DB, Math.min(maximum, value));
}

function clampFrequency(value: number | undefined, fallback: number): number {
  if (value === undefined || !Number.isFinite(value)) {
    return fallback;
  }
  return Math.max(CLIP_EQ_MIN_FREQ_HZ, Math.min(CLIP_EQ_MAX_FREQ_HZ, value));
}

/**
 * Brings any partial or out-of-range settings back into the ranges the engine
 * accepts. Every save goes through this so no non-finite value can reach IPC.
 */
export function sanitizeClipEq(settings?: Partial<ClipEqSettings> | null): ClipEqSettings {
  if (!settings) return { ...DEFAULT_CLIP_EQ };

  const highPassHz = clampFrequency(settings.highPassHz, CLIP_EQ_MIN_FREQ_HZ);
  const lowPassHz = Math.max(
    highPassHz,
    clampFrequency(settings.lowPassHz, CLIP_EQ_MAX_FREQ_HZ),
  );

  return {
    highPassHz,
    lowPassHz,
    peakHz: clampFrequency(settings.peakHz, 1000),
    peakGainDb: clampDb(settings.peakGainDb, CLIP_EQ_PEAK_MAX_DB),
    peakQ: Number.isFinite(settings.peakQ)
      ? Math.max(0.1, Math.min(10, settings.peakQ as number))
      : 1.0,
    gainDb: clampDb(settings.gainDb, CLIP_EQ_GAIN_MAX_DB),
    enabled: settings.enabled ?? true,
  };
}

export function isClipEqActive(eqSettings?: Partial<ClipEqSettings> | null): boolean {
  if (!eqSettings) return false;
  if (eqSettings.enabled === false) return false;
  const hp = eqSettings.highPassHz ?? CLIP_EQ_MIN_FREQ_HZ;
  const lp = eqSettings.lowPassHz ?? CLIP_EQ_MAX_FREQ_HZ;
  const peakGain = eqSettings.peakGainDb ?? 0;
  const clipGain = eqSettings.gainDb ?? 0;
  return (
    hp > CLIP_EQ_MIN_FREQ_HZ ||
    lp < CLIP_EQ_MAX_FREQ_HZ ||
    Math.abs(peakGain) > 0.01 ||
    Math.abs(clipGain) > 0.01
  );
}
