/**
 * Le réglage du limiteur de mastering, celui du bounce.
 *
 * Rien à voir avec le limiteur du transport, qui est un garde-fou et n'a pas
 * de réglage : celui-ci est un outil, il monte le niveau du mix et garantit un
 * plafond. Les deux ne tournent jamais ensemble — un rendu mastérisé se fait
 * sans le garde-fou, dont le plafond de −0,18 dBFS passerait devant.
 */
export interface MasteringSettings {
  /** Le seuil, en décibels sous la pleine échelle. Négatif. */
  thresholdDb: number;
  /** Le plafond de sortie, en décibels sous la pleine échelle. */
  ceilingDb: number;
  /** Le relâchement en millisecondes, ignoré quand l'automatique est actif. */
  releaseMs: number;
  autoRelease: boolean;
}

/**
 * Les défauts.
 *
 * Ce sont les réglages que Sébastien applique déjà à la main après chaque
 * bounce. Un défaut qui correspond à l'usage réel évite d'avoir à retaper les
 * mêmes nombres à chaque fois.
 */
export const DEFAULT_MASTERING_SETTINGS: MasteringSettings = {
  /**
   * Quatre décibels sous le plafond, et non trois virgule sept.
   *
   * Le seuil venait des réglages d'usine d'un L1. Adoucir le « smiling V » du
   * compresseur a ensuite retiré du niveau à l'étage de couleur : mesuré sur les
   * deux courbes pondérées par un spectre de programme, 0,40 dB de moins —
   * 0,28 dB si l'on pondère en bruit blanc. Le mix arrivait donc au limiteur un
   * peu plus bas qu'avant pour un rendu identique.
   *
   * Quatre tout rond tombe dans cette fourchette et reste un chiffre qu'on peut
   * lire. La différence ne se rattrape que lorsque `COMP` est allumé; sans lui,
   * le bounce gagne les mêmes trois dixièmes, ce qui est le sens où l'on se
   * trompe le moins.
   */
  thresholdDb: -4.0,
  ceilingDb: -0.1,
  releaseMs: 1.0,
  autoRelease: true,
};

export const MASTERING_PREFERENCE = "bounce.mastering";
export const MASTERING_ENABLED_PREFERENCE = "bounce.masteringEnabled";

/** Les bornes de chaque champ, partagées par la saisie et la relecture. */
export const MASTERING_LIMITS = {
  thresholdDb: { min: -24, max: 0 },
  ceilingDb: { min: -6, max: 0 },
  releaseMs: { min: 0.1, max: 2_000 },
} as const;

function clamp(value: unknown, fallback: number, min: number, max: number): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(min, value))
    : fallback;
}

/**
 * Relit un réglage enregistré, champ par champ.
 *
 * Chaque champ retombe sur son défaut indépendamment des autres : une
 * préférence écrite par une version plus ancienne, ou tronquée, ne doit pas
 * emporter tout le réglage avec elle.
 */
export function parseMasteringSettings(raw: string | undefined | null): MasteringSettings {
  if (!raw) return DEFAULT_MASTERING_SETTINGS;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_MASTERING_SETTINGS;
  }
  if (typeof parsed !== "object" || parsed === null) return DEFAULT_MASTERING_SETTINGS;
  const settings = parsed as Partial<MasteringSettings>;
  return {
    thresholdDb: clamp(
      settings.thresholdDb,
      DEFAULT_MASTERING_SETTINGS.thresholdDb,
      MASTERING_LIMITS.thresholdDb.min,
      MASTERING_LIMITS.thresholdDb.max,
    ),
    ceilingDb: clamp(
      settings.ceilingDb,
      DEFAULT_MASTERING_SETTINGS.ceilingDb,
      MASTERING_LIMITS.ceilingDb.min,
      MASTERING_LIMITS.ceilingDb.max,
    ),
    releaseMs: clamp(
      settings.releaseMs,
      DEFAULT_MASTERING_SETTINGS.releaseMs,
      MASTERING_LIMITS.releaseMs.min,
      MASTERING_LIMITS.releaseMs.max,
    ),
    autoRelease:
      typeof settings.autoRelease === "boolean"
        ? settings.autoRelease
        : DEFAULT_MASTERING_SETTINGS.autoRelease,
  };
}

export function serializeMasteringSettings(settings: MasteringSettings): string {
  return JSON.stringify(settings);
}

/**
 * Le gain que ce réglage demande, en décibels.
 *
 * Le seuil ne fait pas que déclencher la limitation : il remonte le niveau
 * jusqu'au plafond. C'est le comportement des limiteurs de mastering, mais il
 * ne se devine pas en lisant deux nombres négatifs — d'où le témoin dans la
 * boîte de dialogue.
 */
export function masteringGainDb(settings: MasteringSettings): number {
  return Math.max(0, settings.ceilingDb - settings.thresholdDb);
}

/** Ce qu'on écrit au bout du rendu. */
export type BounceFormat = "wav" | "mp3";

export const BOUNCE_FORMAT_PREFERENCE = "bounce.format";

/**
 * Le WAV est le défaut, et il le reste.
 *
 * Un master se garde sans perte; le MP3 est ce qu'on envoie ensuite. Se
 * tromper dans ce sens-là — croire qu'on garde un WAV et n'avoir qu'un MP3 —
 * ne se rattrape pas.
 */
export const DEFAULT_BOUNCE_FORMAT: BounceFormat = "wav";

export function parseBounceFormat(raw: string | undefined | null): BounceFormat {
  return raw === "mp3" ? "mp3" : DEFAULT_BOUNCE_FORMAT;
}

export const BOUNCE_FORMATS: { id: BounceFormat; label: string; detail: string }[] = [
  { id: "wav", label: "WAV", detail: "44.1 kHz · 16-bit · interleaved stereo" },
  { id: "mp3", label: "MP3", detail: "44.1 kHz · CBR 320 kbps · stereo · LAME q0" },
];

/**
 * Le plafond qui convient à un format.
 *
 * Un codec avec perte ne reconstruit pas les crêtes à l'identique : mesuré sur
 * un mix de soixante-trois minutes, le MP3 monte jusqu'à +0,385 dB là où le
 * WAV s'arrête net au plafond du limiteur. Un lecteur qui écrête à zéro
 * tranche ces dépassements, et ça s'entend.
 *
 * D'où une marge d'un décibel pour le MP3. Ce n'est pas un réglage de plus :
 * le champ `Ceiling` existe déjà et prend simplement la valeur qui convient,
 * qu'on reste libre de corriger.
 */
export function ceilingForFormat(format: BounceFormat): number {
  return format === "mp3" ? -1.0 : -0.1;
}
