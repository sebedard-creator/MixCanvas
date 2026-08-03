pub mod analysis;
mod audio;
mod library;
mod media;
mod project;
mod tempo;
mod timeline;
mod transport;

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use analysis::{BeatModelPaths, analyze_mp3, analyze_mp3_near, analyze_waveform};
use audio::{
    BounceSummary, PreviewEngine, PreviewSnapshot, TimelinePlaybackEngine, bounce_timeline,
};
use library::{AnalysisBatchResult, LibraryImportResult, LibraryStore, LibraryTrack};
use tauri::{Emitter, Manager, State};
use timeline::TimelineSnapshot;
use transport::{TimelineTransport, TimelineTransportSnapshot};

type AudioState = Arc<Mutex<PreviewEngine>>;
type LibraryState = Arc<Mutex<LibraryStore>>;
type AnalysisState = Arc<AtomicBool>;
type TimelineTransportState = Arc<Mutex<TimelineTransport>>;
type TimelinePlaybackState = Arc<Mutex<TimelinePlaybackEngine>>;
/// Où vont les médias, et à quel projet ils appartiennent en ce moment.
///
/// Le nom démarre à `Scratch` et suit le fichier dès qu'on enregistre ou qu'on
/// ouvre. C'est lui qui décide du sous-dossier : sans état, chaque stem serait
/// versé au même endroit que ceux de tous les autres projets, ce qui était le
/// cas et ce qui empêchait de savoir à qui appartenait quoi.
struct MediaLocation {
    root: std::path::PathBuf,
    project: String,
}

type MediaState = Arc<Mutex<MediaLocation>>;

fn with_audio_engine(
    state: &State<'_, AudioState>,
    operation: impl FnOnce(&mut PreviewEngine) -> Result<PreviewSnapshot, String>,
) -> Result<PreviewSnapshot, String> {
    let mut engine = state
        .lock()
        .map_err(|_| "The audio engine is in an invalid state.".to_owned())?;

    operation(&mut engine)
}

#[tauri::command]
fn load_preview(
    path: String,
    state: State<'_, AudioState>,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<PreviewSnapshot, String> {
    suspend_timeline_audio(&library_state, &playback_state, &transport_state)?;
    with_audio_engine(&state, |engine| engine.load(path))
}

fn suspend_timeline_audio(
    library_state: &State<'_, LibraryState>,
    playback_state: &State<'_, TimelinePlaybackState>,
    transport_state: &State<'_, TimelineTransportState>,
) -> Result<(), String> {
    let (tempo_map, end_beat) = timeline_timing(library_state)?;
    {
        let mut playback = playback_state
            .lock()
            .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?;
        playback.pause_if_playing();
        playback.release_output();
    }
    with_timeline_transport(transport_state, |transport| {
        transport.pause(&tempo_map, end_beat)
    })?;
    Ok(())
}

#[tauri::command]
fn play_preview(
    state: State<'_, AudioState>,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<PreviewSnapshot, String> {
    suspend_timeline_audio(&library_state, &playback_state, &transport_state)?;
    with_audio_engine(&state, PreviewEngine::play)
}

#[tauri::command]
fn pause_preview(state: State<'_, AudioState>) -> Result<PreviewSnapshot, String> {
    with_audio_engine(&state, PreviewEngine::pause)
}

#[tauri::command]
fn stop_preview(state: State<'_, AudioState>) -> Result<PreviewSnapshot, String> {
    with_audio_engine(&state, PreviewEngine::stop)
}

#[tauri::command]
fn seek_preview(position_ms: u64, state: State<'_, AudioState>) -> Result<PreviewSnapshot, String> {
    with_audio_engine(&state, |engine| engine.seek(position_ms))
}

#[tauri::command]
fn set_preview_speed(speed: f32, state: State<'_, AudioState>) -> Result<PreviewSnapshot, String> {
    with_audio_engine(&state, |engine| engine.set_speed(speed))
}

#[tauri::command]
fn preview_snapshot(state: State<'_, AudioState>) -> Result<PreviewSnapshot, String> {
    with_audio_engine(&state, |engine| Ok(engine.snapshot()))
}

fn with_library(
    state: &State<'_, LibraryState>,
    operation: impl FnOnce(&mut LibraryStore) -> Result<Vec<LibraryTrack>, String>,
) -> Result<Vec<LibraryTrack>, String> {
    let mut library = state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?;

    operation(&mut library)
}

#[tauri::command]
fn list_library_tracks(state: State<'_, LibraryState>) -> Result<Vec<LibraryTrack>, String> {
    with_library(&state, |library| library.list_tracks())
}

#[tauri::command]
async fn import_library_paths(
    paths: Vec<String>,
    state: State<'_, LibraryState>,
) -> Result<LibraryImportResult, String> {
    let library = Arc::clone(state.inner());

    tauri::async_runtime::spawn_blocking(move || {
        let mut library = library
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?;
        library.import_paths(paths)
    })
    .await
    .map_err(|error| format!("The library import failed: {error}"))?
}

#[tauri::command]
fn remove_library_track(
    id: i64,
    state: State<'_, LibraryState>,
) -> Result<Vec<LibraryTrack>, String> {
    with_library(&state, |library| library.remove_track(id))
}

