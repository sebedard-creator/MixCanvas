import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { formatDuration } from "../lib/formatDuration";
import { timelineLaneFromPointer } from "../lib/timelineLane";
import {
  resolveTimelinePointerDrop,
  type LibraryPointerDrag,
} from "../lib/timelinePointerDrag";
import { tempoBpmAtBeat, tempoCurveGeometry, tempoSecondsAtBeat } from "../lib/tempoCurve";
import { BEATS_PER_MEASURE, snapTimelineBeat } from "../lib/timelineSnap";
import { isDeleteShortcut, resolveViewShortcut, shouldCaptureTimelineZoom } from "../lib/timelineShortcut";
import {
  FILTER_BUBBLE_DEFAULT_WIDTH_BEATS,
  FILTER_BUBBLE_MAX_WIDTH_BEATS,
  FILTER_BUBBLE_MIN_WIDTH_BEATS,
  filterBubblePoints,
  filterStrokeNodes,
  type FilterBubbleShape,
} from "../lib/filterShape";
import {
  filterResizeLimits,
  resizeFilterCurveEnd,
  resizeFilterCurveStart,
  snapFilterBeat,
} from "../lib/filterResize";
import { ClipWaveform } from "./ClipWaveform";
import { StereoVuMeter } from "./StereoVuMeter";
import { AboutModal } from "./AboutModal";
import { HelpModal } from "./HelpModal";
import { libraryDisplayName } from "../lib/libraryDisplayName";
import { isClipEqActive } from "../lib/clipEq";
import { canBeSidechainKey, clipsCoveredByKey } from "../lib/sidechainKey";
import {
  clipTrimLimits,
  clipWithTrim,
  minimumAnchorBeat,
  trimEdgeAtPointer,
  trimForEdge,
  type ClipTrim,
  type TrimEdge,
} from "../lib/clipTrim";
import { TransportGlyph } from "./TransportGlyph";
import { type SmartTool, smartToolAt, smartToolClass } from "../lib/smartTool";
import {
  SHAPE_KINDS,
  SHAPE_PERIODS,
  panShapeNodes,
  volumeShapeNodes,
  type ShapeKind,
  type ShapePeriod,
} from "../lib/automationShapes";
import {
  DEFAULT_TRACK_GAIN_DB,
  FILTER_LANE_UNITS,
  LANE_PAIR_UNITS,
  VOLUME_FLOOR_DB,
  volumeDbAtBeat,
  panCentreY,
  panLabel,
  panValueAtBeat,
  panNodeValue,
  panNodeY,
  automationUnitsAtPointer,
  gainLabel,
  volumeNodeGainDb,
  volumeNodeY,
} from "../lib/volumeCurve";
import type { LibraryTrack } from "../library/types";
import {
  clampTimelineZoom,
  minimumTimelineZoom,
  timelineSeekBeat,
  timelineContentLayout,
  visibleMeasures,
} from "../lib/timelineZoom";
import type { LiveTransport } from "../lib/liveTransport";
import { nodesAcross, visibleBeatRange } from "../lib/automationWindow";
import type {
  TimelineClip,
  TimelineDrawGroup,
  TimelineSnapshot,
  TimelineTransportSnapshot,
} from "../timeline/types";

/** Si la feuille de style ne répond pas, la valeur qu'elle porte aujourd'hui. */
const CLIP_HEADING_FALLBACK_PX = 18;
const MIN_VISIBLE_BEATS = 64;
const TRAILING_BEATS = 16;
const KEYBOARD_ZOOM_DELTA = 120;

type AutomationView = "pan" | "volume" | "both" | "none";

const AUTOMATION_VIEW_ORDER: AutomationView[] = ["pan", "volume", "both", "none"];

const AUTOMATION_VIEW_LABELS: Record<AutomationView, string> = {
  pan: "pan only",
  volume: "volume only",
  both: "volume and pan",
  none: "hidden",
};

const AUTOMATION_VIEW_GLYPHS = {
  pan: "view-pan",
  volume: "view-volume",
  both: "view-both",
  none: "view-none",
} as const;

const DRAW_GLYPHS = {
  step: "draw-step",
  sine: "draw-sine",
  triangle: "draw-triangle",
} as const;

/** Le cycle des formes passe par l'éteint, qui sert d'état désarmé. */
/**
 * La forme suivante, en boucle sur les trois.
 *
 * Il y avait un quatrième cran, « éteint », qui voulait dire « pas de crayon ».
 * Il n'a plus de sens depuis que la position du pointeur décide de l'outil :
 * pour ne pas dessiner, on remonte dans la barre du clip. `DRAW` ne répond plus
 * qu'à une question — *quoi* — et n'en pose plus une seconde.
 */
function nextDrawShape(current: ShapeKind): ShapeKind {
  const index = SHAPE_KINDS.indexOf(current);
  return SHAPE_KINDS[(index + 1) % SHAPE_KINDS.length];
}

/** Une demi-période se lit en fraction; les entières restent des chiffres. */
function periodLabel(period: ShapePeriod): string {
  return period === 0.5 ? "½" : String(period);
}

function periodTitle(period: ShapePeriod): string {
  if (period < 1) return "Two cycles every beat";
  return `One cycle every ${period} beat${period > 1 ? "s" : ""}`;
}

function nextDrawPeriod(current: ShapePeriod): ShapePeriod {
  const index = SHAPE_PERIODS.indexOf(current);
  return SHAPE_PERIODS[(index + 1) % SHAPE_PERIODS.length];
}

function nextAutomationView(current: AutomationView): AutomationView {
  const index = AUTOMATION_VIEW_ORDER.indexOf(current);
  return AUTOMATION_VIEW_ORDER[(index + 1) % AUTOMATION_VIEW_ORDER.length];
}
const TIMELINE_LANES = [0, 1, 2] as const;
const MAX_ZOOM_DELTA_PER_FRAME = 96;
/** Idle delay after which the viewport follows the playhead again. */
const MANUAL_SCROLL_RELEASE_MS = 2_500;
/**
 * De combien la vue peut glisser avant qu'un rendu React redevienne utile.
 *
 * Déplacer la vue ne demande que trois écritures dans le DOM ; ce qui coûte,
 * c'est reconstruire les marqueurs et les waveforms, et cela ne sert à rien
 * tant qu'on n'a pas découvert de contenu neuf. Un peu moins que la marge de
 * 256 px que `waveformWindow` garde de chaque côté, pour que la tranche déjà
 * construite couvre toujours ce qu'on voit.
 */
const VIEW_RENDER_STEP_PX = 192;
/**
 * How close, on screen, the pointer must be to a filter curve's edge to grab it
 * rather than adjust its depth. A pixel distance keeps the target the same size
 * at every zoom level, where a distance in beats would vanish when zoomed out.
 */
const FILTER_EDGE_GRAB_PX = 8;
/** A Draw can retain thousands of DSP points without asking SVG to paint them. */
const DRAW_VISUAL_POINTS_PER_PIXEL = 0.5;

/**
 * Keeps a Draw's one visual stroke bounded to what the viewport can actually
 * resolve. The saved samples remain untouched; this only chooses which points
 * are useful to send to SVG.
 */
function compactDrawPoints<T>(points: readonly T[], maxPoints: number): T[] {
  if (points.length <= maxPoints) return [...points];
  const result: T[] = [points[0]];
  const stride = (points.length - 1) / (maxPoints - 1);
  for (let index = 1; index < maxPoints - 1; index += 1) {
    result.push(points[Math.round(index * stride)]);
  }
  result.push(points[points.length - 1]);
  return result;
}

interface TimelinePanelProps {
  timeline: TimelineSnapshot;
  /** L'état du transport tel que React le connaît : il ne bouge qu'aux évènements. */
  transport: TimelineTransportSnapshot;
  /** Le même, tel que la lecture le fait avancer : à s'abonner, pas à rendre. */
  liveTransport: LiveTransport;
  busy: boolean;
  preparing: boolean;
  libraryTracks?: LibraryTrack[];
  libraryPointerDrag: LibraryPointerDrag | null;
  onLibraryPointerDragComplete: () => void;
  onAddClip: (trackId: number, anchorBeat: number, lane: number) => Promise<void>;
  onMoveClip: (clipId: number, anchorBeat: number, lane: number) => Promise<void>;
  onTrimClip: (clipId: number, trimStartBeats: number, trimEndBeats: number) => Promise<void>;
  onOpenClipEq?: (clip: TimelineClip) => void;
  onMoveTempoPoint: (clipId: number, tempoAnchorBeat: number) => Promise<void>;
  onRemoveClip: (clipId: number) => Promise<void>;
  onClearTimeline?: () => Promise<void>;
  onClearEverything?: () => Promise<void>;
  onSaveProject: () => Promise<void>;
  onLoadProject: () => Promise<void>;
  onBounceMix: () => Promise<void>;
  onTogglePlayback: () => Promise<void>;
  onSeek: (positionBeat: number) => Promise<void>;
  /** Règle le tempo d'un morceau depuis son nœud sur la règle. */
  onSetTrackTempo: (libraryTrackId: number, bpm: number) => Promise<void>;
  selectedLane: number;
  onSelectLane: (lane: number) => void;
  onSetLaneMuted: (lane: number, isMuted: boolean) => Promise<void>;
  onSetLaneSolo: (lane: number, isSolo: boolean) => Promise<void>;
  onSetLimiterEnabled: (limiterEnabled: boolean) => Promise<void>;
  onSetCompressorEnabled: (compressorEnabled: boolean) => Promise<void>;
  /** Vrai quand un clic dans la timeline doit aussi lancer la lecture. */
  autoplay: boolean;
  onSetAutoplay: (autoplay: boolean) => void;
  onSetSidechainKey: (clipId: number, isKey: boolean) => Promise<void>;
  onSetClipStem: (clipId: number, stem: "full" | "vocals" | "instrumental") => Promise<void>;
  onSeparateStems: (clipId: number, stem: "vocals" | "instrumental") => Promise<void>;
  /** Cuire ce clip, ou défaire la cuisson. */
  onSetClipBaked: (clipId: number, baked: boolean) => Promise<void>;
  onAddVolumeNode: (lane: number, beat: number) => Promise<void>;
  onAddPanNode: (lane: number, beat: number) => Promise<void>;
  onMovePanNode: (nodeId: number, beat: number, value: number) => Promise<void>;
  onDeletePanNode: (nodeId: number) => Promise<void>;
  onDrawVolumeShape: (lane: number, startBeat: number, endBeat: number, nodes: [number, number][], shape: ShapeKind, period: ShapePeriod) => Promise<void>;
  onDrawPanShape: (lane: number, startBeat: number, endBeat: number, nodes: [number, number][], shape: ShapeKind, period: ShapePeriod) => Promise<void>;
  onDrawFilterStroke: (lane: number, nodes: [number, number][]) => Promise<void>;
  onMoveVolumeNode: (nodeId: number, beat: number, gainDb: number | null) => Promise<void>;
  onDeleteVolumeNode: (nodeId: number) => Promise<void>;
  onDeleteDrawGroup: (groupId: number) => Promise<void>;
  onDrawFilterBubble: (
    lane: number,
    startBeat: number,
    widthBeats: number,
    value: number,
    shape?: string,
    replacedRange?: { startBeat: number; endBeat: number },
  ) => Promise<void>;
  onClearFilterRange: (lane: number, startBeat: number, endBeat: number) => Promise<void>;
  onUndo?: () => Promise<void>;
  onRedo?: () => Promise<void>;
  canUndo?: boolean;
  canRedo?: boolean;
}

interface ActiveDrag {
  clipId: number;
  startClientX: number;
  startAnchorBeat: number;
  currentAnchorBeat: number;
  minimumAnchorBeat: number;
  startLane: number;
  currentLane: number;
}

interface ActiveTempoPointDrag {
  clipId: number;
  minimumBeat: number;
  maximumBeat: number;
  originalBeat: number;
  currentBeat: number;
}

interface ClipDraft {
  anchorBeat: number;
  lane: number;
}

interface VolumeContextMenu {
  clientX: number;
  clientY: number;
  lane: number;
  beat: number;
  nodeId: number | null;
  drawGroups: TimelineDrawGroup[];
}

interface VolumeNodeDraft {
  beat: number;
  gainDb: number | null;
}

interface FilterBubbleDraft {
  lane: number;
  startBeat: number;
  widthBeats: number;
  value: number;
  shape?: FilterBubbleShape;
  /**
   * Span of the curve this draft is about to replace. The preview hides it, so
   * shrinking a curve does not show its former tail beside the new shape.
   */
  hiddenRange?: { startBeat: number; endBeat: number };
}

/**
 * What a pointer gesture on the Filter sub-lane is doing.
 * - `create`: drawing a new curve, width and depth follow the drag.
 * - `depth`: inside an existing curve, only its intensity follows the drag.
 * - `resize-start` / `resize-end`: an edge was grabbed, only the length moves.
 */
type FilterGesture = "create" | "depth" | "resize-start" | "resize-end";

interface ActiveFilterBubble extends FilterBubbleDraft {
  pointerId: number;
  gesture: FilterGesture;
  startClientX: number;
  initialClickBeat: number;
  /** Span the gesture replaces, so the rewrite can be atomic. */
  replacedRange: { startBeat: number; endBeat: number } | null;
  /** Edge held still while the opposite one is dragged. */
  anchorBeat: number;
  /** Bounds the resize may not cross, set by the neighbouring curves. */
  limitStartBeat: number;
  limitEndBeat: number;
}

/** A drawn curve, from the bypass sample that opens it to the one that closes it. */
interface FilterBubbleRun {
  startBeat: number;
  endBeat: number;
  peakBeat: number;
  peakValue: number;
}

interface FilterContextMenu {
  clientX: number;
  clientY: number;
  lane: number;
  startBeat: number;
  endBeat: number;
}


function filterNodeY(lane: number, value: number) {
  // Bypass sits at the exact middle of the band, on every lane. These offsets
  // used to be hand-tuned per lane because the three sub-lanes had different
  // heights; they are identical now, so one formula covers all three.
  const bypassY = FILTER_LANE_UNITS / 2;
  return lane * LANE_PAIR_UNITS + bypassY - value * (FILTER_LANE_UNITS / 2 - 2);
}

