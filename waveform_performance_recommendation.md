# Recommandation — waveforms performantes, précises et lisibles

Date : 2026-07-30  
Statut : recommandation de conception — aucune modification de rendu n'est incluse dans ce document.

## Décision recommandée

Faire évoluer le rendu des waveforms vers une combinaison de :

1. cache de pics **multi-résolution** durable ;
2. sélection de la résolution et de la plage **selon le viewport** ;
3. dessin par **canvas 2D** plutôt que par grands chemins SVG ;
4. virtualisation des waveforms des clips hors champ.

Cette direction maintient les fonctions utiles à un beatmatch — transitoires nets, stéréo, RMS, zoom fin, trims, déplacements et waveform des stems — tout en supprimant le travail graphique qui n'apporte aucune information visible.

## État actuel

MixCanvas conserve déjà un cache durable de waveform dans la base SQLite de la bibliothèque : six séries de `f32` par fichier (`left/right` × `min/max/RMS`), chacune avec jusqu'à 16 384 buckets. Ce cache évite de redécoder le MP3 pour chaque affichage et constitue déjà l'équivalent fonctionnel d'un fichier `.wfm` de DAW.

Le frontend construit aussi une pyramide de niveaux, ce qui est une bonne fondation. Cependant, la résolution du niveau affiché est actuellement choisie à partir de la **largeur entière du clip**. Une longue pièce peut donc produire quatre chemins SVG très détaillés alors que seule une petite portion est à l'écran. Chaque zoom recrée ces chemins, et le déplacement continu de la timeline doit ensuite les repeindre.

Le coût principal n'est donc généralement pas l'analyse ni la lecture audio : c'est le travail de layout, de parsing et de peinture des grands paths SVG. Il est plus sensible dans la build `NOGPU`, où ce rendu est surtout logiciel.

## Pourquoi une réduction de données ne dégrade pas la lecture

Un écran ne peut montrer qu'un nombre fini de colonnes. Si une région fait 800 pixels de large, dessiner 16 384 valeurs dans cette région ne crée pas 16 384 détails perceptibles. La bonne représentation est une enveloppe min/max par colonne ou par petit groupe de colonnes : elle conserve les kicks et transitoires, contrairement à une moyenne simple qui pourrait les lisser.

À haut zoom, le moteur choisit simplement un niveau plus dense. À faible zoom, il choisit un niveau plus compact. La waveform conserve ainsi la meilleure information réellement visible à chaque échelle.

## Architecture cible

### 1. Cache multi-résolution

Conserver le cache central SQLite — il évite de polluer les dossiers de musique et reste portable avec les données de MixCanvas — mais y stocker ou y dériver durablement les niveaux suivants :

`16 384 → 8 192 → 4 096 → 2 048 → 1 024 → 512 → 256 → 128`

Chaque réduction doit conserver :

- le minimum réel ;
- le maximum réel ;
- le RMS énergétique ;
- les deux canaux stéréo.

Le cache est associé à l'identité du fichier audio : chemin normalisé, taille, date de modification et, si nécessaire, hash léger. Une modification du MP3 invalide seulement son cache. Une version de format permet de reconstruire les anciennes waveforms automatiquement après une évolution du format.

Un fichier `.wfm` indépendant par MP3 n'est pas requis à court terme. Il peut devenir une option d'export ou de partage plus tard, mais il ne résout pas à lui seul le coût du dessin à l'écran.

### 2. Sélection par viewport et par niveau de détail

Le renderer reçoit :

- les bornes musicales visibles du viewport ;
- une marge de préchargement de part et d'autre ;
- la largeur réelle à dessiner en pixels ;
- le trim actif du clip.

Il choisit le niveau dont le nombre de buckets est voisin du nombre de colonnes à dessiner, puis demande uniquement la tranche qui recouvre cette fenêtre. Un clip long hors écran ne produit aucune géométrie waveform.

La marge de préchargement élimine le risque de voir une waveform apparaître en retard pendant un scroll ou pendant le suivi du playhead.

### 3. Canvas 2D statique par voie affichée

Remplacer les quatre paths SVG actuels par un canvas 2D. Pour chaque colonne :

- un trait ou une barre min/max dessine l'enveloppe de pics ;
- une surface ou un trait plus dense dessine le RMS ;
- les deux canaux restent séparés verticalement ;
- la palette actuelle peut être reproduite exactement.

Le canvas ne participe pas aux interactions : clip, trim, drag, beatgrid, automation et menus contextuels restent dans le DOM normal au-dessus de lui. Il devient donc une couche de peinture pure, facile à invalider uniquement quand la fenêtre visible, le zoom, le trim ou la waveform changent.

Le rendu peut être un canvas par piste, ou un canvas par clip uniquement si les mesures montrent qu'il est plus simple à maintenir. Pour trois pistes fixes, un canvas par piste est le choix à privilégier : moins d'éléments, un seul passage de dessin et une gestion claire du clipping.

### 4. Invalidation explicite

Redessiner une waveform uniquement lors de :

- chargement ou changement de waveform ;
- ajout, retrait, déplacement ou trim d'un clip visible ;
- changement de zoom ;
- changement de la fenêtre visible ;
- redimensionnement de la timeline ;
- création ou suppression d'un stem utilisé par le clip.

La progression de lecture ne doit pas reconstruire les données ni la géométrie waveform. Elle déplace uniquement la fenêtre de visualisation et le playhead; si la fenêtre a effectivement avancé vers une nouvelle zone, seul ce nouveau segment est dessiné.

## Ordre d'implantation recommandé

1. **Mesurer l'état actuel** : profiler un mix de référence avec plusieurs longues pièces, au zoom minimal, normal et maximal; mesurer le temps de rendu lors d'un zoom et le FPS durant la lecture.
2. **Rendre le choix de niveau dépendant du viewport** : c'est le meilleur gain immédiat tout en gardant SVG et le design actuel.
3. **Virtualiser les waveforms hors champ** : conserver les clips et leurs interactions, mais ne plus construire leur géométrie graphique.
4. **Passer au canvas 2D** : reproduire visuellement min/max, RMS et stéréo, puis comparer les captures aux SVG existants.
5. **Persister la pyramide** si les mesures d'import montrent que sa reconstruction en mémoire est significative. Ce dernier point est une optimisation de chargement, non une condition au nouveau renderer.

Chaque étape doit être indépendante, testable et réversible. Aucun compromis visuel n'est requis pour les étapes 2 et 3.

## Critères d'acceptation

- Les kicks, transitoires et downbeats restent aussi lisibles qu'aujourd'hui.
- Les canaux gauche/droite et le RMS demeurent visibles.
- Aucun recalcul de waveform ne se produit à chaque tick de transport.
- Un zoom continu ne crée ni frame blanche ni saut visuel.
- Les clips hors viewport ne consomment pas de géométrie graphique active.
- Les waveforms sont disponibles immédiatement après leur première analyse, sans redécoder le MP3 lors des ouvertures suivantes.
- Le rendu NOGPU reste fluide avec un projet de référence comportant trois longues pistes et plusieurs clips.

## Conclusion

La direction n'est pas de rendre la waveform moins définie : elle consiste à afficher, à chaque pixel et à chaque niveau de zoom, la résolution qui transmet réellement le plus d'information musicale. Le cache évite le recalcul audio; le viewport, le niveau de détail et le canvas évitent le coût graphique inutile.
