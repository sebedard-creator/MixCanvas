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
 * Converts a pointer coordinate inside the ruler or a lane into a timeline
 * beat. The click handler lives on a child of `.timeline-content` (the ruler
 * or a lane), whose client rectangle already includes both the virtual side
 * space and the current native scroll offset. Its local x coordinate is
 * therefore the musical x coordinate exactly; subtracting virtual padding a
 * second time wrongly sent most clicks back to beat zero.
 */
export function timelineSeekBeat(
  clientX: number,
  targetLeft: number,
  pixelsPerBeat: number,
): number {
  if (!Number.isFinite(clientX) || !Number.isFinite(targetLeft) || !Number.isFinite(pixelsPerBeat) || pixelsPerBeat <= 0) {
    return 0;
  }
  return (clientX - targetLeft) / pixelsPerBeat;
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
