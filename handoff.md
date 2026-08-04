# Handoff — 2026-08-04

Ce document décrit l'état réel du dépôt à la fin d'une semaine consacrée pour
l'essentiel aux performances de l'interface. Les chiffres ci-dessous proviennent
d'une exécution vérifiée, pas d'un décompte estimé dans le code.

## Vérifications

`.\check.cmd` enchaîne `tsc --noEmit`, `vitest run`, `cargo test`,
`cargo fmt --check` et `cargo clippy --all-targets -- -D warnings`.

| Étape | Résultat au 2026-08-04 |
|---|---|
| `tsc --noEmit` | passe |
| `vitest run` | 230 tests, 27 fichiers |
| `cargo test` | 173 tests réussis, 4 ignorés explicitement |
| `cargo fmt --check` | propre |
| `cargo clippy -D warnings` | propre |
| Sortie du script | code 0 |
| `pnpm build` | production frontend Vite construite |

**Une porte rouge cache les étages suivants.** L'état précédent de ce document
annonçait un seul échec, le formatage, et notait Clippy comme « non exécuté ».
`cargo fmt` échouait en réalité à quinze endroits dans quatre fichiers, et
Clippy — lancé à la main, puisque `check.cmd` s'arrête avant lui — rejetait deux
commandes Tauri passées à neuf arguments. Tant que la porte n'est pas verte, on
ne sait pas ce qu'elle cache : le seul état exploitable est le code 0.

Toute affirmation de ce fichier qui n'est pas vérifiable par `check.cmd` doit
être relue avec méfiance. C'est la leçon du handoff précédent.

**`check` échoue tant que l'application tourne.** Le script de build de Tauri
copie `resources/onnxruntime.dll` à côté de l'exécutable, et Windows refuse
d'écrire sur une bibliothèque chargée : « The process cannot access the file
because it is being used by another process (os error 32) », suivi d'un échec de
Clippy qui n'a rien à voir avec le code. Fermer MixCanvas avant de lancer
`check`.

## Repères pour s'orienter

- Le programme s'est appelé EZ-DJ, puis BeatForge, et s'appelle **MixCanvas**.
  L'identifiant de paquet est `ca.mixcanvas.app`; `adopt_legacy_library` dans
  `src-tauri/src/lib.rs` reprend au premier lancement une base laissée sous un
  ancien identifiant ou dans l'ancien emplacement. Ce code est temporaire.
- Le schéma SQLite est en version **27**. `LATEST_SCHEMA_VERSION` et
  `CURRENT_DATABASE_SCHEMA` dans `src-tauri/src/library.rs` doivent toujours
  s'accorder; des tests parcourent la chaîne de migrations ancienne.
- Le fichier `.mixcanvas` porte le format `mixcanvas-project`, version **1**.
- **54 commandes Tauri** sont enregistrées dans `generate_handler!`.
- L'interface est **unilingue anglaise**. Aucune chaîne affichée ne doit être en
  français. Les commentaires du code et les documents `.md` sont en français,
  délibérément.
- Le dépôt a maintenant un historique Git — cinq commits, le dernier étant
  `c8c48bf day3`. L'auteur fait ses commits lui-même.

## Où vit quoi

- La logique pure du frontend est extraite dans `src/lib/`, chacune avec son
  fichier de test à côté. C'est là que doit aller toute règle susceptible
  d'exister en double avec le backend.
- `src-tauri/src/timeline.rs` détient la géométrie des clips : ancre, rognage,
  chevauchement, choix de piste. `src-tauri/src/audio/timeline.rs` détient le
  moteur temps réel et la chaîne master.
- **Tout ce que le programme écrit vit dans `MixCanvas Files`, à côté de
  l'exécutable** : la base, les ressources déballées, les stems et les cuissons.
  Un projet a son sous-dossier; une session non enregistrée vit dans `Scratch`.
  Le repli sur les données applicatives ne sert qu'à un exécutable posé là où il
  n'a pas le droit d'écrire.
- `architecture.md` explique le *pourquoi* de chaque décision, `changelog.md`
  consigne les changements au jour le jour. Les deux sont tenus à jour dans la
  même passe que le code, pour qu'une reprise de contexte ne perde rien.

## Le défaut qui revient

Six pannes distinctes de ce projet ont eu la même cause : **une règle écrite à
deux endroits**, qui finissent par diverger.

1. Deux cartes de tempo — `project_timing` lisait `anchor_beat`, `render_plan`
   lisait `tempo_anchor_beat`. Le Seek cessait de fonctionner en silence.
