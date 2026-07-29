# Handoff — 2026-07-28

Ce document décrit l'état réel du dépôt après un nouvel audit complet du projet
renommé MixCanvas, puis le remplacement de l'analyse BPM/downbeat principale.
Les chiffres ci-dessous proviennent d'une exécution vérifiée, pas d'un décompte
estimé dans le code.

## Vérifications

`.\check.cmd` enchaîne `tsc --noEmit`, `vitest run`, `cargo test`,
`cargo fmt --check` et `cargo clippy --all-targets -- -D warnings`.

| Étape | Résultat au 2026-07-28 |
|---|---|
| `tsc --noEmit` | passe |
| `vitest run` | 179 tests, 23 fichiers |
| `cargo test` | 142 tests réussis, 4 ignorés explicitement |
| `cargo fmt --check` | propre |
| `cargo clippy -D warnings` | propre |
| Sortie du script | code 0 |
| `pnpm build` | production frontend Vite construite |

Toute affirmation de ce fichier qui n'est pas vérifiable par `check.cmd` doit
être relue avec méfiance. C'est la leçon du handoff précédent.

## Repères pour s'orienter

- Le programme s'est appelé EZ-DJ, puis BeatForge, et s'appelle **MixCanvas**
  depuis cette session.
  L'identifiant de paquet est passé de `ca.ezdj.app` à `ca.mixcanvas.app`, ce
  dont Tauri déduit le dossier de données; `adopt_legacy_library` dans
  `src-tauri/src/lib.rs` reprend au premier lancement une base laissée sous
  l'ancien identifiant. Ce code est temporaire et pourra disparaître quand plus
  aucune installation ne précédera le renommage.
- Le schéma SQLite est en version **24**. `LATEST_SCHEMA_VERSION` et
  `CURRENT_DATABASE_SCHEMA` dans `src-tauri/src/library.rs` doivent toujours
  s'accorder; des tests parcourent la chaîne de migrations ancienne.
- Le fichier `.mixcanvas` porte le format `mixcanvas-project`, version **1**.
- **50 commandes Tauri** sont enregistrées dans `generate_handler!`.
- L'interface est **unilingue anglaise**. Aucune chaîne affichée ne doit être en
  français. Les commentaires du code et les trois documents `.md` sont en
  français, délibérément.

## Où vit quoi

- La logique pure du frontend est extraite dans `src/lib/`, chacune avec son
  fichier de test à côté. C'est là que doit aller toute règle susceptible
  d'exister en double avec le backend.
- `src-tauri/src/timeline.rs` détient la géométrie des clips : ancre, rognage,
  chevauchement, choix de piste. `src-tauri/src/audio/timeline.rs` détient le
  moteur temps réel et la chaîne master.
- `architecture.md` explique le *pourquoi* de chaque décision, `changelog.md`
  consigne les changements au jour le jour. Les deux sont tenus à jour dans la
  même passe que le code, pour qu'une reprise de contexte ne perde rien.

## Le défaut qui revient

Quatre pannes distinctes de ce projet ont eu la même cause : **une règle écrite
à deux endroits**, qui finissent par diverger.

1. Deux cartes de tempo — `project_timing` lisait `anchor_beat`, `render_plan`
   lisait `tempo_anchor_beat`. Le Seek cessait de fonctionner en silence.
2. La courbe des nœuds de volume — le rendu et le glissé employaient deux
   géométries; attraper un nœud le projetait à −∞.
3. La rotation des pistes — une version en TypeScript, une autre en Rust qui ne
   tournait pas du tout. Un dépôt automatique se refusait lui-même.
4. La géométrie du trim — la boîte du clip suivait le brouillon, la forme d'onde
   le rognage validé. Le résultat ressemblait à un time-stretch.

Avant d'ajouter un calcul, chercher s'il existe déjà de l'autre côté de l'IPC.

## Ce qui reste ouvert

- **La beatgrid automatique est maintenant l'algorithme 3, mais le premier
  downbeat reste à écouter.** Beat This! fournit les événements, puis MixCanvas
  ajuste une grille rigide robuste et choisit la phase 4/4. Sur les quatre MP3
  de diagnostic, le code de production retourne 120,000, 125,994, 116,417 et
  125,999 BPM, cohérents avec les taps approximatifs de l'auteur. Les premiers
  downbeats proposés sont 14,020 s pour Bicep, 2,044 s pour *Self Control*,
  30,952 s pour *Hälo* et 30,499 s pour *A Bit of Nostalgia*. Bicep était
  auparavant ancré à 12 s : ne pas déclarer l'un des deux correct sans écoute.
  Le bouton `Snap to beat` et la capture depuis la Preview sont le chemin de
  correction prévu.

