import type { CSSProperties } from "react";

import { formatDuration } from "../lib/formatDuration";

interface MiniPreviewProps {
  fileName: string | null;
  durationMs: number;
  positionMs: number;
  isPlaying: boolean;
  disabled: boolean;
  onToggle: () => void;
  onSeek: (positionMs: number) => void;
}

export function MiniPreview({ fileName, durationMs, positionMs, isPlaying, disabled, onToggle, onSeek }: MiniPreviewProps) {
  if (!fileName) return null;
  const maximum = Math.max(1, durationMs);
  const position = Math.min(positionMs, maximum);
  const progress = durationMs > 0 ? Math.min(100, positionMs / durationMs * 100) : 0;

  return (
    <section className="mini-preview" aria-label={`Preview of ${fileName}`}>
      <button type="button" className="mini-preview-toggle" disabled={disabled} onClick={onToggle} aria-label={isPlaying ? "Pause Preview" : "Play Preview"}>
        {isPlaying ? "Ⅱ" : "▶"}
      </button>
      <div className="mini-preview-main">
        <strong title={fileName}>{fileName}</strong>
        <input
          className="mini-preview-slider"
          type="range"
          min={0}
          max={maximum}
          step={50}
          value={position}
          disabled={disabled || durationMs <= 0}
          onChange={(event) => onSeek(Number(event.currentTarget.value))}
          aria-label={`Seek in ${fileName}`}
          style={{ "--seek-progress": `${progress}%` } as CSSProperties}
        />
      </div>
      <time>{formatDuration(positionMs)} / {formatDuration(durationMs)}</time>
    </section>
  );
}
