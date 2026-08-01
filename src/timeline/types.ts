export interface WaveformPeaks {
  leftMin: number[];
  leftMax: number[];
  leftRms: number[];
  rightMin: number[];
  rightMax: number[];
  rightRms: number[];
}

export interface ClipEqSettings {
  highPassHz: number;
  lowPassHz: number;
  peakHz?: number;       // 20 Hz to 20000 Hz (default 1000 Hz)
  peakGainDb?: number;   // -18 dB to +6 dB (default 0 dB)
  peakQ?: number;        // 0.1 to 10.0 (default 1.0)
  gainDb?: number;       // -Infinity / -48 dB to +12 dB (default 0 dB)
  enabled?: boolean;
}

export interface TimelineClip {
  id: number;
  libraryTrackId: number;
  fileName: string;
  filePath: string;
  lane: number;
  anchorBeat: number;
  tempoAnchorBeat: number;
  bpm: number | null;
  firstBeatMs: number | null;
  preRollBeats: number;
  durationBeats: number;
  visualStartBeat: number;
  visualEndBeat: number;
  trimStartBeats: number;
  trimEndBeats: number;
  isSidechainKey: boolean;
  /** Laquelle des voix du morceau ce clip joue. */
  stem: "full" | "vocals" | "instrumental";
  /** Si le morceau a déjà été séparé : un clic instantané, ou deux minutes. */
  hasStems: boolean;
  /**
   * Si ce clip joue un fichier cuit plutôt que sa source.
   *
   * Son égalisation et l'automation de sa voie sont alors **dans** le son. Les
   * commandes qui les règlent n'ont plus rien à régler sous lui.
   */
  isBaked: boolean;
  /**
   * Si le fichier cuit a disparu du disque.
   *
   * Le clip reste cuit — l'automation retirée doit rester récupérable — mais il
   * joue sa source. Une touche allumée qui n'applique rien est un mensonge
   * silencieux; celle-ci le dit.
   */
  bakeIsMissing: boolean;
  isMissing: boolean;
  needsAnalysis: boolean;
  waveform: WaveformPeaks | null;
  eqSettings?: ClipEqSettings;
}

export interface TimelinePanNode {
  id: number;
  lane: number;
  /** −1 hard left, 0 centre, +1 hard right. */
  value: number;
  beat: number;
}

export interface TimelineSnapshot {
  projectBpm: number;
  limiterEnabled: boolean;
  compressorEnabled: boolean;
  tempoPoints: TimelineTempoPoint[];
  lanes: TimelineLane[];
  clips: TimelineClip[];
  volumeNodes: TimelineVolumeNode[];
  panNodes: TimelinePanNode[];
  filterNodes: TimelineFilterNode[];
}

export interface TimelineVolumeNode {
  id: number;
  lane: number;
  beat: number;
  gainDb: number | null;
}

export interface TimelineFilterNode {
  id: number;
  lane: number;
  beat: number;
  value: number;
  tension: number;
}

export interface TimelineTempoPoint {
  beat: number;
  bpm: number;
  clipId: number | null;
}

export interface TimelineLane {
  lane: number;
  isMuted: boolean;
  isSolo: boolean;
}

export interface TimelineTransportSnapshot {
  status: "paused" | "playing";
  positionBeat: number;
  meterLeft: number;
  meterRight: number;
  meterOverload: boolean;
}
