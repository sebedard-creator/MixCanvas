import type { LibraryTrack } from "../library/types";

export type LibrarySortKey = "artist" | "title" | "bpm" | "inUse";
export type LibrarySortDirection = "ascending" | "descending";

export interface LibrarySort {
  key: LibrarySortKey;
  direction: LibrarySortDirection;
}

/** Par artiste, croissant : l'ordre d'une étagère à disques. */
export const DEFAULT_LIBRARY_SORT: LibrarySort = { key: "artist", direction: "ascending" };

/** La clé sous laquelle le tri est retenu d'une séance à l'autre. */
export const LIBRARY_SORT_PREFERENCE = "library.sort";

const SORT_KEYS: readonly LibrarySortKey[] = ["artist", "title", "bpm", "inUse"];
const SORT_DIRECTIONS: readonly LibrarySortDirection[] = ["ascending", "descending"];

/**
 * Relit le tri retenu, et **retombe sur le défaut à la moindre surprise**.
 *
 * Ce qui arrive ici vient d'un fichier que rien n'empêche d'être vieux, écrit
 * par une version qui connaissait d'autres colonnes, ou modifié à la main.
 * Une préférence illisible ne doit jamais empêcher la bibliothèque de
 * s'afficher : le pire qu'elle puisse coûter est un tri à refaire une fois.
 */
export function parseLibrarySort(raw: string | undefined | null): LibrarySort {
  if (typeof raw !== "string") return DEFAULT_LIBRARY_SORT;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_LIBRARY_SORT;
  }
  if (typeof parsed !== "object" || parsed === null) return DEFAULT_LIBRARY_SORT;
  const { key, direction } = parsed as Partial<LibrarySort>;
  // Les deux champs sont vérifiés séparément : une colonne disparue ne doit pas
  // faire perdre le sens du tri, ni l'inverse.
  return {
    key: SORT_KEYS.includes(key as LibrarySortKey) ? (key as LibrarySortKey) : DEFAULT_LIBRARY_SORT.key,
    direction: SORT_DIRECTIONS.includes(direction as LibrarySortDirection)
      ? (direction as LibrarySortDirection)
      : DEFAULT_LIBRARY_SORT.direction,
  };
}

/** Ce qu'on range. Du JSON, pour que la forme puisse grandir sans migration. */
export function serializeLibrarySort(sort: LibrarySort): string {
  return JSON.stringify({ key: sort.key, direction: sort.direction });
}

function compareOptionalText(
  left: string | null,
  right: string | null,
  direction: number,
): number {
  const normalizedLeft = left?.trim() || null;
  const normalizedRight = right?.trim() || null;
  if (normalizedLeft === null || normalizedRight === null) {
    return Number(normalizedLeft === null) - Number(normalizedRight === null);
  }
  return normalizedLeft.localeCompare(normalizedRight, undefined, { sensitivity: "base" }) * direction;
}

function compareOptionalNumber(left: number | null, right: number | null, direction: number): number {
  if (left === null || right === null) {
    return Number(left === null) - Number(right === null);
  }
  return (left - right) * direction;
}

/**
 * Trie la bibliothèque.
 *
 * `timelineOrder` donne, pour un morceau posé sur la timeline, son rang dans
 * l'ordre où on l'entend — et rien pour les autres. Un simple « oui/non »
 * rangeait les morceaux utilisés dans un tas informe : ce qu'on veut voir,
 * c'est le mix dans l'ordre. Les absents suivent la même règle que les autres
 * valeurs manquantes du fichier — ils passent en dernier, quel que soit le sens
 * du tri, parce qu'une liste dont la queue change de contenu selon le sens se
 * relit mal.
 */
export function sortLibraryTracks(
  tracks: readonly LibraryTrack[],
  timelineOrder: ReadonlyMap<number, number>,
  sort: LibrarySort,
): LibraryTrack[] {
  const direction = sort.direction === "ascending" ? 1 : -1;

  return [...tracks].sort((left, right) => {
    let comparison: number;
    switch (sort.key) {
      case "artist":
        comparison = compareOptionalText(left.artist, right.artist, direction);
        break;
      case "title":
        comparison = compareOptionalText(left.title ?? left.fileName, right.title ?? right.fileName, direction);
        break;
      case "bpm":
        comparison = compareOptionalNumber(left.bpm, right.bpm, direction);
        break;
      case "inUse":
        comparison = compareOptionalNumber(
          timelineOrder.get(left.id) ?? null,
          timelineOrder.get(right.id) ?? null,
          direction,
        );
        break;
    }

    if (comparison !== 0) {
      return comparison;
    }

    return left.fileName.localeCompare(right.fileName, undefined, { sensitivity: "base" });
  });
}
