//! Où vivent les fichiers que le programme fabrique.
//!
//! Un stem et une cuisson sont des WAV de plusieurs dizaines de mégaoctets,
//! écrits une fois et relus à chaque lecture. Ils étaient jusqu'ici versés en
//! vrac dans le dossier de données de l'application, sans lien avec le projet
//! qui les avait demandés : rien ne disait à qui ils appartenaient, et rien ne
//! les effaçait jamais.
//!
//! Ils vivent maintenant dans un dossier par projet, à côté de l'exécutable —
//! la convention d'un programme portable, celle qui permet de copier le tout
//! sur une clé. Tant que le projet n'a pas de nom, c'est `Scratch`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, params};

/// Le dossier qui contient un sous-dossier par projet.
pub const MEDIA_ROOT_NAME: &str = "MixCanvas Files";
/// Le projet qu'on n'a pas encore enregistré.
pub const SCRATCH_PROJECT: &str = "Scratch";

/// Ce que l'enregistrement doit faire des médias du projet précédent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relocation {
    /// Le dossier de départ n'appartient à aucun projet enregistré : personne
    /// d'autre ne pointe vers ses fichiers, ils suivent.
    Move,
    /// Le dossier de départ appartient à un projet qui existe sur le disque.
    /// Ses fichiers doivent rester là où son fichier de projet les attend, donc
    /// on duplique. « Enregistrer sous » sert justement à garder une variante :
    /// déplacer casserait l'original, et c'est le geste qu'on fait pour ne
    /// surtout pas le casser.
    Copy,
    /// Même dossier des deux côtés : il n'y a rien à faire.
    None,
}

/// Ce qu'un enregistrement fera, avant de le faire.
pub fn relocation_for(from: &str, to: &str) -> Relocation {
    if from == to {
        Relocation::None
    } else if from == SCRATCH_PROJECT {
        Relocation::Move
    } else {
        Relocation::Copy
    }
}

/// Le nom de dossier qui correspond à un fichier de projet.
///
/// C'est le nom du fichier sans son extension. Il vient d'une boîte de dialogue
/// d'enregistrement, donc il est déjà valide comme nom de fichier — mais un
/// chemin sans radical lisible existe, et il ne doit pas produire un dossier
/// dont le nom n'en est pas un.
///
/// Un point en tête est refusé : pour Rust, `.mixproj` **est** un radical sans
/// extension, si bien qu'un fichier nommé de sa seule extension donnerait un
/// dossier caché portant le nom du format. Ce n'est pas un nom de projet.
pub fn project_folder_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_owned())
        .filter(|stem| !stem.is_empty() && !stem.starts_with('.'))
        .unwrap_or_else(|| SCRATCH_PROJECT.to_owned())
}

/// La racine des médias, et un mot pour dire d'où elle vient.
///
/// À côté de l'exécutable d'abord : c'est ce qui rend le portable vraiment
/// portable, puisqu'on emporte alors le programme et ses médias ensemble. Mais
/// un exécutable posé dans `Program Files`, sur un partage réseau ou sur une
/// clé protégée n'a pas le droit d'écrire à côté de lui, et l'application ne
/// doit pas mourir d'une installation qu'on n'avait pas prévue : on retombe
/// alors sur le dossier de données, qui est toujours à nous.
///
/// Le test est une **écriture réelle**, pas une lecture de permissions : sous
/// Windows, un dossier peut se déclarer accessible et refuser le fichier.
pub fn media_root(beside_executable: Option<&Path>, app_data: &Path) -> PathBuf {
    if let Some(folder) = beside_executable {
        let candidate = folder.join(MEDIA_ROOT_NAME);
        if is_writable(&candidate) {
            return candidate;
        }
    }
    app_data.join(MEDIA_ROOT_NAME)
}

fn is_writable(folder: &Path) -> bool {
    if fs::create_dir_all(folder).is_err() {
        return false;
    }
    let probe = folder.join(".write-probe");
    let written = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    written
}