export function TimelinePanel({
  timeline,
  transport,
  liveTransport,
  busy,
  preparing,
  libraryTracks = [],
  libraryPointerDrag,
  onLibraryPointerDragComplete,
  onAddClip,
  onMoveClip,
  onTrimClip,
  onOpenClipEq,
  onMoveTempoPoint,
  onRemoveClip,
  onClearTimeline,
  onClearEverything,
  onSaveProject,
  onLoadProject,
  onBounceMix,
  onTogglePlayback,
  onSeek,
  onSetTrackTempo,
  selectedLane,
  onSelectLane,
  onSetLaneMuted,
  onSetLaneSolo,
  onSetLimiterEnabled,
  onSetCompressorEnabled,
  autoplay,
  onSetAutoplay,
  onSetSidechainKey,
  onSetClipStem,
  onSeparateStems,
  onSetClipBaked,
  onAddVolumeNode,
  onAddPanNode,
  onMovePanNode,
  onDeletePanNode,
  onDrawVolumeShape,
  onDrawPanShape,
  onDrawFilterStroke,
  onMoveVolumeNode,
  onDeleteVolumeNode,
  onDeleteDrawGroup,
  onDrawFilterBubble,
  onClearFilterRange,
  onUndo,
  onRedo,
  canUndo,
  canRedo,
}: TimelinePanelProps) {
  const [pixelsPerBeat, setPixelsPerBeat] = useState(16);
  const [clipDrafts, setClipDrafts] = useState<Record<number, ClipDraft>>({});
  const [showHelpModal, setShowHelpModal] = useState<boolean>(false);
  const [showAboutModal, setShowAboutModal] = useState<boolean>(false);
  const [tempoPointDrafts, setTempoPointDrafts] = useState<Record<number, number>>({});
  const [volumeDrafts, setVolumeDrafts] = useState<Record<number, VolumeNodeDraft>>({});
  /* Quelles lignes d'automation sont visibles. Le cycle suit l'ordre demandé :
     panoramique, volume, les deux, aucune. */
  const [automationView, setAutomationView] = useState<AutomationView>("both");
  /* Le crayon. `null` désarme : c'est le premier cran du cycle des formes, ce
     qui évite un troisième bouton pour l'allumer. */
  const [drawShape, setDrawShape] = useState<ShapeKind>(SHAPE_KINDS[0]);
  const [drawPeriod, setDrawPeriod] = useState<ShapePeriod>(1);
  const drawStroke = useRef<{ lane: number; startBeat: number } | null>(null);
  /* Le trait en cours. Dans l'état et non dans une ref : la ligne doit se
     redessiner à chaque mouvement, sans quoi l'amplitude se choisit à
     l'aveugle. */
  const [shapePreview, setShapePreview] = useState<
    { lane: number; startBeat: number; endBeat: number; units: number } | null
  >(null);
  const showVolumeAutomation = automationView === "both" || automationView === "volume";
  /* Rien d'affiché, rien à dessiner : la touche s'éteint plutôt que de s'armer
     pour rien. */
  const showPanAutomation = automationView === "both" || automationView === "pan";
  const drawArmable = automationView !== "none";
  const [panDrafts, setPanDrafts] = useState<Record<number, { beat: number; value: number }>>({});
  const [filterBubbleDraft, setFilterBubbleDraft] = useState<FilterBubbleDraft | null>(null);
  /* Le trait de filtre à main levée : la valeur peinte à chaque quart de temps
     parcouru, dans l'ordre où la main les visite. Dans l'état et non dans une
     ref, parce que la courbe doit s'écrire sous le curseur pendant le geste —
     un pinceau dont on ne voit le résultat qu'au relâchement se dessine à
     l'aveugle. */
  const [filterStroke, setFilterStroke] = useState<{
    lane: number;
    painted: Map<number, number>;
  } | null>(null);
  const filterStrokePointer = useRef<number | null>(null);
  /* `Ctrl` change ce que fait la bande : le pinceau à bulle laisse la place au
     tracé libre. Un modificateur qui ne se voit pas se découvre par accident,
     donc le curseur devient un crayon dès qu'il est enfoncé. */
  const [freehandArmed, setFreehandArmed] = useState(false);
  const [volumeContextMenu, setVolumeContextMenu] = useState<VolumeContextMenu | null>(null);
  const [panContextMenu, setPanContextMenu] = useState<VolumeContextMenu | null>(null);
  const [filterContextMenu, setFilterContextMenu] = useState<FilterContextMenu | null>(null);
  const [dropTargetLane, setDropTargetLane] = useState<number | null>(null);
  const [viewportWidth, setViewportWidth] = useState(0);
  const activeDrag = useRef<ActiveDrag | null>(null);
  /* The edge the pointer is hovering, which is what turns the cursor into a
     bracket, and the edge being dragged once one is grabbed. */
  const [hoveredTrim, setHoveredTrim] = useState<{ clipId: number; edge: TrimEdge } | null>(null);
  /**
   * L'outil que la position du pointeur propose, et le clip qu'il vise.
   *
   * Calculé au survol et non à l'appui : c'est ce qui permet au curseur
   * d'annoncer le geste **avant** qu'on s'engage. Le rognage se contentait de
   * l'appui parce qu'il avait ses propres classes; le crayon a besoin de la
   * même avance.
   */
  const [hoveredTool, setHoveredTool] = useState<{ clipId: number; tool: SmartTool } | null>(null);
  /* Le nœud de tempo qu'on est en train de régler au clavier, s'il y en a un. */
  const [tempoEdit, setTempoEdit] = useState<
    { clipId: number; libraryTrackId: number; fileName: string; x: number; value: string } | null
  >(null);
  /* Lu par l'écouteur de `Delete`, qui ne doit pas se reposer à chaque édition
     en cours. */
  const busyRef = useRef(busy);
  busyRef.current = busy;

  /**
   * Où finit la barre de titre de **ce** clip, mesurée sur elle.
   *
   * Pas une constante recopiée : la feuille de style décide déjà de cette
   * hauteur, et l'écrire une seconde fois ici ferait diverger la frontière du
   * curseur de celle qu'on voit. `offsetHeight` lit la ligne réellement
   * dessinée, et ne coûte pas de recalcul de style — contrairement à
   * `getComputedStyle`, qu'on ne veut pas appeler à chaque déplacement du
   * pointeur.
   */
  const headingHeightOf = (clipElement: Element) => {
    const heading = clipElement.querySelector(".clip-heading");
    const measured = heading instanceof HTMLElement ? heading.offsetHeight : 0;
    return measured > 0 ? measured : CLIP_HEADING_FALLBACK_PX;
  };
  const [trimDrafts, setTrimDrafts] = useState<Record<number, ClipTrim>>({});
  const activeTrim = useRef<{ clipId: number; edge: TrimEdge; trim: ClipTrim } | null>(null);
  const activeTempoPointDrag = useRef<ActiveTempoPointDrag | null>(null);
  const activeVolumeNode = useRef<number | null>(null);
  const activePanNode = useRef<number | null>(null);
  const activeFilterBubble = useRef<ActiveFilterBubble | null>(null);
  const timelineScroll = useRef<HTMLDivElement | null>(null);
  const timelineTracks = useRef<HTMLDivElement | null>(null);
  const pendingZoomDelta = useRef(0);
  const zoomAnimationFrame = useRef<number | null>(null);
  const zoomState = useRef({
    pixelsPerBeat: 16,
    minimumZoom: 1,
  });
  const tempoEditingLocked = busy || transport.status === "playing";

  /* Le crayon a toujours besoin d'une ligne visible — mais il n'y a plus rien à
     désarmer. `drawArmable` suffit : sans ligne affichée, le corps d'un clip
     redevient une prise pour le déplacer, et le curseur ne propose jamais un
     geste qui n'écrirait nulle part. La forme choisie, elle, survit à l'aller-
     retour par `VIEW`. */

  useEffect(() => {
    setClipDrafts({});
    setTempoPointDrafts({});
    setVolumeDrafts({});
    setPanDrafts({});
    setTrimDrafts({});
    setFilterBubbleDraft(null);
  }, [timeline.clips, timeline.tempoPoints, timeline.volumeNodes, timeline.filterNodes]);

  useEffect(() => {
    if (!volumeContextMenu && !panContextMenu && !filterContextMenu) return undefined;
    const closeMenus = () => {
      setVolumeContextMenu(null);
      setPanContextMenu(null);
      setFilterContextMenu(null);
    };
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!(event.target as HTMLElement | null)?.closest(".timeline-context-menu")) {
        closeMenus();
      }
    };
    window.addEventListener("pointerdown", closeOnOutsidePointer);
    window.addEventListener("blur", closeMenus);
    return () => {
      window.removeEventListener("pointerdown", closeOnOutsidePointer);
      window.removeEventListener("blur", closeMenus);
    };
  }, [filterContextMenu, panContextMenu, volumeContextMenu]);

  useEffect(() => {
    const element = timelineScroll.current;
    if (!element) {
      return undefined;
    }
    const updateWidth = () => setViewportWidth(element.clientWidth);
    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const projectEndBeat = useMemo(
    () => timeline.clips.reduce(
      (maximum, clip) => Math.max(maximum, clip.visualEndBeat),
      0,
    ),
    [timeline.clips],
  );
  const sortedClips = useMemo(() => {
    return [...timeline.clips].sort((a, b) => {
      const aBeat = clipDrafts[a.id]?.anchorBeat ?? a.anchorBeat;
      const bBeat = clipDrafts[b.id]?.anchorBeat ?? b.anchorBeat;
      return aBeat - bBeat || a.id - b.id;
    });
  }, [timeline.clips, clipDrafts]);
  const totalBeats = Math.max(MIN_VISIBLE_BEATS, Math.ceil(projectEndBeat + TRAILING_BEATS));
  const minimumZoom = minimumTimelineZoom(viewportWidth, totalBeats);
  const contentWidth = totalBeats * pixelsPerBeat;
  const displayTempoPoints = useMemo(() => {
    const points = [
      { beat: 0, bpm: timeline.projectBpm, clipId: null },
      ...timeline.tempoPoints.filter((point) => point.clipId !== null),
    ]
      .map((point) => ({
        ...point,
        beat: point.clipId === null
          ? point.beat
          : (tempoPointDrafts[point.clipId] ?? point.beat),
      }))
      .sort((left, right) => (
        left.beat - right.beat
        || (left.clipId ?? Number.MIN_SAFE_INTEGER) - (right.clipId ?? Number.MIN_SAFE_INTEGER)
      ));
    return points.reduce<typeof points>((deduplicated, point) => {
      const previous = deduplicated[deduplicated.length - 1];
      if (previous && Math.abs(previous.beat - point.beat) < 0.000_001) {
        deduplicated[deduplicated.length - 1] = point;
      } else {
        deduplicated.push(point);
      }
      return deduplicated;
    }, []);
  }, [tempoPointDrafts, timeline.projectBpm, timeline.tempoPoints]);
  const tempoCurve = useMemo(
    () => tempoCurveGeometry(displayTempoPoints, pixelsPerBeat, contentWidth),
    [contentWidth, displayTempoPoints, pixelsPerBeat],
  );
  /* Le BPM sous la tête de lecture, tenu par abonnement plutôt que par rendu :
     il ne change qu'en franchissant un nœud de tempo, et re-rendre le panneau
     vingt fois par seconde pour deux décimales qui restent les mêmes serait le
     plus mauvais marché de l'interface. */
  const displayTempoPointsRef = useRef(displayTempoPoints);
  displayTempoPointsRef.current = displayTempoPoints;
  /* A tempo ramp changes the readout on every transport poll. Updating this
     one text node directly keeps the rest of the timeline outside React's
     render cycle while playback is running. */
  const bpmDisplay = useRef<HTMLSpanElement | null>(null);
  const writeCurrentBpm = useCallback((bpm: number) => {
    const text = bpm.toFixed(2);
    if (bpmDisplay.current && bpmDisplay.current.textContent !== text) {
      bpmDisplay.current.textContent = text;
    }
  }, []);
  const currentBpmText = tempoBpmAtBeat(displayTempoPoints, transport.positionBeat).toFixed(2);
  const totalTimeMs = Math.round(tempoSecondsAtBeat(displayTempoPoints, projectEndBeat) * 1_000);
  zoomState.current = {
    pixelsPerBeat,
    minimumZoom,
  };

  useLayoutEffect(() => {
    setPixelsPerBeat((current) => Math.max(current, minimumZoom));
  }, [minimumZoom]);

  /**
   * Un cran publie une seule géométrie React.
   *
   * Largeur musicale, placement, grille, clips, courbes et playhead dérivent
   * tous du même `pixelsPerBeat`. Il n'existe donc aucune phase d'aperçu
   * `scaleX`, ni de calque GPU persistant que WebView2 pourrait recomposer avec
   * la texture de l'image précédente. Le regroupement par animation frame
   * absorbe encore les micro-événements d'une rafale de molette.
   */
  const applyTimelineZoom = useCallback((deltaPixels: number) => {
    const boundedDelta = Math.max(-240, Math.min(240, deltaPixels));
    const state = zoomState.current;
    const nextZoom = clampTimelineZoom(
      state.pixelsPerBeat * Math.exp(-boundedDelta * 0.0015),
      state.minimumZoom,
    );
    if (Math.abs(nextZoom - state.pixelsPerBeat) < 0.000_01) return;
    zoomState.current.pixelsPerBeat = nextZoom;
    setPixelsPerBeat(nextZoom);
  }, []);

  const queueTimelineZoom = useCallback((deltaPixels: number) => {
    pendingZoomDelta.current = Math.max(
      -MAX_ZOOM_DELTA_PER_FRAME,
      Math.min(MAX_ZOOM_DELTA_PER_FRAME, pendingZoomDelta.current + deltaPixels),
    );
    if (zoomAnimationFrame.current !== null) return;
    zoomAnimationFrame.current = window.requestAnimationFrame(() => {
      const delta = pendingZoomDelta.current;
      pendingZoomDelta.current = 0;
      zoomAnimationFrame.current = null;
      applyTimelineZoom(delta);
    });
  }, [applyTimelineZoom]);

  const timelineBody = useRef<HTMLDivElement | null>(null);
  const scrollbarThumb = useRef<HTMLDivElement | null>(null);
  const scrollReleaseTimer = useRef<number | null>(null);

  /**
   * Où la vue regarde — deux valeurs, parce qu'elles ne coûtent pas la même chose.
   *
   * `viewBeat` avance vingt fois par seconde et n'est écrite que dans le DOM :
   * trois propriétés, aucun rendu. `renderedBeat` est celle du dernier rendu
   * React ; elle ne bouge que lorsque la vue a assez glissé pour qu'il y ait de
   * nouveaux marqueurs et de nouvelles tranches de waveform à construire. Tout
   * ce qui coûte cher dépend de la seconde, et elle change rarement.
   */
  const livePositionBeat = useRef(transport.positionBeat);
  const manualScrollBeat = useRef<number | null>(null);
  const viewBeat = useRef(transport.positionBeat);
  const [renderedBeat, setRenderedBeat] = useState(transport.positionBeat);
  const renderedBeatRef = useRef(renderedBeat);

  /* Ce dont le tracé a besoin, réécrit à chaque rendu pour que `paintView`
     reste une fonction stable qui voit toujours le zoom courant. */
  const viewGeometry = useRef({
    pixelsPerBeat,
    contentWidth,
    viewportWidth,
    totalBeats,
    thumbWidthPercent: 100,
  });

  /**
   * Place la vue sans passer par un rendu.
   *
   * Deux écritures — le défilement natif du viewport et le curseur de la barre
   * de navigation — là où l'application re-rendait entièrement.
   *
   * Le contenu ne se translate plus : en rendu logiciel, cette transformation
   * forçait Chromium à repeindre son sous-arbre complet, waveforms comprises.
   * La marge virtuelle est statique; seul `scrollLeft` avance à l'intérieur.
   * Un rendu n'est redemandé qu'au franchissement de `VIEW_RENDER_STEP_PX`.
   */
  const paintView = useCallback(() => {
    const geometry = viewGeometry.current;
    const position = livePositionBeat.current;
    const displayed = manualScrollBeat.current ?? position;
    viewBeat.current = displayed;

    const scrollElement = timelineScroll.current;
    if (scrollElement) {
      /* La zone musicale reçoit une demi-fenêtre vide à chaque extrémité.
         Le beat `n` est donc exactement au scrollLeft `n × pixelsPerBeat`;
         cette écriture ne lit pas la mise en page et ne force pas de reflow. */
      const targetScroll = geometry.contentWidth > geometry.viewportWidth
        ? Math.max(0, Math.min(geometry.contentWidth, displayed * geometry.pixelsPerBeat))
        : 0;
      scrollElement.scrollLeft = targetScroll;
    }
    if (scrollbarThumb.current) {
      const ratio = geometry.totalBeats > 0
        ? Math.max(0, Math.min(1, displayed / geometry.totalBeats))
        : 0;
      scrollbarThumb.current.style.left = `${ratio * (100 - geometry.thumbWidthPercent)}%`;
    }

    const driftPx = Math.abs(displayed - renderedBeatRef.current) * geometry.pixelsPerBeat;
    if (driftPx >= VIEW_RENDER_STEP_PX) {
      renderedBeatRef.current = displayed;
      setRenderedBeat(displayed);
    }
  }, []);

  /* Le transport avance : on repeint, on ne re-rend pas. */
  useEffect(() => {
    return liveTransport.subscribe((snapshot) => {
      livePositionBeat.current = snapshot.positionBeat;
      paintView();
      writeCurrentBpm(tempoBpmAtBeat(displayTempoPointsRef.current, snapshot.positionBeat));
    });
  }, [liveTransport, paintView, writeCurrentBpm]);

  const clearScrollReleaseTimer = () => {
    if (scrollReleaseTimer.current !== null) {
      window.clearTimeout(scrollReleaseTimer.current);
      scrollReleaseTimer.current = null;
    }
  };

  /**
   * Hands the viewport back to the playhead a moment after the user stops
   * scrolling. This used to be tied to `transport.positionBeat`, which the
   * transport poll refreshes every 50 ms, so a manual scroll was wiped before
   * it could be seen during playback.
   */
  const holdManualScroll = useCallback(() => {
    clearScrollReleaseTimer();
    scrollReleaseTimer.current = window.setTimeout(() => {
      scrollReleaseTimer.current = null;
      manualScrollBeat.current = null;
      paintView();
    }, MANUAL_SCROLL_RELEASE_MS);
  }, [paintView]);

  /** An explicit navigation wins over a manual scroll immediately. */
  const releaseManualScroll = useCallback(() => {
    clearScrollReleaseTimer();
    manualScrollBeat.current = null;
    paintView();
  }, [paintView]);

  useEffect(() => releaseManualScroll(), [transport.status, releaseManualScroll]);
  useEffect(() => clearScrollReleaseTimer, []);

  /* Ce que la fenêtre montre du contenu, en pixels de contenu — à la position
     du **dernier rendu**, et non à celle de l'image en cours. Les waveforms se
     construisent à partir de là et se donnent leur propre marge : les
     reconstruire à chaque image reviendrait à re-rendre le panneau vingt fois
     par seconde, ce que ce découpage existe précisément pour éviter. */
  const renderedLayout = timelineContentLayout(
    renderedBeat,
    pixelsPerBeat,
    contentWidth,
    viewportWidth,
  );
  const visibleContentFromPx = -(renderedLayout.paddingPx + renderedLayout.offsetPx);

  /*
   * The audio engine owns every clip; React only needs the clips that can be
   * seen or reached before the next cheap viewport refresh.  Keeping a large
   * session's off-screen headings, buttons and SVG containers in the DOM made
   * style/layout scale with the whole mix even after waveform virtualization.
   */
  const renderedClips = useMemo(() => {
    if (contentWidth <= viewportWidth || pixelsPerBeat <= 0) return timeline.clips;
    const marginPx = viewportWidth + 2 * VIEW_RENDER_STEP_PX;
    const fromBeat = Math.max(0, (visibleContentFromPx - marginPx) / pixelsPerBeat);
    const toBeat = (visibleContentFromPx + viewportWidth + marginPx) / pixelsPerBeat;
    return timeline.clips.filter((clip) => {
      const draft = clipDrafts[clip.id];
      const trimmed = clipWithTrim(clip, trimDrafts[clip.id]);
      const offset = draft ? draft.anchorBeat - clip.anchorBeat : 0;
      return trimmed.visualEndBeat + offset >= fromBeat && trimmed.visualStartBeat + offset <= toBeat;
    });
  }, [
    clipDrafts,
    contentWidth,
    pixelsPerBeat,
    timeline.clips,
    trimDrafts,
    visibleContentFromPx,
    viewportWidth,
  ]);

  const clipSequenceNumbers = useMemo(
    () => new Map(sortedClips.map((clip, index) => [clip.id, index + 1])),
    [sortedClips],
  );
  const libraryTracksById = useMemo(
    () => new Map(libraryTracks.map((track) => [track.id, track])),
    [libraryTracks],
  );

  /* Un marqueur par mesure **visible**, pas par mesure du projet : sur un long
     mix, des milliers de marqueurs hors champ étaient mis en page à chaque
     rendu — le gros du coût qui faisait strober le zoom. La fenêtre est élargie
     d'un pas de chaque côté pour couvrir ce que la vue peut parcourir avant le
     prochain rendu. */
  const measures = useMemo(() => {
    const labelStride = Math.max(1, Math.ceil(48 / (pixelsPerBeat * 4)));
    return visibleMeasures(
      renderedBeat,
      pixelsPerBeat,
      viewportWidth + 2 * VIEW_RENDER_STEP_PX,
      totalBeats,
      labelStride,
      contentWidth,
    );
  }, [contentWidth, pixelsPerBeat, renderedBeat, totalBeats, viewportWidth]);

  const visibleBeats = pixelsPerBeat > 0 && viewportWidth > 0 ? viewportWidth / pixelsPerBeat : MIN_VISIBLE_BEATS;
  const thumbWidthRatio = Math.max(0.08, Math.min(1, visibleBeats / totalBeats));
  const thumbWidthPercent = thumbWidthRatio * 100;

  /* La géométrie que `paintView` lira à la prochaine image, et le placement
     initial écrit dans le rendu lui-même — pour que le premier affichage et
     tout changement de zoom soient justes avant même le premier repeint. */
  viewGeometry.current = {
    pixelsPerBeat,
    contentWidth,
    viewportWidth,
    totalBeats,
    thumbWidthPercent,
  };
  const contentNeedsNativeScroll = contentWidth > viewportWidth;
  const currentScrollRatio = totalBeats > 0 ? Math.max(0, Math.min(1, viewBeat.current / totalBeats)) : 0;
  const thumbLeftPercent = currentScrollRatio * (100 - thumbWidthPercent);

  /* Un zoom change la géométrie sans changer la position : le rendu vient de
     réécrire les styles à partir de valeurs de rendu, on les remet aussitôt sur
     la position réelle. Sous `useLayoutEffect`, donc avant que l'écran ne
     montre quoi que ce soit — aucun scintillement possible. */
  useLayoutEffect(() => {
    livePositionBeat.current = liveTransport.read().positionBeat;
    paintView();
    writeCurrentBpm(tempoBpmAtBeat(displayTempoPointsRef.current, livePositionBeat.current));
  }, [contentWidth, liveTransport, paintView, pixelsPerBeat, viewportWidth, writeCurrentBpm]);

  const handleScrollbarPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    const track = event.currentTarget;
    const rect = track.getBoundingClientRect();
    releaseManualScroll();
    const updateScrollFromPointer = (clientX: number) => {
      const clickX = clientX - rect.left;
      const ratio = Math.max(0, Math.min(1, clickX / rect.width));
      const targetBeat = ratio * totalBeats;
      void onSeek(Math.max(0, Math.min(totalBeats, targetBeat)));
    };

    updateScrollFromPointer(event.clientX);

    const handlePointerMove = (moveEvent: PointerEvent) => {
      updateScrollFromPointer(moveEvent.clientX);
    };

    const handlePointerUp = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
  };

  useEffect(() => () => {
    if (zoomAnimationFrame.current !== null) {
      window.cancelAnimationFrame(zoomAnimationFrame.current);
    }
  }, []);

  useEffect(() => {
    const element = timelineBody.current ?? timelineScroll.current;
    if (!element) return undefined;

    const handleWheel = (event: WheelEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("select, input, textarea")) return;

      event.preventDefault();
      event.stopPropagation();

      const isHorizontalScrollGesture = event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY);
      const rawDelta = isHorizontalScrollGesture
        ? (Math.abs(event.deltaX) > 0 ? event.deltaX : event.deltaY)
        : event.deltaY;

      const deltaPixels = event.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? rawDelta * 16
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? rawDelta * Math.max(1, element.clientWidth)
          : rawDelta;

      if (isHorizontalScrollGesture) {
        // Pro Tools Standard: Shift + Wheel -> Horizontal Timeline Viewport Scroll
        const maxScrollBeat = Math.max(128, totalBeats + 64);
        /* Écrit puis peint, sans passer par un rendu : un geste de molette
           produit des dizaines d'évènements, et chacun re-rendait le panneau
           entier. C'est ce qui rendait le défilement horizontal poisseux. */
        const startBeat = manualScrollBeat.current ?? livePositionBeat.current;
        const beatsToMove = (deltaPixels / Math.max(1, zoomState.current.pixelsPerBeat)) * 1.5;
        manualScrollBeat.current = Math.max(0, Math.min(maxScrollBeat, startBeat + beatsToMove));
        paintView();
        holdManualScroll();
      } else {
        // Molette seule (sans Shift) -> Zoom avant / arrière
        queueTimelineZoom(deltaPixels);
      }
    };

    element.addEventListener("wheel", handleWheel, { passive: false });
    return () => element.removeEventListener("wheel", handleWheel);
  }, [holdManualScroll, paintView, totalBeats, queueTimelineZoom]);

  /* L'état de `Ctrl`, suivi au clavier parce que le CSS ne connaît pas les
     modificateurs. Le `blur` remet à zéro : sortir de la fenêtre en le tenant
     laisserait un crayon armé qu'aucune touche ne relâche. */
  useEffect(() => {
    const update = (event: KeyboardEvent) => setFreehandArmed(event.ctrlKey);
    const clear = () => setFreehandArmed(false);
    window.addEventListener("keydown", update);
    window.addEventListener("keyup", update);
    window.addEventListener("blur", clear);
    return () => {
      window.removeEventListener("keydown", update);
      window.removeEventListener("keyup", update);
      window.removeEventListener("blur", clear);
    };
  }, []);

  useEffect(() => {
    const handleTimelineZoomShortcut = (event: KeyboardEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (
        event.ctrlKey ||
        event.metaKey ||
        event.altKey ||
        !shouldCaptureTimelineZoom(event.code, target?.tagName, target?.isContentEditable)
      ) {
        return;
      }
      event.preventDefault();
      queueTimelineZoom(event.code === "KeyR" ? KEYBOARD_ZOOM_DELTA : -KEYBOARD_ZOOM_DELTA);
    };
    window.addEventListener("keydown", handleTimelineZoomShortcut);
    return () => window.removeEventListener("keydown", handleTimelineZoomShortcut);
  }, [queueTimelineZoom]);

  /* Les trois touches du rail `VIEW` au clavier. Elles ne font rien de plus que
     le clic : la même règle décide, de sorte qu'une commande grisée reste
     grisée sous la main. Une frappe qui armerait le crayon alors que le bouton
     refuse de le faire donnerait deux vérités pour un même état. */
  useEffect(() => {
    const handleViewShortcut = (event: KeyboardEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      const shortcut = resolveViewShortcut(
        event.key,
        { shift: event.shiftKey, ctrl: event.ctrlKey, alt: event.altKey, meta: event.metaKey },
        target?.tagName,
        target?.isContentEditable,
      );
      if (!shortcut) return;
      event.preventDefault();
      if (event.repeat) return;
      switch (shortcut) {
        case "view":
          setAutomationView((current) => nextAutomationView(current));
          break;
        case "shape":
          if (drawArmable) setDrawShape((current) => nextDrawShape(current));
          break;
        case "period":
          // La période n'a de sens qu'une fois une forme choisie, comme sa
          // moitié de touche qui reste éteinte tant que le crayon dort.
          if (drawArmable) setDrawPeriod((current) => nextDrawPeriod(current));
          break;
      }
    };
    window.addEventListener("keydown", handleViewShortcut);
    return () => window.removeEventListener("keydown", handleViewShortcut);
  }, [drawArmable, drawShape]);

  /**
   * `Delete` retire le clip sous la souris.
   *
   * C'est la souris qui désigne, pas la sélection : on regarde déjà le clip
   * dont on veut se débarrasser, et la croix de sa barre demande de viser
   * quatorze pixels. Le survol est lu par une référence — l'écouteur se pose
   * une fois, plutôt que de se démonter à chaque clip traversé.
   */
  const hoveredClipRef = useRef<number | null>(null);
  hoveredClipRef.current = hoveredTool?.clipId ?? null;

  useEffect(() => {
    const handleDelete = (event: KeyboardEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (
        !isDeleteShortcut(
          event.key,
          { shift: event.shiftKey, ctrl: event.ctrlKey, alt: event.altKey, meta: event.metaKey },
          target?.tagName,
          target?.isContentEditable,
        )
      ) {
        return;
      }
      const clipId = hoveredClipRef.current;
      if (clipId === null || busyRef.current) return;
      event.preventDefault();
      if (event.repeat) return;
      void onRemoveClip(clipId);
    };
    window.addEventListener("keydown", handleDelete);
    return () => window.removeEventListener("keydown", handleDelete);
  }, [onRemoveClip]);

  useEffect(() => {
    if (!libraryPointerDrag) {
      setDropTargetLane(null);
      return;
    }
    const tracksElement = timelineTracks.current;
    const scrollElement = timelineScroll.current;
    if (!tracksElement || !scrollElement) {
      setDropTargetLane(null);
      if (libraryPointerDrag.phase === "dropped") {
        onLibraryPointerDragComplete();
      }
      return;
    }

    const tracksBounds = tracksElement.getBoundingClientRect();
    const viewportBounds = scrollElement.getBoundingClientRect();
    const target = resolveTimelinePointerDrop(
      libraryPointerDrag.clientX,
      libraryPointerDrag.clientY,
      {
        contentLeft: tracksBounds.left,
        viewportLeft: viewportBounds.left,
        viewportRight: viewportBounds.right,
        top: tracksBounds.top,
        height: tracksBounds.height,
      },
      pixelsPerBeat,
      TIMELINE_LANES.length,
    );
    setDropTargetLane(target?.lane ?? null);

    if (libraryPointerDrag.phase === "dropped") {
      if (target && !busy) {
        void onAddClip(libraryPointerDrag.trackId, target.anchorBeat, target.lane);
      }
      onLibraryPointerDragComplete();
    }
  }, [
    busy,
    libraryPointerDrag,
    onAddClip,
    onLibraryPointerDragComplete,
    pixelsPerBeat,
  ]);

  const handleTimelineSeek = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (busy || (event.target as HTMLElement).closest(".timeline-clip, button")) {
      return;
    }
    const bounds = event.currentTarget.getBoundingClientRect();
    const requestedBeat = timelineSeekBeat(
      event.clientX,
      bounds.left,
      pixelsPerBeat,
    );
    releaseManualScroll();
    void onSeek(Math.max(0, Math.min(projectEndBeat, requestedBeat)));
  };

  const startClipDrag = (event: ReactPointerEvent<HTMLDivElement>, clip: TimelineClip) => {
    // Le crayon armé change de mode : on dessine par-dessus les clips, on ne
    // les déplace plus. Sans cette garde, tout trait commencé sur un clip
    // partait en déplacement.
    if (busy || event.button !== 0 || (event.target as HTMLElement).closest("button")) {
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    activeDrag.current = {
      clipId: clip.id,
      startClientX: event.clientX,
      startAnchorBeat: clipDrafts[clip.id]?.anchorBeat ?? clip.anchorBeat,
      currentAnchorBeat: clipDrafts[clip.id]?.anchorBeat ?? clip.anchorBeat,
      minimumAnchorBeat: minimumAnchorBeat(clip.preRollBeats, clip.trimStartBeats),
      startLane: clipDrafts[clip.id]?.lane ?? clip.lane,
      currentLane: clipDrafts[clip.id]?.lane ?? clip.lane,
    };
  };

  /** The beat under the pointer, in timeline coordinates. */
  const pointerBeat = (clientX: number) => {
    const bounds = timelineTracks.current?.getBoundingClientRect();
    if (!bounds) return null;
    return (clientX - bounds.left) / pixelsPerBeat;
  };

  /**
   * Ce qu'un appui ici déclencherait.
   *
   * Une même lecture sert au curseur et au geste : les calculer séparément
   * laisserait le premier annoncer ce que le second ne fait pas — le défaut
   * qui revient le plus souvent dans ce projet.
   *
   * Les commandes d'un clip n'entrent pas dans le jeu. Un appui né sur `EQ`,
   * `VOX`, la chaîne, `BAKE` ou la croix reste un clic; le trait le capturait
   * autrement et ces boutons semblaient morts.
   */
  const toolAtPointer = (
    event: ReactPointerEvent<HTMLDivElement>,
    clip: TimelineClip,
  ): SmartTool | null => {
    // Rien sur le bouton ici. Un `pointermove` de survol porte `button === -1`
    // — aucun bouton n'a changé d'état —, si bien qu'exiger le bouton gauche
    // faisait sortir la fonction avant tout calcul : le curseur ne changeait
    // jamais au survol, seulement le geste à l'appui. Quel bouton est pressé
    // regarde l'appui, pas la position.
    if (busy) return null;
    if (event.target instanceof Element && event.target.closest(".clip-heading button, .clip-heading-actions")) {
      return null;
    }
    const beat = pointerBeat(event.clientX);
    if (beat === null) return null;
    return smartToolAt(clipWithTrim(clip, trimDrafts[clip.id]), {
      beat,
      offsetY: event.clientY - event.currentTarget.getBoundingClientRect().top,
      headingHeight: headingHeightOf(event.currentTarget),
      pixelsPerBeat,
      canDraw: drawArmable,
    });
  };

  const updateSmartCursor = (event: ReactPointerEvent<HTMLDivElement>, clip: TimelineClip) => {
    if (activeTrim.current || activeDrag.current || busy) return;
    const tool = toolAtPointer(event, clip);
    /* Rien n'est écrit tant que rien ne change.
       Le pointeur bouge plus de cent fois par seconde; l'outil, lui, ne change
       qu'en franchissant une frontière. Poser un objet **neuf** à chaque
       mouvement empêchait React de s'arrêter là : le panneau entier était
       re-rendu à la fréquence de la souris, et les trois jeux de courbes
       d'automation étaient reconstruits d'un bout à l'autre du mix à chaque
       fois. Mesuré, c'était quarante-trois pour cent du fil principal. */
    setHoveredTool((current) => {
      if (!tool) return current === null ? current : null;
      return current && current.clipId === clip.id && current.tool === tool
        ? current
        : { clipId: clip.id, tool };
    });
    // Le bord armé s'allume, ce qui est une seconde information : le curseur
    // dit l'outil, la surbrillance dit **quel** bord partira.
    const edge: TrimEdge | null = tool === "trim-start"
      ? "start"
      : tool === "trim-end"
        ? "end"
        : null;
    setHoveredTrim((current) => {
      if (!edge) return current === null ? current : null;
      return current && current.clipId === clip.id && current.edge === edge
        ? current
        : { clipId: clip.id, edge };
    });
  };

  const startClipTrim = (event: ReactPointerEvent<HTMLDivElement>, clip: TimelineClip) => {
    if (busy || event.button !== 0) return false;
    const beat = pointerBeat(event.clientX);
    if (beat === null) return false;
    const edge = trimEdgeAtPointer(clipWithTrim(clip, trimDrafts[clip.id]), beat, pixelsPerBeat);
    if (!edge) return false;
    event.currentTarget.setPointerCapture(event.pointerId);
    const trim = { trimStartBeats: clip.trimStartBeats, trimEndBeats: clip.trimEndBeats };
    activeTrim.current = { clipId: clip.id, edge, trim };
    setHoveredTrim({ clipId: clip.id, edge });
    return true;
  };

  const moveClipTrim = (event: ReactPointerEvent<HTMLDivElement>, clip: TimelineClip) => {
    const drag = activeTrim.current;
    if (!drag || drag.clipId !== clip.id) return false;
    const beat = pointerBeat(event.clientX);
    if (beat === null) return true;
    const limits = clipTrimLimits(timeline.clips, clip);
    const trim = trimForEdge(clip, drag.edge, beat, limits);
    drag.trim = trim;
    setTrimDrafts((current) => ({ ...current, [clip.id]: trim }));
    return true;
  };

  const finishClipTrim = (clip: TimelineClip) => {
    const drag = activeTrim.current;
    activeTrim.current = null;
    if (!drag || drag.clipId !== clip.id) return false;
    const { trimStartBeats, trimEndBeats } = drag.trim;
    if (trimStartBeats !== clip.trimStartBeats || trimEndBeats !== clip.trimEndBeats) {
      void onTrimClip(clip.id, trimStartBeats, trimEndBeats);
    }
    return true;
  };

  const moveClipDraft = (event: ReactPointerEvent<HTMLDivElement>, clipId: number) => {
    const drag = activeDrag.current;
    if (!drag || drag.clipId !== clipId) {
      return;
    }
    const beatDelta = (event.clientX - drag.startClientX) / pixelsPerBeat;
    const anchorBeat = snapTimelineBeat(
      drag.startAnchorBeat + beatDelta,
      drag.minimumAnchorBeat,
    );
    const tracksElement = timelineTracks.current;
    let lane = drag.startLane;
    if (tracksElement) {
      const bounds = tracksElement.getBoundingClientRect();
      lane = timelineLaneFromPointer(
        event.clientY,
        bounds.top,
        bounds.height,
        TIMELINE_LANES.length,
      );
    }
    drag.currentAnchorBeat = anchorBeat;
    drag.currentLane = lane;
    setClipDrafts((current) => ({ ...current, [clipId]: { anchorBeat, lane } }));
  };

  const finishClipDrag = (clip: TimelineClip) => {
    const drag = activeDrag.current;
    activeDrag.current = null;
    if (!drag || drag.clipId !== clip.id) {
      return;
    }
    const anchorBeat = drag.currentAnchorBeat;
    const lane = drag.currentLane;
    if (anchorBeat !== clip.anchorBeat || lane !== clip.lane) {
      void onMoveClip(clip.id, anchorBeat, lane);
    }
  };

  const cancelClipDrag = (clipId: number) => {
    activeDrag.current = null;
    setClipDrafts((current) => {
      const next = { ...current };
      delete next[clipId];
      return next;
    });
  };

  const startTempoPointDrag = (event: ReactPointerEvent<SVGRectElement>, clip: TimelineClip) => {
    if (busy || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const originalBeat = tempoPointDrafts[clip.id] ?? clip.tempoAnchorBeat;
    activeTempoPointDrag.current = {
      clipId: clip.id,
      minimumBeat: clip.visualStartBeat,
      maximumBeat: clip.visualEndBeat,
      originalBeat,
      currentBeat: originalBeat,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const startNearestTempoPointDrag = (event: ReactPointerEvent<SVGRectElement>) => {
    const bounds = event.currentTarget.ownerSVGElement?.getBoundingClientRect();
    if (!bounds) return;
    const nearest = tempoCurve.markers
      .filter((marker) => marker.clipId !== null)
      .reduce<typeof tempoCurve.markers[number] | null>((closest, marker) => (
        closest === null || Math.abs(marker.x - (event.clientX - bounds.left)) < Math.abs(closest.x - (event.clientX - bounds.left))
          ? marker
          : closest
      ), null);
    if (nearest?.clipId === null || nearest === null) return;
    const clip = timeline.clips.find((candidate) => candidate.id === nearest.clipId);
    if (clip) startTempoPointDrag(event, clip);
  };

  /**
   * Le clic droit sur la règle propose de **taper** le tempo du nœud le plus
   * proche.
   *
   * Le glissé ne déplace le nœud que dans le temps; la valeur, elle, ne se
   * réglait que dans l'éditeur de grille. Or on lit le chiffre juste là, sur la
   * règle — c'est là qu'on veut le corriger.
   */
  const openTempoEdit = (event: ReactMouseEvent<SVGRectElement>) => {
    event.preventDefault();
    event.stopPropagation();
    if (tempoEditingLocked) return;
    const bounds = event.currentTarget.ownerSVGElement?.getBoundingClientRect();
    if (!bounds) return;
    const pointerX = event.clientX - bounds.left;
    const nearest = tempoCurve.markers
      .filter((marker) => marker.clipId !== null)
      .reduce<(typeof tempoCurve.markers)[number] | null>(
        (closest, marker) =>
          closest === null || Math.abs(marker.x - pointerX) < Math.abs(closest.x - pointerX)
            ? marker
            : closest,
        null,
      );
    if (!nearest?.clipId) return;
    const clip = timeline.clips.find((candidate) => candidate.id === nearest.clipId);
    if (!clip?.bpm) return;
    setTempoEdit({
      clipId: clip.id,
      libraryTrackId: clip.libraryTrackId,
      fileName: clip.fileName,
      x: nearest.x,
      value: clip.bpm.toFixed(3),
    });
  };

  const commitTempoEdit = () => {
    if (!tempoEdit) return;
    const bpm = Number(tempoEdit.value.replace(",", "."));
    setTempoEdit(null);
    // Une saisie qui ne veut rien dire laisse le tempo tel quel : répondre zéro
    // à une frappe malheureuse arrêterait le morceau sans prévenir.
    if (!Number.isFinite(bpm) || bpm < 40 || bpm > 300) return;
    void onSetTrackTempo(tempoEdit.libraryTrackId, bpm);
  };

  const moveTempoPointDraft = (event: ReactPointerEvent<SVGRectElement>) => {
    const drag = activeTempoPointDrag.current;
    const bounds = event.currentTarget.ownerSVGElement?.getBoundingClientRect();
    if (!drag || !bounds) return;
    event.preventDefault();
    event.stopPropagation();
    const requestedBeat = (event.clientX - bounds.left) / pixelsPerBeat;
    const minimumMeasure = Math.ceil(Math.max(0, drag.minimumBeat) / BEATS_PER_MEASURE) * BEATS_PER_MEASURE;
    const maximumMeasure = Math.floor(drag.maximumBeat / BEATS_PER_MEASURE) * BEATS_PER_MEASURE;
    const tempoAnchorBeat = Math.min(
      maximumMeasure,
      snapTimelineBeat(requestedBeat, minimumMeasure),
    );
    drag.currentBeat = tempoAnchorBeat;
    setTempoPointDrafts((current) => ({ ...current, [drag.clipId]: tempoAnchorBeat }));
  };

  const finishTempoPointDrag = (event: ReactPointerEvent<SVGRectElement>) => {
    const drag = activeTempoPointDrag.current;
    activeTempoPointDrag.current = null;
    event.stopPropagation();
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (!drag) return;
    if (drag.currentBeat !== drag.originalBeat) {
      void onMoveTempoPoint(drag.clipId, drag.currentBeat);
    }
  };

  const cancelTempoPointDrag = () => {
    const drag = activeTempoPointDrag.current;
    activeTempoPointDrag.current = null;
    if (!drag) return;
    setTempoPointDrafts((current) => {
      const next = { ...current };
      delete next[drag.clipId];
      return next;
    });
  };

  const openVolumeTracksMenu = (event: ReactMouseEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest(".volume-node, .pan-node, .timeline-filter-lane, .clip-heading button")) return;
    event.preventDefault();
    const bounds = event.currentTarget.getBoundingClientRect();
    const lane = timelineLaneFromPointer(event.clientY, bounds.top, bounds.height, TIMELINE_LANES.length);
    const beat = Math.max(0, Math.round((event.clientX - bounds.left) / pixelsPerBeat * 4) / 4);
    // Both Pan and Volume can occupy the same time range. Keep every matching
    // Draw in the menu instead of guessing which visible curve was intended.
    const drawGroups = timeline.drawGroups
      .filter((group) => group.lane === lane && beat >= group.startBeat && beat <= group.endBeat)
      .sort((left, right) => right.id - left.id);
    setVolumeContextMenu({
      clientX: Math.min(event.clientX, window.innerWidth - 175),
      clientY: Math.min(event.clientY, window.innerHeight - 46),
      lane,
      beat,
      nodeId: null,
      drawGroups,
    });
  };

  const filterValueAtPointer = (event: ReactPointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height));
    const value = 1 - ratio * 2;
    return Math.abs(value) <= 0.05 ? 0 : Math.round(value * 100) / 100;
  };

  /**
   * Locates the drawn curve under a beat.
   *
   * A Filter Brush is persisted as a dense run of samples, not as one node, so
   * a curve is the stretch between the bypass samples that enclose an active
   * run. `startBeat` and `endBeat` are those enclosing samples: they are the
   * exact bounds to erase.
   */
  const filterBubbleRuns = (lane: number): FilterBubbleRun[] => {
    const points = timeline.filterNodes
      .filter((node) => node.lane === lane)
      .sort((left, right) => left.beat - right.beat);
    const runs: FilterBubbleRun[] = [];
    let firstActiveIndex = -1;

    for (let index = 0; index <= points.length; index += 1) {
      const point = points[index];
      const isActive = point !== undefined && Math.abs(point.value) > 0.05;
      if (isActive && firstActiveIndex === -1) {
        firstActiveIndex = index;
        continue;
      }
      if (isActive || firstActiveIndex === -1) continue;

      const activePoints = points.slice(firstActiveIndex, index);
      const startBeat = points[firstActiveIndex - 1]?.beat ?? activePoints[0].beat;
      const endBeat = point?.beat ?? activePoints.at(-1)!.beat;
      const peak = activePoints.reduce((strongest, candidate) => (
        Math.abs(candidate.value) > Math.abs(strongest.value) ? candidate : strongest
      ));
      runs.push({ startBeat, endBeat, peakBeat: peak.beat, peakValue: peak.value });
      firstActiveIndex = -1;
    }
    return runs;
  };

  const filterBubbleRunAt = (lane: number, beat: number): FilterBubbleRun | null =>
    filterBubbleRuns(lane).find((run) => run.startBeat <= beat && beat <= run.endBeat) ?? null;

  /**
   * Finds the curve edge the pointer is close enough to grab, and the room that
   * edge has before it would run into the neighbouring curve.
   */
  const filterEdgeAt = (lane: number, beat: number) => {
    const toleranceBeats = FILTER_EDGE_GRAB_PX / Math.max(0.001, pixelsPerBeat);
    const runs = filterBubbleRuns(lane);

    interface ClosestEdge {
      run: FilterBubbleRun;
      edge: "start" | "end";
      distance: number;
      index: number;
    }
    let closest: ClosestEdge | null = null;

    for (let index = 0; index < runs.length; index += 1) {
      const run = runs[index];
      for (const edge of ["start", "end"] as const) {
        const distance = Math.abs((edge === "start" ? run.startBeat : run.endBeat) - beat);
        if (distance > toleranceBeats) continue;
        if (closest === null || distance < closest.distance) {
          closest = { run, edge, distance, index };
        }
      }
    }

    if (closest === null) return null;
    const { run, edge, index } = closest;
    // A resize stops at the neighbour rather than overwriting it.
    return { run, edge, ...filterResizeLimits(runs, index) };
  };

  const existingFilterBubbleAt = (lane: number, beat: number): FilterBubbleDraft | null => {
    const run = filterBubbleRunAt(lane, beat);
    if (!run) return null;

    const totalDuration = Math.max(0.001, run.endBeat - run.startBeat);
    const peakRatio = (run.peakBeat - run.startBeat) / totalDuration;

    let shape: FilterBubbleShape = "triangle";
    if (peakRatio <= 0.35) {
      shape = "ramp_down";
    } else if (peakRatio >= 0.65) {
      shape = "ramp_up";
    }

    return {
      lane,
      startBeat: run.startBeat,
      widthBeats: Math.max(FILTER_BUBBLE_MIN_WIDTH_BEATS, run.endBeat - run.startBeat),
      value: run.peakValue,
      shape,
    };
  };

  const openFilterCurveMenu = (event: ReactMouseEvent<HTMLDivElement>, lane: number) => {
    // Always swallow the event: the filter sub-lane must never fall through to
    // the Volume Node menu of the track underneath.
    event.preventDefault();
    event.stopPropagation();
    if (busy) return;

    const bounds = event.currentTarget.getBoundingClientRect();
    const beat = Math.max(0, (event.clientX - bounds.left) / pixelsPerBeat);
    const run = filterBubbleRunAt(lane, beat);
    if (!run) {
      setFilterContextMenu(null);
      return;
    }

    setVolumeContextMenu(null);
    setFilterContextMenu({
      clientX: Math.min(event.clientX, window.innerWidth - 175),
      clientY: Math.min(event.clientY, window.innerHeight - 46),
      lane,
      startBeat: run.startBeat,
      endBeat: run.endBeat,
    });
  };

  const filterBeatAtPointer = (event: { clientX: number }, element: HTMLElement) => {
    const bounds = element.getBoundingClientRect();
    return Math.max(0, (event.clientX - bounds.left) / pixelsPerBeat);
  };

  const startFilterBubbleDraw = (event: ReactPointerEvent<HTMLDivElement>, lane: number) => {
    if (busy || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();

    const pointerBeat = filterBeatAtPointer(event, event.currentTarget);
    const snappedBeat = snapFilterBeat(pointerBeat);
    const edge = filterEdgeAt(lane, pointerBeat);

    let draft: FilterBubbleDraft;
    let gesture: FilterGesture;
    let anchorBeat = snappedBeat;
    let replacedRange: ActiveFilterBubble["replacedRange"] = null;
    let limitStartBeat = 0;
    let limitEndBeat = Number.POSITIVE_INFINITY;

    if (edge) {
      // An edge was grabbed: keep the curve's depth and shape, move its length.
      const existing = existingFilterBubbleAt(lane, edge.run.peakBeat);
      draft = existing ?? {
        lane,
        startBeat: edge.run.startBeat,
        widthBeats: Math.max(
          FILTER_BUBBLE_MIN_WIDTH_BEATS,
          edge.run.endBeat - edge.run.startBeat,
        ),
        value: edge.run.peakValue,
        shape: "ramp_up",
      };
      gesture = edge.edge === "start" ? "resize-start" : "resize-end";
      anchorBeat = edge.edge === "start" ? edge.run.endBeat : edge.run.startBeat;
      replacedRange = { startBeat: edge.run.startBeat, endBeat: edge.run.endBeat };
      limitStartBeat = edge.limitStartBeat;
      limitEndBeat = edge.limitEndBeat;
      draft = { ...draft, hiddenRange: replacedRange };
    } else {
      const existing = existingFilterBubbleAt(lane, snappedBeat);
      if (existing) {
        draft = existing;
        gesture = "depth";
      } else {
        draft = {
          lane,
          startBeat: snappedBeat,
          widthBeats: FILTER_BUBBLE_DEFAULT_WIDTH_BEATS,
          value: filterValueAtPointer(event),
          shape: event.shiftKey ? "triangle" : "ramp_up",
        };
        gesture = "create";
      }
    }

    activeFilterBubble.current = {
      ...draft,
      pointerId: event.pointerId,
      gesture,
      startClientX: event.clientX,
      initialClickBeat: snappedBeat,
      replacedRange,
      anchorBeat,
      limitStartBeat,
      limitEndBeat,
    };
    setFilterBubbleDraft(draft);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  /**
   * Shows the resize cursor when hovering a curve edge, so the invisible grab
   * zone announces itself. Written straight to the DOM: this runs on every
   * pointer move and must not re-render the timeline.
   */
  const updateFilterCursor = (event: ReactPointerEvent<HTMLDivElement>, lane: number) => {
    if (activeFilterBubble.current || busy) return;
    if (event.ctrlKey) {
      // Le crayon prime : la poignée de longueur n'est pas ce que `Ctrl` fait.
      event.currentTarget.style.cursor = "";
      return;
    }
    const pointerBeat = filterBeatAtPointer(event, event.currentTarget);
    event.currentTarget.style.cursor = filterEdgeAt(lane, pointerBeat) ? "ew-resize" : "";
  };

  const moveFilterBubbleDraw = (event: ReactPointerEvent<HTMLDivElement>) => {
    const active = activeFilterBubble.current;
    if (!active || active.pointerId !== event.pointerId) return;
    event.preventDefault();

    let startBeat = active.startBeat;
    let widthBeats = active.widthBeats;
    let shape: FilterBubbleShape = active.shape ?? "ramp_up";
    // Resizing changes length only; the depth stays where the curve had it.
    let value = active.value;

    const currentBeat = snapFilterBeat(filterBeatAtPointer(event, event.currentTarget));

    if (active.gesture === "create") {
      value = filterValueAtPointer(event);
      const initialBeat = active.initialClickBeat;
      const minBeat = Math.min(initialBeat, currentBeat);
      const maxBeat = Math.max(initialBeat, currentBeat);
      const rawWidth = maxBeat - minBeat;
      widthBeats = rawWidth < FILTER_BUBBLE_MIN_WIDTH_BEATS
        ? FILTER_BUBBLE_DEFAULT_WIDTH_BEATS
        : Math.min(FILTER_BUBBLE_MAX_WIDTH_BEATS, rawWidth);

      if (event.shiftKey) {
        shape = "triangle";
        startBeat = minBeat;
      } else if (currentBeat < initialBeat) {
        // Dragging left: the wall lands on the left edge (|\).
        shape = "ramp_down";
        startBeat = Math.max(0, initialBeat - widthBeats);
      } else {
        // Dragging right: the wall lands on the right edge (/|).
        shape = "ramp_up";
        startBeat = initialBeat;
      }
    } else if (active.gesture === "depth") {
      value = filterValueAtPointer(event);
    } else {
      const limits = {
        limitStartBeat: active.limitStartBeat,
        limitEndBeat: active.limitEndBeat,
      };
      const resized = active.gesture === "resize-end"
        ? resizeFilterCurveEnd(active.anchorBeat, currentBeat, limits)
        : resizeFilterCurveStart(active.anchorBeat, currentBeat, limits);
      startBeat = resized.startBeat;
      widthBeats = resized.endBeat - resized.startBeat;
    }

    const draft: FilterBubbleDraft = {
      lane: active.lane,
      startBeat,
      widthBeats,
      value,
      shape,
      hiddenRange: active.replacedRange ?? undefined,
    };
    activeFilterBubble.current = { ...active, ...draft };
    setFilterBubbleDraft(draft);
  };

  const finishFilterBubbleDraw = (event: ReactPointerEvent<HTMLDivElement>) => {
    const active = activeFilterBubble.current;
    if (!active || active.pointerId !== event.pointerId) return;
    activeFilterBubble.current = null;
    setFilterBubbleDraft(null);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    void onDrawFilterBubble(
      active.lane,
      active.startBeat,
      active.widthBeats,
      active.value,
      active.shape ?? "ramp_up",
      active.replacedRange ?? undefined,
    );
  };

  const cancelFilterBubbleDraw = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (activeFilterBubble.current?.pointerId !== event.pointerId) return;
    activeFilterBubble.current = null;
    setFilterBubbleDraft(null);
  };

  /**
   * Peint la valeur pointée sur le quart de temps sous le curseur.
   *
   * Un quart de temps est le pas de cette bande depuis le pinceau : les plages
   * effacées et redessinées comptent en quarts, et le moteur lisse la coupure
   * sur huit millisecondes de toute façon. Repasser sur un quart déjà peint
   * écrase sa valeur, ce qui rend le geste corrigeable sans le relâcher.
   */
  const paintFilterStroke = (event: ReactPointerEvent<HTMLDivElement>, lane: number) => {
    const beat = snapFilterBeat(filterBeatAtPointer(event, event.currentTarget));
    const value = filterValueAtPointer(event);
    setFilterStroke((current) => {
      const painted = new Map(current?.lane === lane ? current.painted : []);
      painted.set(beat, value);
      return { lane, painted };
    });
  };

  const startFilterStroke = (event: ReactPointerEvent<HTMLDivElement>, lane: number) => {
    if (busy || event.button !== 0) return false;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    filterStrokePointer.current = event.pointerId;
    paintFilterStroke(event, lane);
    return true;
  };

  const moveFilterStroke = (event: ReactPointerEvent<HTMLDivElement>, lane: number) => {
    if (filterStrokePointer.current !== event.pointerId) return;
    paintFilterStroke(event, lane);
  };

  const finishFilterStroke = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (filterStrokePointer.current !== event.pointerId) return;
    filterStrokePointer.current = null;
    const stroke = filterStroke;
    setFilterStroke(null);
    if (!stroke || stroke.painted.size === 0) return;
    const nodes = filterStrokeNodes(stroke.painted);
    if (nodes.length === 0) return;
    void onDrawFilterStroke(stroke.lane, nodes);
  };

  const cancelFilterStroke = () => {
    filterStrokePointer.current = null;
    setFilterStroke(null);
  };

  const filterPointsForLane = (lane: number) => {
    if (filterStroke?.lane === lane) {
      // Pendant un trait libre, la courbe montrée est celle qui sera écrite :
      // les mêmes points, passés par la même fonction.
      const nodes = filterStrokeNodes(filterStroke.painted);
      if (nodes.length > 0) {
        const from = nodes[0][0];
        const to = nodes[nodes.length - 1][0];
        return [
          ...timeline.filterNodes
            .filter((node) => node.lane === lane && (node.beat < from || node.beat > to))
            .map((node) => ({ beat: node.beat, value: node.value })),
          ...nodes.map(([beat, value]) => ({ beat, value })),
        ].sort((left, right) => left.beat - right.beat);
      }
    }
    return filterPointsForLaneWithBubble(lane);
  };

  const filterPointsForLaneWithBubble = (lane: number) => {
    const persisted = timeline.filterNodes
      .filter((node) => node.lane === lane)
      .map((node) => ({ beat: node.beat, value: node.value }));
    if (!filterBubbleDraft || filterBubbleDraft.lane !== lane) {
      return persisted.sort((left, right) => left.beat - right.beat);
    }
    const startBeat = Math.min(
      filterBubbleDraft.startBeat,
      filterBubbleDraft.hiddenRange?.startBeat ?? Number.POSITIVE_INFINITY,
    );
    const endBeat = Math.max(
      filterBubbleDraft.startBeat + filterBubbleDraft.widthBeats,
      filterBubbleDraft.hiddenRange?.endBeat ?? Number.NEGATIVE_INFINITY,
    );
    return [
      ...persisted.filter((point) => point.beat < startBeat || point.beat > endBeat),
      ...filterBubblePoints(filterBubbleDraft),
    ].sort((left, right) => left.beat - right.beat);
  };

  const moveVolumeNodeDraft = (event: ReactPointerEvent<HTMLButtonElement>, nodeId: number, lane: number) => {
    if (activeVolumeNode.current !== nodeId) return;
    const bounds = timelineTracks.current?.getBoundingClientRect();
    if (!bounds) return;
    const beat = Math.max(0, Math.round((event.clientX - bounds.left) / pixelsPerBeat * 4) / 4);
    // Read the pointer through the same units the node is drawn in, so a node
    // resolves to the gain it already shows instead of jumping on the way.
    const units = automationUnitsAtPointer(event.clientY, bounds.top, bounds.height);
    const gainDb = volumeNodeGainDb(lane, units);
    setVolumeDrafts((current) => ({ ...current, [nodeId]: { beat, gainDb } }));
  };

  /**
   * Le crayon. Le glissé donne l'étendue et la hauteur pointée; la forme et la
   * période viennent du bouton. Si les deux lignes sont affichées, un seul
   * geste écrit les deux automations — c'est le même mouvement musical.
   */
  const strokeBeatAt = (clientX: number) => {
    const bounds = timelineTracks.current?.getBoundingClientRect();
    if (!bounds) return null;
    return Math.max(0, Math.round((clientX - bounds.left) / pixelsPerBeat * 4) / 4);
  };

  const moveShapeStroke = (event: ReactPointerEvent<HTMLDivElement>) => {
    const stroke = drawStroke.current;
    if (!stroke) return;
    const bounds = timelineTracks.current?.getBoundingClientRect();
    const endBeat = strokeBeatAt(event.clientX);
    if (!bounds || endBeat === null) return;
    setShapePreview({
      lane: stroke.lane,
      startBeat: stroke.startBeat,
      endBeat,
      units: automationUnitsAtPointer(event.clientY, bounds.top, bounds.height),
    });
  };

  const startShapeStroke = (event: ReactPointerEvent<HTMLDivElement>, lane: number) => {
    if (busy || event.button !== 0) return false;
    // Les commandes d'un clip restent des commandes, crayon armé ou non.
    //
    // Le trait capture le pointeur sur la voie dès l'appui; un appui né sur
    // `EQ`, `VOX`, la chaîne ou la croix n'aboutissait donc jamais au clic, et
    // ces boutons semblaient morts sans qu'on voie pourquoi — il fallait
    // deviner que le crayon était en cause.
    if (
      event.target instanceof Element &&
      event.target.closest(".clip-heading button, .clip-heading-actions")
    ) {
      return false;
    }
    const bounds = timelineTracks.current?.getBoundingClientRect();
    if (!bounds) return false;
    event.currentTarget.setPointerCapture(event.pointerId);
    const startBeat = Math.max(0, Math.round((event.clientX - bounds.left) / pixelsPerBeat * 4) / 4);
    drawStroke.current = { lane, startBeat };
    setShapePreview({
      lane,
      startBeat,
      endBeat: startBeat,
      units: automationUnitsAtPointer(event.clientY, bounds.top, bounds.height),
    });
    return true;
  };

  const finishShapeStroke = (event: ReactPointerEvent<HTMLDivElement>) => {
    const stroke = drawStroke.current;
    drawStroke.current = null;
    setShapePreview(null);
    if (!stroke) return;
    const bounds = timelineTracks.current?.getBoundingClientRect();
    if (!bounds) return;

    const endBeat = Math.max(0, Math.round((event.clientX - bounds.left) / pixelsPerBeat * 4) / 4);
    if (Math.abs(endBeat - stroke.startBeat) < 0.25) return;
    const units = automationUnitsAtPointer(event.clientY, bounds.top, bounds.height);

    if (showVolumeAutomation) {
      // Le niveau déjà en place au départ du trait sert de plafond.
      const restDb = volumeDbAtBeat(timeline.volumeNodes, stroke.lane, stroke.startBeat);
      const pointedDb = volumeNodeGainDb(stroke.lane, units) ?? VOLUME_FLOOR_DB;
      const nodes = volumeShapeNodes(
        stroke.startBeat,
        endBeat,
        restDb,
        pointedDb,
        drawShape,
        drawPeriod,
      ).map((node) => [node.beat, node.gainDb] as [number, number]);
      if (nodes.length > 0) {
        void onDrawVolumeShape(stroke.lane, stroke.startBeat, endBeat, nodes, drawShape, drawPeriod);
      }
    }

    if (showPanAutomation) {
      const nodes = panShapeNodes(
        stroke.startBeat,
        endBeat,
        panNodeValue(stroke.lane, units),
        drawShape,
        drawPeriod,
        panValueAtBeat(timeline.panNodes, stroke.lane, stroke.startBeat),
      ).map((node) => [node.beat, node.value] as [number, number]);
      if (nodes.length > 0) {
        void onDrawPanShape(stroke.lane, stroke.startBeat, endBeat, nodes, drawShape, drawPeriod);
      }
    }
  };

  const movePanNodeDraft = (event: ReactPointerEvent<HTMLButtonElement>, nodeId: number, lane: number) => {
    if (activePanNode.current !== nodeId) return;
    const bounds = timelineTracks.current?.getBoundingClientRect();
    if (!bounds) return;
    const beat = Math.max(0, Math.round((event.clientX - bounds.left) / pixelsPerBeat * 4) / 4);
    const units = automationUnitsAtPointer(event.clientY, bounds.top, bounds.height);
    setPanDrafts((current) => ({ ...current, [nodeId]: { beat, value: panNodeValue(lane, units) } }));
  };

  const finishPanNodeDrag = (node: { id: number; beat: number; value: number }) => {
    activePanNode.current = null;
    const draft = panDrafts[node.id];
    if (draft && (draft.beat !== node.beat || draft.value !== node.value)) {
      void onMovePanNode(node.id, draft.beat, draft.value);
    }
  };

  const finishVolumeNodeDrag = (nodeId: number) => {
    activeVolumeNode.current = null;
    const node = timeline.volumeNodes.find((candidate) => candidate.id === nodeId);
    const draft = volumeDrafts[nodeId];
    if (node && draft && (draft.beat !== node.beat || draft.gainDb !== node.gainDb)) {
      void onMoveVolumeNode(nodeId, draft.beat, draft.gainDb);
    }
  };

  /* Les nœuds que le trait en cours poserait, pour la voie qu'il survole. Ils
     sont calculés par les mêmes fonctions que l'écriture : l'aperçu est le
     résultat, pas une approximation de celui-ci. */
  const previewVolumeNodes = (lane: number) => {
    if (!shapePreview || shapePreview.lane !== lane || !showVolumeAutomation) {
      return null;
    }
    const { startBeat, endBeat, units } = shapePreview;
    if (Math.abs(endBeat - startBeat) < 0.25) return null;
    return volumeShapeNodes(
      startBeat,
      endBeat,
      volumeDbAtBeat(timeline.volumeNodes, lane, startBeat),
      volumeNodeGainDb(lane, units) ?? VOLUME_FLOOR_DB,
      drawShape,
      drawPeriod,
    );
  };

  const previewPanNodes = (lane: number) => {
    if (!shapePreview || shapePreview.lane !== lane || !showPanAutomation) {
      return null;
    }
    const { startBeat, endBeat, units } = shapePreview;
    if (Math.abs(endBeat - startBeat) < 0.25) return null;
    return panShapeNodes(
      startBeat,
      endBeat,
      panNodeValue(lane, units),
      drawShape,
      drawPeriod,
      panValueAtBeat(timeline.panNodes, lane, startBeat),
    );
  };

  /* Les trois jeux de courbes ne dépendent que des nœuds, des brouillons en
     cours et du zoom — jamais du survol ni de la tête de lecture. Sans mémo,
     chaque rendu les redessinait d'un bout à l'autre du mix : sur une timeline
     large de soixante-dix mille pixels, c'est le calcul le plus cher du
     panneau, et il était refait pour un curseur qui change de forme.

     Et même mémorisés, ils n'ont aucune raison de couvrir tout le mix : la
     même correction que pour les waveforms, ici sur la position du dernier
     rendu. Les extrémités restent ancrées à 0 et à la largeur totale — un
     segment plat de plus ne coûte rien, et cela garde la courbe identique là
     où on la regarde. */
  const curveWindow = visibleBeatRange(
    renderedBeat,
    pixelsPerBeat,
    viewportWidth,
    VIEW_RENDER_STEP_PX,
  );
  const { fromBeat: curveFromBeat, toBeat: curveToBeat } = curveWindow;
  const automationPaths = useMemo(() => TIMELINE_LANES.map((lane) => {
    const preview = previewVolumeNodes(lane);
    const low = preview ? Math.min(shapePreview!.startBeat, shapePreview!.endBeat) : 0;
    const high = preview ? Math.max(shapePreview!.startBeat, shapePreview!.endBeat) : 0;
    const points = timeline.volumeNodes
      .filter((node) => node.lane === lane)
      .filter((node) => node.drawGroupId === null)
      .filter((node) => !preview || node.beat < low || node.beat > high)
      .map((node) => ({ ...node, ...(volumeDrafts[node.id] ?? {}) }))
      .concat(preview ? preview.map((node, index) => ({
        id: -1_000_000 - index,
        lane,
        drawGroupId: null,
        beat: node.beat,
        gainDb: node.gainDb,
      })) : [])
      .sort((left, right) => left.beat - right.beat);
    const spanning = points.length > 0
      ? points
      : [{ id: -lane - 1, lane, drawGroupId: null, beat: 0, gainDb: DEFAULT_TRACK_GAIN_DB }];
    const effective = nodesAcross(spanning, curveFromBeat, curveToBeat);
    const first = effective[0];
    const last = effective[effective.length - 1];
    return `M 0 ${volumeNodeY(lane, first.gainDb)} ${effective.map((node) => `L ${node.beat * pixelsPerBeat} ${volumeNodeY(lane, node.gainDb)}`).join(" ")} L ${contentWidth} ${volumeNodeY(lane, last.gainDb)}`;
  }), [
    contentWidth,
    curveFromBeat,
    curveToBeat,
    drawPeriod,
    drawShape,
    pixelsPerBeat,
    shapePreview,
    showVolumeAutomation,
    timeline.volumeNodes,
    volumeDrafts,
  ]);

  /* A Draw has one SVG path of its own. The audio table can retain many
     samples for a smooth envelope without turning the timeline into the same
     number of DOM-visible points or one enormous lane-wide path. */
  const drawVisualPointLimit = Math.max(32, Math.ceil(viewportWidth * DRAW_VISUAL_POINTS_PER_PIXEL));
  const volumeDrawPaths = useMemo(() => timeline.drawGroups
    .filter((group) => group.kind === "volume")
    .filter((group) => group.endBeat >= curveFromBeat && group.startBeat <= curveToBeat)
    .flatMap((group) => {
      const nodes = timeline.volumeNodes
        .filter((node) => node.drawGroupId === group.id)
        .sort((left, right) => left.beat - right.beat);
      if (nodes.length === 0) return [];
      const visible = compactDrawPoints(
        nodesAcross(nodes, curveFromBeat, curveToBeat),
        drawVisualPointLimit,
      );
      return [{
        id: group.id,
        path: `M ${visible.map((node) => `${node.beat * pixelsPerBeat} ${volumeNodeY(group.lane, node.gainDb)}`).join(" L ")}`,
      }];
    }), [
    curveFromBeat,
    curveToBeat,
    drawVisualPointLimit,
    pixelsPerBeat,
    timeline.drawGroups,
    timeline.volumeNodes,
  ]);

  const panPaths = useMemo(() => TIMELINE_LANES.map((lane) => {
    const preview = previewPanNodes(lane);
    const low = preview ? Math.min(shapePreview!.startBeat, shapePreview!.endBeat) : 0;
    const high = preview ? Math.max(shapePreview!.startBeat, shapePreview!.endBeat) : 0;
    const points = timeline.panNodes
      .filter((node) => node.lane === lane)
      .filter((node) => node.drawGroupId === null)
      .filter((node) => !preview || node.beat < low || node.beat > high)
      .map((node) => ({ ...node, ...(panDrafts[node.id] ?? {}) }))
      .concat(preview ? preview.map((node, index) => ({
        id: -2_000_000 - index,
        lane,
        drawGroupId: null,
        beat: node.beat,
        value: node.value,
      })) : [])
      .sort((left, right) => left.beat - right.beat);
    // Sans nœud, la ligne traverse la piste au centre : le panoramique d'une
    // voie qu'on n'a pas touchée est neutre, et se voit.
    if (points.length === 0) {
      return `M 0 ${panCentreY(lane)} L ${contentWidth} ${panCentreY(lane)}`;
    }
    const drawn = nodesAcross(points, curveFromBeat, curveToBeat);
    const first = drawn[0];
    const last = drawn[drawn.length - 1];
    return `M 0 ${panNodeY(lane, first.value)} ${drawn
      .map((node) => `L ${node.beat * pixelsPerBeat} ${panNodeY(lane, node.value)}`)
      .join(" ")} L ${contentWidth} ${panNodeY(lane, last.value)}`;
  }), [
    contentWidth,
    curveFromBeat,
    curveToBeat,
    drawPeriod,
    drawShape,
    panDrafts,
    pixelsPerBeat,
    shapePreview,
    showPanAutomation,
    timeline.panNodes,
  ]);

  const panDrawPaths = useMemo(() => timeline.drawGroups
    .filter((group) => group.kind === "pan")
    .filter((group) => group.endBeat >= curveFromBeat && group.startBeat <= curveToBeat)
    .flatMap((group) => {
      const nodes = timeline.panNodes
        .filter((node) => node.drawGroupId === group.id)
        .sort((left, right) => left.beat - right.beat);
      if (nodes.length === 0) return [];
      const visible = compactDrawPoints(
        nodesAcross(nodes, curveFromBeat, curveToBeat),
        drawVisualPointLimit,
      );
      return [{
        id: group.id,
        path: `M ${visible.map((node) => `${node.beat * pixelsPerBeat} ${panNodeY(group.lane, node.value)}`).join(" L ")}`,
      }];
    }), [
    curveFromBeat,
    curveToBeat,
    drawVisualPointLimit,
    pixelsPerBeat,
    timeline.drawGroups,
    timeline.panNodes,
  ]);

  const filterPaths = useMemo(() => TIMELINE_LANES.map((lane) => {
    const spanning = filterPointsForLane(lane);
    /* Le test « y a-t-il quelque chose à montrer » porte sur **toute** la voie,
       pas sur la tranche : autrement une bulle hors champ éteindrait la forme,
       et elle réapparaîtrait d'un coup en entrant dans la vue. */
    const drawn = nodesAcross(spanning, curveFromBeat, curveToBeat);
    const bubblePath = (direction: "high" | "low") => {
      const hasActive = spanning.some((point) => (direction === "high" ? point.value > 0.01 : point.value < -0.01));
      if (!hasActive) return "";
      const segments = [`M 0 ${filterNodeY(lane, 0)}`];
      for (const point of drawn) {
        const value = direction === "high" ? Math.max(0, point.value) : Math.min(0, point.value);
        segments.push(`L ${point.beat * pixelsPerBeat} ${filterNodeY(lane, value)}`);
      }
      segments.push(`L ${contentWidth} ${filterNodeY(lane, 0)} Z`);
      return segments.join(" ");
    };
    return { high: bubblePath("high"), low: bubblePath("low") };
  }), [
    contentWidth,
    curveFromBeat,
    curveToBeat,
    filterBubbleDraft,
    filterStroke,
    pixelsPerBeat,
    timeline.filterNodes,
  ]);

  return (
    <section className="timeline-panel" aria-label="Timeline">
      <div className="timeline-header">
        <div className="timeline-identity">
          <div className="analog-transport-group">
            <div className="analog-transport-row">
              {/* Vider la timeline a quitté cette rangée pour le bas de
                  l'aide : c'est le seul geste ici qui détruise du travail, et
                  il n'avait rien à faire à portée du pouce, entre deux
                  commandes qu'on presse cent fois par séance. */}
              <div className="transport-stack">
                <div className="analog-transport">
                <button
                  className={`analog-transport-button analog-play${transport.status === "playing" ? " is-active" : ""}`}
                  type="button"
                  disabled={busy || timeline.clips.length === 0 || transport.status === "playing"}
                  onClick={() => void onTogglePlayback()}
                  title="Play · Spacebar"
                >
                  <div className="transport-led-socket">
                    <i className={`transport-led transport-play-led${transport.status === "playing" ? " is-active" : ""}`} aria-hidden="true" />
                  </div>
                  <TransportGlyph name={preparing ? "busy" : "play"} />
                  <span className="transport-button-label">PLAY</span>
                </button>
                <button
                  className={`analog-transport-button analog-pause${transport.status === "paused" ? " is-active" : ""}`}
                  type="button"
                  disabled={busy || transport.status !== "playing"}
                  onClick={() => void onTogglePlayback()}
                  title="Pause · Spacebar"
                >
                  <div className="transport-led-socket">
                    <i className={`transport-led transport-pause-led${transport.status === "paused" ? " is-active" : ""}`} aria-hidden="true" />
                  </div>
                  <TransportGlyph name="pause" />
                  <span className="transport-button-label">PAUSE</span>
                </button>
                </div>
              </div>

              {/* Block 2: master dynamics. Sidechain ducking has no switch of
                  its own — a clip either holds the key or it does not. */}
              <div className="analog-transport">
                <button
                  className={`analog-transport-button analog-comp${timeline.compressorEnabled ? " is-active" : ""}`}
                  type="button"
                  disabled={busy}
                  aria-pressed={timeline.compressorEnabled}
                  onClick={() => void onSetCompressorEnabled(!timeline.compressorEnabled)}
                  title="Toggle Master Glue Compressor (2:1 with console tilt and saturation)"
                >
                  <div className="transport-led-socket">
                    <i className={`transport-led comp-led${timeline.compressorEnabled ? " is-active" : ""}`} aria-hidden="true" />
                  </div>
                  <TransportGlyph name="comp" />
                  <span className="transport-button-label">COMP</span>
                </button>
                <button
                  className={`analog-transport-button analog-limiter${timeline.limiterEnabled ? " is-active" : ""}`}
                  type="button"
                  disabled={busy}
                  aria-pressed={timeline.limiterEnabled}
                  onClick={() => void onSetLimiterEnabled(!timeline.limiterEnabled)}
                  title="Toggle Master Limiter (Overshoot & Peak Protection)"
                >
                  <div className="transport-led-socket">
                    <i className={`transport-led limiter-led${timeline.limiterEnabled ? " is-active" : ""}`} aria-hidden="true" />
                  </div>
                  <TransportGlyph name="limit" />
                  <span className="transport-button-label">LIMIT</span>
                </button>
              </div>

              {/* Undo / Redo Stalk */}
              <div className="timeline-summary">
                <button
                  className="undo-redo-btn"
                  type="button"
                  disabled={busy || !canUndo}
                  onClick={() => void onUndo?.()}
                  title="Undo (Ctrl+Z)"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ display: "block" }}>
                    <path d="M3 7v6h6" />
                    <path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13" />
                  </svg>
                </button>
                <button
                  className="undo-redo-btn"
                  type="button"
                  disabled={busy || !canRedo}
                  onClick={() => void onRedo?.()}
                  title="Redo (Ctrl+Y / Ctrl+Shift+Z)"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ display: "block" }}>
                    <path d="M21 7v6h-6" />
                    <path d="M3 17a9 9 0 0 1 9-9 9 9 0 0 1 6 2.3l3 2.7" />
                  </svg>
                </button>
              </div>

              {/* Ce que l'œil regarde, pas ce que le moteur fait : un réglage
                  d'affichage, donc local à la vue et non persisté. */}
              <div className="analog-transport">
                {/* La touche rogne ce qui dépasse — c'est ce qui tient sa
                    brillance — donc le rappel ne peut pas vivre dedans. Cette
                    ancre lui donne un repère de position sans rien rogner. */}
                <span className="transport-hint">
                <button
                  className={`analog-transport-button analog-view${automationView === "none" ? "" : " is-active"}`}
                  type="button"
                  onClick={() => setAutomationView(nextAutomationView(automationView))}
                  aria-label={`Automation lines: ${AUTOMATION_VIEW_LABELS[automationView]} — click to cycle, or press E`}
                >
                  <div className="transport-led-socket">
                    <i
                      className={`transport-led view-led${automationView === "none" ? "" : " is-active"}`}
                      aria-hidden="true"
                    />
                  </div>
                  <TransportGlyph name={AUTOMATION_VIEW_GLYPHS[automationView]} />
                  <span className="transport-button-label">VIEW</span>
                </button>
                <span className="lane-hint-tip" aria-hidden="true">
                  <strong>Automation lines</strong>
                  <span>Showing: {AUTOMATION_VIEW_LABELS[automationView]}</span>
                  <span>Cycles pan, volume, both, hidden</span>
                  <span><kbd>E</kbd> cycles · <kbd>V</kbd> volume node · <kbd>P</kbd> pan node</span>
                  <span>Both drop at the playhead, on the selected track</span>
                </span>
                </span>

                {/* Coupé en deux : la forme à gauche, la période à droite.
                    Seize combinaisons, deux clics — et l'état éteint est le
                    premier cran de la forme, donc pas de bouton d'armement en
                    plus.
                    Toute la touche change la forme, y compris la diode, le
                    libellé et les marges : elle porte `cursor: pointer` sur
                    toute sa surface, donc une zone inerte s'y lit comme une
                    panne. Seuls les chiffres retiennent leur clic. */}
                <span className="transport-hint">
                <div
                  className={`analog-transport-button analog-draw${drawArmable ? " is-active" : ""}${
                    drawArmable ? "" : " is-disabled"
                  }`}
                  onClick={() => {
                    if (drawArmable) setDrawShape(nextDrawShape(drawShape));
                  }}
                  aria-label={
                    drawArmable
                      ? `Pencil: ${drawShape} — click to cycle, or press S`
                      : "Show a line first — VIEW"
                  }
                >
                  <div className="transport-led-socket">
                    <i
                      className={`transport-led draw-led${drawArmable ? " is-active" : ""}`}
                      aria-hidden="true"
                    />
                  </div>
                  <div className="draw-halves">
                    <button
                      type="button"
                      className="draw-half"
                      disabled={!drawArmable}
                    >
                      <TransportGlyph name={DRAW_GLYPHS[drawShape]} />
                    </button>
                    <button
                      type="button"
                      className={`draw-half draw-half--period${drawPeriod < 1 ? " is-fraction" : ""}`}
                      disabled={!drawArmable}
                      onClick={(event) => {
                        // La touche entière change la forme; seuls les chiffres
                        // gardent leur clic pour eux.
                        event.stopPropagation();
                        setDrawPeriod(nextDrawPeriod(drawPeriod));
                      }}
                      title={periodTitle(drawPeriod)}
                    >
                      {periodLabel(drawPeriod)}
                    </button>
                  </div>
                  <span className="transport-button-label">DRAW</span>
                </div>
                <span className="lane-hint-tip" aria-hidden="true">
                  <strong>Pencil</strong>
                  <span>Left half the shape, right half the period</span>
                  <span><kbd>S</kbd> shape · <kbd>D</kbd> period</span>
                  <span>{drawArmable ? "Drag a clip's body to draw" : "Show a line first — VIEW"}</span>
                </span>
                </span>
              </div>

            </div>
          </div>
        </div>

        <div className="vu-meter-column">
          <StereoVuMeter liveTransport={liveTransport} />
          {/* Sous le VU-mètre, dans la place que `BOUNCE MIX` occupe à côté :
              au-dessus, ce décompte poussait la plaque vers le bas et
              l'empêchait de partir de la même ligne que le reste. */}
          <div className="vu-meter-summary">
            <span>Tracks: <strong>{timeline.clips.length}</strong></span>
            <span>Total Time: <strong>{formatDuration(totalTimeMs)}</strong></span>
          </div>
        </div>

        {/* Ce que fait un clic dans la timeline. Ce n'est ni un réglage
            d'affichage ni un traitement du son : une règle de conduite du
            transport, à part du reste et posée après le VU-mètre.
            Non persisté, comme les réglages de vue — allumé est l'état qu'on
            veut retrouver au lancement, et c'est le défaut. */}
        <div className="analog-transport autoplay-bay">
          <span className="transport-hint">
            <button
              className={`analog-transport-button analog-autoplay${autoplay ? " is-active" : ""}`}
              type="button"
              aria-pressed={autoplay}
              onClick={() => onSetAutoplay(!autoplay)}
              aria-label={
                autoplay
                  ? "Autoplay on: clicking the timeline starts playback"
                  : "Autoplay off: clicking the timeline only moves the playhead"
              }
            >
              <div className="transport-led-socket">
                <i
                  className={`transport-led autoplay-led${autoplay ? " is-active" : ""}`}
                  aria-hidden="true"
                />
              </div>
              <TransportGlyph name="autoplay" />
              <span className="transport-button-label">AUTO</span>
            </button>
            <span className="lane-hint-tip" aria-hidden="true">
              <strong>Autoplay</strong>
              <span>{autoplay ? "On — a click starts playback" : "Off — a click only moves the playhead"}</span>
              <span>Clicking the timeline, whatever else is playing</span>
            </span>
          </span>
        </div>

        <div className="timeline-controls">
          {/* Le rendu hors ligne relit tous les clips de bout en bout : il
              mérite sa propre commande, à côté du groupe et non dedans. */}
          <button
            className="bounce-btn"
            type="button"
            disabled={busy || timeline.clips.length === 0}
            onClick={() => void onBounceMix()}
            title="Render the whole timeline to a 16-bit 44.1 kHz stereo WAV"
          >
            <TransportGlyph name="bounce" />
            <span>BOUNCE MIX</span>
          </button>
          <div className="bpm-column">
            {/* A readout, not a control: the tempo map is driven by the clips,
                each of which places its own target on the curve. */}
            <div className="project-bpm-control" title="Project tempo at the playhead">
              <div className="bpm-header-row">
                <span className="bpm-label-text">BPM</span>
              </div>
              <span className="bpm-value-display" ref={bpmDisplay}>{currentBpmText}</span>
            </div>
            {/* Un projet est un instantané transportable de la session; la
                base reste l'état de travail enregistré au fil de l'eau. */}
            <div className="project-file-row">
              <button
                className="help-btn"
                type="button"
                disabled={busy}
                onClick={() => void onSaveProject()}
                title="Save this session to a portable project file"
              >
                SAVE
              </button>
              <button
                className="help-btn"
                type="button"
                disabled={busy}
                onClick={() => void onLoadProject()}
                title="Replace this session with a saved project file"
              >
                LOAD
              </button>
            </div>
            <button
              className="help-btn"
              type="button"
              onClick={() => setShowHelpModal(true)}
              title="Open Keyboard Shortcuts & Reference Guide (Esc)"
            >
              HELP
            </button>
          </div>
        </div>
      </div>

      <div className="timeline-body" ref={timelineBody}>
        <div className="timeline-lane-controls" aria-label="Fixed track controls">
          <div aria-hidden="true" />
          {TIMELINE_LANES.map((lane) => {
            const laneState = timeline.lanes.find((state) => state.lane === lane) ?? {
              lane,
              isMuted: false,
              isSolo: false,
            };

            return (
              <div className="timeline-lane-cell" key={lane}>
                {/* Un repère, pas une commande : la bande de filtre ne dit pas
                    d'elle-même qu'on peut y dessiner, ni que deux modificateurs
                    changent le trait. Il ne fait rien au clic — il se contente
                    de répondre au survol. */}
                <span
                  className="lane-filter-hint"
                  tabIndex={0}
                  role="note"
                  aria-label={`Filter band for track ${String.fromCharCode(65 + lane)} — drag to draw, Shift for a triangle, Ctrl for freehand`}
                >
                  F
                  <span className="lane-hint-tip" aria-hidden="true">
                    <strong>Filter band</strong>
                    <span><kbd>Drag</kbd> draw a curve</span>
                    <span><kbd>Shift</kbd> + <kbd>Drag</kbd> symmetrical triangle</span>
                    <span><kbd>Ctrl</kbd> + <kbd>Drag</kbd> freehand</span>
                    <span><kbd>Drag an edge</kbd> resize it</span>
                    <span><kbd>Right click</kbd> delete it</span>
                  </span>
                </span>
              <div
                className={`timeline-lane-controls-row${selectedLane === lane ? " is-selected" : ""}`}
                title={`Track ${String.fromCharCode(65 + lane)}${selectedLane === lane ? " · keyboard target (B, Shift+S, Shift+M)" : ""}`}
                onPointerDownCapture={() => onSelectLane(lane)}
              >
                <button
                  className={`lane-mute-button${laneState.isMuted ? " is-active" : ""}`}
                  type="button"
                  disabled={busy}
                  aria-pressed={laneState.isMuted}
                  aria-label={`Mute track ${String.fromCharCode(65 + lane)} — Shift+M on the selected track`}
                  onClick={() => void onSetLaneMuted(lane, !laneState.isMuted)}
                >
                  M
                  {/* Le raccourci vise la piste **sélectionnée**, pas celle-ci :
                      le taire ferait croire que Shift+M coupe la voie qu'on
                      survole. Cliquer ce bouton la sélectionne, donc les deux
                      gestes s'enchaînent. */}
                  <span className="lane-hint-tip" aria-hidden="true">
                    <strong>Mute</strong>
                    <span>Silence track {String.fromCharCode(65 + lane)}</span>
                    <span><kbd>Shift</kbd> + <kbd>M</kbd> on the selected track</span>
                  </span>
                </button>
                <button
                  className={`lane-solo-button${laneState.isSolo ? " is-active" : ""}`}
                  type="button"
                  disabled={busy}
                  aria-pressed={laneState.isSolo}
                  aria-label={`Solo track ${String.fromCharCode(65 + lane)} — Shift+S on the selected track`}
                  onClick={() => void onSetLaneSolo(lane, !laneState.isSolo)}
                >
                  S
                  <span className="lane-hint-tip" aria-hidden="true">
                    <strong>Solo</strong>
                    <span>Hear track {String.fromCharCode(65 + lane)} alone</span>
                    <span><kbd>Shift</kbd> + <kbd>S</kbd> on the selected track</span>
                  </span>
                </button>
              </div>
              </div>
            );
          })}
        </div>

        <div
          className="timeline-scroll"
          ref={timelineScroll}
        >
          <div
            className="timeline-content timeline-content--following"
            style={{
              width: contentWidth,
              boxSizing: contentNeedsNativeScroll ? "content-box" : "border-box",
              paddingInline: contentNeedsNativeScroll ? viewportWidth / 2 : 0,
              marginLeft: contentNeedsNativeScroll ? 0 : Math.max(0, (viewportWidth - contentWidth) / 2),
            }}
          >
          <div className="timeline-ruler" onClick={handleTimelineSeek}>
            <svg
              className="timeline-tempo-curve"
              width={contentWidth}
              height={34}
              viewBox={`0 0 ${contentWidth} 34`}
              aria-label="Global tempo curve"
            >
              <path d={tempoCurve.path} />
              {tempoCurve.markers.some((marker) => marker.clipId !== null) && (
                <rect
                  className="timeline-tempo-drag-surface"
                  x={0}
                  y={0}
                  width={contentWidth}
                  height={34}
                  onPointerDown={startNearestTempoPointDrag}
                  onPointerMove={moveTempoPointDraft}
                  onPointerUp={finishTempoPointDrag}
                  onPointerCancel={cancelTempoPointDrag}
                  onContextMenu={openTempoEdit}
                  onClick={(event) => event.stopPropagation()}
                />
              )}
              {tempoCurve.markers.map((marker) => {
                return (
                  <g key={marker.clipId ?? "project"}>
                    <circle
                      cx={marker.x}
                      cy={marker.y}
                      r={marker.clipId === null ? 2.5 : 3.5}
                    />
                    {marker.clipId !== null && (
                      <g className="tempo-text-badge">
                        <rect
                          x={marker.x + 4}
                          y={Math.max(2, marker.y - 12)}
                          width={44}
                          height={13}
                          rx={3}
                        />
                        <text x={marker.x + 8} y={Math.max(11, marker.y - 3)}>
                          {marker.bpm.toFixed(2)}
                        </text>
                      </g>
                    )}
                  </g>
                );
              })}
            </svg>
            {measures.map((measure) => (
              <span
                className="measure-marker"
                key={measure}
                style={{ left: measure * 4 * pixelsPerBeat }}
              />
            ))}
            {tempoEdit && (
              /* Posée sur le nœud qu'elle règle, pas au coin de l'écran : on
                 voit le chiffre qu'on change à l'endroit où on l'a lu. */
              <div
                className="tempo-edit-popover"
                style={{ left: tempoEdit.x }}
                onClick={(event) => event.stopPropagation()}
              >
                <label htmlFor="tempo-edit-input">{tempoEdit.fileName}</label>
                <input
                  id="tempo-edit-input"
                  type="number"
                  min="40"
                  max="300"
                  step="0.001"
                  autoFocus
                  value={tempoEdit.value}
                  onChange={(event) =>
                    setTempoEdit((current) =>
                      current ? { ...current, value: event.currentTarget.value } : current,
                    )
                  }
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      commitTempoEdit();
                    } else if (event.key === "Escape") {
                      event.preventDefault();
                      setTempoEdit(null);
                    }
                  }}
                  /* Quitter le champ abandonne, comme `Échap` : une valeur à
                     moitié tapée qu'on laisse en cliquant ailleurs ne doit pas
                     changer le tempo du mix. */
                  onBlur={() => setTempoEdit(null)}
                />
                <span>BPM · Enter to set</span>
              </div>
            )}
          </div>

          <div
            className="timeline-tracks"
            ref={timelineTracks}
            onContextMenu={openVolumeTracksMenu}
          >
            {TIMELINE_LANES.map((lane) => {
              const laneState = timeline.lanes.find((state) => state.lane === lane) ?? {
                lane,
                isMuted: false,
                isSolo: false,
              };
              const anySolo = timeline.lanes.some((state) => state.isSolo);
              const isAudible = !laneState.isMuted && (!anySolo || laneState.isSolo);

              return (
                <div
                  className={`timeline-lane-pair${selectedLane === lane ? " is-selected" : ""}`}
                  key={lane}
                  /* Capture, so arming the lane happens before the filter brush
                     or a clip drag claims the gesture, and without cancelling
                     any of them. */
                  onPointerDownCapture={() => onSelectLane(lane)}
                >
                  <div
                    className={`timeline-filter-lane${freehandArmed ? " is-freehand" : ""}`}
                    /* Le repère de mesure, puis le dégradé vertical. Voir
                       `.timeline-lane` : une propriété ordinaire à la place
                       d'une propriété personnalisée. */
                    style={{ backgroundSize: `${pixelsPerBeat * 4}px 100%, 100% 100%` }}
                    data-lane={lane}
                    /* `Ctrl` au moment du clic choisit le geste, et lui seul :
                       une fois le trait commencé, le relâcher en cours de
                       route ne doit pas le transformer en pinceau. */
                    onPointerDown={(event) => {
                      if (event.ctrlKey) {
                        startFilterStroke(event, lane);
                        return;
                      }
                      startFilterBubbleDraw(event, lane);
                    }}
                    onPointerMove={(event) => {
                      if (filterStrokePointer.current !== null) {
                        moveFilterStroke(event, lane);
                        return;
                      }
                      updateFilterCursor(event, lane);
                      moveFilterBubbleDraw(event);
                    }}
                    onPointerUp={(event) => {
                      if (filterStrokePointer.current !== null) {
                        finishFilterStroke(event);
                        return;
                      }
                      finishFilterBubbleDraw(event);
                    }}
                    onPointerCancel={(event) => {
                      if (filterStrokePointer.current !== null) {
                        cancelFilterStroke();
                        return;
                      }
                      cancelFilterBubbleDraw(event);
                    }}
                    onContextMenu={(event) => openFilterCurveMenu(event, lane)}
                  />
                  <div
                    className={`timeline-lane${busy ? " timeline-lane--busy" : ""}${dropTargetLane === lane ? " timeline-lane--drop-target" : ""}${pixelsPerBeat < 1 ? " timeline-lane--compressed" : ""}${isAudible ? "" : " timeline-lane--inaudible"}`}
                    /* Une mesure par tuile. `--beat-width` disparaît avec :
                       aucune règle ne la lisait, et elle invalidait la voie
                       entière à chaque zoom pour rien. */
                    style={{ backgroundSize: `${pixelsPerBeat * 4}px 100%` }}
                    data-lane={lane}
                    onClick={handleTimelineSeek}
                    onPointerMove={moveShapeStroke}
                    onPointerUp={finishShapeStroke}
                    onPointerCancel={() => {
                      drawStroke.current = null;
                      setShapePreview(null);
                    }}
                  />
                </div>
              );
            })}

            {showVolumeAutomation && <svg
              className="volume-automation-lines"
              width={contentWidth}
              height="100%"
              viewBox={`0 0 ${contentWidth} 450`}
              preserveAspectRatio="none"
              aria-hidden="true"
            >
              {automationPaths.map((path, lane) => <path d={path} key={`manual-${lane}`} />)}
              {volumeDrawPaths.map((draw) => <path d={draw.path} key={`draw-${draw.id}`} />)}
            </svg>}

            {showPanAutomation && <svg
              className="pan-automation-lines"
              width={contentWidth}
              height="100%"
              viewBox={`0 0 ${contentWidth} 450`}
              preserveAspectRatio="none"
              aria-hidden="true"
            >
              {panPaths.map((path, lane) => <path d={path} key={`manual-${lane}`} />)}
              {panDrawPaths.map((draw) => <path d={draw.path} key={`draw-${draw.id}`} />)}
            </svg>}

            <svg
              className="filter-automation-lines"
              width={contentWidth}
              height="100%"
              viewBox={`0 0 ${contentWidth} 450`}
              preserveAspectRatio="none"
              aria-hidden="true"
            >
              {filterPaths.map((paths, lane) => (
                <g key={lane}>
                  <path className="filter-automation-bubble filter-automation-bubble--high" d={paths.high} />
                  <path className="filter-automation-bubble filter-automation-bubble--low" d={paths.low} />
                </g>
              ))}
            </svg>

            {showPanAutomation && timeline.panNodes.filter((node) => node.drawGroupId === null).map((node) => {
              const draft = panDrafts[node.id];
              const beat = draft?.beat ?? node.beat;
              const value = draft?.value ?? node.value;
              return (
                <button
                  className="pan-node"
                  type="button"
                  key={node.id}
                  style={{ left: beat * pixelsPerBeat, top: `${panNodeY(node.lane, value) / 4.5}%` }}
                  title={`${String.fromCharCode(65 + node.lane)} · pan ${panLabel(value)} · beat ${beat.toFixed(2)}`}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    setPanContextMenu({
                      clientX: Math.min(event.clientX, window.innerWidth - 175),
                      clientY: Math.min(event.clientY, window.innerHeight - 46),
                      lane: node.lane,
                      beat,
                      nodeId: node.id,
                      drawGroups: [],
                    });
                  }}
                  onPointerDown={(event) => {
                    if (busy || event.button !== 0) return;
                    event.stopPropagation();
                    activePanNode.current = node.id;
                    event.currentTarget.setPointerCapture(event.pointerId);
                  }}
                  onPointerMove={(event) => movePanNodeDraft(event, node.id, node.lane)}
                  onPointerUp={() => finishPanNodeDrag(node)}
                  onPointerCancel={() => {
                    activePanNode.current = null;
                    setPanDrafts((current) => {
                      const next = { ...current };
                      delete next[node.id];
                      return next;
                    });
                  }}
                >
                  <span>{panLabel(value)}</span>
                </button>
              );
            })}

            {showVolumeAutomation && timeline.volumeNodes.filter((node) => node.drawGroupId === null).map((node) => {
              const draft = volumeDrafts[node.id];
              const beat = draft?.beat ?? node.beat;
              const gainDb = draft?.gainDb === undefined ? node.gainDb : draft.gainDb;
              return (
                <button
                  className="volume-node"
                  type="button"
                  key={node.id}
                  style={{ left: beat * pixelsPerBeat, top: `${volumeNodeY(node.lane, gainDb) / 4.5}%` }}
                  title={`${String.fromCharCode(65 + node.lane)} · ${gainLabel(gainDb)} · beat ${beat.toFixed(2)}`}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    setVolumeContextMenu({
                      clientX: Math.min(event.clientX, window.innerWidth - 175),
                      clientY: Math.min(event.clientY, window.innerHeight - 46),
                      lane: node.lane,
                      beat,
                      nodeId: node.id,
                      drawGroups: [],
                    });
                  }}
                  onPointerDown={(event) => {
                    if (busy || event.button !== 0) return;
                    event.stopPropagation();
                    activeVolumeNode.current = node.id;
                    event.currentTarget.setPointerCapture(event.pointerId);
                  }}
                  onPointerMove={(event) => moveVolumeNodeDraft(event, node.id, node.lane)}
                  onPointerUp={() => finishVolumeNodeDrag(node.id)}
                  onPointerCancel={() => {
                    activeVolumeNode.current = null;
                    setVolumeDrafts((current) => {
                      const next = { ...current };
                      delete next[node.id];
                      return next;
                    });
                  }}
                >
                  <span>{gainLabel(gainDb)}</span>
                </button>
              );
            })}

            {/* Filter Brush stores its samples internally; individual samples stay invisible.
            {timeline.filterNodes.map((node) => {
              const draft = filterDrafts[node.id];
              const beat = draft?.beat ?? node.beat;
              const value = draft?.value ?? node.value;
              return (
              <button
                className="filter-node"
                type="button"
                key={node.id}
                style={{ left: beat * pixelsPerBeat, top: `${filterNodeY(node.lane, value) / 4.5}%` }}
                title={`${String.fromCharCode(65 + node.lane)} Filter · ${value > 0 ? "HP" : value < 0 ? "LP" : "Bypass"} · beat ${beat.toFixed(2)}`}
                onContextMenu={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  setFilterContextMenu({
                    clientX: Math.min(event.clientX, window.innerWidth - 175),
                    clientY: Math.min(event.clientY, window.innerHeight - 74),
                    lane: node.lane,
                    beat,
                    value,
                    nodeId: node.id,
                  });
                }}
                onPointerDown={(event) => {
                  if (busy || event.button !== 0) return;
                  event.stopPropagation();
                  activeFilterNode.current = node.id;
                  event.currentTarget.setPointerCapture(event.pointerId);
                }}
                onPointerMove={(event) => moveFilterNodeDraft(event, node.id, node.lane)}
                onPointerUp={(event) => {
                  if (activeFilterNode.current !== node.id) return;
                  finishFilterNodeDrag(node.id);
                  event.currentTarget.releasePointerCapture(event.pointerId);
                }}
                onPointerCancel={() => {
                  activeFilterNode.current = null;
                  setFilterDrafts((current) => {
                    const next = { ...current };
                    delete next[node.id];
                    return next;
                  });
                }}
              />
              );
            })}
            */}

            {timeline.clips.length === 0 && (
              <div className="timeline-empty">
                <strong>Drag an analyzed track here</strong>
                <span>Its first downbeat will snap to a four-beat bar.</span>
              </div>
            )}

            {renderedClips.map((clip) => {
              const draft = clipDrafts[clip.id];
              const anchorBeat = draft?.anchorBeat ?? clip.anchorBeat;
              const lane = draft?.lane ?? clip.lane;
              const laneState = timeline.lanes.find((state) => state.lane === lane);
              const anySolo = timeline.lanes.some((state) => state.isSolo);
              const isAudible = laneState
                ? !laneState.isMuted && (!anySolo || laneState.isSolo)
                : true;
              const live = clipWithTrim(clip, trimDrafts[clip.id]);
              const visualStartBeat = live.visualStartBeat + (draft ? draft.anchorBeat - clip.anchorBeat : 0);
              const liveDurationBeats = live.visualEndBeat - live.visualStartBeat;
              const clipWidth = Math.max(0, liveDurationBeats * pixelsPerBeat);
              const trimEdge = hoveredTrim?.clipId === clip.id ? hoveredTrim.edge : null;
              const seqIndex = clipSequenceNumbers.get(clip.id) ?? 1;
              const track = libraryTracksById.get(clip.libraryTrackId);
              const trackDisplayName = track ? libraryDisplayName(track) : clip.fileName;
              const clipLabel = `${trackDisplayName} - #${seqIndex}`;
              const canBeKey = canBeSidechainKey(clip, timeline.clips);
              const coveredCount = canBeKey ? clipsCoveredByKey(clip, timeline.clips).length : 0;
              const keyTitle = !canBeKey
                ? "Sidechain key — available once this clip overlaps another"
                : clip.isSidechainKey
                  ? `Sidechain key: this clip is silent here and pumps ${coveredCount} clip${coveredCount > 1 ? "s" : ""}`
                  : `Use as sidechain key — pumps ${coveredCount} clip${coveredCount > 1 ? "s" : ""} it overlaps`;
              const classNames = [
                "timeline-clip",
                clip.isSidechainKey ? "timeline-clip--sidechain-key" : "",
                clip.isMissing ? "timeline-clip--missing" : "",
                clip.needsAnalysis ? "timeline-clip--invalid" : "",
                isAudible ? "" : "timeline-clip--inaudible",
                trimEdge ? `timeline-clip--trim-${trimEdge}` : "",
                hoveredTool?.clipId === clip.id ? smartToolClass(hoveredTool.tool) : "",
              ]
                .filter(Boolean)
                .join(" ");

              return (
                <div
                  className={classNames}
                  key={clip.id}
                  /* L'ancre est placée sur l'ancre, et non par une propriété
                     personnalisée posée ici. Un seul descendant la lisait —
                     `.clip-anchor`, un `span` — mais la poser sur le clip
                     invalidait le style calculé de **tout** ce qu'il contient,
                     waveform comprise, à chaque cran de zoom. Troisième
                     occurrence du même défaut; voir `.timeline-lane`. */
                  style={{
                    left: visualStartBeat * pixelsPerBeat,
                    width: clipWidth,
                    top: `calc(${lane * (100 / TIMELINE_LANES.length)}% + (100% / 9) + 7px)`,
                  }}
                  title={`${clipLabel} · track ${String.fromCharCode(65 + lane)} · first downbeat on bar ${anchorBeat / BEATS_PER_MEASURE + 1}`}
                  /* An edge belongs to the trim tool; anywhere else moves the
                     clip. Asking the trim first is what keeps the two gestures
                     from fighting over the same pointer. */
                  onPointerDown={(event) => {
                    if (event.button !== 0) return;
                    const tool = toolAtPointer(event, clip);
                    if (tool === "draw") {
                      if (startShapeStroke(event, lane)) return;
                    } else if (tool === "trim-start" || tool === "trim-end") {
                      if (startClipTrim(event, clip)) return;
                    }
                    startClipDrag(event, clip);
                  }}
                  onPointerMove={(event) => {
                    // Un geste engagé garde la main jusqu'au relâchement : le
                    // pointeur est capturé, donc un trait né dans le corps
                    // continue de dessiner même en passant sur la barre.
                    if (drawStroke.current) {
                      moveShapeStroke(event);
                      return;
                    }
                    if (moveClipTrim(event, clip)) return;
                    updateSmartCursor(event, clip);
                    moveClipDraft(event, clip.id);
                  }}
                  onPointerLeave={() => {
                    if (!activeTrim.current) {
                      setHoveredTrim(null);
                      setHoveredTool(null);
                    }
                  }}
                  onPointerUp={(event) => {
                    if (drawStroke.current) {
                      finishShapeStroke(event);
                      return;
                    }
                    if (finishClipTrim(clip)) return;
                    finishClipDrag(clip);
                  }}
                  onPointerCancel={() => {
                    activeTrim.current = null;
                    setTrimDrafts((current) => {
                      const next = { ...current };
                      delete next[clip.id];
                      return next;
                    });
                    cancelClipDrag(clip.id);
                  }}
                >
                  <div className="clip-heading">
                    <strong className="clip-title">{clipLabel}</strong>
                    <div className="clip-heading-actions">
                      {/* Trois états pour deux touches : rien d'allumé, le
                          morceau entier. Cliquer celle qui est allumée y
                          revient; les deux ensemble voudraient dire la même
                          chose, donc c'est impossible. */}
                      {(["vocals", "instrumental"] as const).map((stem) => {
                        const isOn = clip.stem === stem;
                        const label = stem === "vocals" ? "VOX" : "MUS";
                        return (
                          <button
                            key={stem}
                            type="button"
                            className={`clip-stem-btn${isOn ? " is-active" : ""}`}
                            disabled={busy}
                            aria-pressed={isOn}
                            onClick={(event) => {
                              event.stopPropagation();
                              if (isOn) {
                                void onSetClipStem(clip.id, "full");
                              } else if (clip.hasStems) {
                                void onSetClipStem(clip.id, stem);
                              } else {
                                // Premier clic sur un clip jamais séparé : le
                                // rendu et la bascule sont un seul geste.
                                void onSeparateStems(clip.id, stem);
                              }
                            }}
                            title={
                              isOn
                                ? `Playing ${stem} only — click for the whole track`
                                : clip.hasStems
                                  ? `Play the ${stem} of this track`
                                  : `Separate this track, then play its ${stem}`
                            }
                          >
                            {label}
                          </button>
                        );
                      })}
                      <button
                        type="button"
                        className={`clip-key-btn${clip.isSidechainKey ? " is-active" : ""}`}
                        disabled={busy || !canBeKey}
                        aria-pressed={clip.isSidechainKey}
                        onClick={(event) => {
                          event.stopPropagation();
                          void onSetSidechainKey(clip.id, !clip.isSidechainKey);
                        }}
                        title={keyTitle}
                      >
                        {/* Le dessin vit dans `TransportGlyph`, pas ici : la
                            fenêtre d'aide montre le même bouton, et deux copies
                            du même trait finissent par diverger — c'est
                            précisément ce qui était arrivé. */}
                        <TransportGlyph name="sidechain" />
                      </button>
                      <button
                        type="button"
                        className={`clip-eq-btn-trigger${isClipEqActive(clip.eqSettings) ? " is-active" : ""}`}
                        onClick={(event) => {
                          event.stopPropagation();
                          onOpenClipEq?.(clip);
                        }}
                        title={isClipEqActive(clip.eqSettings) ? "Clip EQ active — Click to edit" : "Open Clip EQ"}
                      >
                        {isClipEqActive(clip.eqSettings) ? "EQ •" : "EQ"}
                      </button>
                      {/* Cuire ce clip : ses effets passent dans un fichier à
                          lui, et la voie repart à plat sous lui. C'est une
                          bascule — le même bouton défait ce qu'il a fait — et
                          l'automation retirée est gardée pour ça. */}
                      <button
                        type="button"
                        className={`clip-bake-btn${clip.isBaked ? " is-active" : ""}${clip.bakeIsMissing ? " is-missing" : ""}`}
                        disabled={busy || clip.isMissing || clip.needsAnalysis}
                        aria-pressed={clip.isBaked}
                        onClick={(event) => {
                          event.stopPropagation();
                          void onSetClipBaked(clip.id, !clip.isBaked);
                        }}
                        title={
                          clip.bakeIsMissing
                            ? "Baked, but its file is gone — the clip is playing its source. Click to undo and get the automation back, then bake again."
                            : clip.isBaked
                              ? "Baked — click to undo it and bring the automation back (replaces what was drawn since)"
                              : "Bake this clip: render its EQ and this lane's automation into its own file, then flatten the lane under it"
                        }
                      >
                        {clip.bakeIsMissing ? "BAKE?" : "BAKE"}
                      </button>
                      <button
                        type="button"
                        className="clip-remove-btn"
                        disabled={tempoEditingLocked}
                        onClick={() => void onRemoveClip(clip.id)}
                        aria-label={`Remove ${clip.fileName} from timeline`}
                        title="Remove clip"
                      >
                        ×
                      </button>
                    </div>
                  </div>
                  <ClipWaveform
                    waveform={clip.waveform}
                    displayWidth={Math.max(1, clipWidth - 16)}
                    trimStartBeats={live.trimStartBeats}
                    trimEndBeats={live.trimEndBeats}
                    durationBeats={liveDurationBeats}
                    /* La fenêtre, ramenée dans les coordonnées du dessin : le
                       clip commence à `visualStartBeat`, et le dessin huit
                       pixels plus loin — c'est l'encart de `.clip-waveform`. */
                    visibleFromPx={visibleContentFromPx - visualStartBeat * pixelsPerBeat - 8}
                    visibleWidthPx={viewportWidth}
                  />
                  <span
                    className="clip-anchor"
                    style={{ left: clip.preRollBeats * pixelsPerBeat }}
                    aria-hidden="true"
                  />
                </div>
              );
            })}
          </div>
          </div>
        </div>
        {timeline.clips.length > 0 && (
          <div className="timeline-playhead timeline-playhead--fixed" aria-hidden="true">
            <span />
          </div>
        )}
      </div>

      {/* Photorealistic TE Horizontal Timeline Scrollbar */}
      <div
        className="timeline-horizontal-scrollbar"
        title="Timeline Horizontal Navigator - Click or drag to scroll timeline position"
        onPointerDown={handleScrollbarPointerDown}
      >
        <div className="timeline-scrollbar-track">
          <div
            ref={scrollbarThumb}
            className="timeline-scrollbar-thumb"
            style={{
              width: `${thumbWidthPercent}%`,
              left: `${thumbLeftPercent}%`,
            }}
          >
            <div className="scrollbar-thumb-ridges" aria-hidden="true">
              <span />
              <span />
              <span />
            </div>
          </div>
        </div>
      </div>
      {libraryPointerDrag?.phase === "dragging" && (
        <div
          className={`timeline-pointer-drag-ghost${dropTargetLane === null ? "" : " is-over-timeline"}`}
          style={{
            left: libraryPointerDrag.clientX + 14,
            top: libraryPointerDrag.clientY + 14,
          }}
          aria-hidden="true"
        >
          ↦ {dropTargetLane === null ? "Timeline" : `Drop on ${String.fromCharCode(65 + dropTargetLane)}`}
        </div>
      )}
      {volumeContextMenu && (
        <div
          className="timeline-context-menu"
          style={{ left: volumeContextMenu.clientX, top: volumeContextMenu.clientY }}
          role="menu"
          onMouseLeave={() => setVolumeContextMenu(null)}
        >
          {volumeContextMenu.nodeId === null ? (
            <>
              {volumeContextMenu.drawGroups.map((group) => (
                <button key={group.id} type="button" role="menuitem" className="is-destructive" disabled={busy} onClick={() => {
                  void onDeleteDrawGroup(group.id);
                  setVolumeContextMenu(null);
                }}>
                  {volumeContextMenu.drawGroups.length === 1 ? "Delete Draw" : `Delete ${group.kind === "pan" ? "Pan" : "Volume"} Draw`}
                </button>
              ))}
              <button
                type="button"
                role="menuitem"
                disabled={busy || !showVolumeAutomation}
                title={showVolumeAutomation ? undefined : "Show the volume line first — VIEW"}
                onClick={() => {
                  void onAddVolumeNode(volumeContextMenu.lane, volumeContextMenu.beat);
                  setVolumeContextMenu(null);
                }}
              >
                Add Volume Node
              </button>
              <button
                type="button"
                role="menuitem"
                disabled={busy || !showPanAutomation}
                title={showPanAutomation ? undefined : "Show the pan line first — VIEW"}
                onClick={() => {
                  void onAddPanNode(volumeContextMenu.lane, volumeContextMenu.beat);
                  setVolumeContextMenu(null);
                }}
              >
                Add Pan Node
              </button>
            </>
          ) : (
            <button
              type="button"
              role="menuitem"
              className="is-destructive"
              disabled={busy}
              onClick={() => {
                void onDeleteVolumeNode(volumeContextMenu.nodeId!);
                setVolumeContextMenu(null);
              }}
            >
              Delete Volume Node
            </button>
          )}
        </div>
      )}
      {panContextMenu && (
        <div
          className="timeline-context-menu"
          style={{ left: panContextMenu.clientX, top: panContextMenu.clientY }}
          role="menu"
          onMouseLeave={() => setPanContextMenu(null)}
        >
          {panContextMenu.nodeId === null ? (
            <button
              type="button"
              role="menuitem"
              disabled={busy}
              onClick={() => {
                void onAddPanNode(panContextMenu.lane, panContextMenu.beat);
                setPanContextMenu(null);
              }}
            >
              Add Pan Node
            </button>
          ) : (
            <button
              type="button"
              role="menuitem"
              className="is-destructive"
              disabled={busy}
              onClick={() => {
                void onDeletePanNode(panContextMenu.nodeId!);
                setPanContextMenu(null);
              }}
            >
              Delete Pan Node
            </button>
          )}
        </div>
      )}
      {filterContextMenu && (
        <div
          className="timeline-context-menu"
          style={{ left: filterContextMenu.clientX, top: filterContextMenu.clientY }}
          role="menu"
          onMouseLeave={() => setFilterContextMenu(null)}
        >
          <button
            type="button"
            role="menuitem"
            className="is-destructive"
            disabled={busy}
            onClick={() => {
              void onClearFilterRange(
                filterContextMenu.lane,
                filterContextMenu.startBeat,
                filterContextMenu.endBeat,
              );
              setFilterContextMenu(null);
            }}
          >
            Delete Filter Curve
          </button>
        </div>
      )}
      <HelpModal
        isOpen={showHelpModal}
        onClose={() => setShowHelpModal(false)}
        onOpenAbout={() => setShowAboutModal(true)}
        isCoveredByAbout={showAboutModal}
        canClearTimeline={!busy && timeline.clips.length > 0}
        onClearTimeline={() => {
          if (window.confirm("Clear every clip from the timeline? Your Library tracks are kept.")) {
            void onClearTimeline?.();
            setShowHelpModal(false);
          }
        }}
        canClearEverything={!busy && (timeline.clips.length > 0 || libraryTracks.length > 0)}
        onClearEverything={() => {
          // Deux phrases : ce qui part, et ce qui ne part pas. La seconde compte
          // autant que la première — on hésite surtout parce qu'on croit
          // risquer ses fichiers.
          if (
            window.confirm(
              [
                "Clear the whole session — every clip AND every track in the library?",
                "",
                "Your audio files on disk are never touched, and this cannot be undone.",
              ].join("\n"),
            )
          ) {
            void onClearEverything?.();
            setShowHelpModal(false);
          }
        }}
      />
      {/* Par-dessus l'aide plutôt qu'à sa place : on y va pour lire une licence,
          pas pour quitter la référence. */}
      <AboutModal isOpen={showAboutModal} onClose={() => setShowAboutModal(false)} />
    </section>
  );
}
