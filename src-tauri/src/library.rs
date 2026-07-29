use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::{
    analysis::{ANALYSIS_ALGORITHM_VERSION, BeatAnalysis, WAVEFORM_BUCKET_COUNT, WaveformPeaks},
    audio::{inspect_mp3, read_mp3_id3_tags},
};

const DATABASE_PRAGMAS: &str = r#"
    PRAGMA journal_mode = WAL;
    PRAGMA foreign_keys = ON;
"#;

const CURRENT_DATABASE_SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS library_tracks (
        id              INTEGER PRIMARY KEY,
        file_path       TEXT NOT NULL,
        path_key        TEXT NOT NULL UNIQUE,
        file_name       TEXT NOT NULL,
        artist          TEXT,
        title           TEXT,
        id3_scanned     INTEGER NOT NULL DEFAULT 0 CHECK (id3_scanned IN (0, 1)),
        duration_ms     INTEGER NOT NULL CHECK (duration_ms >= 0),
        sample_rate     INTEGER NOT NULL CHECK (sample_rate > 0),
        channels        INTEGER NOT NULL CHECK (channels > 0),
        bpm             REAL,
        manual_bpm      REAL,
        bpm_confidence  REAL,
        first_beat_ms   INTEGER,
        manual_first_beat_ms INTEGER,
        beat_count      INTEGER NOT NULL DEFAULT 0,
        analysis_status TEXT NOT NULL DEFAULT 'not_analyzed',
        analysis_error  TEXT,
        analysis_version INTEGER NOT NULL DEFAULT 0,
        added_at        INTEGER NOT NULL DEFAULT (unixepoch())
    );

    CREATE TABLE IF NOT EXISTS track_beats (
        track_id    INTEGER NOT NULL REFERENCES library_tracks(id) ON DELETE CASCADE,
        beat_index  INTEGER NOT NULL,
        position_ms INTEGER NOT NULL CHECK (position_ms >= 0),
        PRIMARY KEY (track_id, beat_index)
    );

    CREATE TABLE IF NOT EXISTS track_waveforms (
        track_id     INTEGER PRIMARY KEY
                     REFERENCES library_tracks(id) ON DELETE CASCADE,
        bucket_count INTEGER NOT NULL CHECK (bucket_count > 0),
        left_min     BLOB NOT NULL,
        left_max     BLOB NOT NULL,
        left_rms     BLOB NOT NULL,
        right_min    BLOB NOT NULL,
        right_max    BLOB NOT NULL,
        right_rms    BLOB NOT NULL,
        generated_at INTEGER NOT NULL DEFAULT (unixepoch())
    );

    CREATE INDEX IF NOT EXISTS library_tracks_name_idx
        ON library_tracks(file_name COLLATE NOCASE);

    CREATE INDEX IF NOT EXISTS library_tracks_artist_title_idx
        ON library_tracks(artist COLLATE NOCASE, title COLLATE NOCASE);

    CREATE TABLE IF NOT EXISTS project_settings (
        id              INTEGER PRIMARY KEY CHECK (id = 1),
        project_bpm     REAL NOT NULL DEFAULT 120.0
                        CHECK (project_bpm >= 40.0 AND project_bpm <= 300.0),
        limiter_enabled INTEGER NOT NULL DEFAULT 1
                        CHECK (limiter_enabled IN (0, 1)),
        compressor_enabled INTEGER NOT NULL DEFAULT 0
                        CHECK (compressor_enabled IN (0, 1))
    );

    INSERT OR IGNORE INTO project_settings (id, project_bpm)
        VALUES (1, 120.0);

    CREATE TABLE IF NOT EXISTS timeline_lanes (
        lane       INTEGER PRIMARY KEY CHECK (lane BETWEEN 0 AND 2),
        is_muted   INTEGER NOT NULL DEFAULT 0 CHECK (is_muted IN (0, 1)),
        is_solo    INTEGER NOT NULL DEFAULT 0 CHECK (is_solo IN (0, 1))
    );

    INSERT OR IGNORE INTO timeline_lanes (lane, is_muted, is_solo)
        VALUES (0, 0, 0), (1, 0, 0), (2, 0, 0);

    CREATE TABLE IF NOT EXISTS timeline_clips (
        id               INTEGER PRIMARY KEY,
        library_track_id INTEGER NOT NULL
                         REFERENCES library_tracks(id) ON DELETE CASCADE,
        lane             INTEGER NOT NULL DEFAULT 0 CHECK (lane BETWEEN 0 AND 2),
        anchor_beat      INTEGER NOT NULL CHECK (anchor_beat >= 0),
        tempo_anchor_beat INTEGER NOT NULL CHECK (tempo_anchor_beat >= 0),
        eq_settings      TEXT,
        trim_start_beats REAL NOT NULL DEFAULT 0.0 CHECK (trim_start_beats >= 0.0),
        trim_end_beats   REAL NOT NULL DEFAULT 0.0 CHECK (trim_end_beats >= 0.0),
        is_sidechain_key INTEGER NOT NULL DEFAULT 0
                         CHECK (is_sidechain_key IN (0, 1)),
        stem             TEXT NOT NULL DEFAULT 'full'
                         CHECK (stem IN ('full', 'vocals', 'instrumental')),
        created_at       INTEGER NOT NULL DEFAULT (unixepoch())
    );

    CREATE INDEX IF NOT EXISTS timeline_clips_track_idx
        ON timeline_clips(library_track_id);

    CREATE TABLE IF NOT EXISTS clip_stems (
        id             INTEGER PRIMARY KEY,
        clip_id        INTEGER NOT NULL
                       REFERENCES timeline_clips(id) ON DELETE CASCADE,
        kind           TEXT NOT NULL CHECK (kind IN ('vocals', 'instrumental')),
        file_path      TEXT NOT NULL,
        source_from_ms INTEGER NOT NULL DEFAULT 0 CHECK (source_from_ms >= 0),
        bucket_count   INTEGER,
        left_min       BLOB,
        left_max       BLOB,
        left_rms       BLOB,
        right_min      BLOB,
        right_max      BLOB,
        right_rms      BLOB,
        created_at     INTEGER NOT NULL DEFAULT (unixepoch()),
        UNIQUE (clip_id, kind)
    );

    CREATE TABLE IF NOT EXISTS timeline_volume_nodes (
        id      INTEGER PRIMARY KEY,
        lane    INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 2),
        beat    REAL NOT NULL CHECK (beat >= 0.0),
        gain_db REAL CHECK (gain_db IS NULL OR gain_db BETWEEN -60.0 AND 12.0),
        UNIQUE (lane, beat)
    );

    CREATE INDEX IF NOT EXISTS timeline_volume_nodes_lane_beat_idx
        ON timeline_volume_nodes(lane, beat);

    CREATE TABLE IF NOT EXISTS timeline_pan_nodes (
        id    INTEGER PRIMARY KEY,
        lane  INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 2),
        beat  REAL NOT NULL CHECK (beat >= 0.0),
        value REAL NOT NULL CHECK (value BETWEEN -1.0 AND 1.0),
        UNIQUE (lane, beat)
    );

    CREATE INDEX IF NOT EXISTS timeline_pan_nodes_lane_beat_idx
        ON timeline_pan_nodes(lane, beat);

    CREATE TABLE IF NOT EXISTS timeline_filter_nodes (
        id      INTEGER PRIMARY KEY,
        lane    INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 2),
        beat    REAL NOT NULL CHECK (beat >= 0.0),
        value   REAL NOT NULL CHECK (value BETWEEN -1.0 AND 1.0),
        tension REAL NOT NULL DEFAULT 0.0 CHECK (tension BETWEEN -1.0 AND 1.0),
        UNIQUE (lane, beat)
    );

    CREATE INDEX IF NOT EXISTS timeline_filter_nodes_lane_beat_idx
        ON timeline_filter_nodes(lane, beat);

    PRAGMA user_version = 24;
"#;

/// Schema version described by `CURRENT_DATABASE_SCHEMA`. The constant and the
/// `PRAGMA user_version` above must move together: the schema is also replayed
/// after a migration, so a stale value there would push the database back down
/// and replay the last migrations on every start.
const LATEST_SCHEMA_VERSION: i64 = 24;

const MIGRATE_VERSION_1_TO_2: &str = r#"
    BEGIN IMMEDIATE;
    ALTER TABLE library_tracks ADD COLUMN bpm_confidence REAL;
    ALTER TABLE library_tracks ADD COLUMN first_beat_ms INTEGER;
    ALTER TABLE library_tracks ADD COLUMN beat_count INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE library_tracks ADD COLUMN analysis_error TEXT;

    CREATE TABLE track_beats (
        track_id    INTEGER NOT NULL REFERENCES library_tracks(id) ON DELETE CASCADE,
        beat_index  INTEGER NOT NULL,
        position_ms INTEGER NOT NULL CHECK (position_ms >= 0),
        PRIMARY KEY (track_id, beat_index)
    );

    PRAGMA user_version = 2;
    COMMIT;
"#;

const MIGRATE_VERSION_2_TO_3: &str = r#"
    BEGIN IMMEDIATE;
    ALTER TABLE library_tracks ADD COLUMN manual_bpm REAL;
    ALTER TABLE library_tracks ADD COLUMN manual_first_beat_ms INTEGER;
    PRAGMA user_version = 3;
    COMMIT;
