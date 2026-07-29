import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

import { formatDuration } from "../lib/formatDuration";
import { libraryDisplayName } from "../lib/libraryDisplayName";
import {
  sortLibraryTracks,
  type LibrarySort,
  type LibrarySortKey,
} from "../lib/librarySort";
import { pointerMovedEnoughToDrag } from "../lib/timelinePointerDrag";
import type { LibraryTrack } from "../library/types";
import { MiniPreview } from "./MiniPreview";

interface PointerDragCandidate {
  pointerId: number;
  trackId: number;
  startX: number;
  startY: number;
  dragging: boolean;
}

interface LibraryContextMenu {
  track: LibraryTrack;
  x: number;
  y: number;
}

const DEFAULT_SORT: LibrarySort = { key: "artist", direction: "ascending" };

const SORT_OPTIONS: Array<{ key: LibrarySortKey; label: string }> = [
  { key: "artist", label: "Artist" },
  { key: "title", label: "Track" },
  { key: "bpm", label: "BPM" },
  { key: "inUse", label: "In Use" },
];

interface LibraryPanelProps {
  tracks: LibraryTrack[];
  libraryBusy: boolean;
  analysisBusy: boolean;
  timelineAddBusy: boolean;
  timelineTrackIds: ReadonlySet<number>;
  previewDisabled: boolean;
  previewingTrackId: number | null;
  activePreviewPath: string | null;
  isPreviewPlaying: boolean;
  previewFileName: string | null;
  previewDurationMs: number;
  previewPositionMs: number;
  message: string | null;
  onAddFiles: () => void;
  onAddFolder: () => void;
  onEditGrid: (track: LibraryTrack) => void;
  onAddToTimeline: (track: LibraryTrack) => void;
  onTimelineDragMove: (trackId: number, clientX: number, clientY: number) => void;
  onTimelineDrop: (trackId: number, clientX: number, clientY: number) => void;
  onTimelineDragCancel: () => void;
  onPreview: (track: LibraryTrack) => void;
  onTogglePreview: () => void;
  onSeekPreview: (positionMs: number) => void;
  onRemove: (track: LibraryTrack) => void;
}

