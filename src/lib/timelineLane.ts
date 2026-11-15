/** Fixed track set shared by layout, pointer geometry and played effects. */
export const TIMELINE_LANES = [0, 1, 2, 3] as const;
export const TIMELINE_LANE_COUNT = TIMELINE_LANES.length;

/** La lettre d'une piste, telle qu'elle s'affiche partout. */
export function laneLetter(lane: number): string {
  return String.fromCharCode(65 + lane);
}

export function timelineLaneFromPointer(
  clientY: number,
  tracksTop: number,
  tracksHeight: number,
  laneCount: number,
): number {
  if (
    !Number.isFinite(clientY)
    || !Number.isFinite(tracksTop)
    || !Number.isFinite(tracksHeight)
    || tracksHeight <= 0
    || !Number.isInteger(laneCount)
    || laneCount <= 0
  ) {
    return 0;
  }

  const lane = Math.floor((clientY - tracksTop) / (tracksHeight / laneCount));
  return Math.max(0, Math.min(laneCount - 1, lane));
}
