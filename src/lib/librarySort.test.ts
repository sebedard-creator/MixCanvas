import { describe, expect, it } from "vitest";

import type { LibraryTrack } from "../library/types";
import {
  DEFAULT_LIBRARY_SORT,
  parseLibrarySort,
  serializeLibrarySort,
  sortLibraryTracks,
  type LibrarySort,
} from "./librarySort";

function track(id: number, overrides: Partial<LibraryTrack> = {}): LibraryTrack {
  return {
    id,
    filePath: `${id}.mp3`,
    fileName: `${id}.mp3`,
    artist: null,
    title: null,
    durationMs: 0,
    sampleRate: 44_100,
    channels: 2,
    bpm: null,
    analyzedBpm: null,
    bpmConfidence: null,
    firstBeatMs: null,
    analyzedFirstBeatMs: null,
    beatCount: 0,
    isCorrected: false,
    analysisStatus: "analyzed",
    analysisError: null,
    analysisVersion: 1,
    isMissing: false,
    ...overrides,
  };
}

describe("sortLibraryTracks", () => {
  const ascending: LibrarySort = { key: "artist", direction: "ascending" };

  it("sorts known artists alphabetically and leaves missing tags last", () => {
    const tracks = [
      track(1, { artist: null }),
      track(2, { artist: "Bicep" }),
      track(3, { artist: "Aphex Twin" }),
    ];

    expect(sortLibraryTracks(tracks, new Map(), ascending).map(({ id }) => id)).toEqual([3, 2, 1]);
  });

  it("lists the tracks used by the timeline in the order they are heard", () => {
    // Un simple « oui/non » rangeait les morceaux utilisés dans un tas
    // informe. Ce qu'on veut voir, c'est le mix dans l'ordre.
    const tracks = [track(1), track(2), track(3), track(4)];
    const order = new Map([
      [3, 0],
      [1, 1],
      [4, 2],
    ]);

    expect(
      sortLibraryTracks(tracks, order, { key: "inUse", direction: "ascending" }).map(({ id }) => id),
    ).toEqual([3, 1, 4, 2]);
  });

  it("keeps the unused tracks at the end whichever way the order runs", () => {
    // Une liste dont la queue change de contenu selon le sens se relit mal :
    // les absents suivent la règle des autres valeurs manquantes.
    const tracks = [track(1), track(2), track(3)];
    const order = new Map([
      [3, 0],
      [1, 1],
    ]);

    expect(
      sortLibraryTracks(tracks, order, { key: "inUse", direction: "descending" }).map(({ id }) => id),
    ).toEqual([1, 3, 2]);
  });

  it("sorts BPM numerically while keeping tracks without a BPM last", () => {
    const tracks = [track(1, { bpm: null }), track(2, { bpm: 128 }), track(3, { bpm: 120 })];

    expect(
      sortLibraryTracks(tracks, new Map(), { key: "bpm", direction: "ascending" }).map(({ id }) => id),
    ).toEqual([3, 2, 1]);
    expect(
      sortLibraryTracks(tracks, new Map(), { key: "bpm", direction: "descending" }).map(({ id }) => id),
    ).toEqual([2, 3, 1]);
  });
});

describe("parseLibrarySort", () => {
  it("round-trips every sort the interface can produce", () => {
    for (const key of ["artist", "title", "bpm", "inUse"] as const) {
      for (const direction of ["ascending", "descending"] as const) {
        const sort: LibrarySort = { key, direction };
        expect(parseLibrarySort(serializeLibrarySort(sort))).toEqual(sort);
      }
    }
  });

  it("falls back when nothing was ever stored", () => {
    expect(parseLibrarySort(undefined)).toEqual(DEFAULT_LIBRARY_SORT);
    expect(parseLibrarySort(null)).toEqual(DEFAULT_LIBRARY_SORT);
  });

  /** Le fichier peut avoir été écrit par une version antérieure, ou touché à la
   *  main. Une préférence illisible ne doit jamais empêcher la bibliothèque de
   *  s'afficher — le pire qu'elle puisse coûter est un tri à refaire. */
  it("falls back on anything it cannot read", () => {
    for (const raw of ["", "{", "null", "12", '"artist"', "[]"]) {
      expect(parseLibrarySort(raw)).toEqual(DEFAULT_LIBRARY_SORT);
    }
  });

  it("keeps the half it understands", () => {
    // Une colonne disparue ne doit pas faire perdre le sens du tri…
    expect(parseLibrarySort('{"key":"genre","direction":"descending"}')).toEqual({
      key: DEFAULT_LIBRARY_SORT.key,
      direction: "descending",
    });
    // …ni l'inverse.
    expect(parseLibrarySort('{"key":"bpm","direction":"sideways"}')).toEqual({
      key: "bpm",
      direction: DEFAULT_LIBRARY_SORT.direction,
    });
  });

  it("ignores anything else the object carries", () => {
    expect(parseLibrarySort('{"key":"bpm","direction":"descending","secret":1}')).toEqual({
      key: "bpm",
      direction: "descending",
    });
  });
});
