/**
 * Trimming a clip's head or tail by dragging its edge.
 *
 * The anchor never moves. Trimming changes how much of the source is heard, not
 * where the clip sits, so the audio under the untrimmed part stays exactly where
 * it was on the timeline — which is the whole reason to reach for the tool
 * rather than move the clip.
 *
 * A trim is stored as how many beats are hidden at each end. Dragging an edge
 * back out is the same gesture with a smaller number, down to zero where the
 * whole source is heard again.
 */

/** How close to an edge the pointer has to be, in pixels, to grab it. */
export const TRIM_GRAB_PX = 7;
/** A clip has to keep at least this much of itself, or it would vanish. */
export const MIN_CLIP_BEATS = 0.5;

export type TrimEdge = "start" | "end";

export interface TrimmableClip {
  visualStartBeat: number;
  visualEndBeat: number;
  trimStartBeats: number;
  trimEndBeats: number;
}

export interface ClipTrim {
  trimStartBeats: number;
  trimEndBeats: number;
}

/** Where the clip would begin and end with nothing trimmed away. */
export function untrimmedBounds(clip: TrimmableClip): { startBeat: number; endBeat: number } {
  return {
    startBeat: clip.visualStartBeat - clip.trimStartBeats,
    endBeat: clip.visualEndBeat + clip.trimEndBeats,
  };
}

/**
 * A clip as it looks mid-drag, before the edit reaches the backend.
 *
 * Everything downstream has to read the same geometry — the box the clip is
 * drawn in, and the window of the waveform shown inside it. Feeding the drag's
 * width to one and the committed trim to the other squeezes a fixed slice of
 * audio into a shrinking box, which looks exactly like a time-stretch.
 */
export function clipWithTrim<T extends TrimmableClip>(clip: T, trim: ClipTrim | undefined): T {
  if (!trim) return clip;
  const bounds = untrimmedBounds(clip);
  return {
    ...clip,
    trimStartBeats: trim.trimStartBeats,
    trimEndBeats: trim.trimEndBeats,
    visualStartBeat: bounds.startBeat + trim.trimStartBeats,
    visualEndBeat: bounds.endBeat - trim.trimEndBeats,
  };
}

/**
 * Which edge the pointer is over, if either.
 *
 * The grab zone is measured in pixels rather than beats so it stays the same
 * size under the hand at every zoom level. It is also clamped to a third of the
 * clip, so the two edges of a very short clip cannot both claim the middle.
 */
export function trimEdgeAtPointer(
  clip: TrimmableClip,
  pointerBeat: number,
  pixelsPerBeat: number,
): TrimEdge | null {
  if (!(pixelsPerBeat > 0)) return null;
  const width = clip.visualEndBeat - clip.visualStartBeat;
  if (width <= 0) return null;

  const reach = Math.min(TRIM_GRAB_PX / pixelsPerBeat, width / 3);
  if (pointerBeat >= clip.visualStartBeat - reach && pointerBeat <= clip.visualStartBeat + reach) {
    return "start";
  }
  if (pointerBeat >= clip.visualEndBeat - reach && pointerBeat <= clip.visualEndBeat + reach) {
    return "end";
  }
  return null;
}

/** Snaps a beat onto the quarter-beat grid, as the rest of the timeline does. */
export function snapTrimBeat(beat: number): number {
  return Math.round(beat * 4) / 4;
}

/**
 * The trim that results from dragging `edge` to `pointerBeat`.
 *
 * `limitStartBeat` and `limitEndBeat` are how far the clip may grow before it
 * would run into a neighbour on the same lane; extending is as constrained as
 * moving, since the space either exists or it does not.
 */
export function trimForEdge(
  clip: TrimmableClip,
  edge: TrimEdge,
  pointerBeat: number,
  limits: { limitStartBeat: number; limitEndBeat: number },
): ClipTrim {
  const bounds = untrimmedBounds(clip);
  const snapped = snapTrimBeat(pointerBeat);

  if (edge === "start") {
    // Never past the source's own beginning, never into the clip before it,
    // and never so far right that nothing of the clip is left.
    const earliest = Math.max(bounds.startBeat, limits.limitStartBeat);
    const latest = clip.visualEndBeat - MIN_CLIP_BEATS;
    const startBeat = Math.min(Math.max(snapped, earliest), latest);
    return {
      trimStartBeats: Math.max(0, startBeat - bounds.startBeat),
      trimEndBeats: clip.trimEndBeats,
    };
  }

  const latest = Math.min(bounds.endBeat, limits.limitEndBeat);
  const earliest = clip.visualStartBeat + MIN_CLIP_BEATS;
  const endBeat = Math.max(Math.min(snapped, latest), earliest);
  return {
    trimStartBeats: clip.trimStartBeats,
    trimEndBeats: Math.max(0, bounds.endBeat - endBeat),
  };
}

