/**
 * Version of the beat analysis this build produces. Mirrors
 * `ANALYSIS_ALGORITHM_VERSION` in `src-tauri/src/analysis.rs`: a track cached
 * below this number is re-analysed once at startup, keeping any manual
 * correction. Both must be bumped together, or an improved algorithm never
 * reaches the tracks already in the library.
 */
export const ANALYSIS_ALGORITHM_VERSION = 3;

export interface LibraryTrack {
  id: number;
  filePath: string;
  fileName: string;
  artist: string | null;
  title: string | null;
  durationMs: number;
  sampleRate: number;
  channels: number;
  bpm: number | null;
  analyzedBpm: number | null;
  bpmConfidence: number | null;
  firstBeatMs: number | null;
  analyzedFirstBeatMs: number | null;
  beatCount: number;
  isCorrected: boolean;
  analysisStatus: string;
  analysisError: string | null;
  analysisVersion: number;
  isMissing: boolean;
}

export interface LibraryImportResult {
  tracks: LibraryTrack[];
  addedCount: number;
  addedTrackIds: number[];
  duplicateCount: number;
  failedCount: number;
}

/**
 * Une piste qui vient d'être analysée, envoyée pendant le lot.
 *
 * Miroir de `AnalysisProgress` dans `src-tauri/src/lib.rs`. La rangée entière
 * voyage, et non le seul tempo : l'interface remplace la sienne par celle-ci
 * sans avoir à savoir lesquels de ses champs viennent de changer.
 */
export interface AnalysisProgress {
  track: LibraryTrack;
  done: number;
  total: number;
}

export interface AnalysisBatchResult {
  tracks: LibraryTrack[];
  analyzedCount: number;
  failedCount: number;
}
