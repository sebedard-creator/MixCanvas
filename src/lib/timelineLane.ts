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