/// Le dossier d'un projet, prêt à recevoir ses médias.
pub fn project_media_folder(root: &Path, project: &str, kind: &str) -> Result<PathBuf, String> {
    let folder = root.join(project).join(kind);
    fs::create_dir_all(&folder)
        .map_err(|error| format!("Could not prepare the media folder: {error}"))?;
    Ok(folder)
}

/// Emmène — ou recopie — les médias d'un projet vers un autre, et réécrit les
/// chemins que la base garde d'eux.
///
/// Les deux vont ensemble et dans cet ordre : des lignes réécrites avant que
/// les fichiers soient arrivés désigneraient des fichiers absents, et un échec
/// à mi-course laisserait la session muette. Ici un fichier qui ne se déplace
/// pas laisse simplement sa ligne inchangée — le clip continue de jouer depuis
/// l'ancien emplacement, ce qui est faux à ranger mais juste à entendre.
///
/// Renvoie le nombre de fichiers effectivement déplacés ou copiés.
pub fn relocate_project_media(
    connection: &mut Connection,
    root: &Path,
    from: &str,
    to: &str,
) -> Result<usize, String> {
    let mode = relocation_for(from, to);
    if mode == Relocation::None {
        return Ok(0);
    }

    let mut moved = Vec::new();
    for (table, column) in [("clip_stems", "file_path"), ("clip_bakes", "file_path")] {
        let mut statement = connection
            .prepare(&format!("SELECT id, {column} FROM {table}"))
            .map_err(|error| format!("Could not read the media: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Could not read the media: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Could not read the media: {error}"))?;
        drop(statement);

        for (id, current) in rows {
            let source = PathBuf::from(&current);
            let Some(target) = retargeted(&source, root, from, to) else {
                continue;
            };
            if !source.is_file() || source == target {
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("Could not prepare the media folder: {error}"))?;
            }
            let carried = match mode {
                // `rename` échoue entre deux volumes, et un dossier de projet
                // peut très bien vivre ailleurs que le dossier de données.
                Relocation::Move => {
                    fs::rename(&source, &target).is_ok()
                        || (fs::copy(&source, &target).is_ok() && {
                            let _ = fs::remove_file(&source);
                            true
                        })
                }
                Relocation::Copy => fs::copy(&source, &target).is_ok(),
                Relocation::None => false,
            };
            if carried {
                moved.push((table, id, target.to_string_lossy().into_owned()));
            }
        }
    }

    let transaction = connection
        .transaction()
        .map_err(|error| format!("Could not record the media: {error}"))?;
    for (table, id, path) in &moved {
        transaction
            .execute(
                &format!("UPDATE {table} SET file_path = ?2 WHERE id = ?1"),
                params![id, path],
            )
            .map_err(|error| format!("Could not record the media: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Could not record the media: {error}"))?;

    if mode == Relocation::Move {
        // Le dossier de départ n'a plus de raison d'être. Il peut rester des
        // fichiers dont aucune ligne ne parlait : `remove_dir` refusera, et
        // c'est bien — le balayage des orphelins s'en occupera en connaissance
        // de cause plutôt qu'en aveugle ici.
        let _ = fs::remove_dir(root.join(from).join("stems"));
        let _ = fs::remove_dir(root.join(from).join("bakes"));
        let _ = fs::remove_dir(root.join(from));
    }

    Ok(moved.len())
}

/// Le chemin qu'un fichier prendrait dans l'autre projet.
///
/// `None` quand il ne vit pas sous le dossier de départ : un média posé
/// ailleurs — dossier de données d'une version antérieure, chemin choisi à la
/// main — appartient à qui l'a mis là, et le déménagement ne le revendique pas.
fn retargeted(source: &Path, root: &Path, from: &str, to: &str) -> Option<PathBuf> {
    let relative = source.strip_prefix(root.join(from)).ok()?;
    Some(root.join(to).join(relative))
}

