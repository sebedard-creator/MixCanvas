import { useEffect, useState } from "react";

import { invoke } from "@tauri-apps/api/core";

import { formatDuration } from "../lib/formatDuration";
import { libraryDisplayName } from "../lib/libraryDisplayName";
import {
  MINIMUM_DOWNBEAT_TAPS,
  RECOMMENDED_DOWNBEAT_TAPS,
  appendDownbeatTap,
  estimateGridFromDownbeatTaps,
  hasExcellentTapAccuracy,
} from "../lib/downbeatTap";
import type { LibraryTrack } from "../library/types";
import { MiniPreview } from "./MiniPreview";

interface BeatgridEditorProps {
  track: LibraryTrack;
  previewFilePath: string | null;
  previewPositionMs: number;
  previewDurationMs: number;
  previewPlaybackSpeed: number;
  isPreviewPlaying: boolean;
  busy: boolean;
  onClose: () => void;
  onPreview: () => void;
  onSeekPreview: (positionMs: number) => void;
  onSetPreviewSpeed: (speed: number) => void;
  onReanalyze: () => void;
  onSave: (bpm: number, firstBeatMs: number) => void;
}

interface PreviewTapSnapshot {
  status: "empty" | "paused" | "playing" | "ended";
  filePath: string | null;
  positionMs: number;
}

function parseNumber(value: string): number {
  return Number(value.replace(",", "."));
}

