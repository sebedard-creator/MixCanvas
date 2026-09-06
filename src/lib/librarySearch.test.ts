import { describe, expect, it } from "vitest";

import { filterLibrary, libraryMatchesSearch, normaliseForSearch } from "./librarySearch";

const tagged = {
  artist: "Paul Kalkbrenner",
  title: "Gebrännt",
  fileName: "01-pk-geb-2005.mp3",
  filePath: "D:/Musique/Techno/01-pk-geb-2005.mp3",
};
const bare = {
  artist: null,
  title: null,
  fileName: "02.Stalker (Club Version).mp3",
  filePath: "D:/Musique/EBM/02.Stalker (Club Version).mp3",
};

describe("normaliseForSearch", () => {
  it("retire les accents et la ponctuation", () => {
    expect(normaliseForSearch("Gebrännt")).toBe("gebrannt");
    expect(normaliseForSearch("01-paul_kalkbrenner-2005.mp3")).toBe("01 paul kalkbrenner 2005 mp3");
  });
});

describe("libraryMatchesSearch", () => {
  it("cherche dans le nom affiché", () => {
    expect(libraryMatchesSearch(tagged, "kalkbrenner")).toBe(true);
    expect(libraryMatchesSearch(tagged, "gebrannt")).toBe(true);
  });

  /* Un morceau bien étiqueté se cherche par son titre, un fichier ramassé
     ailleurs par la bouillie de son nom. Les deux doivent répondre. */
  it("cherche aussi dans le nom de fichier", () => {
    expect(libraryMatchesSearch(tagged, "2005")).toBe(true);
    expect(libraryMatchesSearch(bare, "club")).toBe(true);
  });

  it("se moque des accents et de la casse", () => {
    expect(libraryMatchesSearch(tagged, "GEBRÄNNT")).toBe(true);
    expect(libraryMatchesSearch(tagged, "gebrannt")).toBe(true);
  });

  /* Le point de la recherche par mots : on tape ce dont on se souvient, dans
     l'ordre où ça vient. */
  it("accepte des mots séparés, dans n'importe quel ordre", () => {
    expect(libraryMatchesSearch(tagged, "kalk 2005")).toBe(true);
    expect(libraryMatchesSearch(tagged, "2005 paul")).toBe(true);
    expect(libraryMatchesSearch(tagged, "kalk absent")).toBe(false);
  });

  it("accepte tout quand rien n'est tapé", () => {
    expect(libraryMatchesSearch(bare, "")).toBe(true);
    expect(libraryMatchesSearch(bare, "   ")).toBe(true);
  });
});

describe("filterLibrary", () => {
  it("rend la liste entière sur une recherche vide", () => {
    expect(filterLibrary([tagged, bare], "")).toHaveLength(2);
  });

  it("ne garde que ce qui répond, dans l'ordre reçu", () => {
    expect(filterLibrary([tagged, bare], "stalker")).toEqual([bare]);
    expect(filterLibrary([tagged, bare], "zzz")).toEqual([]);
  });
});

describe("l'étendue de la recherche", () => {
  /* Tout ce que le programme sait d'un morceau, y compris le dossier où il
     vit : c'est parfois le seul souvenir qui reste. */
  it("trouve par le dossier du fichier", () => {
    expect(libraryMatchesSearch(tagged, "techno")).toBe(true);
    expect(libraryMatchesSearch(bare, "ebm")).toBe(true);
    expect(libraryMatchesSearch(tagged, "ebm")).toBe(false);
  });

  it("trouve par l'artiste seul, sans le titre", () => {
    expect(libraryMatchesSearch(tagged, "paul")).toBe(true);
  });
});
