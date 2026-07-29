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

export function sortLibraryTracks(
  tracks: readonly LibraryTrack[],
  inUseTrackIds: ReadonlySet<number>,
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
        comparison = (Number(inUseTrackIds.has(left.id)) - Number(inUseTrackIds.has(right.id))) * direction;
        break;
    }

    if (comparison !== 0) {
      return comparison;
    }

    return left.fileName.localeCompare(right.fileName, undefined, { sensitivity: "base" });
  });
}