- **Les corrections manuelles survivent volontairement à la réanalyse.** Les
  quatre pistes de la base actuelle en portent plusieurs, dont 160 et
  139,03 BPM, alors que les taps de cette session donnent environ 120 et 127.
  La version 3 mettra bien à jour `analyzed_bpm`, mais l'interface continuera
  d'utiliser la valeur manuelle tant que `Restore Automatic` n'aura pas été
  choisi. Ne pas effacer ces données en migration : dans une vraie bibliothèque
  elles représentent du travail de l'utilisateur.

- **L'analyse apprise est plus lente que l'ancien calcul.** En release sur les
  quatre pistes de diagnostic, la passe complète prend environ 9,7 à 33,4 s
  selon la durée. Elle reste sur le thread bloquant existant et ne fige pas
  l'interface. Le modèle complet de 83 Mo a été rejeté parce qu'il donnait les
  mêmes BPM en demandant encore plus de temps. Toute optimisation future doit
  conserver les résultats du corpus avant de gagner des secondes.

- **La concentration de responsabilités devient le risque dominant.**
  `TimelinePanel.tsx` approche 2 700 lignes, `timeline.rs` et
  `audio/timeline.rs` dépassent chacun 4 000 lignes, et `app.css` dépasse
  5 000 lignes. Ce n'est pas une invitation à une grande réécriture : chaque
  extraction devra suivre une frontière testable et garder une seule autorité
  pour la règle concernée. Pour une correction ciblée, chercher d'abord la
  règle existante et ses doublons éventuels.

- **Le time-stretch reste un WSOLA maison, corrigé et mesuré.** L'énergie
  parasite d'une nappe étirée de sept battements est passée de 58 % à 2 %, et la
  hauteur ne suit plus le tempo. Deux tests ignorés servent d'instrument :
  `pads_survive_a_small_tempo_change` et
  `transients_are_neither_doubled_nor_dropped`. Toute modification de la
  granulation se juge dessus en trois secondes.

  Si 2 % s'entend encore sur du matériel réel, la suite est un vocodeur de
  phase — `rustfft` est déjà là pour les stems — ou Rubber Band, dont la licence
  GPL v2 ou ultérieure est compatible avec l'AGPL de ce projet mais qui ajoute
  une chaîne C++ à la compilation.

- **Renommer le dossier casse `node_modules`.** pnpm y range des liens vers son
  magasin local, et ces liens portent l'ancien chemin : `tsc` devient
  introuvable et `check` échoue avant même de compiler. `pnpm install` refuse
  ensuite de nettoyer tout seul faute de terminal interactif. Le remède est de
  supprimer `node_modules` à la main puis de réinstaller.

- **`check` échoue tant que l'application tourne.** Le script de build de Tauri
  copie `resources/onnxruntime.dll` à côté de l'exécutable, et Windows refuse
  d'écrire sur une bibliothèque chargée : « The process cannot access the file
  because it is being used by another process (os error 32) », suivi d'un échec
  de Clippy qui n'a rien à voir avec le code. Fermer MixCanvas avant de lancer
  `check`.

- **Une interpolation de plus existe des deux côtés** : `panValueAtBeat`
  (`src/lib/volumeCurve.ts`) refait ce que fait `interpolated_pan`
  (`src-tauri/src/timeline.rs`), parce que le crayon a besoin du panoramique en
  place *avant* d'écrire, donc sans aller-retour IPC pendant le glissé. Les deux
  suivent la même règle — interpolation linéaire entre voisins, centre sans
  nœud — mais rien ne l'impose. Voir la section précédente : c'est exactement la
  forme que prend le défaut qui revient.

- **Version des manifestes** : `package.json`, `Cargo.toml` et `tauri.conf.json`
  portent `0.0.17`, alors que `architecture.md` documente des jalons jusqu'à
  0.0.19 et que le travail décrit ci-dessus va bien au-delà. À trancher avec
  l'auteur du projet, qui prévoit son premier commit Git en version 1.0.
- **Aucun commit Git** n'existe encore dans le dépôt. C'est un choix assumé, mais
  il prive un audit de `git log` et de `git diff` : l'auditeur voit une photo,
  sans pouvoir distinguer l'ancien du récent.
- **Réglages à faire à l'oreille**, non vérifiables par un test : profondeur et
  retombée du sidechain (`DUCK_DEPTH_DB`, `DUCK_RELEASE_BEATS`), seuil de
  détection du kick (`DUCK_TRANSIENT_RATIO`), dosage de la saturation
  (`COLOUR_SATURATION_MIX`), seuil de détection du premier temps
  (`GROOVE_LEVEL_RATIO`).
- **Rendu visuel non vérifié à l'écran** pour les trois dernières refontes —
  touches de transport, VU-mètre, transparence des clips. Le raisonnement et les
  rapports de contraste ont été calculés, mais l'outil de capture d'écran ne
  fonctionnait pas en fin de session.
