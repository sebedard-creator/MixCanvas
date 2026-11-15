-- SQLite cannot widen a CHECK constraint in place. The caller disables
-- foreign keys before the transaction so clip stems and bakes survive the swap.

CREATE TABLE timeline_lanes_v40 (
        lane       INTEGER PRIMARY KEY CHECK (lane BETWEEN 0 AND 3),
        is_muted   INTEGER NOT NULL DEFAULT 0 CHECK (is_muted IN (0, 1)),
        is_solo    INTEGER NOT NULL DEFAULT 0 CHECK (is_solo IN (0, 1))
    );
INSERT INTO timeline_lanes_v40 (lane, is_muted, is_solo)
    SELECT lane, is_muted, is_solo FROM timeline_lanes;
DROP TABLE timeline_lanes;
ALTER TABLE timeline_lanes_v40 RENAME TO timeline_lanes;

CREATE TABLE timeline_clips_v40 (
        id               INTEGER PRIMARY KEY,
        library_track_id INTEGER NOT NULL
                         REFERENCES library_tracks(id) ON DELETE CASCADE,
        lane             INTEGER NOT NULL DEFAULT 0 CHECK (lane BETWEEN 0 AND 3),
        anchor_beat      INTEGER NOT NULL CHECK (anchor_beat >= 0),
        tempo_anchor_beat INTEGER NOT NULL CHECK (tempo_anchor_beat >= 0),
        eq_settings      TEXT,
        trim_start_beats REAL NOT NULL DEFAULT 0.0 CHECK (trim_start_beats >= 0.0),
        trim_end_beats   REAL NOT NULL DEFAULT 0.0 CHECK (trim_end_beats >= 0.0),
        is_sidechain_key INTEGER NOT NULL DEFAULT 0
                         CHECK (is_sidechain_key IN (0, 1)),
        stem             TEXT NOT NULL DEFAULT 'full'
                         CHECK (stem IN ('full', 'vocals', 'instrumental')),
        muted            INTEGER NOT NULL DEFAULT 0 CHECK (muted IN (0, 1)),
        ducks_under_key  INTEGER NOT NULL DEFAULT 0
                         CHECK (ducks_under_key IN (0, 1)),
        looping          INTEGER NOT NULL DEFAULT 0 CHECK (looping IN (0, 1)),
        loop_lead_beats  REAL NOT NULL DEFAULT 0.0 CHECK (loop_lead_beats >= 0.0),
        loop_tail_beats  REAL NOT NULL DEFAULT 0.0 CHECK (loop_tail_beats >= 0.0),
        tempo_target_bpm REAL,
        created_at       INTEGER NOT NULL DEFAULT (unixepoch())
    );
INSERT INTO timeline_clips_v40 (id, library_track_id, lane, anchor_beat, tempo_anchor_beat, eq_settings, trim_start_beats, trim_end_beats, is_sidechain_key, stem, muted, ducks_under_key, looping, loop_lead_beats, loop_tail_beats, tempo_target_bpm, created_at)
    SELECT id, library_track_id, lane, anchor_beat, tempo_anchor_beat, eq_settings, trim_start_beats, trim_end_beats, is_sidechain_key, stem, muted, ducks_under_key, looping, loop_lead_beats, loop_tail_beats, tempo_target_bpm, created_at FROM timeline_clips;
DROP TABLE timeline_clips;
ALTER TABLE timeline_clips_v40 RENAME TO timeline_clips;

CREATE INDEX IF NOT EXISTS timeline_clips_track_idx
        ON timeline_clips(library_track_id);

CREATE TABLE timeline_volume_nodes_v40 (
        id      INTEGER PRIMARY KEY,
        lane    INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 3),
        beat    REAL NOT NULL CHECK (beat >= 0.0),
        gain_db REAL CHECK (gain_db IS NULL OR gain_db BETWEEN -60.0 AND 12.0),
        draw_group_id INTEGER,
        UNIQUE (lane, beat)
    );
INSERT INTO timeline_volume_nodes_v40 (id, lane, beat, gain_db, draw_group_id)
    SELECT id, lane, beat, gain_db, draw_group_id FROM timeline_volume_nodes;
DROP TABLE timeline_volume_nodes;
ALTER TABLE timeline_volume_nodes_v40 RENAME TO timeline_volume_nodes;

CREATE INDEX IF NOT EXISTS timeline_volume_nodes_lane_beat_idx
        ON timeline_volume_nodes(lane, beat);

CREATE TABLE timeline_pan_nodes_v40 (
        id    INTEGER PRIMARY KEY,
        lane  INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 3),
        beat  REAL NOT NULL CHECK (beat >= 0.0),
        value REAL NOT NULL CHECK (value BETWEEN -1.0 AND 1.0),
        draw_group_id INTEGER,
        UNIQUE (lane, beat)
    );
INSERT INTO timeline_pan_nodes_v40 (id, lane, beat, value, draw_group_id)
    SELECT id, lane, beat, value, draw_group_id FROM timeline_pan_nodes;
DROP TABLE timeline_pan_nodes;
ALTER TABLE timeline_pan_nodes_v40 RENAME TO timeline_pan_nodes;

CREATE INDEX IF NOT EXISTS timeline_pan_nodes_lane_beat_idx
        ON timeline_pan_nodes(lane, beat);