"#;

const MIGRATE_VERSION_3_TO_4: &str = r#"
    BEGIN IMMEDIATE;
    CREATE TABLE project_settings (
        id          INTEGER PRIMARY KEY CHECK (id = 1),
        project_bpm REAL NOT NULL DEFAULT 120.0
                    CHECK (project_bpm >= 40.0 AND project_bpm <= 300.0)
    );
    INSERT INTO project_settings (id, project_bpm) VALUES (1, 120.0);

    CREATE TABLE timeline_clips (
        id               INTEGER PRIMARY KEY,
        library_track_id INTEGER NOT NULL
                         REFERENCES library_tracks(id) ON DELETE CASCADE,
        lane             INTEGER NOT NULL DEFAULT 0 CHECK (lane = 0),
        anchor_beat      INTEGER NOT NULL CHECK (anchor_beat >= 0),
        created_at       INTEGER NOT NULL DEFAULT (unixepoch())
    );
    CREATE INDEX timeline_clips_track_idx ON timeline_clips(library_track_id);

    PRAGMA user_version = 4;
    COMMIT;
"#;

const MIGRATE_VERSION_4_TO_5: &str = r#"
    BEGIN IMMEDIATE;
    CREATE TABLE track_waveforms (
        track_id     INTEGER PRIMARY KEY
                     REFERENCES library_tracks(id) ON DELETE CASCADE,
        bucket_count INTEGER NOT NULL CHECK (bucket_count > 0),
        left_min     BLOB NOT NULL,
        left_max     BLOB NOT NULL,
        right_min    BLOB NOT NULL,
        right_max    BLOB NOT NULL,
        generated_at INTEGER NOT NULL DEFAULT (unixepoch())
    );
    PRAGMA user_version = 5;
    COMMIT;
"#;

const MIGRATE_VERSION_5_TO_6: &str = r#"
    BEGIN IMMEDIATE;
    ALTER TABLE timeline_clips RENAME TO timeline_clips_schema_5;
    CREATE TABLE timeline_clips (
        id               INTEGER PRIMARY KEY,
        library_track_id INTEGER NOT NULL
                         REFERENCES library_tracks(id) ON DELETE CASCADE,
        lane             INTEGER NOT NULL DEFAULT 0 CHECK (lane BETWEEN 0 AND 2),
        anchor_beat      INTEGER NOT NULL CHECK (anchor_beat >= 0),
        created_at       INTEGER NOT NULL DEFAULT (unixepoch())
    );
    INSERT INTO timeline_clips
        (id, library_track_id, lane, anchor_beat, created_at)
        SELECT id, library_track_id, lane, anchor_beat, created_at
        FROM timeline_clips_schema_5;
    DROP TABLE timeline_clips_schema_5;
    CREATE INDEX timeline_clips_track_idx ON timeline_clips(library_track_id);
    PRAGMA user_version = 6;
    COMMIT;
"#;

const MIGRATE_VERSION_6_TO_7: &str = r#"
    BEGIN IMMEDIATE;
    CREATE TABLE timeline_lanes (
        lane       INTEGER PRIMARY KEY CHECK (lane BETWEEN 0 AND 2),
        is_muted   INTEGER NOT NULL DEFAULT 0 CHECK (is_muted IN (0, 1)),
        is_solo    INTEGER NOT NULL DEFAULT 0 CHECK (is_solo IN (0, 1))
    );
    INSERT INTO timeline_lanes (lane, is_muted, is_solo)
        VALUES (0, 0, 0), (1, 0, 0), (2, 0, 0);
    PRAGMA user_version = 7;
    COMMIT;
"#;

const MIGRATE_VERSION_7_TO_8: &str = r#"
    BEGIN IMMEDIATE;
    UPDATE timeline_clips
    SET anchor_beat = MAX(
        ((anchor_beat + 2) / 4) * 4,
        CAST((
            COALESCE((
                SELECT COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms, 0)
                     * COALESCE(tracks.manual_bpm, tracks.bpm, 120.0) / 60000.0
                FROM library_tracks AS tracks
                WHERE tracks.id = timeline_clips.library_track_id
            ), 0.0) + 3.999999999
        ) / 4 AS INTEGER) * 4
    );
    PRAGMA user_version = 8;
    COMMIT;
"#;

const MIGRATE_VERSION_8_TO_9: &str = r#"
    BEGIN IMMEDIATE;
    ALTER TABLE library_tracks
        ADD COLUMN analysis_version INTEGER NOT NULL DEFAULT 0;
    PRAGMA user_version = 9;
    COMMIT;
"#;

const MIGRATE_VERSION_9_TO_10: &str = r#"
    BEGIN IMMEDIATE;
    DROP TABLE track_waveforms;
    CREATE TABLE track_waveforms (
        track_id     INTEGER PRIMARY KEY
                     REFERENCES library_tracks(id) ON DELETE CASCADE,
        bucket_count INTEGER NOT NULL CHECK (bucket_count > 0),
        left_min     BLOB NOT NULL,
        left_max     BLOB NOT NULL,
        left_rms     BLOB NOT NULL,
        right_min    BLOB NOT NULL,
        right_max    BLOB NOT NULL,
        right_rms    BLOB NOT NULL,
        generated_at INTEGER NOT NULL DEFAULT (unixepoch())
    );
    PRAGMA user_version = 10;
    COMMIT;
"#;

const MIGRATE_VERSION_10_TO_11: &str = r#"
    BEGIN IMMEDIATE;
    CREATE TABLE IF NOT EXISTS timeline_volume_nodes (
        id      INTEGER PRIMARY KEY,
        lane    INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 2),
        beat    REAL NOT NULL CHECK (beat >= 0.0),
        gain_db REAL CHECK (gain_db IS NULL OR gain_db BETWEEN -60.0 AND 12.0),
        UNIQUE (lane, beat)
    );
    CREATE INDEX IF NOT EXISTS timeline_volume_nodes_lane_beat_idx
        ON timeline_volume_nodes(lane, beat);
    PRAGMA user_version = 11;
    COMMIT;
"#;

const MIGRATE_VERSION_11_TO_12: &str = r#"
    BEGIN IMMEDIATE;
    ALTER TABLE library_tracks ADD COLUMN artist TEXT;
    ALTER TABLE library_tracks ADD COLUMN title TEXT;
    ALTER TABLE library_tracks
        ADD COLUMN id3_scanned INTEGER NOT NULL DEFAULT 0 CHECK (id3_scanned IN (0, 1));
    CREATE INDEX IF NOT EXISTS library_tracks_artist_title_idx
        ON library_tracks(artist COLLATE NOCASE, title COLLATE NOCASE);
    PRAGMA user_version = 12;
    COMMIT;
"#;

const MIGRATE_VERSION_12_TO_13: &str = r#"
    BEGIN IMMEDIATE;
    ALTER TABLE timeline_clips
        ADD COLUMN tempo_anchor_beat INTEGER NOT NULL DEFAULT 0
        CHECK (tempo_anchor_beat >= 0);
    UPDATE timeline_clips SET tempo_anchor_beat = anchor_beat;
    PRAGMA user_version = 13;
    COMMIT;
"#;

const MIGRATE_VERSION_13_TO_14: &str = r#"
    BEGIN IMMEDIATE;
    CREATE TABLE IF NOT EXISTS timeline_filter_nodes (
        id      INTEGER PRIMARY KEY,
        lane    INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 2),
        beat    REAL NOT NULL CHECK (beat >= 0.0),
        value   REAL NOT NULL CHECK (value BETWEEN -1.0 AND 1.0),
        tension REAL NOT NULL DEFAULT 0.0 CHECK (tension BETWEEN -1.0 AND 1.0),
        UNIQUE (lane, beat)
    );
    CREATE INDEX IF NOT EXISTS timeline_filter_nodes_lane_beat_idx
        ON timeline_filter_nodes(lane, beat);
    PRAGMA user_version = 14;
    COMMIT;
