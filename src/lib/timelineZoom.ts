export const DEFAULT_MIN_TIMELINE_ZOOM = 4;
export const MAX_TIMELINE_ZOOM = 96;

/**
 * Share of the window the project occupies once zoomed all the way out.
 *
 * Deliberately short of a perfect fit: the last notch of zoom out leaves a
 * visible gap at each end, so reaching the limit reads as reaching the limit
 * rather than as the control having jammed. Seeing both ends of the mix at
 * once is also the point of being that far out.
 */
export const TIMELINE_FIT_RATIO = 0.88;

export function minimumTimelineZoom(viewportWidth: number, totalBeats: number): number {
  if (!Number.isFinite(viewportWidth) || viewportWidth <= 0 || !Number.isFinite(totalBeats) || totalBeats <= 0) {
    return DEFAULT_MIN_TIMELINE_ZOOM;
  }

  return Math.max(
    Number.EPSILON,
    Math.min(DEFAULT_MIN_TIMELINE_ZOOM, (viewportWidth * TIMELINE_FIT_RATIO) / totalBeats),
  );
}

export function clampTimelineZoom(requestedZoom: number, minimumZoom: number): number {
  return Math.min(MAX_TIMELINE_ZOOM, Math.max(minimumZoom, requestedZoom));
}

/**
 * Where the timeline content sits inside its viewport.
 *
 * While the content is wider than the window it is pinned to the playhead,
 * which stays on the centre line with a half-window of virtual space on each
 * side. Once the whole project fits, that shift would push half of it off
 * screen for no reason: the project is centred instead, its two ends framed by
 * equal gaps, which is the point of zooming out that far.
 */
export function timelineContentLayout(
  displayBeat: number,
  pixelsPerBeat: number,
  contentWidth: number,
  viewportWidth: number,
): { paddingPx: number; offsetPx: number } {
  const usable =
    Number.isFinite(displayBeat)
    && Number.isFinite(pixelsPerBeat)
    && Number.isFinite(contentWidth)
    && Number.isFinite(viewportWidth)
    && viewportWidth > 0;
  if (!usable) {
    return { paddingPx: 0, offsetPx: 0 };
  }
  if (contentWidth <= viewportWidth) {
    return { paddingPx: 0, offsetPx: (viewportWidth - contentWidth) / 2 };
  }

  return {
    paddingPx: viewportWidth / 2,
    offsetPx: -displayBeat * pixelsPerBeat,
  };
}

export function scrollLeftCenteringBeat(
  beat: number,
  viewportWidth: number,
  nextZoom: number,
  totalBeats: number,
): number {
  if (
    !Number.isFinite(beat)
    || !Number.isFinite(viewportWidth)
    || !Number.isFinite(nextZoom)
    || !Number.isFinite(totalBeats)
    || viewportWidth <= 0
    || nextZoom <= 0
    || totalBeats <= 0
  ) {
    return 0;
  }

  const maximumScroll = Math.max(0, totalBeats * nextZoom - viewportWidth);
  const centeredScroll = beat * nextZoom - viewportWidth / 2;
  return Math.max(0, Math.min(maximumScroll, centeredScroll));
}

/**
 * The playing timeline receives one half-viewport of virtual space on each
 * side. Scrolling to the beat's unshifted coordinate then keeps it exactly
 * under the fixed centre line, including at the first beat.
 */
export function scrollLeftFollowingBeat(
  beat: number,
  pixelsPerBeat: number,
  totalBeats: number,
): number {
  if (
    !Number.isFinite(beat)
    || !Number.isFinite(pixelsPerBeat)
    || !Number.isFinite(totalBeats)
    || pixelsPerBeat <= 0
    || totalBeats <= 0
  ) {
    return 0;
  }

  return Math.max(0, Math.min(totalBeats * pixelsPerBeat, beat * pixelsPerBeat));
}

/**
 * Le zoom en deux temps : un étirement pendant le geste, un vrai rendu après.
 *
 * Chaque cran de molette changeait toutes les coordonnées du monde d'un coup —
 * un marqueur par mesure du projet entier, chaque clip, chaque trame, la règle
 * en pleine largeur. La cadence s'effondrait, la molette s'accumulait pendant
 * les images manquées, et chaque image peinte sautait donc un grand pas de
 * zoom, avec des niveaux d'onde et des étiquettes qui claquaient au passage :
 * le stroboscope reproché à l'outil, en debug comme en release, puisque le
 * coût était la mise en page et non le JavaScript.
 *
 * Pendant le geste, le conteneur est donc simplement **étiré** — une seule
 * transformation, composée par le GPU, sans mise en page ni repeinture. Tout
 * bouge d'un seul bloc parce que tout est un seul bloc : la grille, les ondes,
 * les enveloppes et la tête ne peuvent plus se désynchroniser, par
 * construction. Le vrai rendu, net, se fait une fois le geste posé — ou dès
 * que l'étirement dépasse ce qu'on accepte de montrer.
 */

