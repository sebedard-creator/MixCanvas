import { useEffect, useState } from "react";

import { invoke } from "@tauri-apps/api/core";

import { formatDuration } from "../lib/formatDuration";
import { libraryDisplayName } from "../lib/libraryDisplayName";
import { calculateTapTempo, nextTapSeries } from "../lib/tapTempo";
import type { LibraryTrack } from "../library/types";
import { MiniPreview } from "./MiniPreview";

interface BeatgridEditorProps {
  track: LibraryTrack;
  previewFilePath: string | null;
  previewPositionMs: number;
  previewDurationMs: number;
  isPreviewPlaying: boolean;
  busy: boolean;
  onClose: () => void;
  onPreview: () => void;
  onSeekPreview: (positionMs: number) => void;
  onReanalyze: () => void;
  onSave: (bpm: number, firstBeatMs: number) => void;
  onReset: () => void;
}

function parseNumber(value: string): number {
  return Number(value.replace(",", "."));
}

export function BeatgridEditor({
  track,
  previewFilePath,
  previewPositionMs,
  previewDurationMs,
  isPreviewPlaying,
  busy,
  onClose,
  onPreview,
  onSeekPreview,
  onReanalyze,
  onSave,
  onReset,
}: BeatgridEditorProps) {
  const [bpmInput, setBpmInput] = useState(String(track.bpm ?? 120));
  const [firstBeatInput, setFirstBeatInput] = useState(((track.firstBeatMs ?? 0) / 1_000).toFixed(3));
  const [taps, setTaps] = useState<number[]>([]);
  const [snapping, setSnapping] = useState(false);
  const [snapNote, setSnapNote] = useState<string | null>(null);

  useEffect(() => {
    setBpmInput(String(track.bpm ?? 120));
    setFirstBeatInput(((track.firstBeatMs ?? 0) / 1_000).toFixed(3));
    setTaps([]);
  }, [track.id, track.bpm, track.firstBeatMs]);

  const bpm = parseNumber(bpmInput);
  const firstBeatSeconds = parseNumber(firstBeatInput);
  const firstBeatMs = Math.max(0, Math.round(firstBeatSeconds * 1_000));
  const previewMatches = previewFilePath === track.filePath;
  const isValid =
    Number.isFinite(bpm) &&
    bpm >= 40 &&
    bpm <= 300 &&
    Number.isFinite(firstBeatSeconds) &&
    firstBeatSeconds >= 0 &&
    firstBeatMs <= track.durationMs;

  const scaleBpm = (factor: number) => {
    const scaled = Math.round(bpm * factor * 1_000) / 1_000;
    if (Number.isFinite(scaled)) {
      setBpmInput(String(scaled));
    }
  };

  /* Deux frappes suffisent à produire un tempo : c'est à partir de là que le
     recalage a quelque chose à recaler. */
  const hasTaps = taps.length >= 2;

  const tap = () => {
    const next = nextTapSeries(taps, performance.now());
    const tappedBpm = calculateTapTempo(next);
    setTaps(next);
    setSnapNote(null);
    if (tappedBpm !== null) {
      setBpmInput(String(tappedBpm));
    }
  };

  /**
   * Le tap donne l'ordre de grandeur; les événements du modèle sont ensuite
   * ajustés à une grille DJ rigide sur tout le morceau. Un beat manquant ou
   * surnuméraire ne peut donc pas déplacer les suivants.
   */
  const snapToKicks = async () => {
    const tapped = parseNumber(bpmInput);
    if (!Number.isFinite(tapped) || tapped <= 0 || snapping) return;
    setSnapping(true);
    setSnapNote(null);
    try {
      const refined = await invoke<{ bpm: number; firstBeatMs: number; confidence: number }>(
        "refine_tapped_tempo",
        { id: track.id, tappedBpm: tapped },
      );
      const drift = refined.bpm - tapped;
      setBpmInput(String(Math.round(refined.bpm * 1000) / 1000));
      setFirstBeatInput((refined.firstBeatMs / 1_000).toFixed(3));
      setSnapNote(
        `Snapped to ${refined.bpm.toFixed(2)} BPM (${drift >= 0 ? "+" : ""}${drift.toFixed(2)} from your tap), first downbeat at ${(refined.firstBeatMs / 1000).toFixed(3)} s.`,
      );
    } catch (error) {
      setSnapNote(error instanceof Error ? error.message : String(error));
    } finally {
      setSnapping(false);
    }
  };

  return (
    <section className="beatgrid-editor" aria-labelledby="beatgrid-editor-title">
      <div className="beatgrid-editor-header">
        <div>
          <p className="eyebrow">BEATGRID EDITOR</p>
          <h2 id="beatgrid-editor-title">{libraryDisplayName(track)}</h2>
          <p title={track.filePath}>{track.filePath}</p>
        </div>
        <div className="beatgrid-header-actions">
          {track.isCorrected && <span className="manual-badge">Manual</span>}
          <button className="editor-close-button" type="button" onClick={onClose} aria-label="Close editor">
            ×
          </button>
        </div>
      </div>

      <MiniPreview
        fileName={previewMatches ? track.fileName : null}
        durationMs={previewDurationMs}
        positionMs={previewPositionMs}
        isPlaying={previewMatches && isPreviewPlaying}
        disabled={busy || track.isMissing}
        onToggle={onPreview}
        onSeek={onSeekPreview}
      />

      <div className="beatgrid-editor-body">
        <div className="beatgrid-field-group">
          <label htmlFor="source-bpm">Source BPM</label>
          <div className="bpm-edit-row">
            <input
              id="source-bpm"
              type="number"
              min="40"
              max="300"
              step="0.001"
              value={bpmInput}
              onChange={(event) => setBpmInput(event.currentTarget.value)}
            />
            <button type="button" onClick={() => scaleBpm(0.5)} disabled={!Number.isFinite(bpm)}>
              ÷2
            </button>
            <button type="button" onClick={() => scaleBpm(2)} disabled={!Number.isFinite(bpm)}>
              ×2
            </button>
            {/* Les deux boutons forment une séquence, pas deux commandes
                voisines. Taper donne l'ordre de grandeur, recaler demande à
                l'audio de trancher — et une main ne tombe pratiquement jamais
                sur le tempo exact, donc le second geste n'est pas optionnel.
                Le recalage reste manuel : il relit le fichier, ce qui n'a pas
                sa place entre deux frappes. */}
            <div className="tempo-assist">
              <button className="tap-tempo-button" type="button" onClick={tap}>
                Tap
                <span>{taps.length > 0 ? `${taps.length}/9` : "Tempo"}</span>
              </button>
              <span className="tempo-assist-arrow" aria-hidden="true">→</span>
              <button
                className="snap-kicks-button"
                type="button"
                onClick={() => void snapToKicks()}
                disabled={busy || snapping || !Number.isFinite(bpm) || bpm <= 0}
                title="Fit an exact, rigid beat grid around the tempo you tapped"
              >
                {snapping ? "Snapping…" : "Snap to beat"}
              </button>
            </div>
          </div>
          {snapNote ? (
            <p className="snap-kicks-note">{snapNote}</p>
          ) : (
            <p className="tempo-assist-hint">
              {hasTaps
                ? "Now fit the exact beat grid carried by the whole track."
                : "Tap the beat for a rough tempo, then snap it to the track. Tapping alone rarely lands on the exact BPM."}
            </p>
          )}
          <p>
            Automatic analysis: {track.analyzedBpm?.toFixed(2) ?? "—"} BPM
            {track.bpmConfidence !== null ? ` · confidence ${Math.round(track.bpmConfidence * 100)}%` : ""}
          </p>
        </div>

        <div className="beatgrid-field-group">
          <label htmlFor="first-beat">First Downbeat (1)</label>
          <div className="first-beat-row">
            <div className="seconds-input">
              <input
                id="first-beat"
                type="number"
                min="0"
                max={track.durationMs / 1_000}
                step="0.001"
                value={firstBeatInput}
                onChange={(event) => setFirstBeatInput(event.currentTarget.value)}
              />
              <span>seconds</span>
            </div>
            <button
              className="capture-beat-button"
              type="button"
              disabled={!previewMatches || busy}
              onClick={() => setFirstBeatInput((previewPositionMs / 1_000).toFixed(3))}
            >
              Set to {formatDuration(previewPositionMs)}
            </button>
          </div>
          <p>
            Seek the Preview to beat 1 of a bar, then capture its position. This manual correction
            remains authoritative when the automatic downbeat is ambiguous.
          </p>
        </div>
      </div>

      <div className="beatgrid-editor-footer">
        <div>
          <button className="text-button" type="button" onClick={onReanalyze} disabled={busy || track.isMissing}>
            Reanalyze
          </button>
          <button className="text-button" type="button" onClick={onReset} disabled={busy || !track.isCorrected}>
            Restore Automatic
          </button>
        </div>
        <button
          className="primary-button"
          type="button"
          disabled={busy || !isValid}
          onClick={() => onSave(bpm, firstBeatMs)}
        >
          {busy ? "Saving…" : "Save Correction"}
        </button>
      </div>
    </section>
  );
}
