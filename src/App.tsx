import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { BeatgridEditor } from "./components/BeatgridEditor";
import { ClipEqModal } from "./components/ClipEqModal";
import { LibraryPanel } from "./components/LibraryPanel";
import { TimelinePanel } from "./components/TimelinePanel";
import { formatDuration } from "./lib/formatDuration";
import {
  formatAnalysisProgress,
  formatAnalysisSummary,
  formatImportSummary,
} from "./lib/formatImportSummary";
import { UndoRedoHistory } from "./lib/undoRedo";
import type { LibraryPointerDrag } from "./lib/timelinePointerDrag";
import { snapTimelineBeat } from "./lib/timelineSnap";
import { resolveLaneShortcut, resolveSpaceTarget, shouldCaptureTimelineSpace } from "./lib/timelineShortcut";
import { clipToSplit } from "./lib/laneTarget";
import { ANALYSIS_ALGORITHM_VERSION } from "./library/types";
import type {
  AnalysisBatchResult,
  AnalysisProgress,
  LibraryImportResult,
  LibraryTrack,
} from "./library/types";
import type { ClipEqSettings, TimelineClip, TimelineSnapshot, TimelineTransportSnapshot } from "./timeline/types";

type PreviewStatus = "empty" | "paused" | "playing" | "ended";

interface PreviewSnapshot {
  status: PreviewStatus;
  fileName: string | null;
  filePath: string | null;
  durationMs: number;
  positionMs: number;
  sampleRate: number | null;
  channels: number | null;
}

const EMPTY_PREVIEW: PreviewSnapshot = {
  status: "empty",
  fileName: null,
  filePath: null,
  durationMs: 0,
  positionMs: 0,
  sampleRate: null,
  channels: null,
};

const EMPTY_TIMELINE: TimelineSnapshot = {
  projectBpm: 120,
  limiterEnabled: true,
  compressorEnabled: false,
  tempoPoints: [{ beat: 0, bpm: 120, clipId: null }],
  lanes: [
    { lane: 0, isMuted: false, isSolo: false },
    { lane: 1, isMuted: false, isSolo: false },
    { lane: 2, isMuted: false, isSolo: false },
  ],
  clips: [],
  volumeNodes: [],
  panNodes: [],
  filterNodes: [],
};

const EMPTY_TIMELINE_TRANSPORT: TimelineTransportSnapshot = {
  status: "paused",
  positionBeat: 0,
  meterLeft: 0,
  meterRight: 0,
  meterOverload: false,
};

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}