CREATE TABLE timeline_draw_groups_v40 (
        id         INTEGER PRIMARY KEY,
        kind       TEXT NOT NULL CHECK (kind IN ('volume', 'pan')),
        lane       INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 3),
        start_beat REAL NOT NULL CHECK (start_beat >= 0.0),
        end_beat   REAL NOT NULL CHECK (end_beat >= start_beat),
        shape      TEXT NOT NULL,
        period     REAL NOT NULL CHECK (period > 0.0),
        created_at INTEGER NOT NULL DEFAULT (unixepoch())
    );
INSERT INTO timeline_draw_groups_v40 (id, kind, lane, start_beat, end_beat, shape, period, created_at)
    SELECT id, kind, lane, start_beat, end_beat, shape, period, created_at FROM timeline_draw_groups;
DROP TABLE timeline_draw_groups;
ALTER TABLE timeline_draw_groups_v40 RENAME TO timeline_draw_groups;

CREATE TABLE timeline_filter_nodes_v40 (
        id      INTEGER PRIMARY KEY,
        lane    INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 3),
        beat    REAL NOT NULL CHECK (beat >= 0.0),
        value   REAL NOT NULL CHECK (value BETWEEN -1.0 AND 1.0),
        tension REAL NOT NULL DEFAULT 0.0 CHECK (tension BETWEEN -1.0 AND 1.0),
        UNIQUE (lane, beat)
    );
INSERT INTO timeline_filter_nodes_v40 (id, lane, beat, value, tension)
    SELECT id, lane, beat, value, tension FROM timeline_filter_nodes;
DROP TABLE timeline_filter_nodes;
ALTER TABLE timeline_filter_nodes_v40 RENAME TO timeline_filter_nodes;

CREATE INDEX IF NOT EXISTS timeline_filter_nodes_lane_beat_idx
        ON timeline_filter_nodes(lane, beat);

CREATE TABLE timeline_reverb_nodes_v40 (
        id    INTEGER PRIMARY KEY,
        lane  INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 3),
        beat  REAL NOT NULL CHECK (beat >= 0.0),
        value REAL NOT NULL CHECK (value BETWEEN 0.0 AND 1.0),
        UNIQUE (lane, beat)
    );
INSERT INTO timeline_reverb_nodes_v40 (id, lane, beat, value)
    SELECT id, lane, beat, value FROM timeline_reverb_nodes;
DROP TABLE timeline_reverb_nodes;
ALTER TABLE timeline_reverb_nodes_v40 RENAME TO timeline_reverb_nodes;

CREATE INDEX IF NOT EXISTS timeline_reverb_nodes_lane_beat_idx
        ON timeline_reverb_nodes(lane, beat);

CREATE TABLE timeline_flanger_nodes_v40 (
        id    INTEGER PRIMARY KEY,
        lane  INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 3),
        beat  REAL NOT NULL CHECK (beat >= 0.0),
        value REAL NOT NULL CHECK (value BETWEEN 0.0 AND 1.0),
        UNIQUE (lane, beat)
    );
INSERT INTO timeline_flanger_nodes_v40 (id, lane, beat, value)
    SELECT id, lane, beat, value FROM timeline_flanger_nodes;
DROP TABLE timeline_flanger_nodes;
ALTER TABLE timeline_flanger_nodes_v40 RENAME TO timeline_flanger_nodes;

CREATE INDEX IF NOT EXISTS timeline_flanger_nodes_lane_beat_idx
        ON timeline_flanger_nodes(lane, beat);

CREATE TABLE timeline_bitcrush_nodes_v40 (
        id    INTEGER PRIMARY KEY,
        lane  INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 3),
        beat  REAL NOT NULL CHECK (beat >= 0.0),
        value REAL NOT NULL CHECK (value BETWEEN 0.0 AND 1.0),
        UNIQUE (lane, beat)
    );
INSERT INTO timeline_bitcrush_nodes_v40 (id, lane, beat, value)
    SELECT id, lane, beat, value FROM timeline_bitcrush_nodes;
DROP TABLE timeline_bitcrush_nodes;
ALTER TABLE timeline_bitcrush_nodes_v40 RENAME TO timeline_bitcrush_nodes;

CREATE INDEX IF NOT EXISTS timeline_bitcrush_nodes_lane_beat_idx
        ON timeline_bitcrush_nodes(lane, beat);

CREATE TABLE timeline_delay_nodes_v40 (
        id    INTEGER PRIMARY KEY,
        lane  INTEGER NOT NULL CHECK (lane BETWEEN 0 AND 3),
        beat  REAL NOT NULL CHECK (beat >= 0.0),
        value REAL NOT NULL CHECK (value BETWEEN 0.0 AND 1.0),
        UNIQUE (lane, beat)
    );
INSERT INTO timeline_delay_nodes_v40 (id, lane, beat, value)
    SELECT id, lane, beat, value FROM timeline_delay_nodes;
DROP TABLE timeline_delay_nodes;
ALTER TABLE timeline_delay_nodes_v40 RENAME TO timeline_delay_nodes;

CREATE INDEX IF NOT EXISTS timeline_delay_nodes_lane_beat_idx
        ON timeline_delay_nodes(lane, beat);

INSERT OR IGNORE INTO timeline_lanes (lane, is_muted, is_solo) VALUES (3, 0, 0);
