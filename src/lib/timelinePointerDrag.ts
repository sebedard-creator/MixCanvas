import { timelineLaneFromPointer } from "./timelineLane";
import { snapTimelineBeat } from "./timelineSnap";

export const POINTER_DRAG_THRESHOLD_PX = 6;

export interface LibraryPointerDrag {
  trackId: number;
  clientX: number;
  clientY: number;
  phase: "dragging" | "dropped";
}

export interface TimelineDropGeometry {
  contentLeft: number;
  viewportLeft: number;
  viewportRight: number;
  top: number;
  height: number;
}

export interface TimelinePointerDrop {
  anchorBeat: number;
  lane: number;
}

export function pointerMovedEnoughToDrag(
  startX: number,
  startY: number,
  clientX: number,
  clientY: number,
): boolean {
  return Math.hypot(clientX - startX, clientY - startY) >= POINTER_DRAG_THRESHOLD_PX;
}

export function resolveTimelinePointerDrop(
  clientX: number,
  clientY: number,
  geometry: TimelineDropGeometry,
  pixelsPerBeat: number,
  laneCount = 3,
): TimelinePointerDrop | null {
  const bottom = geometry.top + geometry.height;
  const isInside = clientX >= geometry.viewportLeft
    && clientX <= geometry.viewportRight
    && clientY >= geometry.top
    && clientY <= bottom;
  if (!isInside || pixelsPerBeat <= 0 || laneCount <= 0) {
    return null;
  }

  return {
    anchorBeat: snapTimelineBeat((clientX - geometry.contentLeft) / pixelsPerBeat),
    lane: timelineLaneFromPointer(
      clientY,
      geometry.top,
      geometry.height,
      laneCount,
    ),
  };
}
