/**
 * Filtrer la bibliothèque.
 *
 * La correspondance porte sur **tout ce que le programme sait** d'un morceau :
 * l'artiste, le titre, le nom du fichier et son chemin complet. On ne se
 * souvient pas toujours du même : un morceau bien étiqueté se cherche par son
 * titre, un fichier ramassé ailleurs par la bouillie de son nom, et parfois
 * c'est le dossier qui revient — d'où le chemin.
 *
 * L'album manque à cette liste parce qu'il manque au programme : la trame
 * `TALB` n'est pas lue, aucune colonne ne la conserve. L'ajouter est un
 * chantier d'étiquettes, pas de recherche.
 */

import type { LibraryTrack } from "../library/types";

type SearchableTrack = Pick<LibraryTrack, "artist" | "title" | "fileName" | "filePath">;

/**
 * La forme sous laquelle deux textes se comparent.
 *
 * Minuscules, accents retirés, et tout ce qui n'est ni lettre ni chiffre ramené
 * à une espace. Sans ça, `paul_kalkbrenner-gebrannt` ne répondrait ni à
 * « kalkbrenner » collé aux tirets, ni à « gebrännt » tapé avec son tréma.
 */
export function normaliseForSearch(value: string): string {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

/**
 * Vrai si ce morceau répond à la recherche.
 *
 * Chaque mot tapé doit se retrouver quelque part, et pas forcément dans l'ordre
 * ni côte à côte : « kalk 2005 » trouve `paul_kalkbrenner-gebrannt-2005`. Une
 * recherche vide accepte tout, de sorte que l'appelant n'a pas de cas à part.
 */
export function libraryMatchesSearch(track: SearchableTrack, query: string): boolean {
  const terms = normaliseForSearch(query).split(" ").filter(Boolean);
  if (terms.length === 0) return true;

  const haystack = normaliseForSearch(
    [track.artist, track.title, track.fileName, track.filePath].filter(Boolean).join(" "),
  );
  return terms.every((term) => haystack.includes(term));
}

/** La bibliothèque réduite à ce qui répond. */
export function filterLibrary<T extends SearchableTrack>(tracks: readonly T[], query: string): T[] {
  if (normaliseForSearch(query).length === 0) return [...tracks];
  return tracks.filter((track) => libraryMatchesSearch(track, query));
}