2. La courbe des nœuds de volume — le rendu et le glissé employaient deux
   géométries; attraper un nœud le projetait à −∞.
3. La rotation des pistes — une version en TypeScript, une autre en Rust qui ne
   tournait pas du tout. Un dépôt automatique se refusait lui-même.
4. La géométrie du trim — la boîte du clip suivait le brouillon, la forme d'onde
   le rognage validé. Le résultat ressemblait à un time-stretch.
5. Le budget de nœuds d'une forme — le frontend bornait la forme *dessinée* à
   2 048 puis ajoutait trois nœuds de service; le serveur refusait au-delà de
   2 048. Les deux côtés nomment maintenant `SHAPE_EDGE_NODES`.
6. `panValueAtBeat` (`src/lib/volumeCurve.ts`) refait ce que fait
   `interpolated_pan` (`src-tauri/src/timeline.rs`) — le crayon a besoin du
   panoramique en place *avant* d'écrire, donc sans aller-retour IPC pendant le
   glissé. Les deux suivent la même règle, mais rien ne l'impose. **Encore
   ouvert.**

Avant d'ajouter un calcul, chercher s'il existe déjà de l'autre côté de l'IPC.

## Le second défaut qui revient

La semaine du 31 juillet au 2 août a coûté trois jours de mesures à traquer
**le même défaut sous quatre formes** : un travail par image qu'on ne voyait
pas, parce qu'il ne ressemblait pas à du travail.

1. `--timeline-follow-offset`, une propriété personnalisée sur l'ancêtre de
   toute la timeline, changée vingt fois par seconde. **36 % du fil principal.**
2. `--measure-width`, la même chose sur chaque voie, changée à chaque zoom.
3. `--anchor-offset`, la même chose sur chaque clip, donc sur l'ancêtre de sa
   waveform.
4. Une lecture de `scrollLeft` dans un `useLayoutEffect` sans dépendances, qui
   forçait un recalcul synchrone de style et de mise en page à chaque rendu.
   **188 ms par seconde.**

Les trois premières partagent un mécanisme : **changer une propriété
personnalisée invalide le style calculé de tout le sous-arbre.** Une propriété
ordinaire — `transform`, `background-size`, `left` — n'invalide que son élément.

La leçon de méthode compte davantage que les correctifs : les avoir cherchées
une à une a coûté trois journées, là où deux `grep` exhaustifs auraient tout
donné du premier coup. Ces deux recherches sont désormais les bons réflexes :

```
grep -rn '"--[a-z-]*":' src/            # propriétés personnalisées écrites en JS
grep -rn "getBoundingClientRect\|offsetWidth\|clientWidth\|scrollLeft" src/
```

Au 2026-08-04 la première ne rend rien, et la seconde ne rend que des lectures
situées dans des gestionnaires de pointeur — une par geste, ce qui est sain.

## Ce qui reste ouvert

- **La passe d'optimisation d'interface est terminée pour le rendu logiciel.**
  Les traces comparables du 3 août retiennent `contain: layout paint` sur
  `.timeline-scroll` : Paint passe de 1 090 appels / 828 ms à 434 / 449 ms sur
  environ 19 s, et les pics de peinture plein document passent de 9,16 à
  4,88 ms. Le travail résiduel est la rasterisation de la couche de scroll
  native de Chromium; il ne vient plus de React, des waveforms, du VU ou de la
  mise en page de l'application. L'essai `will-change: scroll-position` ne
  donne aucun gain (3,09 contre 3,07 s/s de RasterTask parallèle) et a été
  retiré. Ne pas réduire la fréquence de scroll ou le détail visuel sans une
  demande explicite; le prochain saut exigerait GPU stable ou réécriture Canvas.

- **Le protocole de mesure reste à conserver pour les futurs changements.** Ne
  comparer que deux enregistrements de gestes semblables et raisonner en ms/s,
  pas sur des totaux de traces de durées différentes. L'attribution du
  profileur à une position dans le paquet vaut mieux que n'importe quel total.

- **Un build de diagnostic non minifié est le seul moyen de nommer le script.**
  `(anonymous)` à 38 % ne dit rien. La recette :
  `npx vite build --minify false --sourcemap`, puis `npx tauri build --no-bundle
  -f embed-resources --config src-tauri/tauri.diagnostic.conf.json`, faute de quoi
  `beforeBuildCommand` refait un frontend minifié par-dessus. Ce fichier de quatre
  lignes est versionné pour cette seule raison : sans lui la recette ci-dessus
  n'est pas exécutable, et on la redécouvre à chaque fois.