function App() {
  const [preview, setPreview] = useState<PreviewSnapshot>(EMPTY_PREVIEW);
  const [busy, setBusy] = useState(false);
  const [library, setLibrary] = useState<LibraryTrack[]>([]);
  const [timeline, setTimeline] = useState<TimelineSnapshot>(EMPTY_TIMELINE);
  const [timelineTransport, setTimelineTransport] = useState<TimelineTransportSnapshot>(
    EMPTY_TIMELINE_TRANSPORT,
  );
  const [libraryBusy, setLibraryBusy] = useState(false);
  const [timelineBusy, setTimelineBusy] = useState(false);
  const [timelinePreparing, setTimelinePreparing] = useState(false);
  /**
   * Un clic dans la timeline lance-t-il la lecture ?
   *
   * Allumé par défaut, parce que c'est le geste qu'on fait le plus souvent —
   * on clique pour écouter là. Mais quand on place des clips à l'oreille, le
   * même clic relance la musique vingt fois de suite, et il faut pouvoir le
   * retenir. Non persisté : l'état allumé est celui qu'on veut au lancement.
   */
  const [autoplay, setAutoplay] = useState(true);
  /* The lane the keyboard acts on. Pointing anywhere inside a track arms it;
     it starts on A so a shortcut always has a defined target, and the track
     controls show which one it is. */
  const [selectedLane, setSelectedLane] = useState(0);
  const [analysisBusy, setAnalysisBusy] = useState(false);
  const [gridBusy, setGridBusy] = useState(false);
  const [editingTrackId, setEditingTrackId] = useState<number | null>(null);
  const [editingClipEq, setEditingClipEq] = useState<TimelineClip | null>(null);
  const [previewingTrackId, setPreviewingTrackId] = useState<number | null>(null);
  const [libraryMessage, setLibraryMessage] = useState<string | null>(null);
  const [libraryPointerDrag, setLibraryPointerDrag] = useState<LibraryPointerDrag | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pendingAnalysisUpgradeIds, setPendingAnalysisUpgradeIds] = useState<number[]>([]);
  /* Fraction de 0 à 1 pendant un bounce, `null` le reste du temps. Un rendu de
     plusieurs minutes sans retour visible passe pour un gel. */
  const [bounceProgress, setBounceProgress] = useState<number | null>(null);
  const [stemProgress, setStemProgress] = useState<number | null>(null);
  /**
   * Ce que le rendu hors ligne en cours est en train de faire.
   *
   * Séparer et cuire empruntent la même fenêtre — les deux immobilisent un clip
   * le temps d'un rendu, et deux fenêtres identiques n'apprendraient rien de
   * plus. Mais elles ne font pas la même chose, et la barre doit le dire :
   * annoncer une séparation pendant une cuisson envoie chercher un bug là où
   * il n'y en a pas.
   */
  const [renderKind, setRenderKind] = useState<"stems" | "bake">("stems");

  // The history reads the timeline through a ref so that every edit callback
  // stays referentially stable. A callback rebuilt on each snapshot would make
  // effects that depend on it — the Clip EQ live save in particular — re-fire
  // on their own result.
  const timelineRef = useRef(timeline);
  timelineRef.current = timeline;
  /* Read through a ref so arming a lane does not tear down and re-register the
     window key listeners on every click inside the timeline. */
  const selectedLaneRef = useRef(selectedLane);
  selectedLaneRef.current = selectedLane;
  /* Le transport se rafraîchit vingt fois par seconde. Lu par une référence,
     il n'entraîne pas `seekTimeline` — et donc tout le panneau — dans son
     rythme. */
  const timelineTransportRef = useRef(timelineTransport);
  timelineTransportRef.current = timelineTransport;
  const history = useRef(new UndoRedoHistory<TimelineSnapshot>());
  const historyBusy = useRef(false);
  const [canUndo, setCanUndo] = useState(false);
  const [canRedo, setCanRedo] = useState(false);

  const syncHistoryFlags = useCallback(() => {
    setCanUndo(history.current.canUndo);
    setCanRedo(history.current.canRedo);
  }, []);

  /**
   * Runs a timeline mutation and records the previous state only once the
   * backend accepted it, so a rejected edit never leaves an undo step that
   * restores the state the project is already in.
   */
  const runTimelineEdit = useCallback(async (
    edit: () => Promise<TimelineSnapshot>,
    onFailure?: () => Promise<unknown>,
  ): Promise<boolean> => {
    const previousSnapshot = timelineRef.current;
    setError(null);
    try {
      const snapshot = await edit();
      history.current.push(previousSnapshot);
      syncHistoryFlags();
      setTimeline(snapshot);
      return true;
    } catch (editError) {
      setError(errorMessage(editError));
      if (onFailure) {
        try {
          await onFailure();
        } catch {
          // Le message de l'édition originale est plus utile que l'échec du rafraîchissement.
        }
      }
      return false;
    }
  }, [syncHistoryFlags]);

  const restoreHistorySnapshot = useCallback(async (
    take: (current: TimelineSnapshot) => TimelineSnapshot | null,
  ) => {
    if (historyBusy.current) return;
    const target = take(timelineRef.current);
    if (!target) return;
    historyBusy.current = true;
    syncHistoryFlags();
    setError(null);
    try {
      const restored = await invoke<TimelineSnapshot>("restore_timeline_snapshot", {
        snapshot: target,
      });
      setTimeline(restored);
      setTimelineTransport(
        await invoke<TimelineTransportSnapshot>("timeline_transport_snapshot"),
      );
    } catch (restoreError) {
      setError(errorMessage(restoreError));
    } finally {
      historyBusy.current = false;
    }
  }, [syncHistoryFlags]);

  const handleUndo = useCallback(
    () => restoreHistorySnapshot((current) => history.current.undo(current)),
    [restoreHistorySnapshot],
  );

  const handleRedo = useCallback(
    () => restoreHistorySnapshot((current) => history.current.redo(current)),
    [restoreHistorySnapshot],
  );

  useEffect(() => {
    const unlisten = listen<number>("bounce-progress", (event) => {
      setBounceProgress(event.payload);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<number>("stems-progress", (event) => {
      setStemProgress(event.payload);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  // La cuisson a son propre événement. Sans cette écoute la barre restait à
  // zéro du début à la fin — le rendu avançait, l'affichage disait le contraire.
  useEffect(() => {
    const unlisten = listen<number>("bake-progress", (event) => {
      setStemProgress(event.payload);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  /**
   * Chaque piste s'affiche dès qu'elle est analysée.
   *
   * Le tracker appris prend plusieurs secondes par morceau : sur un dossier
   * entier, attendre la fin du lot laissait l'interface immobile assez
   * longtemps pour qu'on la croie plantée. On remplace la rangée concernée et
   * on la laisse là — le lot renvoie la liste complète en terminant, qui
   * corrige tout ce qui aurait pu se perdre en route.
   *
   * Une piste absente de la liste n'est pas insérée : elle a pu être retirée
   * pendant le lot, et la faire réapparaître serait pire que de l'ignorer.
   */
  useEffect(() => {
    const unlisten = listen<AnalysisProgress>("analysis-track", (event) => {
      const { track, done, total } = event.payload;
      setLibrary((current) =>
        current.map((existing) => (existing.id === track.id ? track : existing)),
      );
      setLibraryMessage(formatAnalysisProgress(done, total));
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    const handleUndoRedoShortcuts = (event: KeyboardEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (target?.tagName === "INPUT" || target?.tagName === "TEXTAREA" || target?.isContentEditable) {
        return;
      }
      if ((event.ctrlKey || event.metaKey) && !event.altKey) {
        if (event.key.toLowerCase() === "z") {
          event.preventDefault();
          if (event.shiftKey) {
            void handleRedo();
          } else {
            void handleUndo();
          }
        } else if (event.key.toLowerCase() === "y") {
          event.preventDefault();
          void handleRedo();
        }
      }
    };
    window.addEventListener("keydown", handleUndoRedoShortcuts);
    return () => window.removeEventListener("keydown", handleUndoRedoShortcuts);
  }, [handleUndo, handleRedo]);

  const saveClipEq = useCallback(async (clipId: number, eqSettings: ClipEqSettings) => {
    await runTimelineEdit(() =>
      invoke<TimelineSnapshot>("save_clip_eq", { clipId, eqSettings }),
    );
  }, [runTimelineEdit]);

  const refreshTimeline = useCallback(async () => {
    const snapshot = await invoke<TimelineSnapshot>("timeline_snapshot");
    setTimeline(snapshot);
    return snapshot;
  }, []);

  const backfillLibraryWaveforms = useCallback(async () => {
    try {
      const savedCount = await invoke<number>("backfill_library_waveforms");
      if (savedCount > 0) {
        await refreshTimeline();
      }
    } catch (waveformError) {
      setError(errorMessage(waveformError));
    }
  }, [refreshTimeline]);

  useEffect(() => {
    let cancelled = false;

    const loadInitialState = async () => {
      try {
        const [tracks, timelineSnapshot, transportSnapshot] = await Promise.all([
          invoke<LibraryTrack[]>("list_library_tracks"),
          invoke<TimelineSnapshot>("timeline_snapshot"),
          invoke<TimelineTransportSnapshot>("timeline_transport_snapshot"),
        ]);
        if (!cancelled) {
          setLibrary(tracks);
          setTimeline(timelineSnapshot);
          setTimelineTransport(transportSnapshot);
          const outdatedIds = tracks
            .filter(
              (track) =>
                !track.isMissing && (track.analysisVersion ?? 0) < ANALYSIS_ALGORITHM_VERSION,
            )
            .map((track) => track.id);
          if (outdatedIds.length > 0) {
            setPendingAnalysisUpgradeIds(outdatedIds);
          } else {
            void backfillLibraryWaveforms();
          }
        }
      } catch (loadError) {
        if (!cancelled) {
          setError(errorMessage(loadError));
        }
      }
    };

    void loadInitialState();

    return () => {
      cancelled = true;
    };
  }, [backfillLibraryWaveforms]);

  const refreshPreview = useCallback(async () => {
    try {
      const snapshot = await invoke<PreviewSnapshot>("preview_snapshot");
      setPreview(snapshot);
    } catch (refreshError) {
      setError(errorMessage(refreshError));
    }
  }, []);

  useEffect(() => {
    if (preview.status === "empty") {
      return undefined;
    }

    const interval = window.setInterval(() => {
      void refreshPreview();
    }, preview.status === "playing" ? 200 : 800);

    return () => window.clearInterval(interval);
  }, [preview.status, refreshPreview]);

  const runTransportCommand = useCallback(
    async (command: "play_preview" | "pause_preview" | "stop_preview") => {
      setBusy(true);
      setError(null);

      try {
        const snapshot = await invoke<PreviewSnapshot>(command);
        setPreview(snapshot);
        if (command === "play_preview") {
          setTimelineTransport(
            await invoke<TimelineTransportSnapshot>("timeline_transport_snapshot"),
          );
        }
      } catch (commandError) {
        setError(errorMessage(commandError));
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const analyzeLibraryTracks = useCallback(async (ids: number[]) => {
    if (ids.length === 0) {
      setLibraryMessage("No tracks available for analysis.");
      return;
    }

    const requestedIds = new Set(ids);
    setAnalysisBusy(true);
    setError(null);
    setLibraryMessage(null);
    setLibrary((current) =>
      current.map((track) =>
        requestedIds.has(track.id)
          ? { ...track, analysisStatus: "analyzing", analysisError: null }
          : track,
      ),
    );

    try {
      const result = await invoke<AnalysisBatchResult>("analyze_library_tracks", { ids });
      setLibrary(result.tracks);
      await refreshTimeline();
      setLibraryMessage(formatAnalysisSummary(result));
    } catch (analysisError) {
      setError(errorMessage(analysisError));
      try {
        setLibrary(await invoke<LibraryTrack[]>("list_library_tracks"));
      } catch {
        // Le message d'analyse original est plus utile que l'échec du rafraîchissement.
      }
    } finally {
      setAnalysisBusy(false);
    }
  }, [refreshTimeline]);

  useEffect(() => {
    if (pendingAnalysisUpgradeIds.length === 0 || analysisBusy) {
      return;
    }
    const ids = pendingAnalysisUpgradeIds;
    setPendingAnalysisUpgradeIds([]);
    void analyzeLibraryTracks(ids).then(backfillLibraryWaveforms);
  }, [
    analysisBusy,
    analyzeLibraryTracks,
    backfillLibraryWaveforms,
    pendingAnalysisUpgradeIds,
  ]);

  const importLibraryPaths = useCallback(async (paths: string[]) => {
    if (paths.length === 0) {
      return;
    }

    setLibraryBusy(true);
    setError(null);
    setLibraryMessage(null);

    try {
      const result = await invoke<LibraryImportResult>("import_library_paths", { paths });
      setLibrary(result.tracks);
      setLibraryMessage(formatImportSummary(result));
      if (result.addedTrackIds.length > 0) {
        await analyzeLibraryTracks(result.addedTrackIds);
      }
    } catch (importError) {
      setError(errorMessage(importError));
    } finally {
      setLibraryBusy(false);
    }
  }, [analyzeLibraryTracks]);

  const addMp3Files = useCallback(async () => {
    setError(null);

    try {
      const selectedPaths = await open({
        multiple: true,
        directory: false,
        filters: [{ name: "MP3 Files", extensions: ["mp3"] }],
      });

      if (!selectedPaths) {
        return;
      }

      await importLibraryPaths(Array.isArray(selectedPaths) ? selectedPaths : [selectedPaths]);
    } catch (selectionError) {
      setError(errorMessage(selectionError));
    }
  }, [importLibraryPaths]);

  const addMusicFolder = useCallback(async () => {
    setError(null);

    try {
      const selectedPath = await open({
        multiple: false,
        directory: true,
      });

      if (typeof selectedPath === "string") {
        await importLibraryPaths([selectedPath]);
      }
    } catch (selectionError) {
      setError(errorMessage(selectionError));
    }
  }, [importLibraryPaths]);

  const previewLibraryTrack = useCallback(
    async (track: LibraryTrack) => {
      setPreviewingTrackId(track.id);
      setError(null);

      try {
        let snapshot: PreviewSnapshot;

        if (preview.filePath === track.filePath) {
          snapshot = await invoke<PreviewSnapshot>(
            preview.status === "playing" ? "pause_preview" : "play_preview",
          );
        } else {
          await invoke<PreviewSnapshot>("load_preview", { path: track.filePath });
          snapshot = await invoke<PreviewSnapshot>("play_preview");
        }

        setPreview(snapshot);
        setTimelineTransport(
          await invoke<TimelineTransportSnapshot>("timeline_transport_snapshot"),
        );
      } catch (previewError) {
        setError(errorMessage(previewError));
      } finally {
        setPreviewingTrackId(null);
      }
    },
    [preview.filePath, preview.status],
  );

  const removeLibraryTrack = useCallback(async (track: LibraryTrack) => {
    setLibraryBusy(true);
    setError(null);

    try {
      const tracks = await invoke<LibraryTrack[]>("remove_library_track", { id: track.id });
      setLibrary(tracks);
      await refreshTimeline();
      setEditingTrackId((current) => (current === track.id ? null : current));
      setLibraryMessage(null);
    } catch (removeError) {
      setError(errorMessage(removeError));
    } finally {
      setLibraryBusy(false);
    }
  }, [refreshTimeline]);

  const saveBeatgridCorrection = useCallback(
    async (track: LibraryTrack, bpm: number, firstBeatMs: number) => {
      setGridBusy(true);
      setError(null);

      try {
        const tracks = await invoke<LibraryTrack[]>("update_track_beatgrid", {
          id: track.id,
          bpm,
          firstBeatMs,
        });
        setLibrary(tracks);
        await refreshTimeline();
        // The backend quantises the entered position onto the analysed grid,
        // so the message reports what was stored rather than what was typed.
        const stored = tracks.find((candidate) => candidate.id === track.id);
        const savedFirstBeatMs = stored?.firstBeatMs ?? firstBeatMs;
        const snapped = Math.abs(savedFirstBeatMs - firstBeatMs) >= 1;
        setLibraryMessage(
          `${track.fileName} now uses ${bpm.toFixed(3)} BPM with its first downbeat at ${formatDuration(savedFirstBeatMs)}${
            snapped ? ", snapped to the nearest detected beat" : ""
          }.`,
        );
      } catch (gridError) {
        setError(errorMessage(gridError));
      } finally {
        setGridBusy(false);
      }
    },
    [refreshTimeline],
  );

  const resetBeatgridCorrection = useCallback(async (track: LibraryTrack) => {
    setGridBusy(true);
    setError(null);

    try {
      const tracks = await invoke<LibraryTrack[]>("reset_track_beatgrid", { id: track.id });
      setLibrary(tracks);
      await refreshTimeline();
      setLibraryMessage(`${track.fileName} now uses the automatic analysis again.`);
    } catch (gridError) {
      setError(errorMessage(gridError));
    } finally {
      setGridBusy(false);
    }
  }, [refreshTimeline]);

  const addTimelineClip = useCallback(async (
    trackId: number,
    anchorBeat?: number,
    lane?: number,
  ) => {
    setTimelineBusy(true);

    try {
      const added = await runTimelineEdit(
        () => invoke<TimelineSnapshot>("add_timeline_clip", {
          libraryTrackId: trackId,
          anchorBeat: anchorBeat ?? null,
          lane: lane ?? null,
        }),
        refreshTimeline,
      );
      if (added) {
        void backfillLibraryWaveforms();
      }
    } finally {
      setTimelineBusy(false);
    }
  }, [backfillLibraryWaveforms, refreshTimeline, runTimelineEdit]);

  const PROJECT_FILTER = { name: "MixCanvas project", extensions: ["mixcanvas"] };

  /**
   * Rend le mix complet dans un WAV, hors ligne.
   *
   * Le rendu n'est pas temps réel : il tire la source aussi vite que la
   * machine le permet, et peut durer plusieurs minutes sur un long projet.
   * D'où l'état occupé, et un message qui dit ce qui a été écrit.
   */
  const bounceMix = useCallback(async () => {
    const path = await save({
      filters: [{ name: "WAV audio", extensions: ["wav"] }],
      defaultPath: "mix.wav",
    });
    if (!path) return;
    setTimelineBusy(true);
    setError(null);
    setLibraryMessage(null);
    setBounceProgress(0);
    try {
      const summary = await invoke<{
        path: string;
        durationSeconds: number;
        trimmedSeconds: number;
        sampleRate: number;
        bitsPerSample: number;
      }>("bounce_mix", { path });
      const trimmed = summary.trimmedSeconds > 0.001
        ? `, ${summary.trimmedSeconds.toFixed(2)} s of leading silence skipped`
        : "";
      setLibraryMessage(
        `Bounced ${formatDuration(summary.durationSeconds * 1000)} to ${summary.path}`
        + ` — ${summary.bitsPerSample}-bit ${summary.sampleRate / 1000} kHz stereo${trimmed}.`,
      );
    } catch (bounceError) {
      setLibraryMessage(null);
      setError(errorMessage(bounceError));
    } finally {
      setBounceProgress(null);
      setTimelineBusy(false);
    }
  }, []);

  const saveProject = useCallback(async () => {
    const path = await save({ filters: [PROJECT_FILTER], defaultPath: "session.mixcanvas" });
    if (!path) return;
    setTimelineBusy(true);
    setError(null);
    try {
      await invoke("save_project", { path });
      setLibraryMessage(`Project saved to ${path}`);
    } catch (saveError) {
      setError(errorMessage(saveError));
    } finally {
      setTimelineBusy(false);
    }
  }, []);

  /**
   * Ouvrir un projet remplace la session : l'historique d'annulation le suit.
   * Ses instantanés décrivent les clips de la session précédente, qui n'existent
   * plus — un Undo après un chargement restaurerait un état d'un autre projet.
   */
  const loadProject = useCallback(async () => {
    const path = await open({ multiple: false, filters: [PROJECT_FILTER] });
    if (typeof path !== "string") return;
    setTimelineBusy(true);
    setError(null);
    try {
      const snapshot = await invoke<TimelineSnapshot>("load_project", { path });
      setTimeline(snapshot);
      history.current.clear();
      syncHistoryFlags();
      setLibrary(await invoke<LibraryTrack[]>("list_library_tracks"));
      setTimelineTransport(
        await invoke<TimelineTransportSnapshot>("timeline_transport_snapshot"),
      );
      setLibraryMessage(`Project loaded from ${path}`);
      void backfillLibraryWaveforms();
    } catch (loadError) {
      setError(errorMessage(loadError));
    } finally {
      setTimelineBusy(false);
    }
  }, [backfillLibraryWaveforms, syncHistoryFlags]);

  const drawTimelineVolumeShape = useCallback(async (
    lane: number,
    startBeat: number,
    endBeat: number,
    nodes: [number, number][],
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("draw_timeline_volume_shape", {
        lane, startBeat, endBeat, nodes,
      }),
      refreshTimeline,
    );
  }, [refreshTimeline, runTimelineEdit]);

  const drawTimelinePanShape = useCallback(async (
    lane: number,
    startBeat: number,
    endBeat: number,
    nodes: [number, number][],
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("draw_timeline_pan_shape", {
        lane, startBeat, endBeat, nodes,
      }),
      refreshTimeline,
    );
  }, [refreshTimeline, runTimelineEdit]);

  const drawTimelineFilterStroke = useCallback(async (
    lane: number,
    nodes: [number, number][],
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("draw_timeline_filter_stroke", { lane, nodes }),
      refreshTimeline,
    );
  }, [refreshTimeline, runTimelineEdit]);

  const addTimelinePanNode = useCallback(async (lane: number, beat: number) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("add_timeline_pan_node", { lane, beat }),
    );
  }, [runTimelineEdit]);

  const moveTimelinePanNode = useCallback(async (
    nodeId: number,
    beat: number,
    value: number,
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("move_timeline_pan_node", { nodeId, beat, value }),
      refreshTimeline,
    );
  }, [refreshTimeline, runTimelineEdit]);

  const deleteTimelinePanNode = useCallback(async (nodeId: number) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("delete_timeline_pan_node", { nodeId }),
    );
  }, [runTimelineEdit]);

  const trimTimelineClip = useCallback(async (
    clipId: number,
    trimStartBeats: number,
    trimEndBeats: number,
  ) => {
    setTimelineBusy(true);

    try {
      await runTimelineEdit(
        () => invoke<TimelineSnapshot>("trim_timeline_clip", {
          clipId,
          trimStartBeats,
          trimEndBeats,
        }),
        refreshTimeline,
      );
    } finally {
      setTimelineBusy(false);
    }
  }, [refreshTimeline, runTimelineEdit]);

  const moveTimelineClip = useCallback(async (
    clipId: number,
    anchorBeat: number,
    lane: number,
  ) => {
    setTimelineBusy(true);

    try {
      await runTimelineEdit(
        () => invoke<TimelineSnapshot>("move_timeline_clip", { clipId, anchorBeat, lane }),
        refreshTimeline,
      );
    } finally {
      setTimelineBusy(false);
    }
  }, [refreshTimeline, runTimelineEdit]);

  const moveTimelineTempoPoint = useCallback(async (
    clipId: number,
    tempoAnchorBeat: number,
  ) => {
    setTimelineBusy(true);

    try {
      await runTimelineEdit(
        () => invoke<TimelineSnapshot>("move_timeline_tempo_point", {
          clipId,
          tempoAnchorBeat,
        }),
        refreshTimeline,
      );
    } finally {
      setTimelineBusy(false);
    }
  }, [refreshTimeline, runTimelineEdit]);

  const removeTimelineClip = useCallback(async (clipId: number) => {
    setTimelineBusy(true);

    try {
      await runTimelineEdit(
        () => invoke<TimelineSnapshot>("remove_timeline_clip", { clipId }),
      );
    } finally {
      setTimelineBusy(false);
    }
  }, [runTimelineEdit]);

  const clearTimeline = useCallback(async () => {
    setTimelineBusy(true);

    try {
      const cleared = await runTimelineEdit(
        () => invoke<TimelineSnapshot>("clear_timeline"),
      );
      if (cleared) {
        setTimelineTransport(
          await invoke<TimelineTransportSnapshot>("timeline_transport_snapshot"),
        );
      }
    } finally {
      setTimelineBusy(false);
    }
  }, [runTimelineEdit]);

  const toggleTimelineTransport = useCallback(async () => {
    if (timelinePreparing) {
      return;
    }
    setError(null);
    const isStarting = timelineTransport.status !== "playing";
    if (isStarting) {
      setTimelinePreparing(true);
    }
    try {
      const command = timelineTransport.status === "playing" ? "pause_timeline" : "play_timeline";
      setTimelineTransport(await invoke<TimelineTransportSnapshot>(command));
      if (isStarting) {
        setPreview(await invoke<PreviewSnapshot>("preview_snapshot"));
      }
    } catch (transportError) {
      setError(errorMessage(transportError));
    } finally {
      if (isStarting) {
        setTimelinePreparing(false);
      }
    }
  }, [timelinePreparing, timelineTransport.status]);

  const seekTimeline = useCallback(async (positionBeat: number) => {
    setError(null);
    // L'autoplay ne dépend de rien d'autre que de lui-même : allumé, un clic
    // lance la lecture; éteint, il ne fait que poser la tête. Il ne s'appliquait
    // avant qu'au passage depuis le miniplayer, ce qui rendait le même geste
    // tantôt silencieux, tantôt sonore, selon un état qu'on n'avait pas en tête.
    //
    // Deux cas ne sont pas un démarrage et n'en demandent pas : une timeline
    // vide, et une lecture déjà en cours — que `play_timeline` reconstruirait
    // pour rien, avec le trou que ça s'entend.
    const shouldStart =
      autoplay
      && timeline.clips.length > 0
      && timelineTransportRef.current.status !== "playing";
    if (shouldStart) {
      setTimelinePreparing(true);
    }
    try {
      setTimelineTransport(
        await invoke<TimelineTransportSnapshot>("seek_timeline", { positionBeat }),
      );
      if (shouldStart) {
        // `play_timeline` libère la sortie, donc rend le miniplayer muet de
        // lui-même. L'instantané qui suit remet l'interface d'accord avec ça.
        setTimelineTransport(await invoke<TimelineTransportSnapshot>("play_timeline"));
        setPreview(await invoke<PreviewSnapshot>("preview_snapshot"));
      }
    } catch (transportError) {
      setError(errorMessage(transportError));
    } finally {
      if (shouldStart) {
        setTimelinePreparing(false);
      }
    }
  }, [autoplay, timeline.clips.length]);

  const seekPreview = useCallback(async (positionMs: number) => {
    setError(null);
    try {
      setPreview(await invoke<PreviewSnapshot>("seek_preview", { positionMs }));
    } catch (seekError) {
      setError(errorMessage(seekError));
    }
  }, []);

  const setTimelineLaneMuted = useCallback(async (lane: number, isMuted: boolean) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("set_timeline_lane_muted", { lane, isMuted }),
    );
  }, [runTimelineEdit]);

  const setTimelineLaneSolo = useCallback(async (lane: number, isSolo: boolean) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("set_timeline_lane_solo", { lane, isSolo }),
    );
  }, [runTimelineEdit]);

  const setTimelineLimiterEnabled = useCallback(async (limiterEnabled: boolean) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("set_timeline_limiter_enabled", { limiterEnabled }),
    );
  }, [runTimelineEdit]);

  const setTimelineCompressorEnabled = useCallback(async (compressorEnabled: boolean) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("set_timeline_compressor_enabled", { compressorEnabled }),
    );
  }, [runTimelineEdit]);

  const setTimelineSidechainKey = useCallback(async (clipId: number, isKey: boolean) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("set_timeline_sidechain_key", { clipId, isKey }),
    );
  }, [runTimelineEdit]);

  /**
   * Efface la bibliothèque et la timeline d'un seul geste.
   *
   * Ne passe pas par l'historique : ce que ce bouton détruit dépasse ce qu'un
   * Undo sait rendre — les morceaux de la bibliothèque n'y ont jamais figuré.
   * Le laisser empiler une entrée promettrait un retour en arrière qui n'aurait
   * pas lieu.
   */
  const clearLibraryAndTimeline = useCallback(async () => {
    setError(null);
    try {
      setTimeline(await invoke<TimelineSnapshot>("clear_library_and_timeline"));
      setLibrary(await invoke<LibraryTrack[]>("list_library_tracks"));
      history.current.clear();
      syncHistoryFlags();
    } catch (clearError) {
      setError(errorMessage(clearError));
    }
  }, [syncHistoryFlags]);

  const setTimelineClipStem = useCallback(async (
    clipId: number,
    stem: "full" | "vocals" | "instrumental",
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("set_timeline_clip_stem", { clipId, stem }),
    );
  }, [runTimelineEdit]);

  /**
   * Sépare un morceau, puis fait basculer le clip sur la voix demandée.
   *
   * Les deux ne font qu'un geste pour l'utilisateur : il a cliqué sur `VOX`, il
   * veut entendre la voix. Que ça demande un moment la première fois est un
   * détail d'exécution, pas une seconde décision à prendre.
   *
   * Seule la fenêtre du clip est séparée, marge comprise : sur une longue
   * timeline, séparer six minutes de morceau pour huit mesures utilisées
   * coûterait vingt fois le travail nécessaire.
   */
  const separateAndSelectStem = useCallback(async (
    clipId: number,
    stem: "vocals" | "instrumental",
  ) => {
    setRenderKind("stems");
    setStemProgress(0);
    try {
      // Le second appel n'a de sens que si le premier a réussi. Enchaîner sans
      // regarder faisait échouer la bascule sur « pas encore séparé », dont le
      // message remplaçait celui de la séparation : on voyait la conséquence,
      // jamais la cause.
      const separated = await runTimelineEdit(
        () => invoke<TimelineSnapshot>("separate_clip_stems", { clipId }),
      );
      if (!separated) return;
      await runTimelineEdit(
        () => invoke<TimelineSnapshot>("set_timeline_clip_stem", { clipId, stem }),
      );
    } finally {
      setStemProgress(null);
    }
  }, [runTimelineEdit]);

  /**
   * Cuit un clip, ou défait la cuisson.
   *
   * Le rendu passe par la même barre que la séparation : les deux immobilisent
   * le clip pendant un moment, et les distinguer n'apprendrait rien à qui
   * attend. Défaire est instantané et ne mérite pas de barre.
   */
  const setClipBaked = useCallback(async (clipId: number, baked: boolean) => {
    if (!baked) {
      await runTimelineEdit(() => invoke<TimelineSnapshot>("unbake_clip", { clipId }));
      return;
    }
    setRenderKind("bake");
    setStemProgress(0);
    try {
      await runTimelineEdit(() => invoke<TimelineSnapshot>("bake_clip", { clipId }));
    } finally {
      setStemProgress(null);
    }
  }, [runTimelineEdit]);

  const addTimelineVolumeNode = useCallback(async (lane: number, beat: number) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("add_timeline_volume_node", { lane, beat }),
    );
  }, [runTimelineEdit]);

  const moveTimelineVolumeNode = useCallback(async (
    nodeId: number,
    beat: number,
    gainDb: number | null,
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("move_timeline_volume_node", { nodeId, beat, gainDb }),
      refreshTimeline,
    );
  }, [refreshTimeline, runTimelineEdit]);

  const deleteTimelineVolumeNode = useCallback(async (nodeId: number) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("delete_timeline_volume_node", { nodeId }),
    );
  }, [runTimelineEdit]);

  const drawTimelineFilterBubble = useCallback(async (
    lane: number,
    startBeat: number,
    widthBeats: number,
    value: number,
    shape?: string,
    replacedRange?: { startBeat: number; endBeat: number },
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("draw_timeline_filter_bubble", {
        lane,
        startBeat,
        widthBeats,
        value,
        shape,
        // Resizing supersedes the previous span: naming it lets the backend
        // erase and rewrite in one transaction, with no gap in the audio.
        replacedStartBeat: replacedRange?.startBeat ?? null,
        replacedEndBeat: replacedRange?.endBeat ?? null,
      }),
    );
  }, [runTimelineEdit]);

  const clearTimelineFilterRange = useCallback(async (
    lane: number,
    startBeat: number,
    endBeat: number,
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("clear_timeline_filter_range", {
        lane,
        startBeat,
        endBeat,
      }),
    );
  }, [runTimelineEdit]);

  const moveLibraryPointerDrag = useCallback((
    trackId: number,
    clientX: number,
    clientY: number,
  ) => {
    setLibraryPointerDrag({ trackId, clientX, clientY, phase: "dragging" });
  }, []);

  const dropLibraryPointerDrag = useCallback((
    trackId: number,
    clientX: number,
    clientY: number,
  ) => {
    setLibraryPointerDrag({ trackId, clientX, clientY, phase: "dropped" });
  }, []);

  const clearLibraryPointerDrag = useCallback(() => {
    setLibraryPointerDrag(null);
  }, []);

  useEffect(() => {
    if (timelineTransport.status !== "playing") {
      return undefined;
    }

    const interval = window.setInterval(() => {
      void invoke<TimelineTransportSnapshot>("timeline_transport_snapshot")
        .then(setTimelineTransport)
        .catch((transportError) => setError(errorMessage(transportError)));
    }, 50);

    return () => window.clearInterval(interval);
  }, [timelineTransport.status]);

  // Declared before the keyboard effect that reads it: a `const` referenced
  // from a dependency array is evaluated during render, not after it.
  const editingTrack = library.find((track) => track.id === editingTrackId) ?? null;

  const splitTimelineClipAtPlayhead = useCallback(async () => {
    if (editingClipEq || editingTrackId) return;
    const playheadBeat = timelineTransport.positionBeat;
    const targetClip = clipToSplit(
      timelineRef.current.clips,
      selectedLaneRef.current,
      playheadBeat,
    );
    if (!targetClip) return;

    setTimelineBusy(true);
    try {
      const split = await runTimelineEdit(
        () => invoke<TimelineSnapshot>("split_timeline_clip", {
          clipId: targetClip.id,
          splitBeat: playheadBeat,
        }),
      );
      if (split) {
        void backfillLibraryWaveforms();
      }
    } finally {
      setTimelineBusy(false);
    }
  }, [backfillLibraryWaveforms, editingClipEq, editingTrackId, runTimelineEdit, timelineTransport.positionBeat]);

  useEffect(() => {
    const capturesTimelineSpace = (event: KeyboardEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      return shouldCaptureTimelineSpace(event.code, target?.tagName, target?.isContentEditable);
    };

    const handleTimelineShortcut = (event: KeyboardEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;

      const laneShortcut = resolveLaneShortcut(
        event.key,
        { shift: event.shiftKey, ctrl: event.ctrlKey, alt: event.altKey, meta: event.metaKey },
        target?.tagName,
        target?.isContentEditable,
      );
      if (laneShortcut) {
        event.preventDefault();
        if (event.repeat) return;
        const lane = selectedLaneRef.current;
        const laneState = timelineRef.current.lanes.find((state) => state.lane === lane);
        switch (laneShortcut) {
          case "split":
            void splitTimelineClipAtPlayhead();
            break;
          case "solo":
            void setTimelineLaneSolo(lane, !laneState?.isSolo);
            break;
          case "mute":
            void setTimelineLaneMuted(lane, !laneState?.isMuted);
            break;
          case "volume":
            void addTimelineVolumeNode(
              lane,
              snapTimelineBeat(timelineTransport.positionBeat),
            );
            break;
          case "pan":
            void addTimelinePanNode(
              lane,
              snapTimelineBeat(timelineTransport.positionBeat),
            );
            break;
        }
        return;
      }

      if (!capturesTimelineSpace(event)) {
        return;
      }
      event.preventDefault();
      if (event.repeat) {
        return;
      }

      switch (resolveSpaceTarget({
        beatgridEditor: editingTrack !== null,
        clipEq: editingClipEq !== null,
      })) {
        case "beatgrid-preview":
          if (editingTrack) {
            void previewLibraryTrack(editingTrack);
          }
          break;
        case "timeline":
          void toggleTimelineTransport();
          break;
        default:
          break;
      }
    };
    const suppressSpaceActivation = (event: KeyboardEvent) => {
      if (capturesTimelineSpace(event)) {
        event.preventDefault();
      }
    };

    window.addEventListener("keydown", handleTimelineShortcut);
    window.addEventListener("keyup", suppressSpaceActivation);
    return () => {
      window.removeEventListener("keydown", handleTimelineShortcut);
      window.removeEventListener("keyup", suppressSpaceActivation);
    };
  }, [
    editingClipEq,
    editingTrack,
    previewLibraryTrack,
    addTimelinePanNode,
    addTimelineVolumeNode,
    setTimelineLaneMuted,
    setTimelineLaneSolo,
    splitTimelineClipAtPlayhead,
    timelineTransport.positionBeat,
    toggleTimelineTransport,
  ]);

  const isPlaying = preview.status === "playing";
  const controlsDisabled = busy || timelinePreparing;
  const timelinePlaybackLocked = timelinePreparing || timelineTransport.status === "playing";
  const timelineTrackIds = useMemo(
    () => new Set(timeline.clips.map((clip) => clip.libraryTrackId)),
    [timeline.clips],
  );

  useEffect(() => {
    if (!editingTrack || editingTrack.isMissing) {
      return undefined;
    }

    let isCurrent = true;
    const loadPausedPreview = async () => {
      setGridBusy(true);
      setError(null);
      try {
        const snapshot = await invoke<PreviewSnapshot>("load_preview", {
          path: editingTrack.filePath,
        });
        if (!isCurrent) return;
        setPreview(snapshot);
        setTimelineTransport(
          await invoke<TimelineTransportSnapshot>("timeline_transport_snapshot"),
        );
      } catch (previewError) {
        if (isCurrent) setError(errorMessage(previewError));
      } finally {
        if (isCurrent) setGridBusy(false);
      }
    };

    void loadPausedPreview();
    return () => {
      isCurrent = false;
    };
  }, [editingTrack?.id]);

  return (
    <main className="app-shell">
      <div className="workspace">
        <div className="main-workspace">
          <TimelinePanel
            timeline={timeline}
            transport={timelineTransport}
            busy={timelineBusy || timelinePreparing}
            preparing={timelinePreparing}
            libraryTracks={library}
            libraryPointerDrag={libraryPointerDrag}
            selectedLane={selectedLane}
            onSelectLane={setSelectedLane}
            onLibraryPointerDragComplete={clearLibraryPointerDrag}
            onAddClip={addTimelineClip}
            onMoveClip={moveTimelineClip}
            onTrimClip={trimTimelineClip}
            onOpenClipEq={(clip) => setEditingClipEq(clip)}
            onMoveTempoPoint={moveTimelineTempoPoint}
            onRemoveClip={removeTimelineClip}
            onClearTimeline={clearTimeline}
            onClearEverything={clearLibraryAndTimeline}
            onSaveProject={saveProject}
            onLoadProject={loadProject}
            onBounceMix={bounceMix}
            onTogglePlayback={toggleTimelineTransport}
            onSeek={seekTimeline}
            onSetLaneMuted={setTimelineLaneMuted}
            onSetLaneSolo={setTimelineLaneSolo}
            onSetLimiterEnabled={setTimelineLimiterEnabled}
            onSetCompressorEnabled={setTimelineCompressorEnabled}
            autoplay={autoplay}
            onSetAutoplay={setAutoplay}
            onSetSidechainKey={setTimelineSidechainKey}
            onSetClipStem={setTimelineClipStem}
            onSeparateStems={separateAndSelectStem}
            onSetClipBaked={setClipBaked}
            onAddVolumeNode={addTimelineVolumeNode}
            onAddPanNode={addTimelinePanNode}
            onMovePanNode={moveTimelinePanNode}
            onDeletePanNode={deleteTimelinePanNode}
            onDrawVolumeShape={drawTimelineVolumeShape}
            onDrawPanShape={drawTimelinePanShape}
            onDrawFilterStroke={drawTimelineFilterStroke}
            onMoveVolumeNode={moveTimelineVolumeNode}
            onDeleteVolumeNode={deleteTimelineVolumeNode}
            onDrawFilterBubble={drawTimelineFilterBubble}
            onClearFilterRange={clearTimelineFilterRange}
            onUndo={handleUndo}
            onRedo={handleRedo}
            canUndo={canUndo}
            canRedo={canRedo}
          />
        </div>

        <LibraryPanel
          tracks={library}
          libraryBusy={libraryBusy || gridBusy || timelinePlaybackLocked}
          analysisBusy={analysisBusy || timelinePlaybackLocked}
          timelineAddBusy={libraryBusy || gridBusy || analysisBusy || timelineBusy || timelinePreparing}
          timelineTrackIds={timelineTrackIds}
          previewDisabled={controlsDisabled}
          previewingTrackId={previewingTrackId}
          activePreviewPath={preview.filePath}
          isPreviewPlaying={isPlaying}
          previewFileName={preview.fileName}
          previewDurationMs={preview.durationMs}
          previewPositionMs={preview.positionMs}
          message={libraryMessage}
          onAddFiles={() => void addMp3Files()}
          onAddFolder={() => void addMusicFolder()}
          onEditGrid={(track) => setEditingTrackId(track.id)}
          /* No lane: the backend rotates A-B-C and takes the first that is
             actually free at the playhead. Naming one here is what made this
             refuse itself when the rotation came back round to a busy track. */
          onAddToTimeline={(track) => void addTimelineClip(
            track.id,
            snapTimelineBeat(timelineTransport.positionBeat),
          )}
          onTimelineDragMove={moveLibraryPointerDrag}
          onTimelineDrop={dropLibraryPointerDrag}
          onTimelineDragCancel={clearLibraryPointerDrag}
          onPreview={(track) => void previewLibraryTrack(track)}
          onTogglePreview={() => void runTransportCommand(isPlaying ? "pause_preview" : "play_preview")}
          onSeekPreview={(positionMs) => void seekPreview(positionMs)}
          onRemove={(track) => void removeLibraryTrack(track)}
        />
      </div>

      {editingTrack && (
        <div className="beatgrid-overlay" role="presentation">
          <BeatgridEditor
            track={editingTrack}
            previewFilePath={preview.filePath}
            previewPositionMs={preview.positionMs}
            previewDurationMs={preview.durationMs}
            isPreviewPlaying={isPlaying}
            busy={gridBusy || analysisBusy || timelinePlaybackLocked}
            onClose={() => setEditingTrackId(null)}
            onPreview={() => void previewLibraryTrack(editingTrack)}
            onSeekPreview={(positionMs) => void seekPreview(positionMs)}
            onReanalyze={() => void analyzeLibraryTracks([editingTrack.id])}
            onSave={(bpm, firstBeatMs) =>
              void saveBeatgridCorrection(editingTrack, bpm, firstBeatMs)
            }
            onReset={() => void resetBeatgridCorrection(editingTrack)}
          />
        </div>
      )}

      {editingClipEq && (
        <ClipEqModal
          clip={editingClipEq}
          onClose={() => setEditingClipEq(null)}
          onSave={saveClipEq}
        />
      )}

      {stemProgress !== null && (
        <div className="bounce-overlay" role="dialog" aria-modal="true" aria-labelledby="stems-title">
          <div className="bounce-dialog">
            <p className="bounce-eyebrow">OFFLINE RENDER</p>
            <h2 id="stems-title">
              {renderKind === "bake" ? "Baking clip" : "Separating stems"}
            </h2>
            <div
              className="bounce-track"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(stemProgress * 100)}
            >
              <div className="bounce-fill" style={{ width: `${Math.round(stemProgress * 100)}%` }} />
            </div>
            <p className="bounce-percent">{Math.round(stemProgress * 100)}%</p>
            <p className="bounce-note">
              {renderKind === "bake"
                ? "Its EQ and this lane's automation are going into the sound. The lane goes flat under it, and BAKE undoes this."
                : "Only what this clip plays is separated, with a margin around it — not the whole track."}
            </p>
          </div>
        </div>
      )}

      {bounceProgress !== null && (
        <div className="bounce-overlay" role="dialog" aria-modal="true" aria-labelledby="bounce-title">
          <div className="bounce-dialog">
            <p className="bounce-eyebrow">OFFLINE RENDER</p>
            <h2 id="bounce-title">Bouncing the mix</h2>
            <div
              className="bounce-track"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(bounceProgress * 100)}
            >
              <div className="bounce-fill" style={{ width: `${Math.round(bounceProgress * 100)}%` }} />
            </div>
            <p className="bounce-percent">{Math.round(bounceProgress * 100)}%</p>
            <p className="bounce-note">
              Every clip is decoded and time-stretched from end to end. This is not
              real time — it takes as long as the quality needs.
            </p>
          </div>
        </div>
      )}

      {error && (
        <aside className="error-banner" role="alert">
          <strong>Unable to continue</strong>
          <span>{error}</span>
          <button type="button" onClick={() => setError(null)} aria-label="Close message">
            ×
          </button>
        </aside>
      )}

    </main>
  );
}

export default App;
