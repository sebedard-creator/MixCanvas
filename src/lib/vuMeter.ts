/**
 * L'échelle du VU-mètre : **un nombre fixe de décibels par diode**.
 *
 * Elle a longtemps repris l'espacement d'un cadran VU à aiguille — dix
 * décibels tassés dans les quatre premières diodes, trois étalés sur les six
 * dernières. Cet espacement a un sens sur un cadran : les chiffres sont
 * imprimés sous l'aiguille, et l'œil lit une position contre une graduation.
 * Sur une barre de diodes nues il n'en a aucun. Chaque diode valait autre
 * chose que sa voisine — 2,31 dB en bas contre 0,50 dB en haut, quatre fois et
 * demie d'écart — sans rien pour le dire.
 *
 * L'ancienne référence était par ailleurs une amplitude de 0,35, soit
 * −9,1 dBFS pour « 0 VU », et le haut de l'échelle tombait à −6,1 dBFS. Les
 * six derniers décibels avant l'écrêtage n'existaient donc pas à l'écran, et
 * c'est précisément là que la décision se prend. Le mètre est lu avant le
 * limiteur : il doit pouvoir montrer ce que le limiteur va rattraper.
 *
 * La graduation part maintenant du plein niveau. Sur vingt-quatre diodes,
 * quarante décibels font 1,67 dB par diode, partout, et la dernière est
 * l'écrêtage lui-même.
 */
const MIN_VU_DB = -40;
const MAX_VU_DB = 0;

/**
 * Sous ce niveau, le mix n'occupe pas la place dont il dispose : ce n'est pas
 * une faute, mais ce n'est pas non plus un niveau où mixer.
 *
 * C'est le même niveau sonore qu'avant, réécrit dans la nouvelle référence :
 * l'ancien seuil valait −7 VU sous une référence de 0,35, soit −16 dBFS.
 * Changer de graduation ne doit pas déplacer une frontière de couleur en
 * douce.
 */
export const VU_TOO_LOW_DB = -16;

/** Le niveau en dBFS, borné à la plage que la barre sait montrer. */
export function vuDecibels(level: number): number {
  if (!Number.isFinite(level) || level <= 0) {
    return MIN_VU_DB;
  }
  return Math.max(MIN_VU_DB, Math.min(MAX_VU_DB, 20 * Math.log10(level)));
}

/** Où un niveau tombe le long de la barre, de 0 à gauche à 1 à droite. */
export function vuMeterPosition(level: number): number {
  return vuPositionAtDecibels(vuDecibels(level));
}

/** Où une valeur en décibels tombe le long de la barre. */
export function vuPositionAtDecibels(db: number): number {
  const clamped = Math.max(MIN_VU_DB, Math.min(MAX_VU_DB, db));
  return (clamped - MIN_VU_DB) / (MAX_VU_DB - MIN_VU_DB);
}

/** Les bornes de la barre, pour qui doit les annoncer plutôt que les dessiner. */
export const VU_RANGE_DB = { min: MIN_VU_DB, max: MAX_VU_DB } as const;

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