- **Le build portable exige `npx tauri build --no-bundle -f embed-resources`.**
  Un `cargo build --release` produit un binaire qui n'embarque ni le frontend —
  il va chercher le serveur de développement et affiche « can't reach this
  page » — ni les modèles. La taille le dit : 18 Mo au lieu de 62,5.

- **Le premier downbeat de la beatgrid automatique reste à écouter.** Beat This!
  fournit les événements, puis MixCanvas ajuste une grille rigide et choisit la
  phase 4/4. Sur les quatre MP3 de diagnostic, le code de production retourne
  120,000, 125,994, 116,417 et 125,999 BPM. Les premiers downbeats proposés sont
  14,020 s pour Bicep, 2,044 s pour *Self Control*, 30,952 s pour *Hälo* et
  30,499 s pour *A Bit of Nostalgia*. Bicep était auparavant ancré à 12 s : ne
  pas déclarer l'un des deux correct sans écoute.

- **Les corrections manuelles survivent volontairement à la réanalyse.**
  L'interface continue d'utiliser la valeur manuelle tant que `Restore
  Automatic` n'a pas été choisi. Ne pas effacer ces données en migration : dans
  une vraie bibliothèque elles représentent du travail de l'utilisateur.
  Enregistrer une valeur égale à la valeur analysée efface la correction plutôt
  que de la répéter.

- **L'analyse apprise est plus lente que l'ancien calcul.** En release sur les
  quatre pistes de diagnostic, la passe complète prend environ 9,7 à 33,4 s
  selon la durée. Le modèle complet de 83 Mo a été rejeté parce qu'il donnait les
  mêmes BPM en demandant encore plus de temps. Toute optimisation future doit
  conserver les résultats du corpus avant de gagner des secondes.

- **La concentration de responsabilités devient le risque dominant.**
  `TimelinePanel.tsx` dépasse 3 200 lignes, `timeline.rs` 4 800,
  `audio/timeline.rs` 4 100 et `app.css` 5 600. Ce n'est pas une invitation à une
  grande réécriture : chaque extraction devra suivre une frontière testable et
  garder une seule autorité pour la règle concernée.

- **Le time-stretch reste un WSOLA maison, corrigé deux fois et mesuré.**
  L'énergie parasite d'une nappe étirée est passée de 58 % à 2 %. Une seconde
  correction a rendu le rayon de recherche toujours maximal, ce qui a multiplié
  le coût par dix-huit à vingt-deux et faisait craquer deux clips superposés; la
  recherche est devenue hiérarchique, huit fois moins chère, à qualité
  identique. Trois tests servent d'instrument :
  `pads_survive_a_small_tempo_change`, `transients_are_neither_doubled_nor_dropped`
  et `one_grain_of_search_stays_within_its_budget`. Toute modification de la
  granulation se juge dessus, y compris sur son **coût**.

  Si 2 % s'entend encore sur du matériel réel, la suite est un vocodeur de
  phase — `rustfft` est déjà là pour les stems — ou Rubber Band, dont la licence
  GPL v2 ou ultérieure est compatible avec l'AGPL de ce projet mais qui ajoute
  une chaîne C++ à la compilation.

- **Renommer le dossier casse `node_modules`.** pnpm y range des liens vers son
  magasin local, et ces liens portent l'ancien chemin : `tsc` devient
  introuvable et `check` échoue avant même de compiler. `pnpm install` refuse
  ensuite de nettoyer tout seul faute de terminal interactif. Le remède est de
  supprimer `node_modules` à la main puis de réinstaller.

- **Version des manifestes** : `package.json`, `Cargo.toml` et `tauri.conf.json`
  portent `0.0.17`, alors que le travail décrit va bien au-delà. À trancher avec
  l'auteur du projet, qui prévoit sa version 1.0.

- **Réglages à faire à l'oreille**, non vérifiables par un test : profondeur et
  retombée du sidechain (`DUCK_DEPTH_DB`, `DUCK_RELEASE_BEATS`), seuil de
  détection du kick (`DUCK_TRANSIENT_RATIO`), dosage de la saturation
  (`COLOUR_SATURATION_MIX`), seuil de détection du premier temps
  (`GROOVE_LEVEL_RATIO`).

- **Le découpage des courbes d'automation n'a pas été vu à l'écran.** Les voies
  d'automation n'existent qu'avec un projet chargé, ce qui demande Tauri : le
  serveur de développement n'a pas de clips. La logique est couverte par des
  tests, mais une bulle de filtre à cheval sur le bord de l'écran reste à
  regarder.
