/**
 * Quels nœuds une courbe d'automation doit traverser pour couvrir la fenêtre.
 *
 * Une courbe était construite d'un bout à l'autre du mix — sur une timeline
 * large de soixante-dix mille pixels, des milliers de segments dont on en voit
 * un ou deux pour cent. Le même gaspillage que celui déjà corrigé pour les
 * waveforms, et il coûte trois fois : à la construction de la chaîne, à la
 * mise en page du chemin, et à sa peinture.
 *
 * Le découpage garde **un nœud de chaque côté** de la fenêtre. C'est ce qui
 * rend le segment qui entre dans le champ exact : couper au bord donnerait une
 * pente calculée depuis le bord, et la ligne ne passerait plus là où elle
 * passe vraiment.
 */

/**
 * La tranche de `sorted` qui couvre `[fromBeat, toBeat]`, débordée d'un nœud.
 *
 * `sorted` doit être trié par beat croissant. Un tableau vide ressort vide;
 * une fenêtre entièrement avant ou après les nœuds ressort avec le seul nœud
 * voisin, qui suffit à tracer la ligne plate qu'on voit alors.
 */
export function nodesAcross<T extends { beat: number }>(
  sorted: readonly T[],
  fromBeat: number,
  toBeat: number,
): readonly T[] {
  if (sorted.length === 0) return sorted;
  if (!Number.isFinite(fromBeat) || !Number.isFinite(toBeat) || toBeat < fromBeat) {
    return sorted;
  }

  // Le dernier nœud à gauche de la fenêtre, ou le premier de tous.
  let start = 0;
  while (start + 1 < sorted.length && sorted[start + 1].beat <= fromBeat) {
    start += 1;
  }

  // Le premier nœud à droite de la fenêtre, ou le dernier de tous.
  let end = sorted.length - 1;
  while (end > start && sorted[end - 1].beat >= toBeat) {
    end -= 1;
  }

  return start === 0 && end === sorted.length - 1 ? sorted : sorted.slice(start, end + 1);
}

/**
 * Les beats visibles, avec la marge que le découpage grossier de la vue exige.
 *
 * `viewBeat` est le centre de la fenêtre. La marge est donnée en pixels parce
 * que c'est ainsi qu'on la connaît : c'est de combien la vue peut glisser avant
 * qu'un rendu la remette à jour.
 */
export function visibleBeatRange(
  viewBeat: number,
  pixelsPerBeat: number,
  viewportWidth: number,
  marginPx: number,
): { fromBeat: number; toBeat: number } {
  if (!(pixelsPerBeat > 0) || !(viewportWidth > 0) || !Number.isFinite(viewBeat)) {
    return { fromBeat: Number.NEGATIVE_INFINITY, toBeat: Number.POSITIVE_INFINITY };
  }
  const halfBeats = (viewportWidth / 2 + Math.max(0, marginPx)) / pixelsPerBeat;
  return { fromBeat: viewBeat - halfBeats, toBeat: viewBeat + halfBeats };
}
