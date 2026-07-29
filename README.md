# MixCanvas

MixCanvas est un éditeur de mix DJ desktop, gratuit et open source, centré sur une timeline musicale simple. Le projet fournit maintenant une chaîne de travail complète allant de la bibliothèque et de l'analyse BPM jusqu'à l'édition multipiste, la sauvegarde de projet, le bounce et la séparation voix/instrumental, avec un moteur audio Rust en float32.

## État actuel

Les manifestes portent `0.0.17`, mais le développement a dépassé cette numérotation : voir « État d'implémentation » dans `architecture.md`. Fonctions disponibles :

- application desktop Tauri 2;
- interface React et TypeScript;
- ajout de plusieurs MP3 ou exploration récursive d'un dossier;
- bibliothèque SQLite persistante sans serveur externe;
- détection des fichiers déplacés ou manquants;
- retrait non destructif d'une entrée de la bibliothèque;
- analyse BPM automatique en arrière-plan dès l'importation, guidée par un modèle
  beat/downbeat puis ajustée à une grille DJ rigide sur le morceau complet;
- optimisation robuste de la période, de la phase, de la beatgrid uniforme et
  du premier temps des mesures 4/4, sans dérive après un beat manquant;
- indice de confiance et état d'analyse par morceau;
- correction manuelle du BPM avec commandes ×2 et ÷2;
- Tap Tempo stabilisé par la médiane des frappes;
- commande Snap to beat qui transforme le tap approximatif en grille exacte;
- définition manuelle du premier temps depuis la position de la Preview;
- restauration de l'analyse automatique originale;
- trois pistes stéréo sur une timeline zoomable en beats et en mesures;
- glisser-déposer depuis la bibliothèque vers la piste choisie, avec surbrillance de la cible;
- ajout par clic sur les pistes A, B et C en rotation, en retenant la première réellement libre au playhead;
- déplacement de clips avec snapping du premier temps sur les mesures de quatre beats;
- déplacement vertical ou horizontal des clips, y compris pendant Play, avec sauvegarde et actualisation audio immédiates;
- colonne fixe de boutons Mute et Solo persistants, indépendante du zoom et utilisable pendant la lecture;
- conservation visuelle du pré-roll avant le premier beat;
- carte de tempo globale : BPM de départ modifiable par saisie ou Tap Tempo, puis cible automatique à chaque ancre turquoise;
- accélération ou décélération linéaire entre les cibles BPM successives;
- persistance SQLite des clips et de leur position musicale;
- poste de travail plein écran avec timeline centrale, bibliothèque latérale défilable et Preview compacte;
- zoom continu atomique avec la molette et recentrage sur le playhead après la nouvelle mise en page;
- zoom extérieur calculé pour rendre le mix complet visible;
- playhead positionnable par clic et transport Play/Pause piloté par Rust;
- raccourci Espace pour basculer le transport principal;
- lecture audible des clips de la timeline;
- time-stretch maison conservant la tonalité, fondé sur un recouvrement temporel stéréo;
- adaptation automatique de chaque tempo source au BPM courant de la courbe globale;
- affichage de la rampe turquoise et du BPM cible de chaque clip dans la règle;
- mixage des clips superposés en float32 et protection du niveau de sortie;
- VU-mètre master stéréo analogique au centre de l'interface, alimenté par le vrai signal float32 de sortie;
- automation de volume indépendante sur les pistes A, B et C, éditable par Volume Nodes de −∞ à +12 dB;
- automation de panoramique à puissance constante et formes dessinées;
- limiteur master stéréo-lié commutable par le bouton `LIMIT`, à la place de l'écrêtage brut de la sortie;
- témoin vintage `OL` mesuré après le limiteur : il ne signale qu'un écrêtage réellement subi;
- décodage MP3 en continu dans une petite fenêtre PCM, avec lecture, mixage et time-stretch calculés en temps réel;
- changements de BPM, déplacements, reprises et Seek sans décodage préalable de la chanson complète;
- waveforms stéréo DAW haute définition à 16 384 colonnes, avec crêtes min/max et corps RMS;
- pyramide de détail sélectionnée automatiquement selon le zoom du clip;
- rattrapage en arrière-plan des waveforms pour tous les anciens morceaux de la bibliothèque;
- exclusion mutuelle entre la Preview et la lecture principale;
- décodage MP3 avec Symphonia par l'intermédiaire de Rodio;
- sortie audio native CPAL;
- indicateur Preview conditionnel avec Play/Pause;
- barre de Preview cliquable et déplaçable pour avancer ou reculer dans le morceau;
- affichage de la durée, de la progression et du format source;
- scission d'un clip à la touche `B`, sur la piste armée au pointeur, en deux sous-clips autonomes;
- rognage du début ou de la fin d'un clip en saisissant son extrémité, réversible et calé sur le tempo;
- raccourcis `Shift+S` et `Shift+M` pour le solo et le mute de la piste armée;
- historique Undo/Redo sur 50 niveaux, `Ctrl+Z` et `Ctrl+Y`;
- égaliseur trois bandes par clip, réglable en temps réel pendant la lecture;
- Smart Filter bipolaire par piste, dessiné au pinceau, redimensionnable et supprimable au clic droit;
- sauvegarde et chargement de projets `.mixcanvas`;
- bounce WAV stéréo 16 bits / 44,1 kHz avec dither TPDF;
- Undo/Redo sur 50 niveaux;
- séparation locale voix/instrumental par Open-Unmix et ONNX Runtime;
- compresseur de collage master, teinte de console et saturation commutés par le bouton `COMP`;
- compression sidechain : un clip devient la clé, se tait là où il en recouvre d'autres et y impose son pompage;
- bouton `CLEAR TIMELINE` qui vide la timeline sans toucher à la bibliothèque.