/**
 * How far a clip may grow in each direction before meeting a neighbour.
 *
 * Only clips on the same lane can be in the way, and only the nearest one on
 * each side matters.
 */
export function clipTrimLimits(
  clips: readonly { id: number; lane: number; visualStartBeat: number; visualEndBeat: number }[],
  clip: { id: number; lane: number; visualStartBeat: number; visualEndBeat: number },
): { limitStartBeat: number; limitEndBeat: number } {
  let limitStartBeat = 0;
  let limitEndBeat = Number.POSITIVE_INFINITY;

  for (const other of clips) {
    if (other.id === clip.id || other.lane !== clip.lane) continue;
    if (other.visualEndBeat <= clip.visualStartBeat) {
      limitStartBeat = Math.max(limitStartBeat, other.visualEndBeat);
    } else if (other.visualStartBeat >= clip.visualEndBeat) {
      limitEndBeat = Math.min(limitEndBeat, other.visualStartBeat);
    }
  }

  return { limitStartBeat, limitEndBeat };
}

/**
 * Le temps le plus à gauche où l'ancre d'un clip peut se poser.
 *
 * Miroir de `minimum_anchor_beat` dans `src-tauri/src/timeline.rs`, et il doit
 * le rester : le serveur refuserait un déplacement que l'interface aurait
 * autorisé, et le clip reviendrait en arrière sous le curseur.
 *
 * L'ancre porte le **premier temps**, pas le début du clip. Ce qu'on protège,
 * c'est que la partie *visible* ne commence pas avant le temps zéro — donc
 * `ancre − pré-roll + rognage ≥ 0`. Le rognage manquait à ce calcul : un clip
 * dont on avait coupé la tête restait retenu par une part qu'il ne fait plus
 * entendre.
 */
export function minimumAnchorBeat(preRollBeats: number, trimStartBeats: number): number {
  const usable = Number.isFinite(preRollBeats) && Number.isFinite(trimStartBeats);
  if (!usable) return 0;
  return Math.max(0, Math.ceil(preRollBeats - trimStartBeats));
}

/**
 * Une boucle, vue de ses deux poignées.
 *
 * `loopLeadBeats` et `loopTailBeats` disent de combien la boucle déborde du
 * motif, avant et après. Le motif lui-même reste décrit par le rognage : c'est
 * ce qui fait qu'éteindre la boucle rend le clip exactement tel qu'il était.
 */
export interface ClipLoop {
  loopLeadBeats: number;
  loopTailBeats: number;
}

export interface LoopableClip extends TrimmableClip {
  looping: boolean;
  loopLeadBeats: number;
  loopTailBeats: number;
}

/** Où commence et finit le motif, à l'intérieur d'un clip qui boucle. */
export function loopBody(clip: LoopableClip): { startBeat: number; endBeat: number } {
  return {
    startBeat: clip.visualStartBeat + clip.loopLeadBeats,
    endBeat: clip.visualEndBeat - clip.loopTailBeats,
  };
}

/**
 * Le clip tel qu'il paraît pendant qu'on tire une poignée de boucle.
 *
 * Même rôle que `clipWithTrim`, et pour la même raison : la boîte dessinée et
 * la waveform qu'elle contient doivent lire une seule géométrie, sinon on voit
 * une tranche fixe se comprimer dans une boîte qui bouge.
 */
export function clipWithLoop<T extends LoopableClip>(clip: T, loop: ClipLoop | undefined): T {
  if (!loop) return clip;
  const body = loopBody(clip);
  return {
    ...clip,
    loopLeadBeats: loop.loopLeadBeats,
    loopTailBeats: loop.loopTailBeats,
    visualStartBeat: body.startBeat - loop.loopLeadBeats,
    visualEndBeat: body.endBeat + loop.loopTailBeats,
  };
}

/**
 * Le débordement que produit le fait de tirer `edge` jusqu'à `pointerBeat`.
 *
 * Une poignée de boucle ne peut que rallonger : ramenée à l'intérieur du motif,
 * elle s'arrête sur son bord. Pour raccourcir le motif lui-même, il faut
 * éteindre la boucle et rogner — deux gestes distincts pour deux idées
 * distinctes, plutôt qu'une poignée dont le sens change à mi-course.
 */
