use std::{collections::HashSet, path::Path};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::{
    analysis::WaveformPeaks,
    library::{decode_waveform, encode_waveform_values},
    tempo::{TempoMap, TempoPoint},
};

const MIN_BPM: f64 = 40.0;
const MAX_BPM: f64 = 300.0;
const BEATS_PER_MEASURE: i64 = 4;
/// Miroir de `MAX_SHAPE_NODES` cÃ´tÃ© interface : le serveur revalide ce que
/// le geste a dÃ©jÃ  bornÃ©, faute de quoi une commande forgÃ©e pourrait remplir
/// une piste.
const MAX_SHAPE_NODES: usize = 2_048;
/// Le nÅ“ud qui referme la forme, et les deux ancres de repos qui l'encadrent.
///
/// L'interface borne la **forme dessinÃ©e** Ã  `MAX_SHAPE_NODES`, puis ajoute ces
/// trois-lÃ . Le serveur borne ce qu'il **reÃ§oit** : confondre les deux faisait
/// refuser un long trait Ã  la pÃ©riode la plus courte â€” 2051 nÅ“uds arrivaient
/// contre une limite de 2048, et le trait mourait sur un message au lieu de
/// s'inscrire. Miroir de `SHAPE_EDGE_NODES` dans `src/lib/automationShapes.ts`.
const SHAPE_EDGE_NODES: usize = 3;
const MAX_STROKE_NODES: usize = MAX_SHAPE_NODES + SHAPE_EDGE_NODES;
const MAX_TIMELINE_BEAT: f64 = 1_000_000.0;
const MAX_LANE: i64 = 2;
/// Niveau d'une piste lÃ  oÃ¹ l'utilisateur n'a rien dÃ©cidÃ©.
///
/// Deux morceaux beatmatchÃ©s ont leurs kicks en phase : ils s'additionnent de
/// faÃ§on cohÃ©rente, soit +6 dB dans le pire cas. La valeur historique de âˆ’6 dB
/// rÃ©servait exactement cette marge, Ã  une Ã©poque oÃ¹ la sortie Ã©tait bornÃ©e en
/// dur et oÃ¹ tout dÃ©passement s'entendait comme un Ã©crÃªtage.
///
/// Le limiteur occupe dÃ©sormais cette place et travaille par dÃ©faut. Payer six
/// dÃ©cibels en permanence pour un Ã©vÃ©nement qu'il absorbe proprement serait un
/// mauvais Ã©change, d'autant que +6 dB est le pire cas thÃ©orique : deux kicks
/// de morceaux diffÃ©rents n'ont ni la mÃªme phase ni le mÃªme spectre.
///
/// Le moteur audio lit cette mÃªme constante pour les voies sans nÅ“ud, faute de
/// quoi la valeur Ã©crite en base et celle qu'on entend pourraient diverger.
pub const DEFAULT_TRACK_GAIN_DB: f64 = -4.0;

/// Le centre du champ stÃ©rÃ©o, oÃ¹ repose une voie dont personne n'a touchÃ© le
/// panoramique. NommÃ© plutÃ´t qu'Ã©crit en clair, pour que l'ancrage d'un clip
/// neuf et la valeur de repos d'un trait de crayon dÃ©signent la mÃªme chose.
pub const PAN_CENTRE: f64 = 0.0;
const FILTER_BUBBLE_MIN_WIDTH_BEATS: f64 = 2.0;
/// 1 024 measures, roughly half an hour at 128 BPM: long enough for a sweep
/// spanning a whole build without letting one gesture cover a whole mix.
const FILTER_BUBBLE_MAX_WIDTH_BEATS: f64 = 4_096.0;
const FILTER_BUBBLE_STEP_BEATS: f64 = 0.25;
/// A brush is persisted as a run of samples. Past this count the step widens
/// instead, so a long sweep costs no more storage, IPC or hashing than a short
/// one. Under 128 beats the step stays at a quarter beat, exactly as before.
const FILTER_BUBBLE_MAX_SAMPLES: f64 = 512.0;
/// Beats between the edge of a bubble and the bypass sample that closes it.
/// Kept in sync with `FILTER_BUBBLE_BYPASS_EPSILON_BEATS` in
/// `src/lib/filterShape.ts` so React draws the curve the engine plays.
const FILTER_BUBBLE_BYPASS_EPSILON_BEATS: f64 = 0.01;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipEqSettings {
    pub high_pass_hz: f64,
    pub low_pass_hz: f64,
    pub peak_hz: Option<f64>,
    pub peak_gain_db: Option<f64>,
    pub peak_q: Option<f64>,
    pub gain_db: Option<f64>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSnapshot {
    pub project_bpm: f64,
    pub limiter_enabled: bool,
    pub compressor_enabled: bool,
    pub tempo_points: Vec<TempoPoint>,
    pub lanes: Vec<TimelineLane>,
    pub clips: Vec<TimelineClip>,
    pub volume_nodes: Vec<TimelineVolumeNode>,
    pub pan_nodes: Vec<TimelinePanNode>,
    pub draw_groups: Vec<TimelineDrawGroup>,
    pub filter_nodes: Vec<TimelineFilterNode>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineVolumeNode {
    pub id: i64,
    pub lane: i64,
    pub beat: f64,
    pub gain_db: Option<f64>,
    pub draw_group_id: Option<i64>,
}

/// Un point de panoramique. `value` va de âˆ’1 (gauche) Ã  +1 (droite), 0 au
/// centre â€” la mÃªme convention bipolaire que les nÅ“uds de filtre.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePanNode {
    pub id: i64,
    pub lane: i64,
    pub beat: f64,
    pub value: f64,
    pub draw_group_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineDrawGroup {
    pub id: i64,
    pub kind: String,
    pub lane: i64,
    pub start_beat: f64,
    pub end_beat: f64,
    pub shape: String,
    pub period: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineFilterNode {
    pub id: i64,
    pub lane: i64,
    pub beat: f64,
    pub value: f64,
    pub tension: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineLane {
    pub lane: i64,
    pub is_muted: bool,
    pub is_solo: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineClip {
    pub id: i64,
    pub library_track_id: i64,
    pub file_name: String,
    pub file_path: String,
    pub lane: i64,
    pub anchor_beat: i64,
    pub tempo_anchor_beat: i64,
    /// Le BPM **du morceau** : sa vitesse native, telle que l'analyse la donne
    /// ou telle qu'on l'a corrigÃ©e. C'est la source du time-stretch.
    pub bpm: Option<f64>,
    /// Le tempo que la courbe vise Ã  l'ancre de ce clip, s'il en impose un.
    ///
    /// `None` â€” le cas ordinaire â€” veut dire Â« la vitesse native du morceau Â» :
    /// le clip joue Ã  un pour un. Une valeur est une dÃ©cision de mix, et le clip
    /// est Ã©tirÃ© vers elle. Voir [`effective_tempo_target`].
    pub tempo_target_bpm: Option<f64>,
    pub first_beat_ms: Option<u64>,
    pub pre_roll_beats: f64,
    pub duration_beats: f64,
    pub visual_start_beat: f64,
    pub visual_end_beat: f64,
    pub trim_start_beats: f64,
    pub trim_end_beats: f64,
    pub is_sidechain_key: bool,
    /// `full`, `vocals` ou `instrumental` : laquelle des voix du morceau ce clip
    /// joue. La sÃ©paration appartient au morceau, le choix appartient au clip.
    pub stem: String,
    /// Si le morceau a dÃ©jÃ  Ã©tÃ© sÃ©parÃ©. C'est ce qui distingue un clic
    /// instantanÃ© d'un rendu de deux minutes, et l'interface doit le savoir
    /// **avant** de cliquer pour ouvrir la bonne fenÃªtre.
    pub has_stems: bool,
    /// Si le fichier cuit a disparu du disque.
    ///
    /// Le clip reste Â« cuit Â» â€” son automation retirÃ©e vit dans
    /// l'enregistrement et doit rester rÃ©cupÃ©rable â€” mais il joue sa source, ce
    /// qui ne s'entend pas comme une panne. Sans ce drapeau, une touche allumÃ©e
    /// affirme un effet que personne n'applique.
    pub bake_is_missing: bool,
    /// Si ce clip joue un fichier cuit plutÃ´t que sa source.
    ///
    /// L'automation et l'Ã©galisation qu'il portait sont alors **dans** le son :
    /// les commandes qui les rÃ¨glent n'ont plus rien Ã  rÃ©gler, et l'interface
    /// doit pouvoir les Ã©teindre plutÃ´t que de les laisser mentir.
    pub is_baked: bool,
    pub is_missing: bool,
    pub needs_analysis: bool,
    pub eq_settings: Option<ClipEqSettings>,
    pub waveform: Option<WaveformPeaks>,
}

#[derive(Clone, Debug)]
pub(crate) struct TimelineRenderPlan {
    pub project_bpm: f64,
    pub tempo_map: TempoMap,
    pub end_beat: f64,
    pub audible_lane_mask: u8,
    pub limiter_enabled: bool,
    pub compressor_enabled: bool,
    pub clips: Vec<TimelineRenderClip>,
    pub volume_nodes: Vec<TimelineVolumeNode>,
    pub pan_nodes: Vec<TimelinePanNode>,
    pub filter_nodes: Vec<TimelineFilterNode>,
}

#[derive(Clone, Debug)]
pub(crate) struct TimelineRenderClip {
    pub id: i64,
    pub lane: i64,
    pub file_path: String,
    pub source_bpm: f64,
    pub first_beat_ms: u64,
    pub anchor_beat: f64,
    pub visual_start_beat: f64,
    pub duration_beats: f64,
    pub trim_start_beats: f64,
    pub trim_end_beats: f64,
    pub is_sidechain_key: bool,
    pub eq_settings: Option<ClipEqSettings>,
}

#[derive(Clone, Copy, Debug)]
struct ClipGeometry {
    pre_roll_beats: f64,
    duration_beats: f64,
    visual_start_beat: f64,
    visual_end_beat: f64,
    needs_analysis: bool,
}

#[derive(Debug)]
struct SourceTrack {
    file_path: String,
    duration_ms: u64,
    bpm: f64,
    first_beat_ms: u64,
}

pub fn snapshot(connection: &Connection) -> Result<TimelineSnapshot, String> {
    let ProjectSettings {
        project_bpm,
        limiter_enabled,
        compressor_enabled,
    } = project_settings(connection)?;
    let lanes = lane_states(connection)?;
    let mut statement = connection
        .prepare(
            "SELECT clips.id, clips.library_track_id,
                    tracks.file_name, tracks.file_path, clips.lane, clips.anchor_beat,
                    clips.tempo_anchor_beat,
                    COALESCE(tracks.manual_bpm, tracks.bpm),
                    COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms),
                    tracks.duration_ms,
                    -- Trois ondes possibles, dans l'ordre oÃ¹ le moteur choisit
                    -- sa source : le fichier cuit d'abord â€” c'est lui qu'on
                    -- entend, filtre compris â€”, puis le stem quand le clip en
                    -- joue un, puis le morceau entier.
                    COALESCE(bakes.bucket_count, stems.bucket_count, waveforms.bucket_count),
                    COALESCE(bakes.left_min, stems.left_min, waveforms.left_min),
                    COALESCE(bakes.left_max, stems.left_max, waveforms.left_max),
                    COALESCE(bakes.left_rms, stems.left_rms, waveforms.left_rms),
                    COALESCE(bakes.right_min, stems.right_min, waveforms.right_min),
                    COALESCE(bakes.right_max, stems.right_max, waveforms.right_max),
                    COALESCE(bakes.right_rms, stems.right_rms, waveforms.right_rms),
                    clips.eq_settings,
                    clips.trim_start_beats,
                    clips.trim_end_beats,
                    clips.is_sidechain_key,
                    clips.stem,
                    EXISTS(SELECT 1 FROM clip_stems WHERE clip_stems.clip_id = clips.id),
                    bakes.id IS NOT NULL,
                    bakes.file_path,
                    -- En derniÃ¨re colonne, et non Ã  sa place logique : le
                    -- mapping ci-dessous est positionnel, et l'insÃ©rer au milieu
                    -- dÃ©calerait silencieusement vingt indices.
                    clips.tempo_target_bpm
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             LEFT JOIN track_waveforms AS waveforms ON waveforms.track_id = tracks.id
             LEFT JOIN clip_stems AS stems
                    ON stems.clip_id = clips.id AND stems.kind = clips.stem
             LEFT JOIN clip_bakes AS bakes ON bakes.clip_id = clips.id
             ORDER BY clips.lane, clips.anchor_beat, clips.id",
        )
        .map_err(database_read_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<f64>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, Option<i64>>(10)?,
                row.get::<_, Option<Vec<u8>>>(11)?,
                row.get::<_, Option<Vec<u8>>>(12)?,
                row.get::<_, Option<Vec<u8>>>(13)?,
                row.get::<_, Option<Vec<u8>>>(14)?,
                row.get::<_, Option<Vec<u8>>>(15)?,
                row.get::<_, Option<Vec<u8>>>(16)?,
                row.get::<_, Option<String>>(17)?,
                row.get::<_, f64>(18)?,
                row.get::<_, f64>(19)?,
                row.get::<_, i64>(20)? != 0,
                row.get::<_, String>(21)?,
                row.get::<_, i64>(22)? != 0,
                row.get::<_, i64>(23)? != 0,
                row.get::<_, Option<String>>(24)?,
                row.get::<_, Option<f64>>(25)?,
            ))
        })
        .map_err(database_read_error)?;

    let mut clips = Vec::new();
    for row in rows {
        let (
            id,
            library_track_id,
            file_name,
            file_path,
            lane,
            anchor_beat,
            tempo_anchor_beat,
            bpm,
            first_beat_ms,
            duration_ms,
            waveform_bucket_count,
            waveform_left_min,
            waveform_left_max,
            waveform_left_rms,
            waveform_right_min,
            waveform_right_max,
            waveform_right_rms,
            eq_settings_json,
            trim_start_beats,
            trim_end_beats,
            is_sidechain_key,
            stem,
            has_stems,
            is_baked,
            bake_file_path,
            tempo_target_bpm,
        ) = row.map_err(database_read_error)?;
        let geometry = clip_geometry(
            duration_ms,
            bpm,
            first_beat_ms,
            anchor_beat,
            trim_start_beats,
            trim_end_beats,
        );
        let waveform = decode_waveform(
            waveform_bucket_count,
            waveform_left_min,
            waveform_left_max,
            waveform_left_rms,
            waveform_right_min,
            waveform_right_max,
            waveform_right_rms,
        );

        let eq_settings = eq_settings_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<ClipEqSettings>(raw).ok());

        clips.push(TimelineClip {
            id,
            library_track_id,
            file_name,
            is_missing: !Path::new(&file_path).is_file(),
            file_path,
            lane,
            anchor_beat,
            tempo_anchor_beat,
            bpm,
            tempo_target_bpm,
            first_beat_ms: first_beat_ms.and_then(|value| u64::try_from(value).ok()),
            pre_roll_beats: geometry.pre_roll_beats,
            duration_beats: geometry.duration_beats,
            visual_start_beat: geometry.visual_start_beat,
            visual_end_beat: geometry.visual_end_beat,
            trim_start_beats,
            trim_end_beats,
            is_sidechain_key,
            stem,
            has_stems,
            is_baked,
            bake_is_missing: bake_file_path
                .as_deref()
                .is_some_and(|path| !Path::new(path).is_file()),
            needs_analysis: geometry.needs_analysis,
            eq_settings,
            waveform,
        });
    }

    let tempo_points = tempo_points_for_clips(project_bpm, &clips)?;
    let volume_nodes = volume_nodes(connection)?;
    let pan_nodes = pan_nodes(connection)?;
    let draw_groups = draw_groups(connection)?;
    let filter_nodes = filter_nodes(connection)?;
    Ok(TimelineSnapshot {
        project_bpm,
        limiter_enabled,
        compressor_enabled,
        tempo_points,
        lanes,
        clips,
        volume_nodes,
        pan_nodes,
        draw_groups,
        filter_nodes,
    })
}

struct ProjectSettings {
    project_bpm: f64,
    limiter_enabled: bool,
    compressor_enabled: bool,
}

fn project_settings(connection: &Connection) -> Result<ProjectSettings, String> {
    connection
        .query_row(
            "SELECT project_bpm, limiter_enabled, compressor_enabled
             FROM project_settings WHERE id = 1",
            [],
            |row| {
                Ok(ProjectSettings {
                    project_bpm: row.get(0)?,
                    limiter_enabled: row.get::<_, i64>(1)? != 0,
                    compressor_enabled: row.get::<_, i64>(2)? != 0,
                })
            },
        )
        .map_err(database_read_error)
}