#[tauri::command]
fn update_track_beatgrid(
    id: i64,
    bpm: f64,
    first_beat_ms: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<LibraryTrack>, String> {
    with_library(&state, |library| {
        library.update_beatgrid_correction(id, bpm, first_beat_ms)
    })
}

fn with_timeline(
    state: &State<'_, LibraryState>,
    operation: impl FnOnce(&mut rusqlite::Connection) -> Result<TimelineSnapshot, String>,
) -> Result<TimelineSnapshot, String> {
    let mut library = state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?;

    operation(&mut library.connection)
}

/// Écrit la session courante dans un fichier de projet.
/// Rend le mix complet dans un fichier WAV, hors ligne.
///
/// Le verrou de la bibliothèque n'est tenu que le temps de construire le plan :
/// le rendu lui-même peut durer des minutes, et l'interface doit rester
/// vivante. Il part sur un fil bloquant pour la même raison.
#[tauri::command]
async fn bounce_mix(
    path: String,
    app: tauri::AppHandle,
    library_state: State<'_, LibraryState>,
) -> Result<BounceSummary, String> {
    let plan = {
        let library = library_state
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?;
        timeline::render_plan(&library.connection)?
    };

    tauri::async_runtime::spawn_blocking(move || {
        let mut report = |fraction: f64| {
            // Un échec d'émission ne doit pas interrompre un rendu de plusieurs
            // minutes : au pire la barre cesse d'avancer.
            let _ = app.emit("bounce-progress", fraction);
        };
        bounce_timeline(&plan, std::path::Path::new(&path), &mut report)
    })
    .await
    .map_err(|error| format!("The bounce was interrupted: {error}"))?
}

/// Où trouver la bibliothèque ONNX et le modèle.
///
/// Trois endroits possibles, dans cet ordre : le dossier de ressources du
/// paquet installé, celui de l'exécutable, et l'arborescence du dépôt pendant
/// le développement. Une seule de ces pistes marche selon la façon dont le
/// programme a été lancé, et se tromper donnait « This install is missing its
/// resources folder » — un message qui accuse l'installation alors que rien
/// n'est cassé.
///
/// L'erreur énumère ce qui a été cherché et où : un chemin manquant se diagnostique
/// en le lisant, pas en le devinant.
/// Les deux fichiers lourds, emportés dans l'exécutable.
///
/// Compilés seulement avec `--features embed-resources`, pour le paquet
/// portable : un utilisateur copie un fichier et il fonctionne, sans dossier
/// à garder à côté ni rien à installer.
#[cfg(feature = "embed-resources")]
mod embedded {
    pub const RUNTIME: &[u8] = include_bytes!("../resources/onnxruntime.dll");
    pub const PROVIDERS: &[u8] = include_bytes!("../resources/onnxruntime_providers_shared.dll");
    pub const MODEL: &[u8] = include_bytes!("../resources/models/open-unmix-vocals-fp16.onnx");
    pub const BEAT_MODEL: &[u8] = include_bytes!("../resources/models/beat_this_small.onnx");
    pub const MEL_MODEL: &[u8] = include_bytes!("../resources/models/mel_spectrogram.onnx");
}

/// Dépose les fichiers embarqués à côté de la base, une fois pour toutes.
///
/// Écrits une seule fois : les relire à chaque lancement coûterait trente-cinq
/// mégaoctets de copie pour rien. La taille sert de contrôle — un fichier
/// tronqué par un disque plein serait réécrit au lancement suivant plutôt que
/// de faire échouer la séparation sans explication.
#[cfg(feature = "embed-resources")]
fn unpack_embedded_resources(folder: &Path) -> Result<(), String> {
    fs::create_dir_all(folder.join("models"))
        .map_err(|error| format!("Could not prepare the resources folder: {error}"))?;
    for (relative, bytes) in [
        ("onnxruntime.dll", embedded::RUNTIME),
        ("onnxruntime_providers_shared.dll", embedded::PROVIDERS),
        ("models/open-unmix-vocals-fp16.onnx", embedded::MODEL),
        ("models/beat_this_small.onnx", embedded::BEAT_MODEL),
        ("models/mel_spectrogram.onnx", embedded::MEL_MODEL),
    ] {
        let target = folder.join(relative);
        if target
            .metadata()
            .is_ok_and(|data| data.len() == bytes.len() as u64)
        {
            continue;
        }
        fs::write(&target, bytes)
            .map_err(|error| format!("Could not unpack {relative}: {error}"))?;
    }
    Ok(())
}

fn resource_folder_candidates(app: &tauri::AppHandle) -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.clone());
        candidates.push(resources.join("resources"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(folder) = exe.parent()
    {
        candidates.push(folder.to_path_buf());
        candidates.push(folder.join("resources"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(target) = exe.parent().and_then(|folder| folder.parent())
        && let Some(root) = target.parent()
    {
        candidates.push(root.join("resources"));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources"));
    candidates.sort();
    candidates.dedup();
    candidates
}

fn beat_analysis_resources(app: &tauri::AppHandle) -> Result<BeatModelPaths, String> {
    const MEL_MODEL: &str = "mel_spectrogram.onnx";
    const BEAT_MODEL: &str = "beat_this_small.onnx";
    let candidates = resource_folder_candidates(app);
    for folder in &candidates {
        let mel = folder.join("models").join(MEL_MODEL);
        let beats = folder.join("models").join(BEAT_MODEL);
        if mel.is_file() && beats.is_file() {
            return Ok(BeatModelPaths { mel, beats });
        }
    }

    #[cfg(feature = "embed-resources")]
    if let Some(folder) = unpacked_resources_folder(app) {
        unpack_embedded_resources(&folder)?;
        let mel = folder.join("models").join(MEL_MODEL);
        let beats = folder.join("models").join(BEAT_MODEL);
        if mel.is_file() && beats.is_file() {
            return Ok(BeatModelPaths { mel, beats });
        }
    }

    let looked = candidates
        .iter()
        .map(|folder| folder.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "BPM analysis needs models/{MEL_MODEL} and models/{BEAT_MODEL}. Looked in: {looked}"
    ))
}

fn separation_resources(
    app: &tauri::AppHandle,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    const RUNTIME: &str = "onnxruntime.dll";
    const MODEL: &str = "open-unmix-vocals-fp16.onnx";

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.clone());
        candidates.push(resources.join("resources"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(folder) = exe.parent()
    {
        candidates.push(folder.to_path_buf());
        candidates.push(folder.join("resources"));
    }
    // Le dépôt, pendant le développement : l'exécutable est dans
    // `src-tauri/target/debug`, les ressources trois niveaux plus haut.
    if let Ok(exe) = std::env::current_exe()
        && let Some(target) = exe.parent().and_then(|folder| folder.parent())
        && let Some(root) = target.parent()
    {
        candidates.push(root.join("resources"));
    }

    for folder in &candidates {
        let runtime = folder.join(RUNTIME);
        let model = folder.join("models").join(MODEL);
        if runtime.is_file() && model.is_file() {
            return Ok((runtime, model));
        }
    }

    // Rien à côté de l'exécutable : le portable d'un seul fichier dépose ce
    // qu'il porte, puis se sert dedans.
    #[cfg(feature = "embed-resources")]
    if let Some(folder) = unpacked_resources_folder(app) {
        unpack_embedded_resources(&folder)?;
        let runtime = folder.join(RUNTIME);
        let model = folder.join("models").join(MODEL);
        if runtime.is_file() && model.is_file() {
            return Ok((runtime, model));
        }
    }

    let looked = candidates
        .iter()
        .map(|folder| folder.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "Stem separation needs {RUNTIME} and models/{MODEL}. Looked in: {looked}"
    ))
}

/// Sépare un morceau en deux stems, hors ligne.
///
/// Comme le bounce : le verrou n'est tenu que le temps de lire le chemin, et le
/// travail part sur un fil bloquant. Une séparation dure des minutes, et
/// l'interface doit rester vivante.
///
/// La séparation appartient au **morceau**. Elle n'est faite qu'une fois : tous
/// les clips de ce morceau, présents et futurs, basculeront ensuite sans rien
/// recalculer.
#[tauri::command]
async fn separate_clip_stems(
    clip_id: i64,
    app: tauri::AppHandle,
    library_state: State<'_, LibraryState>,
    media_state: State<'_, MediaState>,
) -> Result<TimelineSnapshot, String> {
    // La fenêtre du clip, en millisecondes de la source : c'est tout ce qui sera
    // séparé. Elle est lue sous le verrou, puis relâchée — le rendu dure des
    // minutes.
    let (source, window) = {
        let library = library_state
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?;
        let snapshot = timeline::snapshot(&library.connection)?;
        let clip = snapshot
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| "This clip is no longer on the timeline.".to_owned())?;
        let window = timeline::clip_source_window_ms(clip)
            .ok_or_else(|| "This track needs its BPM analyzed first.".to_owned())?;
        (clip.file_path.clone(), window)
    };

    let (runtime, model) = separation_resources(&app)?;
    let output_dir = media_folder(&media_state, "stems")?;

    let reporter = app.clone();
    let files = tauri::async_runtime::spawn_blocking(move || {
        let mut report = |fraction: f64| {
            // Comme pour le bounce : un échec d'émission ne doit pas
            // interrompre un travail de plusieurs minutes.
            let _ = reporter.emit("stems-progress", fraction);
        };
        audio::stems::separate_track(
            std::path::Path::new(&source),
            &runtime,
            &model,
            &output_dir,
            Some(window),
            &format!("clip-{clip_id}"),
            &mut report,
        )
    })
    .await
    .map_err(|error| format!("The separation was interrupted: {error}"))??;

    let library = library_state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?;
    for (kind, path, peaks) in [
        ("vocals", files.vocals, files.vocals_waveform),
        (
            "instrumental",
            files.instrumental,
            files.instrumental_waveform,
        ),
    ] {
        library
            .connection
            .execute(
                "INSERT INTO clip_stems
                 (clip_id, kind, file_path, source_from_ms, bucket_count,
                  left_min, left_max, left_rms, right_min, right_max, right_rms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(clip_id, kind) DO UPDATE SET
                     file_path = excluded.file_path,
                     source_from_ms = excluded.source_from_ms,
                     bucket_count = excluded.bucket_count,
                     left_min = excluded.left_min,
                     left_max = excluded.left_max,
                     left_rms = excluded.left_rms,
                     right_min = excluded.right_min,
                     right_max = excluded.right_max,
                     right_rms = excluded.right_rms",
                rusqlite::params![
                    clip_id,
                    kind,
                    path.to_string_lossy(),
                    files.source_from_ms.round() as i64,
                    peaks.left_min.len() as i64,
                    library::encode_waveform_values(&peaks.left_min),
                    library::encode_waveform_values(&peaks.left_max),
                    library::encode_waveform_values(&peaks.left_rms),
                    library::encode_waveform_values(&peaks.right_min),
                    library::encode_waveform_values(&peaks.right_max),
                    library::encode_waveform_values(&peaks.right_rms),
                ],
            )
            .map_err(|error| format!("Could not record the stem: {error}"))?;
    }
    timeline::snapshot(&library.connection)
}

/// Cuit un clip : ses effets passent dans un fichier à lui.
///
/// Le verrou de la bibliothèque n'est tenu que pour préparer puis pour ranger.
/// Entre les deux, le rendu tourne seul — il dure des secondes sur un clip
/// court, mais l'interface doit rester vivante, et la lecture aussi.
#[tauri::command]
async fn bake_clip(
    clip_id: i64,
    app: tauri::AppHandle,
    library_state: State<'_, LibraryState>,
    media_state: State<'_, MediaState>,
) -> Result<TimelineSnapshot, String> {
    let spec = {
        let library = library_state
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?;
        timeline::prepare_bake(&library.connection, clip_id)?
    };

    let folder = media_folder(&media_state, "bakes")?;
    // L'horodatage évite qu'une seconde cuisson du même clip écrase le fichier
    // que le moteur est peut-être en train de lire.
    let path = folder.join(format!(
        "clip-{clip_id}-{}.wav",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis())
            .unwrap_or_default()
    ));

    let reporter = app.clone();
    let render_path = path.clone();
    let plan = spec.plan;
    tauri::async_runtime::spawn_blocking(move || {
        let mut report = |fraction: f64| {
            let _ = reporter.emit("bake-progress", fraction);
        };
        bounce_timeline(&plan, &render_path, &mut report)
    })
    .await
    .map_err(|error| format!("The bake was interrupted: {error}"))??;

    // La forme d'onde du fichier cuit : le clip doit montrer ce qu'il joue
    // maintenant, filtre compris, et non l'onde d'avant la cuisson.
    let waveform = analyze_waveform(&path).ok();

    let mut library = library_state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?;
    timeline::commit_bake(
        &mut library.connection,
        clip_id,
        &path.to_string_lossy(),
        spec.source_from_ms,
        &spec.removed,
        waveform.as_ref(),
    )
}

/// Défait une cuisson : l'automation revient, le fichier part.
///
/// Le fichier est effacé **après** que la base a validé. Dans l'autre ordre, un
/// échec d'écriture laisserait un enregistrement pointant vers un fichier
/// disparu — un clip qui joue sans ses effets sans qu'on sache pourquoi.
#[tauri::command]
fn unbake_clip(
    clip_id: i64,
    library_state: State<'_, LibraryState>,
) -> Result<TimelineSnapshot, String> {
    let mut library = library_state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?;
    let (snapshot, file_path) = timeline::unbake_clip(&mut library.connection, clip_id)?;
    if let Some(path) = file_path {
        // Un fichier qu'on n'arrive pas à effacer — encore ouvert par le
        // moteur, disque en lecture seule — ne doit pas faire échouer une
        // opération que la base a déjà validée. Il ne coûte que sa place.
        let _ = fs::remove_file(path);
    }
    Ok(snapshot)
}

/// Le dossier où écrire un média du projet courant.
fn media_folder(media_state: &State<'_, MediaState>, kind: &str) -> Result<PathBuf, String> {
    let location = media_state
        .lock()
        .map_err(|_| "The media folder is in an invalid state.".to_owned())?;
    media::project_media_folder(&location.root, &location.project, kind)
}

#[tauri::command]
fn save_project(
    path: String,
    library_state: State<'_, LibraryState>,
    media_state: State<'_, MediaState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<(), String> {
    let target = std::path::Path::new(&path);
    let project = media::project_folder_name(target);
    let (root, current) = {
        let location = media_state
            .lock()
            .map_err(|_| "The media folder is in an invalid state.".to_owned())?;
        (location.root.clone(), location.project.clone())
    };

    // Le moteur tient ses décodeurs ouverts pendant la lecture, et Windows
    // refuse de déplacer un fichier ouvert. On le fait taire avant de toucher
    // aux fichiers — comme pour l'ouverture d'un projet, et pour la même
    // raison.
    if media::relocation_for(&current, &project) != media::Relocation::None {
        suspend_timeline_audio(&library_state, &playback_state, &transport_state)?;
    }

    let mut library = library_state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?;
    media::relocate_project_media(&mut library.connection, &root, &current, &project)?;
    // Le projet est écrit **après** le déménagement : c'est ainsi qu'il porte
    // les chemins d'arrivée. Écrit avant, il désignerait des fichiers qui ne
    // sont déjà plus là.
    project::write_to(&library.connection, target)?;

    let mut location = media_state
        .lock()
        .map_err(|_| "The media folder is in an invalid state.".to_owned())?;
    location.project = project;
    Ok(())
}

/// Remplace la session courante par celle d'un fichier de projet.
///
/// Le moteur audio est arrêté avant l'écriture : reconstruire la timeline sous
/// une lecture en cours reviendrait à changer le plan pendant qu'il est joué.
#[tauri::command]
fn load_project(
    path: String,
    library_state: State<'_, LibraryState>,
    media_state: State<'_, MediaState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let file = project::read_from(std::path::Path::new(&path))?;
    suspend_timeline_audio(&library_state, &playback_state, &transport_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        project::apply(connection, &file)?;
        timeline::snapshot(connection)
    })?;

    // Les médias qu'on écrira ensuite appartiennent désormais à ce projet-ci.
    // Sans ça, un stem séparé après une ouverture serait écrit dans le dossier
    // du projet précédent, et le déménagement suivant l'y oublierait.
    let mut location = media_state
        .lock()
        .map_err(|_| "The media folder is in an invalid state.".to_owned())?;
    location.project = media::project_folder_name(std::path::Path::new(&path));
    Ok(snapshot)
}

#[tauri::command]
fn timeline_snapshot(state: State<'_, LibraryState>) -> Result<TimelineSnapshot, String> {
    with_timeline(&state, |connection| timeline::snapshot(connection))
}

#[tauri::command]
fn add_timeline_clip(
    library_track_id: i64,
    anchor_beat: Option<f64>,
    lane: Option<i64>,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::add_clip(connection, library_track_id, anchor_beat, lane)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn move_timeline_clip(
    clip_id: i64,
    anchor_beat: f64,
    lane: i64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::move_clip(connection, clip_id, anchor_beat, lane)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn trim_timeline_clip(
    clip_id: i64,
    trim_start_beats: f64,
    trim_end_beats: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::set_clip_trim(connection, clip_id, trim_start_beats, trim_end_beats)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn move_timeline_tempo_point(
    clip_id: i64,
    tempo_anchor_beat: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::move_tempo_point(connection, clip_id, tempo_anchor_beat)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn remove_timeline_clip(
    clip_id: i64,
    state: State<'_, LibraryState>,
) -> Result<TimelineSnapshot, String> {
    with_timeline(&state, |connection| {
        timeline::remove_clip(connection, clip_id)
    })
}

fn synchronize_lane_mix(
    snapshot: &TimelineSnapshot,
    playback_state: &State<'_, TimelinePlaybackState>,
) -> Result<(), String> {
    playback_state
        .lock()
        .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?
        .set_audible_lane_mask(timeline::audible_lane_mask(snapshot));
    Ok(())
}

#[tauri::command]
fn set_timeline_lane_muted(
    lane: i64,
    is_muted: bool,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
) -> Result<TimelineSnapshot, String> {
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::set_lane_muted(connection, lane, is_muted)
    })?;
    synchronize_lane_mix(&snapshot, &playback_state)?;
    Ok(snapshot)
}

#[tauri::command]
fn set_timeline_limiter_enabled(
    limiter_enabled: bool,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
) -> Result<TimelineSnapshot, String> {
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::set_limiter_enabled(connection, limiter_enabled)
    })?;
    // Shared atomically with the queued source, like Mute and Solo: the change
    // is audible immediately, without rebuilding the plan.
    playback_state
        .lock()
        .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?
        .set_limiter_enabled(snapshot.limiter_enabled);
    Ok(snapshot)
}

#[tauri::command]
fn set_timeline_compressor_enabled(
    compressor_enabled: bool,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
) -> Result<TimelineSnapshot, String> {
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::set_compressor_enabled(connection, compressor_enabled)
    })?;
    playback_state
        .lock()
        .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?
        .set_compressor_enabled(snapshot.compressor_enabled);
    Ok(snapshot)
}

