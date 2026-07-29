import type { LibraryTrack } from "../library/types";

export function libraryDisplayName(
  track: Pick<LibraryTrack, "artist" | "title" | "fileName">,
): string {
  const artist = track.artist?.trim();
  const title = track.title?.trim();

  if (artist && title) {
    return `${artist} - ${title}`;
  }

  return title || artist || track.fileName;
}