export function loopForEdge(
  clip: LoopableClip,
  edge: TrimEdge,
  pointerBeat: number,
  limits: { limitStartBeat: number; limitEndBeat: number },
): ClipLoop {
  const body = loopBody(clip);
  const snapped = snapTrimBeat(pointerBeat);

  if (edge === "start") {
    // Jamais dans le voisin de gauche, jamais avant le premier temps du
    // projet, et jamais au-delà du début du motif.
    const earliest = Math.max(0, limits.limitStartBeat);
    const startBeat = Math.min(Math.max(snapped, earliest), body.startBeat);
    return {
      loopLeadBeats: Math.max(0, body.startBeat - startBeat),
      loopTailBeats: clip.loopTailBeats,
    };
  }

  const latest = limits.limitEndBeat;
  const endBeat = Math.max(Math.min(snapped, latest), body.endBeat);
  return {
    loopLeadBeats: clip.loopLeadBeats,
    loopTailBeats: Math.max(0, endBeat - body.endBeat),
  };
}

/** Un tour de boucle, tel qu'il se dessine dans la boîte du clip. */
export interface LoopTurn {
  key: string;
  offsetPx: number;
  widthPx: number;
  trimStartBeats: number;
  trimEndBeats: number;
  durationBeats: number;
}

/**
 * Les tours à dessiner, du premier au dernier.
 *
 * Miroir de `loop_tiles` dans `src-tauri/src/timeline.rs`, et il doit le
 * rester : ce qu'on voit et ce qu'on entend sont deux lectures du même
 * découpage, et deux carrelages qui divergent donnent une waveform qui ment.
 *
 * Un clip sans boucle rend un seul tour — le clip lui-même — pour que l'appelant
 * n'ait pas deux chemins à écrire.
 */
export function loopTurns(clip: LoopableClip, pixelsPerBeat: number): LoopTurn[] {
  const width = clip.visualEndBeat - clip.visualStartBeat;
  const plain: LoopTurn = {
    key: "body",
    offsetPx: 0,
    widthPx: Math.max(1, width * pixelsPerBeat),
    trimStartBeats: clip.trimStartBeats,
    trimEndBeats: clip.trimEndBeats,
    durationBeats: width,
  };

  const body = loopBody(clip);
  const bodyBeats = body.endBeat - body.startBeat;
  if (!clip.looping || !(bodyBeats > 0) || !Number.isFinite(bodyBeats)) return [plain];

  const first = -Math.ceil(clip.loopLeadBeats / bodyBeats);
  const last = Math.ceil(clip.loopTailBeats / bodyBeats);

  const turns: LoopTurn[] = [];
  for (let turn = first; turn <= last; turn += 1) {
    const startBeat = body.startBeat + turn * bodyBeats;
    const headCut = Math.max(0, clip.visualStartBeat - startBeat);
    const tailCut = Math.max(0, startBeat + bodyBeats - clip.visualEndBeat);
    const duration = bodyBeats - headCut - tailCut;
    // Un reste plus court qu'un millième de temps ne se voit pas et coûterait
    // un canvas.
    if (duration <= 1e-3) continue;
    turns.push({
      key: `turn-${turn}`,
      offsetPx: (startBeat + headCut - clip.visualStartBeat) * pixelsPerBeat,
      widthPx: Math.max(1, duration * pixelsPerBeat),
      trimStartBeats: clip.trimStartBeats + headCut,
      trimEndBeats: clip.trimEndBeats + tailCut,
      durationBeats: duration,
    });
  }
  if (turns.length === 0) return [plain];

  /* Les tours sont ramenés sur la grille de pixels.
     Chaque bord est arrondi, et la largeur se déduit du bord suivant plutôt
     que d'être arrondie pour elle-même : deux arrondis indépendants laissent
     un pixel de vide ou de recouvrement à la couture. Un bord fractionnaire
     ferait aussi ré-échantillonner l'image du tour à chaque trame pendant la
     lecture, chacun à sa propre fraction — la waveform se mettait à danser. */
  const right = clip.visualEndBeat - clip.visualStartBeat;
  return turns.map((entry, index) => {
    const from = Math.round(entry.offsetPx);
    const next = index + 1 < turns.length
      ? Math.round(turns[index + 1].offsetPx)
      : Math.round(right * pixelsPerBeat);
    return { ...entry, offsetPx: from, widthPx: Math.max(1, next - from) };
  });
}