/// The master dynamics are applied from atomics shared with the queued source,
/// exactly like Mute and Solo: toggling one must take effect during playback
/// without rebuilding the plan or re-decoding anything.
pub fn set_limiter_enabled(
    connection: &Connection,
    limiter_enabled: bool,
) -> Result<TimelineSnapshot, String> {
    connection
        .execute(
            "UPDATE project_settings SET limiter_enabled = ?1 WHERE id = 1",
            params![limiter_enabled],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

/// Names the clip whose audio drives the sidechain, or clears it.
///
/// Exactly one clip can hold the key: naming a new one releases the previous,
/// in the same transaction, so the project is never briefly keyed by two clips
/// at once â€” which would duck twice as deep for a moment.
pub fn set_sidechain_key(
    connection: &mut Connection,
    clip_id: i64,
    is_key: bool,
) -> Result<TimelineSnapshot, String> {
    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute("UPDATE timeline_clips SET is_sidechain_key = 0", [])
        .map_err(database_write_error)?;
    if is_key {
        let changed = transaction
            .execute(
                "UPDATE timeline_clips SET is_sidechain_key = 1 WHERE id = ?1",
                [clip_id],
            )
            .map_err(database_write_error)?;
        if changed == 0 {
            return Err("This clip no longer exists in the timeline.".to_owned());
        }
    }
    transaction.commit().map_err(database_write_error)?;
    snapshot(connection)
}

pub fn set_compressor_enabled(
    connection: &Connection,
    compressor_enabled: bool,
) -> Result<TimelineSnapshot, String> {
    connection
        .execute(
            "UPDATE project_settings SET compressor_enabled = ?1 WHERE id = 1",
            params![compressor_enabled],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

fn pan_nodes(connection: &Connection) -> Result<Vec<TimelinePanNode>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, lane, beat, value, draw_group_id
             FROM timeline_pan_nodes
             ORDER BY lane, beat, id",
        )
        .map_err(database_read_error)?;
    statement
        .query_map([], |row| {
            Ok(TimelinePanNode {
                id: row.get(0)?,
                lane: row.get(1)?,
                beat: row.get(2)?,
                value: row.get(3)?,
                draw_group_id: row.get(4)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)
}

fn volume_nodes(connection: &Connection) -> Result<Vec<TimelineVolumeNode>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, lane, beat, gain_db, draw_group_id
             FROM timeline_volume_nodes
             ORDER BY lane, beat, id",
        )
        .map_err(database_read_error)?;
    statement
        .query_map([], |row| {
            Ok(TimelineVolumeNode {
                id: row.get(0)?,
                lane: row.get(1)?,
                beat: row.get(2)?,
                gain_db: row.get(3)?,
                draw_group_id: row.get(4)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)
}

fn draw_groups(connection: &Connection) -> Result<Vec<TimelineDrawGroup>, String> {
    let mut statement = connection
        .prepare("SELECT id, kind, lane, start_beat, end_beat, shape, period FROM timeline_draw_groups ORDER BY lane, start_beat, id")
        .map_err(database_read_error)?;
    statement
        .query_map([], |row| {
            Ok(TimelineDrawGroup {
                id: row.get(0)?,
                kind: row.get(1)?,
                lane: row.get(2)?,
                start_beat: row.get(3)?,
                end_beat: row.get(4)?,
                shape: row.get(5)?,
                period: row.get(6)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)
}

fn filter_nodes(connection: &Connection) -> Result<Vec<TimelineFilterNode>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, lane, beat, value, tension
             FROM timeline_filter_nodes
             ORDER BY lane, beat, id",
        )
        .map_err(database_read_error)?;
    statement
        .query_map([], |row| {
            Ok(TimelineFilterNode {
                id: row.get(0)?,
                lane: row.get(1)?,
                beat: row.get(2)?,
                value: row.get(3)?,
                tension: row.get(4)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)
}

fn lane_states(connection: &Connection) -> Result<Vec<TimelineLane>, String> {
    let mut statement = connection
        .prepare(
            "SELECT lane, is_muted, is_solo
             FROM timeline_lanes
             ORDER BY lane",
        )
        .map_err(database_read_error)?;
    let lanes = statement
        .query_map([], |row| {
            Ok(TimelineLane {
                lane: row.get(0)?,
                is_muted: row.get::<_, i64>(1)? != 0,
                is_solo: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(database_read_error)?;
    lanes
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)
}

pub(crate) fn audible_lane_mask(snapshot: &TimelineSnapshot) -> u8 {
    let any_solo = snapshot.lanes.iter().any(|lane| lane.is_solo);
    snapshot.lanes.iter().fold(0_u8, |mask, lane| {
        let audible = !lane.is_muted && (!any_solo || lane.is_solo);
        if audible {
            mask | (1_u8 << lane.lane)
        } else {
            mask
        }
    })
}

/// Everything the tempo map and the project length depend on, without the
/// waveform blobs or the per-clip filesystem check that `snapshot` performs.
/// The transport polls this several times a second.
#[derive(Clone, Copy, Debug)]
struct TimingRow {
    id: i64,
    anchor_beat: i64,
    tempo_anchor_beat: i64,
    bpm: Option<f64>,
    /// La cible imposÃ©e par le clip, s'il y en a une. Voir
    /// [`effective_tempo_target`] : cette colonne doit suivre celle du snapshot,
    /// faute de quoi le transport et le plan de rendu ne visent plus le mÃªme
    /// tempo.
    tempo_target_bpm: Option<f64>,
    first_beat_ms: Option<i64>,
    duration_ms: i64,
    trim_start_beats: f64,
    trim_end_beats: f64,
}

fn timing_rows(connection: &Connection) -> Result<Vec<TimingRow>, String> {
    let mut statement = connection
        .prepare(
            "SELECT clips.id, clips.anchor_beat, clips.tempo_anchor_beat,
                    COALESCE(tracks.manual_bpm, tracks.bpm),
                    clips.tempo_target_bpm,
                    COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms),
                    tracks.duration_ms,
                    clips.trim_start_beats, clips.trim_end_beats
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             ORDER BY clips.anchor_beat, clips.id",
        )
        .map_err(database_read_error)?;
    statement
        .query_map([], |row| {
            Ok(TimingRow {
                id: row.get(0)?,
                anchor_beat: row.get(1)?,
                tempo_anchor_beat: row.get(2)?,
                bpm: row.get(3)?,
                tempo_target_bpm: row.get(4)?,
                first_beat_ms: row.get(5)?,
                duration_ms: row.get(6)?,
                trim_start_beats: row.get(7)?,
                trim_end_beats: row.get(8)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)
}

/// Le tempo que la courbe globale vise Ã  l'ancre d'un clip.
///
/// Une seule rÃ¨gle, appelÃ©e par les deux constructeurs de cibles â€” celui du
/// plan de rendu et celui du transport. Les laisser diverger est la panne qui
/// revient le plus souvent dans ce projet, et elle est silencieuse : les deux
/// cartes cessent de se reconnaÃ®tre et le Seek s'arrÃªte de fonctionner sans
/// rien dire.
///
/// La cible du clip l'emporte quand elle existe; sinon c'est la vitesse native
/// du morceau, et le clip joue Ã  un pour un.
fn effective_tempo_target(target_bpm: Option<f64>, source_bpm: Option<f64>) -> Option<f64> {
    target_bpm
        .or(source_bpm)
        .filter(|bpm| bpm.is_finite() && (MIN_BPM..=MAX_BPM).contains(bpm))
}

/// A tempo target sits on `tempo_anchor_beat`, which a turquoise node can be
/// dragged along independently of the clip's audio anchor. Every caller must
/// build its map from this same function: two maps derived from different
/// columns produce different signatures, and the playback engine then rejects
/// its own cache and stops seeking.
fn tempo_targets(rows: &[TimingRow]) -> Vec<TempoPoint> {
    rows.iter()
        .filter_map(|row| {
            effective_tempo_target(row.tempo_target_bpm, row.bpm)
                .map(|bpm| TempoPoint::clip_target(row.tempo_anchor_beat as f64, bpm, row.id))
        })
        .collect()
}

fn project_end_beat(rows: &[TimingRow]) -> f64 {
    rows.iter().fold(0.0_f64, |end, row| {
        let geometry = clip_geometry(
            row.duration_ms,
            row.bpm,
            row.first_beat_ms,
            row.anchor_beat,
            row.trim_start_beats,
            row.trim_end_beats,
        );
        end.max(geometry.visual_end_beat)
    })
}

pub fn project_timing(connection: &Connection) -> Result<(TempoMap, f64), String> {
    let project_bpm = project_settings(connection)?.project_bpm;
    let rows = timing_rows(connection)?;
    let tempo_map = TempoMap::new(project_bpm, tempo_targets(&rows))?;

    Ok((tempo_map, project_end_beat(&rows)))
}

pub(crate) fn render_plan(connection: &Connection) -> Result<TimelineRenderPlan, String> {
    let timeline = snapshot(connection)?;
    if timeline.clips.is_empty() {
        return Err("Add at least one clip before starting the timeline.".to_owned());
    }

    let audible_lane_mask = audible_lane_mask(&timeline);
    let mut clips = Vec::with_capacity(timeline.clips.len());
    let mut end_beat = 0.0_f64;
    for clip in timeline.clips {
        if clip.is_missing {
            return Err(format!(
                "{} is missing. Put the file back before playing the timeline.",
                clip.file_name
            ));
        }
        let source_bpm = clip.bpm.ok_or_else(|| {
            format!(
                "{} needs its BPM analyzed before it can play.",
                clip.file_name
            )
        })?;
        let first_beat_ms = clip.first_beat_ms.ok_or_else(|| {
            format!(
                "{} needs its first beat corrected before it can play.",
                clip.file_name
            )
        })?;

        end_beat = end_beat.max(clip.visual_end_beat);
        // Le clip lit son stem s'il en joue un. Les fichiers sÃ©parÃ©s Ã©tant
        // alignÃ©s Ã  l'Ã©chantillon prÃ¨s sur l'original, tout ce qui a Ã©tÃ© rÃ©glÃ©
        // dessus â€” ancre, rognage, grille, automation â€” reste valable.
        let (file_path, stem_from_ms) =
            clip_audio_source(connection, clip.id, &clip.stem, &clip.file_path);
        // Le stem ne couvre que la fenÃªtre du clip, donc tout ce qui s'y lit
        // est dÃ©calÃ© de ce que la fenÃªtre a coupÃ©.
        //
        // Le dÃ©calage est retirÃ© du **rognage**, pas du premier temps. Sur un
        // clip dÃ©jÃ  rognÃ©, la fenÃªtre commence bien aprÃ¨s le premier temps du
        // morceau : `first_beat_ms - dÃ©calage` devenait nÃ©gatif, et le borner Ã
        // zÃ©ro faisait lire le moteur des secondes trop loin â€” d'oÃ¹ un stem
        // tantÃ´t dÃ©calÃ©, tantÃ´t muet, alors que la forme d'onde restait juste.
        // Le rognage, lui, contient dÃ©jÃ  ce dÃ©calage par construction et reste
        // positif : `premier temps + (rognage âˆ’ dÃ©calage)` donne exactement la
        // mÃªme position, sans jamais passer sous zÃ©ro.
        let stem_trim_beats = if stem_from_ms > 0.0 {
            stem_from_ms / (60_000.0 / source_bpm)
        } else {
            0.0
        };
        let trim_start_beats = (clip.trim_start_beats - stem_trim_beats).max(0.0);
        clips.push(TimelineRenderClip {
            id: clip.id,
            lane: clip.lane,
            file_path,
            source_bpm,
            first_beat_ms,
            anchor_beat: clip.anchor_beat as f64,
            visual_start_beat: clip.visual_start_beat,
            duration_beats: clip.duration_beats,
            trim_start_beats,
            trim_end_beats: clip.trim_end_beats,
            is_sidechain_key: clip.is_sidechain_key,
            eq_settings: clip.eq_settings,
        });
    }

    Ok(TimelineRenderPlan {
        project_bpm: timeline.project_bpm,
        tempo_map: TempoMap::new(timeline.project_bpm, timeline.tempo_points.clone())?,
        end_beat,
        audible_lane_mask,
        limiter_enabled: timeline.limiter_enabled,
        compressor_enabled: timeline.compressor_enabled,
        clips,
        volume_nodes: timeline.volume_nodes,
        pan_nodes: timeline.pan_nodes,
        filter_nodes: timeline.filter_nodes,
    })
}

fn tempo_points_for_clips(
    project_bpm: f64,
    clips: &[TimelineClip],
) -> Result<Vec<TempoPoint>, String> {
    // Same targets, same filter and same column as `tempo_targets`, so the map
    // the engine renders from and the map the transport polls stay identical.
    let targets = clips
        .iter()
        .filter_map(|clip| {
            effective_tempo_target(clip.tempo_target_bpm, clip.bpm)
                .map(|bpm| TempoPoint::clip_target(clip.tempo_anchor_beat as f64, bpm, clip.id))
        })
        .collect();
    Ok(TempoMap::new(project_bpm, targets)?.points().to_vec())
}

pub fn add_clip(
    connection: &mut Connection,
    library_track_id: i64,
    requested_anchor_beat: Option<f64>,
    requested_lane: Option<i64>,
) -> Result<TimelineSnapshot, String> {
    let source = source_track(connection, library_track_id)?;
    if !Path::new(&source.file_path).is_file() {
        return Err("This MP3 is missing, and cannot be added to the timeline.".to_owned());
    }

    let pre_roll_beats = beats_for_milliseconds(source.first_beat_ms, source.bpm);
    let minimum_anchor = minimum_anchor_beat(pre_roll_beats, 0.0);
    let current = snapshot(connection)?;
    let is_first_clip = current.clips.is_empty();
    let duration_ms = i64::try_from(source.duration_ms)
        .map_err(|_| "This track's duration is not valid.".to_owned())?;

    // A lane the caller named is the caller's decision, overlap and all. With
    // none named the rotation only sets where to start looking: advancing one
    // lane per clip says nothing about whether that lane is free at the anchor,
    // which is how an automatic placement used to land on an occupied track and
    // refuse itself.
    let candidates: Vec<i64> = match requested_lane {
        Some(lane) => {
            validate_lane(lane)?;
            vec![lane]
        }
        None => {
            let lane_count = MAX_LANE + 1;
            let start = next_rotation_lane(&current.clips);
            (0..lane_count)
                .map(|offset| (start + offset) % lane_count)
                .collect()
        }
    };

    let mut placement = None;
    for lane in candidates {
        let requested = requested_anchor_beat.unwrap_or_else(|| {
            let last_end = current
                .clips
                .iter()
                .filter(|clip| clip.lane == lane)
                .map(|clip| clip.visual_end_beat)
                .fold(0.0_f64, f64::max);
            (last_end + pre_roll_beats).ceil()
        });
        let anchor_beat = snap_anchor_beat(requested, minimum_anchor)?;
        let geometry = clip_geometry(
            duration_ms,
            Some(source.bpm),
            i64::try_from(source.first_beat_ms).ok(),
            anchor_beat,
            0.0,
            0.0,
        );

        let is_overlapping = current.clips.iter().any(|c| {
            c.lane == lane
                && clips_overlap(
                    geometry.visual_start_beat,
                    geometry.visual_end_beat,
                    c.visual_start_beat,
                    c.visual_end_beat,
                )
        });
        if !is_overlapping {
            placement = Some((lane, anchor_beat, geometry));
            break;
        }
    }

    let Some((lane, anchor_beat, geometry)) = placement else {
        return Err(if requested_lane.is_some() {
            "That track is already busy at this point. Drop the clip somewhere \
             else, or onto a free track."
                .to_owned()
        } else {
            "All three tracks are already busy at this point. Move the playhead, \
             or make room on one of them."
                .to_owned()
        });
    };

    let transaction = connection.transaction().map_err(database_write_error)?;
    if is_first_clip {
        transaction
            .execute(
                "UPDATE project_settings SET project_bpm = ?1 WHERE id = 1",
                [rounded_bpm(source.bpm)],
            )
            .map_err(database_write_error)?;
    }
    transaction
        .execute(
            "INSERT INTO timeline_clips
             (library_track_id, lane, anchor_beat, tempo_anchor_beat)
             VALUES (?1, ?2, ?3, ?3)",
            params![library_track_id, lane, anchor_beat],
        )
        .map_err(database_write_error)?;
    for table in [VOLUME_AUTOMATION, PAN_AUTOMATION] {
        seed_clip_automation_nodes(
            &transaction,
            table,
            lane,
            geometry.visual_start_beat,
            geometry.visual_end_beat,
        )?;
    }
    transaction.commit().map_err(database_write_error)?;

    snapshot(connection)
}

/// Pose aux deux bouts d'un clip neuf ses nÅ“uds d'ancrage, Ã  la valeur de repos
/// de la ligne.
///
/// Sans eux, une automation Ã©crite plus loin sur la voie remonterait jusqu'au
/// dÃ©but du clip : la ligne rampe entre ses nÅ“uds, et le premier nÅ“ud d'une
/// voie vaut pour tout ce qui le prÃ©cÃ¨de. Les ancres bornent donc le clip Ã  ce
/// qu'il est censÃ© Ãªtre avant qu'on y touche, et deviennent les poignÃ©es par
/// lesquelles on l'attrape.
///
/// `ON CONFLICT DO NOTHING` : si un nÅ“ud occupe dÃ©jÃ  la place, il porte un
/// rÃ©glage voulu par quelqu'un et l'ancrage n'a rien Ã  y redire.
fn seed_clip_automation_nodes(
    transaction: &Transaction<'_>,
    table: AutomationTable,
    lane: i64,
    visual_start_beat: f64,
    visual_end_beat: f64,
) -> Result<(), String> {
    for beat in [visual_start_beat, visual_end_beat] {
        let beat = validate_automation_beat(beat, table.node_label)?;
        transaction
            .execute(
                &format!(
                    "INSERT INTO {} (lane, beat, {})
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(lane, beat) DO NOTHING",
                    table.name, table.value_column
                ),
                params![lane, beat, table.rest_value],
            )
            .map_err(database_write_error)?;
    }
    Ok(())
}

/// Une table d'automation de voie, dÃ©signÃ©e par son nom et par le mot qui la
/// nomme dans un message d'erreur.
///
/// Le volume et le panoramique subissent exactement la mÃªme manÅ“uvre quand un
/// clip se dÃ©place. L'Ã©crire deux fois est le dÃ©faut qui revient le plus
/// souvent dans ce projet : les deux copies finissent par diverger, et
/// l'oubliÃ©e s'arrÃªte de suivre sans rien dire. Le nom de table est une
/// constante du programme, jamais une entrÃ©e, donc son insertion dans le SQL
/// n'ouvre rien.
#[derive(Clone, Copy)]
struct AutomationTable {
    name: &'static str,
    node_label: &'static str,
    /// La colonne qui porte la valeur : les deux tables ne la nomment pas
    /// pareil, parce qu'un dÃ©cibel et un cÃ´tÃ© ne sont pas la mÃªme grandeur.
    value_column: &'static str,
    /// Ce que vaut la ligne quand personne n'y a touchÃ©.
    rest_value: f64,
}

const VOLUME_AUTOMATION: AutomationTable = AutomationTable {
    name: "timeline_volume_nodes",
    node_label: "Volume Node",
    value_column: "gain_db",
    rest_value: DEFAULT_TRACK_GAIN_DB,
};

const PAN_AUTOMATION: AutomationTable = AutomationTable {
    name: "timeline_pan_nodes",
    node_label: "Pan Node",
    value_column: "value",
    rest_value: PAN_CENTRE,
};

/// Emporte avec le clip les nÅ“uds d'automation qu'il contient.
///
/// Les nÅ“uds passent par un garage hors timeline avant d'Ãªtre posÃ©s Ã  leur
/// destination : sans lui, dÃ©caler d'un cran une suite de nÅ“uds ferait entrer
/// le premier dans la place encore occupÃ©e par le second, et la contrainte
/// d'unicitÃ© refuserait le dÃ©placement.
fn move_clip_automation_nodes(
    transaction: &Transaction<'_>,
    table: AutomationTable,
    old_lane: i64,
    new_lane: i64,
    visual_start_beat: f64,
    visual_end_beat: f64,
    beat_delta: f64,
) -> Result<(), String> {
    const RANGE_EPSILON: f64 = 0.125_001;
    let mut statement = transaction
        .prepare(&format!(
            "SELECT id, beat
             FROM {}
             WHERE lane = ?1 AND beat >= ?2 AND beat <= ?3
             ORDER BY beat, id",
            table.name
        ))
        .map_err(database_read_error)?;
    let selected = statement
        .query_map(
            params![
                old_lane,
                visual_start_beat - RANGE_EPSILON,
                visual_end_beat + RANGE_EPSILON
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)),
        )
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    drop(statement);

    if selected.is_empty() || (old_lane == new_lane && beat_delta.abs() < f64::EPSILON) {
        return Ok(());
    }

    let selected_ids: HashSet<i64> = selected.iter().map(|(id, _)| *id).collect();
    let mut all_nodes = transaction
        .prepare(&format!("SELECT id, lane, beat FROM {}", table.name))
        .map_err(database_read_error)?;
    let existing = all_nodes
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    drop(all_nodes);

    // Translation, pas repose : les positions gardent leur finesse d'origine.
    // Les recaler sur le quart de temps Ã©crasait les uns sur les autres les
    // nÅ“uds d'une forme dessinÃ©e â€” un sinus en pose une douzaine par cycle,
    // donc plusieurs par quart â€” et la contrainte d'unicitÃ© refusait alors le
    // dÃ©placement d'un clip qui n'avait pourtant rien d'ambigu. Le calage sur
    // le quart appartient au geste qui pose un nÅ“ud, pas Ã  un clip qui avance.
    let moved = selected
        .iter()
        .map(|(id, beat)| {
            let target = *beat + beat_delta;
            if !target.is_finite() || !(0.0..=MAX_TIMELINE_BEAT).contains(&target) {
                return Err(format!(
                    "The {} position is outside the timeline.",
                    table.node_label
                ));
            }
            Ok((*id, target))
        })
        .collect::<Result<Vec<_>, String>>()?;
    // Et la place n'est prise que par un nÅ“ud rÃ©ellement au mÃªme endroit :
    // comparer au quart de temps aurait fait barrer la route Ã  toute une forme
    // par un seul nÅ“ud voisin.
    for (_, target_beat) in &moved {
        if existing.iter().any(|(id, lane, beat)| {
            *lane == new_lane
                && !selected_ids.contains(id)
                && (*beat - target_beat).abs() < BEAT_SAME_SLOT_EPSILON
        }) {
            return Err(format!(
                "A {} outside this clip already occupies the destination.",
                table.node_label
            ));
        }
    }

    for (index, (id, _)) in moved.iter().enumerate() {
        let temporary_beat = MAX_TIMELINE_BEAT + 1.0 + index as f64;
        transaction
            .execute(
                &format!("UPDATE {} SET beat = ?2 WHERE id = ?1", table.name),
                params![id, temporary_beat],
            )
            .map_err(database_write_error)?;
    }
    for (id, target_beat) in moved {
        transaction
            .execute(
                &format!(
                    "UPDATE {} SET lane = ?2, beat = ?3 WHERE id = ?1",
                    table.name
                ),
                params![id, new_lane, target_beat],
            )
            .map_err(database_write_error)?;
    }
    Ok(())
}

/// Updates the compact Draw record once the dense automation samples belonging
/// to a clip have moved.  Its bounds stay useful for hit-testing `Delete Draw`.
fn move_clip_draw_groups(
    transaction: &Transaction<'_>,
    kind: &str,
    old_lane: i64,
    new_lane: i64,
    visual_start_beat: f64,
    visual_end_beat: f64,
    beat_delta: f64,
) -> Result<(), String> {
    transaction
        .execute(
            "UPDATE timeline_draw_groups
             SET lane = ?2, start_beat = start_beat + ?3, end_beat = end_beat + ?3
             WHERE kind = ?1 AND lane = ?4 AND start_beat >= ?5 AND end_beat <= ?6",
            params![
                kind,
                new_lane,
                beat_delta,
                old_lane,
                visual_start_beat - 0.125_001,
                visual_end_beat + 0.125_001
            ],
        )
        .map_err(database_write_error)?;
    Ok(())
}

pub fn move_clip(
    connection: &mut Connection,
    clip_id: i64,
    requested_anchor_beat: f64,
    requested_lane: i64,
) -> Result<TimelineSnapshot, String> {
    validate_lane(requested_lane)?;
    let source = connection
        .query_row(
            "SELECT tracks.duration_ms,
                    COALESCE(tracks.manual_bpm, tracks.bpm),
                    COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms),
                    clips.lane,
                    clips.anchor_beat,
                    clips.tempo_anchor_beat,
                    clips.trim_start_beats,
                    clips.trim_end_beats
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             WHERE clips.id = ?1",
            [clip_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<f64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, f64>(6)?,
                    row.get::<_, f64>(7)?,
                ))
            },
        )
        .optional()
        .map_err(database_read_error)?
        .ok_or_else(|| "This clip is no longer on the timeline.".to_owned())?;
    let (
        duration_ms,
        bpm,
        first_beat_ms,
        current_lane,
        old_anchor_beat,
        _tempo_anchor_beat,
        trim_start,
        trim_end,
    ) = source;
    let geometry = clip_geometry(duration_ms, bpm, first_beat_ms, 0, trim_start, trim_end);
    if geometry.needs_analysis {
        return Err("This track needs its BPM analyzed before it can be moved.".to_owned());
    }
    let anchor_beat = snap_anchor_beat(
        requested_anchor_beat,
        minimum_anchor_beat(geometry.pre_roll_beats, trim_start),
    )?;
    let old_geometry = clip_geometry(
        duration_ms,
        bpm,
        first_beat_ms,
        old_anchor_beat,
        trim_start,
        trim_end,
    );
    let new_geometry = clip_geometry(
        duration_ms,
        bpm,
        first_beat_ms,
        anchor_beat,
        trim_start,
        trim_end,
    );

    let current = snapshot(connection)?;
    let is_overlapping = current.clips.iter().any(|c| {
        c.id != clip_id
            && c.lane == requested_lane
            && clips_overlap(
                new_geometry.visual_start_beat,
                new_geometry.visual_end_beat,
                c.visual_start_beat,
                c.visual_end_beat,
            )
    });
    if is_overlapping {
        return Err(
            "That track is already busy here. Drop the clip somewhere else, or onto a free track."
                .to_owned(),
        );
    }
    let transaction = connection.transaction().map_err(database_write_error)?;
    // Les deux lignes suivent le clip : une automation qui reste en arriÃ¨re
    // dÃ©crirait un geste sur du silence, et le clip arriverait sur le geste du
    // voisin.
    for table in [VOLUME_AUTOMATION, PAN_AUTOMATION] {
        move_clip_automation_nodes(
            &transaction,
            table,
            current_lane,
            requested_lane,
            old_geometry.visual_start_beat,
            old_geometry.visual_end_beat,
            new_geometry.visual_start_beat - old_geometry.visual_start_beat,
        )?;
    }
    for kind in ["volume", "pan"] {
        move_clip_draw_groups(
            &transaction,
            kind,
            current_lane,
            requested_lane,
            old_geometry.visual_start_beat,
            old_geometry.visual_end_beat,
            new_geometry.visual_start_beat - old_geometry.visual_start_beat,
        )?;
    }
    transaction
        .execute(
            "UPDATE timeline_clips
             SET anchor_beat = ?2,
                 lane = ?3,
                 tempo_anchor_beat = tempo_anchor_beat + (?2 - ?4)
             WHERE id = ?1",
            params![clip_id, anchor_beat, requested_lane, old_anchor_beat],
        )
        .map_err(database_write_error)?;
    transaction.commit().map_err(database_write_error)?;

    snapshot(connection)
}

/// Deux nÅ“uds sÃ©parÃ©s de moins que cela occupent la mÃªme place.
///
/// SQLite compare les rÃ©els exactement; ce seuil est donc un peu plus large que
/// la contrainte, ce qui refuse d'avance un dÃ©placement qu'elle aurait acceptÃ©
/// de justesse. C'est le bon sens de l'inÃ©galitÃ© : mieux vaut un message clair
/// qu'un empilement de nÅ“uds Ã  un millioniÃ¨me de temps l'un de l'autre.
const BEAT_SAME_SLOT_EPSILON: f64 = 1e-6;

/// Choisit laquelle des voix d'un morceau ce clip joue.
///
/// La sÃ©paration appartient au **morceau** et le choix au **clip** : deux clips
/// du mÃªme morceau peuvent jouer l'un la voix, l'autre l'instrumental, sans
/// qu'on sÃ©pare deux fois.
///
/// Le fichier doit exister avant qu'on puisse le dÃ©signer. Laisser passer un
/// clip vers un stem absent donnerait un clip muet dont rien n'expliquerait le
/// silence.
pub fn set_clip_stem(
    connection: &Connection,
    clip_id: i64,
    stem: &str,
) -> Result<TimelineSnapshot, String> {
    if !matches!(stem, "full" | "vocals" | "instrumental") {
        return Err("That is not a stem this clip can play.".to_owned());
    }
    let exists: Option<i64> = connection
        .query_row(
            "SELECT id FROM timeline_clips WHERE id = ?1",
            [clip_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(database_read_error)?;
    if exists.is_none() {
        return Err("This clip is no longer on the timeline.".to_owned());
    }

    if stem != "full" {
        let path: Option<String> = connection
            .query_row(
                "SELECT file_path FROM clip_stems WHERE clip_id = ?1 AND kind = ?2",
                params![clip_id, stem],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_read_error)?;
        match path {
            Some(path) if Path::new(&path).is_file() => {}
            _ => {
                return Err(
                    "This track has not been separated yet â€” run Separate Stems first."
                        .to_owned(),
                );
            }
        }
    }

    connection
        .execute(
            "UPDATE timeline_clips SET stem = ?2 WHERE id = ?1",
            params![clip_id, stem],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

/// Le fichier qu'un clip doit rÃ©ellement lire, selon la voix qu'il joue.
///
/// Un clip bascule sur son stem sans bouger : les fichiers sÃ©parÃ©s sont alignÃ©s
/// Ã  l'Ã©chantillon prÃ¨s sur l'original, donc l'ancre, le rognage et la grille
/// restent valables tels quels.
pub(crate) fn clip_audio_source(
    connection: &Connection,
    clip_id: i64,
    stem: &str,
    original: &str,
) -> (String, f64) {
    // Le bake passe avant tout le reste : le fichier cuit contient dÃ©jÃ  le stem
    // qui jouait au moment de la cuisson, ainsi que l'Ã©galisation et
    // l'automation. Le relire Ã  travers un stem reviendrait Ã  choisir deux fois.
    if let Some(baked) = baked_audio_source(connection, clip_id) {
        return baked;
    }
    if stem == "full" {
        return (original.to_owned(), 0.0);
    }
    connection
        .query_row(
            "SELECT file_path, source_from_ms FROM clip_stems WHERE clip_id = ?1 AND kind = ?2",
            params![clip_id, stem],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .ok()
        .flatten()
        .filter(|(path, _)| Path::new(path).is_file())
        .map(|(path, from_ms)| (path, from_ms as f64))
        .unwrap_or_else(|| (original.to_owned(), 0.0))
}

/// Le fichier cuit d'un clip, s'il en a un et qu'il est toujours lÃ .
///
/// Un bake dont le fichier a disparu â€” dossier de donnÃ©es effacÃ©, disque
/// externe absent â€” ne doit pas rendre le clip muet : on retombe sur la source,
/// donc sur le clip sans ses effets. C'est faux Ã  l'oreille, mais audible et
/// rÃ©parable d'un clic sur `BAKE`; un silence, lui, ne se diagnostique pas.
fn baked_audio_source(connection: &Connection, clip_id: i64) -> Option<(String, f64)> {
    connection
        .query_row(
            "SELECT file_path, source_from_ms FROM clip_bakes WHERE clip_id = ?1",
            params![clip_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .ok()
        .flatten()
        .filter(|(path, _)| Path::new(path).is_file())
        .map(|(path, from_ms)| (path, from_ms as f64))
}

/// La tranche de la source qu'un clip fait entendre, en millisecondes.
///
/// **L'origine est le dÃ©but du fichier, pas le premier temps.** Un clip fait
/// entendre tout ce qui prÃ©cÃ¨de ce premier temps â€” le prÃ©-roll â€”, et
/// `duration_beats` le compte. Ancrer cette fenÃªtre sur le premier temps
/// laissait le prÃ©-roll dehors : sur un morceau dont le premier temps tombe Ã
/// deux minutes quarante-six, un stem sÃ©parÃ© Ã©tait muet sur toute cette
/// premiÃ¨re partie.
pub(crate) fn clip_source_window_ms(clip: &TimelineClip) -> Option<(f64, f64)> {
    let beat_ms = 60_000.0 / clip.bpm?;
    let start = clip.trim_start_beats * beat_ms;
    Some((start, start + clip.duration_beats * beat_ms))
}

/// A clip has to keep at least this much of itself, or the drag would erase it.
/// Mirrors `MIN_CLIP_BEATS` on the interface side.
const MIN_CLIP_BEATS: f64 = 0.5;

/// Hides part of a clip's head or tail, or gives it back.
///
/// The anchor is deliberately untouched: a trim changes how much of the source
/// is heard, not where the clip sits, so everything still audible stays exactly
/// where it was. That is what separates this from a move.
pub fn set_clip_trim(
    connection: &mut Connection,
    clip_id: i64,
    trim_start_beats: f64,
    trim_end_beats: f64,
) -> Result<TimelineSnapshot, String> {
    if !trim_start_beats.is_finite() || !trim_end_beats.is_finite() {
        return Err("The requested trim is not a valid length.".to_owned());
    }
    let trim_start_beats = trim_start_beats.max(0.0);
    let trim_end_beats = trim_end_beats.max(0.0);

    let source = connection
        .query_row(
            "SELECT tracks.duration_ms,
                    COALESCE(tracks.manual_bpm, tracks.bpm),
                    COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms),
                    clips.lane,
                    clips.anchor_beat
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             WHERE clips.id = ?1",
            [clip_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<f64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(database_read_error)?
        .ok_or_else(|| "This clip is no longer on the timeline.".to_owned())?;
    let (duration_ms, bpm, first_beat_ms, lane, anchor_beat) = source;

    let whole = clip_geometry(duration_ms, bpm, first_beat_ms, anchor_beat, 0.0, 0.0);
    if whole.needs_analysis {
        return Err("This track needs its BPM analyzed before it can be trimmed.".to_owned());
    }
    // Trimming past the far end would invert the clip; trimming past its own
    // length would ask for audio the file does not contain.
    if trim_start_beats + trim_end_beats > whole.duration_beats - MIN_CLIP_BEATS {
        return Err("A clip cannot be trimmed away entirely.".to_owned());
    }

    let geometry = clip_geometry(
        duration_ms,
        bpm,
        first_beat_ms,
        anchor_beat,
        trim_start_beats,
        trim_end_beats,
    );
    let current = snapshot(connection)?;
    let is_overlapping = current.clips.iter().any(|c| {
        c.id != clip_id
            && c.lane == lane
            && clips_overlap(
                geometry.visual_start_beat,
                geometry.visual_end_beat,
                c.visual_start_beat,
                c.visual_end_beat,
            )
    });
    if is_overlapping {
        return Err("There is another clip in the way on this track.".to_owned());
    }

    connection
        .execute(
            "UPDATE timeline_clips
             SET trim_start_beats = ?2, trim_end_beats = ?3
             WHERE id = ?1",
            params![clip_id, trim_start_beats, trim_end_beats],
        )
        .map_err(database_write_error)?;

    snapshot(connection)
}

pub fn move_tempo_point(
    connection: &Connection,
    clip_id: i64,
    requested_tempo_anchor_beat: f64,
) -> Result<TimelineSnapshot, String> {
    let source = connection
        .query_row(
            "SELECT tracks.duration_ms,
                    COALESCE(tracks.manual_bpm, tracks.bpm),
                    COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms),
                    clips.anchor_beat,
                    clips.trim_start_beats,
                    clips.trim_end_beats
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             WHERE clips.id = ?1",
            [clip_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<f64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, f64>(5)?,
                ))
            },
        )
        .optional()
        .map_err(database_read_error)?
        .ok_or_else(|| "This clip is no longer on the timeline.".to_owned())?;
    let (duration_ms, bpm, first_beat_ms, anchor_beat, trim_start, trim_end) = source;
    let geometry = clip_geometry(
        duration_ms,
        bpm,
        first_beat_ms,
        anchor_beat,
        trim_start,
        trim_end,
    );
    if geometry.needs_analysis {
        return Err(
            "This track needs its BPM analyzed before its tempo marker can be moved.".to_owned(),
        );
    }
    let tempo_anchor_beat = snap_tempo_anchor_beat(
        requested_tempo_anchor_beat,
        geometry.visual_start_beat,
        geometry.visual_end_beat,
    )?;
    connection
        .execute(
            "UPDATE timeline_clips SET tempo_anchor_beat = ?2 WHERE id = ?1",
            params![clip_id, tempo_anchor_beat],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

/// Impose Ã  ce clip le tempo que la courbe doit viser Ã  son ancre, ou lui rend
/// la vitesse native de son morceau avec `None`.
///
/// **N'Ã©crit pas dans la bibliothÃ¨que.** C'Ã©tait le dÃ©faut : rÃ©gler le tempo
/// d'un nÅ“ud appelait la correction de BPM du morceau, si bien qu'une dÃ©cision
/// de mix rÃ©Ã©crivait une analyse â€” dÃ©finitivement, et pour tous les usages
/// futurs de ce morceau â€” tout en dÃ©plaÃ§ant la courbe sous **les autres** clips,
/// dont le beatmatching Ã©tait perdu. Corriger une analyse et choisir une
/// vitesse sont deux gestes distincts, et ils ont dÃ©sormais deux chemins.
pub fn set_clip_tempo_target(
    connection: &Connection,
    clip_id: i64,
    target_bpm: Option<f64>,
) -> Result<TimelineSnapshot, String> {
    if let Some(bpm) = target_bpm
        && !(bpm.is_finite() && (MIN_BPM..=MAX_BPM).contains(&bpm))
    {
        return Err(format!(
            "The tempo has to be between {MIN_BPM:.0} and {MAX_BPM:.0}."
        ));
    }

    let updated = connection
        .execute(
            "UPDATE timeline_clips SET tempo_target_bpm = ?2 WHERE id = ?1",
            params![clip_id, target_bpm],
        )
        .map_err(database_write_error)?;
    if updated == 0 {
        return Err("This clip is no longer on the timeline.".to_owned());
    }
    snapshot(connection)
}

/// Les morceaux de `span` que rien dans `covers` ne recouvre.
///
/// Une voie accepte des clips qui se chevauchent dès qu'on lui en nomme une :
/// la plage d'un clip retiré peut donc appartenir aussi à son voisin, et son
/// automation avec. On n'efface que ce qui n'était qu'à lui.
fn uncovered_intervals(span: (f64, f64), covers: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut open = vec![span];
    for &(cover_start, cover_end) in covers {
        let mut next = Vec::with_capacity(open.len() + 1);
        for (start, end) in open {
            // À côté : le morceau survit entier.
            if cover_end <= start || cover_start >= end {
                next.push((start, end));
                continue;
            }
            if cover_start > start {
                next.push((start, cover_start));
            }
            if cover_end < end {
                next.push((cover_end, end));
            }
        }
        open = next;
    }
    // Un reste large de rien n'a aucun nœud à contenir.
    open.retain(|(start, end)| end - start > 1.0e-9);
    open
}

/// La voie et la portée visible de chaque clip, pour une voie donnée.
fn lane_clip_spans(connection: &Connection, lane: i64) -> Result<Vec<(i64, f64, f64)>, String> {
    let mut statement = connection
        .prepare(
            "SELECT clips.id, tracks.duration_ms,
                    COALESCE(tracks.manual_bpm, tracks.bpm),
                    COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms),
                    clips.anchor_beat, clips.trim_start_beats, clips.trim_end_beats
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             WHERE clips.lane = ?1",
        )
        .map_err(database_read_error)?;
    let spans = statement
        .query_map([lane], |row| {
            let geometry = clip_geometry(
                row.get::<_, i64>(1)?,
                row.get::<_, Option<f64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, f64>(6)?,
            );
            Ok((
                row.get::<_, i64>(0)?,
                geometry.visual_start_beat,
                geometry.visual_end_beat,
            ))
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    Ok(spans)
}

/// Retire un clip **et l'automation qui n'existait que pour lui**.
///
/// L'automation vit sur la voie, pas sur le clip : elle lui survivait donc, et
/// une courbe de filtre restait à travailler un endroit où plus rien ne joue.
/// Elle part avec lui — mais seulement là où aucun autre clip de la voie ne
/// couvre la même plage, faute de quoi retirer un clip détruirait le travail
/// fait pour son voisin.
///
/// Les nœuds de filtre demandent un soin de plus. Les effacer en travers d'une
/// bulle laisserait ses bords survivre de part et d'autre, et la courbe
/// interpolerait droit à travers le vide : le filtre resterait engagé là où on
/// vient justement de tout retirer. Un nœud de bypass est donc reposé à chaque
/// bord de ce qu'on efface, dès qu'il reste quelque chose à border.
pub fn remove_clip(connection: &mut Connection, clip_id: i64) -> Result<TimelineSnapshot, String> {
    let Some((lane, span_start, span_end)) = connection
        .query_row(
            "SELECT clips.lane, tracks.duration_ms,
                    COALESCE(tracks.manual_bpm, tracks.bpm),
                    COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms),
                    clips.anchor_beat, clips.trim_start_beats, clips.trim_end_beats
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             WHERE clips.id = ?1",
            [clip_id],
            |row| {
                let geometry = clip_geometry(
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<f64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, f64>(6)?,
                );
                Ok((
                    row.get::<_, i64>(0)?,
                    geometry.visual_start_beat,
                    geometry.visual_end_beat,
                ))
            },
        )
        .optional()
        .map_err(database_read_error)?
    else {
        // Déjà parti : rien à faire, et surtout rien à effacer au hasard.
        return snapshot(connection);
    };

    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute("DELETE FROM timeline_clips WHERE id = ?1", [clip_id])
        .map_err(database_write_error)?;

    let survivors: Vec<(f64, f64)> = lane_clip_spans(&transaction, lane)?
        .into_iter()
        .map(|(_, start, end)| (start, end))
        .collect();

    for (start, end) in uncovered_intervals((span_start, span_end), &survivors) {
        for table in [
            "timeline_volume_nodes",
            "timeline_pan_nodes",
            "timeline_filter_nodes",
        ] {
            transaction
                .execute(
                    &format!("DELETE FROM {table} WHERE lane = ?1 AND beat >= ?2 AND beat <= ?3"),
                    params![lane, start, end],
                )
                .map_err(database_write_error)?;
        }

        // Un geste Draw dont il ne reste plus aucun point n'a plus de courbe à
        // représenter, et encombrerait son menu contextuel.
        transaction
            .execute(
                "DELETE FROM timeline_draw_groups
                 WHERE lane = ?1 AND start_beat >= ?2 AND end_beat <= ?3",
                params![lane, start, end],
            )
            .map_err(database_write_error)?;

        let remaining_filter: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM timeline_filter_nodes WHERE lane = ?1",
                [lane],
                |row| row.get(0),
            )
            .map_err(database_read_error)?;
        if remaining_filter > 0 {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO timeline_filter_nodes (lane, beat, value, tension)
                     VALUES (?1, ?2, 0.0, 0.0)
                     ON CONFLICT(lane, beat) DO UPDATE SET value = 0.0, tension = 0.0",
                )
                .map_err(database_write_error)?;
            for edge in [start, end] {
                insert
                    .execute(params![lane, edge])
                    .map_err(database_write_error)?;
            }
        }
    }

    transaction.commit().map_err(database_write_error)?;
    snapshot(connection)
}

pub fn clear_timeline(connection: &Connection) -> Result<TimelineSnapshot, String> {
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
             DELETE FROM timeline_clips;
             DELETE FROM timeline_volume_nodes;
             DELETE FROM timeline_pan_nodes;
             DELETE FROM timeline_draw_groups;
             DELETE FROM timeline_filter_nodes;
             UPDATE timeline_lanes SET is_muted = 0, is_solo = 0;
             COMMIT;",
        )
        .map_err(database_write_error)?;

    snapshot(connection)
}

pub fn set_lane_muted(
    connection: &Connection,
    lane: i64,
    is_muted: bool,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    connection
        .execute(
            "UPDATE timeline_lanes SET is_muted = ?2 WHERE lane = ?1",
            params![lane, is_muted],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

pub fn set_lane_solo(
    connection: &Connection,
    lane: i64,
    is_solo: bool,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    connection
        .execute(
            "UPDATE timeline_lanes SET is_solo = ?2 WHERE lane = ?1",
            params![lane, is_solo],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

pub fn add_volume_node(
    connection: &Connection,
    lane: i64,
    requested_beat: f64,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    let beat = validate_volume_beat(requested_beat)?;
    let gain_db = interpolated_volume_db(&volume_nodes(connection)?, lane, beat);
    connection
        .execute(
            "INSERT INTO timeline_volume_nodes (lane, beat, gain_db)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(lane, beat) DO NOTHING",
            params![lane, beat, gain_db],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

pub fn move_volume_node(
    connection: &Connection,
    node_id: i64,
    requested_beat: f64,
    gain_db: Option<f64>,
) -> Result<TimelineSnapshot, String> {
    let beat = validate_volume_beat(requested_beat)?;
    validate_gain_db(gain_db)?;
    let changed = connection
        .execute(
            "UPDATE timeline_volume_nodes SET beat = ?2, gain_db = ?3 WHERE id = ?1",
            params![node_id, beat, gain_db],
        )
        .map_err(|error| {
            if error.to_string().contains("UNIQUE constraint failed") {
                "A Volume Node already exists at this position.".to_owned()
            } else {
                database_write_error(error)
            }
        })?;
    if changed == 0 {
        return Err("This Volume Node no longer exists.".to_owned());
    }
    snapshot(connection)
}

pub fn delete_volume_node(
    connection: &Connection,
    node_id: i64,
) -> Result<TimelineSnapshot, String> {
    connection
        .execute("DELETE FROM timeline_volume_nodes WHERE id = ?1", [node_id])
        .map_err(database_write_error)?;
    snapshot(connection)
}

/// Pose un point de panoramique, en reprenant la valeur dÃ©jÃ  en vigueur Ã  cet
/// endroit : ajouter un nÅ“ud ne doit jamais dÃ©placer le son, seulement offrir
/// une poignÃ©e pour le faire ensuite.
pub fn add_pan_node(
    connection: &Connection,
    lane: i64,
    requested_beat: f64,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    let beat = validate_pan_beat(requested_beat)?;
    let value = interpolated_pan(&pan_nodes(connection)?, lane, beat);
    connection
        .execute(
            "INSERT INTO timeline_pan_nodes (lane, beat, value)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(lane, beat) DO NOTHING",
            params![lane, beat, value],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

pub fn move_pan_node(
    connection: &Connection,
    node_id: i64,
    requested_beat: f64,
    value: f64,
) -> Result<TimelineSnapshot, String> {
    let beat = validate_pan_beat(requested_beat)?;
    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
        return Err("A Pan Node has to sit between hard left and hard right.".to_owned());
    }
    let changed = connection
        .execute(
            "UPDATE timeline_pan_nodes SET beat = ?2, value = ?3 WHERE id = ?1",
            params![node_id, beat, value],
        )
        .map_err(|error| {
            if error.to_string().contains("UNIQUE constraint failed") {
                "A Pan Node already exists at this position.".to_owned()
            } else {
                database_write_error(error)
            }
        })?;
    if changed == 0 {
        return Err("This Pan Node no longer exists.".to_owned());
    }
    snapshot(connection)
}

pub fn delete_pan_node(connection: &Connection, node_id: i64) -> Result<TimelineSnapshot, String> {
    connection
        .execute("DELETE FROM timeline_pan_nodes WHERE id = ?1", [node_id])
        .map_err(database_write_error)?;
    snapshot(connection)
}

pub fn delete_draw_group(
    connection: &mut Connection,
    group_id: i64,
) -> Result<TimelineSnapshot, String> {
    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute(
            "DELETE FROM timeline_volume_nodes WHERE draw_group_id = ?1",
            [group_id],
        )
        .map_err(database_write_error)?;
    transaction
        .execute(
            "DELETE FROM timeline_pan_nodes WHERE draw_group_id = ?1",
            [group_id],
        )
        .map_err(database_write_error)?;
    let changed = transaction
        .execute("DELETE FROM timeline_draw_groups WHERE id = ?1", [group_id])
        .map_err(database_write_error)?;
    if changed == 0 {
        return Err("This Draw no longer exists.".to_owned());
    }
    transaction.commit().map_err(database_write_error)?;
    snapshot(connection)
}

/// Ã‰crit une forme d'automation de volume, en remplaÃ§ant ce qui occupait
/// l'Ã©tendue couverte.
///
/// L'ancienne plage et la nouvelle partent dans la mÃªme transaction : une forme
/// Ã  demi effacÃ©e ne doit jamais s'entendre, et un trait interrompu ne doit pas
/// laisser derriÃ¨re lui la queue de ce qu'il remplaÃ§ait.
fn validate_draw_metadata(shape: &str, period: f64) -> Result<(), String> {
    if !matches!(shape, "step" | "sine" | "triangle") {
        return Err("That Draw shape is not supported.".to_owned());
    }
    if !period.is_finite() || !(0.25..=16.0).contains(&period) {
        return Err("That Draw period is outside the supported range.".to_owned());
    }
    Ok(())
}

/// A later stroke can erase every sample of an earlier one. Drop only empty
/// records; a partially overwritten Draw retains its remaining audio samples.
fn prune_empty_draw_groups(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute(
            "DELETE FROM timeline_draw_groups
             WHERE id NOT IN (
               SELECT DISTINCT draw_group_id FROM timeline_volume_nodes WHERE draw_group_id IS NOT NULL
               UNION
               SELECT DISTINCT draw_group_id FROM timeline_pan_nodes WHERE draw_group_id IS NOT NULL
             )",
            [],
        )
        .map_err(database_write_error)?;
    Ok(())
}

pub fn draw_volume_shape(
    connection: &mut Connection,
    lane: i64,
    start_beat: f64,
    end_beat: f64,
    nodes: &[(f64, f64)],
    shape: &str,
    period: f64,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    let from = validate_volume_beat(start_beat.min(end_beat))?;
    let to = validate_volume_beat(start_beat.max(end_beat))?;
    if nodes.len() > MAX_STROKE_NODES {
        return Err("This stroke asks for more nodes than a lane can hold.".to_owned());
    }
    for (_, gain_db) in nodes {
        validate_gain_db(Some(*gain_db))?;
    }
    validate_draw_metadata(shape, period)?;

    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute(
            "DELETE FROM timeline_volume_nodes WHERE lane = ?1 AND beat BETWEEN ?2 AND ?3",
            params![lane, from, to],
        )
        .map_err(database_write_error)?;
    prune_empty_draw_groups(&transaction)?;
    transaction
        .execute(
            "INSERT INTO timeline_draw_groups (kind, lane, start_beat, end_beat, shape, period)
         VALUES ('volume', ?1, ?2, ?3, ?4, ?5)",
            params![lane, from, to, shape, period],
        )
        .map_err(database_write_error)?;
    let draw_group_id = transaction.last_insert_rowid();
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO timeline_volume_nodes (lane, beat, gain_db, draw_group_id)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(lane, beat) DO UPDATE SET gain_db = excluded.gain_db, draw_group_id = excluded.draw_group_id",
            )
            .map_err(database_write_error)?;
        for (beat, gain_db) in nodes {
            insert
                .execute(params![lane, beat, gain_db, draw_group_id])
                .map_err(database_write_error)?;
        }
    }
    transaction.commit().map_err(database_write_error)?;
    snapshot(connection)
}

/// La mÃªme chose pour le panoramique.
pub fn draw_pan_shape(
    connection: &mut Connection,
    lane: i64,
    start_beat: f64,
    end_beat: f64,
    nodes: &[(f64, f64)],
    shape: &str,
    period: f64,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    let from = validate_pan_beat(start_beat.min(end_beat))?;
    let to = validate_pan_beat(start_beat.max(end_beat))?;
    if nodes.len() > MAX_STROKE_NODES {
        return Err("This stroke asks for more nodes than a lane can hold.".to_owned());
    }
    for (_, value) in nodes {
        if !value.is_finite() || !(-1.0..=1.0).contains(value) {
            return Err("A Pan Node has to sit between hard left and hard right.".to_owned());
        }
    }
    validate_draw_metadata(shape, period)?;

    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute(
            "DELETE FROM timeline_pan_nodes WHERE lane = ?1 AND beat BETWEEN ?2 AND ?3",
            params![lane, from, to],
        )
        .map_err(database_write_error)?;
    prune_empty_draw_groups(&transaction)?;
    transaction
        .execute(
            "INSERT INTO timeline_draw_groups (kind, lane, start_beat, end_beat, shape, period)
         VALUES ('pan', ?1, ?2, ?3, ?4, ?5)",
            params![lane, from, to, shape, period],
        )
        .map_err(database_write_error)?;
    let draw_group_id = transaction.last_insert_rowid();
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO timeline_pan_nodes (lane, beat, value, draw_group_id)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(lane, beat) DO UPDATE SET value = excluded.value, draw_group_id = excluded.draw_group_id",
            )
            .map_err(database_write_error)?;
        for (beat, value) in nodes {
            insert
                .execute(params![lane, beat, value, draw_group_id])
                .map_err(database_write_error)?;
        }
    }
    transaction.commit().map_err(database_write_error)?;
    snapshot(connection)
}

/// Pose un nÅ“ud de filtre isolÃ©.
///
/// Plus aucune commande n'y mÃ¨ne : le pinceau Ã©crit des plages entiÃ¨res, et le
/// tracÃ© libre aussi. La fonction reste parce que deux tests ont besoin de
/// poser un nÅ“ud pour vÃ©rifier autre chose â€” la signature de lecture, le
/// remplacement d'une plage â€”, pas parce qu'elle est encore un geste.
#[cfg(test)]
pub fn add_filter_node(
    connection: &Connection,
    lane: i64,
    requested_beat: f64,
    value: f64,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    let beat = validate_volume_beat(requested_beat)?;
    validate_filter_value(value)?;
    connection
        .execute(
            "INSERT INTO timeline_filter_nodes (lane, beat, value)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(lane, beat) DO UPDATE SET value = excluded.value, tension = 0.0",
            params![lane, beat, snapped_filter_value(value)],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

/// Stores a user-drawn Filter Brush as a dense, invisible automation curve.
/// The endpoints always return to bypass and the sine envelope avoids a hard
/// corner in both the picture and the audible cutoff sweep.
///
/// `replaced_range` names a curve this gesture supersedes. Resizing one is
/// exactly that: the old span must disappear and the new one appear together,
/// or a shortened curve would leave its former tail behind â€” and a curve that
/// vanished before being rewritten would be heard opening up mid-playback.
pub fn draw_filter_bubble(
    connection: &mut Connection,
    lane: i64,
    requested_start_beat: f64,
    requested_width_beats: f64,
    value: f64,
    shape_type: Option<String>,
    replaced_range: Option<(f64, f64)>,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    let start_beat = validate_volume_beat(requested_start_beat)?;
    validate_filter_value(value)?;
    if !requested_width_beats.is_finite() || requested_width_beats <= 0.0 {
        return Err("Filter Brush width is invalid.".to_owned());
    }
    let width_beats = (requested_width_beats * 4.0).round() / 4.0;
    let width_beats =
        width_beats.clamp(FILTER_BUBBLE_MIN_WIDTH_BEATS, FILTER_BUBBLE_MAX_WIDTH_BEATS);
    let end_beat = (start_beat + width_beats).min(MAX_TIMELINE_BEAT);
    let shape = shape_type.as_deref().unwrap_or("ramp_up");

    let mut delete_from = start_beat - FILTER_BUBBLE_BYPASS_EPSILON_BEATS;
    let mut delete_to = end_beat + 0.25;
    if let Some((replaced_start, replaced_end)) = replaced_range {
        if !replaced_start.is_finite()
            || !replaced_end.is_finite()
            || replaced_end < replaced_start
            || replaced_start < 0.0
            || replaced_end > MAX_TIMELINE_BEAT
        {
            return Err("That filter range is outside the timeline.".to_owned());
        }
        delete_from = delete_from.min(replaced_start - FILTER_BUBBLE_BYPASS_EPSILON_BEATS);
        delete_to = delete_to.max(replaced_end + 0.25);
    }

    // The whole brush lands in one transaction, so the DSP can never read a
    // half-drawn gesture, and the hundreds of samples share a single commit
    // instead of one implicit transaction each.
    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute(
            "DELETE FROM timeline_filter_nodes
             WHERE lane = ?1 AND beat >= ?2 AND beat <= ?3",
            params![lane, delete_from, delete_to],
        )
        .map_err(database_write_error)?;

    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO timeline_filter_nodes (lane, beat, value, tension)
                 VALUES (?1, ?2, ?3, 0.0)
                 ON CONFLICT(lane, beat) DO UPDATE SET value = excluded.value, tension = 0.0",
            )
            .map_err(database_write_error)?;

        let step_beats = filter_bubble_step_beats(end_beat - start_beat);
        let steps = ((end_beat - start_beat) / step_beats).round() as usize;
        for step in 0..=steps {
            let beat = if step == steps {
                end_beat
            } else {
                start_beat + step as f64 * step_beats
            };
            let progress = if end_beat <= start_beat {
                0.0
            } else {
                ((beat - start_beat) / (end_beat - start_beat)).clamp(0.0, 1.0)
            };

            let bubble_value =
                snapped_filter_value(value * filter_shape_multiplier(shape, progress));
            insert
                .execute(params![lane, beat, bubble_value])
                .map_err(database_write_error)?;
        }

        // A ramp ends away from bypass, so it needs an explicit sample bringing
        // the band back. A triangle already returns to zero on its own edges.
        if shape == "ramp_up" && end_beat + FILTER_BUBBLE_BYPASS_EPSILON_BEATS <= MAX_TIMELINE_BEAT
        {
            insert
                .execute(params![
                    lane,
                    end_beat + FILTER_BUBBLE_BYPASS_EPSILON_BEATS,
                    0.0
                ])
                .map_err(database_write_error)?;
        } else if shape == "ramp_down" && start_beat >= FILTER_BUBBLE_BYPASS_EPSILON_BEATS {
            insert
                .execute(params![
                    lane,
                    start_beat - FILTER_BUBBLE_BYPASS_EPSILON_BEATS,
                    0.0
                ])
                .map_err(database_write_error)?;
        }
    }

    transaction.commit().map_err(database_write_error)?;

    snapshot(connection)
}

/// Ã‰crit un trait de filtre tracÃ© Ã  main levÃ©e.
///
/// Le pinceau Ã  bulle calcule sa forme ici, Ã  partir d'une largeur et d'une
/// profondeur; un trait libre, lui, arrive dÃ©jÃ  dessinÃ© â€” c'est la main qui l'a
/// fait, et le serveur n'a rien Ã  en dÃ©duire. Il ne lui reste qu'Ã  vÃ©rifier ce
/// qu'il reÃ§oit et Ã  remplacer la plage d'un seul coup.
///
/// Les ancres au bypass qui referment le trait arrivent avec les points peints,
/// dans le mÃªme tableau, posÃ©es par `filterStrokeNodes`
/// (`src/lib/filterShape.ts`) : c'est ce qui permet Ã  la courbe tracÃ©e sous le
/// curseur d'Ãªtre exactement celle qui sera jouÃ©e. Les recalculer ici en
/// donnerait deux propriÃ©taires, et c'est ainsi que ce projet a perdu six
/// rÃ¨gles.
pub fn draw_filter_stroke(
    connection: &mut Connection,
    lane: i64,
    nodes: &[(f64, f64)],
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    if nodes.is_empty() {
        return Err("This stroke has nothing to draw.".to_owned());
    }
    if nodes.len() as f64 > FILTER_BUBBLE_MAX_SAMPLES {
        return Err("This stroke asks for more samples than a curve can hold.".to_owned());
    }
    let mut previous = f64::NEG_INFINITY;
    for (beat, value) in nodes {
        validate_filter_value(*value)?;
        if !beat.is_finite() || !(0.0..=MAX_TIMELINE_BEAT).contains(beat) {
            return Err("That filter stroke is outside the timeline.".to_owned());
        }
        if *beat <= previous {
            return Err("A filter stroke has to run forwards.".to_owned());
        }
        previous = *beat;
    }

    let from = nodes[0].0;
    let to = nodes[nodes.len() - 1].0;

    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute(
            "DELETE FROM timeline_filter_nodes
             WHERE lane = ?1 AND beat >= ?2 AND beat <= ?3",
            params![
                lane,
                from - FILTER_BUBBLE_BYPASS_EPSILON_BEATS,
                to + FILTER_BUBBLE_BYPASS_EPSILON_BEATS
            ],
        )
        .map_err(database_write_error)?;
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO timeline_filter_nodes (lane, beat, value, tension)
                 VALUES (?1, ?2, ?3, 0.0)
                 ON CONFLICT(lane, beat) DO UPDATE SET value = excluded.value, tension = 0.0",
            )
            .map_err(database_write_error)?;
        for (beat, value) in nodes {
            insert
                .execute(params![lane, beat, snapped_filter_value(*value)])
                .map_err(database_write_error)?;
        }
    }
    transaction.commit().map_err(database_write_error)?;

    snapshot(connection)
}

/// Spacing between two persisted samples of a brush.
///
/// A quarter beat up to `FILTER_BUBBLE_MAX_SAMPLES` samples, then wide enough
/// to keep that count. The result stays on the quarter-beat grid so every
/// sample lands where `validate_volume_beat` would snap it, and so the range
/// deleted when the curve is redrawn or erased still covers all of them.
/// A wider step costs nothing audible: the engine interpolates between samples
/// and smooths the cutoff over 8 ms, and a sweep that long moves slowly.
fn filter_bubble_step_beats(width_beats: f64) -> f64 {
    if width_beats <= 0.0 {
        return FILTER_BUBBLE_STEP_BEATS;
    }
    let quarter_steps = (width_beats / FILTER_BUBBLE_STEP_BEATS).ceil();
    if quarter_steps <= FILTER_BUBBLE_MAX_SAMPLES {
        return FILTER_BUBBLE_STEP_BEATS;
    }
    (quarter_steps / FILTER_BUBBLE_MAX_SAMPLES).ceil() * FILTER_BUBBLE_STEP_BEATS
}

/// Fraction of the bubble value reached at `progress` along its width.
/// `src/lib/filterShape.ts` mirrors this so the drawn curve is the heard one.
fn filter_shape_multiplier(shape: &str, progress: f64) -> f64 {
    match shape {
        "ramp_down" => 1.0 - progress,
        "triangle" => 1.0 - (2.0 * progress - 1.0).abs(),
        _ => progress, // "ramp_up"
    }
}

/// Restores a previously observed timeline for Undo/Redo.
///
/// The snapshot travels through the frontend, so it is validated here exactly
/// like a fresh edit would be, and it is written in a single transaction: a
/// failure halfway through must not leave the project emptied.
pub fn restore_snapshot(
    connection: &mut Connection,
    target: &TimelineSnapshot,
) -> Result<TimelineSnapshot, String> {
    validate_restored_snapshot(target)?;

    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute(
            "UPDATE project_settings
             SET project_bpm = ?1, limiter_enabled = ?2, compressor_enabled = ?3
             WHERE id = 1",
            params![
                rounded_bpm(target.project_bpm),
                target.limiter_enabled,
                target.compressor_enabled
            ],
        )
        .map_err(database_write_error)?;

    for lane in &target.lanes {
        transaction
            .execute(
                "UPDATE timeline_lanes SET is_muted = ?1, is_solo = ?2 WHERE lane = ?3",
                params![lane.is_muted, lane.is_solo, lane.lane],
            )
            .map_err(database_write_error)?;
    }

    // Les clips ne sont plus remplacÃ©s, ils sont **corrigÃ©s**.
    //
    // L'ancienne manÅ“uvre effaÃ§ait tout puis rÃ©insÃ©rait : `clip_stems` partait
    // en cascade, et il fallait la relever et la reposer Ã  la main. Ce
    // sauvetage nommait ses colonnes une par une, donc il en oubliait â€” la
    // forme d'onde du stem disparaissait Ã  chaque Undo, quel que soit le geste
    // annulÃ©. Le mÃªme piÃ¨ge attendrait la prochaine table rattachÃ©e aux clips.
    //
    // Mettre Ã  jour ce qui reste et ne supprimer que ce qui s'en va laisse
    // intact tout ce qui pend aux clips, aujourd'hui comme demain.
    let kept: Vec<i64> = target.clips.iter().map(|clip| clip.id).collect();
    let placeholders = kept
        .iter()
        .enumerate()
        .map(|(index, _)| format!("?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let condition = if kept.is_empty() {
        String::new()
    } else {
        format!(" WHERE id NOT IN ({placeholders})")
    };
    transaction
        .execute(
            &format!("DELETE FROM timeline_clips{condition}"),
            rusqlite::params_from_iter(kept.iter()),
        )
        .map_err(database_write_error)?;
    for clip in &target.clips {
        let eq_json = clip
            .eq_settings
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| format!("This clip's EQ settings are not valid: {error}"))?;
        transaction
            .execute(
                "INSERT INTO timeline_clips (id, library_track_id, lane, anchor_beat, tempo_anchor_beat, trim_start_beats, trim_end_beats, is_sidechain_key, eq_settings, stem)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                     library_track_id = excluded.library_track_id,
                     lane = excluded.lane,
                     anchor_beat = excluded.anchor_beat,
                     tempo_anchor_beat = excluded.tempo_anchor_beat,
                     trim_start_beats = excluded.trim_start_beats,
                     trim_end_beats = excluded.trim_end_beats,
                     is_sidechain_key = excluded.is_sidechain_key,
                     eq_settings = excluded.eq_settings,
                     stem = excluded.stem",
                params![
                    clip.id,
                    clip.library_track_id,
                    clip.lane,
                    clip.anchor_beat,
                    clip.tempo_anchor_beat,
                    clip.trim_start_beats,
                    clip.trim_end_beats,
                    clip.is_sidechain_key,
                    eq_json,
                    clip.stem,
                ],
            )
            .map_err(database_write_error)?;
    }

    transaction
        .execute("DELETE FROM timeline_volume_nodes", [])
        .map_err(database_write_error)?;
    transaction
        .execute("DELETE FROM timeline_pan_nodes", [])
        .map_err(database_write_error)?;
    transaction
        .execute("DELETE FROM timeline_draw_groups", [])
        .map_err(database_write_error)?;
    for group in &target.draw_groups {
        transaction
            .execute(
                "INSERT INTO timeline_draw_groups
                 (id, kind, lane, start_beat, end_beat, shape, period)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    group.id,
                    group.kind,
                    group.lane,
                    group.start_beat,
                    group.end_beat,
                    group.shape,
                    group.period,
                ],
            )
            .map_err(database_write_error)?;
    }
    for node in &target.volume_nodes {
        transaction
            .execute(
                "INSERT INTO timeline_volume_nodes (id, lane, beat, gain_db, draw_group_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    node.id,
                    node.lane,
                    node.beat,
                    node.gain_db,
                    node.draw_group_id
                ],
            )
            .map_err(database_write_error)?;
    }

    // Le panoramique fait partie de l'Ã©tat restaurÃ© au mÃªme titre que le
    // volume : l'oublier ici laisserait un Undo rÃ©Ã©crire tout sauf lui.
    for node in &target.pan_nodes {
        transaction
            .execute(
                "INSERT INTO timeline_pan_nodes (id, lane, beat, value, draw_group_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    node.id,
                    node.lane,
                    node.beat,
                    node.value,
                    node.draw_group_id
                ],
            )
            .map_err(database_write_error)?;
    }

    transaction
        .execute("DELETE FROM timeline_filter_nodes", [])
        .map_err(database_write_error)?;
    for node in &target.filter_nodes {
        transaction
            .execute(
                "INSERT INTO timeline_filter_nodes (id, lane, beat, value, tension) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![node.id, node.lane, node.beat, node.value, node.tension],
            )
            .map_err(database_write_error)?;
    }

    transaction.commit().map_err(database_write_error)?;

    snapshot(connection)
}

fn validate_restored_snapshot(target: &TimelineSnapshot) -> Result<(), String> {
    validate_bpm(target.project_bpm)?;

    for group in &target.draw_groups {
        validate_lane(group.lane)?;
        validate_restored_beat(group.start_beat)?;
        validate_restored_beat(group.end_beat)?;
        if group.end_beat < group.start_beat {
            return Err("This history entry holds an inverted Draw range.".to_owned());
        }
        if !matches!(group.kind.as_str(), "volume" | "pan") {
            return Err("This history entry holds an unknown Draw kind.".to_owned());
        }
        validate_draw_metadata(&group.shape, group.period)?;
    }

    for lane in &target.lanes {
        validate_lane(lane.lane)?;
    }

    for node in &target.pan_nodes {
        validate_lane(node.lane)?;
        validate_restored_beat(node.beat)?;
        if !node.value.is_finite() || !(-1.0..=1.0).contains(&node.value) {
            return Err("This history entry holds a pan outside the stereo field.".to_owned());
        }
    }

    for clip in &target.clips {
        validate_lane(clip.lane)?;
        validate_restored_beat(clip.anchor_beat as f64)?;
        validate_restored_beat(clip.tempo_anchor_beat as f64)?;
        if !clip.trim_start_beats.is_finite()
            || !clip.trim_end_beats.is_finite()
            || clip.trim_start_beats < 0.0
            || clip.trim_end_beats < 0.0
        {
            return Err("This clip has an invalid trim.".to_owned());
        }
    }

    for node in &target.volume_nodes {
        validate_lane(node.lane)?;
        validate_restored_beat(node.beat)?;
        validate_gain_db(node.gain_db)?;
    }

    for node in &target.filter_nodes {
        validate_lane(node.lane)?;
        validate_restored_beat(node.beat)?;
        validate_filter_value(node.value)?;
        validate_filter_tension(node.tension)?;
    }

    Ok(())
}

fn validate_restored_beat(beat: f64) -> Result<(), String> {
    if !beat.is_finite() || !(0.0..=MAX_TIMELINE_BEAT).contains(&beat) {
        return Err("This history entry holds a position outside the timeline.".to_owned());
    }
    Ok(())
}

/// Erases every Filter Brush sample between two beats, inclusive.
///
/// A drawn curve is stored as a dense run of samples rather than a single node,
/// so removing one means removing the whole range the gesture wrote, including
/// the bypass samples that close it.
pub fn clear_filter_range(
    connection: &Connection,
    lane: i64,
    start_beat: f64,
    end_beat: f64,
) -> Result<TimelineSnapshot, String> {
    validate_lane(lane)?;
    if !start_beat.is_finite()
        || !end_beat.is_finite()
        || start_beat < 0.0
        || end_beat < start_beat
        || end_beat > MAX_TIMELINE_BEAT
    {
        return Err("That filter range is outside the timeline.".to_owned());
    }
    connection
        .execute(
            "DELETE FROM timeline_filter_nodes
             WHERE lane = ?1 AND beat >= ?2 AND beat <= ?3",
            params![lane, start_beat, end_beat],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

/// Borne et cale un nÅ“ud d'automation sur le quart de temps.
///
/// La rÃ¨gle est la mÃªme pour les deux lignes; seul le mot change dans le
/// message, parce que l'utilisateur doit savoir laquelle il vient de sortir de
/// la timeline.
fn validate_automation_beat(beat: f64, node_label: &str) -> Result<f64, String> {
    if !beat.is_finite() || !(0.0..=MAX_TIMELINE_BEAT).contains(&beat) {
        return Err(format!(
            "The {node_label} position is outside the timeline."
        ));
    }
    Ok((beat * 4.0).round() / 4.0)
}

fn validate_pan_beat(beat: f64) -> Result<f64, String> {
    validate_automation_beat(beat, PAN_AUTOMATION.node_label)
}

fn validate_volume_beat(beat: f64) -> Result<f64, String> {
    validate_automation_beat(beat, VOLUME_AUTOMATION.node_label)
}

fn validate_gain_db(gain_db: Option<f64>) -> Result<(), String> {
    if gain_db.is_some_and(|value| !value.is_finite() || !(-60.0..=12.0).contains(&value)) {
        return Err("Volume must be between -âˆž dB and +12 dB.".to_owned());
    }
    Ok(())
}

pub fn save_clip_eq(
    connection: &Connection,
    clip_id: i64,
    eq_settings: &ClipEqSettings,
) -> Result<TimelineSnapshot, String> {
    let json_str = serde_json::to_string(eq_settings).map_err(|e| e.to_string())?;
    connection
        .execute(
            "UPDATE timeline_clips SET eq_settings = ?1 WHERE id = ?2",
            params![json_str, clip_id],
        )
        .map_err(database_write_error)?;
    snapshot(connection)
}

pub fn split_timeline_clip(
    connection: &mut Connection,
    clip_id: i64,
    split_beat: f64,
) -> Result<TimelineSnapshot, String> {
    let current_clip = connection
        .query_row(
            "SELECT clips.library_track_id, clips.lane, clips.anchor_beat,
                    clips.tempo_anchor_beat, clips.eq_settings,
                    clips.trim_start_beats, clips.trim_end_beats,
                    tracks.duration_ms,
                    COALESCE(tracks.manual_bpm, tracks.bpm),
                    COALESCE(tracks.manual_first_beat_ms, tracks.first_beat_ms)
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             WHERE clips.id = ?1",
            [clip_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, f64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<f64>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                ))
            },
        )
        .optional()
        .map_err(database_read_error)?
        .ok_or_else(|| "The clip to split no longer exists.".to_owned())?;

    let (
        library_track_id,
        lane,
        anchor_beat,
        tempo_anchor_beat,
        eq_settings,
        trim_start,
        trim_end,
        duration_ms,
        bpm,
        first_beat_ms,
    ) = current_clip;

    let geometry = clip_geometry(
        duration_ms,
        bpm,
        first_beat_ms,
        anchor_beat,
        trim_start,
        trim_end,
    );
    if geometry.needs_analysis {
        return Err("This track needs analyzing before it can be split.".to_owned());
    }

    if split_beat <= geometry.visual_start_beat + 0.01
        || split_beat >= geometry.visual_end_beat - 0.01
    {
        return Err("Invalid split point: the playhead has to be inside the clip.".to_owned());
    }

    let offset_beats = split_beat - geometry.visual_start_beat;
    let new_left_trim_end = trim_end + (geometry.duration_beats - offset_beats);
    let new_right_trim_start = trim_start + offset_beats;
    let right_tempo_anchor_beat = tempo_anchor_beat.max(split_beat.round() as i64);

    // Shortening the original clip and creating its right half must land
    // together: a partial split would silently drop the tail of the audio.
    let transaction = connection.transaction().map_err(database_write_error)?;
    transaction
        .execute(
            "UPDATE timeline_clips SET trim_end_beats = ?1 WHERE id = ?2",
            params![new_left_trim_end, clip_id],
        )
        .map_err(database_write_error)?;
    transaction
        .execute(
            "INSERT INTO timeline_clips (library_track_id, lane, anchor_beat, tempo_anchor_beat, eq_settings, trim_start_beats, trim_end_beats, stem)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                     (SELECT stem FROM timeline_clips WHERE id = ?8))",
            params![
                library_track_id,
                lane,
                anchor_beat,
                right_tempo_anchor_beat,
                eq_settings,
                new_right_trim_start,
                trim_end,
                clip_id,
            ],
        )
        .map_err(database_write_error)?;

    // La moitiÃ© droite hÃ©rite des stems de l'originale.
    //
    // Les deux viennent de la mÃªme source, et le fichier sÃ©parÃ© couvre dÃ©jÃ
    // l'Ã©tendue qu'elles se partagent : elles peuvent le dÃ©signer toutes les
    // deux. Sans cela, la nouvelle moitiÃ© retombait sur le morceau complet â€” et
    // comme le clip garde sa touche allumÃ©e, l'affichage et le son se
    // contredisaient.
    let right_id = transaction.last_insert_rowid();
    transaction
        .execute(
            "INSERT INTO clip_stems
             (clip_id, kind, file_path, source_from_ms, bucket_count,
              left_min, left_max, left_rms, right_min, right_max, right_rms)
             SELECT ?1, kind, file_path, source_from_ms, bucket_count,
                    left_min, left_max, left_rms, right_min, right_max, right_rms
             FROM clip_stems WHERE clip_id = ?2",
            params![right_id, clip_id],
        )
        .map_err(database_write_error)?;

    // Et de la cuisson, pour la mÃªme raison : le fichier cuit couvre l'Ã©tendue
    // que les deux moitiÃ©s se partagent, avec la mÃªme origine dans la source.
    // Sans cela, la moitiÃ© droite repartait de l'original â€” donc sans les effets
    // qu'on venait d'y cuire, en gardant sa touche allumÃ©e.
    //
    // `removed` est recopiÃ©e telle quelle : dÃ©cuire l'une ou l'autre rend la
    // mÃªme automation, ce qui est bien ce qu'on veut, puisqu'elle appartenait Ã
    // la voie et non Ã  la moitiÃ©.
    transaction
        .execute(
            "INSERT INTO clip_bakes
             (clip_id, file_path, source_from_ms, removed, bucket_count,
              left_min, left_max, left_rms, right_min, right_max, right_rms)
             SELECT ?1, file_path, source_from_ms, removed, bucket_count,
                    left_min, left_max, left_rms, right_min, right_max, right_rms
             FROM clip_bakes WHERE clip_id = ?2",
            params![right_id, clip_id],
        )
        .map_err(database_write_error)?;
    transaction.commit().map_err(database_write_error)?;

    snapshot(connection)
}

fn validate_filter_value(value: f64) -> Result<(), String> {
    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
        return Err("Filter value must be between -1.0 and +1.0.".to_owned());
    }
    Ok(())
}

fn validate_filter_tension(tension: f64) -> Result<(), String> {
    if !tension.is_finite() || !(-1.0..=1.0).contains(&tension) {
        return Err("Filter curve tension must be between -1.0 and +1.0.".to_owned());
    }
    Ok(())
}

fn snapped_filter_value(value: f64) -> f64 {
    if value.abs() <= 0.05 { 0.0 } else { value }
}

/// Panoramique en vigueur Ã  un beat donnÃ©, interpolÃ© linÃ©airement entre les
/// nÅ“uds voisins. Une piste sans nÅ“ud est au centre.
pub(crate) fn interpolated_pan(nodes: &[TimelinePanNode], lane: i64, beat: f64) -> f64 {
    let lane_nodes = nodes
        .iter()
        .filter(|node| node.lane == lane)
        .collect::<Vec<_>>();
    let previous = lane_nodes.iter().rev().find(|node| node.beat <= beat);
    let next = lane_nodes.iter().find(|node| node.beat >= beat);
    match (previous, next) {
        (Some(previous), Some(next)) => {
            if (next.beat - previous.beat).abs() < f64::EPSILON {
                return next.value;
            }
            let mix = (beat - previous.beat) / (next.beat - previous.beat);
            previous.value + (next.value - previous.value) * mix
        }
        (Some(previous), None) => previous.value,
        (None, Some(next)) => next.value,
        (None, None) => 0.0,
    }
}

fn interpolated_volume_db(nodes: &[TimelineVolumeNode], lane: i64, beat: f64) -> Option<f64> {
    let lane_nodes = nodes
        .iter()
        .filter(|node| node.lane == lane)
        .collect::<Vec<_>>();
    if lane_nodes.is_empty() {
        return Some(0.0);
    }
    let next_index = lane_nodes.partition_point(|node| node.beat < beat);
    match (next_index.checked_sub(1), lane_nodes.get(next_index)) {
        (None, Some(next)) => next.gain_db,
        (Some(previous), None) => lane_nodes[previous].gain_db,
        (Some(previous), Some(next)) => {
            let previous = lane_nodes[previous];
            let span = next.beat - previous.beat;
            if span <= f64::EPSILON {
                return next.gain_db;
            }
            let mix = (beat - previous.beat) / span;
            let previous_db = previous.gain_db.unwrap_or(-60.0);
            let next_db = next.gain_db.unwrap_or(-60.0);
            let value = previous_db + (next_db - previous_db) * mix;
            (value > -59.95).then_some((value * 10.0).round() / 10.0)
        }
        (None, None) => Some(DEFAULT_TRACK_GAIN_DB),
    }
}

fn source_track(connection: &Connection, id: i64) -> Result<SourceTrack, String> {
    let source = connection
        .query_row(
            "SELECT file_path, duration_ms,
                    COALESCE(manual_bpm, bpm),
                    COALESCE(manual_first_beat_ms, first_beat_ms)
             FROM library_tracks
             WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<f64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(database_read_error)?
        .ok_or_else(|| "This track is no longer in the library.".to_owned())?;
    let bpm = source
        .2
        .ok_or_else(|| "Analyze this track's BPM before adding it to the timeline.".to_owned())?;
    validate_bpm(bpm)?;
    let duration_ms =
        u64::try_from(source.1).map_err(|_| "This track's duration is not valid.".to_owned())?;
    let first_beat_ms = source
        .3
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| "This track's first beat is not valid.".to_owned())?;
    if first_beat_ms > duration_ms {
        return Err("The first beat falls past the end of the track.".to_owned());
    }

    Ok(SourceTrack {
        file_path: source.0,
        duration_ms,
        bpm,
        first_beat_ms,
    })
}

fn clips_overlap(start_a: f64, end_a: f64, start_b: f64, end_b: f64) -> bool {
    start_a < end_b - 0.05 && end_a > start_b + 0.05
}

fn clip_geometry(
    duration_ms: i64,
    bpm: Option<f64>,
    first_beat_ms: Option<i64>,
    anchor_beat: i64,
    trim_start_beats: f64,
    trim_end_beats: f64,
) -> ClipGeometry {
    let valid = duration_ms >= 0
        && bpm.is_some_and(|value| value.is_finite() && (MIN_BPM..=MAX_BPM).contains(&value))
        && first_beat_ms.is_some_and(|value| value >= 0 && value <= duration_ms);
    if !valid {
        return ClipGeometry {
            pre_roll_beats: 0.0,
            duration_beats: 0.0,
            visual_start_beat: anchor_beat as f64,
            visual_end_beat: anchor_beat as f64,
            needs_analysis: true,
        };
    }

    let bpm = bpm.unwrap_or_default();
    let first_beat_ms = first_beat_ms.unwrap_or_default() as u64;
    let pre_roll_beats = beats_for_milliseconds(first_beat_ms, bpm);
    let full_duration_beats = beats_for_milliseconds(duration_ms as u64, bpm);
    let duration_beats = (full_duration_beats - trim_start_beats - trim_end_beats).max(0.0);
    let visual_start_beat = (anchor_beat as f64 - pre_roll_beats) + trim_start_beats;

    ClipGeometry {
        pre_roll_beats,
        duration_beats,
        visual_start_beat,
        visual_end_beat: visual_start_beat + duration_beats,
        needs_analysis: false,
    }
}

fn beats_for_milliseconds(milliseconds: u64, bpm: f64) -> f64 {
    milliseconds as f64 * bpm / 60_000.0
}

/// Le temps le plus Ã  gauche oÃ¹ l'ancre d'un clip peut se poser.
///
/// L'ancre porte le **premier temps**, pas le dÃ©but du clip : entre les deux il
/// y a le prÃ©-roll, tout ce que le morceau fait entendre avant sa premiÃ¨re
/// mesure. Ce qu'on ne veut pas, c'est que la partie *visible* commence avant
/// le temps zÃ©ro â€” donc `ancre âˆ’ prÃ©-roll + rognage â‰¥ 0`.
///
/// Le rognage manquait Ã  ce calcul. Un clip dont on avait coupÃ© le dÃ©but
/// restait bloquÃ© Ã  sa position d'origine : la butÃ©e protÃ©geait encore une
/// tÃªte que le clip ne fait plus entendre, et le premier clip d'une timeline
/// refusait de reculer jusqu'Ã  zÃ©ro. On ne borne que ce qui s'entend.
///
/// Le plancher Ã  zÃ©ro reste : le schÃ©ma interdit une ancre nÃ©gative, et un
/// clip dont le prÃ©-roll dÃ©passe le rognage garde donc sa marge.
fn minimum_anchor_beat(pre_roll_beats: f64, trim_start_beats: f64) -> i64 {
    ((pre_roll_beats - trim_start_beats).ceil() as i64).max(0)
}

fn snap_anchor_beat(requested: f64, minimum: i64) -> Result<i64, String> {
    if !requested.is_finite() || requested.abs() > MAX_TIMELINE_BEAT {
        return Err("That position is outside the timeline.".to_owned());
    }

    let nearest_measure = (requested / BEATS_PER_MEASURE as f64).round() as i64 * BEATS_PER_MEASURE;
    let minimum_measure =
        ((minimum + BEATS_PER_MEASURE - 1) / BEATS_PER_MEASURE) * BEATS_PER_MEASURE;
    Ok(nearest_measure.max(minimum_measure))
}

fn snap_tempo_anchor_beat(requested: f64, minimum: f64, maximum: f64) -> Result<i64, String> {
    if !requested.is_finite() || requested.abs() > MAX_TIMELINE_BEAT {
        return Err("That position is outside the timeline.".to_owned());
    }
    let minimum_measure =
        (minimum.max(0.0) / BEATS_PER_MEASURE as f64).ceil() as i64 * BEATS_PER_MEASURE;
    let maximum_measure = (maximum / BEATS_PER_MEASURE as f64).floor() as i64 * BEATS_PER_MEASURE;
    if maximum_measure < minimum_measure {
        return Err("This clip is too short to hold a movable tempo marker.".to_owned());
    }
    let nearest_measure = (requested / BEATS_PER_MEASURE as f64).round() as i64 * BEATS_PER_MEASURE;
    Ok(nearest_measure.clamp(minimum_measure, maximum_measure))
}

fn validate_bpm(bpm: f64) -> Result<(), String> {
    if !bpm.is_finite() || !(MIN_BPM..=MAX_BPM).contains(&bpm) {
        return Err("The project tempo has to be between 40 and 300 BPM.".to_owned());
    }
    Ok(())
}

/// Where an automatic placement starts looking: one lane on from the most
/// recently added clip, so successive drops walk A, B, C rather than piling up.
///
/// The newest clip is the one with the highest id, since ids are handed out in
/// insertion order â€” the order the clips sit in the timeline says nothing about
/// which arrived last.
fn next_rotation_lane(clips: &[TimelineClip]) -> i64 {
    let lane_count = MAX_LANE + 1;
    match clips.iter().max_by_key(|clip| clip.id) {
        Some(newest) => (newest.lane + 1).rem_euclid(lane_count),
        None => 0,
    }
}

fn validate_lane(lane: i64) -> Result<(), String> {
    if !(0..=MAX_LANE).contains(&lane) {
        return Err("The track has to be A, B or C.".to_owned());
    }
    Ok(())
}

// â”€â”€â”€ Bake â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Combien de source on cuit de part et d'autre de ce que le clip fait entendre.
///
/// Sans cette marge, rallonger le rognage aprÃ¨s une cuisson tomberait dans le
/// vide : le fichier s'arrÃªterait exactement lÃ  oÃ¹ le clip s'arrÃªtait. Huit
/// secondes couvrent les retouches ordinaires; au-delÃ , il faut dÃ©cuire.
const BAKE_MARGIN_MS: f64 = 8_000.0;

/// L'automation qu'un bake a emportÃ©e, telle qu'elle Ã©tait.
///
/// C'est ce qui rend l'opÃ©ration rÃ©versible. Sans elle, cuire un effet dans un
/// fichier serait un aller simple â€” et un bouton dont on ne revient pas finit
/// par ne plus Ãªtre cliquÃ© du tout.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemovedAutomation {
    lane: i64,
    from_beat: f64,
    to_beat: f64,
    /// `(temps, gain)`. Le gain est facultatif : `NULL` vaut le silence.
    volume: Vec<(f64, Option<f64>)>,
    pan: Vec<(f64, f64)>,
    /// `(temps, valeur, tension)`.
    filter: Vec<(f64, f64, f64)>,
}

/// Tout ce qu'il faut pour cuire un clip, lu sans rien modifier.
///
/// La lecture et l'Ã©criture sont sÃ©parÃ©es parce que le rendu dure : le verrou
/// de la bibliothÃ¨que est pris pour prÃ©parer, relÃ¢chÃ© pendant la cuisson, puis
/// repris pour ranger le rÃ©sultat.
pub(crate) struct BakeSpec {
    pub plan: TimelineRenderPlan,
    /// OÃ¹ commence le fichier cuit dans la source, en millisecondes.
    pub source_from_ms: f64,
    pub removed: RemovedAutomation,
}

/// PrÃ©pare la cuisson d'un clip : le plan de rendu, et ce qui sera retirÃ©.
///
/// Le plan ne contient **que** ce clip, sur sa voie, avec l'automation de cette
/// voie et son Ã©galisation. Trois choses en sont dÃ©libÃ©rÃ©ment absentes :
///
/// - **L'Ã©tirement temporel.** La carte de tempo est fixÃ©e au tempo propre de la
///   source, donc le rapport vaut un. Cuire un clip dÃ©jÃ  Ã©tirÃ© vers le tempo du
///   projet le ferait Ã©tirer une seconde fois le jour oÃ¹ ce tempo change.
///   L'automation, elle, reste indexÃ©e sur les temps : son alignement avec le
///   son est le mÃªme avant et aprÃ¨s.
/// - **Le compresseur et le limiteur**, qui appartiennent au bus gÃ©nÃ©ral. Les
///   cuire dans un clip les appliquerait deux fois.
/// - **Le sidechain**, qui n'est pas un effet du clip mais une relation avec un
///   autre clip â€” lequel peut encore bouger. FigÃ©, il pomperait Ã  contretemps.
pub(crate) fn prepare_bake(connection: &Connection, clip_id: i64) -> Result<BakeSpec, String> {
    let timeline = snapshot(connection)?;
    let clip = timeline
        .clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| "This clip is no longer on the timeline.".to_owned())?;
    if clip.is_missing {
        return Err(format!(
            "{} is missing. Put the file back before baking it.",
            clip.file_name
        ));
    }
    if clip.is_baked {
        return Err("This clip is already baked.".to_owned());
    }
    let source_bpm = clip
        .bpm
        .ok_or_else(|| format!("{} needs its BPM analyzed first.", clip.file_name))?;
    let first_beat_ms = clip
        .first_beat_ms
        .ok_or_else(|| format!("{} needs its first beat corrected first.", clip.file_name))?;

    let duration_ms = connection
        .query_row(
            "SELECT tracks.duration_ms
             FROM timeline_clips AS clips
             JOIN library_tracks AS tracks ON tracks.id = clips.library_track_id
             WHERE clips.id = ?1",
            params![clip_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(database_read_error)?;

    // Le clip Ã©largi de sa marge, des deux cÃ´tÃ©s. `clip_geometry` recalcule
    // alors une Ã©tendue plus longue qui commence plus tÃ´t â€” c'est elle qu'on
    // rend, et le rognage d'origine s'y retrouvera par simple soustraction.
    let beat_ms = 60_000.0 / source_bpm;
    let margin_beats = BAKE_MARGIN_MS / beat_ms;
    let baked_trim_start = (clip.trim_start_beats - margin_beats).max(0.0);
    let baked_trim_end = (clip.trim_end_beats - margin_beats).max(0.0);
    let geometry = clip_geometry(
        duration_ms,
        Some(source_bpm),
        Some(first_beat_ms as i64),
        clip.anchor_beat,
        baked_trim_start,
        baked_trim_end,
    );
    if geometry.needs_analysis || geometry.duration_beats <= 0.0 {
        return Err(format!("{} has nothing to bake.", clip.file_name));
    }

    // La source que le clip joue **aujourd'hui** : son stem, s'il en a choisi
    // un. Cuire l'original alors qu'on entend la voix seule produirait un
    // fichier qui ne ressemble pas Ã  ce qu'on Ã©coutait.
    let (file_path, stem_from_ms) =
        clip_audio_source(connection, clip.id, &clip.stem, &clip.file_path);
    let stem_trim_beats = if stem_from_ms > 0.0 {
        stem_from_ms / beat_ms
    } else {
        0.0
    };

    let plan = TimelineRenderPlan {
        project_bpm: source_bpm,
        tempo_map: TempoMap::new(source_bpm, Vec::new())?,
        end_beat: geometry.visual_end_beat,
        audible_lane_mask: 1 << clip.lane,
        limiter_enabled: false,
        compressor_enabled: false,
        clips: vec![TimelineRenderClip {
            id: clip.id,
            lane: clip.lane,
            file_path,
            source_bpm,
            first_beat_ms,
            anchor_beat: clip.anchor_beat as f64,
            visual_start_beat: geometry.visual_start_beat,
            duration_beats: geometry.duration_beats,
            trim_start_beats: (baked_trim_start - stem_trim_beats).max(0.0),
            trim_end_beats: baked_trim_end,
            is_sidechain_key: false,
            eq_settings: clip.eq_settings.clone(),
        }],
        volume_nodes: timeline.volume_nodes.clone(),
        pan_nodes: timeline.pan_nodes.clone(),
        filter_nodes: timeline.filter_nodes.clone(),
    };

    Ok(BakeSpec {
        removed: collect_removed_automation(
            &timeline,
            clip.lane,
            geometry.visual_start_beat,
            geometry.visual_end_beat,
        ),
        source_from_ms: baked_trim_start * beat_ms,
        plan,
    })
}

/// Ce que la cuisson emportera : l'automation de la voie sur l'Ã©tendue rendue.
fn collect_removed_automation(
    timeline: &TimelineSnapshot,
    lane: i64,
    from_beat: f64,
    to_beat: f64,
) -> RemovedAutomation {
    let inside =
        |node_lane: i64, beat: f64| node_lane == lane && beat >= from_beat && beat <= to_beat;
    RemovedAutomation {
        lane,
        from_beat,
        to_beat,
        volume: timeline
            .volume_nodes
            .iter()
            .filter(|node| inside(node.lane, node.beat))
            .map(|node| (node.beat, node.gain_db))
            .collect(),
        pan: timeline
            .pan_nodes
            .iter()
            .filter(|node| inside(node.lane, node.beat))
            .map(|node| (node.beat, node.value))
            .collect(),
        filter: timeline
            .filter_nodes
            .iter()
            .filter(|node| inside(node.lane, node.beat))
            .map(|node| (node.beat, node.value, node.tension))
            .collect(),
    }
}

/// Range le fichier cuit et retire l'automation qu'il contient dÃ©sormais.
///
/// Une seule transaction : un enregistrement Ã©crit sans que l'automation parte
/// donnerait un clip qui joue ses effets deux fois â€” une fois dans le fichier,
/// une fois par la voie.
pub fn commit_bake(
    connection: &mut Connection,
    clip_id: i64,
    file_path: &str,
    source_from_ms: f64,
    removed: &RemovedAutomation,
    waveform: Option<&WaveformPeaks>,
) -> Result<TimelineSnapshot, String> {
    let serialised = serde_json::to_string(removed)
        .map_err(|error| format!("The removed automation could not be stored: {error}"))?;
    let transaction = connection.transaction().map_err(database_write_error)?;

    let (bucket_count, left_min, left_max, left_rms, right_min, right_max, right_rms) =
        match waveform {
            Some(peaks) => (
                Some(peaks.left_min.len() as i64),
                Some(encode_waveform_values(&peaks.left_min)),
                Some(encode_waveform_values(&peaks.left_max)),
                Some(encode_waveform_values(&peaks.left_rms)),
                Some(encode_waveform_values(&peaks.right_min)),
                Some(encode_waveform_values(&peaks.right_max)),
                Some(encode_waveform_values(&peaks.right_rms)),
            ),
            None => (None, None, None, None, None, None, None),
        };

    transaction
        .execute(
            "INSERT INTO clip_bakes
             (clip_id, file_path, source_from_ms, removed, bucket_count,
              left_min, left_max, left_rms, right_min, right_max, right_rms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(clip_id) DO UPDATE SET
                 file_path = excluded.file_path,
                 source_from_ms = excluded.source_from_ms,
                 removed = excluded.removed,
                 bucket_count = excluded.bucket_count,
                 left_min = excluded.left_min,
                 left_max = excluded.left_max,
                 left_rms = excluded.left_rms,
                 right_min = excluded.right_min,
                 right_max = excluded.right_max,
                 right_rms = excluded.right_rms",
            params![
                clip_id,
                file_path,
                source_from_ms.max(0.0).round() as i64,
                serialised,
                bucket_count,
                left_min,
                left_max,
                left_rms,
                right_min,
                right_max,
                right_rms,
            ],
        )
        .map_err(database_write_error)?;

    clear_lane_automation(&transaction, removed)?;
    transaction.commit().map_err(database_write_error)?;
    snapshot(connection)
}

/// Efface l'automation de la voie sur l'Ã©tendue cuite, et referme les bords.
///
/// Les deux nÅ“uds de repos ne sont pas une politesse : la voie continue aprÃ¨s le
/// clip, et sans eux la ligne rejoindrait le nÅ“ud suivant en rampant depuis le
/// dernier nÅ“ud d'avant â€” de l'automation que personne n'a demandÃ©e, en travers
/// de ce qui suit.
fn clear_lane_automation(
    transaction: &Transaction<'_>,
    removed: &RemovedAutomation,
) -> Result<(), String> {
    for (table, column, rest) in [
        ("timeline_volume_nodes", "gain_db", DEFAULT_TRACK_GAIN_DB),
        ("timeline_pan_nodes", "value", PAN_CENTRE),
        ("timeline_filter_nodes", "value", 0.0),
    ] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE lane = ?1 AND beat >= ?2 AND beat <= ?3"),
                params![removed.lane, removed.from_beat, removed.to_beat],
            )
            .map_err(database_write_error)?;
        for beat in [removed.from_beat, removed.to_beat] {
            transaction
                .execute(
                    &format!(
                        "INSERT INTO {table} (lane, beat, {column}) VALUES (?1, ?2, ?3)
                         ON CONFLICT(lane, beat) DO UPDATE SET {column} = excluded.{column}"
                    ),
                    params![removed.lane, beat, rest],
                )
                .map_err(database_write_error)?;
        }
    }
    Ok(())
}

/// DÃ©fait une cuisson : l'automation revient, le fichier s'en va.
///
/// Ce qui a Ã©tÃ© dessinÃ© **depuis** la cuisson, sur cette voie et sur cette
/// Ã©tendue, est remplacÃ© â€” deux automations ne peuvent pas occuper les mÃªmes
/// temps. L'annulation le couvre, et le bouton le dit.
///
/// Le chemin du fichier est renvoyÃ© pour que l'appelant l'efface : le faire ici
/// mettrait une Ã©criture sur disque dans une transaction de base de donnÃ©es, et
/// un Ã©chec Ã  la validation laisserait un enregistrement sans son fichier.
pub fn unbake_clip(
    connection: &mut Connection,
    clip_id: i64,
) -> Result<(TimelineSnapshot, Option<String>), String> {
    let record = connection
        .query_row(
            "SELECT file_path, removed FROM clip_bakes WHERE clip_id = ?1",
            params![clip_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(database_read_error)?;
    let Some((file_path, serialised)) = record else {
        return Err("This clip is not baked.".to_owned());
    };
    let removed: RemovedAutomation = serde_json::from_str(&serialised)
        .map_err(|error| format!("The stored automation could not be read: {error}"))?;

    let transaction = connection.transaction().map_err(database_write_error)?;
    for table in [
        "timeline_volume_nodes",
        "timeline_pan_nodes",
        "timeline_filter_nodes",
    ] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE lane = ?1 AND beat >= ?2 AND beat <= ?3"),
                params![removed.lane, removed.from_beat, removed.to_beat],
            )
            .map_err(database_write_error)?;
    }
    for (beat, gain_db) in &removed.volume {
        transaction
            .execute(
                "INSERT INTO timeline_volume_nodes (lane, beat, gain_db) VALUES (?1, ?2, ?3)",
                params![removed.lane, beat, gain_db],
            )
            .map_err(database_write_error)?;
    }
    for (beat, value) in &removed.pan {
        transaction
            .execute(
                "INSERT INTO timeline_pan_nodes (lane, beat, value) VALUES (?1, ?2, ?3)",
                params![removed.lane, beat, value],
            )
            .map_err(database_write_error)?;
    }
    for (beat, value, tension) in &removed.filter {
        transaction
            .execute(
                "INSERT INTO timeline_filter_nodes (lane, beat, value, tension)
                 VALUES (?1, ?2, ?3, ?4)",
                params![removed.lane, beat, value, tension],
            )
            .map_err(database_write_error)?;
    }
    transaction
        .execute(
            "DELETE FROM clip_bakes WHERE clip_id = ?1",
            params![clip_id],
        )
        .map_err(database_write_error)?;
    transaction.commit().map_err(database_write_error)?;

    Ok((snapshot(connection)?, Some(file_path)))
}

fn rounded_bpm(bpm: f64) -> f64 {
    (bpm * 1_000.0).round() / 1_000.0
}

fn database_read_error(error: rusqlite::Error) -> String {
    format!("Could not read the timeline: {error}")
}

fn database_write_error(error: rusqlite::Error) -> String {
    format!("Could not write to the timeline: {error}")
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_TRACK_GAIN_DB, FILTER_BUBBLE_MAX_SAMPLES, FILTER_BUBBLE_MAX_WIDTH_BEATS,
        FILTER_BUBBLE_STEP_BEATS, TimelineLane, TimelineSnapshot, add_clip, add_filter_node,
        add_pan_node, add_volume_node, audible_lane_mask, clear_filter_range, clear_timeline,
        clip_geometry, clips_overlap, draw_filter_bubble, filter_bubble_step_beats,
        minimum_anchor_beat, move_clip, move_tempo_point, move_volume_node, project_timing,
        remove_clip, restore_snapshot, set_clip_trim, set_lane_muted, set_lane_solo,
        set_sidechain_key, snap_anchor_beat, snap_tempo_anchor_beat, snapshot, split_timeline_clip,
    };
    use crate::library::LibraryStore;
    use rusqlite::params;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn geometry_places_the_source_first_beat_on_its_anchor() {
        let geometry = clip_geometry(120_000, Some(120.0), Some(500), 8, 0.0, 0.0);

        assert!(!geometry.needs_analysis);
        assert!((geometry.pre_roll_beats - 1.0).abs() < f64::EPSILON);
        assert!((geometry.duration_beats - 240.0).abs() < f64::EPSILON);
        assert!((geometry.visual_start_beat - 7.0).abs() < f64::EPSILON);
        assert!((geometry.visual_end_beat - 247.0).abs() < f64::EPSILON);
    }

    #[test]
    fn minimum_anchor_keeps_audio_after_project_start() {
        assert_eq!(minimum_anchor_beat(0.0, 0.0), 0);
        assert_eq!(minimum_anchor_beat(0.2, 0.0), 1);
        assert_eq!(minimum_anchor_beat(2.0, 0.0), 2);
    }

    /// Un clip rognÃ© du dÃ©but doit pouvoir reculer d'autant.
    ///
    /// La butÃ©e protÃ©geait le prÃ©-roll entier, y compris la part qu'on venait
    /// de couper : le premier clip d'une timeline refusait de reculer jusqu'Ã
    /// zÃ©ro, retenu par une tÃªte qu'il ne fait plus entendre.
    #[test]
    fn trimming_the_head_lets_a_clip_move_that_much_further_left() {
        // Huit temps de prÃ©-roll, deux coupÃ©s : il reste six temps Ã  protÃ©ger.
        assert_eq!(minimum_anchor_beat(8.0, 2.0), 6);
        // Tout le prÃ©-roll coupÃ© : le clip commence sur son premier temps et
        // peut donc se poser au tout dÃ©but.
        assert_eq!(minimum_anchor_beat(8.0, 8.0), 0);
        // RognÃ© au-delÃ  : on ne descend pas sous zÃ©ro, le schÃ©ma l'interdit.
        assert_eq!(minimum_anchor_beat(8.0, 40.0), 0);
        // Et la marge d'un clip non rognÃ© ne bouge pas d'un pouce.
        assert_eq!(minimum_anchor_beat(8.4, 0.0), 9);
    }

    /// La butÃ©e et la gÃ©omÃ©trie doivent parler de la mÃªme chose : Ã  l'ancre
    /// minimale, le clip visible commence Ã  zÃ©ro ou aprÃ¨s â€” jamais avant.
    #[test]
    fn the_clamp_and_the_geometry_agree_on_where_zero_is() {
        for (duration_ms, bpm, first_beat_ms, trim_start) in [
            (300_000_i64, 120.0_f64, 4_000_i64, 0.0_f64),
            (300_000, 120.0, 4_000, 3.0),
            (300_000, 128.0, 15_000, 12.0),
            (300_000, 174.0, 500, 60.0),
        ] {
            let probe = clip_geometry(
                duration_ms,
                Some(bpm),
                Some(first_beat_ms),
                0,
                trim_start,
                0.0,
            );
            let minimum = minimum_anchor_beat(probe.pre_roll_beats, trim_start);
            let placed = clip_geometry(
                duration_ms,
                Some(bpm),
                Some(first_beat_ms),
                minimum,
                trim_start,
                0.0,
            );
            assert!(
                placed.visual_start_beat >= -1e-9,
                "Ã  l'ancre minimale le clip commencerait Ã  {} â€” avant le dÃ©but",
                placed.visual_start_beat
            );
            // Quand la butÃ©e est ce qui retient, elle doit retenir *juste* :
            // pas un temps de jeu perdu. Quand elle est dÃ©jÃ  Ã  zÃ©ro, c'est le
            // plancher du schÃ©ma â€” une ancre ne peut pas Ãªtre nÃ©gative â€” et le
            // reste est irrÃ©ductible : un clip rognÃ© plus loin que son
            // prÃ©-roll ne peut pas commencer au temps zÃ©ro.
            assert!(
                placed.visual_start_beat < 1.0 || minimum == 0,
                "la butÃ©e laisse {} temps de jeu inutiles avant le dÃ©but",
                placed.visual_start_beat
            );
        }
    }

    #[test]
    fn requested_positions_snap_to_four_beat_measures_and_clamp() {
        assert_eq!(snap_anchor_beat(5.99, 0).expect("position should snap"), 4);
        assert_eq!(snap_anchor_beat(6.0, 0).expect("position should snap"), 8);
        assert_eq!(snap_anchor_beat(13.2, 0).expect("position should snap"), 12);
        assert_eq!(
            snap_anchor_beat(-12.0, 2).expect("position should clamp"),
            4
        );
        assert_eq!(snap_anchor_beat(8.0, 9).expect("position should clamp"), 12);
        assert!(snap_anchor_beat(f64::NAN, 0).is_err());
    }

    #[test]
    fn tempo_targets_snap_within_their_clip_bounds() {
        assert_eq!(
            snap_tempo_anchor_beat(26.1, 8.2, 27.9).expect("target should snap"),
            24
        );
        assert_eq!(
            snap_tempo_anchor_beat(50.0, 8.2, 27.9).expect("target should clamp"),
            24
        );
        assert!(snap_tempo_anchor_beat(8.0, 8.2, 11.9).is_err());
    }

    #[test]
    fn mute_and_solo_produce_the_expected_live_lane_mask() {
        let mut snapshot = TimelineSnapshot {
            project_bpm: 120.0,
            limiter_enabled: true,
            compressor_enabled: false,
            tempo_points: vec![crate::tempo::TempoPoint::project_start(120.0)],
            lanes: vec![
                TimelineLane {
                    lane: 0,
                    is_muted: false,
                    is_solo: false,
                },
                TimelineLane {
                    lane: 1,
                    is_muted: true,
                    is_solo: false,
                },
                TimelineLane {
                    lane: 2,
                    is_muted: false,
                    is_solo: false,
                },
            ],
            clips: Vec::new(),
            volume_nodes: Vec::new(),
            pan_nodes: Vec::new(),
            draw_groups: Vec::new(),
            filter_nodes: Vec::new(),
        };
        assert_eq!(audible_lane_mask(&snapshot), 0b101);

        snapshot.lanes[2].is_solo = true;
        assert_eq!(audible_lane_mask(&snapshot), 0b100);

        snapshot.lanes[2].is_muted = true;
        assert_eq!(audible_lane_mask(&snapshot), 0);
    }

    #[test]
    fn trimming_hides_material_without_moving_the_clip() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-trim-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'trim.mp3', 60000, 44100, 2,
                             120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            let added = add_clip(&mut store.connection, track_id, Some(8.0), Some(0))
                .expect("clip should be added");
            let clip_id = added.clips[0].id;
            let anchor = added.clips[0].anchor_beat;
            let whole_start = added.clips[0].visual_start_beat;
            let whole_end = added.clips[0].visual_end_beat;

            // Head trimmed: the start moves in, the tail does not budge, and
            // the anchor stays put so the audio keeps its place on the grid.
            let trimmed = set_clip_trim(&mut store.connection, clip_id, 6.0, 0.0)
                .expect("the head should trim");
            let clip = &trimmed.clips[0];
            assert_eq!(clip.anchor_beat, anchor, "a trim must not move the clip");
            assert!((clip.visual_start_beat - (whole_start + 6.0)).abs() < 1e-9);
            assert!((clip.visual_end_beat - whole_end).abs() < 1e-9);

            // Tail trimmed as well, from the other side.
            let both = set_clip_trim(&mut store.connection, clip_id, 6.0, 10.0)
                .expect("the tail should trim");
            assert!((both.clips[0].visual_end_beat - (whole_end - 10.0)).abs() < 1e-9);

            // And back out again: a trim is not a destructive edit.
            let restored = set_clip_trim(&mut store.connection, clip_id, 0.0, 0.0)
                .expect("the clip should be given its material back");
            assert!((restored.clips[0].visual_start_beat - whole_start).abs() < 1e-9);
            assert!((restored.clips[0].visual_end_beat - whole_end).abs() < 1e-9);

            let gone = set_clip_trim(&mut store.connection, clip_id, 1000.0, 0.0)
                .expect_err("a clip cannot be trimmed out of existence");
            assert!(gone.contains("entirely"), "got {gone}");

            // A neighbour on the same lane is in the way of growing back.
            let second = add_clip(&mut store.connection, track_id, None, Some(0))
                .expect("a second clip should queue behind the first");
            let second_id = second
                .clips
                .iter()
                .max_by_key(|clip| clip.id)
                .expect("the new clip should be present")
                .id;
            set_clip_trim(&mut store.connection, second_id, 20.0, 0.0)
                .expect("the second clip should trim its head");
            let collision = set_clip_trim(&mut store.connection, clip_id, 0.0, 0.0)
                .and_then(|_| set_clip_trim(&mut store.connection, second_id, 0.0, 0.0));
            assert!(
                collision.is_ok(),
                "growing back into space it already owned should be allowed"
            );
        }

        let _ = fs::remove_file(&fake_mp3);
        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            if candidate.exists() {
                let _ = fs::remove_file(candidate);
            }
        }
    }

    /// Chaque table d'automation ajoutÃ©e doit Ãªtre inscrite dans les deux
    /// endroits qui les Ã©numÃ¨rent : `clear_timeline` et `restore_snapshot`. Le
    /// panoramique avait Ã©tÃ© oubliÃ© dans les deux â€” CLEAR le laissait derriÃ¨re,
    /// et un Undo rÃ©Ã©crivait tout sauf lui.
    #[test]
    fn clearing_and_restoring_account_for_every_automation_table() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-clear-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'clear.mp3', 60000, 44100, 2, 120.0, 0, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();
            add_clip(&mut store.connection, track_id, Some(0.0), Some(0))
                .expect("a clip should be added");
            add_volume_node(&store.connection, 0, 4.0).expect("a volume node");
            add_pan_node(&store.connection, 0, 4.0).expect("a pan node");
            add_filter_node(&store.connection, 0, 4.0, 0.5).expect("a filter node");

            let full = snapshot(&store.connection).expect("the timeline should read");
            assert!(
                !full.pan_nodes.is_empty(),
                "the pan node should be there to begin with"
            );

            // Un Undo doit rÃ©tablir le panoramique comme le reste.
            let cleared = clear_timeline(&store.connection).expect("the timeline should clear");
            assert!(cleared.clips.is_empty(), "clips should go");
            assert!(cleared.volume_nodes.is_empty(), "volume nodes should go");
            assert!(cleared.filter_nodes.is_empty(), "filter nodes should go");
            assert!(cleared.pan_nodes.is_empty(), "pan nodes should go too");

            let restored =
                restore_snapshot(&mut store.connection, &full).expect("the history should restore");
            assert_eq!(
                restored.pan_nodes.len(),
                full.pan_nodes.len(),
                "an Undo should bring the pan automation back"
            );
            assert_eq!(restored.volume_nodes.len(), full.volume_nodes.len());
        }

        let _ = fs::remove_file(&fake_mp3);
        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            if candidate.exists() {
                let _ = fs::remove_file(candidate);
            }
        }
    }

    #[test]
    fn an_automatic_placement_skips_the_tracks_that_are_busy_at_the_playhead() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-rotation-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'rotation.mp3', 60000, 44100, 2,
                             120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            // Three clips dropped at the same point walk A, B, C.
            let mut lanes = Vec::new();
            for _ in 0..3 {
                let snapshot = add_clip(&mut store.connection, track_id, Some(8.0), None)
                    .expect("each drop should find a free lane");
                lanes.push(
                    snapshot
                        .clips
                        .iter()
                        .max_by_key(|clip| clip.id)
                        .expect("the clip just added should be in the snapshot")
                        .lane,
                );
            }
            assert_eq!(lanes, vec![0, 1, 2], "placements should walk the lanes");

            // The regression: a fourth drop at the same point brought the
            // rotation back round to A, which is busy. Rotation only chooses
            // where to start looking, so this has to report that there is
            // genuinely nowhere left rather than refuse a lane it never tried.
            let full = add_clip(&mut store.connection, track_id, Some(8.0), None)
                .expect_err("a fourth clip at the same point has nowhere to go");
            assert!(
                full.contains("All three tracks"),
                "the message should say every lane was tried, got {full}"
            );

            // Move one out of the way and the next drop takes the lane it freed.
            let occupant = add_clip(&mut store.connection, track_id, Some(400.0), Some(1))
                .expect("a clip well past the playhead should be accepted");
            let stray = occupant
                .clips
                .iter()
                .find(|clip| clip.lane == 1 && clip.anchor_beat < 100)
                .expect("lane B should still hold its original clip")
                .id;
            remove_clip(&mut store.connection, stray).expect("the stray clip should be removed");
            let reused = add_clip(&mut store.connection, track_id, Some(8.0), None)
                .expect("the freed lane should be available again");
            assert_eq!(
                reused
                    .clips
                    .iter()
                    .max_by_key(|clip| clip.id)
                    .expect("the clip just added should be in the snapshot")
                    .lane,
                1,
                "the drop should land on the lane that was freed"
            );
        }

        let _ = fs::remove_file(&fake_mp3);
        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            if candidate.exists() {
                let _ = fs::remove_file(candidate);
            }
        }
    }

    #[test]
    fn clips_persist_and_an_automatic_append_starts_after_the_previous_audio() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-timeline-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'timeline.mp3', 60000, 44100, 2,
                             120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            let first = add_clip(&mut store.connection, track_id, Some(0.2), Some(0))
                .expect("first clip should be added");
            assert_eq!(first.project_bpm, 120.0);
            assert_eq!(first.clips[0].anchor_beat, 4);
            assert_eq!(first.clips[0].tempo_anchor_beat, 4);
            assert!((first.clips[0].visual_start_beat - 3.0).abs() < f64::EPSILON);

            // Named lane, no anchor: queue behind what that lane already holds.
            // Rotation is covered on its own; this test is about the append.
            let second = add_clip(&mut store.connection, track_id, None, Some(0))
                .expect("second clip should append");
            assert_eq!(second.clips.len(), 2);
            assert_eq!(second.clips[1].anchor_beat, 124);
            assert!((second.clips[1].visual_start_beat - 123.0).abs() < f64::EPSILON);
            let third = add_clip(&mut store.connection, track_id, None, Some(2))
                .expect("third clip should start on the empty third lane");
            assert_eq!(third.clips[2].lane, 2);
            assert_eq!(third.clips[2].anchor_beat, 4);
            assert!(
                third
                    .volume_nodes
                    .iter()
                    .any(|node| node.lane == 2 && (node.beat - 3.0).abs() < f64::EPSILON)
            );
            assert!(
                third
                    .volume_nodes
                    .iter()
                    .any(|node| node.lane == 2 && (node.beat - 123.0).abs() < f64::EPSILON)
            );
            let with_external_node = add_volume_node(&store.connection, 2, 200.0)
                .expect("an external volume node should be added");
            assert!(
                with_external_node
                    .volume_nodes
                    .iter()
                    .any(|node| node.lane == 2 && (node.beat - 200.0).abs() < f64::EPSILON)
            );
            let moved = move_clip(&mut store.connection, third.clips[2].id, 8.2, 1)
                .expect("clip should move between lanes");
            let moved_clip = moved
                .clips
                .iter()
                .find(|clip| clip.id == third.clips[2].id)
                .expect("moved clip should remain in snapshot");
            assert_eq!(moved_clip.lane, 1);
            assert_eq!(moved_clip.anchor_beat, 8);
            assert_eq!(moved_clip.tempo_anchor_beat, 8);
            assert!(
                moved
                    .volume_nodes
                    .iter()
                    .any(|node| node.lane == 1 && (node.beat - 7.0).abs() < f64::EPSILON)
            );
            assert!(
                moved
                    .volume_nodes
                    .iter()
                    .any(|node| node.lane == 1 && (node.beat - 127.0).abs() < f64::EPSILON)
            );
            assert!(
                moved
                    .volume_nodes
                    .iter()
                    .any(|node| node.lane == 2 && (node.beat - 200.0).abs() < f64::EPSILON)
            );
            let retimed = move_tempo_point(&store.connection, moved_clip.id, 20.0)
                .expect("tempo target should move without moving the clip");
            let retimed_clip = retimed
                .clips
                .iter()
                .find(|clip| clip.id == moved_clip.id)
                .expect("retimed clip should remain in snapshot");
            assert_eq!(retimed_clip.anchor_beat, 8);
            assert_eq!(retimed_clip.tempo_anchor_beat, 20);
            let muted =
                set_lane_muted(&store.connection, 1, true).expect("lane mute should be persisted");
            assert!(muted.lanes[1].is_muted);
            let soloed =
                set_lane_solo(&store.connection, 2, true).expect("lane solo should be persisted");
            assert!(soloed.lanes[2].is_solo);
            let timing = project_timing(&store.connection).expect("timing should be available");
            assert_eq!(timing.0.bpm_at_beat(0.0), 120.0);
            assert!((timing.1 - 243.0).abs() < f64::EPSILON);

            let with_node =
                add_volume_node(&store.connection, 1, 8.1).expect("volume node should be added");
            let node = *with_node
                .volume_nodes
                .iter()
                .find(|node| node.lane == 1 && (node.beat - 8.0).abs() < f64::EPSILON)
                .expect("manual node should be present");
            let moved_node = move_volume_node(&store.connection, node.id, 12.0, Some(-9.0))
                .expect("volume node should move");
            let moved_node = moved_node
                .volume_nodes
                .iter()
                .find(|candidate| candidate.id == node.id)
                .expect("manual node should remain present");
            assert_eq!(moved_node.beat, 12.0);
            assert_eq!(moved_node.gain_db, Some(-9.0));

            let brushed_filter =
                draw_filter_bubble(&mut store.connection, 1, 24.0, 4.0, -0.8, None, None)
                    .expect("filter brush should replace its range atomically");
            // A default brush is a `ramp_up`: it starts at bypass on its first
            // beat, reaches the requested value on its last beat, then returns
            // to bypass just after the bubble.
            // (Erasing this whole run is covered by `clear_filter_range`.)
            assert!(brushed_filter.filter_nodes.iter().any(|node| node.lane == 1
                && (node.beat - 24.0).abs() < f64::EPSILON
                && node.value == 0.0));
            assert!(brushed_filter.filter_nodes.iter().any(|node| node.lane == 1
                && (node.beat - 26.0).abs() < f64::EPSILON
                && (node.value + 0.4).abs() < 1.0e-9));
            assert!(brushed_filter.filter_nodes.iter().any(|node| node.lane == 1
                && (node.beat - 28.0).abs() < f64::EPSILON
                && (node.value + 0.8).abs() < 1.0e-9));
            assert!(brushed_filter.filter_nodes.iter().any(|node| node.lane == 1
                && (node.beat - 28.01).abs() < 1.0e-9
                && node.value == 0.0));

            store
                .update_beatgrid_correction(track_id, 120.0, 1_000)
                .expect("first downbeat correction should save");
            let corrected = snapshot(&store.connection)
                .expect("an existing timeline clip should adopt the corrected downbeat");
            assert_eq!(corrected.clips[0].first_beat_ms, Some(1_000));
            assert!((corrected.clips[0].visual_start_beat - 2.0).abs() < f64::EPSILON);
        }

        {
            let store = LibraryStore::open(&database_path).expect("database should reopen");
            let restored = snapshot(&store.connection).expect("timeline should be restored");
            assert_eq!(restored.clips.len(), 3);
            assert!(restored.clips.iter().any(|clip| clip.lane == 1));
            assert!(restored.lanes[1].is_muted);
            assert!(restored.lanes[2].is_solo);
            assert_eq!(restored.volume_nodes.len(), 7);
            // Les nÅ“uds posÃ©s automatiquement portent le niveau par dÃ©faut;
            // seul celui dÃ©placÃ© Ã  la main s'en Ã©carte. L'assertion nommait
            // autrefois `-6.0` des deux cÃ´tÃ©s et rÃ©ussissait donc par
            // coÃ¯ncidence, sans distinguer les deux.
            let (moved, automatic): (
                Vec<&super::TimelineVolumeNode>,
                Vec<&super::TimelineVolumeNode>,
            ) = restored
                .volume_nodes
                .iter()
                .partition(|node| node.gain_db == Some(-9.0));
            assert_eq!(moved.len(), 1, "the hand-moved node should keep its level");
            assert!(
                automatic
                    .iter()
                    .all(|node| node.gain_db == Some(DEFAULT_TRACK_GAIN_DB))
            );
            // La courbe de filtre survit Ã  la rÃ©ouverture. Elle est dÃ©signÃ©e
            // par sa crÃªte telle que le pinceau l'a posÃ©e : le nÅ“ud isolÃ©
            // qu'on plaÃ§ait ici autrefois n'est plus atteignable depuis
            // l'interface, et l'affirmer aurait vÃ©rifiÃ© du code mort.
            assert!(restored.filter_nodes.iter().any(|node| node.lane == 1
                && (node.beat - 28.0).abs() < f64::EPSILON
                && (node.value + 0.8).abs() < 1.0e-9));
            assert_eq!(restored.clips[0].first_beat_ms, Some(1_000));
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

    /// `project_timing` feeds the transport while `render_plan` feeds the audio
    /// engine. If the two disagree, `matches_timing` rejects the engine's own
    /// cache and Seek silently stops working, so they are compared directly
    /// after the two edits that used to break them apart.
    #[test]
    fn transport_timing_matches_the_render_plan_after_a_tempo_move_and_a_split() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-timing-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        let assert_agrees = |connection: &rusqlite::Connection, stage: &str| {
            let timeline = snapshot(connection).expect("snapshot should read");
            let (transport_map, transport_end) =
                project_timing(connection).expect("timing should read");
            let plan_map =
                crate::tempo::TempoMap::new(timeline.project_bpm, timeline.tempo_points.clone())
                    .expect("the snapshot tempo points should rebuild");
            let plan_end = timeline
                .clips
                .iter()
                .fold(0.0_f64, |end, clip| end.max(clip.visual_end_beat));

            assert_eq!(
                transport_map.signature(),
                plan_map.signature(),
                "tempo signatures diverged {stage}"
            );
            assert!(
                (transport_end - plan_end).abs() < 1.0e-9,
                "end beat diverged {stage}: {transport_end} vs {plan_end}"
            );
        };

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'timing.mp3', 60000, 44100, 2,
                             120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            let added = add_clip(&mut store.connection, track_id, Some(4.0), Some(0))
                .expect("clip should be added");
            let clip_id = added.clips[0].id;
            assert_agrees(&store.connection, "after adding a clip");

            // Dragging the turquoise node moves `tempo_anchor_beat` only.
            move_tempo_point(&store.connection, clip_id, 40.0).expect("tempo target should move");
            assert_agrees(&store.connection, "after moving the tempo target");

            let split = split_timeline_clip(&mut store.connection, clip_id, 43.0)
                .expect("clip should split");
            assert_agrees(&store.connection, "after splitting the clip");

            // Removing the tail leaves a clip whose audio stops before the end
            // of its source file: the project length must follow the trim.
            let right_id = split
                .clips
                .iter()
                .map(|clip| clip.id)
                .find(|id| *id != clip_id)
                .expect("the split creates a right subclip");
            super::remove_clip(&mut store.connection, right_id).expect("subclip should be removed");
            assert_agrees(&store.connection, "after removing the trimmed tail");

            let (_, end_beat) = project_timing(&store.connection).expect("timing should read");
            assert!(
                (end_beat - 43.0).abs() < 1.0e-9,
                "the trimmed project should end on the split, not on the full source: {end_beat}"
            );
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

    /// Le dÃ©faut du 3 aoÃ»t : rÃ©gler le tempo d'un nÅ“ud corrigeait le BPM du
    /// **morceau**. Les deux valeurs bougeaient donc ensemble, le clip gardait
    /// un ratio de un pour un au lieu d'accÃ©lÃ©rer, la courbe se dÃ©plaÃ§ait sous
    /// tous les autres clips â€” et l'analyse de la bibliothÃ¨que Ã©tait Ã©crasÃ©e
    /// sans retour.
    #[test]
    fn a_clip_tempo_target_stretches_the_clip_and_leaves_the_library_alone() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-target-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'target.mp3', 60000, 44100, 2,
                             120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            let added = add_clip(&mut store.connection, track_id, Some(0.0), Some(0))
                .expect("clip should be added");
            let clip_id = added.clips[0].id;

            let after = super::set_clip_tempo_target(&store.connection, clip_id, Some(128.0))
                .expect("the target should be written");
            let clip = &after.clips[0];

            // La vitesse native du morceau ne bouge pas : c'est elle qui donne
            // le ratio d'Ã©tirement, et l'Ã©craser Ã©tait tout le dÃ©faut.
            assert_eq!(clip.bpm, Some(120.0));
            assert_eq!(clip.tempo_target_bpm, Some(128.0));

            // Et la bibliothÃ¨que n'a rien reÃ§u : aucune correction manuelle.
            let manual: Option<f64> = store
                .connection
                .query_row(
                    "SELECT manual_bpm FROM library_tracks WHERE id = ?1",
                    [track_id],
                    |row| row.get(0),
                )
                .expect("the track should still be there");
            assert_eq!(manual, None, "a mix decision must not rewrite the analysis");

            // La courbe vise bien 128 lÃ  oÃ¹ le clip est posÃ©, donc le clip est
            // Ã©tirÃ© de 120 vers 128 au lieu de jouer Ã  un pour un.
            let (transport_map, _) = project_timing(&store.connection).expect("timing should read");
            let anchor = clip.tempo_anchor_beat as f64;
            assert!(
                (transport_map.bpm_at_beat(anchor) - 128.0).abs() < 1.0e-9,
                "la courbe doit viser 128 à l'ancre du clip"
            );

            // Le transport et le plan de rendu doivent viser le mÃªme tempo.
            let plan_map =
                crate::tempo::TempoMap::new(after.project_bpm, after.tempo_points.clone())
                    .expect("the snapshot tempo points should rebuild");
            assert_eq!(transport_map.signature(), plan_map.signature());

            // Et l'on peut rendre au clip la vitesse de son morceau.
            let restored = super::set_clip_tempo_target(&store.connection, clip_id, None)
                .expect("the target should be cleared");
            assert_eq!(restored.clips[0].tempo_target_bpm, None);
            let (restored_map, _) = project_timing(&store.connection).expect("timing should read");
            assert!(
                (restored_map.bpm_at_beat(restored.clips[0].tempo_anchor_beat as f64) - 120.0)
                    .abs()
                    < 1.0e-9,
                "sans cible, la courbe reprend la vitesse du morceau"
            );
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
    fn what_a_removed_clip_takes_with_it_stops_where_a_neighbour_begins() {
        // Rien autour : toute la plage part.
        assert_eq!(
            super::uncovered_intervals((8.0, 24.0), &[]),
            vec![(8.0, 24.0)]
        );

        // Un voisin qui mord la fin : seul le début lui appartenait.
        assert_eq!(
            super::uncovered_intervals((8.0, 24.0), &[(16.0, 40.0)]),
            vec![(8.0, 16.0)]
        );

        // Un voisin au milieu coupe la plage en deux morceaux.
        assert_eq!(
            super::uncovered_intervals((0.0, 32.0), &[(12.0, 20.0)]),
            vec![(0.0, 12.0), (20.0, 32.0)]
        );

        // Entièrement recouvert : on n'efface rien du tout, sinon retirer un
        // clip détruirait l'automation faite pour celui qui reste.
        assert!(super::uncovered_intervals((8.0, 24.0), &[(0.0, 40.0)]).is_empty());

        // Deux voisins qui se rejoignent exactement ne laissent pas un reste
        // large de rien entre eux.
        assert!(super::uncovered_intervals((8.0, 24.0), &[(0.0, 16.0), (16.0, 40.0)]).is_empty());

        // Un voisin qui touche le bord sans le franchir ne retire rien.
        assert_eq!(
            super::uncovered_intervals((8.0, 24.0), &[(24.0, 40.0)]),
            vec![(8.0, 24.0)]
        );
    }

    #[test]
    fn brush_sampling_keeps_a_quarter_beat_grid_and_a_bounded_node_count() {
        // Short curves keep the historical quarter-beat resolution.
        assert_eq!(filter_bubble_step_beats(2.0), 0.25);
        assert_eq!(filter_bubble_step_beats(128.0), 0.25);

        // Longer ones widen the step instead of multiplying the samples.
        for width in [129.0, 256.0, 1_000.0, FILTER_BUBBLE_MAX_WIDTH_BEATS] {
            let step = filter_bubble_step_beats(width);
            assert!(
                step >= FILTER_BUBBLE_STEP_BEATS,
                "step {step} is finer than the grid at width {width}"
            );
            assert!(
                (step / FILTER_BUBBLE_STEP_BEATS - (step / FILTER_BUBBLE_STEP_BEATS).round()).abs()
                    < 1.0e-9,
                "step {step} left the quarter-beat grid at width {width}"
            );
            let samples = (width / step).ceil() + 1.0;
            assert!(
                samples <= FILTER_BUBBLE_MAX_SAMPLES + 1.0,
                "width {width} would write {samples} samples"
            );
        }
    }

    #[test]
    fn a_very_long_brush_is_stored_without_flooding_the_project() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-longbrush-{}-{suffix}.sqlite3",
            std::process::id()
        ));

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");

            let long = draw_filter_bubble(
                &mut store.connection,
                0,
                0.0,
                FILTER_BUBBLE_MAX_WIDTH_BEATS,
                -0.9,
                None,
                None,
            )
            .expect("a full-length brush should be accepted");

            let nodes: Vec<_> = long
                .filter_nodes
                .iter()
                .filter(|node| node.lane == 0)
                .collect();
            assert!(
                nodes.len() <= FILTER_BUBBLE_MAX_SAMPLES as usize + 2,
                "a long brush wrote {} samples",
                nodes.len()
            );

            // It still reaches the requested depth at its far edge.
            assert!(
                nodes.iter().any(
                    |node| (node.beat - FILTER_BUBBLE_MAX_WIDTH_BEATS).abs() < 1.0e-9
                        && (node.value + 0.9).abs() < 1.0e-9
                ),
                "the ramp should reach its value on its last beat"
            );

            // A width beyond the maximum is clamped, not refused.
            let clamped = draw_filter_bubble(
                &mut store.connection,
                1,
                0.0,
                FILTER_BUBBLE_MAX_WIDTH_BEATS * 4.0,
                0.5,
                None,
                None,
            )
            .expect("an over-long request should clamp");
            let last = clamped
                .filter_nodes
                .iter()
                .filter(|node| node.lane == 1)
                .map(|node| node.beat)
                .fold(0.0_f64, f64::max);
            assert!(last <= FILTER_BUBBLE_MAX_WIDTH_BEATS + 0.25);
        }

        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            if candidate.exists() {
                fs::remove_file(candidate).expect("test database should be removed");
            }
        }
    }

    /// Resizing a curve rewrites it over its former span. Shortening one is the
    /// case that matters: without naming the range it replaces, the old tail
    /// would survive past the new end.
    #[test]
    fn redrawing_over_a_replaced_range_leaves_no_tail_behind() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-resize-{}-{suffix}.sqlite3",
            std::process::id()
        ));

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");

            draw_filter_bubble(&mut store.connection, 0, 16.0, 32.0, -0.7, None, None)
                .expect("the original curve should be drawn");
            let far_end = 48.0;

            // Shrink it to a quarter of its length, naming what it replaces.
            let shortened = draw_filter_bubble(
                &mut store.connection,
                0,
                16.0,
                8.0,
                -0.7,
                None,
                Some((16.0, far_end)),
            )
            .expect("the curve should shrink");

            assert!(
                !shortened
                    .filter_nodes
                    .iter()
                    .any(|node| node.lane == 0 && node.beat > 24.5),
                "the former tail must not survive the resize"
            );
            assert!(
                shortened
                    .filter_nodes
                    .iter()
                    .any(|node| node.lane == 0 && (node.beat - 24.0).abs() < 1.0e-9),
                "the curve should still reach its new end"
            );

            // Growing it back the other way works from the left edge too.
            let grown = draw_filter_bubble(
                &mut store.connection,
                0,
                4.0,
                20.0,
                -0.7,
                None,
                Some((16.0, 24.0)),
            )
            .expect("the curve should grow");
            let first = grown
                .filter_nodes
                .iter()
                .filter(|node| node.lane == 0)
                .map(|node| node.beat)
                .fold(f64::MAX, f64::min);
            assert!(
                (first - 4.0).abs() < 1.0e-9,
                "the curve now starts at {first}"
            );

            assert!(
                draw_filter_bubble(
                    &mut store.connection,
                    0,
                    4.0,
                    8.0,
                    0.5,
                    None,
                    Some((20.0, 4.0)),
                )
                .is_err(),
                "a reversed replaced range must be refused"
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
    fn clearing_a_filter_range_erases_a_whole_brush_and_spares_its_neighbours() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-filter-{}-{suffix}.sqlite3",
            std::process::id()
        ));

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");

            // Two brushes on lane 1, plus one on lane 0 that must not move.
            draw_filter_bubble(&mut store.connection, 1, 8.0, 4.0, -0.8, None, None)
                .expect("first brush should be drawn");
            draw_filter_bubble(&mut store.connection, 1, 40.0, 4.0, 0.6, None, None)
                .expect("second brush should be drawn");
            draw_filter_bubble(&mut store.connection, 0, 8.0, 4.0, 0.5, None, None)
                .expect("a brush on another lane should be drawn");

            let lane_one = |timeline: &TimelineSnapshot| {
                timeline
                    .filter_nodes
                    .iter()
                    .filter(|node| node.lane == 1)
                    .count()
            };
            let before = snapshot(&store.connection).expect("timeline should read");
            assert!(lane_one(&before) > 30, "a brush writes a dense run");

            // The range covers the bubble and the bypass sample closing it.
            let cleared = clear_filter_range(&store.connection, 1, 8.0, 12.0 + 0.01)
                .expect("the first brush should be erased");

            assert!(
                !cleared
                    .filter_nodes
                    .iter()
                    .any(|node| node.lane == 1 && (8.0..=12.01).contains(&node.beat)),
                "no sample of the erased brush may remain"
            );
            assert!(
                cleared
                    .filter_nodes
                    .iter()
                    .any(|node| node.lane == 1 && node.beat >= 40.0),
                "the other brush on the same lane must survive"
            );
            assert!(
                cleared.filter_nodes.iter().any(|node| node.lane == 0),
                "the other lane must be untouched"
            );

            assert!(clear_filter_range(&store.connection, 3, 0.0, 4.0).is_err());
            assert!(clear_filter_range(&store.connection, 1, 8.0, 4.0).is_err());
            assert!(clear_filter_range(&store.connection, 1, f64::NAN, 4.0).is_err());
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
    fn only_one_clip_can_hold_the_sidechain_key() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-sidechain-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'key.mp3', 60000, 44100, 2, 120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            let first = add_clip(&mut store.connection, track_id, Some(4.0), Some(0))
                .expect("first clip should be added")
                .clips[0]
                .id;
            let second = add_clip(&mut store.connection, track_id, Some(8.0), Some(1))
                .expect("second clip should be added")
                .clips
                .iter()
                .map(|clip| clip.id)
                .find(|id| *id != first)
                .expect("a second clip should exist");

            let keyed = set_sidechain_key(&mut store.connection, first, true)
                .expect("the key should be set");
            assert!(
                keyed
                    .clips
                    .iter()
                    .any(|c| c.id == first && c.is_sidechain_key)
            );

            // Naming another key releases the first, in the same write.
            let moved = set_sidechain_key(&mut store.connection, second, true)
                .expect("the key should move");
            assert_eq!(
                moved
                    .clips
                    .iter()
                    .filter(|clip| clip.is_sidechain_key)
                    .map(|clip| clip.id)
                    .collect::<Vec<_>>(),
                vec![second]
            );

            let cleared = set_sidechain_key(&mut store.connection, second, false)
                .expect("the key should clear");
            assert!(!cleared.clips.iter().any(|clip| clip.is_sidechain_key));

            assert!(set_sidechain_key(&mut store.connection, 9_999, true).is_err());
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

    /// Le panoramique dÃ©crit un geste sur un son prÃ©cis. LaissÃ© en arriÃ¨re quand
    /// le clip s'en va, il dÃ©crit ce geste sur du silence â€” et le clip arrive
    /// sur celui du voisin.
    #[test]
    fn both_automation_lines_travel_with_the_clip_that_holds_them() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-automation-follow-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'pan.mp3', 60000, 44100, 2, 120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            let placed = add_clip(&mut store.connection, track_id, Some(4.0), Some(0))
                .expect("clip should be added");
            let clip = placed.clips[0].id;
            let start = placed.clips[0].visual_start_beat;

            // Le dÃ©pÃ´t a posÃ© ses deux ancres, au centre, aux bouts du clip.
            let seeded: Vec<_> = placed
                .pan_nodes
                .iter()
                .filter(|node| node.lane == 0)
                .collect();
            assert_eq!(seeded.len(), 2);
            assert!(seeded.iter().all(|node| node.value.abs() < f64::EPSILON));
            assert!(
                seeded
                    .iter()
                    .any(|node| (node.beat - start).abs() < f64::EPSILON)
            );

            // Deux nÅ“uds de plus dans le clip, un dernier bien Ã  l'Ã©cart :
            // celui-lÃ  ne doit pas bouger, sans quoi c'est toute la voie qui
            // suivrait.
            add_pan_node(&store.connection, 0, start + 2.0).expect("pan node should be added");
            add_pan_node(&store.connection, 0, start + 6.0).expect("pan node should be added");
            let before = add_pan_node(&store.connection, 0, 400.0)
                .expect("a pan node outside the clip should be added");
            assert_eq!(before.pan_nodes.len(), 5);

            let moved = move_clip(&mut store.connection, clip, 20.0, 1)
                .expect("clip should move to another lane");
            let delta = moved
                .clips
                .iter()
                .find(|c| c.id == clip)
                .expect("moved clip should remain in the snapshot")
                .visual_start_beat
                - start;
            assert!(delta.abs() > 1.0, "the move should have shifted the clip");

            for offset in [2.0, 6.0] {
                assert!(
                    moved.pan_nodes.iter().any(|node| node.lane == 1
                        && (node.beat - (start + offset + delta)).abs() < f64::EPSILON),
                    "the pan node at +{offset} should have travelled with the clip"
                );
            }
            assert!(
                moved
                    .pan_nodes
                    .iter()
                    .any(|node| node.lane == 0 && (node.beat - 400.0).abs() < f64::EPSILON),
                "a pan node outside the clip should stay where it was"
            );
            assert_eq!(moved.pan_nodes.len(), 5);
            // Le volume voyage par le mÃªme chemin : les deux graines posÃ©es Ã
            // l'ajout doivent se retrouver sur la nouvelle voie.
            assert_eq!(
                moved
                    .volume_nodes
                    .iter()
                    .filter(|node| node.lane == 1)
                    .count(),
                2
            );
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

    /// Un sinus dessinÃ© pose une douzaine de nÅ“uds par cycle, donc plusieurs
    /// par quart de temps. Les recaler sur le quart en dÃ©plaÃ§ant le clip les
    /// Ã©crasait les uns sur les autres, et la contrainte d'unicitÃ© renvoyait
    /// Â« UNIQUE constraint failed Â» sur un geste parfaitement ordinaire.
    #[test]
    fn a_drawn_shape_travels_with_its_clip_without_collapsing_onto_the_quarter_beat() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-drawn-follow-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'drawn.mp3', 60000, 44100, 2, 120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            let placed = add_clip(&mut store.connection, track_id, Some(4.0), Some(0))
                .expect("clip should be added");
            let clip = placed.clips[0].id;
            let start = placed.clips[0].visual_start_beat;

            // Douze nÅ“uds sur deux temps : six par temps, donc plusieurs dans
            // le mÃªme quart.
            let drawn: Vec<(f64, f64)> = (0..12)
                .map(|step| {
                    let beat = start + 1.0 + f64::from(step) / 6.0;
                    (beat, (f64::from(step) / 6.0).sin())
                })
                .collect();
            let painted = super::draw_pan_shape(
                &mut store.connection,
                0,
                drawn[0].0,
                drawn[drawn.len() - 1].0,
                &drawn,
                "sine",
                1.0,
            )
            .expect("the stroke should be written");
            // Les douze du trait, plus les deux ancres du dÃ©pÃ´t, que le trait
            // ne recouvre pas.
            assert_eq!(painted.pan_nodes.len(), drawn.len() + 2);
            assert_eq!(painted.draw_groups.len(), 1);
            assert_eq!(painted.draw_groups[0].shape, "sine");
            assert_eq!(painted.draw_groups[0].period, 1.0);

            let moved = move_clip(&mut store.connection, clip, 24.0, 1)
                .expect("a clip holding a drawn shape should still move");
            let delta = moved
                .clips
                .iter()
                .find(|c| c.id == clip)
                .expect("moved clip should remain in the snapshot")
                .visual_start_beat
                - start;

            assert_eq!(moved.pan_nodes.len(), drawn.len() + 2);
            assert_eq!(moved.draw_groups.len(), 1);
            assert_eq!(moved.draw_groups[0].lane, 1);
            for (beat, _) in &drawn {
                assert!(
                    moved
                        .pan_nodes
                        .iter()
                        .any(|node| node.lane == 1 && (node.beat - (beat + delta)).abs() < 1e-9),
                    "the node drawn at {beat} should have kept its exact place in the shape"
                );
            }

            let after_delete =
                super::delete_draw_group(&mut store.connection, moved.draw_groups[0].id)
                    .expect("the whole Draw should be removable in one action");
            assert!(after_delete.draw_groups.is_empty());
            assert_eq!(
                after_delete.pan_nodes.len(),
                2,
                "manual clip anchors stay behind"
            );
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

    /// Le trait libre Ã©crit ce qu'on lui donne, et rien d'autre : il remplace
    /// sa plage d'un seul coup et laisse en place ce qui est au-delÃ .
    #[test]
    fn a_freehand_filter_stroke_replaces_its_range_and_nothing_else() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-filter-stroke-{}-{suffix}.sqlite3",
            std::process::id()
        ));

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            super::add_filter_node(&store.connection, 0, 64.0, 0.9)
                .expect("a node far from the stroke should be added");

            let stroke: Vec<(f64, f64)> = vec![
                (7.99, 0.0),
                (8.0, 0.4),
                (8.25, 0.6),
                (8.5, -0.5),
                (8.51, 0.0),
            ];
            let drawn = super::draw_filter_stroke(&mut store.connection, 0, &stroke)
                .expect("the stroke should be written");

            for (beat, value) in &stroke {
                assert!(
                    drawn.filter_nodes.iter().any(|node| node.lane == 0
                        && (node.beat - beat).abs() < 1e-9
                        && (node.value - value).abs() < 1e-9),
                    "the sample painted at {beat} should have been written"
                );
            }
            assert!(
                drawn
                    .filter_nodes
                    .iter()
                    .any(|node| (node.beat - 64.0).abs() < 1e-9),
                "a curve outside the stroke should be left alone"
            );

            // Redessiner par-dessus ne doit pas laisser de queue de l'ancien.
            let again: Vec<(f64, f64)> = vec![(7.99, 0.0), (8.0, -0.2), (8.51, 0.0)];
            let redrawn = super::draw_filter_stroke(&mut store.connection, 0, &again)
                .expect("the stroke should be redrawn");
            assert!(
                !redrawn
                    .filter_nodes
                    .iter()
                    .any(|node| (node.beat - 8.25).abs() < 1e-9),
                "the sample the second stroke covered should be gone"
            );

            // Ce que le serveur refuse : un trait qui recule, une valeur hors
            // du champ, un trait vide.
            assert!(super::draw_filter_stroke(&mut store.connection, 0, &[]).is_err());
            assert!(
                super::draw_filter_stroke(&mut store.connection, 0, &[(8.0, 0.0), (7.0, 0.0)])
                    .is_err()
            );
            assert!(super::draw_filter_stroke(&mut store.connection, 0, &[(8.0, 4.0)]).is_err());
        }

        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            if candidate.exists() {
                fs::remove_file(candidate).expect("test database should be removed");
            }
        }
    }

    /// DÃ©cuire doit rendre exactement ce que cuire a emportÃ©.
    ///
    /// C'est la promesse du bouton : sans elle, `BAKE` est un aller simple, et
    /// un bouton dont on ne revient pas ne se clique plus. Le test vÃ©rifie les
    /// trois automations Ã  la fois â€” le volume, le panoramique et le filtre â€”
    /// parce que la troisiÃ¨me porte une colonne de plus et qu'une boucle
    /// Ã©crite pour deux l'oublie sans rien dire.
    #[test]
    fn a_bake_gives_back_exactly_what_it_took() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-bake-roundtrip-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'bake.mp3', 120000, 44100, 2, 120.0, 500, 240, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();
            let placed = add_clip(&mut store.connection, track_id, Some(8.0), Some(1))
                .expect("clip should be added");
            let clip = placed.clips[0].clone();

            // De l'automation dans le clip, et de l'automation en dehors : la
            // seconde ne doit pas bouger d'un cheveu.
            let place = |connection: &rusqlite::Connection, beat: f64, gain: f64| {
                connection
                    .execute(
                        "INSERT INTO timeline_volume_nodes (lane, beat, gain_db)
                         VALUES (1, ?1, ?2)",
                        params![beat, gain],
                    )
                    .expect("volume node should be added");
            };
            for beat in [clip.visual_start_beat + 2.0, clip.visual_start_beat + 4.0] {
                place(&store.connection, beat, -12.0);
                store
                    .connection
                    .execute(
                        "INSERT INTO timeline_pan_nodes (lane, beat, value) VALUES (1, ?1, 0.5)",
                        params![beat],
                    )
                    .expect("pan node should be added");
                store
                    .connection
                    .execute(
                        "INSERT INTO timeline_filter_nodes (lane, beat, value, tension)
                         VALUES (1, ?1, 0.7, 0.25)",
                        params![beat],
                    )
                    .expect("filter node should be added");
            }
            let outside = clip.visual_end_beat + 16.0;
            place(&store.connection, outside, -3.0);

            let spec =
                super::prepare_bake(&store.connection, clip.id).expect("the bake should prepare");
            assert_eq!(spec.plan.clips.len(), 1, "un seul clip est cuit");
            assert!(
                !spec.plan.limiter_enabled && !spec.plan.compressor_enabled,
                "le bus gÃ©nÃ©ral n'entre pas dans un clip"
            );
            assert!(
                !spec.plan.clips[0].is_sidechain_key,
                "le sidechain reste vivant"
            );
            // Le rapport d'Ã©tirement vaut un : le tempo de rendu est celui de
            // la source, sinon le fichier cuit serait Ã©tirÃ© une seconde fois.
            assert!((spec.plan.project_bpm - 120.0).abs() < 1e-9);
            let before = super::snapshot(&store.connection).expect("snapshot");
            // ComptÃ© sur l'Ã©tat rÃ©el plutÃ´t qu'Ã©crit en dur : poser un clip
            // sÃ¨me dÃ©jÃ  des nÅ“uds Ã  ses bornes, et un nombre fixe ne dirait
            // que la date Ã  laquelle le test a Ã©tÃ© Ã©crit.
            let in_range =
                |beat: f64| beat >= spec.removed.from_beat && beat <= spec.removed.to_beat;
            let expected_volume = before
                .volume_nodes
                .iter()
                .filter(|node| node.lane == 1 && in_range(node.beat))
                .count();
            assert_eq!(spec.removed.volume.len(), expected_volume);
            assert!(
                spec.removed.volume.len() >= 2,
                "le clip porte bien l'automation qu'on vient d'y poser"
            );
            assert_eq!(spec.removed.filter.len(), 2);
            let baked_file = database_path.with_extension("baked.wav");
            fs::write(&baked_file, []).expect("fake bake should be created");
            let after = super::commit_bake(
                &mut store.connection,
                clip.id,
                &baked_file.to_string_lossy(),
                1_500.0,
                &spec.removed,
                None,
            )
            .expect("the bake should commit");

            assert!(after.clips[0].is_baked, "le clip se dit cuit");
            // La voie est plate sur l'Ã©tendue cuite : il ne reste que les deux
            // ancres de repos, sans quoi ce qui suit le clip hÃ©riterait d'une
            // rampe que personne n'a demandÃ©e.
            let inside: Vec<_> = after
                .volume_nodes
                .iter()
                .filter(|node| {
                    node.lane == 1
                        && node.beat >= spec.removed.from_beat
                        && node.beat <= spec.removed.to_beat
                })
                .collect();
            assert_eq!(inside.len(), 2, "deux ancres, et rien entre elles");
            assert!(
                inside
                    .iter()
                    .all(|node| node.gain_db == Some(DEFAULT_TRACK_GAIN_DB))
            );
            assert!(
                after
                    .volume_nodes
                    .iter()
                    .any(|node| (node.beat - outside).abs() < 1e-9 && node.gain_db == Some(-3.0)),
                "le nÅ“ud hors du clip est restÃ©"
            );

            let (restored, removed_file) =
                super::unbake_clip(&mut store.connection, clip.id).expect("the bake should undo");
            assert_eq!(
                removed_file.as_deref(),
                Some(&*baked_file.to_string_lossy())
            );
            assert!(!restored.clips[0].is_baked, "le clip n'est plus cuit");

            let key = |snapshot: &TimelineSnapshot| {
                let mut volume: Vec<_> = snapshot
                    .volume_nodes
                    .iter()
                    .map(|node| ((node.beat * 1e6) as i64, node.lane, node.gain_db))
                    .collect();
                let mut pan: Vec<_> = snapshot
                    .pan_nodes
                    .iter()
                    .map(|node| ((node.beat * 1e6) as i64, node.lane, node.value))
                    .collect();
                let mut filter: Vec<_> = snapshot
                    .filter_nodes
                    .iter()
                    .map(|node| {
                        (
                            (node.beat * 1e6) as i64,
                            node.lane,
                            node.value,
                            node.tension,
                        )
                    })
                    .collect();
                volume.sort_by_key(|entry| (entry.1, entry.0));
                pan.sort_by_key(|entry| (entry.1, entry.0));
                filter.sort_by_key(|entry| (entry.1, entry.0));
                (volume, pan, filter)
            };
            assert_eq!(
                key(&restored),
                key(&before),
                "l'automation revient exactement comme elle Ã©tait"
            );
        }

        let _ = fs::remove_file(&fake_mp3);
        let _ = fs::remove_file(database_path.with_extension("baked.wav"));
        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            let _ = fs::remove_file(candidate);
        }
    }

    /// Scinder un clip sÃ©parÃ© doit donner deux clips sÃ©parÃ©s.
    ///
    /// Les deux moitiÃ©s viennent de la mÃªme source, et le fichier de stem couvre
    /// dÃ©jÃ  leur Ã©tendue commune : elles le partagent. Sans cela la moitiÃ©
    /// droite retombait sur le morceau complet tout en gardant sa touche
    /// allumÃ©e â€” l'affichage et le son se contredisaient.
    #[test]
    fn splitting_a_clip_carries_its_stems_to_both_halves() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-split-stems-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'split.mp3', 60000, 44100, 2, 120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            let placed = add_clip(&mut store.connection, track_id, Some(4.0), Some(0))
                .expect("clip should be added");
            let clip = placed.clips[0].id;

            // Un stem factice, comme la sÃ©paration en poserait un.
            let stem_file = database_path.with_extension("stem.wav");
            fs::write(&stem_file, []).expect("fake stem should be created");
            store
                .connection
                .execute(
                    "INSERT INTO clip_stems (clip_id, kind, file_path, source_from_ms)
                     VALUES (?1, 'vocals', ?2, 1500)",
                    params![clip, stem_file.to_string_lossy()],
                )
                .expect("stem should be recorded");
            super::set_clip_stem(&store.connection, clip, "vocals")
                .expect("the clip should play its vocals");

            let split_beat = placed.clips[0].visual_start_beat + 8.0;
            let after = super::split_timeline_clip(&mut store.connection, clip, split_beat)
                .expect("the clip should split");
            assert_eq!(after.clips.len(), 2);

            for half in &after.clips {
                assert_eq!(half.stem, "vocals", "les deux moitiÃ©s jouent la voix");
                assert!(half.has_stems, "les deux moitiÃ©s ont leur stem");
            }
            // Et le dÃ©calage de source voyage avec, sans quoi la moitiÃ© droite
            // jouerait Ã  cÃ´tÃ© de sa grille.
            let offsets: Vec<i64> = store
                .connection
                .prepare("SELECT source_from_ms FROM clip_stems ORDER BY clip_id")
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get::<_, i64>(0))?
                        .collect::<Result<Vec<_>, _>>()
                })
                .expect("les dÃ©calages devraient se lire");
            assert_eq!(offsets, vec![1500, 1500]);

            let _ = fs::remove_file(&stem_file);
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

    /// Un Undo ne doit jamais coÃ»ter un stem, quel que soit le geste annulÃ©.
    ///
    /// Le premier cas trouvÃ© passait par un nÅ“ud de panoramique, mais le geste
    /// n'y Ã©tait pour rien : `restore_snapshot` remplace **tous** les clips, et
    /// tout ce qui pend Ã  eux tombe avec. Le test pose donc les trois lignes
    /// d'automation, annule chacune, et exige que le stem et sa forme d'onde
    /// soient toujours lÃ  â€” plutÃ´t que de vÃ©rifier celle par laquelle le dÃ©faut
    /// s'est manifestÃ©.
    #[test]
    fn undoing_any_automation_leaves_the_stems_untouched() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-undo-stems-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");
        let stem_file = database_path.with_extension("stem.wav");
        fs::write(&stem_file, []).expect("fake stem should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'undo.mp3', 60000, 44100, 2, 120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();
            let placed = add_clip(&mut store.connection, track_id, Some(4.0), Some(0))
                .expect("clip should be added");
            let clip = placed.clips[0].id;

            // Un stem complet : le fichier, son dÃ©calage, et sa forme d'onde.
            store
                .connection
                .execute(
                    "INSERT INTO clip_stems
                     (clip_id, kind, file_path, source_from_ms, bucket_count, left_min)
                     VALUES (?1, 'vocals', ?2, 1500, 4, ?3)",
                    params![clip, stem_file.to_string_lossy(), vec![1_u8, 2, 3, 4]],
                )
                .expect("stem should be recorded");
            super::set_clip_stem(&store.connection, clip, "vocals")
                .expect("the clip should play it");

            let before = snapshot(&store.connection).expect("snapshot should read");

            for label in ["panoramique", "volume", "filtre"] {
                match label {
                    "panoramique" => add_pan_node(&store.connection, 0, 8.0),
                    "volume" => add_volume_node(&store.connection, 0, 8.0),
                    _ => add_filter_node(&store.connection, 0, 8.0, 0.5),
                }
                .expect("the automation should be added");
                restore_snapshot(&mut store.connection, &before).expect("undo should succeed");

                let restored = snapshot(&store.connection).expect("snapshot should read");
                let clip_after = restored
                    .clips
                    .iter()
                    .find(|candidate| candidate.id == clip)
                    .unwrap_or_else(|| {
                        panic!("le clip devrait survivre Ã  l'annulation d'un {label}")
                    });
                assert_eq!(clip_after.stem, "vocals", "aprÃ¨s un {label}");
                assert!(
                    clip_after.has_stems,
                    "le stem devrait rester aprÃ¨s un {label}"
                );

                let (path, offset, buckets): (String, i64, Option<i64>) = store
                    .connection
                    .query_row(
                        "SELECT file_path, source_from_ms, bucket_count
                         FROM clip_stems WHERE clip_id = ?1 AND kind = 'vocals'",
                        [clip],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .unwrap_or_else(|_| panic!("le stem devrait Ãªtre lisible aprÃ¨s un {label}"));
                assert_eq!(path, stem_file.to_string_lossy(), "aprÃ¨s un {label}");
                assert_eq!(offset, 1500, "le dÃ©calage devrait survivre Ã  un {label}");
                assert_eq!(
                    buckets,
                    Some(4),
                    "la forme d'onde du stem devrait survivre Ã  un {label}"
                );
            }
        }

        let _ = fs::remove_file(&stem_file);
        fs::remove_file(&fake_mp3).expect("fake MP3 should be removed");
        for suffix in ["", "-wal", "-shm"] {
            let candidate =
                std::path::PathBuf::from(format!("{}{}", database_path.to_string_lossy(), suffix));
            if candidate.exists() {
                fs::remove_file(candidate).expect("test database should be removed");
            }
        }
    }

    /// La fenÃªtre d'un clip commence au dÃ©but du fichier, prÃ©-roll compris.
    ///
    /// Le cas qui a mordu : un morceau dont le premier temps dÃ©tectÃ© tombe Ã
    /// 2 min 46. Le clip joue ces deux minutes quarante-six, et un stem qui ne
    /// les contient pas est muet lÃ  oÃ¹ l'on attend du son.
    #[test]
    fn a_clip_window_starts_at_the_file_not_at_the_first_beat() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-window-origin-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'late.mp3', 298899, 44100, 2, 150.17, 165689, 700, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();
            let placed = add_clip(&mut store.connection, track_id, Some(416.0), Some(0))
                .expect("clip should be added");
            let clip = &placed.clips[0];

            let (from, to) =
                super::clip_source_window_ms(clip).expect("the window should be known");
            assert!(
                from.abs() < 1.0,
                "un clip non rognÃ© commence au dÃ©but du fichier, pas Ã  {from} ms"
            );
            assert!(
                to > 298_000.0,
                "la fenÃªtre doit couvrir tout le morceau : {to} ms"
            );
            // Et le premier temps tombe bien au milieu de cette fenÃªtre, ce qui
            // est tout l'intÃ©rÃªt du cas.
            assert!(from < 165_689.0 && 165_689.0 < to);
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
    fn overlapping_clips_are_detected_but_touching_ones_are_not() {
        assert!(!clips_overlap(0.0, 16.0, 16.0, 64.0));
        assert!(clips_overlap(0.0, 16.0, 15.0, 32.0));
        assert!(clips_overlap(10.0, 20.0, 5.0, 25.0));
        assert!(!clips_overlap(0.0, 16.0, 20.0, 32.0));
    }

    #[test]
    fn splitting_a_clip_keeps_both_halves_in_place_and_adjacent() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-split-{}-{suffix}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        fs::write(&fake_mp3, []).expect("fake MP3 should be created");

        {
            let mut store = LibraryStore::open(&database_path).expect("database should open");
            store
                .connection
                .execute(
                    "INSERT INTO library_tracks
                     (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                      bpm, first_beat_ms, beat_count, analysis_status)
                     VALUES (?1, ?2, 'split.mp3', 60000, 44100, 2,
                             120.0, 500, 120, 'analyzed')",
                    params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
                )
                .expect("track should be inserted");
            let track_id = store.connection.last_insert_rowid();

            // 60 s at 120 BPM is 120 beats, with a one beat pre-roll before the
            // downbeat anchored on bar 2: the clip covers beats 3 to 123.
            let added = add_clip(&mut store.connection, track_id, Some(4.0), Some(0))
                .expect("clip should be added");
            let clip_id = added.clips[0].id;
            assert!((added.clips[0].visual_start_beat - 3.0).abs() < 1.0e-9);
            assert!((added.clips[0].visual_end_beat - 123.0).abs() < 1.0e-9);

            let split = split_timeline_clip(&mut store.connection, clip_id, 43.0)
                .expect("a playhead inside the clip should split it");
            assert_eq!(split.clips.len(), 2);

            let left = split
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .expect("the original clip becomes the left subclip");
            let right = split
                .clips
                .iter()
                .find(|clip| clip.id != clip_id)
                .expect("the split creates a right subclip");

            // Neither half moves: they still start and end where the source did.
            assert!((left.visual_start_beat - 3.0).abs() < 1.0e-9);
            assert!((left.visual_end_beat - 43.0).abs() < 1.0e-9);
            assert!((left.duration_beats - 40.0).abs() < 1.0e-9);
            assert!((left.trim_start_beats - 0.0).abs() < 1.0e-9);
            assert!((left.trim_end_beats - 80.0).abs() < 1.0e-9);

            assert!((right.visual_start_beat - 43.0).abs() < 1.0e-9);
            assert!((right.visual_end_beat - 123.0).abs() < 1.0e-9);
            assert!((right.duration_beats - 80.0).abs() < 1.0e-9);
            assert!((right.trim_start_beats - 40.0).abs() < 1.0e-9);
            assert!((right.trim_end_beats - 0.0).abs() < 1.0e-9);

            assert_eq!(right.lane, left.lane);
            assert!(!clips_overlap(
                left.visual_start_beat,
                left.visual_end_beat,
                right.visual_start_beat,
                right.visual_end_beat,
            ));

            // A subclip can be split again, and the halves stay anchored.
            let again = split_timeline_clip(&mut store.connection, right.id, 83.0)
                .expect("a subclip should split again");
            assert_eq!(again.clips.len(), 3);
            let middle = again
                .clips
                .iter()
                .find(|clip| clip.id == right.id)
                .expect("the right subclip becomes the middle one");
            assert!((middle.visual_start_beat - 43.0).abs() < 1.0e-9);
            assert!((middle.visual_end_beat - 83.0).abs() < 1.0e-9);
            let last = again
                .clips
                .iter()
                .find(|clip| clip.id != clip_id && clip.id != right.id)
                .expect("the second split creates a new subclip");
            assert!((last.visual_start_beat - 83.0).abs() < 1.0e-9);
            assert!((last.visual_end_beat - 123.0).abs() < 1.0e-9);
            assert!((last.trim_start_beats - 80.0).abs() < 1.0e-9);

            // A playhead outside the clip is refused and leaves the timeline alone.
            assert!(split_timeline_clip(&mut store.connection, clip_id, 200.0).is_err());
            assert!(split_timeline_clip(&mut store.connection, clip_id, 3.0).is_err());
            assert_eq!(
                snapshot(&store.connection)
                    .expect("timeline should still read")
                    .clips
                    .len(),
                3
            );
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
}
