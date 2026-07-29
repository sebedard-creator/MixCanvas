//! Fichiers de projet portables.
//!
//! La base SQLite reste l'état de travail vivant : elle enregistre chaque
//! édition au fil de l'eau, et c'est elle que l'application rouvre au
//! démarrage. Un fichier de projet en est un instantané transportable, que
//! l'on peut déplacer d'une machine à l'autre.
//!
//! Le fichier porte tout ce qui décrit une session : les morceaux référencés
//! avec leurs corrections manuelles, leurs formes d'onde, et l'intégralité du
//! timeline. Un clip y désigne son morceau par son **rang dans le fichier** et
//! non par son identifiant de base : les identifiants n'ont de sens que dans la
//! base qui les a émis, et un projet doit s'ouvrir sur une machine qui n'a
//! jamais vu ces morceaux.

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::analysis::WaveformPeaks;
use crate::library::{
    database_read_error, database_write_error, normalize_path_key, save_waveform_in_transaction,
};

/// Marqueur de format. Un fichier qui ne le porte pas n'est pas un projet.
const PROJECT_FORMAT: &str = "mixcanvas-project";
/// Version du format. À incrémenter dès qu'un fichier écrit aujourd'hui ne
/// pourrait plus être relu tel quel, jamais pour un simple ajout de champ.
const PROJECT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    format: String,
    version: u32,
    project_bpm: f64,
    limiter_enabled: bool,
    compressor_enabled: bool,
    tracks: Vec<ProjectTrack>,
    lanes: Vec<ProjectLane>,
    clips: Vec<ProjectClip>,
    volume_nodes: Vec<ProjectVolumeNode>,
    #[serde(default)]
    pan_nodes: Vec<ProjectPanNode>,
    filter_nodes: Vec<ProjectFilterNode>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectTrack {
    file_path: String,
    file_name: String,
    artist: Option<String>,
    title: Option<String>,
    duration_ms: i64,
    sample_rate: Option<i64>,
    channels: Option<i64>,
    bpm: Option<f64>,
    bpm_confidence: Option<f64>,
    first_beat_ms: Option<i64>,
    beat_count: Option<i64>,
    analysis_status: String,
    analysis_version: Option<i64>,
    /// Les corrections faites à la main dans le Beatgrid Editor. C'est la seule
    /// partie d'un morceau qu'aucune ré-analyse ne peut retrouver.
    manual_bpm: Option<f64>,
    manual_first_beat_ms: Option<i64>,
    waveform: Option<ProjectWaveform>,
}

/// Les six rampes d'une forme d'onde, en octets f32 petit-boutistes encodés en
/// base64.
///
/// Écrire les flottants en JSON les rendrait plus volumineux que le binaire
/// qu'ils représentent : seize mille valeurs par rampe, six rampes, et chaque
/// nombre occuperait une dizaine de caractères.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectWaveform {
    bucket_count: usize,
    left_min: String,
    left_max: String,
    left_rms: String,
    right_min: String,
    right_max: String,
    right_rms: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectLane {
    lane: i64,
    is_muted: bool,
    is_solo: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectClip {
    /// Rang du morceau dans `tracks`, et non son identifiant de base.
    track_index: usize,
    lane: i64,
    anchor_beat: i64,
    tempo_anchor_beat: i64,
    trim_start_beats: f64,
    trim_end_beats: f64,
    is_sidechain_key: bool,
    eq_settings: Option<String>,
    /// `full`, `vocals` ou `instrumental` : laquelle des voix le clip joue.
    ///
    /// `default` pour que les projets écrits avant ce champ se relisent : ils
    /// jouaient forcément le morceau entier, ce que `Default` donne.
    #[serde(default = "full_stem")]
    stem: String,
    /// Les voix déjà séparées de ce clip, et le fichier de chacune.
    #[serde(default)]
    stems: Vec<ProjectClipStem>,
    /// La cuisson de ce clip, s'il en a une.
    #[serde(default)]
    bake: Option<ProjectClipBake>,
}

fn full_stem() -> String {
    "full".to_owned()
}

/// Une voix séparée, telle que le projet la retient.
///
/// Le **chemin** voyage, pas le son. Un WAV de séparation pèse trente-cinq
/// mégaoctets; deux par clip, sur vingt clips, feraient un projet d'un
/// gigaoctet et demi qu'on n'enverrait à personne. Un fichier absent au
/// rechargement n'est pas une erreur : le clip retombe sur sa source, ce qui
/// s'entend et se répare d'un clic.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectClipStem {
    kind: String,
    file_path: String,
    source_from_ms: i64,
}

