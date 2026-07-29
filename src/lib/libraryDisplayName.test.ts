import { describe, expect, it } from "vitest";

import { libraryDisplayName } from "./libraryDisplayName";

describe("libraryDisplayName", () => {
  it("shows artist and title when both ID3 fields are available", () => {
    expect(
      libraryDisplayName({ artist: "Bicep", title: "Glue", fileName: "track-01.mp3" }),
    ).toBe("Bicep - Glue");
  });

  it("uses the available tag before falling back to the filename", () => {
    expect(libraryDisplayName({ artist: null, title: "Glue", fileName: "track-01.mp3" })).toBe("Glue");
    expect(libraryDisplayName({ artist: null, title: null, fileName: "track-01.mp3" })).toBe("track-01.mp3");
  });
});
