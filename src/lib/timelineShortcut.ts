const TEXT_ENTRY_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"]);

function isTextEntryTarget(targetTagName?: string, isContentEditable = false): boolean {
  return isContentEditable || (targetTagName !== undefined && TEXT_ENTRY_TAGS.has(targetTagName));
}

export function shouldCaptureTimelineSpace(
  code: string,
  targetTagName?: string,
  isContentEditable = false,
): boolean {
  return (
    code === "Space" &&
    !isTextEntryTarget(targetTagName, isContentEditable)
  );
}

/** Which player the spacebar drives, given what is open on top of the timeline. */
export type SpaceTarget = "beatgrid-preview" | "timeline" | "none";

/**
 * A modal owns the transport while it is open.
 *
 * This matters beyond focus: starting the timeline releases the Preview output,
 * so a spacebar that reached the timeline from behind the Beatgrid Editor cut
 * off the very audio the editor was auditioning through.
 */
export function resolveSpaceTarget(open: {
  beatgridEditor: boolean;
  clipEq: boolean;
}): SpaceTarget {
  if (open.beatgridEditor) return "beatgrid-preview";
  if (open.clipEq) return "none";
  return "timeline";
}

/** Keyboard actions that operate on the lane the user last pointed at. */
export type LaneShortcut = "split" | "solo" | "mute" | "volume" | "pan" | null;

/**
 * Resolves a keypress against the lane-scoped shortcuts.
 *
 * Shift is the lane modifier, so `S` and `M` stay free for anything typed and
 * match the mute and solo buttons they stand for. `B`, `V` and `P` need no
 * modifier and tolerate one, since nothing else claims them.
 *
 * Any other modifier hands the key back to the browser or the system: Ctrl+S
 * and Ctrl+M are not ours to take.
 */
export function resolveLaneShortcut(
  key: string,
  modifiers: { shift: boolean; ctrl: boolean; alt: boolean; meta: boolean },
  targetTagName?: string,
  isContentEditable = false,
): LaneShortcut {
  if (isTextEntryTarget(targetTagName, isContentEditable)) return null;
  if (modifiers.ctrl || modifiers.alt || modifiers.meta) return null;

  const letter = key.toLowerCase();
  if (letter === "b") return "split";
  if (letter === "v") return "volume";
  if (letter === "p") return "pan";
  if (!modifiers.shift) return null;
  if (letter === "s") return "solo";
  if (letter === "m") return "mute";
  return null;
}

/** Keyboard actions that drive what the timeline shows and how it draws. */
export type ViewShortcut = "view" | "shape" | "period" | null;

/**
 * Resolves a keypress against the shortcuts that mirror the `VIEW` and `DRAW`
 * keycaps.
 *
 * Sans modificateur, et jamais avec : `Shift+S` appartient au solo d'une piste,
 * et le laisser aussi faire tourner les formes ferait deux choses d'une frappe.
 * `E`, `S` et `D` sont voisines sous la main gauche, comme les trois commandes
 * qu'elles reprennent le sont dans leur rail.
 */
export function resolveViewShortcut(
  key: string,
  modifiers: { shift: boolean; ctrl: boolean; alt: boolean; meta: boolean },
  targetTagName?: string,
  isContentEditable = false,
): ViewShortcut {
  if (isTextEntryTarget(targetTagName, isContentEditable)) return null;
  if (modifiers.shift || modifiers.ctrl || modifiers.alt || modifiers.meta) return null;

  switch (key.toLowerCase()) {
    case "e":
      return "view";
    case "s":
      return "shape";
    case "d":
      return "period";
    default:
      return null;
  }
}

/**
 * Si cette frappe demande de supprimer le clip sous la souris.
 *
 * `Delete` et `Backspace` : le premier est le geste attendu sur un clavier
 * complet, le second sur un portable qui n'a que lui. Aucun modificateur —
 * `Ctrl+Delete` et compagnie appartiennent à l'édition de texte, et une
 * suppression déclenchée par un raccourci voisin serait un mauvais réveil.
 *
 * La même garde que les autres raccourcis : jamais pendant une saisie, faute de
 * quoi effacer un caractère dans un champ emporterait un clip.
 */
export function isDeleteShortcut(
  key: string,
  modifiers: { shift: boolean; ctrl: boolean; alt: boolean; meta: boolean },
  targetTagName?: string,
  isContentEditable = false,
): boolean {
  if (isTextEntryTarget(targetTagName, isContentEditable)) return false;
  if (modifiers.shift || modifiers.ctrl || modifiers.alt || modifiers.meta) return false;
  return key === "Delete" || key === "Backspace";
}

export function shouldCaptureTimelineZoom(
  code: string,
  targetTagName?: string,
  isContentEditable = false,
): boolean {
  return (
    (code === "KeyR" || code === "KeyT") &&
    !isTextEntryTarget(targetTagName, isContentEditable)
  );
}