/// Une cuisson, telle que le projet la retient.
///
/// **`removed` est la seule copie de l'automation que la cuisson a emportée.**
/// Sans elle dans le projet, enregistrer puis rouvrir perdait à la fois le
/// fichier cuit et l'automation qu'il contenait : le clip revenait sec, sur une
/// voie plate, et il n'y avait plus rien à restaurer. Elle voyage donc même si
/// le fichier, lui, ne se retrouve pas.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectClipBake {
    file_path: String,
    source_from_ms: i64,
    removed: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectVolumeNode {
    lane: i64,
    beat: f64,
    gain_db: Option<f64>,
}

/// `serde(default)` sur le champ : un projet écrit avant le panoramique se
/// relit sans lui, plutôt que d'être refusé pour un champ manquant.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPanNode {
    lane: i64,
    beat: f64,
    value: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectFilterNode {
    lane: i64,
    beat: f64,
    value: f64,
    tension: f64,
}

fn decode_ramp(encoded: &str, bucket_count: usize) -> Result<Vec<f32>, String> {
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| "This project's waveform data is damaged.".to_owned())?;
    if bytes.len() != bucket_count * 4 {
        return Err("This project's waveform data has an unexpected length.".to_owned());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

/// Rassemble l'état courant en un fichier de projet.
pub fn collect(connection: &Connection) -> Result<ProjectFile, String> {
    let (project_bpm, limiter_enabled, compressor_enabled) = connection
        .query_row(
            "SELECT project_bpm, limiter_enabled, compressor_enabled
             FROM project_settings WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, f64>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, i64>(2)? != 0,
                ))
            },
        )
        .map_err(database_read_error)?;

    // La bibliothèque entière part dans le fichier, pas seulement les morceaux
    // posés sur le timeline. Un projet est un instantané complet de la session :
    // n'emporter que les clips ferait disparaître au rechargement les morceaux
    // importés mais pas encore utilisés, et leurs corrections de beatgrid avec.
    let mut statement = connection
        .prepare(
            "SELECT id, file_path, file_name, artist, title, duration_ms, sample_rate,
                    channels, bpm, bpm_confidence, first_beat_ms, beat_count,
                    analysis_status, analysis_version, manual_bpm, manual_first_beat_ms
             FROM library_tracks
             ORDER BY id",
        )
        .map_err(database_read_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                ProjectTrack {
                    file_path: row.get(1)?,
                    file_name: row.get(2)?,
                    artist: row.get(3)?,
                    title: row.get(4)?,
                    duration_ms: row.get(5)?,
                    sample_rate: row.get(6)?,
                    channels: row.get(7)?,
                    bpm: row.get(8)?,
                    bpm_confidence: row.get(9)?,
                    first_beat_ms: row.get(10)?,
                    beat_count: row.get(11)?,
                    analysis_status: row.get(12)?,
                    analysis_version: row.get(13)?,
                    manual_bpm: row.get(14)?,
                    manual_first_beat_ms: row.get(15)?,
                    waveform: None,
                },
            ))
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    drop(statement);

    let mut track_ids = Vec::with_capacity(rows.len());
    let mut tracks = Vec::with_capacity(rows.len());
    for (id, mut track) in rows {
        track.waveform = read_waveform(connection, id)?;
        track_ids.push(id);
        tracks.push(track);
    }

    let index_of = |id: i64| -> Option<usize> { track_ids.iter().position(|other| *other == id) };

    let mut lane_statement = connection
        .prepare("SELECT lane, is_muted, is_solo FROM timeline_lanes ORDER BY lane")
        .map_err(database_read_error)?;
    let lanes = lane_statement
        .query_map([], |row| {
            Ok(ProjectLane {
                lane: row.get(0)?,
                is_muted: row.get::<_, i64>(1)? != 0,
                is_solo: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    drop(lane_statement);

    let mut clip_statement = connection
        .prepare(
            "SELECT library_track_id, lane, anchor_beat, tempo_anchor_beat,
                    trim_start_beats, trim_end_beats, is_sidechain_key, eq_settings,
                    stem, id
             FROM timeline_clips ORDER BY lane, anchor_beat",
        )
        .map_err(database_read_error)?;
    let clip_rows = clip_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(9)?,
                ProjectClip {
                    track_index: 0,
                    lane: row.get(1)?,
                    anchor_beat: row.get(2)?,
                    tempo_anchor_beat: row.get(3)?,
                    trim_start_beats: row.get(4)?,
                    trim_end_beats: row.get(5)?,
                    is_sidechain_key: row.get::<_, i64>(6)? != 0,
                    eq_settings: row.get(7)?,
                    stem: row.get(8)?,
                    stems: Vec::new(),
                    bake: None,
                },
            ))
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    drop(clip_statement);

    let mut clips = Vec::with_capacity(clip_rows.len());
    for (track_id, clip_id, mut clip) in clip_rows {
        clip.track_index = index_of(track_id)
            .ok_or_else(|| "A clip refers to a track that is not in the library.".to_owned())?;
        clip.stems = read_clip_stems(connection, clip_id)?;
        clip.bake = read_clip_bake(connection, clip_id)?;
        clips.push(clip);
    }

    let mut volume_statement = connection
        .prepare("SELECT lane, beat, gain_db FROM timeline_volume_nodes ORDER BY lane, beat")
        .map_err(database_read_error)?;
    let volume_nodes = volume_statement
        .query_map([], |row| {
            Ok(ProjectVolumeNode {
                lane: row.get(0)?,
                beat: row.get(1)?,
                gain_db: row.get(2)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    drop(volume_statement);

    let mut pan_statement = connection
        .prepare("SELECT lane, beat, value FROM timeline_pan_nodes ORDER BY lane, beat")
        .map_err(database_read_error)?;
    let pan_nodes = pan_statement
        .query_map([], |row| {
            Ok(ProjectPanNode {
                lane: row.get(0)?,
                beat: row.get(1)?,
                value: row.get(2)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    drop(pan_statement);

    let mut filter_statement = connection
        .prepare("SELECT lane, beat, value, tension FROM timeline_filter_nodes ORDER BY lane, beat")
        .map_err(database_read_error)?;
    let filter_nodes = filter_statement
        .query_map([], |row| {
            Ok(ProjectFilterNode {
                lane: row.get(0)?,
                beat: row.get(1)?,
                value: row.get(2)?,
                tension: row.get(3)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    drop(filter_statement);

    Ok(ProjectFile {
        format: PROJECT_FORMAT.to_owned(),
        version: PROJECT_VERSION,
        project_bpm,
        limiter_enabled,
        compressor_enabled,
        tracks,
        lanes,
        clips,
        volume_nodes,
        pan_nodes,
        filter_nodes,
    })
}

/// Les voix déjà séparées d'un clip.
///
/// La forme d'onde n'est pas emportée : elle se relit du fichier en une passe,
/// et la recopier dans le projet doublerait sa taille pour une donnée qu'on
/// sait reconstruire.
fn read_clip_stems(connection: &Connection, clip_id: i64) -> Result<Vec<ProjectClipStem>, String> {
    let mut statement = connection
        .prepare(
            "SELECT kind, file_path, source_from_ms
             FROM clip_stems WHERE clip_id = ?1 ORDER BY kind",
        )
        .map_err(database_read_error)?;
    let rows = statement
        .query_map([clip_id], |row| {
            Ok(ProjectClipStem {
                kind: row.get(0)?,
                file_path: row.get(1)?,
                source_from_ms: row.get(2)?,
            })
        })
        .map_err(database_read_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_read_error)?;
    Ok(rows)
}

fn read_clip_bake(
    connection: &Connection,
    clip_id: i64,
) -> Result<Option<ProjectClipBake>, String> {
    connection
        .query_row(
            "SELECT file_path, source_from_ms, removed FROM clip_bakes WHERE clip_id = ?1",
            [clip_id],
            |row| {
                Ok(ProjectClipBake {
                    file_path: row.get(0)?,
                    source_from_ms: row.get(1)?,
                    removed: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(database_read_error)
}

fn read_waveform(
    connection: &Connection,
    track_id: i64,
) -> Result<Option<ProjectWaveform>, String> {
    connection
        .query_row(
            "SELECT bucket_count, left_min, left_max, left_rms, right_min, right_max, right_rms
             FROM track_waveforms WHERE track_id = ?1",
            [track_id],
            |row| {
                let bucket_count: i64 = row.get(0)?;
                Ok(ProjectWaveform {
                    bucket_count: bucket_count.max(0) as usize,
                    left_min: BASE64.encode(row.get::<_, Vec<u8>>(1)?),
                    left_max: BASE64.encode(row.get::<_, Vec<u8>>(2)?),
                    left_rms: BASE64.encode(row.get::<_, Vec<u8>>(3)?),
                    right_min: BASE64.encode(row.get::<_, Vec<u8>>(4)?),
                    right_max: BASE64.encode(row.get::<_, Vec<u8>>(5)?),
                    right_rms: BASE64.encode(row.get::<_, Vec<u8>>(6)?),
                })
            },
        )
        .optional()
        .map_err(database_read_error)
}

pub fn write_to(connection: &Connection, path: &Path) -> Result<(), String> {
    let project = collect(connection)?;
    let text = serde_json::to_string_pretty(&project)
        .map_err(|error| format!("This project could not be written: {error}"))?;
    std::fs::write(path, text).map_err(|error| format!("This project could not be saved: {error}"))
}

pub fn read_from(path: &Path) -> Result<ProjectFile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("This project could not be opened: {error}"))?;
    let project: ProjectFile = serde_json::from_str(&text)
        .map_err(|_| "This file is not a MixCanvas project.".to_owned())?;
    if project.format != PROJECT_FORMAT {
        return Err("This file is not a MixCanvas project.".to_owned());
    }
    if project.version > PROJECT_VERSION {
        return Err("This project was saved by a newer version of MixCanvas.".to_owned());
    }
    Ok(project)
}

/// Remplace la session courante par celle du fichier.
///
/// La bibliothèque est **remise à l'état du fichier**, pas complétée : ouvrir un
/// projet doit donner la session qu'on a enregistrée, et non un mélange avec la
/// précédente. Les morceaux sont rapprochés par leur clé de chemin, ceux qui
/// sont déjà là gardent leur identifiant — inutile de réanalyser ce que la base
/// connaît déjà — et ceux que le fichier ne contient pas sont retirés.
///
/// Ne rien retirer serait sans danger mais faux : c'est ce que faisait la
/// première version, et la bibliothèque accumulait les morceaux de toutes les
/// sessions ouvertes.
pub fn apply(connection: &mut Connection, project: &ProjectFile) -> Result<(), String> {
    let transaction = connection.transaction().map_err(database_write_error)?;

    let mut track_ids = Vec::with_capacity(project.tracks.len());
    for track in &project.tracks {
        let key = normalize_path_key(Path::new(&track.file_path));
        let existing: Option<i64> = transaction
            .query_row(
                "SELECT id FROM library_tracks WHERE path_key = ?1",
                [&key],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_read_error)?;

        let id = match existing {
            Some(id) => {
                // Le fichier fait autorité sur la beatgrid : c'est l'état que
                // l'utilisateur a enregistré.
                transaction
                    .execute(
                        "UPDATE library_tracks
                         SET manual_bpm = ?2, manual_first_beat_ms = ?3
                         WHERE id = ?1",
                        params![id, track.manual_bpm, track.manual_first_beat_ms],
                    )
                    .map_err(database_write_error)?;
                id
            }
            None => {
                transaction
                    .execute(
                        "INSERT INTO library_tracks
                         (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                          bpm, analysis_status, added_at, bpm_confidence, first_beat_ms,
                          beat_count, manual_bpm, manual_first_beat_ms, analysis_version,
                          artist, title, id3_scanned)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, strftime('%s','now'),
                                 ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 1)",
                        params![
                            track.file_path,
                            key,
                            track.file_name,
                            track.duration_ms,
                            track.sample_rate,
                            track.channels,
                            track.bpm,
                            track.analysis_status,
                            track.bpm_confidence,
                            track.first_beat_ms,
                            track.beat_count,
                            track.manual_bpm,
                            track.manual_first_beat_ms,
                            track.analysis_version,
                            track.artist,
                            track.title,
                        ],
                    )
                    .map_err(database_write_error)?;
                transaction.last_insert_rowid()
            }
        };

        if let Some(waveform) = &track.waveform {
            store_waveform(&transaction, id, waveform)?;
        }
        track_ids.push(id);
    }

    transaction
        .execute_batch(
            "DELETE FROM timeline_clips;
             DELETE FROM timeline_volume_nodes;
             DELETE FROM timeline_pan_nodes;
             DELETE FROM timeline_filter_nodes;",
        )
        .map_err(database_write_error)?;

    // Les morceaux que le fichier ne mentionne pas s'en vont. Leurs formes
    // d'onde et leurs grilles suivent par cascade; les MP3 eux-mêmes ne sont
    // jamais touchés.
    if track_ids.is_empty() {
        transaction
            .execute("DELETE FROM library_tracks", [])
            .map_err(database_write_error)?;
    } else {
        let placeholders = vec!["?"; track_ids.len()].join(", ");
        let kept: Vec<&dyn rusqlite::ToSql> = track_ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        transaction
            .execute(
                &format!("DELETE FROM library_tracks WHERE id NOT IN ({placeholders})"),
                kept.as_slice(),
            )
            .map_err(database_write_error)?;
    }

    transaction
        .execute(
            "UPDATE project_settings
             SET project_bpm = ?1, limiter_enabled = ?2, compressor_enabled = ?3
             WHERE id = 1",
            params![
                project.project_bpm,
                i64::from(project.limiter_enabled),
                i64::from(project.compressor_enabled),
            ],
        )
        .map_err(database_write_error)?;

    for lane in &project.lanes {
        transaction
            .execute(
                "UPDATE timeline_lanes SET is_muted = ?2, is_solo = ?3 WHERE lane = ?1",
                params![lane.lane, i64::from(lane.is_muted), i64::from(lane.is_solo)],
            )
            .map_err(database_write_error)?;
    }

    for clip in &project.clips {
        let track_id = track_ids
            .get(clip.track_index)
            .copied()
            .ok_or_else(|| "This project refers to a track it does not contain.".to_owned())?;
        transaction
            .execute(
                "INSERT INTO timeline_clips
                 (library_track_id, lane, anchor_beat, created_at, tempo_anchor_beat,
                  eq_settings, trim_start_beats, trim_end_beats, is_sidechain_key, stem)
                 VALUES (?1, ?2, ?3, strftime('%s','now'), ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    track_id,
                    clip.lane,
                    clip.anchor_beat,
                    clip.tempo_anchor_beat,
                    clip.eq_settings,
                    clip.trim_start_beats,
                    clip.trim_end_beats,
                    i64::from(clip.is_sidechain_key),
                    clip.stem,
                ],
            )
            .map_err(database_write_error)?;

        // Les médias du clip suivent le clip qu'on vient d'insérer : c'est son
        // identifiant tout neuf qu'ils doivent porter, pas celui de la session
        // qui a écrit le projet.
        let clip_id = transaction.last_insert_rowid();
        for stem in &clip.stems {
            transaction
                .execute(
                    "INSERT INTO clip_stems (clip_id, kind, file_path, source_from_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![clip_id, stem.kind, stem.file_path, stem.source_from_ms],
                )
                .map_err(database_write_error)?;
        }
        if let Some(bake) = &clip.bake {
            transaction
                .execute(
                    "INSERT INTO clip_bakes (clip_id, file_path, source_from_ms, removed)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![clip_id, bake.file_path, bake.source_from_ms, bake.removed],
                )
                .map_err(database_write_error)?;
        }
    }

    for node in &project.volume_nodes {
        transaction
            .execute(
                "INSERT INTO timeline_volume_nodes (lane, beat, gain_db) VALUES (?1, ?2, ?3)",
                params![node.lane, node.beat, node.gain_db],
            )
            .map_err(database_write_error)?;
    }

    for node in &project.pan_nodes {
        transaction
            .execute(
                "INSERT INTO timeline_pan_nodes (lane, beat, value) VALUES (?1, ?2, ?3)",
                params![node.lane, node.beat, node.value],
            )
            .map_err(database_write_error)?;
    }

    for node in &project.filter_nodes {
        transaction
            .execute(
                "INSERT INTO timeline_filter_nodes (lane, beat, value, tension)
                 VALUES (?1, ?2, ?3, ?4)",
                params![node.lane, node.beat, node.value, node.tension],
            )
            .map_err(database_write_error)?;
    }

    transaction.commit().map_err(database_write_error)
}

fn store_waveform(
    transaction: &rusqlite::Transaction<'_>,
    track_id: i64,
    waveform: &ProjectWaveform,
) -> Result<(), String> {
    let peaks = WaveformPeaks {
        left_min: decode_ramp(&waveform.left_min, waveform.bucket_count)?,
        left_max: decode_ramp(&waveform.left_max, waveform.bucket_count)?,
        left_rms: decode_ramp(&waveform.left_rms, waveform.bucket_count)?,
        right_min: decode_ramp(&waveform.right_min, waveform.bucket_count)?,
        right_max: decode_ramp(&waveform.right_max, waveform.bucket_count)?,
        right_rms: decode_ramp(&waveform.right_rms, waveform.bucket_count)?,
    };
    save_waveform_in_transaction(transaction, track_id, &peaks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibraryStore;
    use rusqlite::params;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Les rampes partent en base64 des octets bruts de la base; ce miroir sert
    /// à vérifier que le décodage lit bien la même disposition.
    fn encode_ramp(values: &[f32]) -> String {
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        BASE64.encode(bytes)
    }

    #[test]
    fn a_ramp_survives_the_round_trip_exactly() {
        // Une forme d'onde porte des crêtes signées; un encodage qui perdrait
        // le signe replierait la moitié basse du dessin sur la haute.
        let values = vec![-1.0_f32, -0.5, -0.0009, 0.0, 0.0009, 0.5, 1.0];
        let encoded = encode_ramp(&values);
        let decoded = decode_ramp(&encoded, values.len()).expect("the ramp should decode");
        assert_eq!(decoded, values);
    }

    #[test]
    fn a_truncated_ramp_is_refused_rather_than_half_read() {
        let encoded = encode_ramp(&[0.25_f32; 4]);
        assert!(decode_ramp(&encoded, 5).is_err());
        assert!(decode_ramp("not base64 at all!!", 4).is_err());
    }

    fn scratch_store(suffix: &str) -> (LibraryStore, std::path::PathBuf, std::path::PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mixcanvas-project-{}-{suffix}-{stamp}.sqlite3",
            std::process::id()
        ));
        let fake_mp3 = database_path.with_extension("mp3");
        std::fs::write(&fake_mp3, []).expect("fake MP3 should be created");
        let store = LibraryStore::open(&database_path).expect("database should open");
        (store, database_path, fake_mp3)
    }

    fn scrub(paths: &[std::path::PathBuf]) {
        for path in paths {
            for suffix in ["", "-wal", "-shm"] {
                let candidate =
                    std::path::PathBuf::from(format!("{}{}", path.to_string_lossy(), suffix));
                let _ = std::fs::remove_file(candidate);
            }
        }
    }

    /// Un clip cuit doit traverser un enregistrement sans rien perdre.
    ///
    /// C'est là qu'il y avait perte sèche : le format ne portait ni le stem
    /// choisi, ni les fichiers séparés, ni la cuisson. Or `removed` est la
    /// **seule** copie de l'automation qu'une cuisson a emportée — rouvrir un
    /// projet rendait donc le clip sec, sur une voie plate, sans rien à
    /// restaurer.
    #[test]
    fn a_baked_clip_keeps_its_media_and_its_buried_automation() {
        let (mut origin, origin_path, fake_mp3) = scratch_store("bake-origin");
        origin
            .connection
            .execute(
                "INSERT INTO library_tracks
                 (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                  bpm, first_beat_ms, beat_count, analysis_status)
                 VALUES (?1, ?2, 'baked.mp3', 120000, 44100, 2, 120.0, 500, 240, 'analyzed')",
                params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
            )
            .expect("track should be inserted");
        let track_id = origin.connection.last_insert_rowid();
        let placed =
            crate::timeline::add_clip(&mut origin.connection, track_id, Some(8.0), Some(1))
                .expect("a clip should be added");
        let clip_id = placed.clips[0].id;

        // Un stem choisi, ses deux fichiers, et une cuisson par-dessus.
        let mut media = Vec::new();
        for kind in ["vocals", "instrumental"] {
            // `set_clip_stem` refuse une voix dont le fichier n'est pas là :
            // des chemins inventés ne passeraient pas cette porte.
            let file = fake_mp3.with_extension(format!("{kind}.wav"));
            std::fs::write(&file, []).expect("fake stem should be created");
            origin
                .connection
                .execute(
                    "INSERT INTO clip_stems (clip_id, kind, file_path, source_from_ms)
                     VALUES (?1, ?2, ?3, 1500)",
                    params![clip_id, kind, file.to_string_lossy()],
                )
                .expect("stem should be recorded");
            media.push(file);
        }
        crate::timeline::set_clip_stem(&origin.connection, clip_id, "vocals")
            .expect("the clip should play its vocals");
        let buried = r#"{"lane":1,"fromBeat":8.0,"toBeat":40.0,"volume":[[12.0,-9.5]],"pan":[],"filter":[[16.0,0.6,0.25]]}"#;
        origin
            .connection
            .execute(
                "INSERT INTO clip_bakes (clip_id, file_path, source_from_ms, removed)
                 VALUES (?1, ?2, 2500, ?3)",
                params![
                    clip_id,
                    fake_mp3.with_extension("baked.wav").to_string_lossy(),
                    buried
                ],
            )
            .expect("bake should be recorded");

        let text = serde_json::to_string(
            &collect(&origin.connection).expect("the session should be collected"),
        )
        .expect("the project should serialize");

        let (mut target, target_path, _) = scratch_store("bake-target");
        let reread: ProjectFile = serde_json::from_str(&text).expect("it should read back");
        apply(&mut target.connection, &reread).expect("the project should apply");

        let restored =
            crate::timeline::snapshot(&target.connection).expect("the timeline should read");
        assert_eq!(restored.clips.len(), 1);
        let clip = &restored.clips[0];
        assert_eq!(
            clip.stem, "vocals",
            "la voix choisie traverse l'enregistrement"
        );
        assert!(clip.has_stems, "et les fichiers séparés avec elle");
        assert!(clip.is_baked, "le clip est toujours cuit");

        let baked_path = fake_mp3.with_extension("baked.wav");
        let (path, offset, removed) = target
            .connection
            .query_row(
                "SELECT file_path, source_from_ms, removed FROM clip_bakes WHERE clip_id = ?1",
                [clip.id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .expect("the bake should be back");
        assert_eq!(path, baked_path.to_string_lossy());
        assert_eq!(offset, 2500);
        assert_eq!(
            removed, buried,
            "l'automation enfouie revient au caractère près — c'est la seule copie"
        );

        // Le clip a un identifiant neuf dans la base cible : les médias doivent
        // avoir suivi *ce* clip, pas celui de la session qui a écrit le projet.
        let orphans: i64 = target
            .connection
            .query_row(
                "SELECT COUNT(*) FROM clip_stems WHERE clip_id <> ?1",
                [clip.id],
                |row| row.get(0),
            )
            .expect("stems should count");
        assert_eq!(
            orphans, 0,
            "aucun média ne pointe vers un clip qui n'existe pas"
        );

        scrub(&[origin_path, target_path]);
        let _ = std::fs::remove_file(&fake_mp3);
        for file in media {
            let _ = std::fs::remove_file(file);
        }
    }

    #[test]
    fn a_project_rebuilds_the_same_session_in_a_database_that_never_saw_it() {
        let (mut origin, origin_path, fake_mp3) = scratch_store("origin");
        origin
            .connection
            .execute(
                "INSERT INTO library_tracks
                 (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                  bpm, first_beat_ms, beat_count, analysis_status, manual_bpm,
                  manual_first_beat_ms)
                 VALUES (?1, ?2, 'session.mp3', 60000, 44100, 2, 120.0, 500, 120,
                         'analyzed', 128.5, 1750)",
                params![fake_mp3.to_string_lossy(), fake_mp3.to_string_lossy()],
            )
            .expect("track should be inserted");
        let track_id = origin.connection.last_insert_rowid();
        crate::timeline::add_clip(&mut origin.connection, track_id, Some(8.0), Some(1))
            .expect("a clip should be added");
        crate::timeline::set_lane_muted(&origin.connection, 2, true).expect("lane should mute");

        let file = collect(&origin.connection).expect("the session should be collected");
        let text = serde_json::to_string(&file).expect("the project should serialize");

        // Une base neuve, qui n'a jamais vu ce morceau : la situation d'un
        // projet transporté sur une autre machine.
        let (mut target, target_path, _) = scratch_store("target");
        let reread: ProjectFile = serde_json::from_str(&text).expect("it should read back");
        apply(&mut target.connection, &reread).expect("the project should apply");

        let restored = crate::timeline::snapshot(&target.connection)
            .expect("the rebuilt timeline should read");
        assert_eq!(restored.clips.len(), 1, "the clip should come back");
        let clip = &restored.clips[0];
        assert_eq!(clip.lane, 1, "on the lane it was saved from");
        assert_eq!(clip.anchor_beat, 8);
        // La correction manuelle est la seule chose qu'aucune ré-analyse ne
        // retrouverait : elle doit traverser le fichier intacte.
        assert_eq!(clip.bpm, Some(128.5), "the manual BPM should survive");
        assert!(
            restored
                .lanes
                .iter()
                .any(|lane| lane.lane == 2 && lane.is_muted),
            "lane states should survive"
        );

        scrub(&[origin_path, target_path, fake_mp3]);
    }

    #[test]
    fn loading_leaves_the_library_holding_the_project_and_nothing_else() {
        // Le défaut signalé : la bibliothèque cumulait les morceaux de toutes
        // les sessions ouvertes, si bien qu'un projet chargé s'ajoutait au
        // précédent au lieu de le remplacer.
        let (origin, origin_path, origin_mp3) = scratch_store("saved");
        origin
            .connection
            .execute(
                "INSERT INTO library_tracks
                 (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                  analysis_status)
                 VALUES (?1, ?2, 'saved.mp3', 60000, 44100, 2, 'analyzed')",
                params![origin_mp3.to_string_lossy(), origin_mp3.to_string_lossy()],
            )
            .expect("the saved track should be inserted");
        let file = collect(&origin.connection).expect("the session should be collected");

        // Une base qui contient déjà un tout autre morceau.
        let (mut target, target_path, stray_mp3) = scratch_store("stray");
        target
            .connection
            .execute(
                "INSERT INTO library_tracks
                 (file_path, path_key, file_name, duration_ms, sample_rate, channels,
                  analysis_status)
                 VALUES (?1, ?2, 'stray.mp3', 60000, 44100, 2, 'analyzed')",
                params![stray_mp3.to_string_lossy(), stray_mp3.to_string_lossy()],
            )
            .expect("the stray track should be inserted");

        apply(&mut target.connection, &file).expect("the project should apply");

        let names: Vec<String> = target
            .connection
            .prepare("SELECT file_name FROM library_tracks ORDER BY file_name")
            .expect("the library should read")
            .query_map([], |row| row.get(0))
            .expect("names should map")
            .collect::<Result<_, _>>()
            .expect("names should collect");
        assert_eq!(
            names,
            vec!["saved.mp3".to_owned()],
            "the library should hold the project's tracks and no others"
        );

        scrub(&[origin_path, target_path, origin_mp3, stray_mp3]);
    }

    #[test]
    fn a_file_that_is_not_a_project_is_refused_by_name() {
        let stray =
            std::env::temp_dir().join(format!("mixcanvas-stray-{}.json", std::process::id()));
        std::fs::write(&stray, br#"{"format":"something-else","version":1}"#).expect("write");
        let refused = read_from(&stray).expect_err("a foreign file should be refused");
        assert!(refused.contains("not a MixCanvas project"), "got {refused}");
        let _ = std::fs::remove_file(stray);
    }
}
