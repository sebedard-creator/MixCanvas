import { describe, expect, it } from "vitest";

import type { LibraryTrack } from "../library/types";
import { sortLibraryTracks, type LibrarySort } from "./librarySort";

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