/**
 * L'écart au-delà duquel deux crans ne forment plus un même geste.
 *
 * Un cran **isolé** se rend immédiatement, sans étirement : une seule image
 * change, dans le sens commandé, et la paire étirer-puis-poser — avec ce
 * qu'elle traîne d'artefacts de composition — n'existe même pas. L'étirement
 * ne sert qu'aux rafales, là où rendre à chaque cran faisait strober.
 */
export const ZOOM_BURST_WINDOW_MS = 160;

/** Un cran qui suit un autre d'assez près pour être le même geste. */
export function isZoomGestureBurst(
  nowMs: number,
  lastTickMs: number,
  hasPendingPreview: boolean,
): boolean {
  return hasPendingPreview || nowMs - lastTickMs < ZOOM_BURST_WINDOW_MS;
}

/** L'étirement au-delà duquel on rend pour de vrai plutôt que d'étirer. */
export const ZOOM_PREVIEW_MIN_SCALE = 0.5;
export const ZOOM_PREVIEW_MAX_SCALE = 2;

/**
 * Le silence après le dernier cran, avant le rendu net.
 *
 * Assez court pour que la netteté revienne dès que la main s'arrête; assez
 * long pour que les crans d'un même geste — 30 à 60 ms d'écart à la molette —
 * n'intercalent pas un rendu complet entre chacun d'eux.
 */
export const ZOOM_SETTLE_MS = 90;

export function zoomPreviewScale(committedZoom: number, pendingZoom: number): number {
  if (!Number.isFinite(committedZoom) || !Number.isFinite(pendingZoom) || committedZoom <= 0) {
    return 1;
  }
  return pendingZoom / committedZoom;
}

export function zoomPreviewNeedsCommit(scale: number): boolean {
  return scale < ZOOM_PREVIEW_MIN_SCALE || scale > ZOOM_PREVIEW_MAX_SCALE;
}

/**
 * Le point du contenu qui ne bouge pas pendant l'étirement, en pixels.
 *
 * C'est le même point que le rendu net garde immobile : le temps affiché au
 * centre quand le contenu déborde, le milieu du projet quand il tient en
 * entier. Étirer autour d'un autre point ferait glisser l'image pendant le
 * geste puis sauter au rendu — précisément ce qu'on soigne.
 */
export function timelineZoomAnchorPx(
  displayBeat: number,
  pixelsPerBeat: number,
  contentWidth: number,
  viewportWidth: number,
): number {
  const usable =
    Number.isFinite(displayBeat)
    && Number.isFinite(pixelsPerBeat)
    && Number.isFinite(contentWidth)
    && Number.isFinite(viewportWidth)
    && viewportWidth > 0;
  if (!usable) return 0;
  if (contentWidth <= viewportWidth) {
    return contentWidth / 2;
  }
  return displayBeat * pixelsPerBeat;
}

/**
 * Les mesures dont le marqueur vaut la peine d'exister : celles qu'on voit.
 *
 * Il y en avait une par mesure du projet **entier** — des milliers sur un long
 * mix — toutes mises en page à chaque rendu alors qu'une poignée tombe dans la
 * fenêtre. La marge d'un cran de chaque côté couvre le défilement entre deux
 * rendus; l'alignement sur `labelStride` fait qu'une fenêtre qui glisse révèle
 * toujours les mêmes marqueurs, au lieu d'en inventer d'autres à ses bords.
 */
export function visibleMeasures(
  displayBeat: number,
  pixelsPerBeat: number,
  viewportWidth: number,
  totalBeats: number,
  labelStride: number,
  contentWidth: number,
): number[] {
  const totalMeasures = Math.ceil(totalBeats / 4);
  const stride = Math.max(1, Math.floor(labelStride));
  const everyMeasure = () => {
    const all: number[] = [];
    for (let measure = 0; measure <= totalMeasures; measure += stride) {
      all.push(measure);
    }
    return all;
  };
  const usable =
    Number.isFinite(displayBeat)
    && Number.isFinite(pixelsPerBeat)
    && Number.isFinite(viewportWidth)
    && pixelsPerBeat > 0
    && viewportWidth > 0;
  if (!usable || contentWidth <= viewportWidth) {
    return everyMeasure();
  }

  const beatsAcross = viewportWidth / pixelsPerBeat;
  const firstBeat = displayBeat - beatsAcross / 2;
  const lastBeat = displayBeat + beatsAcross / 2;
  const from = Math.max(0, (Math.floor(firstBeat / (4 * stride)) - 1) * stride);
  const to = Math.min(totalMeasures, (Math.ceil(lastBeat / (4 * stride)) + 1) * stride);
  const measures: number[] = [];
  for (let measure = from; measure <= to; measure += stride) {
    measures.push(measure);
  }
  return measures;
}