export function LibraryPanel({
  tracks,
  libraryBusy,
  analysisBusy,
  timelineAddBusy,
  timelineTrackIds,
  previewDisabled,
  previewingTrackId,
  activePreviewPath,
  isPreviewPlaying,
  previewFileName,
  previewDurationMs,
  previewPositionMs,
  message,
  onAddFiles,
  onAddFolder,
  onEditGrid,
  onAddToTimeline,
  onTimelineDragMove,
  onTimelineDrop,
  onTimelineDragCancel,
  onPreview,
  onTogglePreview,
  onSeekPreview,
  onRemove,
}: LibraryPanelProps) {
  const pointerDrag = useRef<PointerDragCandidate | null>(null);
  const suppressClickUntil = useRef(0);
  const contextMenuRef = useRef<HTMLDivElement | null>(null);
  const [sort, setSort] = useState<LibrarySort>(DEFAULT_SORT);
  const [contextMenu, setContextMenu] = useState<LibraryContextMenu | null>(null);
  const sortedTracks = useMemo(
    () => sortLibraryTracks(tracks, timelineTrackIds, sort),
    [sort, timelineTrackIds, tracks],
  );

  const selectSort = (key: LibrarySortKey) => {
    setSort((current) => {
      if (current.key === key) {
        return {
          key,
          direction: current.direction === "ascending" ? "descending" : "ascending",
        };
      }
      return { key, direction: key === "inUse" ? "descending" : "ascending" };
    });
  };

  useEffect(() => {
    const closeContextMenu = (event: MouseEvent) => {
      if (!contextMenuRef.current?.contains(event.target as Node)) {
        setContextMenu(null);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setContextMenu(null);
      }
    };
    window.addEventListener("click", closeContextMenu);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("click", closeContextMenu);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, []);

  const startPointerDrag = (
    event: ReactPointerEvent<HTMLButtonElement>,
    trackId: number,
  ) => {
    if (event.button !== 0) {
      return;
    }
    pointerDrag.current = {
      pointerId: event.pointerId,
      trackId,
      startX: event.clientX,
      startY: event.clientY,
      dragging: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const movePointerDrag = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const candidate = pointerDrag.current;
    if (!candidate || candidate.pointerId !== event.pointerId) {
      return;
    }
    if (!candidate.dragging) {
      candidate.dragging = pointerMovedEnoughToDrag(
        candidate.startX,
        candidate.startY,
        event.clientX,
        event.clientY,
      );
    }
    if (candidate.dragging) {
      event.preventDefault();
      onTimelineDragMove(candidate.trackId, event.clientX, event.clientY);
    }
  };

  const finishPointerDrag = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const candidate = pointerDrag.current;
    if (!candidate || candidate.pointerId !== event.pointerId) {
      return;
    }
    pointerDrag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (candidate.dragging) {
      suppressClickUntil.current = performance.now() + 300;
      event.preventDefault();
      onTimelineDrop(candidate.trackId, event.clientX, event.clientY);
    }
  };

  const cancelPointerDrag = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const candidate = pointerDrag.current;
    if (!candidate || candidate.pointerId !== event.pointerId) {
      return;
    }
    pointerDrag.current = null;
    if (candidate.dragging) {
      onTimelineDragCancel();
    }
  };

  return (
    <section className="library-panel" aria-labelledby="library-title">
      <div className="library-header">
        <div className="library-title-row">
          <h2 id="library-title">LIBRARY</h2>
          <span className="track-count">{tracks.length}</span>
          <div className="library-actions">
            <button className="secondary-button" type="button" onClick={onAddFolder} disabled={libraryBusy || analysisBusy}>
              Add Folder
            </button>
            <button className="primary-button" type="button" onClick={onAddFiles} disabled={libraryBusy || analysisBusy}>
              {libraryBusy ? "Importing…" : "+ MP3"}
            </button>
          </div>
        </div>
        <div className="library-header-controls">
          <div className="library-sort" role="group" aria-label="Sort library">
            <span>Sort</span>
            {SORT_OPTIONS.map((option) => {
              const isSelected = sort.key === option.key;
              return (
                <button
                  className={isSelected ? "is-selected" : undefined}
                  type="button"
                  key={option.key}
                  aria-pressed={isSelected}
                  onClick={() => selectSort(option.key)}
                  title={`Sort by ${option.label}${isSelected ? `, ${sort.direction}` : ""}`}
                >
                  {option.label}
                  {isSelected && <i aria-hidden="true">{sort.direction === "ascending" ? "↑" : "↓"}</i>}
                </button>
              );
            })}
          </div>
        </div>
      </div>

      <MiniPreview
        fileName={previewFileName}
        durationMs={previewDurationMs}
        positionMs={previewPositionMs}
        isPlaying={isPreviewPlaying}
        disabled={previewDisabled}
        onToggle={onTogglePreview}
        onSeek={onSeekPreview}
      />

      {message && <p className="library-message">{message}</p>}

      {tracks.length === 0 ? (
        <div className="library-empty">
          <span className="library-empty-icon" aria-hidden="true">♫</span>
          <div>
            <strong>Your library is empty</strong>
            <p>Add MP3 files or select a music folder.</p>
          </div>
        </div>
      ) : (
        <div className="library-table" role="table" aria-label="MP3 Library">
          <div className="library-table-header" role="row">
            <span role="columnheader">Track</span>
            <span role="columnheader">BPM</span>
          </div>

          {sortedTracks.map((track) => {
            const displayName = libraryDisplayName(track);
            const isInTimeline = timelineTrackIds.has(track.id);
            const isActive = activePreviewPath === track.filePath;
            const isLoading = previewingTrackId === track.id;
            const previewLabel = isActive && isPreviewPlaying ? "Ⅱ" : "▶";
            const previewTitle = isLoading
              ? "Loading Preview…"
              : isActive && isPreviewPlaying
                ? "Pause Preview"
                : isActive
                  ? "Resume Preview"
                  : "Preview";

            const bpmControl = track.analysisStatus === "analyzing" ? (
              <span className="analysis-running"><span />Analyzing...</span>
            ) : track.bpm ? (
              <button
                className={`bpm-value${track.isCorrected ? " bpm-value--manual" : ""}`}
                type="button"
                disabled={libraryBusy || analysisBusy || track.isMissing}
                onClick={() => onEditGrid(track)}
                title={`Beatgrid - ${track.beatCount} beats - First downbeat ${formatDuration(track.firstBeatMs ?? 0)} - Click to edit`}
              >
                <strong>{track.bpm.toFixed(2)}</strong>
                <span className="bpm-edit-hint" aria-hidden="true">EDIT</span>
              </button>
            ) : (
              <button
                className="bpm-value bpm-value--uncertain"
                type="button"
                disabled={libraryBusy || analysisBusy || track.isMissing}
                onClick={() => onEditGrid(track)}
                title={track.analysisError ?? "Open Beatgrid to inspect or reanalyze this track"}
              >
                <strong>--</strong>
                <span className="bpm-edit-hint" aria-hidden="true">EDIT</span>
              </button>
            );

            return (
              <div
                className={`library-row${track.isMissing ? " library-row--missing" : ""}${isActive ? " library-row--active" : ""}${isInTimeline ? " library-row--in-use" : ""}`}
                role="row"
                key={track.id}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setContextMenu({
                    track,
                    x: Math.min(event.clientX, window.innerWidth - 174),
                    y: Math.min(event.clientY, window.innerHeight - 48),
                  });
                }}
              >
                <div className="library-track-cell" role="cell">
                  <button
                    className="timeline-drag-handle"
                    type="button"
                    disabled={timelineAddBusy || track.isMissing || track.bpm === null}
                    onDragStart={(event) => event.preventDefault()}
                    onPointerDown={(event) => startPointerDrag(event, track.id)}
                    onPointerMove={movePointerDrag}
                    onPointerUp={finishPointerDrag}
                    onPointerCancel={cancelPointerDrag}
                    onClick={(event) => {
                      if (performance.now() < suppressClickUntil.current) {
                        event.preventDefault();
                        return;
                      }
                      onAddToTimeline(track);
                    }}
                    aria-label={`Add ${displayName} to the timeline`}
                    title="Drag to a track, or click to add at the playhead on the next free track"
                  >
                    +
                  </button>
                  <div className="library-track-details">
                    <div className="library-track-title-line">
                      {/* Le nom écoute le morceau, comme la flèche à côté.
                          C'est le geste qu'on tente en premier devant une
                          liste de musique, et il ne faisait rien. */}
                      <strong
                        className="library-track-name"
                        role="button"
                        tabIndex={previewDisabled || isLoading || track.isMissing ? -1 : 0}
                        aria-disabled={previewDisabled || isLoading || track.isMissing}
                        title={previewTitle}
                        onClick={() => {
                          if (previewDisabled || isLoading || track.isMissing) return;
                          onPreview(track);
                        }}
                        onKeyDown={(event) => {
                          if (event.key !== "Enter" && event.key !== " ") return;
                          event.preventDefault();
                          if (previewDisabled || isLoading || track.isMissing) return;
                          onPreview(track);
                        }}
                      >
                        {displayName}
                      </strong>
                      {bpmControl}
                    </div>
                    <span title={track.filePath}>{track.filePath}</span>
                  </div>
                </div>
                <div className="bpm-cell library-preview-cell" role="cell">
                  <button
                    className="library-inline-preview"
                    type="button"
                    disabled={previewDisabled || isLoading || track.isMissing}
                    onClick={() => onPreview(track)}
                    aria-label={previewTitle}
                    title={previewTitle}
                  >
                    {previewLabel}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
      {contextMenu && (
        <div
          ref={contextMenuRef}
          className="library-context-menu"
          role="menu"
          aria-label={`Actions for ${libraryDisplayName(contextMenu.track)}`}
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <button
            type="button"
            role="menuitem"
            className="is-destructive"
            disabled={libraryBusy || analysisBusy}
            onClick={() => {
              setContextMenu(null);
              onRemove(contextMenu.track);
            }}
          >
            Remove Track
          </button>
        </div>
      )}
    </section>
  );
}