export function BeatgridEditor({
  track,
  previewFilePath,
  previewPositionMs,
  previewDurationMs,
  previewPlaybackSpeed,
  isPreviewPlaying,
  busy,
  onClose,
  onPreview,
  onSeekPreview,
  onSetPreviewSpeed,
  onReanalyze,
  onSave,
}: BeatgridEditorProps) {
  const [bpmInput, setBpmInput] = useState(String(track.bpm ?? 120));
  const [firstBeatInput, setFirstBeatInput] = useState(((track.firstBeatMs ?? 0) / 1_000).toFixed(3));
  const [taps, setTaps] = useState<number[]>([]);
  const [capturingTap, setCapturingTap] = useState(false);
  const [snapping, setSnapping] = useState(false);
  const [snapNote, setSnapNote] = useState<string | null>(null);
  const [tapAccuracyMs, setTapAccuracyMs] = useState<number | null>(null);

  /**
   * Remet l'atelier d'accord avec le morceau.
   *
   * `track.isCorrected` fait partie des dépendances, et c'est lui qui rend
   * « Restore Automatic » fiable. Les deux autres ne suffisent pas : la
   * sauvegarde cale le premier temps manuel sur la grille analysée, si bien
   * qu'une remise à zéro retombe souvent sur **exactement** les mêmes nombres.
   * L'effet ne se déclenchait alors pas, et l'éditeur gardait les valeurs
   * tapées comme si de rien n'était. La correction, elle, bascule toujours de
   * vrai à faux : c'est le seul signal qui ne peut pas manquer.
   */
  useEffect(() => {
    setBpmInput(String(track.bpm ?? 120));
    setFirstBeatInput(((track.firstBeatMs ?? 0) / 1_000).toFixed(3));
    setTaps([]);
    setSnapNote(null);
    setTapAccuracyMs(null);
  }, [track.id, track.bpm, track.firstBeatMs, track.isCorrected]);

  /**
   * Les valeurs que l'analyse a trouvées, s'il y en a.
   *
   * `Restore Automatic` y ramène les champs — et rien de plus. Il écrivait
   * jusqu'ici dans la base sur-le-champ, ce qui n'a pas de sens dans une
   * fenêtre qui a un bouton `Save` : tant qu'on n'enregistre pas, rien ne doit
   * changer. C'est `Save` qui décide, et la base reconnaît alors que ces
   * valeurs-là ne sont pas une correction.
   */
  const analysedBpm = track.analyzedBpm;
  const analysedFirstBeatMs = track.analyzedFirstBeatMs;
  const canRestore =
    analysedBpm !== null
    && analysedFirstBeatMs !== null
    && (bpmInput !== String(analysedBpm)
      || firstBeatInput !== (analysedFirstBeatMs / 1_000).toFixed(3));

  const restoreAutomatic = () => {
    if (analysedBpm === null || analysedFirstBeatMs === null) return;
    setBpmInput(String(analysedBpm));
    setFirstBeatInput((analysedFirstBeatMs / 1_000).toFixed(3));
    setTaps([]);
    setTapAccuracyMs(null);
    setSnapNote("Back to the analysed grid — Save to keep it.");
  };

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

  const tapEstimate = estimateGridFromDownbeatTaps(taps);
  /* Le même seuil que le bouton du lecteur : les deux doivent s'allumer
     ensemble, sinon l'un dit « demi-vitesse » pendant que l'autre l'ignore. */
  const isSlowPreview = previewPlaybackSpeed < 0.75;

  /**
   * Each press names the next bar's musical 1. The timestamp comes from the
   * Preview engine's source clock, not the browser event clock or the polling
   * display, so UI refresh cadence cannot quantise the manual grid.
   */
  const tapDownbeat = async () => {
    if (!previewMatches || !isPreviewPlaying || busy || capturingTap) return;
    setCapturingTap(true);
    try {
      const snapshot = await invoke<PreviewTapSnapshot>("preview_snapshot");
      if (snapshot.filePath !== track.filePath || snapshot.status !== "playing") {
        setTapAccuracyMs(null);
        setSnapNote("Start this track's Preview before tapping consecutive bar ones.");
        return;
      }

      const next = appendDownbeatTap(taps, snapshot.positionMs);
      const estimate = estimateGridFromDownbeatTaps(next);
      setTaps(next);
      setTapAccuracyMs(null);

      if (next.length === 1) {
        setFirstBeatInput((snapshot.positionMs / 1_000).toFixed(3));
        setSnapNote(`First 1 captured at ${(snapshot.positionMs / 1_000).toFixed(3)} s. Tap the next bar's 1.`);
      } else if (estimate) {
        setBpmInput(String(estimate.bpm));
        setFirstBeatInput((estimate.firstBeatMs / 1_000).toFixed(3));
        const recommendation =
          next.length < RECOMMENDED_DOWNBEAT_TAPS
            ? `Continue to ${RECOMMENDED_DOWNBEAT_TAPS} bars for greater accuracy.`
            : "The manual grid is ready to refine or save.";
        setTapAccuracyMs(estimate.rmsErrorMs);
        setSnapNote(recommendation);
      } else if (next.length < MINIMUM_DOWNBEAT_TAPS) {
        setSnapNote(
          `${next.length}/${MINIMUM_DOWNBEAT_TAPS} bar ones captured. Keep tapping the 1 of every consecutive measure.`,
        );
      } else {
        setSnapNote("Those taps are not consecutive measures. Clear the series and tap every bar's 1 without skipping one.");
      }
    } catch (error) {
      setTapAccuracyMs(null);
      setSnapNote(error instanceof Error ? error.message : String(error));
    } finally {
      setCapturingTap(false);
    }
  };

  /**
   * Le tap donne la période approximative et la position capturée donne
   * l'intention musicale. Le modèle raffine une grille DJ rigide, puis cette
   * position est déplacée sur son beat le plus proche sans jamais être
   * remplacée par le downbeat automatique.
   */
  /**
   * Vrai tant que quelque chose écrit dans les deux champs sous nos yeux.
   *
   * L'affinage les **possède** le temps qu'il tourne : il les réécrira en
   * rendant. Tout ce qui les change ou les enregistre entre-temps se ferait
   * donc effacer sans rien dire — `Save` enregistrait les valeurs d'avant le
   * calcul, et `Restore Automatic` remettait la grille analysée pour la voir
   * disparaître une seconde plus tard. Le même défaut à trois boutons, donc
   * une seule règle plutôt que trois gardes recopiées.
   *
   * Les touches de tap et `Clear`, elles, écrivent ailleurs — dans la série de
   * frappes, pas dans les champs — et restent disponibles.
   */
  const fieldsAreOwned = busy || snapping;

  const snapToKicks = async () => {
    const tapped = parseNumber(bpmInput);
    if (!Number.isFinite(tapped) || tapped <= 0 || !isValid || snapping) return;
    setSnapping(true);
    setTapAccuracyMs(null);
    setSnapNote(null);
    try {
      const refined = await invoke<{ bpm: number; firstBeatMs: number; confidence: number }>(
        "refine_tapped_tempo",
        { id: track.id, tappedBpm: tapped, anchorMs: firstBeatMs },
      );
      const drift = refined.bpm - tapped;
      setBpmInput(String(Math.round(refined.bpm * 1000) / 1000));
      setFirstBeatInput((refined.firstBeatMs / 1_000).toFixed(3));
      setSnapNote(
        `Snapped to ${refined.bpm.toFixed(3)} BPM (${drift >= 0 ? "+" : ""}${drift.toFixed(3)} from the Tap 1 fit). Your chosen downbeat is now exactly on the nearest beat at ${(refined.firstBeatMs / 1000).toFixed(3)} s.`,
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
        playbackSpeed={previewPlaybackSpeed}
        onToggle={onPreview}
        onSeek={onSeekPreview}
        onSetPlaybackSpeed={onSetPreviewSpeed}
      />

      {/* La marche à suivre, dite une fois et dans l'ordre.
       *
       * Les deux champs se présentaient côte à côte, également remplissables,
       * sans rien dire duquel on part : un nouvel arrivant lisait deux
       * méthodes concurrentes au lieu d'une procédure. Ils deviennent le
       * *résultat* de ces trois pas, et l'ordre est écrit plutôt que deviné.
       *
       * La demi-vitesse est une recommandation **actionnable** : la répéter en
       * prose à côté d'un bouton qu'il faut aller chercher ailleurs, c'est la
       * faire ignorer. */}
      <ol className="beatgrid-recipe">
        <li className={isSlowPreview ? "is-done" : undefined}>
          <span className="beatgrid-recipe-step" aria-hidden="true">1</span>
          <div>
            <strong>Play at half speed — strongly recommended.</strong>
            <p>
              Twice as long between bars means half the tapping error. Tap 1 reads the source
              clock, so the tempo it measures is the real one either way.
            </p>
          </div>
          <button
            className={`beatgrid-recipe-action${isSlowPreview ? " is-active" : ""}`}
            type="button"
            disabled={busy || track.isMissing}
            aria-pressed={isSlowPreview}
            onClick={() => onSetPreviewSpeed(isSlowPreview ? 1 : 0.5)}
          >
            {isSlowPreview ? "½ SPEED ON" : "SET ½ SPEED"}
          </button>
        </li>
        <li className={taps.length >= MINIMUM_DOWNBEAT_TAPS ? "is-done" : undefined}>
          <span className="beatgrid-recipe-step" aria-hidden="true">2</span>
          <div>
            <strong>Tap the 1 of every bar, in a row.</strong>
            <p>
              {MINIMUM_DOWNBEAT_TAPS} bars minimum; {RECOMMENDED_DOWNBEAT_TAPS} or more for a grid
              that still holds at the end of a long track.
            </p>
          </div>
        </li>
        <li>
          <span className="beatgrid-recipe-step" aria-hidden="true">3</span>
          <div>
            <strong>Snap to beat, then save.</strong>
            <p>Snapping pulls your taps onto the detected beats. Both fields below fill themselves.</p>
          </div>
        </li>
      </ol>

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
              onChange={(event) => {
                setBpmInput(event.currentTarget.value);
                setTaps([]);
                setSnapNote(null);
                setTapAccuracyMs(null);
              }}
              /* Pas d'incrément au clavier non plus : les flèches sont
                 retirées du champ, et les laisser répondre en cachette ferait
                 deux comportements pour un seul contrôle. */
              onKeyDown={(event) => {
                if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                  event.preventDefault();
                }
              }}
            />
            <div className="tempo-assist">
              <button
                className="tap-tempo-button"
                type="button"
                onClick={() => void tapDownbeat()}
                disabled={busy || capturingTap || !previewMatches || !isPreviewPlaying}
                title="Tap the first beat of each consecutive measure while Preview is playing"
              >
                {capturingTap ? "Reading…" : "Tap 1"}
                <span>{taps.length > 0 ? `${taps.length} BARS` : "EACH BAR"}</span>
              </button>
              <button
                className="downbeat-tap-reset"
                type="button"
                disabled={taps.length === 0}
                onClick={() => {
                  setTaps([]);
                  setSnapNote(null);
                  setTapAccuracyMs(null);
                  setBpmInput(String(track.bpm ?? 120));
                  setFirstBeatInput(((track.firstBeatMs ?? 0) / 1_000).toFixed(3));
                }}
                title="Clear the Tap 1 series"
              >
                Clear
              </button>
              <span className="tempo-assist-arrow" aria-hidden="true">→</span>
              <button
                className="snap-kicks-button"
                type="button"
                onClick={() => void snapToKicks()}
                disabled={busy || snapping || !isValid || tapEstimate === null}
                title="Refine the Tap 1 grid against the track's detected beats"
              >
                {snapping ? "Snapping…" : "Snap to beat"}
              </button>
            </div>
          </div>
          {tapAccuracyMs !== null && tapEstimate ? (
            <p className="snap-kicks-note">
              {taps.length} consecutive bar ones fit {tapEstimate.bpm.toFixed(3)} BPM · accuracy{" "}
              <span
                className={
                  hasExcellentTapAccuracy(taps.length, tapAccuracyMs)
                    ? "tap-accuracy tap-accuracy--good"
                    : "tap-accuracy"
                }
              >
                {tapAccuracyMs}
              </span>{" "}
              ms
              . {snapNote}
            </p>
          ) : snapNote ? (
            <p className="snap-kicks-note">{snapNote}</p>
          ) : (
            <p className="tempo-assist-hint">
              Play the Preview and tap the 1 of each consecutive measure. Four bars are required;
              eight or more improve long-range accuracy.
            </p>
          )}
          <p>
            Automatic analysis: {track.analyzedBpm?.toFixed(2) ?? "—"} BPM
            {track.bpmConfidence !== null ? ` · confidence ${Math.round(track.bpmConfidence * 100)}%` : ""}
          </p>
        </div>

        <div className="beatgrid-field-group">
          <label htmlFor="first-beat">
            First downbeat <span className="beatgrid-field-origin">set by Tap 1</span>
          </label>
          <div className="first-beat-row">
            <div className="seconds-input">
              <input
                id="first-beat"
                type="number"
                min="0"
                max={track.durationMs / 1_000}
                step="0.001"
                value={firstBeatInput}
                onChange={(event) => {
                  setFirstBeatInput(event.currentTarget.value);
                  setTaps([]);
                  setSnapNote(null);
                  setTapAccuracyMs(null);
                }}
              />
              <span>seconds</span>
            </div>
            <button
              className="capture-beat-button"
              type="button"
              disabled={!previewMatches || busy}
              onClick={() => {
                setFirstBeatInput((previewPositionMs / 1_000).toFixed(3));
                setTaps([]);
                setSnapNote(null);
                setTapAccuracyMs(null);
              }}
            >
              Set to {formatDuration(previewPositionMs)}
            </button>
          </div>
          <p>
            You should not need to touch this. It is here for the rare track whose 1 the analysis
            and your taps both miss — seek the Preview to the downbeat and capture it.
          </p>
        </div>
      </div>

      <div className="beatgrid-editor-footer">
        <div>
          <button className="text-button" type="button" onClick={onReanalyze} disabled={fieldsAreOwned || track.isMissing}>
            Reanalyze
          </button>
          <button
            className="text-button"
            type="button"
            onClick={restoreAutomatic}
            disabled={fieldsAreOwned || !canRestore}
            title="Put the analysed tempo and downbeat back in the fields — Save to keep them"
          >
            Restore Automatic
          </button>
        </div>
        <button
          className="primary-button"
          type="button"
          disabled={fieldsAreOwned || !isValid}
          onClick={() => onSave(bpm, firstBeatMs)}
        >
          {busy ? "Saving…" : "Save"}
        </button>
      </div>
    </section>
  );
}