/// Efface les fichiers vers lesquels **plus aucune ligne ne pointe**.
///
/// C'est un test plus fort que « inutilisé dans la séquence », et volontairement
/// : un stem coûte deux minutes de calcul, et une suppression qui se trompe au
/// moment où l'on ferme — quand personne ne regarde et qu'aucune annulation
/// n'est plus possible — les perd pour de bon. Si aucune ligne ne le désigne,
/// en revanche, rien ne pourra jamais le rouvrir. C'est vrai par construction.
///
/// Ne descend que dans la racine des médias : un fichier hors d'elle n'a pas
/// été écrit par le programme, et n'est pas à lui.
pub fn sweep_orphans(connection: &Connection, root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut referenced = std::collections::HashSet::new();
    for table in ["clip_stems", "clip_bakes"] {
        let mut statement = connection
            .prepare(&format!("SELECT file_path FROM {table}"))
            .map_err(|error| format!("Could not read the media: {error}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("Could not read the media: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Could not read the media: {error}"))?;
        for path in rows {
            referenced.insert(comparable(Path::new(&path)));
        }
    }

    let mut removed = Vec::new();
    for project in read_dir(root) {
        for kind in read_dir(&project) {
            for file in read_dir(&kind) {
                if !file.is_file() || referenced.contains(&comparable(&file)) {
                    continue;
                }
                if fs::remove_file(&file).is_ok() {
                    removed.push(file);
                }
            }
        }
    }
    Ok(removed)
}

fn read_dir(folder: &Path) -> Vec<PathBuf> {
    fs::read_dir(folder)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

/// De quoi comparer deux chemins qui désignent le même fichier.
///
/// Windows ne distingue pas la casse, et les séparateurs se mélangent dès qu'un
/// chemin a transité par du JSON. Comparer les chaînes brutes ferait passer
/// pour orphelin un fichier bel et bien référencé — donc l'effacerait.
fn comparable(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unnamed_project_keeps_its_scratch_folder() {
        assert_eq!(
            project_folder_name(Path::new("C:/mix/Soirée.mixproj")),
            "Soirée"
        );
        assert_eq!(project_folder_name(Path::new("C:/mix/.mixproj")), "Scratch");
        assert_eq!(project_folder_name(Path::new("C:/")), "Scratch");
    }

    /// « Enregistrer sous » depuis un projet nommé ne doit pas casser l'original.
    #[test]
    fn saving_moves_from_scratch_but_copies_from_a_named_project() {
        assert_eq!(relocation_for(SCRATCH_PROJECT, "Soirée"), Relocation::Move);
        assert_eq!(relocation_for("Soirée", "Soirée v2"), Relocation::Copy);
        assert_eq!(relocation_for("Soirée", "Soirée"), Relocation::None);
    }

    #[test]
    fn a_media_file_outside_the_project_folder_is_left_alone() {
        let root = Path::new("C:/app/MixCanvas Files");
        // Sous le dossier de départ : il suit.
        assert_eq!(
            retargeted(
                Path::new("C:/app/MixCanvas Files/Scratch/stems/a.wav"),
                root,
                "Scratch",
                "Soirée"
            ),
            Some(PathBuf::from("C:/app/MixCanvas Files/Soirée/stems/a.wav"))
        );
        // Ailleurs : il appartient à qui l'a mis là.
        assert_eq!(
            retargeted(Path::new("D:/ailleurs/a.wav"), root, "Scratch", "Soirée"),
            None
        );
    }

    fn scratch_root(label: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mixcanvas-media-{}-{label}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("root should be created");
        root
    }

    fn memory_db() -> Connection {
        let connection = Connection::open_in_memory().expect("database should open");
        connection
            .execute_batch(
                "CREATE TABLE clip_stems (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL);
                 CREATE TABLE clip_bakes (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL);",
            )
            .expect("tables should be created");
        connection
    }

    /// Le premier enregistrement emmène les médias, et la base suit.
    #[test]
    fn the_first_save_carries_the_media_and_rewrites_the_paths() {
        let root = scratch_root("move");
        let from = project_media_folder(&root, SCRATCH_PROJECT, "stems").expect("folder");
        let stem = from.join("clip-1 [vocals].wav");
        fs::write(&stem, b"audio").expect("stem should be written");
        let baked_folder = project_media_folder(&root, SCRATCH_PROJECT, "bakes").expect("folder");
        let bake = baked_folder.join("clip-1-42.wav");
        fs::write(&bake, b"audio").expect("bake should be written");

        let mut connection = memory_db();
        connection
            .execute(
                "INSERT INTO clip_stems (file_path) VALUES (?1)",
                params![stem.to_string_lossy()],
            )
            .expect("stem row");
        connection
            .execute(
                "INSERT INTO clip_bakes (file_path) VALUES (?1)",
                params![bake.to_string_lossy()],
            )
            .expect("bake row");

        let carried = relocate_project_media(&mut connection, &root, SCRATCH_PROJECT, "Soirée")
            .expect("the media should move");

        assert_eq!(carried, 2);
        assert!(!stem.is_file(), "l'original est parti");
        assert!(
            root.join("Soirée/stems/clip-1 [vocals].wav").is_file(),
            "et il est arrivé"
        );
        assert!(root.join("Soirée/bakes/clip-1-42.wav").is_file());
        let stored: String = connection
            .query_row("SELECT file_path FROM clip_stems", [], |row| row.get(0))
            .expect("path should read");
        assert!(
            stored.contains("Soirée"),
            "la base désigne l'arrivée, pas le départ : {stored}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// « Enregistrer sous » depuis un projet nommé duplique : l'original doit
    /// continuer de trouver ses fichiers là où son projet les attend.
    #[test]
    fn saving_under_a_new_name_leaves_the_first_project_playable() {
        let root = scratch_root("copy");
        let from = project_media_folder(&root, "Soirée", "stems").expect("folder");
        let stem = from.join("clip-1 [vocals].wav");
        fs::write(&stem, b"audio").expect("stem should be written");

        let mut connection = memory_db();
        connection
            .execute(
                "INSERT INTO clip_stems (file_path) VALUES (?1)",
                params![stem.to_string_lossy()],
            )
            .expect("stem row");

        relocate_project_media(&mut connection, &root, "Soirée", "Soirée v2")
            .expect("the media should copy");

        assert!(stem.is_file(), "l'original reste : son projet en dépend");
        assert!(root.join("Soirée v2/stems/clip-1 [vocals].wav").is_file());

        let _ = fs::remove_dir_all(&root);
    }

    /// Le balayage n'efface que ce vers quoi plus rien ne pointe.
    #[test]
    fn the_sweep_only_takes_what_nothing_refers_to() {
        let root = scratch_root("sweep");
        let folder = project_media_folder(&root, "Soirée", "stems").expect("folder");
        let kept = folder.join("used.wav");
        let orphan = folder.join("forgotten.wav");
        fs::write(&kept, b"audio").expect("kept should be written");
        fs::write(&orphan, b"audio").expect("orphan should be written");

        let connection = memory_db();
        connection
            .execute(
                // La casse et les séparateurs diffèrent volontairement : un
                // chemin passé par du JSON revient rarement tel quel, et le
                // comparer brut ferait effacer un fichier bel et bien utilisé.
                "INSERT INTO clip_stems (file_path) VALUES (?1)",
                params![kept.to_string_lossy().to_uppercase().replace('/', "\\")],
            )
            .expect("stem row");

        let removed = sweep_orphans(&connection, &root).expect("the sweep should run");

        assert!(kept.is_file(), "un fichier référencé ne bouge pas");
        assert!(!orphan.is_file(), "un orphelin part");
        assert_eq!(removed.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn paths_that_differ_only_in_case_or_separator_are_the_same_file() {
        // Sinon un fichier référencé passerait pour orphelin, et le balayage
        // l'effacerait.
        assert_eq!(
            comparable(Path::new(r"C:\App\Files\Scratch\stems\A.wav")),
            comparable(Path::new("c:/app/files/scratch/stems/a.wav"))
        );
    }
}
