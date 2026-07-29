const MIN_VU_DB = -20;
const MAX_VU_DB = 3;
const VU_REFERENCE_LEVEL = 0.35;
const MIN_NEEDLE_ANGLE = -48;
const MAX_NEEDLE_ANGLE = 48;

const VU_SCALE = [
  { db: -20, position: 0 },
  { db: -10, position: 0.18 },
  { db: -7, position: 0.3 },
  { db: -5, position: 0.4 },
  { db: -3, position: 0.52 },
  { db: -2, position: 0.59 },
  { db: -1, position: 0.67 },
  { db: 0, position: 0.75 },
  { db: 1, position: 0.83 },
  { db: 2, position: 0.91 },
  { db: 3, position: 1 },
] as const;

export const VU_TICK_VALUES = VU_SCALE.map(({ db }) => db);

export function vuDecibels(level: number): number {
  if (!Number.isFinite(level) || level <= 0) {
    return MIN_VU_DB;
  }
  return Math.max(
    MIN_VU_DB,
    Math.min(MAX_VU_DB, 20 * Math.log10(level / VU_REFERENCE_LEVEL)),
  );
}

export function vuMeterPosition(level: number): number {
  const db = vuDecibels(level);
  for (let index = 1; index < VU_SCALE.length; index += 1) {
    const right = VU_SCALE[index];
    if (db <= right.db) {
      const left = VU_SCALE[index - 1];
      const progress = (db - left.db) / (right.db - left.db);
      return left.position + (right.position - left.position) * progress;
    }
  }
  return 1;
}

export function vuNeedleAngle(level: number): number {
  return MIN_NEEDLE_ANGLE + vuMeterPosition(level) * (MAX_NEEDLE_ANGLE - MIN_NEEDLE_ANGLE);
}

/** Where a decibel value falls along the meter, from 0 at the left to 1 at the right. */
export function vuPositionAtDecibels(db: number): number {
  const clamped = Math.max(MIN_VU_DB, Math.min(MAX_VU_DB, db));
  for (let index = 1; index < VU_SCALE.length; index += 1) {
    const right = VU_SCALE[index];
    if (clamped <= right.db) {
      const left = VU_SCALE[index - 1];
      const progress = (clamped - left.db) / (right.db - left.db);
      return left.position + (right.position - left.position) * progress;
    }
  }
  return 1;
}

/**
 * Below this the mix is not using the headroom it has: not a fault, but not a
 * level to mix at either.
 */
export const VU_TOO_LOW_DB = -7;

/** What a lens means, rather than what colour it happens to be. */
export type VuZone = "low" | "safe" | "clip";

/**
 * The zone a lens belongs to.
 *
 * The boundary is written in decibels and converted here, so it keeps its
 * meaning if the number of lenses ever changes. Only the last lens is red: red
 * is reserved for a level that actually distorts, and a meter whose top third
 * is red teaches its user to ignore it.
 */
export function vuSegmentZone(index: number, segmentCount: number): VuZone {
  if (index >= segmentCount - 1) return "clip";
  // The level a lens needs before it lights, i.e. its right-hand edge.
  const litAt = (index + 1) / segmentCount;
  return litAt <= vuPositionAtDecibels(VU_TOO_LOW_DB) ? "low" : "safe";
}