Le moteur ne rend pas la timeline complète et ne décode pas une chanson complète avant Play. Chaque ancre turquoise devient une cible du tempo global égale au BPM source de son clip; le BPM évolue linéairement entre deux cibles et demeure constant après la dernière. Chaque clip ouvre son MP3 seulement lorsqu'il devient actif et conserve une fenêtre PCM bornée autour de la position courante. La source audio convertit continuellement la position temporelle en beat de projet, puis en position source. Un moteur WSOLA stéréo-lié recherche une waveform corrélée avant chaque raccord, applique un fondu cosinus et conserve la tonalité sans varispeed; l'interpolation cubique remplace l'ancien rééchantillonnage linéaire. La timeline travaille directement à la fréquence réelle du périphérique audio afin d'éviter une double conversion 44,1↔48 kHz. Le transport, les trois pistes et le playhead utilisent la même conversion beat↔temps. Les pistes A/B/C reçoivent leur automation de volume en dB avant d'être sommées dans le bus stéréo float32; leurs états Mute/Solo agissent par masque atomique sans redécodage. Le bus master traverse ensuite le sidechain, le compresseur de collage, la teinte de console et sa saturation, la mesure, puis le limiteur; la borne physique de 0,98 ne sert plus que de dernier recours, et le témoin `OL` est mesuré après le limiteur afin de ne signaler qu'un écrêtage réellement subi. Un ajout ou un déplacement pendant Play remplace uniquement le plan compact de relations temporelles et reprend au même beat musical, sans rendu ni décodage complet. La limite de sécurité actuelle est de quatre heures et les ratios de time-stretch vont de 0,5× à 2×.

Depuis le jalon 0.0.16, le bus master alimente deux enveloppes de mesure indépendantes avant la borne de sortie. Le témoin `OL` est mesuré séparément, après le limiteur master : il ne s'allume que sur un écrêtage réellement subi, et reste donc éteint tant que le limiteur retient les crêtes. Les barres de LED L/R ont une attaque de type VU et une retombée plus lente; elles observent le signal sans le modifier. Les LED, les boutons mécaniques Play/Pause et le témoin de surcharge poursuivent la direction visuelle « studio vintage » de MixCanvas.

Après une mise à niveau, les ancres de timeline et le schéma sont migrés automatiquement. Les analyses mises en cache portent désormais une version : MixCanvas réanalyse seul les résultats anciens une seule fois, sans demander « Analyser tout ». Une correction manuelle existante demeure volontairement prioritaire.

La base utilisateur est créée sous `%APPDATA%\ca.mixcanvas.app\library.sqlite3` sur Windows. Elle contient uniquement l'index et les métadonnées; les MP3 restent à leur emplacement original.

## Prérequis de développement Windows

- Node.js avec Corepack;
- Rustup avec la toolchain indiquée dans `rust-toolchain.toml`;
- Microsoft C++ Build Tools avec le workload « Desktop development with C++ »;
- Microsoft Edge WebView2, déjà présent sur les versions modernes de Windows.

Les utilisateurs d'une version compilée n'auront pas besoin de ces outils.

## Installation des dépendances

```powershell
.\install.cmd
```

Le script utilise Corepack, fourni avec Node.js, pour télécharger la version verrouillée de pnpm dans `.corepack`. Le store pnpm est configuré dans `.pnpm-store` par `pnpm-workspace.yaml`, et les crates Cargo dans `.cargo-home`. Ces dossiers, `node_modules` et les sorties de compilation restent locaux et sont exclus de Git. Les versions reproductibles sont conservées dans `pnpm-lock.yaml` et `Cargo.lock`.

Il n'est pas nécessaire d'installer pnpm globalement ni de modifier le `PATH` Windows.
Les deux modèles de beat/downbeat nécessaires à l'analyse font partie du dépôt
et du paquet; aucun environnement Python, service externe ou téléchargement au
premier lancement n'est requis.

## Lancement

```powershell
.\dev.cmd
```

## Vérifications

```powershell
.\check.cmd
```

État de référence vérifié le 2026-07-28 : TypeScript, 179 tests frontend,
142 tests Rust, formatage Rust et Clippy passent; quatre tests d'intégration ou
de mesure longue restent ignorés explicitement. La production frontend Vite se
construit également. Les détails et les limites connues sont consignés dans
`handoff.md`.

## Documentation du projet

- `architecture.md` décrit la mécanique interne et les décisions techniques;
- `changelog.md` consigne quotidiennement les modifications;
- `handoff.md` résume l'état vérifié du dépôt à la fin de la dernière session;
- `THIRD_PARTY_NOTICES.md` conserve les licences et empreintes des modèles
  distribués;
- `LICENSE` contient la GNU Affero General Public License version 3.

## Licence

MixCanvas est distribué sous la licence [GNU AGPL version 3 uniquement](LICENSE), identifiée par l'expression SPDX `AGPL-3.0-only`.
