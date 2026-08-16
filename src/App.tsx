import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
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
import { createLiveTransport } from "./lib/liveTransport";
import type { PlayedEffect } from "./lib/mixEffects";
import { UNDO_HISTORY_LIMIT, UndoRedoHistory } from "./lib/undoRedo";
import {
  BOUNCE_FORMATS,
  BOUNCE_FORMAT_PREFERENCE,
  DEFAULT_BOUNCE_FORMAT,
  DEFAULT_MASTERING_SETTINGS,
  MASTERING_ENABLED_PREFERENCE,
  MASTERING_LIMITS,
  MASTERING_PREFERENCE,
  masteringGainDb,
  parseMasteringSettings,
  serializeMasteringSettings,
  ceilingForFormat,
  parseBounceFormat,
  type BounceFormat,
  type MasteringSettings,
} from "./lib/masteringSettings";
import {
  DEFAULT_LIBRARY_SORT,
  LIBRARY_SORT_PREFERENCE,
  parseLibrarySort,
  serializeLibrarySort,
  type LibrarySort,
} from "./lib/librarySort";
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
  playbackSpeed: number;
  sampleRate: number | null;
  channels: number | null;
}

const EMPTY_PREVIEW: PreviewSnapshot = {
  status: "empty",
  fileName: null,
  filePath: null,
  durationMs: 0,
  positionMs: 0,
  playbackSpeed: 1,
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
    drawGroups: [],
    filterNodes: [],
    reverbNodes: [],
    flangerNodes: [],
    bitcrushNodes: [],
    delayNodes: [],
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
  /**
   * L'état du transport, dans les deux régimes dont il a besoin.
   *
   * `timelineTransport` est celui de React : il ne bouge que lorsqu'un
   * évènement le fait bouger — jouer, mettre en pause, se déplacer, éditer.
   * `liveTransport` est celui de la lecture : il avance vingt fois par
   * seconde, et ce qui en dépend s'y abonne plutôt que de re-rendre.
   */
  const [timelineTransport, setTimelineTransport] = useState<TimelineTransportSnapshot>(
    EMPTY_TIMELINE_TRANSPORT,
  );
  const liveTransport = useRef(createLiveTransport(EMPTY_TIMELINE_TRANSPORT)).current;
  const publishTransport = useCallback(
    (snapshot: TimelineTransportSnapshot) => {
      liveTransport.publish(snapshot);
      setTimelineTransport(snapshot);
    },
    [liveTransport],
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
  /** Vrai pendant qu'un fichier venu du bureau survole la fenêtre. */
  const [fileDropActive, setFileDropActive] = useState(false);
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
  /** L'écran des effets joués. Non persisté : on l'ouvre pour un geste. */
  /**
   * Le tri de la bibliothèque, retenu d'une séance à l'autre.
   *
   * Détenu ici et non dans le panneau : c'est une préférence du programme, elle
   * doit être chargée au démarrage et réécrite à chaque changement. Faire vivre
   * l'état dans le panneau et sa persistance ici aurait donné deux sources pour
   * une même chose.
   */
  const [librarySort, setLibrarySort] = useState<LibrarySort>(DEFAULT_LIBRARY_SORT);

  const changeLibrarySort = useCallback(
    (update: (current: LibrarySort) => LibrarySort) => {
      setLibrarySort((current) => {
        const next = update(current);
        /* Écrit sans attendre : un tri qu'on ne peut pas retenir n'a pas à
           empêcher de trier. L'échec est signalé, le choix s'applique. */
        void invoke("write_app_preference", {
          key: LIBRARY_SORT_PREFERENCE,
          value: serializeLibrarySort(next),
        }).catch((preferenceError) => setError(errorMessage(preferenceError)));
        return next;
      });
    },
    [],
  );

  /**
   * Les passes que l'on est **en train** de jouer, pour que la timeline les
   * dessine avant même qu'elles ne soient écrites.
   *
   * Une liste plate plutôt qu'une table par effet : elle se parcourt telle
   * quelle au rendu, et il n'y en a jamais plus de douze.
   */
  const [livePasses, setLivePasses] = useState<
    Array<{ effect: PlayedEffect; lane: number; startBeat: number }>
  >([]);

  const trackLivePass = useCallback(
    (effect: PlayedEffect, lane: number, startBeat: number | null) => {
      setLivePasses((current) => {
        const rest = current.filter((pass) => pass.effect !== effect || pass.lane !== lane);
        return startBeat === null ? rest : [...rest, { effect, lane, startBeat }];
      });
    },
    [],
  );


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
  /**
   * L'historique ne garde pas les waveforms.
   *
   * Un instantané embarque ses clips, et un clip embarque les crêtes dessinées
   * de tout son audio. Sur cinquante niveaux d'annulation, la même image
   * décodée était donc recopiée cinquante fois en mémoire — de très loin le
   * plus gros poste du programme, pour une donnée que la restauration n'ouvre
   * jamais : `restore_snapshot` ne lit pas `clip.waveform`, et l'interface se
   * redessine à partir de l'instantané que le Rust **renvoie**, relu de la
   * base, pas à partir de celui qu'on lui a envoyé.
   *
   * `null` plutôt qu'un champ retiré : le champ est un `Option` côté Rust, et
   * une forme qui reste valable traverse la frontière sans rien demander.
   */
  const history = useRef(
    new UndoRedoHistory<TimelineSnapshot>(UNDO_HISTORY_LIMIT, (snapshot) => ({
      ...snapshot,
      clips: snapshot.clips.map((clip) => (clip.waveform === null ? clip : { ...clip, waveform: null })),
    })),
  );
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
      publishTransport(
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
        const [tracks, timelineSnapshot, transportSnapshot, preferences] = await Promise.all([
          invoke<LibraryTrack[]>("list_library_tracks"),
          invoke<TimelineSnapshot>("timeline_snapshot"),
          invoke<TimelineTransportSnapshot>("timeline_transport_snapshot"),
          invoke<Record<string, string>>("read_app_preferences"),
        ]);
        if (!cancelled) {
          /* Posé avant la liste : le tri doit être en place quand les morceaux
             arrivent, sinon on les verrait se réordonner sous les yeux. */
          setLibrarySort(parseLibrarySort(preferences[LIBRARY_SORT_PREFERENCE]));
          setMastering(parseMasteringSettings(preferences[MASTERING_PREFERENCE]));
          /* Armé sauf refus explicite : une préférence absente est celle de
             quelqu'un qui n'a jamais ouvert la boîte. */
          setMasteringEnabled(preferences[MASTERING_ENABLED_PREFERENCE] !== "0");
          setBounceFormat(parseBounceFormat(preferences[BOUNCE_FORMAT_PREFERENCE]));
          setLibrary(tracks);
          setTimeline(timelineSnapshot);
          publishTransport(transportSnapshot);
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
          publishTransport(
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

  /* L'import est lu par une référence : l'écouteur natif se pose une fois, et
     ne se démonte pas à chaque fois que la bibliothèque change. */
  const importLibraryPathsRef = useRef(importLibraryPaths);
  importLibraryPathsRef.current = importLibraryPaths;

  /**
   * Les fichiers lâchés depuis l'explorateur entrent dans la bibliothèque.
   *
   * Le dépôt est accepté **partout dans la fenêtre**, pas seulement sur le
   * panneau : un MP3 lâché ici ne peut aller nulle part ailleurs, et exiger de
   * viser juste n'ajouterait que des échecs. Le panneau s'allume pendant le
   * survol pour dire où ça atterrit.
   *
   * Le tri est fait par l'import : il descend dans les dossiers et ne retient
   * que les MP3, donc un dossier entier — ou un mélange — se lâche tel quel.
   */
  useEffect(() => {
    // `getCurrentWebview()` lève quand le pont natif n'est pas là — dans un
    // navigateur ordinaire, pendant le développement de l'interface. Non
    // protégé, il emportait tout le rendu : une fenêtre blanche pour une
    // intégration facultative. Sans pont, on perd le dépôt de fichiers, et
    // rien d'autre.
    let stop: (() => void) | null = null;
    let cancelled = false;
    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          if (event.payload.type === "enter" || event.payload.type === "over") {
            setFileDropActive(true);
            return;
          }
          setFileDropActive(false);
          if (event.payload.type === "drop") {
            void importLibraryPathsRef.current(event.payload.paths);
          }
        })
        .then((unlisten) => {
          if (cancelled) unlisten();
          else stop = unlisten;
        })
        .catch(() => {});
    } catch {
      // Pas de pont : pas de dépôt de fichiers.
    }
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

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
        publishTransport(
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
        const stored = tracks.find((candidate) => candidate.id === track.id);
        const savedBpm = stored?.bpm ?? bpm;
        const savedFirstBeatMs = stored?.firstBeatMs ?? firstBeatMs;
        setLibraryMessage(
          `${track.fileName} now uses ${savedBpm.toFixed(3)} BPM with its first downbeat at ${formatDuration(savedFirstBeatMs)}.`,
        );
      } catch (gridError) {
        setError(errorMessage(gridError));
      } finally {
        setGridBusy(false);
      }
    },
    [refreshTimeline],
  );

  /**
   * Tourne la grille d'un morceau d'un temps, sans toucher à son tempo.
   *
   * L'analyse pose parfois le premier temps sur le deux ou le trois de la
   * mesure. Le calcul vit côté Rust, pas ici : deux menus appellent ce geste —
   * la bibliothèque et le clip — et la période d'un temps recopiée aux deux
   * endroits aurait fini par diverger.
   */
  const shiftTrackDownbeat = useCallback(
    async (trackId: number, beats: number) => {
      setGridBusy(true);
      setError(null);
      try {
        const tracks = await invoke<LibraryTrack[]>("shift_track_downbeat", {
          id: trackId,
          beats,
        });
        setLibrary(tracks);
        await refreshTimeline();
        const stored = tracks.find((candidate) => candidate.id === trackId);
        if (stored?.firstBeatMs != null) {
          setLibraryMessage(
            `${stored.fileName} now starts its bar at ${formatDuration(stored.firstBeatMs)}.`,
          );
        }
      } catch (shiftError) {
        setError(errorMessage(shiftError));
      } finally {
        setGridBusy(false);
      }
    },
    [refreshTimeline],
  );

  /**
   * Règle le tempo d'un morceau depuis son nœud sur la règle.
   *
   * Un nœud de tempo **est** le BPM du morceau posé à cet endroit : l'éditer
   * revient à corriger sa grille, et cela passe donc par le même chemin que
   * l'éditeur de grille — mêmes contrôles, même règle qui efface la correction
   * quand on retape la valeur de l'analyse. Le premier temps ne bouge pas :
   * on règle le tempo, pas la phase.
   */
  /**
   * Le tempo visé à l'ancre d'un clip — une décision de mix, pas une correction
   * d'analyse.
   *
   * Cela passait autrefois par `saveBeatgridCorrection`, donc par la
   * bibliothèque : régler le tempo d'un nœud réécrivait le BPM du morceau, ce
   * qui déplaçait la courbe sous tous les autres clips et perdait leur
   * beatmatching — en écrasant l'analyse au passage, sans retour. Les deux
   * gestes ont maintenant deux chemins; celui-ci ne touche que le clip.
   */
  const setClipTempoTarget = useCallback(async (clipId: number, bpm: number | null) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("set_timeline_clip_tempo_target", {
        clipId,
        targetBpm: bpm,
      }),
      refreshTimeline,
    );
  }, [refreshTimeline, runTimelineEdit]);

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
  /**
   * Le limiteur de mastering, et s'il est armé.
   *
   * Retenu entre deux lancements : c'est un réglage de chaîne, pas une
   * décision qu'on reprend à zéro à chaque rendu. Mais il reste montré avant
   * chaque bounce, parce qu'appliquer trois décibels de gain sans le savoir
   * serait pire que de retaper les chiffres.
   *
   * Armé par défaut. Un mix sorti sans limiteur est plus faible que tout ce
   * à côté de quoi il sera écouté, et c'est le défaut qu'on ne remarque pas :
   * il ne fait pas de bruit, il en fait moins. Le décocher est un geste
   * délibéré, pour qui masterise ailleurs.
   */
  const [masteringEnabled, setMasteringEnabled] = useState(true);
  const [mastering, setMastering] = useState<MasteringSettings>(DEFAULT_MASTERING_SETTINGS);
  const [bounceOptionsOpen, setBounceOptionsOpen] = useState(false);
  const [bounceFormat, setBounceFormat] = useState<BounceFormat>(DEFAULT_BOUNCE_FORMAT);

  const rememberMastering = useCallback((enabled: boolean, settings: MasteringSettings) => {
    void invoke("write_app_preference", {
      key: MASTERING_ENABLED_PREFERENCE,
      value: enabled ? "1" : "0",
    }).catch((preferenceError) => setError(errorMessage(preferenceError)));
    void invoke("write_app_preference", {
      key: MASTERING_PREFERENCE,
      value: serializeMasteringSettings(settings),
    }).catch((preferenceError) => setError(errorMessage(preferenceError)));
  }, []);

  const rememberFormat = useCallback((format: BounceFormat) => {
    void invoke("write_app_preference", {
      key: BOUNCE_FORMAT_PREFERENCE,
      value: format,
    }).catch((preferenceError) => setError(errorMessage(preferenceError)));
  }, []);

  const bounceMix = useCallback(async () => {
    setBounceOptionsOpen(false);
    rememberMastering(masteringEnabled, mastering);
    rememberFormat(bounceFormat);
    const path = await save({
      filters:
        bounceFormat === "mp3"
          ? [{ name: "MP3 audio", extensions: ["mp3"] }]
          : [{ name: "WAV audio", extensions: ["wav"] }],
      defaultPath: `mix.${bounceFormat}`,
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
      }>("bounce_mix", {
        path,
        format: bounceFormat,
        mastering: masteringEnabled ? mastering : null,
      });
      const trimmed = summary.trimmedSeconds > 0.001
        ? `, ${summary.trimmedSeconds.toFixed(2)} s of leading silence skipped`
        : "";
      setLibraryMessage(
        `Bounced ${formatDuration(summary.durationSeconds * 1000)} to ${summary.path}`
        + ` — ${summary.bitsPerSample > 0
          ? `${summary.bitsPerSample}-bit`
          : "320 kbps"} ${summary.sampleRate / 1000} kHz stereo${trimmed}.`,
      );
    } catch (bounceError) {
      setLibraryMessage(null);
      setError(errorMessage(bounceError));
    } finally {
      setBounceProgress(null);
      setTimelineBusy(false);
    }
  }, [bounceFormat, mastering, masteringEnabled, rememberFormat, rememberMastering]);

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
      publishTransport(
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
    shape: "step" | "sine" | "triangle",
    period: number,
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("draw_timeline_volume_shape", {
        lane, startBeat, endBeat, nodes, shape, period,
      }),
      refreshTimeline,
    );
  }, [refreshTimeline, runTimelineEdit]);

  const drawTimelinePanShape = useCallback(async (
    lane: number,
    startBeat: number,
    endBeat: number,
    nodes: [number, number][],
    shape: "step" | "sine" | "triangle",
    period: number,
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("draw_timeline_pan_shape", {
        lane, startBeat, endBeat, nodes, shape, period,
      }),
      refreshTimeline,
    );
  }, [refreshTimeline, runTimelineEdit]);

  const deleteTimelineDrawGroup = useCallback(async (groupId: number) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("delete_timeline_draw_group", { groupId }),
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
        publishTransport(
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
      publishTransport(await invoke<TimelineTransportSnapshot>(command));
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
      publishTransport(
        await invoke<TimelineTransportSnapshot>("seek_timeline", { positionBeat }),
      );
      if (shouldStart) {
        // `play_timeline` libère la sortie, donc rend le miniplayer muet de
        // lui-même. L'instantané qui suit remet l'interface d'accord avec ça.
        publishTransport(await invoke<TimelineTransportSnapshot>("play_timeline"));
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

  const setPreviewSpeed = useCallback(async (speed: number) => {
    setError(null);
    try {
      setPreview(await invoke<PreviewSnapshot>("set_preview_speed", { speed }));
    } catch (speedError) {
      setError(errorMessage(speedError));
    }
  }, []);

  useEffect(() => {
    if (editingTrackId === null && preview.playbackSpeed !== 1) {
      void setPreviewSpeed(1);
    }
  }, [editingTrackId, preview.playbackSpeed, setPreviewSpeed]);

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

  /**
   * Les boutons d'effet tenus, envoyés tels quels au moteur.
   *
   * Ne passe **pas** par `runTimelineEdit` : rien n'est écrit en base, il n'y a
   * pas d'instantané à empiler dans l'historique, et un aller-retour par appui
   * ferait manquer le temps. C'est un geste joué, pas une édition.
   */
  const setTimelineEffectKeys = useCallback(async (effect: PlayedEffect, keys: number) => {
    try {
      await invoke("set_timeline_effect_keys", { effect, keys });
    } catch (keyError) {
      setError(errorMessage(keyError));
    }
  }, []);

  /**
   * Les pistes sous la gomme, envoyées au moteur pour qu'il les taise.
   *
   * Le balayage ne s'écrit qu'au relâchement — sa longueur n'est connue qu'à la
   * fin — donc le plan en cours porte encore la passe qu'on retire. Sans ce
   * masque, on entendait l'effet continuer sous la gomme.
   */
  const setTimelineEffectErase = useCallback(async (lanes: number) => {
    try {
      await invoke("set_timeline_effect_erase", { lanes });
    } catch (eraseError) {
      setError(errorMessage(eraseError));
    }
  }, []);

  /**
  /**
   * Écrit une passe d'effet au relâchement du bouton.
   *
   * Ceci **est** une édition, contrairement au masque tenu : elle s'écrit en
   * base, elle entre dans l'historique, et elle apparaît sur la timeline.
   */
  const writeTimelineEffectSpan = useCallback(async (
    effect: PlayedEffect,
    lane: number,
    startBeat: number,
    endBeat: number,
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("write_timeline_effect_span", {
        effect,
        lane,
        startBeat,
        endBeat,
      }),
    );
  }, [runTimelineEdit]);

  /**
   * Efface l'automation d'un effet — ou de tous, si aucun n'est nommé.
   *
   * La gomme de l'écran des effets ne nomme rien : elle emporte ce qu'elle
   * balaie, et le fait en une seule édition pour qu'un seul `Ctrl+Z` la défasse.
   */
  const clearTimelineEffectRange = useCallback(async (
    effect: PlayedEffect | null,
    lane: number,
    startBeat: number,
    endBeat: number,
  ) => {
    await runTimelineEdit(
      () => invoke<TimelineSnapshot>("clear_timeline_effect_range", {
        effect,
        lane,
        startBeat,
        endBeat,
      }),
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

    /* Publiée, et non mise dans l'état : la position avance vingt fois par
       seconde, et ce qui la suit s'y est abonné pour l'écrire directement
       dans le DOM. Seul un changement de régime — la lecture qui s'arrête
       d'elle-même au bout du mix — vaut un rendu. */
    const interval = window.setInterval(() => {
      void invoke<TimelineTransportSnapshot>("timeline_transport_snapshot")
        .then((snapshot) => {
          if (snapshot.status === liveTransport.read().status) {
            liveTransport.publish(snapshot);
            return;
          }
          publishTransport(snapshot);
        })
        .catch((transportError) => setError(errorMessage(transportError)));
    }, 50);

    return () => window.clearInterval(interval);
  }, [liveTransport, publishTransport, timelineTransport.status]);

  // Declared before the keyboard effect that reads it: a `const` referenced
  // from a dependency array is evaluated during render, not after it.
  const editingTrack = library.find((track) => track.id === editingTrackId) ?? null;

  const splitTimelineClipAtPlayhead = useCallback(async () => {
    if (editingClipEq || editingTrackId) return;
    // Lue au moment du geste, et non à celui du dernier rendu : c'est là que
    // la tête de lecture est vraiment.
    const playheadBeat = liveTransport.read().positionBeat;
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
  }, [backfillLibraryWaveforms, editingClipEq, editingTrackId, liveTransport, runTimelineEdit]);

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
              snapTimelineBeat(liveTransport.read().positionBeat),
            );
            break;
          case "pan":
            void addTimelinePanNode(
              lane,
              snapTimelineBeat(liveTransport.read().positionBeat),
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
    liveTransport,
    splitTimelineClipAtPlayhead,
    toggleTimelineTransport,
  ]);

  const isPlaying = preview.status === "playing";
  const controlsDisabled = busy || timelinePreparing;
  const timelinePlaybackLocked = timelinePreparing || timelineTransport.status === "playing";
  /**
   * Chaque morceau posé sur la timeline, et son rang dans l'ordre où on
   * l'entend.
   *
   * Un morceau peut avoir plusieurs clips : c'est le **premier** qui compte,
   * puisque c'est là qu'il entre dans le mix. À temps égal, la voie tranche —
   * il faut un ordre stable, sinon la liste se réarrange toute seule entre deux
   * rendus.
   */
  const timelineTrackOrder = useMemo(() => {
    const first = new Map<number, { beat: number; lane: number }>();
    for (const clip of timeline.clips) {
      const current = first.get(clip.libraryTrackId);
      if (
        !current
        || clip.visualStartBeat < current.beat
        || (clip.visualStartBeat === current.beat && clip.lane < current.lane)
      ) {
        first.set(clip.libraryTrackId, { beat: clip.visualStartBeat, lane: clip.lane });
      }
    }
    const ranked = [...first.entries()].sort(
      ([, left], [, right]) => left.beat - right.beat || left.lane - right.lane,
    );
    return new Map(ranked.map(([trackId], rank) => [trackId, rank]));
  }, [timeline.clips]);

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
        publishTransport(
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
            liveTransport={liveTransport}
            livePasses={livePasses}
            onSetEffectKeys={(effect, keys) => void setTimelineEffectKeys(effect, keys)}
            onSetErasing={(lanes) => void setTimelineEffectErase(lanes)}
            onLivePass={trackLivePass}
            onWriteEffectSpan={(effect, lane, startBeat, endBeat) =>
              void writeTimelineEffectSpan(effect, lane, startBeat, endBeat)}
            onEraseEffectSpan={(lane, startBeat, endBeat) =>
              clearTimelineEffectRange(null, lane, startBeat, endBeat)}
            onClearEffectRange={clearTimelineEffectRange}
            onShiftClipDownbeat={(trackId, beats) => void shiftTrackDownbeat(trackId, beats)}
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
            onBounceMix={() => setBounceOptionsOpen(true)}
            onTogglePlayback={toggleTimelineTransport}
            onSeek={seekTimeline}
            onSetClipTempoTarget={setClipTempoTarget}
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
            onDeleteDrawGroup={deleteTimelineDrawGroup}
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

        {/* La colonne de droite porte la bibliothèque **et** le panneau des
            effets, qui se pose dessus. Il tire ainsi sa largeur et sa place de
            la colonne elle-même plutôt que de recalculer celles de l'enveloppe
            — un calcul dupliqué qui tombait déjà à trente-huit pixels à côté. */}
        <div className="library-column">
        <LibraryPanel
          fileDropActive={fileDropActive}
          tracks={library}
          sort={librarySort}
          onSortChange={changeLibrarySort}
          libraryBusy={libraryBusy || gridBusy || timelinePlaybackLocked}
          analysisBusy={analysisBusy || timelinePlaybackLocked}
          timelineAddBusy={libraryBusy || gridBusy || analysisBusy || timelineBusy || timelinePreparing}
          timelineTrackOrder={timelineTrackOrder}
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
            snapTimelineBeat(liveTransport.read().positionBeat),
          )}
          onTimelineDragMove={moveLibraryPointerDrag}
          onTimelineDrop={dropLibraryPointerDrag}
          onTimelineDragCancel={clearLibraryPointerDrag}
          onPreview={(track) => void previewLibraryTrack(track)}
          onTogglePreview={() => void runTransportCommand(isPlaying ? "pause_preview" : "play_preview")}
          onSeekPreview={(positionMs) => void seekPreview(positionMs)}
          onRemove={(track) => void removeLibraryTrack(track)}
          onShiftDownbeat={(track, beats) => void shiftTrackDownbeat(track.id, beats)}
        />
      </div>
      </div>

      {editingTrack && (
        <div className="beatgrid-overlay" role="presentation">
          <BeatgridEditor
            track={editingTrack}
            previewFilePath={preview.filePath}
            previewPositionMs={preview.positionMs}
            previewDurationMs={preview.durationMs}
            previewPlaybackSpeed={preview.playbackSpeed}
            isPreviewPlaying={isPlaying}
            busy={gridBusy || analysisBusy || timelinePlaybackLocked}
            onClose={() => setEditingTrackId(null)}
            onPreview={() => void previewLibraryTrack(editingTrack)}
            onSeekPreview={(positionMs) => void seekPreview(positionMs)}
            onSetPreviewSpeed={(speed) => void setPreviewSpeed(speed)}
            onReanalyze={() => void analyzeLibraryTracks([editingTrack.id])}
            onSave={(bpm, firstBeatMs) =>
              void saveBeatgridCorrection(editingTrack, bpm, firstBeatMs)
            }
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

      {/* Les options du rendu, avant de demander où écrire.

          Le limiteur de mastering est montré à chaque bounce même si son
          réglage est retenu : il applique plusieurs décibels de gain, et un
          gain qu'on a oublié d'avoir armé est la pire des surprises sur un
          master. */}
      {bounceOptionsOpen && (
        <div className="bounce-overlay" role="dialog" aria-modal="true" aria-labelledby="bounce-options-title">
          <div className="bounce-dialog bounce-options">
            <p className="bounce-eyebrow">OFFLINE RENDER</p>
            <h2 id="bounce-options-title">Bounce the mix</h2>
            {/* Le format d'abord : il décide de ce que le reste veut dire.
                Le dither n'a de sens que pour le WAV, et la profondeur non
                plus. */}
            <div className="bounce-formats" role="radiogroup" aria-label="Format">
              {BOUNCE_FORMATS.map((option) => (
                <label
                  key={option.id}
                  className={`bounce-format${bounceFormat === option.id ? " is-picked" : ""}`}
                >
                  <input
                    type="radio"
                    name="bounce-format"
                    checked={bounceFormat === option.id}
                    /* Le plafond suit le format : un codec avec perte
                       redessine les crêtes un peu plus haut, et il lui faut
                       la marge. Pas de réglage de plus — le champ qui existe
                       déjà prend la bonne valeur, et reste modifiable. */
                    onChange={() => {
                      setBounceFormat(option.id);
                      setMastering((current) => ({
                        ...current,
                        ceilingDb: ceilingForFormat(option.id),
                      }));
                    }}
                  />
                  <span>
                    <strong>{option.label}</strong>
                    <em>{option.detail}</em>
                  </span>
                </label>
              ))}
            </div>

            <p className="bounce-note">
              {bounceFormat === "mp3"
                ? "Encoded straight from the mix, with no 16-bit step in between."
                : "Dithered on the way down to 16 bits."}
            </p>

            <label className="bounce-toggle">
              <input
                type="checkbox"
                checked={masteringEnabled}
                onChange={(event) => setMasteringEnabled(event.target.checked)}
              />
              <span>
                <strong>Mastering Limiter</strong>
                <em>
                  Brings the mix up to full level and keeps every peak under the
                  ceiling. Leave it on unless you are mastering the file
                  elsewhere.
                </em>
              </span>
            </label>

            <div className={`bounce-fields${masteringEnabled ? "" : " is-off"}`}>
              <label>
                <span>Threshold</span>
                <input
                  type="number"
                  step={0.1}
                  min={MASTERING_LIMITS.thresholdDb.min}
                  max={MASTERING_LIMITS.thresholdDb.max}
                  value={mastering.thresholdDb}
                  disabled={!masteringEnabled}
                  onChange={(event) =>
                    setMastering((current) => ({
                      ...current,
                      thresholdDb: Number(event.target.value),
                    }))}
                />
                <em>dB</em>
              </label>
              <label>
                <span>Ceiling</span>
                <input
                  type="number"
                  step={0.1}
                  min={MASTERING_LIMITS.ceilingDb.min}
                  max={MASTERING_LIMITS.ceilingDb.max}
                  value={mastering.ceilingDb}
                  disabled={!masteringEnabled}
                  onChange={(event) =>
                    setMastering((current) => ({
                      ...current,
                      ceilingDb: Number(event.target.value),
                    }))}
                />
                <em>dB</em>
              </label>
              <label>
                <span>Release</span>
                <input
                  type="number"
                  step={0.1}
                  min={MASTERING_LIMITS.releaseMs.min}
                  max={MASTERING_LIMITS.releaseMs.max}
                  value={mastering.releaseMs}
                  disabled={!masteringEnabled || mastering.autoRelease}
                  onChange={(event) =>
                    setMastering((current) => ({
                      ...current,
                      releaseMs: Number(event.target.value),
                    }))}
                />
                <em>ms</em>
              </label>
              <label className="bounce-toggle bounce-toggle--inline">
                <input
                  type="checkbox"
                  checked={mastering.autoRelease}
                  disabled={!masteringEnabled}
                  onChange={(event) =>
                    setMastering((current) => ({
                      ...current,
                      autoRelease: event.target.checked,
                    }))}
                />
                <span>Auto release</span>
              </label>
            </div>

            {/* Deux nombres négatifs ne disent pas qu'on demande du gain. */}
            {masteringEnabled && (
              <p className="bounce-note bounce-gain">
                Lifts the mix by <strong>{masteringGainDb(mastering).toFixed(1)} dB</strong>,
                then holds it under {mastering.ceilingDb.toFixed(1)} dB.
              </p>
            )}

            <div className="bounce-actions">
              <button
                className="bounce-btn bounce-btn--wide"
                type="button"
                onClick={() => setBounceOptionsOpen(false)}
              >
                <span>CANCEL</span>
              </button>
              <button
                className="bounce-btn bounce-btn--wide bounce-btn--go"
                type="button"
                onClick={() => void bounceMix()}
              >
                <span>CHOOSE FILE &amp; BOUNCE</span>
              </button>
            </div>
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