"#;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryTrack {
    id: i64,
    file_path: String,
    file_name: String,
    artist: Option<String>,
    title: Option<String>,
    duration_ms: u64,
    sample_rate: u32,
    channels: u16,
    bpm: Option<f64>,
    analyzed_bpm: Option<f64>,
    bpm_confidence: Option<f64>,
    first_beat_ms: Option<u64>,
    analyzed_first_beat_ms: Option<u64>,
    beat_count: u64,
    is_corrected: bool,
    analysis_status: String,
    analysis_error: Option<String>,
    analysis_version: u32,
    is_missing: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryImportResult {
    tracks: Vec<LibraryTrack>,
    added_count: usize,
    added_track_ids: Vec<i64>,
    duplicate_count: usize,
    failed_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisBatchResult {
    pub tracks: Vec<LibraryTrack>,
    pub analyzed_count: usize,
    pub failed_count: usize,
}

#[derive(Clone, Debug)]
pub struct AnalysisTarget {
    pub id: i64,
    pub file_path: PathBuf,
}

#[derive(Debug)]
struct NewLibraryTrack {
    file_path: String,
    path_key: String,
    file_name: String,
    artist: Option<String>,
    title: Option<String>,
    duration_ms: u64,
    sample_rate: u32,
    channels: u16,
}

#[derive(Debug)]
struct DiscoveredPaths {
    files: Vec<PathBuf>,
    duplicate_count: usize,
    failed_count: usize,
}

pub struct LibraryStore {
    pub(crate) connection: Connection,
}

impl LibraryStore {
    pub fn open(database_path: &Path) -> Result<Self, String> {
        let connection = Connection::open(database_path)
            .map_err(|error| format!("Could not open the library: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("Could not configure the library: {error}"))?;
        connection
            .execute_batch(DATABASE_PRAGMAS)
            .map_err(|error| format!("Could not initialize the library: {error}"))?;
        initialize_database(&connection)?;
        connection
            .execute(
                "UPDATE library_tracks
                 SET analysis_status = 'not_analyzed'
                 WHERE analysis_status = 'analyzing'",
                [],
            )
            .map_err(database_write_error)?;

        let mut store = Self { connection };
        store.backfill_id3_metadata()?;
        Ok(store)
    }

    /// Chemin du fichier d'un morceau, pour les opérations qui doivent relire
    /// l'audio sans garder le verrou de la bibliothèque pendant le décodage.
    pub fn track_path(&self, id: i64) -> Result<String, String> {
        self.connection
            .query_row(
                "SELECT file_path FROM library_tracks WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(database_read_error)?
            .ok_or_else(|| "This track is no longer in the library.".to_owned())
    }

    pub fn list_tracks(&self) -> Result<Vec<LibraryTrack>, String> {
        self.query_tracks(None)
    }

    /// Une piste seule, telle que la bibliothèque la présente.
    ///
    /// Passe par la même lecture que `list_tracks` : la rangée brute ne devient
    /// une `LibraryTrack` qu'au prix d'une dizaine de règles — le BPM manuel qui
    /// masque l'analysé, le compte de temps recalculé quand on a corrigé, le
    /// fichier disparu — et une seconde copie de ce raisonnement finirait par
    /// s'écarter de la première sans que rien ne le signale.
    pub fn track(&self, id: i64) -> Result<Option<LibraryTrack>, String> {
        Ok(self.query_tracks(Some(id))?.into_iter().next())
    }

    fn query_tracks(&self, only: Option<i64>) -> Result<Vec<LibraryTrack>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, file_path, file_name, artist, title, duration_ms, sample_rate, channels,
                        bpm, manual_bpm, bpm_confidence,
                        first_beat_ms, manual_first_beat_ms, beat_count,
                        analysis_status, analysis_error, analysis_version
                 FROM library_tracks
                 WHERE ?1 IS NULL OR id = ?1
                 ORDER BY COALESCE(NULLIF(artist, ''), file_name) COLLATE NOCASE,
                          COALESCE(NULLIF(title, ''), file_name) COLLATE NOCASE, id",
            )
            .map_err(database_read_error)?;

        let rows = statement
            .query_map([only], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<f64>>(8)?,
                    row.get::<_, Option<f64>>(9)?,
                    row.get::<_, Option<f64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, i64>(16)?,
                ))
            })
            .map_err(database_read_error)?;

        let mut tracks = Vec::new();
        for row in rows {
            let (
                id,
                file_path,
                file_name,
                artist,
                title,
                duration_ms,
                sample_rate,
                channels,
                analyzed_bpm,
                manual_bpm,
                bpm_confidence,
                analyzed_first_beat_ms,
                manual_first_beat_ms,
                analyzed_beat_count,
                analysis_status,
                analysis_error,
                analysis_version,
            ) = row.map_err(database_read_error)?;
            let duration_ms = u64::try_from(duration_ms).unwrap_or_default();
            let analyzed_first_beat_ms =
                analyzed_first_beat_ms.and_then(|value| u64::try_from(value).ok());
            let manual_first_beat_ms =
                manual_first_beat_ms.and_then(|value| u64::try_from(value).ok());
            let bpm = manual_bpm.or(analyzed_bpm);
            let first_beat_ms = manual_first_beat_ms.or(analyzed_first_beat_ms);
            let is_corrected = manual_bpm.is_some() || manual_first_beat_ms.is_some();
            let beat_count = if is_corrected {
                effective_beat_count(duration_ms, bpm, first_beat_ms)
            } else {
                u64::try_from(analyzed_beat_count).unwrap_or_default()
            };

            tracks.push(LibraryTrack {
                id,
                is_missing: !Path::new(&file_path).is_file(),
                file_path,
                file_name,
                artist,
                title,
                duration_ms,
                sample_rate: u32::try_from(sample_rate).unwrap_or_default(),
                channels: u16::try_from(channels).unwrap_or_default(),
                bpm,
                analyzed_bpm,
                bpm_confidence,
                first_beat_ms,
                analyzed_first_beat_ms,
                beat_count,
                is_corrected,
                analysis_status,
                analysis_error,
                analysis_version: u32::try_from(analysis_version).unwrap_or_default(),
            });
        }

        Ok(tracks)
    }

    pub fn import_paths(&mut self, raw_paths: Vec<String>) -> Result<LibraryImportResult, String> {
        let discovered = discover_mp3_paths(raw_paths);
        let existing_keys = self.existing_path_keys()?;
        let mut duplicate_count = discovered.duplicate_count;
        let mut failed_count = discovered.failed_count;
        let mut candidates = Vec::new();

        for path in discovered.files {
            let path_key = normalize_path_key(&path);
            if existing_keys.contains(&path_key) {
                duplicate_count += 1;
                continue;
            }

            match inspect_mp3(&path) {
                Ok(metadata) => candidates.push(NewLibraryTrack {
                    file_name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Untitled track".to_owned()),
                    file_path: path.to_string_lossy().into_owned(),
                    path_key,
                    artist: metadata.artist,
                    title: metadata.title,
                    duration_ms: duration_millis(metadata.duration),
                    sample_rate: metadata.sample_rate,
                    channels: metadata.channels,
                }),
                Err(_) => failed_count += 1,
            }
        }

        let transaction = self
            .connection
            .transaction()
            .map_err(database_write_error)?;
        let mut added_count = 0;
        let mut added_track_ids = Vec::new();

        for track in candidates {
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO library_tracks
                     (file_path, path_key, file_name, artist, title, id3_scanned,
                      duration_ms, sample_rate, channels)
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8)",
                    params![
                        track.file_path,
                        track.path_key,
                        track.file_name,
                        track.artist,
                        track.title,
                        saturating_i64(track.duration_ms),
                        i64::from(track.sample_rate),
                        i64::from(track.channels),
                    ],
                )
                .map_err(database_write_error)?;

            if inserted == 1 {
                added_count += 1;
                added_track_ids.push(transaction.last_insert_rowid());
            } else {
                duplicate_count += 1;
            }
        }

        transaction.commit().map_err(database_write_error)?;

        Ok(LibraryImportResult {
            tracks: self.list_tracks()?,
            added_count,
            added_track_ids,
            duplicate_count,
            failed_count,
        })
    }

    fn backfill_id3_metadata(&mut self) -> Result<(), String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, file_path
                 FROM library_tracks
                 WHERE id3_scanned = 0",
            )
            .map_err(database_read_error)?;
        let pending = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(database_read_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_read_error)?;
        drop(statement);

        let discovered = pending
            .into_iter()
            .filter_map(|(id, file_path)| {
                Path::new(&file_path)
                    .is_file()
                    .then(|| (id, read_mp3_id3_tags(Path::new(&file_path))))
            })
            .collect::<Vec<_>>();
        if discovered.is_empty() {
            return Ok(());
        }

        let transaction = self
            .connection
            .transaction()
            .map_err(database_write_error)?;
        for (id, tags) in discovered {
            transaction
                .execute(
                    "UPDATE library_tracks
                     SET artist = ?1, title = ?2, id3_scanned = 1
                     WHERE id = ?3",
                    params![tags.artist, tags.title, id],
                )
                .map_err(database_write_error)?;
        }
        transaction.commit().map_err(database_write_error)
    }

    /// Vide la bibliothèque et, avec elle, tout ce qui en descend.
    ///
    /// Les clips, les formes d'onde, les corrections de beatgrid et les stems
    /// partent en cascade avec leurs morceaux. Ce qui n'en descend pas — les
    /// automations de voie, l'état des pistes — est effacé à part : ce sont des
    /// réglages du projet, pas de la bibliothèque, et les laisser derrière
    /// donnerait une timeline vide portant encore les gestes de la précédente.
    ///
    /// Les fichiers audio de l'utilisateur ne sont jamais touchés : la
    /// bibliothèque ne les contient pas, elle les désigne.
    pub fn clear_everything(&self) -> Result<(), String> {
        self.connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 DELETE FROM library_tracks;
                 DELETE FROM timeline_volume_nodes;
                 DELETE FROM timeline_pan_nodes;
                 DELETE FROM timeline_filter_nodes;
                 UPDATE timeline_lanes SET is_muted = 0, is_solo = 0;
                 COMMIT;",
            )
            .map_err(database_write_error)
    }

    pub fn remove_track(&self, id: i64) -> Result<Vec<LibraryTrack>, String> {
        self.connection
            .execute("DELETE FROM library_tracks WHERE id = ?1", [id])
            .map_err(database_write_error)?;
        self.list_tracks()
    }

    pub fn analysis_targets(&self, ids: &[i64]) -> Result<Vec<AnalysisTarget>, String> {
        let requested = ids.iter().copied().collect::<HashSet<_>>();
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, file_path
                 FROM library_tracks
                 ORDER BY file_name COLLATE NOCASE, id",
            )
            .map_err(database_read_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(database_read_error)?;
        let mut targets = Vec::new();

        for row in rows {
            let (id, file_path) = row.map_err(database_read_error)?;
            if requested.contains(&id) {
                targets.push(AnalysisTarget {
                    id,
                    file_path: PathBuf::from(file_path),
                });
            }
        }

        Ok(targets)
    }

    pub fn library_waveform_targets(&self) -> Result<Vec<AnalysisTarget>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT DISTINCT tracks.id, tracks.file_path
                 FROM library_tracks AS tracks
                 LEFT JOIN track_waveforms AS waveforms ON waveforms.track_id = tracks.id
                 WHERE waveforms.track_id IS NULL
                   AND tracks.analysis_status <> 'analyzing'
                 ORDER BY tracks.file_name COLLATE NOCASE, tracks.id",
            )
            .map_err(database_read_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(AnalysisTarget {
                    id: row.get(0)?,
                    file_path: PathBuf::from(row.get::<_, String>(1)?),
                })
            })
            .map_err(database_read_error)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(database_read_error)
    }

    pub fn mark_analysis_running(&mut self, id: i64) -> Result<(), String> {
        let transaction = self
            .connection
            .transaction()
            .map_err(database_write_error)?;
        transaction
            .execute("DELETE FROM track_beats WHERE track_id = ?1", [id])
            .map_err(database_write_error)?;
        transaction
            .execute("DELETE FROM track_waveforms WHERE track_id = ?1", [id])
            .map_err(database_write_error)?;
        transaction
            .execute(
                "UPDATE library_tracks
                 SET bpm = NULL,
                     bpm_confidence = NULL,
                     first_beat_ms = NULL,
                     beat_count = 0,
                     analysis_status = 'analyzing',
                     analysis_error = NULL
                 WHERE id = ?1",
                [id],
            )
            .map_err(database_write_error)?;
        transaction.commit().map_err(database_write_error)
    }

    pub fn save_analysis(&mut self, id: i64, analysis: &BeatAnalysis) -> Result<(), String> {
        let transaction = self
            .connection
            .transaction()
            .map_err(database_write_error)?;
        transaction
            .execute("DELETE FROM track_beats WHERE track_id = ?1", [id])
            .map_err(database_write_error)?;

        {
            let mut insert_beat = transaction
                .prepare(
                    "INSERT INTO track_beats (track_id, beat_index, position_ms)
                     VALUES (?1, ?2, ?3)",
                )
                .map_err(database_write_error)?;

            for (index, position_ms) in analysis.beats_ms.iter().enumerate() {
                insert_beat
                    .execute(params![
                        id,
                        saturating_i64(index as u64),
                        saturating_i64(*position_ms),
                    ])
                    .map_err(database_write_error)?;
            }
        }

        save_waveform_in_transaction(&transaction, id, &analysis.waveform)?;

        transaction
            .execute(
                "UPDATE library_tracks
                 SET bpm = ?2,
                     bpm_confidence = ?3,
                     first_beat_ms = ?4,
                     beat_count = ?5,
                     analysis_status = 'analyzed',
                     analysis_error = NULL,
                     analysis_version = ?6
                 WHERE id = ?1",
                params![
                    id,
                    analysis.bpm,
                    analysis.confidence,
                    saturating_i64(analysis.first_beat_ms),
                    saturating_i64(analysis.beats_ms.len() as u64),
                    i64::from(ANALYSIS_ALGORITHM_VERSION),
                ],
            )
            .map_err(database_write_error)?;
        transaction.commit().map_err(database_write_error)
    }

    pub fn save_waveform(&mut self, id: i64, waveform: &WaveformPeaks) -> Result<(), String> {
        let transaction = self
            .connection
            .transaction()
            .map_err(database_write_error)?;
        save_waveform_in_transaction(&transaction, id, waveform)?;
        transaction.commit().map_err(database_write_error)
    }

    pub fn mark_analysis_error(&self, id: i64, error: &str) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE library_tracks
                 SET analysis_status = 'error', analysis_error = ?2, analysis_version = ?3
                 WHERE id = ?1",
                params![id, error, i64::from(ANALYSIS_ALGORITHM_VERSION)],
            )
            .map_err(database_write_error)?;
        Ok(())
    }

    pub fn update_beatgrid_correction(
        &self,
        id: i64,
        bpm: f64,
        first_beat_ms: u64,
    ) -> Result<Vec<LibraryTrack>, String> {
        if !bpm.is_finite() || !(40.0..=300.0).contains(&bpm) {
            return Err("A manual BPM has to be between 40 and 300.".to_owned());
        }

        let duration_ms = self
            .connection
            .query_row(
                "SELECT duration_ms FROM library_tracks WHERE id = ?1",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(database_read_error)?;
        let duration_ms = u64::try_from(duration_ms).unwrap_or_default();
        if first_beat_ms > duration_ms {
            return Err("The first beat cannot fall past the end of the track.".to_owned());
        }

        let analyzed = self
            .connection
            .query_row(
                "SELECT bpm, first_beat_ms FROM library_tracks WHERE id = ?1",
                [id],
                |row| Ok((row.get::<_, Option<f64>>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .map_err(database_read_error)?;
        let first_beat_ms = snap_to_analyzed_grid(analyzed.0, analyzed.1, bpm, first_beat_ms);

        self.connection
            .execute(
                "UPDATE library_tracks
                 SET manual_bpm = ?2, manual_first_beat_ms = ?3
                 WHERE id = ?1",
                params![
                    id,
                    (bpm * 1_000.0).round() / 1_000.0,
                    saturating_i64(first_beat_ms),
                ],
            )
            .map_err(database_write_error)?;
        self.list_tracks()
    }

    pub fn reset_beatgrid_correction(&self, id: i64) -> Result<Vec<LibraryTrack>, String> {
        self.connection
            .execute(
                "UPDATE library_tracks
                 SET manual_bpm = NULL, manual_first_beat_ms = NULL
                 WHERE id = ?1",
                [id],
            )
            .map_err(database_write_error)?;
        self.list_tracks()
    }

    fn existing_path_keys(&self) -> Result<HashSet<String>, String> {
        let mut statement = self
            .connection
            .prepare("SELECT path_key FROM library_tracks")
            .map_err(database_read_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(database_read_error)?;

        rows.collect::<Result<HashSet<_>, _>>()
            .map_err(database_read_error)
    }
}

fn discover_mp3_paths(raw_paths: Vec<String>) -> DiscoveredPaths {
    let mut pending = raw_paths.into_iter().map(PathBuf::from).collect::<Vec<_>>();
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut duplicate_count = 0;
    let mut failed_count = 0;

    while let Some(path) = pending.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                failed_count += 1;
                continue;
            }
        };

        if metadata.is_dir() {
            match fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => pending.push(entry.path()),
                            Err(_) => failed_count += 1,
                        }
                    }
                }
                Err(_) => failed_count += 1,
            }
            continue;
        }

        if !metadata.is_file() || !is_mp3_path(&path) {
            continue;
        }

        let key = normalize_path_key(&path);
        if seen.insert(key) {
            files.push(path);
        } else {
            duplicate_count += 1;
        }
    }

    DiscoveredPaths {
        files,
        duplicate_count,
        failed_count,
    }
}

