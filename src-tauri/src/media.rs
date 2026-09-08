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
    collections::{HashMap, HashSet},
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

    // On raisonne par **fichier**, pas par ligne.
    //
    // Deux clips issus d'une scission ou d'une duplication désignent le même
    // fichier : la scission et le duplicata recopient délibérément le chemin
    // plutôt que le contenu. En traitant les lignes une à une, la première
    // déplaçait le fichier et la suivante trouvait sa source disparue, donc
    // repartait par `!source.is_file()` — en laissant son chemin pointer un
    // emplacement désormais vide. La première sauvegarde cassait ainsi la
    // moitié des références à un média partagé, silencieusement.
    let mut rows = Vec::new();
    for (table, column) in [("clip_stems", "file_path"), ("clip_bakes", "file_path")] {
        let mut statement = connection
            .prepare(&format!("SELECT id, {column} FROM {table}"))
            .map_err(|error| format!("Could not read the media: {error}"))?;
        let found = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Could not read the media: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Could not read the media: {error}"))?;
        drop(statement);
        rows.extend(found.into_iter().map(|(id, current)| (table, id, current)));
    }

    // Chaque source distincte n'est portée qu'une fois; sa destination sert
    // ensuite à toutes les lignes qui la désignaient.
    let mut carried: HashMap<PathBuf, String> = HashMap::new();
    let mut refused: HashSet<PathBuf> = HashSet::new();
    for (_, _, current) in &rows {
        let source = PathBuf::from(current);
        if carried.contains_key(&source) || refused.contains(&source) {
            continue;
        }
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
        let ok = match mode {
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
        if ok {
            carried.insert(source, target.to_string_lossy().into_owned());
        } else {
            refused.insert(source);
        }
    }

    let moved: Vec<(&str, i64, String)> = rows
        .into_iter()
        .filter_map(|(table, id, current)| {
            carried
                .get(&PathBuf::from(&current))
                .map(|path| (table, id, path.clone()))
        })
        .collect();

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

    // Le nombre de **fichiers** portés, et non celui des lignes réécrites :
    // un média partagé par deux clips reste un seul fichier déplacé.
    Ok(carried.len())
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

/// Efface les fichiers oubliés dans `Scratch`, et **seulement** là.
///
/// C'est un test plus fort que « inutilisé dans la séquence », et volontairement
/// : un stem coûte deux minutes de calcul, et une suppression qui se trompe au
/// moment où l'on ferme — quand personne ne regarde et qu'aucune annulation
/// n'est plus possible — les perd pour de bon. Si aucune ligne ne le désigne,
/// en revanche, rien ne pourra jamais le rouvrir. C'est vrai par construction.
///
/// **Les dossiers de projets nommés ne sont jamais touchés.** La première
/// version balayait tout ce que la base ne désignait pas, en s'appuyant sur
/// l'idée que « si aucune ligne ne le désigne, rien ne pourra jamais le
/// rouvrir ». C'est faux : un projet enregistré porte ses propres références,
/// sur le disque, hors de la base. Vider la timeline suffisait à faire
/// disparaître les lignes, et la fermeture emportait alors un fichier cuit dont
/// le projet avait besoin — il se rouvrait « cuit » sans plus rien jouer.
///
/// `Scratch` est le seul dossier dont on puisse affirmer qu'aucun projet ne le
/// réclame : c'est celui d'une session qui n'a pas de nom. Ce qui traîne dans
/// un dossier nommé y reste, et un ménage volontaire — que l'utilisateur
/// déclenche en connaissance de cause — reste à faire.
pub fn sweep_orphans(connection: &Connection, root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    // Les tables d'attente comptent parmi les références.
    //
    // Elles gardent les médias d'un clip supprimé le temps qu'on puisse annuler.
    // Les ignorer ferait effacer par ce balayage-ci le fichier que l'annulation
    // s'apprête à rendre — le clip reviendrait avec sa ligne et sans son WAV,
    // ce qui est le défaut qu'on vient de fermer, repris par l'autre bout.
    let mut referenced = std::collections::HashSet::new();
    for table in [
        "clip_stems",
        "clip_bakes",
        "removed_clip_stems",
        "removed_clip_bakes",
    ] {
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
        if project.file_name().and_then(|name| name.to_str()) != Some(SCRATCH_PROJECT) {
            continue;
        }
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
pub fn comparable(path: &Path) -> String {
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
                // Les quatre tables que la balayeuse consulte : les vivantes
                // et les deux d'attente, qui gardent les médias d'un clip
                // supprimé tant qu'on peut annuler.
                "CREATE TABLE clip_stems (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL);
                 CREATE TABLE clip_bakes (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL);
                 CREATE TABLE removed_clip_stems (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL);
                 CREATE TABLE removed_clip_bakes (id INTEGER PRIMARY KEY, file_path TEXT NOT NULL);",
            )
            .expect("tables should be created");
        connection
    }

    /// Le balayage ne prend pas ce que l'annulation s'apprête à rendre.
    ///
    /// Les tables d'attente gardent les médias d'un clip supprimé le temps qu'on
    /// puisse annuler. Les ignorer ici ferait effacer le fichier que
    /// l'annulation va réclamer — le clip reviendrait avec sa ligne et sans son
    /// WAV, ce qui est le défaut qu'on vient de fermer, repris par l'autre bout.
    #[test]
    fn the_sweep_spares_media_an_undo_could_still_want() {
        let root = scratch_root("held");
        let folder = project_media_folder(&root, SCRATCH_PROJECT, "bakes").expect("folder");
        let held = folder.join("waiting.wav");
        let orphan = folder.join("nobody.wav");
        fs::write(&held, b"audio").expect("held should be written");
        fs::write(&orphan, b"audio").expect("orphan should be written");

        let connection = memory_db();
        connection
            .execute(
                "INSERT INTO removed_clip_bakes (file_path) VALUES (?1)",
                params![held.to_string_lossy()],
            )
            .expect("the held row should be written");

        let removed = sweep_orphans(&connection, &root).expect("the sweep should run");

        assert!(held.is_file(), "un média en attente d'annulation reste");
        assert!(!orphan.is_file(), "un vrai orphelin part");
        assert_eq!(removed.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    /// Un fichier partagé par deux clips garde ses deux références.
    ///
    /// La scission et la duplication recopient le **chemin** d'un stem ou d'une
    /// cuisson, pas son contenu : deux lignes désignent alors le même fichier.
    /// En traitant les lignes une à une, la première déplaçait le fichier et la
    /// seconde repartait sans rien faire, sa source ayant disparu — elle
    /// continuait donc de pointer un emplacement vide. La première sauvegarde
    /// cassait ainsi la moitié des références, sans rien signaler.
    #[test]
    fn a_file_two_clips_share_keeps_both_references() {
        let root = scratch_root("shared");
        let folder = project_media_folder(&root, SCRATCH_PROJECT, "stems").expect("folder");
        let shared = folder.join("shared.wav");
        fs::write(&shared, b"audio").expect("the shared file should be written");

        let mut connection = memory_db();
        for _ in 0..2 {
            connection
                .execute(
                    "INSERT INTO clip_stems (file_path) VALUES (?1)",
                    params![shared.to_string_lossy()],
                )
                .expect("stem row");
        }

        let carried = relocate_project_media(&mut connection, &root, SCRATCH_PROJECT, "Saved")
            .expect("the media should move");

        // Un seul fichier porté, même si deux lignes le désignaient.
        assert_eq!(
            carried, 1,
            "le compte porte sur les fichiers, pas les lignes"
        );

        let paths: Vec<String> = connection
            .prepare("SELECT file_path FROM clip_stems ORDER BY id")
            .expect("statement")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows");
        assert_eq!(paths.len(), 2);
        for path in &paths {
            assert!(
                Path::new(path).is_file(),
                "chaque référence doit désigner un fichier qui existe : {path}"
            );
            assert!(
                path.contains("Saved"),
                "et pointer la nouvelle place : {path}"
            );
        }
        assert_eq!(paths[0], paths[1], "les deux désignent toujours le même");
        assert!(!shared.is_file(), "l'ancien emplacement est libéré");

        let _ = fs::remove_dir_all(&root);
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

    /// Le balayage efface un fichier dont un **projet enregistré** a besoin.
    ///
    /// C'est le défaut rapporté : un projet rouvert se dit cuit sans jouer son
    /// fichier. La prémisse du balayage — « si aucune ligne ne le désigne, rien
    /// ne pourra jamais le rouvrir » — est fausse dès qu'un projet sur le
    /// disque porte lui aussi des références. Vider la timeline suffit à faire
    /// disparaître les lignes, et la fermeture emporte alors le fichier.
    #[test]
    fn the_sweep_must_not_take_a_saved_project_s_media() {
        let root = scratch_root("saved");
        let folder = project_media_folder(&root, "Soirée", "bakes").expect("folder");
        let baked = folder.join("clip-1-42.wav");
        fs::write(&baked, b"audio").expect("bake should be written");

        // La session a été vidée : plus une seule ligne ne désigne ce fichier.
        // Le projet « Soirée », lui, existe toujours sur le disque et le
        // réclamera à la prochaine ouverture.
        let connection = memory_db();

        let removed = sweep_orphans(&connection, &root).expect("the sweep should run");

        assert!(
            baked.is_file(),
            "le fichier d'un projet nommé a été effacé — le projet ne le retrouvera plus"
        );
        assert!(removed.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    /// Le balayage n'efface que ce vers quoi plus rien ne pointe.
    #[test]
    fn the_sweep_only_takes_what_nothing_refers_to() {
        let root = scratch_root("sweep");
        let folder = project_media_folder(&root, SCRATCH_PROJECT, "stems").expect("folder");
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
