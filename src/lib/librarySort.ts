import type { LibraryTrack } from "../library/types";

export type LibrarySortKey = "artist" | "title" | "bpm" | "inUse";
export type LibrarySortDirection = "ascending" | "descending";

export interface LibrarySort {
  key: LibrarySortKey;
  direction: LibrarySortDirection;
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