/// Naming the key changes which clip is heard, so unlike the master switches
/// it rebuilds the playback plan rather than flipping an atomic.
#[tauri::command]
fn set_timeline_sidechain_key(
    clip_id: i64,
    is_key: bool,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::set_sidechain_key(connection, clip_id, is_key)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn set_timeline_lane_solo(
    lane: i64,
    is_solo: bool,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
) -> Result<TimelineSnapshot, String> {
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::set_lane_solo(connection, lane, is_solo)
    })?;
    synchronize_lane_mix(&snapshot, &playback_state)?;
    Ok(snapshot)
}

#[tauri::command]
fn add_timeline_volume_node(
    lane: i64,
    beat: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::add_volume_node(connection, lane, beat)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn move_timeline_volume_node(
    node_id: i64,
    beat: f64,
    gain_db: Option<f64>,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::move_volume_node(connection, node_id, beat, gain_db)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn delete_timeline_volume_node(
    node_id: i64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::delete_volume_node(connection, node_id)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

/// Écrit une forme d'automation d'un seul trait.
///
/// Les nœuds arrivent calculés par l'interface : la géométrie d'une forme est
/// la même des deux côtés, et la dupliquer en Rust ferait diverger ce qu'on
/// voit de ce qu'on entend. Le serveur borne et valide, il ne redessine pas.
// Même raison que `draw_timeline_filter_bubble` : six paramètres de domaine,
// sous la limite, plus les trois états gérés que Tauri injecte. Ce sont ces
// derniers qui font passer le compte à neuf.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn draw_timeline_volume_shape(
    lane: i64,
    start_beat: f64,
    end_beat: f64,
    nodes: Vec<(f64, f64)>,
    shape: String,
    period: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::draw_volume_shape(
            connection, lane, start_beat, end_beat, &nodes, &shape, period,
        )
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

// Même raison que `draw_timeline_filter_bubble`.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn draw_timeline_pan_shape(
    lane: i64,
    start_beat: f64,
    end_beat: f64,
    nodes: Vec<(f64, f64)>,
    shape: String,
    period: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::draw_pan_shape(
            connection, lane, start_beat, end_beat, &nodes, &shape, period,
        )
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn draw_timeline_filter_stroke(
    lane: i64,
    nodes: Vec<(f64, f64)>,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::draw_filter_stroke(connection, lane, &nodes)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn set_timeline_clip_stem(
    clip_id: i64,
    stem: String,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::set_clip_stem(connection, clip_id, &stem)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn add_timeline_pan_node(
    lane: i64,
    beat: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::add_pan_node(connection, lane, beat)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn move_timeline_pan_node(
    node_id: i64,
    beat: f64,
    value: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::move_pan_node(connection, node_id, beat, value)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn delete_timeline_pan_node(
    node_id: i64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::delete_pan_node(connection, node_id)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn delete_timeline_draw_group(
    group_id: i64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::delete_draw_group(connection, group_id)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

// A Tauri command receives its parameters flat, alongside the three pieces of
// managed state it needs; grouping them would only hide the IPC signature.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn draw_timeline_filter_bubble(
    lane: i64,
    start_beat: f64,
    width_beats: f64,
    value: f64,
    shape: Option<String>,
    replaced_start_beat: Option<f64>,
    replaced_end_beat: Option<f64>,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let replaced_range = replaced_start_beat.zip(replaced_end_beat);
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::draw_filter_bubble(
            connection,
            lane,
            start_beat,
            width_beats,
            value,
            shape,
            replaced_range,
        )
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn clear_timeline_filter_range(
    lane: i64,
    start_beat: f64,
    end_beat: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::clear_filter_range(connection, lane, start_beat, end_beat)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn restore_timeline_snapshot(
    snapshot: TimelineSnapshot,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let restored = with_timeline(&library_state, |connection| {
        timeline::restore_snapshot(connection, &snapshot)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(restored)
}

#[tauri::command]
fn save_clip_eq(
    clip_id: i64,
    eq_settings: timeline::ClipEqSettings,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::save_clip_eq(connection, clip_id, &eq_settings)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn split_timeline_clip(
    clip_id: i64,
    split_beat: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::split_timeline_clip(connection, clip_id, split_beat)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

#[tauri::command]
fn clear_timeline(
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    let previous_timing = timeline_timing(&library_state)?;
    let snapshot = with_timeline(&library_state, |connection| {
        timeline::clear_timeline(connection)
    })?;
    refresh_live_timeline_after_edit(
        &library_state,
        &playback_state,
        &transport_state,
        previous_timing,
    )?;
    Ok(snapshot)
}

/// Efface la bibliothèque et la timeline d'un coup.
///
/// Le moteur est arrêté avant, comme au chargement d'un projet : vider la base
/// sous une lecture en cours reviendrait à retirer le plan des mains de ce qui
/// le joue. Les fichiers audio ne sont pas touchés — la bibliothèque les
/// désigne, elle ne les contient pas.
#[tauri::command]
fn clear_library_and_timeline(
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineSnapshot, String> {
    suspend_timeline_audio(&library_state, &playback_state, &transport_state)?;
    let library = library_state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?;
    library.clear_everything()?;
    timeline::snapshot(&library.connection)
}

fn timeline_timing(state: &State<'_, LibraryState>) -> Result<(tempo::TempoMap, f64), String> {
    let library = state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?;
    timeline::project_timing(&library.connection)
}

fn refresh_live_timeline_after_edit(
    library_state: &State<'_, LibraryState>,
    playback_state: &State<'_, TimelinePlaybackState>,
    transport_state: &State<'_, TimelineTransportState>,
    previous_timing: (tempo::TempoMap, f64),
) -> Result<(), String> {
    let was_playing = playback_state
        .lock()
        .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?
        .transport_position(&previous_timing.0, previous_timing.1)
        .is_some_and(|(_, playing)| playing);
    if !was_playing {
        return Ok(());
    }

    let plan = {
        let library = library_state
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?;
        match timeline::render_plan(&library.connection) {
            Ok(plan) => plan,
            Err(error) => {
                // Clearing the timeline, or undoing back to an empty one, leaves
                // nothing to render. The edit itself already succeeded, so stop
                // the transport instead of reporting a failure the user cannot act on.
                let is_empty = timeline::snapshot(&library.connection)
                    .is_ok_and(|timeline| timeline.clips.is_empty());
                drop(library);
                if !is_empty {
                    return Err(error);
                }
                return stop_live_timeline(library_state, playback_state, transport_state);
            }
        }
    };
    let audio_position = playback_state
        .lock()
        .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?
        .refresh_while_playing(&plan, &previous_timing.0, previous_timing.1)?;

    if let Some(position) = audio_position {
        with_timeline_transport(transport_state, |transport| {
            transport.synchronize_audio(
                plan.tempo_map.beat_at_seconds(position.as_secs_f64()),
                true,
                plan.end_beat,
            )
        })?;
    }
    Ok(())
}

/// Releases the timeline output and parks the transport at the start.
/// Used when an edit leaves no clip left to play.
fn stop_live_timeline(
    library_state: &State<'_, LibraryState>,
    playback_state: &State<'_, TimelinePlaybackState>,
    transport_state: &State<'_, TimelineTransportState>,
) -> Result<(), String> {
    let (tempo_map, end_beat) = timeline_timing(library_state)?;
    {
        let mut playback = playback_state
            .lock()
            .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?;
        playback.pause_if_playing();
        playback.release_output();
    }
    with_timeline_transport(transport_state, |transport| {
        transport.pause(&tempo_map, end_beat)
    })?;
    Ok(())
}

fn with_timeline_transport(
    state: &State<'_, TimelineTransportState>,
    operation: impl FnOnce(&mut TimelineTransport) -> Result<TimelineTransportSnapshot, String>,
) -> Result<TimelineTransportSnapshot, String> {
    let mut transport = state
        .lock()
        .map_err(|_| "The timeline transport is in an invalid state.".to_owned())?;
    operation(&mut transport)
}

#[tauri::command]
fn timeline_transport_snapshot(
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineTransportSnapshot, String> {
    let (tempo_map, end_beat) = timeline_timing(&library_state)?;
    let meter_levels = {
        let playback = playback_state
            .lock()
            .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?;
        playback.meter_levels()
    };
    let snapshot = with_timeline_transport(&transport_state, |transport| {
        transport.snapshot(&tempo_map, end_beat)
    })?;
    Ok(snapshot.with_meter(meter_levels.0, meter_levels.1, meter_levels.2))
}

#[tauri::command]
async fn play_timeline(
    library_state: State<'_, LibraryState>,
    audio_state: State<'_, AudioState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineTransportSnapshot, String> {
    let plan = {
        let library = library_state
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?;
        timeline::render_plan(&library.connection)?
    };
    let mut position_beat = {
        let mut transport = transport_state
            .lock()
            .map_err(|_| "The timeline transport is in an invalid state.".to_owned())?;
        transport.position_beat(&plan.tempo_map, plan.end_beat)?
    };
    if position_beat >= plan.end_beat {
        position_beat = 0.0;
    }
    audio_state
        .lock()
        .map_err(|_| "The audio engine is in an invalid state.".to_owned())?
        .release_output();

    let playback = Arc::clone(playback_state.inner());
    let transport = Arc::clone(transport_state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let target = playback
            .lock()
            .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?
            .prepare_and_play(&plan, position_beat)?;
        transport
            .lock()
            .map_err(|_| "The timeline transport is in an invalid state.".to_owned())?
            .synchronize_audio(
                plan.tempo_map.beat_at_seconds(target.as_secs_f64()),
                true,
                plan.end_beat,
            )
    })
    .await
    .map_err(|error| format!("Preparing the timeline audio was interrupted: {error}"))?
}

#[tauri::command]
fn pause_timeline(
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineTransportSnapshot, String> {
    let (tempo_map, end_beat) = timeline_timing(&library_state)?;
    playback_state
        .lock()
        .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?
        .pause();
    with_timeline_transport(&transport_state, |transport| {
        transport.pause(&tempo_map, end_beat)
    })
}

#[tauri::command]
fn seek_timeline(
    position_beat: f64,
    library_state: State<'_, LibraryState>,
    playback_state: State<'_, TimelinePlaybackState>,
    transport_state: State<'_, TimelineTransportState>,
) -> Result<TimelineTransportSnapshot, String> {
    let (tempo_map, end_beat) = timeline_timing(&library_state)?;
    playback_state
        .lock()
        .map_err(|_| "The timeline audio engine is in an invalid state.".to_owned())?
        .seek_if_current(position_beat, &tempo_map, end_beat)?;
    with_timeline_transport(&transport_state, |transport| {
        transport.seek(position_beat, end_beat)
    })
}

/// Ce qu'un tap corrigé propose. L'utilisateur reste maître de l'appliquer :
/// la commande ne persiste rien, elle répond.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TempoSuggestion {
    bpm: f64,
    first_beat_ms: u64,
    confidence: f64,
}

/// Aligns the downbeat chosen by the user to the nearest beat of a refined
/// rigid grid. The grid origin may be any beat reported by the model: shifting
/// it by a whole number of beats produces the same beat lattice.
///
/// Crucially, the returned point keeps the user's bar phase. The model supplies
/// timing precision, not the musical decision of which beat should be called 1.
fn snap_manual_downbeat(requested_ms: u64, bpm: f64, analyzed_grid_origin_ms: u64) -> u64 {
    if !bpm.is_finite() || bpm <= 0.0 {
        return requested_ms;
    }

    let period_ms = 60_000.0 / bpm;
    let origin_ms = analyzed_grid_origin_ms as f64;
    let beats = ((requested_ms as f64 - origin_ms) / period_ms).round();
    let snapped_ms = origin_ms + beats * period_ms;
    if !snapped_ms.is_finite() || snapped_ms < 0.0 {
        return requested_ms;
    }

    snapped_ms.round() as u64
}

/// Recale un tempo tapé sur celui que portent réellement les kicks.
///
/// Le verrou de la bibliothèque n'est tenu que le temps de lire le chemin :
/// décoder le MP3 prend des secondes, et l'interface doit rester vivante.
#[tauri::command]
async fn refine_tapped_tempo(
    id: i64,
    tapped_bpm: f64,
    anchor_ms: u64,
    app: tauri::AppHandle,
    library_state: State<'_, LibraryState>,
) -> Result<TempoSuggestion, String> {
    if !tapped_bpm.is_finite() || tapped_bpm <= 0.0 {
        return Err(
            "Tap at least four consecutive bar ones before snapping to the beat.".to_owned(),
        );
    }

    let path = {
        let library = library_state
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?;
        library.track_path(id)?
    };
    let models = beat_analysis_resources(&app)?;

    tauri::async_runtime::spawn_blocking(move || {
        let analysis = analyze_mp3_near(std::path::Path::new(&path), tapped_bpm, &models)?;
        let first_beat_ms = snap_manual_downbeat(anchor_ms, analysis.bpm, analysis.first_beat_ms);
        Ok(TempoSuggestion {
            bpm: analysis.bpm,
            first_beat_ms,
            confidence: analysis.confidence,
        })
    })
    .await
    .map_err(|error| format!("Snapping to the kicks was interrupted: {error}"))?
}

#[tauri::command]
async fn analyze_library_tracks(
    ids: Vec<i64>,
    app: tauri::AppHandle,
    library_state: State<'_, LibraryState>,
    analysis_state: State<'_, AnalysisState>,
) -> Result<AnalysisBatchResult, String> {
    let models = beat_analysis_resources(&app)?;
    if analysis_state
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A BPM analysis is already running.".to_owned());
    }

    let library = Arc::clone(library_state.inner());
    let analysis_flag = Arc::clone(analysis_state.inner());
    let reporter = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        run_analysis_batch(&library, &ids, &models, &reporter)
    })
    .await
    .map_err(|error| format!("The BPM analysis was interrupted: {error}"));
    analysis_flag.store(false, Ordering::SeqCst);

    result?
}

#[tauri::command]
async fn backfill_library_waveforms(
    library_state: State<'_, LibraryState>,
    analysis_state: State<'_, AnalysisState>,
) -> Result<usize, String> {
    if analysis_state
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(0);
    }

    let library = Arc::clone(library_state.inner());
    let analysis_flag = Arc::clone(analysis_state.inner());
    let result = tauri::async_runtime::spawn_blocking(move || run_waveform_backfill(&library))
        .await
        .map_err(|error| format!("Preparing the waveforms was interrupted: {error}"));
    analysis_flag.store(false, Ordering::SeqCst);

    result?
}

fn run_waveform_backfill(library_state: &LibraryState) -> Result<usize, String> {
    let targets = library_state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?
        .library_waveform_targets()?;
    let mut saved_count = 0;

    for target in targets {
        let Ok(waveform) = analyze_waveform(&target.file_path) else {
            continue;
        };
        library_state
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?
            .save_waveform(target.id, &waveform)?;
        saved_count += 1;
    }

    Ok(saved_count)
}

/// Ce qui part vers l'interface dès qu'une piste est passée.
///
/// La rangée entière, pas seulement le tempo : l'interface remplace la sienne
/// par celle-ci, et une mise à jour partielle l'obligerait à savoir lesquels de
/// ses champs sont désormais périmés.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisProgress {
    track: LibraryTrack,
    /// Combien de pistes du lot sont passées, celle-ci comprise.
    done: usize,
    total: usize,
}

fn run_analysis_batch(
    library_state: &LibraryState,
    ids: &[i64],
    models: &BeatModelPaths,
    reporter: &tauri::AppHandle,
) -> Result<AnalysisBatchResult, String> {
    let targets = {
        let library = library_state
            .lock()
            .map_err(|_| "The library is in an invalid state.".to_owned())?;
        library.analysis_targets(ids)?
    };
    let total = targets.len();
    let mut analyzed_count = 0;
    let mut failed_count = 0;

    for (index, target) in targets.into_iter().enumerate() {
        {
            let mut library = library_state
                .lock()
                .map_err(|_| "The library is in an invalid state.".to_owned())?;
            library.mark_analysis_running(target.id)?;
        }

        match analyze_mp3(&target.file_path, models) {
            Ok(analysis) => {
                let mut library = library_state
                    .lock()
                    .map_err(|_| "The library is in an invalid state.".to_owned())?;
                library.save_analysis(target.id, &analysis)?;
                analyzed_count += 1;
            }
            Err(error) => {
                let library = library_state
                    .lock()
                    .map_err(|_| "The library is in an invalid state.".to_owned())?;
                library.mark_analysis_error(target.id, &error)?;
                failed_count += 1;
            }
        }

        // La piste part maintenant, pas à la fin du lot. Le tracker appris met
        // plusieurs secondes par morceau : sur un dossier entier, l'ancienne
        // version laissait l'interface figée assez longtemps pour qu'on la
        // croie plantée. L'échec compte autant que la réussite — une piste qui
        // n'a pas pu être analysée doit cesser d'afficher « Analyzing... ».
        //
        // Une émission perdue ne casse rien : le lot renvoie la liste complète
        // en terminant, qui fait autorité. C'est pourquoi l'erreur est ignorée
        // plutôt que d'interrompre une analyse en cours.
        emit_analysis_progress(library_state, reporter, target.id, index + 1, total);
    }

    let tracks = library_state
        .lock()
        .map_err(|_| "The library is in an invalid state.".to_owned())?
        .list_tracks()?;

    Ok(AnalysisBatchResult {
        tracks,
        analyzed_count,
        failed_count,
    })
}

fn emit_analysis_progress(
    library_state: &LibraryState,
    reporter: &tauri::AppHandle,
    id: i64,
    done: usize,
    total: usize,
) {
    let Ok(library) = library_state.lock() else {
        return;
    };
    let Ok(Some(track)) = library.track(id) else {
        return;
    };
    drop(library);
    let _ = reporter.emit("analysis-track", AnalysisProgress { track, done, total });
}

/// The bundle identifiers this application shipped under before it became
/// MixCanvas, newest first.
///
/// Two renames, deux dossiers de données possibles. Chercher le plus récent
/// d'abord : une installation passée par les trois porte les deux anciens
/// dossiers, et c'est celui de MixCanvas qui contient le travail à jour.
const LEGACY_IDENTIFIERS: [&str; 2] = ["ca.beatforge.app", "ca.ezdj.app"];

/// The database, plus the write-ahead log and shared-memory index that belong
/// to it. Carrying the database alone would silently drop every transaction the
/// last session had not yet checkpointed — on a working library that is most of
/// an evening's edits.
const DATABASE_FILES: [&str; 3] = [
    "library.sqlite3-wal",
    "library.sqlite3-shm",
    "library.sqlite3",
];

/// Brings a library left behind by the previous bundle identifier into the
/// current data directory.
///
/// Tauri derives the data directory from the identifier, so renaming the
/// application points it at an empty folder: an existing installation would
/// start up looking as though the whole library had been lost.
///
/// The copy runs only when the current folder holds no database of its own, so
/// it can never overwrite work, and the old folder is left untouched, so a
/// failure costs nothing. The database itself is copied last: it is what the
/// guard above tests, so an interrupted copy simply leaves the move to be
/// retried on the next launch rather than exposing a database without its log.
///
/// This can go once no installation predates the rename.
fn adopt_legacy_library(data_directory: &Path, database_path: &Path) -> io::Result<()> {
    if database_path.exists() {
        return Ok(());
    }
    let Some(target_directory) = database_path.parent() else {
        return Ok(());
    };

    // Les endroits où une bibliothèque a pu être écrite, du plus récent au plus
    // ancien : le dossier de données de la version courante — celui d'où l'on
    // vient de déménager — puis ceux des deux noms précédents du programme.
    let mut candidates = vec![data_directory.to_path_buf()];
    if let Some(parent) = data_directory.parent() {
        candidates.extend(LEGACY_IDENTIFIERS.iter().map(|id| parent.join(id)));
    }

    for legacy_directory in candidates {
        if legacy_directory == target_directory
            || !legacy_directory.join("library.sqlite3").is_file()
        {
            continue;
        }
        for name in DATABASE_FILES {
            let source = legacy_directory.join(name);
            if source.is_file() {
                fs::copy(&source, target_directory.join(name))?;
            }
        }
        // Le premier trouvé gagne : c'est le plus récent.
        return Ok(());
    }
    Ok(())
}

/// Où déballer ce que l'exécutable porte en lui.
///
/// Le même dossier que tout le reste — à côté du programme —, et non les
/// données applicatives : un portable dont trente-cinq mégaoctets de modèles
/// dorment dans un répertoire caché n'est pas portable, et personne ne sait
/// quoi effacer pour repartir à neuf.
#[cfg(feature = "embed-resources")]
fn unpacked_resources_folder(app: &tauri::AppHandle) -> Option<PathBuf> {
    let data = app.path().app_data_dir().ok()?;
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    Some(media::media_root(beside.as_deref(), &data).join("resources"))
}

/// Comment WebView2 doit peindre l'interface.
///
/// Le scintillement du zoom venait de la composition matérielle de WebView2 —
/// pas de notre mise en page. La preuve tient en deux essais : une approche
/// tout en transformations, censée être la plus douce pour le GPU, l'a
/// **aggravée**; couper le GPU l'a fait disparaître. On ne « répare » pas ça
/// depuis ici : c'est le compositeur de Chromium sur un pilote donné.
///
/// Le choix est donc **à l'exécution**, pas à la compilation. Une feature de
/// compilation obligerait à livrer deux exécutables, ou à parier pour tous les
/// utilisateurs à partir d'une seule machine. Un seul binaire qui s'ajuste
/// permet de comparer sans reconstruire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderMode {
    /// Tout en logiciel. Le défaut : correct sur n'importe quel pilote, et
    /// l'interface est du DOM en deux dimensions — le vrai travail (décodage,
    /// analyse, DSP) est en Rust et n'y touche pas.
    Software,
    /// Rastérisation matérielle, composition logicielle. L'entre-deux qui
    /// corrige le plus souvent cette famille d'artefacts sans rendre la carte
    /// inutile; à essayer avant de conclure que le GPU est perdu.
    Hybrid,
    /// Accélération complète, pour une machine dont le pilote se tient bien.
    Hardware,
}

/// Le mode demandé sur la ligne de commande.
///
/// Le dernier argument reconnu l'emporte, comme partout ailleurs : un raccourci
/// qu'on modifie en ajoutant un mot à la fin doit faire ce qu'il annonce.
fn render_mode_from_args<I, S>(arguments: I) -> RenderMode
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut mode = RenderMode::Software;
    for argument in arguments {
        match argument.as_ref() {
            "--gpu" => mode = RenderMode::Hardware,
            "--gpu-safe" => mode = RenderMode::Hybrid,
            "--no-gpu" => mode = RenderMode::Software,
            _ => {}
        }
    }
    mode
}

/// Ce que ce mode ajoute aux arguments de navigateur de WebView2.
fn browser_arguments_for(mode: RenderMode) -> &'static str {
    match mode {
        RenderMode::Software => "--disable-gpu",
        RenderMode::Hybrid => "--disable-gpu-compositing",
        RenderMode::Hardware => "",
    }
}

/// Fusionne un drapeau dans ce que l'environnement demandait déjà.
///
/// Une variable posée par l'utilisateur avant le lancement est respectée : on
/// ajoute, on ne remplace pas, et on n'ajoute pas deux fois.
fn merge_browser_arguments(existing: &str, flag: &str) -> String {
    if flag.is_empty() {
        return existing.trim().to_owned();
    }
    if existing
        .split_ascii_whitespace()
        .any(|argument| argument == flag)
    {
        return existing.trim().to_owned();
    }
    if existing.trim().is_empty() {
        flag.to_owned()
    } else {
        format!("{} {flag}", existing.trim())
    }
}

/// Applique le mode de rendu avant que le moindre WebView existe.
#[cfg(target_os = "windows")]
fn apply_render_mode(mode: RenderMode) {
    const KEY: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
    let merged = merge_browser_arguments(
        &std::env::var(KEY).unwrap_or_default(),
        browser_arguments_for(mode),
    );
    if merged.is_empty() {
        return;
    }
    // SAFETY: `run` appelle ceci avant `tauri::Builder`, donc avant que
    // MixCanvas ou WebView2 ne démarre le moindre thread.
    unsafe { std::env::set_var(KEY, merged) };
}

#[cfg(not(target_os = "windows"))]
fn apply_render_mode(_mode: RenderMode) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    apply_render_mode(render_mode_from_args(std::env::args().skip(1)));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Le ménage à la fermeture, et **seulement** celui-là : les fichiers
        // vers lesquels plus aucune ligne ne pointe. « Inutilisé dans la
        // séquence » se déciderait sur l'état courant, et une erreur de
        // jugement au moment où l'on ferme — quand personne ne regarde et
        // qu'aucune annulation n'est possible — coûterait des minutes de
        // séparation. « Non référencé » est vrai par construction.
        //
        // Un échec ne retarde pas la fermeture : au pire il reste des fichiers,
        // et le lancement suivant repassera.
        .on_window_event(|window, event| {
            if !matches!(event, tauri::WindowEvent::Destroyed) {
                return;
            }
            let app = window.app_handle();
            let (Some(library), Some(media)) = (
                app.try_state::<LibraryState>(),
                app.try_state::<MediaState>(),
            ) else {
                return;
            };
            let Ok(library) = library.lock() else { return };
            let Ok(media) = media.lock() else { return };
            let _ = media::sweep_orphans(&library.connection, &media.root);
        })
        .manage(Arc::new(Mutex::new(PreviewEngine::default())))
        .manage(Arc::new(Mutex::new(TimelineTransport::default())))
        .manage(Arc::new(Mutex::new(TimelinePlaybackEngine::default())))
        .manage(Arc::new(AtomicBool::new(false)))
        .setup(|app| {
            let data_directory = app.path().app_data_dir().map_err(|error| {
                io::Error::other(format!("The data folder cannot be reached: {error}"))
            })?;
            fs::create_dir_all(&data_directory)?;

            // **Tout** ce que le programme écrit vit dans un seul dossier, à
            // côté de l'exécutable : la base, les ressources déballées, les
            // stems et les cuissons. Un portable qui cache sa base dans les
            // données applicatives n'est pas portable — on l'emporte sans sa
            // bibliothèque, et on ne sait pas quoi effacer pour repartir à
            // neuf. Le repli sur les données applicatives reste, pour un
            // exécutable posé là où il n'a pas le droit d'écrire.
            let beside = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
            let root = media::media_root(beside.as_deref(), &data_directory);
            fs::create_dir_all(&root)?;

            let database_path = root.join("library.sqlite3");
            adopt_legacy_library(&data_directory, &database_path)?;
            let library = LibraryStore::open(&database_path).map_err(io::Error::other)?;
            app.manage(Arc::new(Mutex::new(library)));

            app.manage(Arc::new(Mutex::new(MediaLocation {
                root,
                project: media::SCRATCH_PROJECT.to_owned(),
            })));

            // L'inspecteur, seulement s'il est demandé.
            //
            // Mesurer le rendu demande de le mesurer **sur la machine qui
            // rame** : une build de debug n'a pas les mêmes coûts, et un banc
            // d'essai écrit ailleurs m'a déjà induit en erreur. Ouvert à la
            // demande plutôt que par un raccourci clavier, pour que ce soit
            // reproductible et que ça ne surprenne personne.
            if std::env::args()
                .skip(1)
                .any(|argument| argument == "--devtools")
                && let Some(window) = app.get_webview_window("main")
            {
                window.open_devtools();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_preview,
            play_preview,
            pause_preview,
            stop_preview,
            seek_preview,
            set_preview_speed,
            preview_snapshot,
            list_library_tracks,
            import_library_paths,
            remove_library_track,
            update_track_beatgrid,
            refine_tapped_tempo,
            analyze_library_tracks,
            backfill_library_waveforms,
            bounce_mix,
            save_project,
            load_project,
            timeline_snapshot,
            add_timeline_clip,
            move_timeline_clip,
            trim_timeline_clip,
            move_timeline_tempo_point,
            remove_timeline_clip,
            set_timeline_lane_muted,
            set_timeline_lane_solo,
            set_timeline_limiter_enabled,
            set_timeline_compressor_enabled,
            set_timeline_sidechain_key,
            add_timeline_volume_node,
            move_timeline_volume_node,
            delete_timeline_volume_node,
            draw_timeline_volume_shape,
            draw_timeline_pan_shape,
            add_timeline_pan_node,
            move_timeline_pan_node,
            delete_timeline_pan_node,
            delete_timeline_draw_group,
            draw_timeline_filter_bubble,
            draw_timeline_filter_stroke,
            set_timeline_clip_stem,
            separate_clip_stems,
            bake_clip,
            unbake_clip,
            clear_timeline_filter_range,
            save_clip_eq,
            split_timeline_clip,
            clear_timeline,
            clear_library_and_timeline,
            restore_timeline_snapshot,
            timeline_transport_snapshot,
            play_timeline,
            pause_timeline,
            seek_timeline
        ])
        .run(tauri::generate_context!())
        .expect("MixCanvas failed to start");
}

#[cfg(all(test, feature = "embed-resources"))]
mod embedded_tests {
    use super::unpack_embedded_resources;

    /// Le portable d'un seul fichier doit savoir se déballer.
    #[test]
    fn the_embedded_resources_land_whole_and_are_written_once() {
        let dossier = std::env::temp_dir().join(format!(
            "mixcanvas-embed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("horloge")
                .as_nanos()
        ));
        unpack_embedded_resources(&dossier).expect("le déballage devrait aboutir");

        for (relatif, attendu) in [
            ("onnxruntime.dll", super::embedded::RUNTIME.len()),
            (
                "onnxruntime_providers_shared.dll",
                super::embedded::PROVIDERS.len(),
            ),
            (
                "models/open-unmix-vocals-fp16.onnx",
                super::embedded::MODEL.len(),
            ),
            (
                "models/beat_this_small.onnx",
                super::embedded::BEAT_MODEL.len(),
            ),
            (
                "models/mel_spectrogram.onnx",
                super::embedded::MEL_MODEL.len(),
            ),
        ] {
            let taille = std::fs::metadata(dossier.join(relatif))
                .unwrap_or_else(|_| panic!("{relatif} devrait exister"))
                .len();
            assert_eq!(taille, attendu as u64, "{relatif} est tronqué");
        }

        // Deuxième passage : rien n'est réécrit, la date de la copie ne bouge
        // pas. Trente-cinq mégaoctets recopiés à chaque lancement seraient
        // payés pour rien.
        let temoin = dossier.join("onnxruntime.dll");
        let avant = std::fs::metadata(&temoin).expect("témoin").modified().ok();
        unpack_embedded_resources(&dossier).expect("le second déballage devrait aboutir");
        let apres = std::fs::metadata(&temoin).expect("témoin").modified().ok();
        assert_eq!(avant, apres, "le fichier a été réécrit sans raison");

        // Et un fichier tronqué se refait, plutôt que de faire échouer la
        // séparation sans explication.
        std::fs::write(&temoin, b"tronque").expect("écriture");
        unpack_embedded_resources(&dossier).expect("le rattrapage devrait aboutir");
        assert_eq!(
            std::fs::metadata(&temoin).expect("témoin").len(),
            super::embedded::RUNTIME.len() as u64
        );

        let _ = std::fs::remove_dir_all(&dossier);
    }
}

#[cfg(test)]
mod render_mode_tests {
    use super::{
        RenderMode, browser_arguments_for, merge_browser_arguments, render_mode_from_args,
    };

    /// Sans rien demander, on peint en logiciel : correct sur n'importe quel
    /// pilote, et c'est le seul défaut qui ne peut pas rendre l'outil
    /// inutilisable sur la machine de quelqu'un d'autre.
    #[test]
    fn software_is_what_you_get_without_asking() {
        assert_eq!(
            render_mode_from_args::<[&str; 0], &str>([]),
            RenderMode::Software
        );
        assert_eq!(
            render_mode_from_args(["--some-other-flag"]),
            RenderMode::Software
        );
        assert_eq!(browser_arguments_for(RenderMode::Software), "--disable-gpu");
    }

    #[test]
    fn each_mode_is_reachable_and_the_last_word_wins() {
        assert_eq!(render_mode_from_args(["--gpu"]), RenderMode::Hardware);
        assert_eq!(render_mode_from_args(["--gpu-safe"]), RenderMode::Hybrid);
        // Un raccourci qu'on corrige en ajoutant un mot à la fin doit faire ce
        // qu'annonce ce dernier mot.
        assert_eq!(
            render_mode_from_args(["--gpu", "--no-gpu"]),
            RenderMode::Software
        );
        assert_eq!(browser_arguments_for(RenderMode::Hardware), "");
        assert_eq!(
            browser_arguments_for(RenderMode::Hybrid),
            "--disable-gpu-compositing"
        );
    }

    /// Ce que l'utilisateur avait posé avant nous survit.
    #[test]
    fn existing_browser_arguments_are_kept_and_never_doubled() {
        assert_eq!(
            merge_browser_arguments("--lang=fr", "--disable-gpu"),
            "--lang=fr --disable-gpu"
        );
        assert_eq!(
            merge_browser_arguments("--disable-gpu", "--disable-gpu"),
            "--disable-gpu"
        );
        assert_eq!(
            merge_browser_arguments("", "--disable-gpu"),
            "--disable-gpu"
        );
        // En mode matériel on n'ajoute rien, et on n'efface pas non plus.
        assert_eq!(merge_browser_arguments("  --lang=fr  ", ""), "--lang=fr");
    }
}

#[cfg(test)]
mod tests {
    use super::{DATABASE_FILES, LEGACY_IDENTIFIERS, adopt_legacy_library, snap_manual_downbeat};
    use std::fs;

    /// Two sibling folders under a scratch root, mirroring how the identifier
    /// decides the data directory's name.
    fn data_directories(suffix: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("mixcanvas-rename-{}-{suffix}", std::process::id()));
        let current = root.join("ca.mixcanvas.app");
        // Le plus ancien des deux : si l'adoption sait le trouver, elle sait
        // aussi trouver l'autre, qui est cherché avant lui.
        let legacy = root.join(LEGACY_IDENTIFIERS[1]);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&current).expect("current directory should be created");
        fs::create_dir_all(&legacy).expect("legacy directory should be created");
        (current, legacy)
    }

    /// Deux renommages, deux dossiers possibles : c'est le plus récent qui
    /// porte le travail à jour, et c'est donc lui qu'il faut reprendre.
    #[test]
    fn the_newest_of_two_abandoned_libraries_is_the_one_adopted() {
        let root =
            std::env::temp_dir().join(format!("mixcanvas-rename-{}-both", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let current = root.join("ca.mixcanvas.app");
        fs::create_dir_all(&current).expect("current directory should be created");
        // Aucun ancien identifiant ne doit être celui d'aujourd'hui : sans cette
        // garde, un remplacement global mal ciblé rendait la liste inutile tout
        // en laissant ce test passer, les deux dossiers n'en faisant qu'un.
        assert!(
            !LEGACY_IDENTIFIERS.contains(&"ca.mixcanvas.app"),
            "un identifiant hérité ne peut pas être l'identifiant courant"
        );
        for (identifier, mark) in [
            (LEGACY_IDENTIFIERS[0], b"mixcanvas"),
            (LEGACY_IDENTIFIERS[1], b"ezdj----."),
        ] {
            let folder = root.join(identifier);
            fs::create_dir_all(&folder).expect("legacy directory should be created");
            fs::write(folder.join("library.sqlite3"), mark).expect("library should be written");
        }

        let database = current.join("library.sqlite3");
        adopt_legacy_library(&current, &database).expect("adoption should succeed");

        assert_eq!(
            fs::read(&database).expect("database should read"),
            b"mixcanvas",
            "l'installation la plus récente doit gagner"
        );

        let _ = fs::remove_dir_all(&root);
    }

    fn seed_legacy(legacy: &std::path::Path) {
        for name in DATABASE_FILES {
            fs::write(legacy.join(name), name.as_bytes()).expect("legacy file should be written");
        }
    }

    #[test]
    fn a_library_left_by_the_old_name_is_adopted_with_its_write_ahead_log() {
        let (current, legacy) = data_directories("adopt");
        seed_legacy(&legacy);

        let database = current.join("library.sqlite3");
        adopt_legacy_library(&current, &database).expect("adoption should succeed");

        for name in DATABASE_FILES {
            let carried = fs::read(current.join(name)).expect("file should have been carried over");
            assert_eq!(carried, name.as_bytes(), "{name} should arrive intact");
        }
        // Nothing is destroyed: a failed launch must not cost the only copy.
        assert!(legacy.join("library.sqlite3").is_file());

        let _ = fs::remove_dir_all(current.parent().expect("root should exist"));
    }

    #[test]
    fn an_existing_library_is_never_overwritten() {
        let (current, legacy) = data_directories("keep");
        seed_legacy(&legacy);
        let database = current.join("library.sqlite3");
        fs::write(&database, b"the library already here").expect("database should be written");

        adopt_legacy_library(&current, &database).expect("adoption should succeed");

        assert_eq!(
            fs::read(&database).expect("database should read"),
            b"the library already here",
            "an installation that already has a library must keep it"
        );

        let _ = fs::remove_dir_all(current.parent().expect("root should exist"));
    }

    #[test]
    fn a_fresh_installation_finds_nothing_to_adopt_and_says_so_quietly() {
        let (current, _legacy) = data_directories("fresh");
        let database = current.join("library.sqlite3");

        adopt_legacy_library(&current, &database).expect("adoption should succeed");

        assert!(
            !database.exists(),
            "nothing should be invented out of thin air"
        );

        let _ = fs::remove_dir_all(current.parent().expect("root should exist"));
    }

    #[test]
    fn the_database_is_carried_last_so_an_interrupted_copy_retries_cleanly() {
        // The guard tests the database, so it has to arrive after the log it
        // depends on. Were the order reversed, an interrupted copy would leave
        // a database that the next launch would open without its log.
        assert_eq!(
            DATABASE_FILES.last(),
            Some(&"library.sqlite3"),
            "the database must be the last file copied"
        );
    }

    #[test]
    fn a_manual_downbeat_snaps_to_the_nearest_beat_of_the_refined_grid() {
        assert_eq!(snap_manual_downbeat(710, 120.0, 250), 750);
        assert_eq!(snap_manual_downbeat(1_010, 120.0, 2_000), 1_000);
    }

    #[test]
    fn the_models_bar_phase_does_not_replace_the_users_bar_phase() {
        let chosen = 4_740;
        let from_first_model_beat = snap_manual_downbeat(chosen, 120.0, 250);
        let from_later_model_beat = snap_manual_downbeat(chosen, 120.0, 4_250);

        assert_eq!(from_first_model_beat, 4_750);
        assert_eq!(from_later_model_beat, 4_750);
    }

    #[test]
    fn an_impossible_snap_before_the_file_keeps_the_manual_position() {
        assert_eq!(snap_manual_downbeat(10, 128.0, 300), 10);
        assert_eq!(snap_manual_downbeat(1_234, f64::NAN, 300), 1_234);
    }
}
