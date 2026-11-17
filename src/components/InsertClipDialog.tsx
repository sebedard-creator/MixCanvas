import { useEffect, useRef } from "react";
import { BEATS_PER_MEASURE } from "../lib/timelineSnap";
import type { TimelineInsertPreview } from "../lib/timelineInsert";
import type { TimelineInsertion } from "../timeline/types";

interface InsertClipDialogProps {
  sourceName: string;
  targetName: string;
  before: TimelineInsertPreview | null;
  after: TimelineInsertPreview | null;
  busy: boolean;
  onChoose: (position: TimelineInsertion["position"]) => void;
  onCancel: () => void;
}

/** Native modal owns focus trapping, background inertness and focus restoration. */
export function InsertClipDialog({ sourceName, targetName, before, after, busy, onChoose, onCancel }: InsertClipDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    cancelButton.current?.focus();
    return () => element?.close();
  }, []);

  return (
    <dialog ref={dialog} className="timeline-insert-dialog" aria-labelledby="timeline-insert-title"
      aria-describedby="timeline-insert-description"
      onKeyDown={(event) => event.stopPropagation()}
      onCancel={(event) => { event.preventDefault(); onCancel(); }}>
      <h2 id="timeline-insert-title">Insert a track</h2>
      <p id="timeline-insert-description">Insert <strong>{sourceName}</strong> before or after <strong>{targetName}</strong>?</p>
      <p>Following clips, tempo points and all automation move together on all four tracks. The existing clip is kept.</p>
      <div className="timeline-insert-options">
        {([ ["before", before], ["after", after] ] as const).map(([position, preview]) => (
          <button key={position} type="button" disabled={busy || !preview} onClick={() => onChoose(position)}>
            <strong>Insert {position}</strong>
            <span>{preview ? `Shift the following mix by ${preview.shiftBeats / BEATS_PER_MEASURE} bars` : "Not enough room on the timeline"}</span>
          </button>
        ))}
      </div>
      <div className="timeline-insert-footer">
        <span>Ctrl+Z undoes the whole insertion.</span>
        <button ref={cancelButton} type="button" onClick={onCancel}>Cancel</button>
      </div>
    </dialog>
  );
}