fn is_mp3_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"))
}

pub(crate) fn normalize_path_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('/', "\\");

    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn effective_beat_count(duration_ms: u64, bpm: Option<f64>, first_beat_ms: Option<u64>) -> u64 {
    let (Some(bpm), Some(first_beat_ms)) = (bpm, first_beat_ms) else {
        return 0;
    };
    if !bpm.is_finite() || bpm <= 0.0 || first_beat_ms > duration_ms {
        return 0;
    }

    (((duration_ms - first_beat_ms) as f64 / (60_000.0 / bpm)).floor() as u64).saturating_add(1)
}

pub(crate) fn database_read_error(error: rusqlite::Error) -> String {
    format!("Could not read the library: {error}")
}

pub(crate) fn database_write_error(error: rusqlite::Error) -> String {
    format!("Could not write to the library: {error}")
}

pub(crate) fn save_waveform_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    track_id: i64,
    waveform: &WaveformPeaks,
) -> Result<(), String> {
    let bucket_count = waveform.left_min.len();
    if bucket_count == 0
        || bucket_count > WAVEFORM_BUCKET_COUNT
        || waveform.left_max.len() != bucket_count
        || waveform.left_rms.len() != bucket_count
        || waveform.right_min.len() != bucket_count
        || waveform.right_max.len() != bucket_count
        || waveform.right_rms.len() != bucket_count
    {
        return Err("The computed waveform is not valid.".to_owned());
    }

    transaction
        .execute(
            "INSERT INTO track_waveforms
             (track_id, bucket_count, left_min, left_max, left_rms,
              right_min, right_max, right_rms, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())
             ON CONFLICT(track_id) DO UPDATE SET
                 bucket_count = excluded.bucket_count,
                 left_min = excluded.left_min,
                 left_max = excluded.left_max,
                 left_rms = excluded.left_rms,
                 right_min = excluded.right_min,
                 right_max = excluded.right_max,
                 right_rms = excluded.right_rms,
                 generated_at = excluded.generated_at",
            params![
                track_id,
                saturating_i64(bucket_count as u64),
                encode_waveform_values(&waveform.left_min),
                encode_waveform_values(&waveform.left_max),
                encode_waveform_values(&waveform.left_rms),
                encode_waveform_values(&waveform.right_min),
                encode_waveform_values(&waveform.right_max),
                encode_waveform_values(&waveform.right_rms),
            ],
        )
        .map_err(database_write_error)?;
    Ok(())
}

pub(crate) fn encode_waveform_values(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_waveform_values(blob: &[u8], bucket_count: usize) -> Option<Vec<f32>> {
    if blob.len() != bucket_count.checked_mul(size_of::<f32>())? {
        return None;
    }

    blob.chunks_exact(size_of::<f32>())
        .map(|chunk| Some(f32::from_le_bytes(chunk.try_into().ok()?)))
        .collect()
}

pub(crate) fn decode_waveform(
    bucket_count: Option<i64>,
    left_min: Option<Vec<u8>>,
    left_max: Option<Vec<u8>>,
    left_rms: Option<Vec<u8>>,
    right_min: Option<Vec<u8>>,
    right_max: Option<Vec<u8>>,
    right_rms: Option<Vec<u8>>,
) -> Option<WaveformPeaks> {
    let bucket_count = usize::try_from(bucket_count?).ok()?;
    if bucket_count == 0 || bucket_count > WAVEFORM_BUCKET_COUNT {
        return None;
    }

    Some(WaveformPeaks {
        left_min: decode_waveform_values(&left_min?, bucket_count)?,
        left_max: decode_waveform_values(&left_max?, bucket_count)?,
        left_rms: decode_waveform_values(&left_rms?, bucket_count)?,
        right_min: decode_waveform_values(&right_min?, bucket_count)?,
        right_max: decode_waveform_values(&right_max?, bucket_count)?,
        right_rms: decode_waveform_values(&right_rms?, bucket_count)?,
    })
}

/// Quantises a hand-entered first beat onto the analysed grid.
///
/// Nobody clicks or taps exactly on a beat, and a downbeat a hundred
/// milliseconds off makes every clip built on it drift. The analysis already
/// knows where the beats are, so the correction only has to say *which* beat is
/// beat one; the position itself is read off the grid.
///
/// The grid is extended arithmetically rather than looked up in the stored
/// beats, so a downbeat placed before the analysed first beat — the whole point
/// of the correction when a track opens without drums — snaps to the right
/// beat instead of jumping forward to the first stored one.
///
/// If the BPM was corrected too, the analysed downbeat still supplies the
/// phase origin and the corrected period supplies the spacing. Taking the
/// mouse position literally in that case was the reason "Set to" appeared not
/// to snap precisely when the automatic BPM was wrong.
fn snap_to_analyzed_grid(
    analyzed_bpm: Option<f64>,
    analyzed_first_beat_ms: Option<i64>,
    manual_bpm: f64,
    requested_ms: u64,
) -> u64 {
    let (Some(analyzed_bpm), Some(analyzed_first_beat_ms)) = (analyzed_bpm, analyzed_first_beat_ms)
    else {
        return requested_ms;
    };
    if !analyzed_bpm.is_finite()
        || !(40.0..=300.0).contains(&analyzed_bpm)
        || analyzed_first_beat_ms < 0
        // A small BPM correction keeps the analysed phase useful. A half-time,
        // double-time or otherwise radical reinterpretation does not: which
        // old beat becomes the new bar one is a musical choice, so preserve
        // the position the user captured instead of moving it unexpectedly.
        || ((manual_bpm / analyzed_bpm) - 1.0).abs() > 0.15
    {
        return requested_ms;
    }

    let period_ms = 60_000.0 / manual_bpm;
    let origin_ms = analyzed_first_beat_ms as f64;
    let beats = ((requested_ms as f64 - origin_ms) / period_ms).round();
    let snapped = origin_ms + beats * period_ms;
    if snapped < 0.0 {
        return requested_ms;
    }
    snapped.round() as u64
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    col_type_and_constraints: &str,
) -> Result<(), String> {
    let pragma_sql = format!("PRAGMA table_info({table})");
    let mut stmt = connection
        .prepare(&pragma_sql)
        .map_err(database_read_error)?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;

    if !columns.iter().any(|c| c == column) {
        let alter_sql =
            format!("ALTER TABLE {table} ADD COLUMN {column} {col_type_and_constraints};");
        connection
            .execute_batch(&alter_sql)
            .map_err(database_write_error)?;
    }
    Ok(())
}

fn initialize_database(connection: &Connection) -> Result<(), String> {
    let mut version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(database_read_error)?;

    if version == 0 {
        return connection
            .execute_batch(CURRENT_DATABASE_SCHEMA)
            .map_err(|error| format!("Could not create the library: {error}"));
    }
    if !(1..=LATEST_SCHEMA_VERSION).contains(&version) {
        return Err(format!(
            "The library uses an unsupported schema version ({version})."
        ));
    }

    while version < LATEST_SCHEMA_VERSION {
        match version {
            14 => {
                ensure_column(connection, "timeline_clips", "eq_settings", "TEXT")?;
                connection
                    .execute_batch("PRAGMA user_version = 15;")
                    .map_err(database_write_error)?;
                version = 15;
            }
            15 => {
                // The constraints match CURRENT_DATABASE_SCHEMA so an upgraded
                // database and a fresh one accept exactly the same values.
                ensure_column(
                    connection,
                    "timeline_clips",
                    "trim_start_beats",
                    "REAL NOT NULL DEFAULT 0.0 CHECK (trim_start_beats >= 0.0)",
                )?;
                ensure_column(
                    connection,
                    "timeline_clips",
                    "trim_end_beats",
                    "REAL NOT NULL DEFAULT 0.0 CHECK (trim_end_beats >= 0.0)",
                )?;
                connection
                    .execute_batch("PRAGMA user_version = 16;")
                    .map_err(database_write_error)?;
                version = 16;
            }
            16 => {
                ensure_column(
                    connection,
                    "project_settings",
                    "limiter_enabled",
                    "INTEGER NOT NULL DEFAULT 1 CHECK (limiter_enabled IN (0, 1))",
                )?;
                connection
                    .execute_batch("PRAGMA user_version = 17;")
                    .map_err(database_write_error)?;
                version = 17;
            }
            17 => {
                // Off by default: turning it on for existing projects would
                // change how every saved mix sounds without being asked.
                ensure_column(
                    connection,
                    "project_settings",
                    "compressor_enabled",
                    "INTEGER NOT NULL DEFAULT 0 CHECK (compressor_enabled IN (0, 1))",
                )?;
                connection
                    .execute_batch("PRAGMA user_version = 18;")
                    .map_err(database_write_error)?;
                version = 18;
            }
            18 => {
                // Off by default, like the other master processors: turning a
                // saved mix into a pumping one unasked would be a surprise.
                ensure_column(
                    connection,
                    "project_settings",
                    "ducking_enabled",
                    "INTEGER NOT NULL DEFAULT 0 CHECK (ducking_enabled IN (0, 1))",
                )?;
                ensure_column(
                    connection,
                    "timeline_clips",
                    "is_sidechain_key",
                    "INTEGER NOT NULL DEFAULT 0 CHECK (is_sidechain_key IN (0, 1))",
                )?;
                connection
                    .execute_batch("PRAGMA user_version = 19;")
                    .map_err(database_write_error)?;
                version = 19;
            }
            19 => {
                // The project-wide ducking switch turned out to duplicate the
                // key itself: a clip either holds the key or it does not.
                connection
                    .execute_batch(
                        "BEGIN IMMEDIATE;
                         ALTER TABLE project_settings DROP COLUMN ducking_enabled;
                         PRAGMA user_version = 20;
                         COMMIT;",
                    )
                    .map_err(database_write_error)?;
                version = 20;
            }
            20 => {
                // L'automation de panoramique, calquée sur celle du volume :
                // une valeur bipolaire par piste, −1 à gauche, +1 à droite.
                connection
                    .execute_batch(
                        "BEGIN IMMEDIATE;
                         CREATE TABLE IF NOT EXISTS timeline_pan_nodes (
                             id    INTEGER PRIMARY KEY,
                             lane  INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 2),
                             beat  REAL NOT NULL CHECK (beat >= 0.0),
                             value REAL NOT NULL CHECK (value BETWEEN -1.0 AND 1.0),
                             UNIQUE (lane, beat)
                         );
                         CREATE INDEX IF NOT EXISTS timeline_pan_nodes_lane_beat_idx
                             ON timeline_pan_nodes(lane, beat);
                         PRAGMA user_version = 21;
                         COMMIT;",
                    )
                    .map_err(database_write_error)?;
                version = 21;
            }
            21 => {
                // Les stems : un fichier par voix séparée, rattaché au morceau
                // dont il sort. Le clip ne retient que lequel il joue — la
                // séparation appartient au morceau, de sorte qu'un second clip
                // du même morceau bascule sans rien recalculer.
                connection
                    .execute_batch(
                        "BEGIN IMMEDIATE;
                         CREATE TABLE IF NOT EXISTS track_stems (
                             id               INTEGER PRIMARY KEY,
                             library_track_id INTEGER NOT NULL
                                              REFERENCES library_tracks(id) ON DELETE CASCADE,
                             kind             TEXT NOT NULL
                                              CHECK (kind IN ('vocals', 'instrumental')),
                             file_path        TEXT NOT NULL,
                             waveform         BLOB,
                             created_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                             UNIQUE (library_track_id, kind)
                         );
                         COMMIT;",
                    )
                    .map_err(database_write_error)?;
                ensure_column(
                    connection,
                    "timeline_clips",
                    "stem",
                    "TEXT NOT NULL DEFAULT 'full' CHECK (stem IN ('full', 'vocals', 'instrumental'))",
                )?;
                connection
                    .execute_batch("PRAGMA user_version = 22;")
                    .map_err(database_write_error)?;
                version = 22;
            }
            22 => {
                // La séparation redescend du morceau au clip.
                //
                // Séparer six minutes de musique pour huit mesures utilisées,
                // c'est payer vingt fois le travail sur une longue timeline. Le
                // stem ne couvre donc que la fenêtre du clip, et retient où
                // elle commence dans la source — sans quoi la géométrie du
                // clip, calculée depuis le fichier d'origine, se décalerait
                // d'autant.
                connection
                    .execute_batch(
                        "BEGIN IMMEDIATE;
                         DROP TABLE IF EXISTS track_stems;
                         CREATE TABLE IF NOT EXISTS clip_stems (
                             id             INTEGER PRIMARY KEY,
                             clip_id        INTEGER NOT NULL
                                            REFERENCES timeline_clips(id) ON DELETE CASCADE,
                             kind           TEXT NOT NULL
                                            CHECK (kind IN ('vocals', 'instrumental')),
                             file_path      TEXT NOT NULL,
                             source_from_ms INTEGER NOT NULL DEFAULT 0
                                            CHECK (source_from_ms >= 0),
                             created_at     INTEGER NOT NULL DEFAULT (unixepoch()),
                             UNIQUE (clip_id, kind)
                         );
                         PRAGMA user_version = 23;
                         COMMIT;",
                    )
                    .map_err(database_write_error)?;
                version = 23;
            }
            23 => {
                // La forme d'onde du stem. Un clip qui joue la voix en montrant
                // le mix complet ment sur ce qu'on entend.
                for column in [
                    "bucket_count INTEGER",
                    "left_min BLOB",
                    "left_max BLOB",
                    "left_rms BLOB",
                    "right_min BLOB",
                    "right_max BLOB",
                    "right_rms BLOB",
                ] {
                    let (name, kind) = column.split_once(' ').unwrap_or((column, ""));
                    ensure_column(connection, "clip_stems", name, kind)?;
                }
                connection
                    .execute_batch("PRAGMA user_version = 24;")
                    .map_err(database_write_error)?;
                version = 24;
            }
            _ => {
                let (target_version, migration) = match version {
                    1 => (2, MIGRATE_VERSION_1_TO_2),
                    2 => (3, MIGRATE_VERSION_2_TO_3),
                    3 => (4, MIGRATE_VERSION_3_TO_4),
                    4 => (5, MIGRATE_VERSION_4_TO_5),
                    5 => (6, MIGRATE_VERSION_5_TO_6),
                    6 => (7, MIGRATE_VERSION_6_TO_7),
                    7 => (8, MIGRATE_VERSION_7_TO_8),
                    8 => (9, MIGRATE_VERSION_8_TO_9),
                    9 => (10, MIGRATE_VERSION_9_TO_10),
                    10 => (11, MIGRATE_VERSION_10_TO_11),
                    11 => (12, MIGRATE_VERSION_11_TO_12),
                    12 => (13, MIGRATE_VERSION_12_TO_13),
                    13 => (14, MIGRATE_VERSION_13_TO_14),
                    _ => unreachable!("schema version was validated"),
                };
                connection.execute_batch(migration).map_err(|error| {
                    format!("Could not migrate the library to version {target_version}: {error}")
                })?;
                version = target_version;
            }
        }
    }

    connection
        .execute_batch(CURRENT_DATABASE_SCHEMA)
        .map_err(|error| format!("Could not verify the library: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        LATEST_SCHEMA_VERSION, LibraryStore, decode_waveform, discover_mp3_paths,
        effective_beat_count, encode_waveform_values, normalize_path_key, snap_to_analyzed_grid,
    };
    use crate::analysis::{ANALYSIS_ALGORITHM_VERSION, BeatAnalysis, WaveformPeaks};
    use rusqlite::params;
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn library_survives_database_reopening() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-library-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");

        {
            let store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        fake_mp3.to_string_lossy(),
                        normalize_path_key(&fake_mp3),
                        "test.mp3",
                        123_000_i64,
                        44_100_i64,
                        2_i64,
                    ],
                )
                .expect("track should be inserted");
        }

        {
            let store = LibraryStore::open(&database_path).expect("database should reopen");
            let tracks = store.list_tracks().expect("tracks should be listed");

            assert_eq!(tracks.len(), 1);
            assert_eq!(tracks[0].file_name, "test.mp3");
            assert!(tracks[0].is_missing);
        }

        remove_database_files(&database_path);
    }

    /// The schema constant is replayed after every migration. If it still
    /// stamped an older version, reopening would push the database back down
    /// and replay the last migrations on each start, forever.
    /// The case that prompted this: a downbeat given as "about 61.9 seconds"
    /// on a 126 BPM track whose analysed grid starts at 61 946 ms.
    #[test]
    fn a_hand_entered_downbeat_is_quantised_onto_the_analysed_grid() {
        let analysed_bpm = Some(126.0);
        let analysed_first = Some(61_946);
        let period: f64 = 60_000.0 / 126.0;

        // Rough by a quarter of a beat in either direction: same beat.
        assert_eq!(
            snap_to_analyzed_grid(analysed_bpm, analysed_first, 126.0, 61_900),
            61_946
        );
        assert_eq!(
            snap_to_analyzed_grid(analysed_bpm, analysed_first, 126.0, 62_050),
            61_946
        );

        // Past the halfway mark it belongs to the next beat, as it should.
        let next = (61_946.0 + period).round() as u64;
        assert_eq!(
            snap_to_analyzed_grid(analysed_bpm, analysed_first, 126.0, 62_250),
            next
        );

        // The grid extends backwards, so a downbeat placed in a beatless intro
        // lands on the right beat instead of jumping to the analysed anchor.
        let earlier = (61_946.0 - period * 40.0).round() as u64;
        assert_eq!(
            snap_to_analyzed_grid(analysed_bpm, analysed_first, 126.0, earlier + 60),
            earlier
        );
    }

    #[test]
    fn corrected_tempo_still_snaps_around_the_known_downbeat_phase() {
        assert_eq!(
            snap_to_analyzed_grid(Some(126.0), Some(61_946), 124.0, 61_900),
            61_946
        );
        assert_eq!(
            snap_to_analyzed_grid(Some(126.0), Some(61_946), 124.0, 62_250),
            (61_946.0_f64 + 60_000.0 / 124.0).round() as u64
        );
    }

    #[test]
    fn an_unanalysed_track_keeps_the_entered_position() {
        assert_eq!(
            snap_to_analyzed_grid(None, Some(1_000), 128.0, 1_234),
            1_234
        );
        assert_eq!(
            snap_to_analyzed_grid(Some(128.0), None, 128.0, 1_234),
            1_234
        );
        assert_eq!(
            snap_to_analyzed_grid(Some(f64::NAN), Some(0), 128.0, 1_234),
            1_234
        );
        // Near the start of the file the nearest beat is simply beat zero.
        assert_eq!(snap_to_analyzed_grid(Some(128.0), Some(0), 128.0, 100), 0);
        // But a snap that would land before the file starts is refused: with
        // the grid anchored at 300 ms, the beat nearest 10 ms sits at -169 ms.
        assert_eq!(snap_to_analyzed_grid(Some(128.0), Some(300), 128.0, 10), 10);
    }

    /// Vider la bibliothèque doit tout emporter, y compris ce qui n'en descend
    /// pas directement.
    ///
    /// Les clips partent en cascade avec leurs morceaux; les automations de
    /// voie, elles, n'ont pas de clé étrangère vers eux. Les oublier laisserait
    /// une timeline vide portant encore les gestes de la précédente — et c'est
    /// exactement l'oubli que ce projet a déjà commis deux fois ailleurs.
    #[test]
    fn clearing_everything_leaves_no_trace_of_the_previous_session() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-wipe-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'wipe.mp3', 60000, 44100, 2, 120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();
            store
                .connection
                .execute_batch(&format!(
                    "INSERT INTO timeline_clips (library_track_id, lane, anchor_beat, tempo_anchor_beat)
                     VALUES ({track_id}, 0, 4, 4);
                     INSERT INTO timeline_volume_nodes (lane, beat, gain_db) VALUES (0, 8.0, -6.0);
                     INSERT INTO timeline_pan_nodes (lane, beat, value) VALUES (1, 12.0, 0.5);
                     INSERT INTO timeline_filter_nodes (lane, beat, value) VALUES (2, 16.0, -0.5);
                     UPDATE timeline_lanes SET is_muted = 1 WHERE lane = 1;"
                ))
                .expect("a session should be seeded");

            store.clear_everything().expect("everything should clear");

            for table in [
                "library_tracks",
                "timeline_clips",
                "timeline_volume_nodes",
                "timeline_pan_nodes",
                "timeline_filter_nodes",
            ] {
                let left: i64 = store
                    .connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .expect("the table should be readable");
                assert_eq!(left, 0, "{table} devrait être vide");
            }
            let marked: i64 = store
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM timeline_lanes WHERE is_muted = 1 OR is_solo = 1",
                    [],
                    |row| row.get(0),
                )
                .expect("lane states should be readable");
            assert_eq!(marked, 0, "les pistes devraient revenir à leur état neutre");
        }

        fs::remove_file(&fake_mp3).expect("fake MP3 should be removed");
        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            if candidate.exists() {
                fs::remove_file(candidate).expect("test database should be removed");
            }
        }
    }

    #[test]
    fn a_reopened_database_settles_on_the_latest_schema_version() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-schema-{}-{suffix}.sqlite3",
            std::process::id()
        ));

        {
            let store = LibraryStore::open(&database_path).expect("database should be created");
            let version: i64 = store
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .expect("version should read");
            assert_eq!(version, LATEST_SCHEMA_VERSION);
        }

        // Simulate a database left at the version the schema used to stamp,
        // without the columns the later migrations add.
        {
            let store = LibraryStore::open(&database_path).expect("database should reopen");
            store
                .connection
                .execute_batch(
                    "ALTER TABLE timeline_clips DROP COLUMN trim_end_beats;
                     ALTER TABLE timeline_clips DROP COLUMN trim_start_beats;
                     ALTER TABLE timeline_clips DROP COLUMN eq_settings;
                     PRAGMA user_version = 14;",
                )
                .expect("database should be rolled back to schema 14");
        }

        {
            let store = LibraryStore::open(&database_path).expect("database should migrate");
            let version: i64 = store
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .expect("version should read");
            assert_eq!(version, LATEST_SCHEMA_VERSION);

            let mut columns = store
                .connection
                .prepare("PRAGMA table_info(timeline_clips)")
                .expect("table info should read");
            let names = columns
                .query_map([], |row| row.get::<_, String>(1))
                .expect("columns should list")
                .collect::<Result<Vec<_>, _>>()
                .expect("columns should read");
            for expected in ["eq_settings", "trim_start_beats", "trim_end_beats"] {
                assert!(
                    names.iter().any(|name| name == expected),
                    "missing {expected}"
                );
            }

            // A column added by one migration and dropped by a later one has to
            // be gone once the chain has run all the way through.
            let mut settings = store
                .connection
                .prepare("PRAGMA table_info(project_settings)")
                .expect("table info should read");
            let settings_names = settings
                .query_map([], |row| row.get::<_, String>(1))
                .expect("columns should list")
                .collect::<Result<Vec<_>, _>>()
                .expect("columns should read");
            assert!(
                !settings_names.iter().any(|name| name == "ducking_enabled"),
                "the retired ducking switch should not survive the migrations"
            );
        }

        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            if candidate.exists() {
                fs::remove_file(candidate).expect("test database should be removed");
            }
        }
    }

    #[test]
    fn version_one_database_is_migrated_without_losing_tracks() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-migration-{}-{suffix}.sqlite3",
            std::process::id()
        ));

        {
            let connection = rusqlite::Connection::open(&database_path)
                .expect("version one database should open");
            connection
                .execute_batch(
                    "CREATE TABLE library_tracks (
                        id INTEGER PRIMARY KEY,
                        file_path TEXT NOT NULL,
                        path_key TEXT NOT NULL UNIQUE,
                        file_name TEXT NOT NULL,
                        duration_ms INTEGER NOT NULL,
                        sample_rate INTEGER NOT NULL,
                        channels INTEGER NOT NULL,
                        bpm REAL,
                        analysis_status TEXT NOT NULL DEFAULT 'not_analyzed',
                        added_at INTEGER NOT NULL DEFAULT (unixepoch())
                    );
                    INSERT INTO library_tracks
                        (file_path, path_key, file_name, duration_ms, sample_rate, channels)
                    VALUES ('missing.mp3', 'missing.mp3', 'migration.mp3', 60000, 44100, 2);
                    PRAGMA user_version = 1;",
                )
                .expect("version one schema should be created");
        }

        {
            let store = LibraryStore::open(&database_path).expect("database should migrate");
            let version = store
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .expect("schema version should be readable");
            let tracks = store
                .list_tracks()
                .expect("migrated track should be listed");

            assert_eq!(version, LATEST_SCHEMA_VERSION);
            assert_eq!(tracks.len(), 1);
            assert_eq!(tracks[0].file_name, "migration.mp3");
            assert_eq!(tracks[0].beat_count, 0);
        }

        remove_database_files(&database_path);
    }

    #[test]
    fn version_seven_clip_anchors_migrate_to_complete_measures() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-measure-migration-{}-{suffix}.sqlite3",
            std::process::id()
        ));

        {
            let store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES ('missing.mp3', 'measure-grid', 'measure.mp3', 60000, 44100, 2,
                             120.0, 500, 120, 'analyzed')",
                    [],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();
            store
                .connection
                .execute(
                    "INSERT INTO timeline_clips
                     (library_track_id, lane, anchor_beat, tempo_anchor_beat)
                     VALUES (?1, 0, 1, 1)",
                    [track_id],
                )
                .expect("legacy clip should be inserted");
            store
                .connection
                .execute_batch(
                    "DROP INDEX IF EXISTS library_tracks_artist_title_idx;
                     ALTER TABLE timeline_clips DROP COLUMN tempo_anchor_beat;
                     ALTER TABLE library_tracks DROP COLUMN id3_scanned;
                     ALTER TABLE library_tracks DROP COLUMN title;
                     ALTER TABLE library_tracks DROP COLUMN artist;
                     ALTER TABLE library_tracks DROP COLUMN analysis_version;
                     PRAGMA user_version = 7;",
                )
                .expect("legacy schema version should be simulated");
        }

        {
            let store = LibraryStore::open(&database_path).expect("database should migrate");
            let (version, anchor): (i64, i64) = (
                store
                    .connection
                    .query_row("PRAGMA user_version", [], |row| row.get(0))
                    .expect("schema version should be readable"),
                store
                    .connection
                    .query_row("SELECT anchor_beat FROM timeline_clips", [], |row| {
                        row.get(0)
                    })
                    .expect("clip anchor should be readable"),
            );

            assert_eq!(version, LATEST_SCHEMA_VERSION);
            assert_eq!(anchor, 4);
        }

        remove_database_files(&database_path);
    }

    #[test]
    fn folder_discovery_keeps_mp3_files_and_deduplicates_paths() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "mixcanvas-discovery-{}-{suffix}",
            std::process::id()
        ));
        let mp3_path = directory.join("morceau.MP3");
        let text_path = directory.join("notes.txt");

        fs::create_dir(&directory).expect("test directory should be created");
        fs::write(&mp3_path, []).expect("fake MP3 should be created");
        fs::write(&text_path, []).expect("text file should be created");

        let discovered = discover_mp3_paths(vec![
            directory.to_string_lossy().into_owned(),
            mp3_path.to_string_lossy().into_owned(),
        ]);

        assert_eq!(discovered.files, vec![mp3_path.clone()]);
        assert_eq!(discovered.duplicate_count, 1);
        assert_eq!(discovered.failed_count, 0);

        fs::remove_file(mp3_path).expect("fake MP3 should be removed");
        fs::remove_file(text_path).expect("text file should be removed");
        fs::remove_dir(directory).expect("test directory should be removed");
    }

    #[test]
    fn corrected_grid_count_uses_manual_bpm_and_first_beat() {
        assert_eq!(effective_beat_count(60_000, Some(120.0), Some(0)), 121);
        assert_eq!(effective_beat_count(60_000, Some(120.0), Some(500)), 120);
        assert_eq!(effective_beat_count(60_000, None, Some(0)), 0);
        assert_eq!(effective_beat_count(60_000, Some(120.0), None), 0);
    }

    #[test]
    fn waveform_blobs_round_trip_without_losing_signed_peaks() {
        let waveform = WaveformPeaks {
            left_min: vec![-1.0, -0.25],
            left_max: vec![0.5, 0.75],
            left_rms: vec![0.4, 0.6],
            right_min: vec![-0.75, -0.5],
            right_max: vec![0.25, 1.0],
            right_rms: vec![0.3, 0.7],
        };
        let decoded = decode_waveform(
            Some(2),
            Some(encode_waveform_values(&waveform.left_min)),
            Some(encode_waveform_values(&waveform.left_max)),
            Some(encode_waveform_values(&waveform.left_rms)),
            Some(encode_waveform_values(&waveform.right_min)),
            Some(encode_waveform_values(&waveform.right_max)),
            Some(encode_waveform_values(&waveform.right_rms)),
        )
        .expect("valid waveform should decode");

        assert_eq!(decoded.left_min, waveform.left_min);
        assert_eq!(decoded.left_max, waveform.left_max);
        assert_eq!(decoded.left_rms, waveform.left_rms);
        assert_eq!(decoded.right_min, waveform.right_min);
        assert_eq!(decoded.right_max, waveform.right_max);
        assert_eq!(decoded.right_rms, waveform.right_rms);
    }

    #[test]
    fn saved_analysis_is_stamped_with_the_current_algorithm_version() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-analysis-version-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let mut store = LibraryStore::open(&database_path).expect("database should open");
        store
            .connection
            .execute(
                "INSERT INTO library_tracks
                 (file_path, path_key, file_name, duration_ms, sample_rate, channels)
                 VALUES ('version.mp3', 'version.mp3', 'version.mp3', 60000, 44100, 2)",
                [],
            )
            .expect("track should be inserted");
        let id = store.connection.last_insert_rowid();
        store
            .save_analysis(
                id,
                &BeatAnalysis {
                    bpm: 128.0,
                    confidence: 0.9,
                    first_beat_ms: 250,
                    beats_ms: vec![250, 719],
                    waveform: WaveformPeaks {
                        left_min: vec![-1.0],
                        left_max: vec![1.0],
                        left_rms: vec![0.5],
                        right_min: vec![-1.0],
                        right_max: vec![1.0],
                        right_rms: vec![0.5],
                    },
                },
            )
            .expect("analysis should be saved");

        let tracks = store.list_tracks().expect("tracks should be listed");
        assert_eq!(tracks[0].analysis_version, ANALYSIS_ALGORITHM_VERSION);
        assert_eq!(tracks[0].bpm, Some(128.0));

        drop(store);
        remove_database_files(&database_path);
    }

    /// Ce que `track` renvoie doit être exactement ce que `list_tracks`
    /// aurait mis dans la liste, sans quoi une rangée publiée en cours de lot
    /// contredirait celle qui arrive à la fin.
    #[test]
    fn one_track_reads_exactly_as_it_does_in_the_full_list() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-single-track-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let mut store = LibraryStore::open(&database_path).expect("database should open");
        for name in ["alpha.mp3", "beta.mp3"] {
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels)
                     VALUES (?1, ?1, ?1, 60000, 44100, 2)",
                    params![name],
                )
                .expect("track should be inserted");
        }
        let wanted = store.connection.last_insert_rowid();
        // Une piste corrigée à la main : c'est là que la lecture fait le plus
        // de travail — le BPM manuel masque l'analysé et le compte de temps se
        // recalcule — donc c'est là qu'une seconde copie divergerait.
        store
            .save_analysis(
                wanted,
                &BeatAnalysis {
                    bpm: 128.0,
                    confidence: 0.9,
                    first_beat_ms: 250,
                    beats_ms: vec![250, 719],
                    waveform: WaveformPeaks {
                        left_min: vec![-1.0],
                        left_max: vec![1.0],
                        left_rms: vec![0.5],
                        right_min: vec![-1.0],
                        right_max: vec![1.0],
                        right_rms: vec![0.5],
                    },
                },
            )
            .expect("analysis should be saved");
        store
            .connection
            .execute(
                "UPDATE library_tracks SET manual_bpm = 174.0 WHERE id = ?1",
                params![wanted],
            )
            .expect("manual bpm should be stored");

        let from_list = store
            .list_tracks()
            .expect("tracks should be listed")
            .into_iter()
            .find(|track| track.id == wanted)
            .expect("the track should be in the list");
        let alone = store
            .track(wanted)
            .expect("the track should be readable")
            .expect("the track should exist");

        assert_eq!(alone.bpm, Some(174.0));
        assert!(alone.is_corrected);
        assert_eq!(alone.beat_count, from_list.beat_count);
        assert_eq!(alone.bpm, from_list.bpm);
        assert_eq!(alone.analyzed_bpm, from_list.analyzed_bpm);
        assert_eq!(alone.first_beat_ms, from_list.first_beat_ms);
        assert_eq!(alone.analysis_status, from_list.analysis_status);
        assert_eq!(alone.analysis_version, from_list.analysis_version);
        assert_eq!(alone.is_missing, from_list.is_missing);

        assert!(
            store
                .track(wanted + 1_000)
                .expect("a missing id should not be an error")
                .is_none(),
            "un identifiant inconnu doit répondre « rien », pas une erreur"
        );

        drop(store);
        remove_database_files(&database_path);
    }

    #[test]
    fn waveform_backfill_targets_the_library_without_requiring_a_timeline_clip() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-waveform-targets-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let mut store = LibraryStore::open(&database_path).expect("database should open");
        store
            .connection
            .execute(
                "INSERT INTO library_tracks
                 (file_path, path_key, file_name, duration_ms, sample_rate, channels)
                 VALUES ('first.mp3', 'first.mp3', 'first.mp3', 60000, 44100, 2),
                        ('second.mp3', 'second.mp3', 'second.mp3', 60000, 44100, 2)",
                [],
            )
            .expect("tracks should be inserted");
        let second_id = store.connection.last_insert_rowid();
        let first_id = second_id - 1;
        store
            .save_waveform(
                first_id,
                &WaveformPeaks {
                    left_min: vec![-1.0],
                    left_max: vec![1.0],
                    left_rms: vec![0.5],
                    right_min: vec![-1.0],
                    right_max: vec![1.0],
                    right_rms: vec![0.5],
                },
            )
            .expect("first waveform should be saved");

        let targets = store
            .library_waveform_targets()
            .expect("backfill targets should load");

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, second_id);
        drop(store);
        remove_database_files(&database_path);
    }

    #[test]
    fn version_nine_waveforms_are_invalidated_for_the_daw_format() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-waveform-v10-{}-{suffix}.sqlite3",
            std::process::id()
        ));

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      analysis_status)
                     VALUES ('missing.mp3', 'waveform-v10', 'waveform-v10.mp3',
                             60000, 44100, 2, 'analyzed')",
                    [],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();
            store
                .save_waveform(
                    track_id,
                    &WaveformPeaks {
                        left_min: vec![-1.0],
                        left_max: vec![1.0],
                        left_rms: vec![0.5],
                        right_min: vec![-1.0],
                        right_max: vec![1.0],
                        right_rms: vec![0.5],
                    },
                )
                .expect("waveform should be saved");
            store
                .connection
                .execute_batch(
                    "DROP INDEX IF EXISTS library_tracks_artist_title_idx;
                     ALTER TABLE timeline_clips DROP COLUMN tempo_anchor_beat;
                     ALTER TABLE library_tracks DROP COLUMN id3_scanned;
                     ALTER TABLE library_tracks DROP COLUMN title;
                     ALTER TABLE library_tracks DROP COLUMN artist;
                     PRAGMA user_version = 9;",
                )
                .expect("version nine should be simulated");
        }

        {
            let store = LibraryStore::open(&database_path).expect("database should migrate");
            let version = store
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .expect("schema version should be readable");
            let waveform_count = store
                .connection
                .query_row("SELECT COUNT(*) FROM track_waveforms", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("waveform count should be readable");
            assert_eq!(version, LATEST_SCHEMA_VERSION);
            assert_eq!(waveform_count, 0);
            assert_eq!(
                store
                    .library_waveform_targets()
                    .expect("waveform should be scheduled again")
                    .len(),
                1
            );
        }

        remove_database_files(&database_path);
    }

    #[test]
    fn manual_correction_can_be_reset_without_losing_the_analysis() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-correction-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let store = LibraryStore::open(&database_path).expect("database should open");
        store
            .connection
            .execute(
                "INSERT INTO library_tracks
                 (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                  bpm, first_beat_ms, beat_count, analysis_status)
                 VALUES ('missing.mp3', 'missing.mp3', 'correction.mp3', 60000, 44100, 2,
                         128.0, 1000, 126, 'analyzed')",
                [],
            )
            .expect("analyzed track should be inserted");
        let id = store.connection.last_insert_rowid();

        let corrected = store
            .update_beatgrid_correction(id, 64.0, 500)
            .expect("correction should be saved");
        assert_eq!(corrected[0].bpm, Some(64.0));
        assert_eq!(corrected[0].analyzed_bpm, Some(128.0));
        assert_eq!(corrected[0].first_beat_ms, Some(500));
        assert_eq!(corrected[0].analyzed_first_beat_ms, Some(1_000));
        assert!(corrected[0].is_corrected);

        let restored = store
            .reset_beatgrid_correction(id)
            .expect("automatic analysis should be restored");
        assert_eq!(restored[0].bpm, Some(128.0));
        assert_eq!(restored[0].first_beat_ms, Some(1_000));
        assert!(!restored[0].is_corrected);

        drop(store);
        remove_database_files(&database_path);
    }

    fn remove_database_files(database_path: &Path) {
        for suffix in ["", "-shm", "-wal"] {
            let path = format!("{}{suffix}", database_path.to_string_lossy());
            let _ = fs::remove_file(path);
        }
    }
}
