# Journal des modifications de MixCanvas

Ce document consigne quotidiennement les changements matériels apportés au projet : fonctionnalités, architecture, expérience utilisateur, corrections et documentation.

Les entrées sont classées de la plus récente à la plus ancienne. Une journée peut distinguer les éléments ajoutés, modifiés, corrigés et les décisions prises. Une décision discutée mais non retenue ne doit pas être présentée comme une fonction terminée.

## 2026-07-30

- **Une recommandation de performance waveform est formalisée.** `waveform_performance_recommendation.md` documente l'état actuel, le rôle du cache SQLite déjà présent et la direction retenue : cache multi-résolution, sélection selon le viewport, canvas 2D et virtualisation des clips hors champ. C'est une décision de conception seulement; aucun rendu existant n'est modifié par cette entrée.

## 2026-07-29

- **Un morceau à 48 kHz se sépare enfin.** La timeline l'acceptait, la séparation le refusait net — « needs 44.1 kHz for now ». Le modèle est bien entraîné à 44,1 kHz et un spectre analysé ailleurs range les mêmes sons dans d'autres bandes, mais la réponse était de **rééchantillonner**, pas de renvoyer l'utilisateur.
  - Interpolation sinc par `rubato`, déjà dans l'arbre via `beat-this` : le déclarer en dépendance directe ne coûte ni téléchargement ni compilation, et évite d'écrire un rééchantillonneur maison là où le résultat s'écoute. Mêmes paramètres que ceux dont `beat-this` se sert avec la même version.
  - Le rééchantillonnage a lieu **avant la moindre conversion de millisecondes en indices**. Plus bas, une seule de ces conversions restée à l'ancienne fréquence décalerait la fenêtre du clip sous sa grille, sans que rien ne le signale.
  - Chaque canal passe séparément : deux instances identiques appliquent le même retard, donc l'image stéréo ne bouge pas. Quatre tests couvrent le passage sans changement à fréquence égale, la **durée** préservée à dix millisecondes près — c'est elle qui porte la grille —, le niveau d'un sinus conservé sans saturation, et le canal vide qui n'est pas une erreur.

- **Un clip rogné du début peut de nouveau reculer.** La butée de gauche protégeait le pré-roll entier, y compris la part qu'on venait de couper : le premier clip d'une timeline refusait d'aller jusqu'à zéro, retenu par une tête qu'il ne fait plus entendre. On ne borne que ce qui s'entend — `ancre ≥ pré-roll − rognage`, plancher à zéro.
  - La règle vit forcément en double, Rust et interface. Elle est donc nommée et testée des deux côtés plutôt que recopiée à l'aveugle, et un test croise la butée avec la géométrie pour vérifier qu'à l'ancre minimale le clip commence bien à zéro — sans jeu perdu.
  - Il reste une limite de conception, et elle est réelle : un clip rogné **plus loin que son pré-roll** ne peut toujours pas commencer au temps zéro, parce que son ancre devrait être négative et que le schéma l'interdit. Le test le dit explicitement au lieu de le masquer.

- **« Restore Automatic » remet vraiment tout à zéro.** Il ne réinitialisait ni le premier temps affiché ni la mention « corrigé ». La base, elle, faisait son travail — un test le prouve : tempo, premier temps, compte de temps et mention reviennent tous à l'analyse. C'était l'éditeur qui ne se resynchronisait pas.
  - Son effet ne se déclenchait que si le tempo ou le premier temps du morceau changeait de valeur. Or la sauvegarde cale le premier temps manuel sur la grille analysée : une remise à zéro retombe souvent sur **exactement** les mêmes nombres, l'effet ne partait pas, et l'éditeur gardait les valeurs tapées. C'est maintenant l'état « corrigé » qui le déclenche — le seul signal qui ne peut pas manquer, puisqu'il bascule toujours de vrai à faux.

- **L'écoute passe à −4 dB.** Un MP3 masterisé sort à pleine échelle, et cette écoute sert à travailler — taper les temps, chercher un premier temps — pas à juger un mix. Le niveau est écrit en décibels et converti, plutôt qu'en gain linéaire : « 0,63 » ne se relit pas.

- **Le dernier pas de la marche à suivre ne touche plus le panneau.** Zéro pixel entre les deux, mesuré : le coin arrondi disparaissait dans le fond clair et la bande avait l'air coupée. Le rembourrage du bas est désormais celui du haut.


- **Le choix du rendu passe de la compilation à l'exécution, et le logiciel devient le défaut.** Le scintillement du zoom venait de la composition matérielle de WebView2 — pas de notre mise en page. La preuve tient en deux essais : une approche tout en transformations, censée être la plus douce pour le GPU, l'a **aggravée**; couper le GPU l'a fait disparaître. On ne répare pas ça depuis ici, c'est le compositeur de Chromium sur un pilote donné.
  - Une *feature de compilation* obligeait à livrer deux exécutables, ou à parier pour tous les utilisateurs à partir d'une seule machine. Un seul binaire décide au lancement : rien par défaut donne le logiciel, `--gpu` l'accélération complète, `--gpu-safe` la rastérisation matérielle avec composition logicielle — l'entre-deux qui corrige le plus souvent cette famille d'artefacts sans rendre la carte inutile. Comparer ne demande plus de reconstruire.
  - Le logiciel comme défaut est un arbitrage assumé : l'interface est du DOM en deux dimensions, et le vrai travail — décodage, analyse, DSP — est en Rust, hors d'atteinte. Un scintillement rend l'outil inutilisable; quelques pourcents de processeur, non.
  - Une variable `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` posée avant le lancement est respectée : on ajoute, on ne remplace pas, et jamais deux fois. Trois tests couvrent le choix du mode, la dernière option qui l'emporte, et cette fusion.
  - `ABOUT` explique quoi lancer si l'affichage scintille malgré tout.

- **L'anneau d'analyse tourne pour de bon, et « Analyzing » perd ses points de suspension.** Un arc de trois quarts au bleu de l'application plutôt qu'un quart de tour : à neuf pixels, un quart se lisait comme un point qui vibre. Il reste **indéterminé** et le restera — `analyze_file` est un seul appel opaque dans la caisse `beat-this`, sans rappel de progression, et un anneau qui se remplirait sur une durée estimée mentirait précisément quand l'analyse traîne, c'est-à-dire quand on le regarde.

- **L'éditeur de grille dit sa marche à suivre au lieu de la laisser deviner.** Les deux champs se présentaient côte à côte, également remplissables, sans rien dire duquel on part : un nouvel arrivant y lisait deux méthodes concurrentes plutôt qu'une procédure.
  - Trois pas numérotés entre l'écoute et les champs. Le premier — **la demi-vitesse** — porte son propre bouton : une recommandation en prose à côté d'un réglage qu'il faut aller chercher ailleurs se fait ignorer. Un pas franchi s'éteint plutôt que de disparaître, sinon les suivants sautent sous le curseur.
  - Le premier temps garde son champ mais devient un **résultat** : son étiquette dit d'où vient la valeur, et son texte d'aide dit qu'on ne devrait pas avoir à y toucher.


- **Le Beatgrid Editor gagne une Preview `½ SPEED` et une rangée Tap 1 stable.**
  - Le ralenti est un varispeed Preview à `0.5` : il abaisse le pitch mais préserve la forme franche des transitoires, plus utile ici qu'un time-stretch susceptible de les étaler.
  - Correction après le premier essai portable : Rodio rapporte une position transformée par le varispeed, qui faisait initialement calculer la moitié du BPM réel. Le backend convertit désormais explicitement cette coordonnée vers le temps du MP3 source; à `0.5`, Tap 1 obtient donc automatiquement le double du BPM brut ralenti. Les Seek utilisent la conversion inverse et les changements de vitesse se recalent sur la même position source sans saut.
  - La vitesse fait partie du snapshot Preview et passe par une commande bornée à `0.5` ou `1.0`. Le chargement d'un morceau, le retour à la timeline et la fermeture du Beatgrid Editor restaurent la vitesse normale.
  - `÷2` et `×2` sont retirés. `Clear` reste toujours rendu au même endroit — simplement désactivé avant le premier tap — de sorte que le bouton Tap 1 ne se décale plus au moment précis où l'utilisateur doit le refrapper.
  - Après quatre prises au minimum, la valeur numérique de l'accuracy devient verte lorsqu'elle est strictement inférieure à 20 ms. Avant quatre mesures, aucune accuracy ni couleur de qualité n'est affichée.
  - Un test Rust couvre les deux seules vitesses autorisées, un autre verrouille la conversion bidirectionnelle entre temps Rodio et temps source, et un test TypeScript verrouille ensemble le seuil de quatre prises et la frontière stricte de 20 ms.
  - La première build `MixCanvas-0.0.17-2026-07-29-TAP1-SLOW-NOGPU-portable.exe` est conservée dans le journal comme essai, mais remplacée : sa conversion temporelle à demi-vitesse divisait le BPM par deux.
  - Vérification finale : 194 tests TypeScript et 154 tests Rust réussis, 4 mesures audio lourdes ignorées comme prévu, formatage et Clippy conformes.
  - La build corrigée `MixCanvas-0.0.17-2026-07-29-TAP1-SLOWFIX-NOGPU-portable.exe` combine la conversion temporelle finale, `disable-gpu` et les cinq ressources ONNX intégrées. Les trois modèles, les deux DLL ONNX et le flag WebView2 sans GPU ont été retrouvés dans le fichier; le programme est resté actif six secondes puis a accepté une fermeture normale. Taille : 65 503 744 octets (62,47 Mio). SHA-256 : `55F702C3F2E9C79536494B0DEB3F8EEB8F8CF5575221AAA743A663653F66C8D6`.

- **Tap Tempo devient Tap 1 et mesure une vraie grille manuelle sur plusieurs mesures.**
  - Chaque pression représente le premier temps de la mesure suivante : MixCanvas connaît donc automatiquement les beats `0, 4, 8, 12…`, sans demander un nombre de mesures à l'utilisateur.
  - Les positions viennent de l'horloge source du moteur Preview par `preview_snapshot`, pas de `performance.now()` ni de la position React rafraîchie périodiquement. La cadence graphique ne peut donc plus ajouter sa propre erreur à la frappe.
  - Une régression linéaire ajuste simultanément le BPM et le premier temps à partir de toutes les mesures. Quatre taps sont requis, huit recommandés et seize conservés au maximum; l'interface affiche l'écart temporel RMS et permet de vider la série.
  - Une mesure sautée est détectée par ses intervalles incohérents et la série est refusée plutôt que transformée en faux BPM. Un Seek arrière redémarre naturellement la mesure, tandis qu'une saisie manuelle retire l'indicateur Tap 1 devenu caduc.
  - `Snap to beat` reste disponible dès qu'une grille Tap 1 valide existe : il reçoit maintenant un BPM beaucoup mieux contraint et conserve le premier temps musical choisi par l'utilisateur.
  - Sept tests TypeScript couvrent le minimum de quatre mesures, le BPM et la phase, la résistance aux erreurs humaines, une mesure sautée, le redémarrage après Seek arrière, la limite de seize taps et l'indicateur d'accuracy.
  - La build `MixCanvas-0.0.17-2026-07-29-TAP1-NOGPU-portable.exe` combine Tap 1, `disable-gpu` et `embed-resources`. Les trois modèles, les deux DLL ONNX et le flag WebView2 sans GPU ont été vérifiés dans le fichier. Le programme est resté actif six secondes avec sa fenêtre principale puis a accepté une fermeture normale. Taille : 65 498 112 octets (62,46 Mio). SHA-256 : `C09E5A42A2D71CFD5FD26DC2DB77EE64384E31773F102A04AD1629E2FCE1166F`.

- **Le Beatgrid Editor manuel ne dépend plus du downbeat automatique qu'il doit corriger.**
  - Le point capturé avec `Set to…` devient l'autorité musicale : « Snap to beat » affine le BPM autour du Tap Tempo, puis déplace uniquement ce point vers le beat le plus proche de la grille rigide raffinée.
  - Le downbeat proposé par le modèle ne remplace plus le `1` choisi par l'utilisateur. Il ne sert que d'origine mathématique de la pulsation; une origine décalée d'un nombre entier de beats décrit la même grille.
  - « Save Correction » conserve exactement les valeurs affichées. La sauvegarde ne quantifie plus silencieusement la position manuelle sur l'ancienne grille automatique, comportement qui rendait impossible la correction d'une analyse erronée.
  - Trois tests unitaires couvrent le beat le plus proche, l'indépendance envers la phase de mesure du modèle et la protection au début du fichier. Le test de persistance utilise désormais une correction proche de l'analyse automatique et exige que ses `61,900 s` demeurent distinctes des `61,946 s` automatiques.
  - La build monofichier `MixCanvas-0.0.17-2026-07-29-BEATGRID-NOGPU-portable.exe` combine ce correctif avec `embed-resources` et `disable-gpu`. Les modèles Beat This, Mel et Open-Unmix ainsi que les DLL ONNX sont présents dans l'exécutable. Un lancement de contrôle est resté actif avec sa fenêtre principale puis a accepté une fermeture normale. Taille : 65 497 600 octets (62,46 Mio). SHA-256 : `1119F82C2741C59F66561EB0B75C7E1A2DE704992CC78091E8A58B68E9832BFD`.

- **Une build diagnostique sans accélération GPU peut maintenant être produite sans modifier la build normale.** La feature Rust `disable-gpu` ajoute le browser flag officiel `--disable-gpu` aux arguments de WebView2 avant l'initialisation de Tauri. Elle est volontairement absente des builds ordinaires et ne sert qu'à répondre à une question précise : si le flash de zoom disparaît dans cette variante, la composition WebView2 ou le pilote graphique est impliqué; s'il demeure, la cause est dans la séquence de layout/peinture. Les arguments WebView2 déjà définis sont conservés plutôt qu'écrasés.
  - `MixCanvas-0.0.17-2026-07-29-NOGPU-portable.exe` combine `disable-gpu` et `embed-resources`. Le binaire final contient bien `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` et `--disable-gpu`; cette vérification distingue la variante d'un simple renommage de la build normale. Taille : 62,46 Mio. SHA-256 : `E3140F26125D535CD1E55F1CDCF7055162034FD2B2FB4B43C30FB3A137667DA2`.
  - **Résultat utilisateur confirmé : le flash disparaît dans la build `NOGPU`.** Le test A/B utilise le même logiciel, le même projet et le même correctif de layout; seule l'accélération matérielle change. La cause est donc isolée dans la voie de composition GPU de WebView2 ou du pilote graphique. La variante sans GPU devient le daily driver de diagnostic, sans devenir encore le défaut permanent avant mesure de son coût CPU.

- **Le flash d'une image à chaque zoom est retiré à sa source.** Le chemin ajouté aujourd'hui avait réintroduit exactement la mécanique que l'architecture du zoom interdit : le recentrage du contenu passait par `translateX`, puis une rafale de molette étirait un enfant par `scaleX`, lui-même conservé dans un calque GPU permanent avec `will-change`. React publiait bien une géométrie cohérente, mais WebView2 pouvait composer immédiatement les transformations avec une texture encore issue de l'échelle précédente; grille, clips, courbes, waveforms et playhead représentaient alors deux images différentes pendant une frame — le flash désynchronisé observé avec la molette comme avec `R`/`T`.
  - L'aperçu étiré, son délai de pose, son conteneur et son calque permanent sont supprimés. Il n'existe plus d'échelle visuelle « en attente » à réconcilier avec l'échelle réellement rendue.
  - Le regroupement des événements par animation frame demeure : une rafale de molette produit une seule variation bornée par image, mais chaque variation publie maintenant directement un unique `pixelsPerBeat`.
  - La largeur musicale et son placement `left`/`margin-inline` sont dérivés de ce même état et posés dans le même commit DOM. Le playhead reste centré et le zoom extérieur continue de cadrer tout le projet; l'optimisation qui ne crée que les marqueurs de mesure visibles est conservée.
  - Vérification complète : build frontend de production, 191 tests TypeScript et 156 tests Rust réussis.
  - La build monofichier `MixCanvas-0.0.17-2026-07-29-zoomfix-portable.exe` embarque le correctif et toutes les ressources ONNX. Elle a été lancée depuis le dossier `portable`, est demeurée active et a accepté une fermeture normale. Taille : 62,46 Mio. SHA-256 : `A0C2B657FF58606FAA628A1550A8B99727AD81DFD75B9A0DD3D271BB72B2E371`.

- **Nouvelle build portable quotidienne validée.** `MixCanvas-0.0.17-2026-07-29-portable.exe` est une build release monofichier de 62,46 Mio produite avec `embed-resources` : ONNX Runtime, le modèle Beat This, le modèle de spectrogramme et le modèle Open-Unmix sont embarqués dans l'exécutable. Les 195 tests de l'interface et les 156 tests Rust ont réussi avant compilation; l'exécutable final a ensuite été lancé depuis le dossier `portable`, est demeuré actif et a accepté une fermeture normale. SHA-256 : `8F1CDBE368AE52C40CB5F0378F56A8F56FCF8D0E05617C5A2780114BA2215772`.

- **Un cran de zoom isolé se rend immédiatement — l'étirement ne sert plus qu'aux rafales.** Le micro-saut inverse persistait sur un cran seul, en plus petit, et grossissait avec la surface affichée : la signature d'un artefact de composition dans la paire étirer-puis-poser elle-même — dépromotion du calque à chaque pose, re-rastérisation plein écran — et non d'une erreur de géométrie, celle-ci étant prouvée identique au pixel. Un cran isolé n'a pas besoin de cette paire : il commet en une seule image, dans le sens commandé, et ses artefacts n'existent plus **parce que le mécanisme n'existe plus** pour ce cas. Mesuré : sur un cran seul, la grille change une fois et l'étireur ne quitte jamais `scaleX(1)`.
  - L'étireur garde désormais une transformation en permanence (`scaleX(1)`, jamais l'absence) et son calque avec — `will-change` : retirer la propriété dépromouvait le calque à chaque pose, et la re-rastérisation qui suit grandit précisément avec ce qu'il y a à rastériser. « Plus gros le clip, plus gros le glitch. »
  - Les rafales gardent l'étirement : premier cran net immédiatement, les suivants étirés, pose nette au silence. C'est là que rendre à chaque cran faisait strober, et seulement là.

- **Le zoom sautait d'abord dans l'autre sens — corrigé, et cette fois mesuré avant d'être livré.** Le correctif d'hier avait introduit son propre défaut : l'étirement du geste et le placement du contenu écrivaient la **même propriété**, l'un à la main, l'autre par React. Or React ne réécrit une propriété que si *sa* version a changé — et le placement ne bouge pas quand la tête est au temps zéro. L'étirement de la main survivait donc au rendu net, chaque pose sur-zoomait, et le premier cran du geste suivant remplaçait ce reliquat par une petite valeur : un saut dans le sens **contraire**, puis la bonne commande. Le stroboscope même qu'on croyait soigné.
  - Relevé dans le DOM avant correction : après la pose, la grille rendue à 18,478 px et l'élément portant **encore** `scaleX(1.15488)`. Après correction, la même séquence — deux crans avant, un cran arrière, molette et clavier — montre l'aperçu dans le sens commandé à chaque cran et l'étireur **vide** après chaque pose.
  - Le correctif est une règle, pas une rustine : **une propriété, un écrivain**. L'étirement habite désormais son propre élément, que React ne touche jamais; il s'efface dans le commit même qui rend le zoom net — après l'écriture du DOM, avant la peinture. Plus tôt, une image montrerait l'ancienne mise en page sans étirement; plus tard, la nouvelle encore étirée.

- **Le zoom ne strobe plus : il étire pendant le geste, et ne rend qu'après.** Le mal de tête reproché n'était pas une couche en retard — un rendu React est atomique, tout part dans la même image. C'était le **coût par cran** : chaque tick de molette changeait toutes les coordonnées du monde d'un coup — un marqueur par mesure du projet *entier*, chaque clip, chaque trame, la règle en pleine largeur — et relançait leur mise en page. La cadence s'effondrait, la molette s'accumulait pendant les images manquées, et chaque image peinte sautait donc un *grand* pas de zoom, avec des niveaux d'onde et des étiquettes qui claquaient au passage. En release aussi : le coût était la mise en page, pas le JavaScript.
  - Pendant le geste, le contenu est désormais simplement **étiré** — une seule transformation, composée par le GPU, sans mise en page ni repeinture. Grille, ondes, enveloppes et tête de lecture ne peuvent plus se désynchroniser **par construction** : il n'y a qu'un objet animé. Le rendu net se fait 90 ms après le dernier cran, ou dès que l'étirement dépasse le double ou la moitié — quelques rendus entiers par grand zoom, plutôt que trente rendus déchirés.
  - **Le point fixe de l'étirement est celui du rendu** : le temps affiché au centre. Un test vérifie numériquement que l'ancre tombe au pixel près là où la mise en page nette la posera — sinon l'image glisserait pendant le geste puis sauterait au rendu.
  - **Un appui pendant l'étirement rend d'abord net** : les gestes lisent leurs positions dans le zoom rendu, et un écran étiré leur ferait viser à côté d'un facteur d'échelle.
  - **La règle ne fabrique plus que les marqueurs visibles.** Il y en avait un par mesure du projet — 4 500 sur un mix de deux heures — tous mis en page à chaque rendu pour une trentaine dans la fenêtre. Leur fenêtre est alignée sur le pas des étiquettes, pour qu'un défilement révèle toujours les mêmes marqueurs au lieu d'en inventer aux bords.
  - **Le suivi de la tête passe de `left`/`margin` à une transformation.** Les premiers relançaient une mise en page vingt fois par seconde pendant la lecture; la transformation se compose sans rien repeindre — et c'est la même propriété qui porte l'étirement du zoom.
  - Le compromis, assumé : pendant un grand coup de molette, textes et ondes s'étirent visiblement (jusqu'à ×2) le temps que la main s'arrête — cent millisecondes plus tard, tout est net. C'est l'échange du flou de Figma ou d'une carte : un étirement franc qui se lit comme voulu, contre un stroboscope qui se lit comme cassé.


- **Les médias vivent enfin quelque part.** Stems et cuissons étaient versés en vrac dans le dossier de données, sans lien avec le projet qui les avait demandés : rien ne disait à qui appartenait quoi, et rien ne les effaçait jamais — 163 Mo de stems dormaient déjà là, dont plusieurs orphelins.
  - Un dossier **`MixCanvas Files`** à côté de l'exécutable, avec un sous-dossier par projet. C'est la convention d'un programme portable : on copie le tout sur une clé et ça marche ailleurs. Repli sur le dossier de données si l'écriture est refusée — `Program Files`, un partage réseau, une clé protégée — et le test est une **écriture réelle**, pas une lecture de permissions : sous Windows un dossier peut se déclarer accessible et refuser le fichier.
  - Tant que la session n'a pas de nom, c'est **`Scratch`**. Au premier enregistrement les médias suivent, et la base est réécrite dans la même opération.
  - **Enregistrer sous un nouveau nom copie au lieu de déplacer.** C'est le geste qu'on fait pour garder une variante; déplacer casserait l'original, dont le fichier de projet attend ses médias là où il les a laissés. Depuis `Scratch`, en revanche, personne d'autre ne les désigne : ils suivent.
  - Le moteur est fait taire avant de toucher aux fichiers. Windows refuse de déplacer un fichier ouvert, et la lecture tient ses décodeurs — même raison que pour l'ouverture d'un projet.
  - Le projet est écrit **après** le déménagement, donc il porte les chemins d'arrivée. Écrit avant, il aurait désigné des fichiers déjà partis.
  - **Le ménage à la fermeture n'efface que les non-référencés**, et pas les « inutilisés dans la séquence ». La nuance est le cœur de la décision : un stem coûte deux minutes de calcul, et une erreur de jugement au moment où l'on ferme — quand personne ne regarde et qu'aucune annulation n'est plus possible — les perd pour de bon. « Plus aucune ligne ne pointe vers lui » est vrai par construction; « la séquence ne s'en sert pas » est un raisonnement, et un raisonnement peut se tromper.
  - Les chemins sont comparés à la casse et aux séparateurs près. Un chemin qui a transité par du JSON revient rarement tel quel, et le comparer brut ferait passer pour orphelin un fichier bel et bien utilisé — donc l'effacerait. C'est un test à part.
  - `ABOUT` dit maintenant où sont ces fichiers, comment ils sont rangés et ce qui est effacé. La question s'est posée une fois; elle se reposera.

- **Un projet enregistré perdait les stems et les cuissons — et pour une cuisson, c'était une perte sèche.** Le format portait la voie, l'ancre, le rognage, le sidechain et l'égalisation, mais ni la voix choisie, ni les fichiers séparés, ni la cuisson. Or `removed` est la **seule** copie de l'automation qu'un bake a emportée : enregistrer puis rouvrir rendait le clip sec, sur une voie plate, et il n'y avait plus rien à restaurer. Régression introduite avec `BAKE`, et qui ne s'était pas vue.
  - Le format transporte désormais `stem`, les fichiers séparés et la cuisson, `removed` comprise. **Les chemins voyagent, pas le son** : un WAV de séparation pèse trente-cinq mégaoctets, deux par clip sur vingt clips feraient un projet d'un gigaoctet et demi qu'on n'enverrait à personne. Un fichier absent au rechargement n'est pas une erreur — le clip retombe sur sa source, ce qui s'entend et se répare d'un clic.
  - L'automation enfouie, elle, voyage **même si le fichier ne se retrouve pas**. C'est la seule donnée irremplaçable du lot : un WAV se recalcule, une courbe dessinée à la main ne se retrouve nulle part.
  - Les champs nouveaux sont optionnels à la lecture : un projet écrit avant aujourd'hui se relit sans rien changer, en jouant le morceau entier — ce qu'il faisait de toute façon.
  - Un test suit un clip cuit de bout en bout, jusque dans une base qui n'a jamais vu ce morceau : la voix choisie, les fichiers, la cuisson et l'automation enfouie au caractère près. Il vérifie aussi qu'aucun média ne reste rattaché à l'identifiant de la session qui a écrit le projet — le clip en reçoit un neuf à l'arrivée.

- **Un seul outil, dont la position décide.** Le crayon s'armait par un bouton et prenait alors toute la voie : il fallait se rappeler dans quel mode on était, et un mode qu'on ne voit pas se découvre en cassant quelque chose. Désormais c'est l'endroit du **premier appui** qui choisit — les bords rognent, la barre de titre déplace, le corps dessine, et le vide de la voie place la tête de lecture.
  - Le geste choisi ne change plus ensuite. Un trait commencé dans le corps continue de dessiner même en passant sur la barre : la capture du pointeur le tient jusqu'au relâchement, curseur compris.
  - **Un bord reste un bord même dans la barre.** L'ordre des questions n'est pas interchangeable : demander la hauteur avant l'horizontale ferait disparaître les sept pixels de prise du rognage sur toute la hauteur du titre.
  - **Sans ligne affichée, le corps redevient une prise.** Un crayon n'a nulle part où écrire quand les deux automations sont masquées; le montrer promettrait un geste impossible. `VIEW` garde ainsi un rôle qu'on comprend, et le curseur ne ment jamais.
  - **`DRAW` perd son cran « éteint ».** Il ne pouvait plus vouloir dire « pas de crayon » — pour ne pas dessiner, on remonte dans la barre. Il ne répond plus qu'à une question : *quoi*. Trois formes, une période, et rien à armer.
  - La frontière entre barre et corps est **mesurée sur la barre elle-même**, pas recopiée dans le code. Une constante en double aurait fini par s'écarter de la ligne qu'on voit; `offsetHeight` lit celle qui est dessinée, et ne coûte pas de recalcul de style à chaque déplacement du pointeur.
  - **Le curseur ne suivait pas.** La garde « bouton gauche » avait été écrite dans la fonction commune à l'appui **et** au survol; or un `pointermove` de survol porte `button === -1` — aucun bouton n'a changé d'état — et la fonction sortait avant tout calcul. Le geste partait juste, l'icône restait figée. Mesuré sur un vrai événement plutôt que déduit : `button: -1`, `isTrusted: true`. Quel bouton est pressé regarde l'appui, pas la position; la question est retournée là d'où elle vient.
  - Une conséquence à connaître : **on ne dessine plus sur une voie vide**, seulement au-dessus d'un clip. C'est ce que la règle implique, et la bande de filtre garde son propre tracé.

- **`BAKE` : un clip rendu avec ses effets, dans un fichier à lui.** L'égalisation du clip et l'automation de sa voie passent dans le son, et la voie repart à plat sous lui — on peut alors redessiner par-dessus, autant de fois qu'on veut. C'est ce qui manquait pour poser un filtre sur un motif déjà dessiné : deux automations ne peuvent pas occuper les mêmes temps, mais une automation peut se poser sur un son qui en contient déjà une.
  - **C'est une bascule, pas un aller simple.** L'automation retirée est rangée dans l'enregistrement de la cuisson : recliquer la rend. Un bouton dont on ne revient pas finit par ne plus être cliqué du tout. Ce qui a été dessiné *depuis* la cuisson est remplacé — deux automations ne peuvent pas cohabiter — et l'annulation le couvre.
  - **Le fichier est cuit au tempo propre de la source**, sans étirement. Cuire un clip déjà étiré vers le tempo du projet le ferait étirer une seconde fois le jour où ce tempo change. L'automation reste indexée sur les temps : son alignement avec le son est le même avant et après.
  - **Le compresseur, le limiteur et le sidechain n'y entrent pas.** Les deux premiers appartiennent au bus général et seraient appliqués deux fois. Le troisième n'est pas un effet du clip mais une relation avec un autre clip, lequel peut encore bouger : figé, il pomperait à contretemps.
  - **Huit secondes de marge de chaque côté**, comme les stems : sans elles, rallonger le rognage après une cuisson tomberait dans le vide.
  - **Deux nœuds de repos referment les bords.** La voie continue après le clip; sans eux, la ligne rejoindrait le nœud suivant en rampant depuis le dernier nœud d'avant — de l'automation que personne n'a demandée, en travers de ce qui suit. Un test vérifie que décuire rend **exactement** ce que cuire a emporté, sur les trois automations à la fois : la troisième porte une colonne de plus, et une boucle écrite pour deux l'oublie sans rien dire.
  - Toute la mécanique existait déjà : `clip_stems` était un « ce clip joue depuis un autre fichier », avec son décalage et sa forme d'onde. Le bake est une seconde table de la même forme, consultée avant elle — le fichier cuit contient déjà le stem qui jouait, et le relire à travers un stem reviendrait à choisir deux fois. Scinder un clip cuit donne deux clips cuits, pour la même raison qu'un clip séparé donne deux clips séparés.
  - Un bake dont le fichier a disparu retombe sur la source. C'est faux à l'oreille, mais audible et réparable d'un clic; un silence, lui, ne se diagnostique pas.

- **La barre de cuisson annonçait « Separating stems », et ne bougeait pas.** Deux défauts d'un coup, dont le second se cachait derrière le premier : la fenêtre était celle des stems, réutilisée telle quelle, et l'écoute de `bake-progress` manquait — le Rust émettait sa progression, personne ne l'entendait. La barre serait restée à zéro du début à la fin.
  - La fenêtre reste partagée : séparer et cuire immobilisent tous deux un clip le temps d'un rendu, et deux fenêtres identiques n'apprendraient rien de plus. C'est ce qu'elle **dit** qui change. Annoncer une séparation pendant une cuisson envoie chercher un bug là où il n'y en a pas.

- **`EDIT` devient `EDITED` sur un tempo posé à la main.** La teinte se repère le long de la colonne, le mot se lit sur une seule rangée : les deux signaux ne s'adressent pas au même coup d'œil.

- **La grille de la timeline n'était pas irrégulière, mais elle en avait l'air.** Elle était tracée par **deux** trames superposées — une par temps, une par mesure. Or la largeur d'un temps est presque toujours fractionnaire (le zoom est exponentiel, et l'ajustement à la fenêtre divise une largeur par un nombre de temps), et le navigateur arrondit chaque répétition au pixel. Deux trames de périodes *w* et *4w* arrondissent chacune pour son compte : la ligne de mesure tombait tantôt exactement sur une ligne de temps, tantôt un pixel à côté. D'où des traits tantôt fins, tantôt doubles, tantôt épais — sans qu'aucune position ne soit fausse.
  - Une **seule** trame désormais, dont la période est la mesure et qui porte les quatre traits : le premier temps marqué, les trois autres pâles. Plus rien à faire coïncider, donc plus rien à dérégler.
  - Les quatre traits sont placés en **fractions de la période** plutôt qu'en multiples de la largeur d'un temps. Les deux valent le même nombre, mais seule la première façon garantit qu'ils tombent aux quarts exacts de ce qui est répété. Vérifié à un zoom fractionnaire : sur une mesure de 53,7 px, les traits sortent à 0 · 13,425 · 26,85 · 40,275.
  - La trame n'est écrite qu'une fois; les deux fonds, clair et sombre, ne redéfinissent que leurs deux encres.

- **L'autoplay ne dépend plus que de lui-même.** Il ne s'appliquait qu'au passage depuis le miniplayer : le même clic était tantôt silencieux, tantôt sonore, selon un état qu'on n'avait pas en tête. Allumé, un clic dans la timeline lance la lecture; éteint, il ne fait que poser la tête. Rien d'autre n'entre en compte.
  - Deux cas ne sont pas un démarrage et n'en demandent pas : une timeline vide, et une lecture **déjà en cours** — que `play_timeline` reconstruirait pour rien, avec le trou que ça s'entend.
  - Le transport se rafraîchit vingt fois par seconde; il est lu par une référence pour ne pas entraîner tout le panneau dans son rythme.

- **`AUTO` passe à droite du VU-mètre**, dans son propre boîtier, et s'ancre en haut pour partir de la même ligne que les touches de transport.

- **Un tempo posé à la main se voit enfin, et il est bleu.** La marque existait depuis le début dans `.bpm-value--manual`, mais écrite pour le fond sombre du panneau seul : dans une rangée, `.library-row .bpm-value` la surclasse — deux classes contre une — et la reprenait entièrement. Elle n'atteignait donc **jamais** la bibliothèque. On cherchait une indication qui, à la lettre, existait sans jamais s'afficher.
  - L'italique tenté d'abord **était** bien appliqué et rendu — 197 pixels d'écart sur 181 encrés, mesurés au canevas — mais illisible à dix pixels en chasse fixe. La mesure a servi à écarter la piste d'un `font-style` perdu dans la cascade, puis à conclure que le signal lui-même ne pouvait pas porter : il a été retiré plutôt que gardé comme décor.
  - Le bleu est **celui de l'application** — la pastille du menu, les chiffres du résumé, l'icône. C'est la seule couleur encore libre sur cette rangée : l'ambre appartient au sidechain, le turquoise aux voix, le rouge à la tête de lecture. Une valeur corrigée est un état du programme, pas d'une de ses fonctions; elle porte donc la couleur du programme. Le vert d'origine, lui, n'appartenait à rien.
  - Le bord tranche à 4,8:1 sur la plaque contre 2,0:1 pour le bord neutre voisin, et la mention `EDIT` tient 5,4:1 sur le lavis. Le chiffre garde son encre : la teinte doit se repérer le long de la colonne, pas repeindre la valeur.

- **La bande de filtre reçoit le trait du premier temps**, et lui seul. Elle porte déjà un dégradé vertical qui dit le sens du filtre; y ajouter les quatre traits la chargerait pour rien. Même période et même origine que les voies au-dessus — les deux repères tombent l'un sur l'autre.

- **Les pistes s'affichent au fur et à mesure de l'analyse.** Le tracker appris prend plusieurs secondes par morceau; sur un dossier entier, attendre la fin du lot laissait l'interface immobile assez longtemps pour qu'on la croie plantée. Chaque piste part maintenant dès qu'elle est passée, avec un compte qui avance — `Analyzing 12 of 87...`.
  - **Une rangée entière voyage**, pas seulement le tempo : l'interface remplace la sienne sans avoir à savoir lesquels de ses champs viennent de changer. Un échec compte autant qu'une réussite — une piste qui n'a pas pu être analysée doit cesser d'afficher « Analyzing... », et c'est justement celle qu'on attendrait le plus longtemps.
  - Le lot renvoie toujours la liste complète en terminant, qui **fait autorité** : une émission perdue ne casse rien, ce qui permet de l'ignorer plutôt que d'interrompre une analyse en cours pour un événement qui n'est pas arrivé.
  - Lire **une** piste passe par la même requête que les lire toutes. Une rangée ne devient une piste qu'au prix d'une dizaine de règles — le BPM manuel qui masque l'analysé, le compte de temps recalculé quand on a corrigé, le fichier disparu — et une seconde copie de ce raisonnement aurait fini par contredire la première. Un test compare les deux lectures sur une piste corrigée à la main, là où le travail est le plus lourd.

- **Nouveau bouton `AUTO`**, à droite de `DRAW`. Allumé — le défaut —, un clic dans la timeline lance la lecture, ce qu'on veut la plupart du temps. Éteint, il pose seulement la tête de lecture : quand on place des clips à l'oreille, le même clic relançait la musique vingt fois de suite.
  - Éteint, l'écoute en cours **s'arrête quand même**. Retenir le départ de la timeline sans taire le miniplayer aurait laissé deux sources jouer ensemble — pire que le démarrage qu'on voulait éviter.

- **L'icône de la chaîne du sidechain vit désormais à un seul endroit.** La fenêtre d'aide en montrait une autre : un caractère du système, `⛓`, que le bouton du clip n'avait jamais utilisé. Les deux dessins ont divergé parce qu'il y en avait deux — le second rejoint la famille des marques de transport, et la fenêtre d'aide y puise.
  - Ce qui a demandé que l'aide sache afficher un **dessin** et non seulement un mot. La plaque prend alors son encre à elle, sinon le glyphe gardait celle de la barre de transport et restait sombre sur le bouton allumé.

- **Un BPM posé à la main se distingue dans la bibliothèque**, et son infobulle donne le tempo détecté — mais seulement s'il diffère : sur une piste dont seul le premier temps a bougé, le répéter à l'identique ne dit rien.

- **La fenêtre `ABOUT` décrit la détection de tempo.** C'est ce qu'on interroge en premier quand une grille tombe à côté : d'où vient le tempo, ce qui se passe quand le modèle ne peut pas tourner, et où corriger à la main. RTen et beat-this-rs rejoignent les crédits, avec l'attribution à l'Institute of Computational Perception de JKU Linz.

- **L'icône du programme passe au bleu de l'interface.** Le trait était turquoise — la couleur des touches `VOX`/`MUS`. L'icône est la marque du programme entier, pas d'une de ses fonctions; elle prend donc le bleu de la pastille du menu et des chiffres du résumé. Tout le jeu d'icônes a été régénéré, sans quoi seule la source vectorielle aurait changé.

## 2026-07-28

- **Portable quotidien autonome construit.**
  - `MixCanvas-0.0.17-BPM3-portable.exe` est compilé en release avec LTO et
    `embed-resources`; il contient le modèle de stems, les deux DLL ONNX ainsi
    que les deux modèles Beat This BPM/downbeat.
  - Taille : 65 292 288 octets (62,27 Mo). SHA-256 :
    `b43450d3f094066ffe7b91ea4e32e2ac2cda8a103246e85093260c22bdb0034f`.
  - Un lancement de contrôle est resté vivant huit secondes puis s'est fermé
    proprement. Le test `embed-resources` vérifie maintenant explicitement la
    présence et la taille des cinq ressources déballées, y compris les deux
    nouveaux modèles BPM.

- **La beatgrid automatique passe à l'algorithme 3.** L'autocorrélation maison
  reste un secours, mais la source principale est désormais le petit modèle
  Beat This! suivi d'un ajustement de grille DJ propre à MixCanvas.
  - Les anciennes valeurs manuelles de la base n'ont pas été prises pour une
    vérité terrain : le contrôle a été refait au Tap Tempo par l'auteur du
    projet. Les références approximatives sont Bicep 120, *A Bit of Nostalgia*
    127, *Hälo* 115 et *Self Control* 126 BPM.
  - Les modèles Beat This petit et complet ont tous deux proposé environ
    120/125/115,39/125. Le modèle complet de 83 Mo demandait 31 à 52 secondes
    par piste sans améliorer ces quatre décisions; il a été rejeté.
  - Sonara 0.3.5, évalué séparément, a produit
    123,05/123,05/116,71/126,01. Son analyse est rapide, mais moins proche des
    taps sur ce petit corpus; il n'a pas été intégré.
  - La médiane brute des événements Beat This n'était pas assez précise non
    plus. MixCanvas ajuste maintenant une période et une phase rigides aux
    événements sur tout le morceau : votes de paires à 2–16 secondes, médiane
    pondérée, phase circulaire à la milliseconde, index de beat indépendant,
    puis régression avec rejet robuste à 50/30/20 ms. Un événement manquant ou
    doublé ne décale jamais ceux qui suivent.
  - Le même code de production donne **120,000**, **125,999**, **116,417** et
    **125,994 BPM** sur Bicep, *A Bit of Nostalgia*, *Hälo* et *Self Control*.
    Les erreurs RMS de la grille ajustée mesurées durant l'audit vont d'environ
    1,8 à 9,2 ms. Le BPM est conservé au millième.
  - Les downbeats du modèle votent pour l'une des quatre phases de mesure; la
    détection de grave existante choisit ensuite le premier `1` musical après
    l'introduction. Les sorties observées sont 14,020 s, 30,499 s, 30,952 s et
    2,044 s. Celle de Bicep diffère de l'ancien 12 s et doit être validée à
    l'oreille plutôt que forcée arbitrairement.
  - Trois tests nouveaux couvrent les événements manquants/doublés, les faux
    downbeats et le choix de mesure près du seuil d'entrée de groove.

- **Snap to kicks devient Snap to beat et ajuste enfin une vraie grille.**
  - Le Tap Tempo donne seulement l'ordre de grandeur au modèle; la même grille
    robuste que l'analyse automatique calcule le BPM exact.
  - Une correction de BPM proche de l'analyse conserve maintenant la phase
    connue et quantifie la position capturée sur la grille corrigée. C'était le
    chaînon manquant : auparavant toute différence de plus de 0,001 BPM
    désactivait silencieusement le snap.
  - Une réinterprétation radicale — demi-temps, double-temps ou plus de 15 % —
    garde volontairement la position capturée telle quelle, car choisir quel
    ancien beat devient le nouveau `1` est alors une décision musicale.

- **Les modèles d'analyse voyagent avec MixCanvas.**
  - `beat_this_small.onnx` (10,6 Mo) et `mel_spectrogram.onnx` (271 ko) sont
    versionnés dans `src-tauri/resources/models` et inclus par le paquet Tauri.
    RTen fonctionne en Rust pur : aucune installation Python, DLL, connexion ou
    dépense externe.
  - Seule la chaîne RTen est optimisée dans le profil de développement. Une
    première version optimisait toutes les dépendances et faisait recompiler
    Tauri inutilement; elle a été resserrée avant livraison.
  - `THIRD_PARTY_NOTICES.md` conserve la licence MIT de Beat This! et
    beat-this-rs ainsi que les SHA-256 exacts des deux modèles.
  - L'outil `examples/analyze_tracks.rs` réutilise l'analyseur de production et
    accepte désormais le dossier de ressources en premier argument.
  - La version d'analyse passe de 2 à 3 des deux côtés de l'IPC. Les caches
    anciens seront réanalysés; les corrections manuelles restent prioritaires
    et peuvent être retirées avec `Restore Automatic`.

- **Vérification de la nouvelle analyse.** `check.cmd` passe : 23 fichiers /
  179 tests frontend, 142 tests Rust réussis et 4 mesures ou intégrations
  ignorées explicitement, formatage et Clippy propres. La construction Vite de
  production passe également. Une première exécution de `check` a correctement
  attrapé l'ancien exemple `analyze_tracks` puis un test dont l'attente
  contredisait la nouvelle règle de snap; les deux ont été corrigés avant la
  passe verte.

- **Nouvel audit de référence après le renommage en MixCanvas.** Aucun
  comportement applicatif n'a été modifié pendant cette passe; l'objectif était
  de reconstruire une carte fiable du projet avant le prochain diagnostic.
  - `architecture.md` décrit maintenant les responsabilités actuelles du
    frontend, du backend Rust, du moteur audio, de la persistance et des stems,
    ainsi que le risque créé par les quatre très gros modules.
  - L'historique SQLite est remis à niveau jusqu'au schéma **24** : panoramique,
    stems par morceau, remplacement par des stems limités au clip, puis
    waveforms propres aux stems.
  - La liste « hors portée » ne présente plus la séparation en stems ni les
    traitements internes déjà livrés comme des fonctions futures.
  - `README.md` reflète maintenant les projets `.mixcanvas`, le bounce,
    l'Undo/Redo, le panoramique, les stems et les véritables barres de LED du
    master.
  - `handoff.md` devient la photographie vérifiée du 2026-07-28 : schéma 24,
    format de projet 1, 50 commandes Tauri, 23 fichiers / 179 tests frontend,
    139 tests Rust réussis et 4 ignorés explicitement.
  - `tsc --noEmit`, Vitest, `cargo test`, `cargo fmt --check`, Clippy avec
    `-D warnings` et la construction de production Vite passent. Une première
    tentative trop courte a fermé le flux de sortie de Vitest et produit
    `EPIPE`; la vérification complète relancée sans cette interruption est
    verte.

- **Un seul fichier, rien à installer.** La bibliothèque ONNX et le modèle de séparation entrent désormais **dans** l'exécutable : le portable ne traîne plus de dossier `resources` à côté de lui, qu'il suffisait d'oublier en le copiant pour que `VOX`/`MUS` cesse de fonctionner sans dire pourquoi.
  - Les deux fichiers sont déposés dans le dossier de données au premier usage, pas à chaque lancement : trente-cinq mégaoctets recopiés à chaque démarrage seraient payés pour rien. La taille sert de contrôle — une copie tronquée par un disque plein se refait au lancement suivant, plutôt que de faire échouer la séparation.
  - Ce n'est **pas** le comportement par défaut, mais l'option de compilation `embed-resources`. Les deux fichiers ne sont pas versionnés — ils pèsent trente-cinq mégaoctets — et un dépôt fraîchement cloné doit rester constructible sans eux. Le dossier posé à côté de l'exécutable reste cherché en premier : un développeur qui remplace le modèle n'a pas à recompiler.

- **Le time-stretch étire enfin au lieu de transposer.** Trois corrections, mesurées à chaque étape sur une nappe rendue hors ligne par le moteur complet.
  - **Le fondu ne couvre plus tout le pas** mais un raccord de 256 images. Mélanger en permanence deux flux dont l'écart croît revient à lire à leur vitesse moyenne : c'était un rééchantillonnage déguisé, et c'est ce qui faisait suivre la hauteur au tempo.
  - **La recherche de raccord couvre toute son étendue** — 512 images — au lieu d'être déduite de la correction à faire, qui donnait ±21 images pour sept battements d'écart. Recaler la phase d'un grave à 110 Hz en demande 400 : le recalage était impossible par construction.
  - **Le grain passe de 512 à 2048 images.** Quatre-vingt-six raccords par seconde laissaient chacun leur trace sur un son tenu; il en reste vingt-et-un. La fenêtre de comparaison suit, à cinq périodes d'un grave.
  - Résultat, à 130 → 123 BPM : les raies reviennent à leur place — 110/129/164/221/264/328 pour une source à 110/131/165/220/262/330 —, et **l'énergie parasite tombe de 58 % à 2 %**, soit vingt-huit fois moins. L'enveloppe ne se creuse plus.
  - **Rien coûté aux attaques** : un train de frappes rend le même compte avec l'ancien et le nouveau grain. Un second test le vérifie, pour qu'on n'échange pas un défaut contre un autre.

- **Le time-stretch transpose au lieu d'étirer** — mesuré sur une nappe, à travers le moteur complet et le rendu hors ligne. À 130 → 123 BPM les raies descendent de 110/131/165 Hz à 105/124/156, soit un rapport de 0,948 : exactement 123/130. La hauteur suit le tempo. Un morceau joué presque un demi-ton en dessous des autres, c'est la dissonance entendue.
  - **58 % de l'énergie sortante** est à des fréquences absentes de la source; à taux 1 le moteur est transparent au centième de décibel près. La chaîne va donc bien, le défaut est dans la granulation.
  - Deux causes identifiées, notées dans `handoff.md` : le fondu couvre **tout** le pas — mélanger en permanence deux flux dont l'écart croît revient à lire à leur vitesse moyenne —, et le rayon de recherche est tiré de la correction à faire, soit ±21 échantillons là où recaler un grave de 110 Hz en demande 400.
  - Le test qui l'a établi reste comme instrument de la correction : il mesure les raies dominantes, l'énergie hors raies et le creusement de l'enveloppe, en comparant toujours à la source plutôt qu'à une platitude théorique — un accord de partiels non harmoniques bat de lui-même, et ma première version accusait le moteur de ce que le signal faisait tout seul.

- **Le recalage WSOLA est mesuré, et il est juste.** Cherchant l'origine d'un rendu « granuleux » au-delà d'une dizaine de battements d'écart, j'ai soupçonné la corrélation : elle ne lit qu'un échantillon sur huit, et dans l'aigu une période ne fait plus que quinze échantillons. Mesure faite, elle ne s'égare pas — **moins de dix degrés d'erreur de 110 Hz à 3 kHz**. L'hypothèse était fausse.
  - Le diagnostic devient un garde-fou plutôt que d'être jeté : la mesure est désormais une assertion, et un sous-échantillonnage plus grossier ou une recherche plus courte la feraient tomber. Le test voisin tolérait huit échantillons d'écart sur une période de cent — 28° — ce qui laissait passer précisément ce qu'on soupçonnait.

- **Les boutons d'un clip ne répondaient plus quand le crayon était armé.** Le trait capture le pointeur sur la voie dès l'appui, y compris lorsque cet appui naît sur `EQ`, `VOX`, `MUS`, la chaîne ou la croix : le clic n'aboutissait jamais. Les boutons semblaient morts sans qu'on voie pourquoi — il fallait deviner que le crayon était en cause, et l'éteindre pour retrouver la fenêtre d'égalisation.
  - Le trait laisse désormais passer ce qui naît dans la barre d'un clip. Le corps du clip reste dessinable, ce qui était le but de ce mode : seules les commandes s'en extraient. Le geste de sélection le faisait déjà pour ces mêmes boutons; le crayon l'avait oublié.

- **Le gain d'un clip se tape autant qu'il se glisse.** Le curseur avance par demi-décibels et impose de viser; au clavier on écrit `-4,5` et c'est réglé. La valeur affichée est devenue le champ lui-même, avec un creux qui n'apparaît qu'au survol — au repos la case reste une lecture, comme les autres valeurs de la fenêtre.
  - La saisie n'est appliquée **qu'à la validation**. Chaque frappe intermédiaire d'un `-12` passerait sinon par `-`, puis `-1`, que le moteur jouerait au vol.
  - Une saisie qui ne veut rien dire **laisse le réglage tel quel**. Un champ qui répondrait zéro à une frappe malheureuse couperait le clip sans prévenir. `Échap` abandonne la saisie, `Entrée` la valide.
  - Ce que la case accepte vit dans `parseClipEqGainDb`, testé à part : les formes évidentes, la **virgule décimale** d'un clavier français, l'unité `dB` qu'on recopie depuis l'affichage, `-inf` et `-∞` pour le silence, et le vrai **signe moins d'Unicode** que colle un traitement de texte. Un test a d'ailleurs attrapé le pire des cas — `Number("")` valant zéro, un `+` seul poussait le gain à +12 au lieu d'être refusé.

- **La fenêtre `ABOUT` demande une attribution en cas de fork.** Elle distingue ce que la licence **exige** — conserver les mentions, signaler ce qu'on a modifié — de ce qui est **demandé** : nommer MixCanvas et renvoyer au projet d'origine. Écrire « une attribution serait appréciée » sans cette distinction ferait passer une obligation pour une faveur, et l'auteur y perdrait ce que l'AGPL lui accorde déjà.

- **Cliquer le nom d'un morceau l'écoute**, comme la flèche au bout de sa rangée. C'est le geste qu'on tente en premier devant une liste de musique, et il ne faisait rien.
  - Le nom garde l'aspect d'un titre — c'en est un — et ne se donne qu'au survol, par un **soulignement** plutôt qu'un changement de couleur : cette liste se dessine sur deux fonds selon la disposition, clair dans l'application et sombre en panneau seul, et une couleur juste sur l'un disparaît sur l'autre. Le premier essai était blanc, invisible sur la rangée claire.
  - Il reste atteignable au clavier, se déclenche sur Entrée comme sur Espace, et se retire du parcours quand le morceau est absent ou l'écoute indisponible.

- **Le rappel de `VIEW` et `DRAW` ne s'affichait pas du tout.** La touche porte `overflow: hidden` — c'est ce qui tient sa brillance — et son cadre s'arrête 8 px au-dessus de là où le panneau s'ouvre : il était calculé, positionné, et intégralement rogné. Les mesures de position ne l'avaient pas vu, une boîte rognée ayant toujours ses coordonnées.
  - Chaque touche concernée est désormais entourée d'une ancre qui ne rogne rien et épouse sa taille. Vérifié : les six touches font toujours 54 px, la plaque reste alignée avec le VU-mètre, et plus aucun des onze rappels ne sort du cadre.

- **`VIEW` et `DRAW` portent le même rappel**, avec leurs raccourcis : `E` pour l'un, `S` et `D` pour l'autre. Le panneau s'ouvre **sous** la touche et centré sur elle — le rail est horizontal, et ouvrir par le côté aurait fait sortir celui de la dernière touche de la fenêtre.
  - **Neuf dixièmes de seconde avant qu'il paraisse**, contre un tiers pour les repères de voie. Ces deux touches sont sous la main toute une séance, et leur rail est traversé cent fois pour atteindre `PLAY` : un délai court y ferait clignoter une plaque en permanence. Là, elle ne se montre qu'à qui s'arrête pour la lire.
  - Le panneau **laisse passer les clics**. Ouvert, il recouvre la timeline; sans cela il aurait volé le clic destiné à ce qu'il cache. Vérifié : `DRAW` continue de faire défiler ses formes.

- **`M` et `S` s'allument au survol comme le `F`.** Ils n'avaient aucun état de survol — seul le `F` répondait, ce qui les faisait passer pour des objets de natures différentes alors qu'ils forment une colonne. Même plaque sombre, même lettre claire pour les trois.
  - Une piste **déjà coupée ou solo garde sa couleur** : elle annonce un état, pas une invitation. Le survol s'y contente d'un éclat, sans effacer ce qu'elle dit.
  - Le curseur, lui, reste différent à dessein : main sur `M` et `S`, point d'interrogation sur le `F`. C'est la seule chose qui prévienne qu'un clic sur le `F` ne fera rien.

- **`M` et `S` portent le même panneau que `F`.** Les trois repères flottants de la colonne partagent désormais un seul rappel, même plaque et mêmes touches dessinées — c'était trois affordances de même forme expliquées de trois façons.
  - Chacun précise que le raccourci vise la piste **sélectionnée** et non celle qu'on survole : c'est la nuance qui compte, et la croire fausse coûte une piste coupée par erreur au mauvais moment. Cliquer le bouton sélectionne justement sa piste, donc les deux gestes s'enchaînent.
  - **Le panneau attend un tiers de seconde avant de s'ouvrir**, et se ferme aussitôt. Sans ce délai, `M` et `S` — pressés cent fois par séance — feraient surgir une plaque à chaque passage de souris : le rappel deviendrait la gêne qu'il devait éviter. Là, il ne se montre qu'à celui qui s'arrête pour chercher.
  - Chaque rappel s'ouvre **vers l'espace libre de sa voie** : le `F` vers le bas, puisqu'il siège dans le premier tiers de la paire; `M` et `S` vers le haut, parce que vers le bas ceux de la dernière piste sortaient du corps de la timeline, que `overflow: hidden` rogne. Mesuré aux neuf panneaux : aucun ne déborde, le plus juste à cinq pixels du fond.

- **Un repère `F` au centre de chaque bande de filtre**, dans la colonne de `M` et `S` et taillé comme eux. La bande ne disait pas d'elle-même qu'on peut y dessiner, ni que deux modificateurs changent le trait : au survol, un rappel liste les cinq gestes — glisser, `Shift` pour un triangle, `Ctrl` à main levée, glisser un bord pour redimensionner, clic droit pour effacer.
  - Ce n'est **pas une commande** : il ne fait rien au clic et porte un curseur d'aide. Il reste atteignable au clavier, le rappel s'ouvrant aussi au focus.
  - La lettre est un `F` ordinaire, dans la police de `M` et `S`. Un `ƒ` ressortait plus gros et décentré : il porte une hampe et un jambage là où une capitale n'a ni l'un ni l'autre, et aucune taille ne le recentre sans un calage propre à une police, qui casserait au premier repli. Surtout, `M` et `S` sont les initiales de Mute et Solo — `F` pour Filter suit la même grammaire, là où `ƒ` est un signe d'une autre nature. Mesuré : les trois glyphes ont la même encre, 5 px de large, et le même décalage optique de −0,3 px.
  - Le rappel s'ouvre **vers le bas** et non centré sur le repère : centré, celui de la première voie remontait de soixante-six pixels et passait au-dessus du corps de la timeline, que `overflow: hidden` rogne — il aurait été invisible précisément là où un nouvel utilisateur regarde en premier. Mesuré aux trois voies : les trois tiennent, la dernière à cinq pixels du fond.
  - Contraste 11,6:1 pour le texte, 15,4:1 pour le titre.

- **La ligne de volume occupe enfin toute la voie** — 85 % de sa hauteur au lieu de 43 %. Le plafond de +12 dB tombait au tiers, et tout le bas ne servait à rien : un même glissé devait résoudre le double d'amplitude par pixel. La course passe de 46 à 91 unités.
- **Le silence arrive à −40 dB, plus à −60.** Entre les deux il n'y a rien à entendre dans un mix : c'était de la course perdue, prise sur la partie de l'enveloppe où le travail se fait. Le moteur accepte toujours jusqu'à −60, donc un projet ancien portant de telles valeurs les joue sans broncher — il ne peut simplement plus les redessiner.
  - Les deux effets se cumulent : **0,26 dB par unité dans le gain** au lieu de 0,52, et **0,88 dans la coupe** au lieu de 2,6. Trois fois plus fin là où l'on baisse une piste sous une voix.
  - Le silence reste **dessiné au plancher**, et non dans une bande à part. Les séparer aurait fait sauter de six unités tout nœud posé au plancher dès qu'on le saisit — exactement le défaut qui a fait naître ce module. Il arrive donc plus tôt par la valeur, pas par la position.
  - Les bornes des formes dessinées **suivent désormais celles de la ligne** au lieu d'en tenir une copie écrite en dur : le plancher y était resté à −60, et une forme pouvait descendre sous ce que la ligne sait redessiner.

- **Un stem était muet sur tout ce qui précède le premier temps du morceau.** La fenêtre à séparer était ancrée sur ce premier temps, alors qu'un clip fait entendre aussi ce qui le précède — le pré-roll, que `duration_beats` compte. Sur *Happier Than Ever*, dont le premier temps détecté tombe à **2 min 46**, les deux premières minutes du clip devenaient silencieuses, et la forme d'onde le montrait fidèlement : le dessin ne mentait pas, c'est le fichier qui manquait.
  - La règle est sortie de la commande dans `clip_source_window_ms`, avec le cas réel en test — premier temps à 165 689 ms, fenêtre qui doit partir de zéro. C'est la deuxième fenêtre de ce module à se tromper d'origine; celle-ci se vérifie maintenant sans lancer de rendu.
  - Le diagnostic est venu de la base plutôt que du raisonnement : `source_from_ms` valait 161 689, soit le premier temps moins la marge. Deux minutes de trop.
  - Un second test, sur le vrai modèle, vérifie qu'une séparation fenêtrée couvre bien ses seize secondes, qu'aucun des deux stems ne s'éteint sur un huitième de sa durée, et qu'ils diffèrent réellement l'un de l'autre.

- **L'Undo perdait toujours la forme d'onde du stem** — après un filtre, un panoramique ou un volume indifféremment. Le geste n'y était pour rien : `restore_snapshot` remplaçait **tous** les clips, et `clip_stems` tombait en cascade avec eux. Le sauvetage posé la veille nommait ses colonnes une par une, donc il en oubliait sept sur onze; le stem revenait, son dessin non, et le clip se remettait à montrer le mix complet en jouant la voix.
  - La correction ne consiste pas à allonger cette liste. **Les clips ne sont plus remplacés, ils sont corrigés** : seuls ceux absents de l'état restauré sont supprimés, les autres sont mis à jour en place. Rien de ce qui pend aux clips n'est plus touché — ni aujourd'hui, ni par la prochaine table qu'on leur rattachera. C'est la troisième fois que ce défaut se manifeste par ce chemin; cette fois la porte est fermée, pas rebouchée.
  - Le test pose les **trois** lignes d'automation et annule chacune, plutôt que celle par laquelle le défaut s'est manifesté. Il vérifie le fichier du stem, son décalage de source et sa forme d'onde.

- **`Tracks` et `Total Time` passent sous le VU-mètre**, et les quatre ensembles de l'en-tête partent enfin de la même ligne. Ce décompte était **au-dessus** de la plaque : il la poussait vers le bas, et l'empêchait de commencer là où commencent `BOUNCE MIX` et le BPM.
  - Sous la plaque, il occupe la place que `BOUNCE MIX` prend à côté — de la hauteur qui ne servait à rien. Mesuré : transport et VU-mètre de 18 à 82, `BOUNCE MIX` et BPM de 18 à 100, le décompte de 85 à 95.
  - Les deux blocs se calent désormais **par le haut** et non par le bas. Ce qui est plus court laisse sa place en dessous, où le regard ne la cherche pas, plutôt qu'au-dessus, où elle creusait un trou entre les commandes.
  - Vérifié à 1600, 1280 et 1024 px : l'alignement tient, et l'en-tête passe à la ligne sans déborder quand la fenêtre ne suffit plus.

- **Les touches de transport s'allongent de 44 à 54 px**, ce qui met leur plaque au gabarit exact du VU-mètre : les deux blocs vont maintenant de 31 à 95, mesuré. Retirer `CLEAR TIMELINE` avait raccourci cette colonne de dix pixels, et les deux plaques se regardaient sans se répondre.
  - La hauteur seule ne suffisait pas : chaque bloc étant centré dans son coin, et le VU-mètre portant son libellé au-dessus, leurs centres ne coïncidaient pas. `.timeline-identity` s'étire donc sur la ligne et se cale **par le bas**, ce qui pose les deux plaques sur le même trait.
  - L'alignement a dû être écrit dans la **dernière** règle qui parle de ce bloc : une première tentative n'a rien fait, une autre règle plus bas dans le fichier l'emportant en silence. Le même piège que pour la marge de l'en-tête de bibliothèque la semaine dernière.
  - Vérifié aussi à 1024 px : la rangée passe à la ligne sans déborder, et les plaques restent alignées.

- **`CLEAR TIMELINE & LIBRARY`**, à côté du précédent : il vide la session entière, morceaux de la bibliothèque compris. La commande arrête le moteur avant d'écrire, comme au chargement d'un projet — vider la base sous une lecture en cours reviendrait à retirer le plan des mains de ce qui le joue.
  - Il ne passe **pas par l'historique**, et le vide : ce qu'il détruit dépasse ce qu'un Undo sait rendre, puisque les morceaux de la bibliothèque n'y ont jamais figuré. Empiler une entrée promettrait un retour en arrière qui n'aurait pas lieu.
  - Ce qui ne descend pas des morceaux — automations de voie, mute et solo — est effacé **à part**. Les clips partent en cascade, ces réglages n'ont aucune clé étrangère vers eux, et les laisser derrière donnerait une timeline vide portant encore les gestes de la précédente. C'est l'oubli que ce projet a déjà commis deux fois; un test l'interdit maintenant, table par table.
  - Deux boutons rouges côte à côte doivent se distinguer d'un coup d'œil, sans quoi le plus grave se presse à la place de l'autre : le second est en rouge plein là où le premier n'a qu'une bordure. La confirmation dit ce qui part **et ce qui ne part pas** — on hésite surtout parce qu'on croit risquer ses fichiers, et ils ne sont jamais touchés.
  - Mesuré : trois boutons de 26 px dans un pied de page de 821 px, sans débordement ni chevauchement avec l'astuce et `CLOSE GUIDE`.

- **`CLEAR TIMELINE` quitte la rangée de transport pour le bas de l'aide**, à droite d'`ABOUT`. C'est le seul geste du programme qui détruise du travail, et il vivait à portée du pouce entre des commandes qu'on presse cent fois par séance. L'atteindre demande maintenant d'ouvrir une fenêtre — exactement la friction qu'il mérite, et la confirmation reste par-dessus.
  - Même gabarit qu'`ABOUT` — hauteur, corps, interlettrage — mais en rouge : il partage sa place, pas sa nature. La bordure suffit à le distinguer au repos, le fond ne s'allume qu'au survol, une fois l'intention manifeste. Mesuré : 26 px tous les deux, 8 px d'écart.
  - Il se grise quand la timeline est déjà vide, et le dit — « The timeline is already empty ».
  - **Les six touches de transport restent alignées** : la rangée se cale par le bas, donc retirer le bouton qui la coiffait n'a pas décalé `PLAY` et `PAUSE` par rapport à `COMP`, `LIMIT`, `VIEW` et `DRAW`. Vérifié : un seul bord inférieur pour les six.

- **Le programme s'appelle MixCanvas.** BeatForge existait déjà — trouvé avant le premier commit, ce qui est le bon moment. « Mix » plutôt que « Beat » parce que ce n'est pas un outil de création de rythmes mais un éditeur de mix, et parce que le préfixe `Beat*` est le plus encombré du logiciel musical : c'est lui qui a causé la collision. « Canvas » parce que c'est devenu littéralement vrai — un crayon qui dessine des formes d'automation, un tracé libre pour le filtre, des lignes qu'on peint à la main.
  - **L'identifiant de paquet devient `ca.mixcanvas.app`**, et `adopt_legacy_library` connaît désormais **deux** anciens dossiers de données. Elle cherche `ca.beatforge.app` avant `ca.ezdj.app` : une installation passée par les trois porte les deux, et c'est le plus récent qui contient le travail à jour. Un test le vérifie en posant les deux et en exigeant que ce soit le bon qui gagne.
  - **L'icône n'avait jamais été régénérée** au renommage précédent : le SVG portait le monogramme de BeatForge, mais les PNG et l'ICO dataient du 18 juillet et montraient encore le lettrage d'EZ-DJ. C'est ce que la barre de titre affichait depuis. Le jeu complet est refait à partir d'un dessin neuf — une courbe d'automation turquoise tracée sur un cadre de toile, ses deux nœuds, et les formes d'onde en dessous. Turquoise comme les touches `VOX`/`MUS` : l'icône appartient à la palette de l'interface.
  - Les entrées passées du journal qui **racontent** le premier renommage gardent leurs noms d'alors. Un journal qui se réécrit à chaque changement de nom cesse d'être un journal.
  - Le fichier de projet passe de `.beatforge` à `.mixcanvas`.
  - **Le remplacement global s'est retourné contre lui-même** : il a réécrit `ca.beatforge.app` jusque dans la liste des identifiants *hérités*, qui s'est donc mise à contenir l'identifiant courant. Une bibliothèque venue de BeatForge n'aurait pas été reprise — le dossier de données aurait paru vide. Le test ne l'a pas vu parce qu'il désignait ses deux dossiers par cette même liste : l'ancien et le courant n'en faisaient plus qu'un, et l'adoption réussissait sans rien faire. Il affirme désormais qu'aucun identifiant hérité n'est celui d'aujourd'hui.

- **L'aide est reclassée par le geste**, non par le sujet. L'ancien découpage — Transport, Navigation, Editing, Controls & FX — versait dix-sept entrées dans la dernière catégorie : touches, glissés et boutons mélangés, sans qu'on sache où chercher. On n'ouvre pas cette fenêtre en se demandant « quelle est la catégorie de ce que je veux faire », mais « qu'est-ce que je peux appuyer, glisser, cliquer ». Trois familles — **clavier, souris, boutons** — et chaque entrée n'appartient qu'à une seule.
  - **Les descriptions s'alignent enfin.** Chaque rangée portait sa propre boîte, sa touche à gauche et son texte poussé à droite : quarante boîtes empilées, et pas deux descriptions qui commençaient au même endroit — l'œil ne pouvait pas balayer la colonne. Une grille à colonne fixe remplace tout ça, les rangées se contentant d'un filet. Mesuré : les 39 descriptions partent du même pixel.
  - Onze petits paquets nommés à l'intérieur des trois familles, la fenêtre passe de 640 à 840 px, et les touches `VOX`/`MUS`, `SAVE`/`LOAD` — jamais documentées — y figurent enfin.

- **Fenêtre `ABOUT`**, atteinte depuis le bas de l'aide. Elle porte l'auteur et son courriel, la licence du programme expliquée en clair plutôt qu'en renvoi, les douze bibliothèques embarquées avec leur licence, et ce que le programme fait de la musique — rien ne quitte la machine.
  - Les licences sont **relevées des manifestes**, `cargo metadata` et `package.json`, pas écrites de mémoire : une liste approximative serait pire que pas de liste, puisque c'est un document légal. La MPL-2.0 de Symphonia est signalée à part, parce qu'elle oblige à quelque chose que les autres n'obligent pas.
  - `Esc` n'appartient qu'à la fenêtre du dessus. Les deux se fermaient d'une seule pression. On aurait pu s'en remettre à l'ordre des écouteurs — capture avant bulle — mais c'est un raisonnement qui se casse dès qu'on déplace une ligne : un drapeau explicite le dit. Vérifié à l'écran sur les quatre étapes.

- **Cinq commandes Tauri enregistrées et jamais appelées sont retirées** : les quatre qui posaient, déplaçaient, supprimaient ou courbaient un **nœud de filtre isolé**, et `set_project_bpm`. Le pinceau et le tracé libre écrivent des plages entières depuis longtemps, et le tempo se règle par ses points; ces portes ne menaient plus nulle part. Chacune restait pourtant appelable par tout ce qui atteint le pont IPC — c'est de la surface d'attaque, pas seulement du désordre. Il en reste 48.
  - Les fonctions de `timeline.rs` derrière elles partent aussi, et avec elles la portion de test qui les exerçait : vérifier un geste que l'interface ne permet plus, c'est tester du code mort. L'assertion de persistance qui s'appuyait dessus vise désormais la courbe telle que le pinceau la pose.
  - `add_filter_node` survit en `#[cfg(test)]` : deux tests ont besoin de poser un nœud pour vérifier autre chose — la signature de lecture, le remplacement d'une plage.
  - Un lint de `clippy` traînait dans le correctif de la veille, jamais vu parce que la DLL verrouillée par l'application faisait échouer la compilation avant lui.

## 2026-07-27

- **Un Undo détruisait les stems.** `restore_snapshot` remplace tous les clips par un `DELETE` suivi d'`INSERT`, et `clip_stems` part en cascade avec eux. Annuler un panoramique effaçait donc des fichiers déjà rendus, et le clip retombait sur le morceau complet — ce n'est pas le panoramique qui était annulé, c'était la séparation.
  - Deux oublis dans la même fonction : la colonne `stem` ne figurait pas dans son `INSERT`, et les lignes de `clip_stems` n'étaient pas préservées. **C'est exactement le défaut qui revient dans ce projet** — une colonne ajoutée sans visiter les deux endroits qui écrivent la timeline. Le troisième cas en un mois.
  - Les stems sont maintenant relevés avant l'effacement et reposés après. Ceux dont le clip a disparu de l'état restauré s'en vont avec lui : leur fichier ne désigne plus rien.

- **Séparer un clip rogné donnait un stem muet ou décalé**, alors que la forme d'onde restait juste — on n'entendait pas ce qu'on voyait.
  - Le décalage de la fenêtre était retiré du **premier temps**, qui devenait négatif sur un clip déjà rogné : la fenêtre commence alors bien après le premier temps du morceau. Le borner à zéro faisait lire le moteur des secondes trop loin dans un fichier qui ne les contient pas.
  - Il est désormais retiré du **rognage**, qui contient déjà ce décalage par construction et reste donc positif. `premier temps + (rognage − décalage)` donne exactement la même position, sans jamais passer sous zéro.

- **Scinder un clip séparé donnait une moitié droite sans stem.** Les stems sont rattachés au clip, et la scission crée un clip neuf qui n'en héritait pas : la nouvelle moitié retombait sur le morceau complet **tout en gardant sa touche allumée**, si bien que l'affichage et le son se contredisaient.
  - Les deux moitiés partagent désormais le même fichier : elles viennent de la même source, et le stem couvre déjà l'étendue qu'elles se partagent. Le décalage de source voyage avec, sans quoi la moitié droite jouerait à côté de sa grille. Un test couvre les deux points.

- **La forme d'onde du stem remplace celle du mix.** Un clip qui joue la voix en montrant la forme d'onde du morceau entier ment sur ce qu'on entend.
  - Les crêtes sont calculées **à l'échelle du morceau**, pas de la fenêtre séparée : rangées aux mêmes cases que celles de la source, silencieuses en dehors. Des crêtes calculées sur la seule fenêtre s'étaleraient comme si elles couvraient tout le morceau, et le dessin ne correspondrait plus au son. Le silence hors fenêtre est honnête — c'est exactement ce que le clip jouerait si on l'allongeait.
  - Le `snapshot` les fait gagner par un `COALESCE` sur une jointure `clip_stems`, donc **aucune ligne à changer côté affichage** : toute la géométrie existante — rognage, durée, position — reste valable.
  - Schéma 24 : sept colonnes de plus sur `clip_stems`.

- **La barre restait figée à zéro** pendant tout le début du rendu. Le décodage traverse le MP3 avant que la première tranche n'existe, et sur un morceau de six minutes il dure plus longtemps que la séparation elle-même — il n'en rapportait rien.
  - Il **rapporte maintenant son avancement**, et occupe le premier quart de la barre. Une barre qui reste à zéro une minute puis file en dix secondes ne renseigne sur rien : le décodage est une vraie partie du travail, il occupe une vraie partie de la barre.
  - Il **s'arrête à la fin de la fenêtre** au lieu de lire la queue du fichier pour rien. Sur un clip pris au début d'un long morceau, c'est l'essentiel du temps qui disparaît.

- **La séparation redescend du morceau au clip.** Séparer six minutes de musique pour huit mesures utilisées, c'était payer vingt fois le travail nécessaire — insoutenable sur une timeline de deux heures. Le rendu ne traite plus que la fenêtre que le clip fait entendre, marge de quatre secondes comprise.
  - La marge a deux raisons : le modèle décide mieux avec du contexte autour de ce qu'il sépare, et un rognage repoussé de quelques mesures après coup ne doit pas retomber dans le silence.
  - **La difficulté était la géométrie.** Un clip calcule sa position depuis le fichier source — durée, premier temps, ancre, rognage. Un stem qui ne couvre qu'une fenêtre ne commence pas au même endroit, et tout ce calcul se décalerait. Le stem retient donc `source_from_ms`, l'instant de la source où il commence, et le plan de rendu recule le premier temps d'autant. Le clip ne bouge pas d'un pouce.
  - Schéma 23 : `track_stems` devient `clip_stems`, rattachée au clip et supprimée avec lui.

- **Deux défauts, dont le second cachait le premier.** Cliquer sur `VOX` renvoyait « This track has not been separated yet », alors que c'est précisément ce que le clic devait déclencher.
  - `runTimelineEdit` **attrape** l'erreur, affiche la bannière et renvoie `false` — que la séquence ignorait. La séparation échouait, la bascule s'exécutait quand même, et son message remplaçait celui de la séparation : on voyait la conséquence, jamais la cause. La séquence s'arrête maintenant sur le premier échec.
  - La vraie cause : Tauri copie les ressources dans `target/debug/**resources**/`, un niveau plus bas que `resource_dir()`. La recherche essaie désormais le dossier du paquet, celui de l'exécutable et l'arborescence du dépôt, et **l'erreur énumère ce qu'elle a cherché et où** — un chemin manquant se diagnostique en le lisant, pas en le devinant. L'ancien message accusait l'installation d'être incomplète alors que rien n'était cassé.

- **Le premier clic sur `VOX` ou `MUS` sépare le morceau**, et la fenêtre de progression du bounce s'ouvre — même plaque, même barre, titre « Separating stems ». Séparer puis basculer sont **un seul geste** : l'utilisateur a cliqué sur `VOX`, il veut entendre la voix; que ça demande deux minutes la première fois est un détail d'exécution, pas une seconde décision à prendre.
  - Le clip sait **avant le clic** si son morceau a déjà été séparé — `hasStems` vient du serveur. C'est ce qui distingue un basculement instantané d'un rendu de deux minutes, et l'infobulle le dit : « Separate this track, then play its vocals » plutôt que « Play the vocals ».
  - La fenêtre annonce ce que l'attente achète : « The whole track is separated, not just this clip — every clip of it will switch instantly from now on. »

- **Une vraie séparation a tourné.** Quatre secondes d'audio, deux fichiers écrits, progression jusqu'à un, en 1,7 s sur le processeur. Le test qui le prouve — `separates_a_real_file` — est marqué `#[ignore]` : `check` doit rester vert sur une machine qui n'a pas encore la bibliothèque. C'est pourtant le seul qui vérifie la chaîne entière, du décodage au fichier écrit, et il attrape ce qu'aucun test unitaire ne verrait — une bibliothèque absente, un modèle qui refuse la forme reçue, une sortie transposée à l'envers.
  - `onnxruntime.dll` et `onnxruntime_providers_shared.dll` sont dans `src-tauri/resources/`, hors dépôt et embarquées dans le paquet. La DLL du paquet DirectML se charge **sans** `DirectML.dll` : Windows en fournit une en système, et le fournisseur GPU n'est de toute façon pas encore activé côté `ort`.

- **La séparation est écrite de bout en bout.** `separate_track` décode le morceau, l'analyse par tranches de 256 trames, passe chaque tranche au modèle, applique le masque, réinverse, et écrit deux WAV 16 bits. La commande `separate_track_stems` la lance sur un fil bloquant et émet sa progression, comme le bounce.
  - **Tranche par tranche, jamais en entier.** Le spectrogramme complet d'un morceau de cinq minutes pèse deux cents mégaoctets par canal : le tenir pour n'en regarder que 256 trames à la fois aurait coûté quatre cents mégaoctets pour rien. Un test vérifie que le découpage donne le **même signal** que le traitement d'un bloc, à 1e-5 près — si le raccord laissait une marche, elle s'entendrait toutes les six secondes.
  - Les deux gardes — bibliothèque ONNX et modèle présents — sont **avant** le premier appel à `ort`, parce que celui-ci charge sa DLL paresseusement et **panique** si elle manque. Une garde placée après aurait tué le programme au lieu de rendre une erreur.
  - **44,1 kHz exigé, franchement.** Un spectre analysé à une autre fréquence range les mêmes sons dans d'autres bandes : le modèle chercherait une voix là où il n'y en a pas. Rééchantillonner à la volée abîmerait un audio destiné à être entendu dans un mix, donc on refuse avec un message clair plutôt que de livrer un stem médiocre.
  - Le `#![allow(dead_code)]` posé la veille est **retiré** : tout ce qui dormait est appelé. Les trois formes « morceau entier » qui restent sont marquées `#[cfg(test)]` — elles disent ce que les tranches doivent donner, et c'est contre elles qu'elles sont vérifiées.
  - `tauri.conf.json` embarque désormais le modèle et la DLL dans le paquet.

- **Les touches `VOX` et `MUS` sont sur les clips**, à gauche de la chaîne du sidechain. Trois états pour deux touches : rien d'allumé, le morceau entier; cliquer celle qui est allumée y revient. Les deux ensemble voudraient dire la même chose, donc c'est impossible par construction.
  - Elles sont turquoise, pas ambre : l'ambre appartient au sidechain, et deux fonctions de la même couleur sur une rangée de vingt pixels ne se distingueraient plus. Mesuré : 14 px de haut comme `EQ` et la chaîne, rangée de 111 px.
  - Le **plan de rendu lit le fichier du stem** quand le clip en joue un. Les fichiers séparés étant alignés à l'échantillon près sur l'original, l'ancre, le rognage, la grille et toute l'automation restent valables tels quels — le clip ne bouge pas d'un pouce.
  - `set_clip_stem` **refuse un stem qui n'existe pas encore** : « This track has not been separated yet ». Un clip qu'on laisserait basculer vers un fichier absent serait muet sans que rien ne l'explique. Tant que l'inférence n'est pas écrite, c'est ce message que les touches renvoient — elles montrent l'état, elles ne mentent pas dessus.

- **La transformée de Fourier de la séparation, écrite et testée.** `src-tauri/src/audio/stems.rs` : analyse en fenêtre de Hann périodique de 4096 points, avancement de 1024, et son inverse par addition-recouvrement. Quatre tests la tiennent, dont celui qui compte — un signal transformé puis réinversé revient à 1e-4 près.
  - La **somme des carrés de la fenêtre est divisée à la fin** plutôt que supposée constante. Elle l'est au milieu du morceau, elle ne l'est pas aux deux premières et dernières trames, et c'est exactement là qu'une division omise laisse un fondu que personne n'a demandé. Un test vérifie qu'un signal constant revient à son niveau **du premier au dernier échantillon**.
  - La fenêtre est **périodique**, pas symétrique. Un échantillon d'écart entre les deux suffit à casser la reconstruction : la somme des carrés cesse d'être constante et laisse une ondulation au rythme de l'avancement, qu'on prendrait pour un défaut du modèle.
  - Le masque garde la **phase du mélange** — l'approximation que fait toute séparation de ce type. Un test vérifie qu'un masque à un laisse le spectre intact, qu'un masque à zéro fait taire, et qu'un masque à moitié divise l'amplitude sans toucher à l'argument.
  - Et le test qui justifie tout le choix d'architecture : **les deux stems se resomment exactement en l'original**, puisque l'instrumental est une soustraction dans le domaine temporel et non une seconde prédiction.
  - `ort` est en dépendance en **chargement dynamique** : la DLL d'ONNX Runtime vivra à côté de l'exécutable au lieu d'être liée, ce qui est la condition d'un paquet portable.

- **Le modèle de séparation est produit et vérifié.** `open-unmix-vocals-fp16.onnx`, 17,9 Mo, autonome, licence MIT. Entrée : un spectrogramme d'amplitude `(1, 2, 2049, 256)`; FFT de 4096, pas d'avancement de 1024, fenêtre de Hann. Seule la cible « voix » est exportée — l'instrumental se calcule par différence dans le domaine temporel, ce qui garantit que les deux stems se resomment exactement et divise le fichier par deux.
  - **Demucs ne s'exporte pas en ONNX**, et ce n'est pas réparable par un réglage. Trois obstacles franchis ou constatés dans cet ordre : une assertion interne de `pad1d` qui lit une comparaison de tenseurs par `.item()`, donc une décision dépendant des données que `torch.export` refuse de tracer; le segment annoncé de 44 s, dont le graphe ne tient pas en mémoire; puis le mur — `aten.pad` sur un tenseur complexe, sans traduction ONNX possible. Sa branche spectrale est bâtie sur les nombres complexes, qu'ONNX n'a pas. Les patcher un par un reviendrait à réécrire `_spec` et `_ispec`.
  - Open-unmix passe parce que **sa transformée de Fourier vit hors du modèle** : c'est exactement ce qui bloque Demucs, et exactement ce que MixCanvas devait faire de son côté. La qualité est un cran en dessous; l'oreille tranchera, et le chemin de retour vers Demucs reste ouvert puisque toute la plomberie Rust est indépendante du modèle.
  - Le script journalise chaque échec dans `scripts/export-logs/` et n'affiche que ce qui se lit : un export ONNX raté recrache le graphe entier, des centaines de pages où la cause tient en une ligne.

- **Séparation en stems — premières fondations.** Le schéma passe en version 22 : une table `track_stems` rattache à chaque morceau ses voix séparées, et le clip ne retient que lequel il joue. La séparation appartient donc au **morceau**, pas au clip — un second clip du même morceau bascule sans rien recalculer.
  - Le décodeur accepte désormais le **WAV** en plus du MP3. Les stems sortent de la séparation en PCM et doivent rentrer par la même porte, sans quoi ils ne seraient jouables par rien.
  - `scripts/export-demucs-onnx.py` convertit les poids Demucs v4 en ONNX, à lancer une fois. Demucs plutôt qu'un `.onnx` tout fait parce que ses poids sont sous licence MIT, donc redistribuables dans un binaire public sans zone grise; le prix est cette conversion, Meta ne publiant que du PyTorch. Le script essaie trois modèles du plus capable au plus docile et s'arrête au premier qui passe la vérification — l'export est l'étape qui peut résister, `htdemucs` faisant sa transformée de Fourier dans le graphe.

- **L'icône du sidechain devient une chaîne**, deux maillons pris l'un dans l'autre, à la place de la clé de serrurier. Il fallait connaître le mot « key » pour lire le dessin, alors que la chaîne dit ce que fait le bouton.

- **Le filtre se dessine à main levée, `Ctrl` enfoncé.** La bande garde son pinceau à bulle; `Ctrl` la transforme en surface de tracé libre, et le curseur devient un crayon avant même le clic — un modificateur qui ne se voit pas se découvre par accident. Plus direct qu'une enveloppe préétablie pour un balayage qu'on entend avant de savoir le décrire.
  - Le trait **peint la valeur pointée sur chaque quart de temps parcouru**, dans l'ordre où la main les visite. Repasser sur un quart déjà peint écrase sa valeur : le geste se corrige sans se relâcher. Le quart est le pas de cette bande depuis le pinceau, et le moteur lisse la coupure sur huit millisecondes de toute façon.
  - `Ctrl` au moment du clic décide du geste, et lui seul : le relâcher en cours de route ne transforme pas un trait commencé en pinceau à bulle.
  - **Une seule fonction produit l'aperçu et la charge envoyée** — `filterStrokeNodes` —, ancres au bypass comprises. La courbe tracée sous le curseur est donc exactement celle qui sera jouée, et non une approximation qui finirait par en diverger. Le serveur écrit ce qu'il reçoit après l'avoir validé, et ne recalcule rien.
  - Ce qu'il refuse : un trait vide, un trait qui recule, une valeur hors du champ, plus de 512 échantillons. Couvert par un test qui vérifie aussi qu'un second trait par-dessus le premier ne laisse pas de queue de l'ancien.

- **Trois raccourcis pour le rail d'affichage** : `E` fait défiler `VIEW`, `S` la forme du crayon, `D` sa période. Trois touches voisines sous la main gauche, comme les commandes qu'elles reprennent le sont dans leur rail.
  - Sans modificateur, et jamais avec : `Shift+S` appartient au solo d'une piste, et le laisser aussi faire tourner les formes ferait deux choses d'une frappe.
  - Elles ne font rien de plus que le clic : `S` reste sans effet tant qu'aucune ligne n'est affichée, `D` tant que le crayon dort — exactement comme les moitiés de touche grisées. Une frappe qui armerait le crayon là où le bouton refuse donnerait deux vérités pour un même état.
  - Vérifié à l'écran, cycle complet des trois touches, plus `Shift+S` qui ne les touche pas et une saisie au clavier dans un champ de texte qui ne déclenche rien.

- **Le tempo d'un morceau porte sa mention `EDIT`**, empilée sous le chiffre dans la même boîte. Rien ne disait que ce nombre s'ouvrait : le bouton avait déjà le relief des autres commandes, mais un chiffre reste un chiffre, et l'infobulle ne se lit qu'après avoir cherché.
  - Empilé plutôt que côte à côte, pour ne pas prendre à un titre déjà tronqué la largeur qui lui reste. La boîte mesure 23 px et la rangée en fait 38 : rien ne bouge, et la liste montre toujours seize morceaux.
  - Six pixels de haut, donc 5,0:1 de contraste sur la face du bouton — le gris de premier jet tombait à 3,4:1, ce qui passe pour un libellé de douze pixels et disparaît à six. La mention reste très en retrait du chiffre lui-même, à 14,4:1.

- **La bibliothèque montre seize morceaux là où elle en montrait onze.** Chaque rangée réservait 54 px pour un contenu qui en occupe 24 — la poignée, seule chose plus haute que le texte — plus 12 px de marge. Les dix-huit pixels restants ne portaient rien : le chemin du fichier, qui justifiait cette hauteur à l'origine, est masqué dans cette disposition depuis longtemps. La rangée passe donc à 38 px.
  - L'en-tête perd huit pixels de marge verticale, et ses vingt pixels de côté descendent à douze : ils dépassaient de douze ceux de la liste juste en dessous, si bien que le titre et les morceaux ne partaient pas du même bord.
  - Trois règles se disputaient la marge de l'en-tête, la dernière du fichier l'emportant en silence. La valeur est réglée là où elle gagne, et non par une quatrième couche par-dessus — c'est en posant la retouche au mauvais endroit que je m'en suis aperçu : elle n'avait aucun effet.
  - Mesuré à l'écran : liste de 604 à 612 px, rangée de 54 à 38 px, seize rangées entières contre onze. Les titres longs se coupent toujours proprement, et la poignée `+` garde ses 24 px — c'est une cible de glissé.

- **La forme « aléatoire » est retirée du crayon.** À l'usage elle ne servait pas : une automation qu'on ne peut pas prévoir avant de la tracer se juge après coup, et se redessine. Le cycle du bouton fait donc éteint → carré → sinus → triangle → éteint, soit douze combinaisons avec les quatre périodes. Le générateur pseudo-aléatoire et son pictogramme partent avec elle plutôt que de rester en dormance.

- **Le panoramique de la clé compte maintenant dans le sidechain.** Une grosse caisse poussée sur un côté n'envoie plus que la moitié d'elle-même dans la somme mono, là où le centre en envoie `√2/2` de chaque côté : trois décibels de moins à l'arrivée, donc un pompage d'autant plus léger. C'est ce que fait une console dont le départ est pris après le panoramique.
  - Le poids est **ramené au centre**, pas absolu : une clé au milieu — le cas courant — pompe exactement comme avant. Seule la clé décalée change de comportement.
  - Il fallait le porter par la **profondeur**, et non par le niveau envoyé au détecteur. Le déclenchement compare une énergie rapide à une énergie lente : il est insensible au niveau qu'on lui donne, donc atténuer son entrée n'aurait rien changé au pompage. C'est le même chemin que l'enveloppe de volume, corrigée pour la même raison.

- **Un clip déposé pose aussi ses deux ancres de panoramique**, au centre, comme il posait déjà celles de volume. Sans elles, une automation écrite plus loin sur la voie remontait jusqu'au début du clip : la ligne rampe entre ses nœuds, et le premier nœud d'une voie vaut pour tout ce qui le précède. Les ancres bornent le clip à ce qu'il est avant qu'on y touche, et donnent les poignées par lesquelles on l'attrape. Un nœud déjà en place n'est pas écrasé — il porte un réglage voulu par quelqu'un.
  - Le semis passe par la même fonction pour les deux lignes, chaque table déclarant la colonne qui porte sa valeur et son repos : −4 dB pour le volume, le centre pour le panoramique. `PAN_CENTRE` est nommé plutôt qu'écrit en clair, pour que l'ancrage d'un clip et le repos d'un trait de crayon désignent la même chose.

- **Déplacer un clip portant une forme dessinée échouait sur `UNIQUE constraint failed`.** Le déplacement recalait chaque nœud sur le quart de temps, alors qu'un sinus dessiné en pose une douzaine par cycle — donc plusieurs par quart. Ils s'écrasaient les uns sur les autres et la contrainte d'unicité refusait tout le geste. Le calage sur le quart appartient à la main qui pose un nœud, pas à un clip qui avance : le déplacement est maintenant une translation, et les positions gardent leur finesse d'origine.
  - La place n'est plus jugée prise qu'à un millionième de temps près, au lieu du quart : sinon un seul nœud voisin barrait la route à toute une forme.
  - Le défaut existait déjà pour le volume dessiné, avant même que le panoramique se mette à suivre; il ne demandait qu'un clip à déplacer.

- **L'automation de panoramique suit son clip**, comme celle de volume le faisait déjà. Restée en arrière, elle décrivait un geste sur du silence — et le clip arrivait sur le geste du voisin. Les nœuds situés hors du clip ne bougent pas : c'est le clip qui emporte ce qu'il contient, pas la voie qui se décale.
  - Plutôt que de recopier la manœuvre pour la seconde table, **une seule fonction sert les deux**, désignée par le nom de sa table. Ce projet a déjà perdu six règles écrites en double, dont l'énumération des tables d'automation le mois dernier; celle-ci ne pouvait pas diverger deux fois de la même manière. Les deux validateurs de position se ramènent au même corps.
  - Le garage hors timeline vaut aussi pour le panoramique : sans lui, décaler d'un cran une suite de nœuds ferait entrer le premier dans la place encore occupée par le second, et la contrainte d'unicité refuserait le déplacement.

- **Le crayon se désarme quand `VIEW` masque tout.** Armé sans ligne affichée, il capturait le geste et n'écrivait rien : le trait partait dans le vide, sans que rien ne le dise. Son cran revient donc à « éteint » dès que les deux lignes sont masquées, et la touche s'éteint à 0,42 d'opacité — comme une touche désactivée — avec pour infobulle `Show a line first — VIEW`. Elle redevient armable dès qu'une ligne revient, et reste alors sur « éteint » plutôt que de se rallumer toute seule. Vérifié à l'écran sur les cinq étapes du cycle.

- **Toute la touche `DRAW` change la forme**, y compris la diode, le libellé et les marges; seuls les chiffres retiennent leur clic pour la période. Les deux moitiés seules répondaient, si bien qu'un clic à côté ne faisait rien du tout — et la touche porte `cursor: pointer` sur toute sa surface, donc une zone inerte s'y lit comme une panne, pas comme une limite. Vérifié à l'écran, cible par cible : diode, libellé, moitié de forme et fond avancent la forme d'un cran sans toucher à la période; les chiffres avancent la période sans toucher à la forme.

- **Le crayon accepte la demi-période** : le bouton fait désormais défiler ½, 1, 2 et 4 temps. Un cycle par temps était le plus rapide qu'on pouvait tracer, alors que le trémolo et l'auto-pan en croches — deux cycles par temps — sont justement les figures qu'on dessine le plus. La fraction est écrite `½` plutôt que `0.5`, qui tiendrait mal dans une touche de 18 px et se lirait comme un nombre de temps.
  - Sa taille est réglée **sur le bouton lui-même**, là où vit celle des chiffres : une fraction vulgaire est dessinée à mi-hauteur du corps, donc au corps des chiffres elle ressortirait deux fois plus petite qu'eux. Vérifié à l'écran : 13 px contre 9 px, sans débordement de la touche.

- **Un trait de crayon commence et finit au repos.** Sans cela, la ligne rampait depuis le nœud d'avant le trait jusqu'à la première valeur de la forme, et repartait de la dernière vers le nœud d'après : de l'automation créée *vers* le dessin, que personne n'avait demandée. Le trait porte désormais une ancre à chaque bout — le centre en panoramique, le niveau déjà en place en volume — et la forme est resserrée d'un centième de temps pour leur laisser la place. Deux centièmes pris sur une étendue entière ne s'entendent pas; un auto-pan qui démarre à gauche parce que le sinus commence là, si.
  - L'ancrage **ne peut pas se déduire de la forme** : selon qu'elle est carrée, sinusoïdale ou tirée au hasard, son premier point tombe sur une crête, un creux ou n'importe où. Il est donc posé, pas calculé, et les quatre formes sont couvertes par un test.
  - L'ancre de fin est **bornée au trait**. L'accumulation des flottants sur des centaines de cycles suffisait à la sortir de quelques millionièmes de temps au-delà de l'étendue annoncée — assez pour que le serveur refuse le nœud.
  - Le panoramique de repos est **lu là où le trait commence**, et non supposé centré : dessiner sur une piste déjà décalée la ramenait au centre à chaque bout. `panValueAtBeat` interpole entre les nœuds voisins, comme le fait le moteur.

## 2026-07-26

- **`CLEAR TIMELINE` laissait derrière lui l'automation de panoramique**, et le même oubli se répétait dans `restore_snapshot` : un Undo réécrivait tout sauf elle. Le second défaut était le plus grave, et personne ne l'avait encore vu.
  - Deux endroits énumèrent les tables d'automation — celui qui vide et celui qui restaure — et ajouter une table sans les visiter tous les deux passe sans bruit. Un test pose désormais un nœud de chaque sorte, vide, vérifie que les trois sont parties, restaure et vérifie qu'elles reviennent toutes.
  - `validate_restored_snapshot` contrôle aussi le panoramique reçu : une entrée d'historique ne doit pas pouvoir replacer une valeur hors du champ stéréo.

- **Le crayon ne pouvait pas dessiner par-dessus un clip** : le clip réclamait le geste pour se déplacer, si bien que tout trait commencé sur de l'audio — c'est-à-dire presque tous — partait en déplacement.
  - **Le crayon armé est un mode.** Déplacement et rognage de clip sont suspendus tant qu'il l'est, et le curseur devient un crayon sur toute la timeline. Un mode qu'on ne voit pas se découvre en cassant quelque chose.
  - **La courbe s'écrit pendant le geste**, et non à son relâchement. L'aperçu passe par les mêmes fonctions que l'écriture — c'est le résultat, pas une approximation — et remplace dans la ligne ce que le trait recouvrira. Sans lui, l'amplitude et la hauteur se choisissaient à l'aveugle. Mesuré : la courbe passe de 2 à 66 nœuds pendant un trait, puis se fige à ce qui est écrit.

- **Le VU-mètre chevauchait les touches de transport.** `.timeline-identity` portait `flex: 1 1 0` et `min-width: 0` : elle se laissait donc écraser à 258 px là où elle en demandait 480, et son contenu débordait par-dessous le VU. Le débordement existait depuis longtemps — mesuré à 328/258 bien avant —, `DRAW` l'a seulement poussé jusqu'à devenir visible.
  - Aucun des trois blocs de l'en-tête ne se laisse plus écraser sous sa largeur utile : chacun porte des commandes d'une taille donnée, et les rétrécir ne les rend pas plus petites, seulement tronquées. Quand la fenêtre ne suffit plus, c'est l'**en-tête qui passe à la ligne** — un retour à la ligne se lit, un chevauchement ressemble à un défaut.
  - Marges et espacement de l'en-tête resserrés de 52 à 32 px et de 28 à 14 px, ce qui repousse le point de bascule sans le supprimer.
- **Les six touches de transport posent sur la même ligne.** La rangée était centrée verticalement, si bien que PLAY/PAUSE — descendu sous `CLEAR TIMELINE` — flottait plus bas que COMP, LIMIT, VIEW et DRAW. Elle s'aligne désormais par le bas : les six touches ferment à 86 px, mesuré.
- **La touche `DRAW` se lit comme deux commandes.** Les deux moitiés étaient de largeur égale et séparées d'un simple trait, ce qui donnait un seul bouton portant deux marques. La forme prend maintenant les deux tiers — c'est elle qu'on regarde et qu'on change le plus —, la période le tiers restant, et chaque moitié a son creux et sa lumière propres, comme deux touches voisines. Le sillon entre elles est une ombre suivie d'une lumière, pas un filet. Le pictogramme passe de 13 à 16 px.
- **`CLEAR TIMELINE` ne coiffe plus que PLAY/PAUSE.** Étalé sur toute la rangée, il donnait l'impression de commander aussi COMP, LIMIT, VIEW et DRAW.

- **Crayon d'automation, à la Pro Tools.** Un bouton coupé en deux dans le rail de `VIEW` : la forme à gauche — carré, sinus, triangle, aléatoire —, la période à droite — 1, 2 ou 4 temps. Douze combinaisons en deux clics, et l'état éteint est le premier cran de la forme, ce qui évite un troisième bouton pour l'armer.
  - Armé, un glissé sur une piste **dessine la forme sur l'étendue parcourue**, la hauteur du pointeur donnant l'amplitude. Si les deux lignes sont affichées, **un seul geste écrit les deux automations** : c'est le même mouvement musical.
  - **Volume et panoramique n'oscillent pas autour de la même chose**, parce que les deux grandeurs n'ont pas la même nature. Un niveau a un plafond — celui déjà en place — donc la forme creuse vers le bas, ce qui est le geste d'un gate ou d'un trémolo. Un panoramique a un centre, donc pointer L60 donne un balancement L60 ↔ R60.
  - Le **carré porte des nœuds doublés** à chaque transition. L'interpolation entre nœuds étant linéaire, un palier tenu par un seul nœud ressortirait en triangle.
  - **La résolution cède avant la longueur** : un sinus long est plus grossier qu'un sinus court mais couvre toute son étendue. Elle ne peut pourtant pas céder indéfiniment — mille cycles demandent au moins deux mille nœuds — donc au-delà du plafond le trait s'arrête là où il cesse d'être représentable, ce qui se voit, plutôt que de s'amincir en silence.
  - L'aléatoire est **déterministe** : refaire le même geste redonne la même suite. Un hasard non reproductible serait intestable, et surprendrait qui refait un trait pour l'ajuster.
  - Le dessin **remplace** ce qu'il recouvre, l'ancienne plage et la nouvelle dans une seule transaction. Le serveur revalide le plafond de nœuds : une commande forgée ne doit pas pouvoir remplir une piste.
- **Le niveau par défaut vivait encore en double.** Le frontend gardait `-6` écrit en dur pour la ligne d'une voie sans nœud, resté tel quel quand le moteur est passé à −4 : la ligne dessinée mentait sur ce qu'on entend. `DEFAULT_TRACK_GAIN_DB` est désormais nommé des deux côtés, avec un commentaire qui pointe l'autre.

- **La course du panoramique passe de ±16 à ±46 unités**, soit presque toute la hauteur de la voie audio. La course étroite coûtait de la précision : le même glissé devait résoudre tout le champ stéréo, si bien qu'un pixel valait plusieurs pour cent de panoramique. Les deux lignes se croisent davantage, mais la couleur et la forme les séparaient déjà — et le nouveau bouton peut en masquer une.
- **Bouton `VIEW`**, à droite des flèches Undo/Redo et de la taille d'une touche de transport. Il fait défiler ce qui est affiché : panoramique, volume, les deux, rien.
  - Le pictogramme **montre ce qui sera affiché** plutôt que de dire « voir » dans l'abstrait : les deux lignes d'une piste, le volume plein et coudé, le panoramique pointillé, chacune éteinte à 20 % quand elle est masquée. L'état caché se lit donc comme les lignes elles-mêmes, effacées.
  - Les **nœuds suivent leur ligne** : une poignée sans sa courbe n'aurait pas de sens. Et le menu contextuel grise l'entrée d'ajout dont la ligne est masquée, en disant pourquoi — proposer d'ajouter un nœud invisible tromperait.
  - C'est un réglage d'affichage, donc local à la vue et non persisté : rien de ce que le moteur rend n'en dépend.

- **Automation de panoramique**, une ligne par piste, centrée par défaut. Clic droit sur une piste pour `Add Pan Node`, ou raccourci `P` qui en pose un au playhead sur la piste armée. **Nœud vers le haut, la piste part à gauche; vers le bas, à droite.**
  - **Loi à puissance constante** : les deux gains valent `√2/2` au centre, soit −3 dB chacun, de sorte que la somme de puissance ne bouge pas d'un bout à l'autre du balayage. Une loi linéaire ferait paraître le centre plus fort que les extrêmes, ce qui s'entend comme une bosse au milieu d'un mouvement. Un test vérifie la puissance en sept points, la symétrie du centre et les deux extrêmes.
  - Le panoramique agit **par voie, après son volume et avant la sommation** — la place d'un panoramique de tranche.
  - Ligne **ambre pointillée** contre le bleu marine du volume, nœuds en **losange** plutôt qu'en disque. Les deux courbes se croisent forcément; c'est la couleur et la forme qui les séparent, pas la place. Sans nœud, la ligne traverse la piste en son centre, de sorte qu'un panoramique neutre se voit au lieu d'être absent.
  - La convention haut/gauche est écrite **une fois** dans `panNodeY`, et son inverse relit la même géométrie — rien dans une image stéréo ne désigne un haut, donc la règle doit être tenue en un seul endroit.
  - **Le panoramique entre dans la signature de lecture.** L'omettre aurait laissé un mix en cache survivre à l'édition d'une courbe : exactement le défaut qui avait rendu la clé de sidechain inopérante.
  - Schéma 21 pour `timeline_pan_nodes`, et le champ entre dans le fichier de projet avec un `serde(default)` — un projet écrit avant le panoramique se relit sans lui plutôt que d'être refusé.

- **Le sidechain ignorait l'enveloppe de volume de la piste-clé.** Impossible d'écrire une progression de pompage en montant graduellement son fader. Deux défauts distincts, et le second était le vrai.
  - Le détecteur lisait le clip **avant** son automation de volume : le fader de la clé ne changeait rien à ce qu'il entendait. Il lit désormais le signal au niveau réglé.
  - Mais surtout la profondeur était **fixe** : `self.gain = DUCK_FLOOR` à chaque déclenchement. Corriger le détecteur seul n'aurait donné que deux états — pompage plein tant qu'il déclenchait, rien dès qu'il passait sous son plancher de bruit. La profondeur suit maintenant le niveau de la clé, ce qui est ce qui permet d'en écrire une progression.
  - Elle est mise à l'échelle **en décibels et non en gain** : à mi-course on veut la moitié du pompage tel qu'on l'entend, ce qu'une interpolation linéaire du gain ne donnerait pas (−4,6 dB au lieu de −7,5).
  - La **remontée garde le même temps musical** quelle que soit la profondeur. Le facteur par image est calculé à chaque déclenchement depuis la profondeur retenue, faute de quoi un pompage léger serait remonté plus vite et le groove aurait changé pendant qu'on monte le fader.
  - `DUCK_DEPTH_DB` et `DUCK_FLOOR` disaient le même chiffre de deux façons; seul le premier sert au rendu désormais, et un test vérifie que le second — réservé aux tests — reste d'accord avec lui.

- **Fenêtre de progression pendant le bounce.** Un rendu de plusieurs minutes sans retour visible passe pour un gel. Une modale affiche une barre encastrée, le pourcentage, et rappelle que le rendu n'est pas temps réel.
  - La progression est **mesurée, pas estimée** : `TimelineMixSource` implémente `ExactSizeIterator`, donc le nombre exact d'échantillons restants après le repositionnement est connu.
  - Elle n'est émise **qu'au changement de point de pourcentage** : un rendu produit donc cent messages au plus, quelle que soit la longueur du mix. Inonder l'interface d'événements la ralentirait précisément pendant qu'elle doit rester réactive. Un échec d'émission n'interrompt pas le rendu — au pire la barre cesse d'avancer.
- **Raccourci `V`** : ajoute un nœud de volume sur la piste armée, au playhead. Même logique que `B`, `Shift+S` et `Shift+M`, et même garde — sans modificateur, jamais pendant une saisie de texte, rendu au système si Ctrl, Alt ou Meta est tenu.
- Le menu d'aide gagne `V` et `BOUNCE MIX`, et précise que le clic droit pose un nœud **n'importe où**, là où `V` le pose au playhead.

- **Bounce Mix** : un bouton pleine hauteur, à gauche du groupe BPM / SAVE / LOAD / HELP, rend le mix complet hors ligne dans un WAV **16 bits, 44,1 kHz, stéréo entrelacé**. Un dialogue demande le nom et l'emplacement.
  - Le rendu **réutilise `TimelineMixSource`**, la source même que joue le transport : time-stretch, égaliseur de clip, filtres, automation de volume, sidechain, compresseur, teinte, limiteur. Un moteur de rendu séparé finirait par diverger de celui qu'on entend, et un bounce qui ne ressemble pas au monitoring ne sert à rien.
  - **Pas de silence de tête.** Le bounce commence au premier temps où un clip se fait entendre, pas au beat zéro : un projet dont le premier clip démarre à la mesure trois n'a aucune raison d'exporter deux mesures de vide. Un test le vérifie — huit temps à 120 BPM font quatre secondes écartées.
  - **Dithering triangulaire** d'un LSB de crête à crête sur la conversion vers 16 bits. Tronquer corrèle l'erreur au signal, ce qui s'entend comme une distorsion sur les fondus et les queues, là où le niveau descend vers les derniers bits. Le bruit décorrèle cette erreur : on échange une distorsion audible contre un souffle constant très bas. Le générateur est déterministe, donc deux bounces du même mix donnent le même fichier.
  - Rendu **à 44,1 kHz**, la fréquence des MP3 sources : aucune conversion de fréquence, dont la qualité n'aurait rien à gagner.
  - Écriture **au fil de l'eau**, avec les tailles RIFF corrigées par rembobinage à la fin. Un mix d'une heure fait 635 Mo qu'il n'y a aucune raison de tenir en mémoire. Un test contrôle chaque champ de l'en-tête à son décalage exact : un octet de travers donne un fichier illisible, ou pire, lu au mauvais format.
  - Le bouton est **aluminium**, comme les autres commandes du bandeau, et porte un **pictogramme dessiné dans `TransportGlyph`** — le même système que les marques de PLAY, PAUSE, COMP et LIMIT : trait de 1,4, bouts arrondis, boîte de 12. Une flèche descendant vers un support, pour dire que le mix va vers un fichier. Le rubis puis la trame diagonale essayés d'abord réclamaient l'attention en permanence pour une commande qu'on n'utilise qu'une fois le mix terminé. `align-self: stretch` lui donne la hauteur du groupe voisin sans nombre magique à maintenir.
  - Les interrupteurs master sont **ceux du projet** : `COMP` et `LIMIT` valent au bounce ce qu'ils valent au monitoring, puisque `render_plan` les lit en base et que `prepare_timeline` les transmet à la source. C'est une conséquence de la réutilisation, pas une précaution à se rappeler — mais un test verrouille les quatre combinaisons, parce que ce lien casserait sans bruit.
  - La commande part sur un fil bloquant et ne tient le verrou de la bibliothèque que le temps de construire le plan : le rendu peut durer des minutes, l'interface reste vivante.

- **`Tap` et `Snap to kicks` deviennent un seul geste en deux temps.** Ils étaient deux boutons voisins, sans rien qui dise que le second n'est pas optionnel — or une main ne tombe pratiquement jamais sur le tempo exact, donc taper sans recaler ne sert à rien.
  - Un cadre gris bleu pâle les réunit, une flèche les ordonne, et une consigne dessous dit quoi faire : « Tap the beat for a rough tempo, then snap it to the kicks. » Dès deux frappes elle devient « Now snap it to the kicks — the audio decides the exact tempo. »
  - **Le repère est la phrase, pas la couleur.** Ni le cadre, ni la flèche, ni les boutons ne changent d'apparence après un tap. Les premières versions allumaient tout en rubis puis en ambre : `Tap` porte déjà le rubis plein, et une commande qui se repeint à chaque geste fatigue plus qu'elle n'informe.
  - La consigne et la note de résultat prennent le **bleu marine des autres paragraphes de l'éditeur**. Je les avais écrites en bleu pâle après avoir calculé leur contraste sur `#181b22`, la coque sombre — alors qu'elles s'affichent sur `#dce0e6`, le panneau clair du corps. La sonde le montrait, je ne l'avais pas interrogée.
  - Elles adoptent aussi la **fonte du corps** (Inter, 11 px). La monospace du programme sert aux valeurs et aux libellés courts, pas à une phrase suivie.
  - **Régression réparée** : envelopper les deux boutons dans un `<div>` avait rompu `.bpm-edit-row > button`, un combinateur enfant direct, et les deux perdaient tout leur habillage — d'où l'étiquette de `Snap to kicks` qui n'était plus centrée. La règle accepte désormais les boutons du groupe.
- **`Save Correction` passe au rubis**, comme `Tap` avec qui il forme la séquence : on tape, on recale, on enregistre. Il portait le bleu nuit que le programme emploie pour une commande ordinaire, alors qu'il conclut le travail de l'éditeur. La portée est limitée au pied de l'éditeur : `.primary-button` habille aussi le « + MP3 » de la bibliothèque.

- **Le niveau par défaut d'une piste passe de −6 à −4 dB.** Les −6 dB étaient la réserve classique pour deux platines : deux morceaux beatmatchés ont leurs kicks en phase, donc +6 dB dans le pire cas, et la somme retombait pile à pleine échelle. Ce calcul datait d'une sortie **bornée en dur**, où tout dépassement s'entendait comme un écrêtage.
  - Le limiteur occupe désormais cette place et travaille par défaut. Payer six décibels en permanence pour un événement qu'il absorbe proprement est un mauvais échange — d'autant que +6 dB est le pire cas théorique : deux kicks de morceaux différents n'ont ni la même phase ni le même spectre, et montent plutôt de 3 à 4 dB.
  - **La valeur vivait en double** : `DEFAULT_TRACK_GAIN_DB` dans `timeline.rs` pour ce qui s'écrit en base, et un `-6.0` en dur dans `audio/timeline.rs` pour les voies sans nœud. Changer l'une aurait laissé l'autre derrière — le défaut récurrent du projet, pris cette fois avant qu'il ne morde. Le moteur lit maintenant la constante partagée, et un test vérifie qu'une voie sans nœud sonne comme une voie dont le nœud porte le défaut.
  - Deux tests réussissaient par coïncidence, en nommant `-6.0` des deux côtés d'une comparaison. L'un déplaçait un nœud à −6 dB puis vérifiait que *tous* les nœuds portaient le défaut; il distingue désormais le nœud déplacé à la main des nœuds automatiques. Dans l'autre, mon remplacement global avait écrasé à tort un −6 dB qui était le **milieu arithmétique** d'une interpolation entre 0 et −12, sans rapport avec le niveau par défaut.
  - Seuls les nouveaux clips sont concernés; les enveloppes existantes ne bougent pas.

- **Fichiers de projet portables**, avec `SAVE` et `LOAD` dans le bloc de droite de l'en-tête. Extension `.mixcanvas`, format JSON, une seule dépendance ajoutée (`base64`, déjà présente en transitif).
  - Le fichier porte tout ce qui décrit une session : chemins des morceaux, **corrections manuelles de BPM et de premier temps**, formes d'onde, réglages master, états Mute/Solo, et l'intégralité du timeline — clips, rognages, EQ par clip, clés sidechain, nœuds de volume et de filtre.
  - Un clip y désigne son morceau par son **rang dans le fichier**, jamais par son identifiant de base : un identifiant n'a de sens que dans la base qui l'a émis, et un projet doit s'ouvrir sur une machine qui n'a jamais vu ces morceaux. Un test le vérifie en appliquant un projet à une base neuve.
  - Les formes d'onde voyagent en base64 des octets `f32` bruts. Écrire les flottants en JSON les rendrait **plus gros que le binaire** qu'ils représentent : seize mille valeurs par rampe, six rampes, une dizaine de caractères par nombre. Comptez environ 520 Ko par morceau.
  - Ouvrir un projet **remplace** la session et vide l'historique d'annulation : ses instantanés décrivent des clips qui n'existent plus, et un Undo après chargement restaurerait l'état d'un autre projet. Le moteur audio est arrêté avant l'écriture — reconstruire la timeline sous une lecture en cours reviendrait à changer le plan pendant qu'il est joué.
  - Rien n'est supprimé de la bibliothèque au chargement : les morceaux du fichier y entrent s'ils manquent, gardent leur identifiant s'ils y sont déjà. Ouvrir un projet ne fait pas disparaître les morceaux d'un autre.
  - La base SQLite reste l'**état de travail vivant**, enregistré au fil de l'eau; le fichier de projet en est un instantané transportable. Les deux ne se remplacent pas.

- **Les zones du VU-mètre disent enfin quelque chose.** Bleu « trop bas », ambre « plage de travail », et **une seule lentille rouge** tout au bout. Sur 24 : 7 bleues, 16 ambre, 1 rouge.
  - Le rouge ne signifie plus que la distorsion. L'ancienne répartition en teignait quatre — un mètre dont le tiers supérieur est rouge apprend à son utilisateur à l'ignorer, et le témoin perd exactement ce qui en fait un témoin.
  - La frontière bleu/ambre est écrite **en décibels** (`VU_TOO_LOW_DB = -7`) et convertie en index par `vuSegmentZone`. Écrite en numéros de segment, elle aurait changé de sens en silence le jour où le nombre de LED bouge; un test vérifie d'ailleurs que la répartition tient à 12, 24 et 48 lentilles.
  - Le bleu marine de l'ancienne version était la pire couleur possible pour la plage utile : sur la fenêtre sombre, les quatorze premières lentilles — celles qu'on regarde — étaient les moins lisibles des vingt-quatre.

- **« Snap to kicks » : un tempo tapé est recalé sur celui que portent les kicks.** Le tap donne l'ordre de grandeur, l'audio tranche. Un bouton dans le Beatgrid Editor rejoue l'analyse en ne cherchant la période qu'autour de la valeur tapée, et remplit le BPM comme le premier temps.
  - L'analyse automatique se trompe surtout de **période**; les attaques qu'elle détecte restent bonnes. `estimate_beat_grid` cherchait dans toute la plage 70–190 BPM — un tap lui dit désormais *où* chercher, et la corrélation dit *quoi* trouver. Testé : un tap à 124,2 / 126,0 / 130,5 / 131,8 retombe à moins d'un demi-BPM de 128,0.
  - La fenêtre fait **±8 %**. Assez large pour une main peu sûre — la médiane de huit intervalles place la plupart des gens à deux ou trois pour cent près — et assez étroite pour ne jamais atteindre le demi-temps ni le double-temps, vers lesquels une fenêtre plus généreuse finirait par glisser. Un tap hors de la plage utile est ignoré plutôt que de produire une fenêtre vide.
  - Le a priori qui pénalise les tempos au-dessus de 175 BPM ne s'applique pas à une valeur tapée : il sert à départager deux hypothèses également plausibles, et l'utilisateur a déjà tranché.
  - La commande ne **persiste rien** : elle propose, l'utilisateur applique. Le verrou de la bibliothèque n'est tenu que le temps de lire le chemin, le décodage se faisant hors verrou pour que l'interface reste vivante.
  - Le bouton `Tap` était resté dans l'ancienne palette mauve — hors d'atteinte du balayage de CSS mort, puisque sa classe est bien employée. Harmonisé au passage.
- Une dernière chaîne française traduite : « L'analyse BPM s'est interrompue ». Ni accent ni mot-outil de ma liste, elle avait échappé aux **deux** balayages.
- `[profile.release]` gagne `lto = "fat"` et `codegen-units = 1`. Sans effet sur les compilations de développement; sur un binaire de production, le thread audio traverse plusieurs petites fonctions par échantillon et les 16 unités par défaut empêchaient l'inlining entre elles. La compilation passe à 5 min 45 s.

- **883 lignes de CSS mort supprimées** : 56 classes sur 253 n'étaient produites par aucun balisage, soit 135 règles. La feuille passe de 5 217 à 4 334 lignes, et le bundle CSS de 77,9 à 64,5 ko.
  - Ce sont des couches entières abandonnées par les refontes successives : l'ancien VU-mètre à aiguille (`.vu-face`, `.vu-needle`, `.vu-pivot`, `.vu-scale-arc`, `.vu-tick`), l'ancien en-tête (`.topbar`, `.brand`, `.hero-panel`, `.version-pill`), l'ancienne rangée de bibliothèque (`.track-art`, `.track-heading`, `.availability`, `.remove-track-button`), l'ancienne Preview (`.preview-card`, `.progress-slider`, `.time-row`) et l'ancienne chrome de timeline (`.timeline-lane-strip`, `.zoom-control`, `.timeline-title-row`). Le `.pulse-dot` vert menthe signalé plus tôt part avec.
  - La détection est **conservatrice** : une classe n'est retenue morte que si aucune de ses sous-chaînes n'apparaît nulle part dans le TSX ni dans `index.html`, et les deux bases composées dynamiquement — `led-vu-segment--${zone}` et `timeline-clip--trim-${edge}` — sont développées avant comparaison. Une recherche littérale seule les aurait déclarées mortes à tort.
  - Une règle n'est retirée que si **tous** ses sélecteurs sont inatteignables; un sélecteur groupé partagé avec une classe vivante est réduit, pas supprimé. Un sélecteur descendant dont l'ancêtre est mort l'est aussi, même si son dernier composant vit ailleurs.
  - Vérifié après coup : accolades équilibrées, 253 → 197 classes définies soit exactement les 56 attendues, aucune classe vivante perdue, commentaires d'architecture intacts, `check.cmd` vert et build de production propre.
- `backups/` entre dans `.gitignore` : 3,3 Mo de sauvegardes SQLite locales qui n'ont rien à faire dans un dépôt.

- **La colonne de la bibliothèque passe de 360 à 316 px**, les 44 px allant au timeline. C'est près du plancher : la rangée de titre de l'en-tête — nom, compteur, `Add Folder`, `+ MP3` — mesure 269 px à elle seule, plus 40 px de rembourrage, ce qui ne laisse que 5 px de marge. En dessous, ce sont les boutons qui se comprimeraient, pas les noms de morceaux; ceux-ci tronquent déjà proprement avec une infobulle portant le nom entier.
  - La règle responsive qui ramenait la colonne à 320 px sous 1050 px est retirée : elle l'aurait désormais **élargie** sur les écrans qui ont le moins de place.
- **Le menu d'aide remis d'équerre.** Il annonçait « Click Clip » pour ajouter un nœud de volume, ce qui est faux — c'est un clic droit sur la piste. Et sept interactions n'y figuraient pas du tout : les touches `T` et `R` (zoom avant et arrière au clavier, jamais documentées), déplacer un clip, le glisser depuis la bibliothèque, déplacer un nœud de volume, déplacer le point de tempo turquoise, et le trim rendu explicite comme réversible.

- **La zone de forme d'onde laisse passer la grille de tempo**, pour aider au beatmatch. La bande de titre reste opaque.
  - Ce n'était pas une affaire d'opacité : le fond du clip laissait **déjà** passer 48 % de lumière. Le coupable était un `backdrop-filter: blur(10px)` — les lignes de temps arrivaient en bouillie plutôt qu'en lignes, donc le clip paraissait opaque quel que soit son alpha. Le flou est retiré, le fond passe à 50 %.
  - Bénéfice au passage : plus de passe de composition par clip à chaque image de lecture. `backdrop-filter` est coûteux, et il y en avait un par clip.
  - La bande de titre tenait son aspect solide de ce même flou. Le corps devenant réellement transparent, elle reçoit un fond opaque à elle — aluminium, comme les autres petites plaques de la console — sans quoi une ligne de mesure traverserait le nom du morceau.

- **Trim de clip, à la Pro Tools.** Approcher une extrémité change le curseur en **bracket** (`[` au début, `]` à la fin) et l'arête s'allume en rubis; le glissé masque ou redonne du matériel, calé sur le quart de temps.
  - **L'ancre ne bouge pas.** Un trim change ce qu'on entend du morceau, pas où le clip se trouve : tout ce qui reste audible garde exactement sa place sur la grille. C'est ce qui le distingue d'un déplacement, et c'est pour ça qu'on l'utilise.
  - **Réversible.** Le rognage est stocké comme le nombre de temps masqués à chaque bout; retirer un trim, c'est le même geste avec un nombre plus petit, jusqu'à zéro où le morceau entier revient. Rien n'est détruit.
  - Le geste ne peut pas manger le clip (un demi-temps minimum reste) ni le faire pousser dans un voisin de la même piste. Les deux limites sont revalidées côté Rust, pas seulement dans l'interface.
  - Les colonnes `trim_start_beats` / `trim_end_beats` existaient depuis le split mais aucune commande ne les écrivait; `set_clip_trim` comble ce trou.
  - **La forme d'onde suit le rognage au lieu de s'étirer.** Pendant le glissé, la boîte du clip prenait la largeur du brouillon mais `ClipWaveform` recevait encore le rognage validé : une tranche fixe d'échantillons était comprimée dans une boîte qui rétrécissait — visuellement un time-stretch, pas un rognage. `clipWithTrim` calcule la géométrie une seule fois et la boîte comme la fenêtre de forme d'onde en descendent. J'avais moi-même dupliqué ce calcul en écrivant l'outil; c'est exactement la faute que la duplication provoque.
  - Les curseurs sont servis comme fichiers depuis `public/cursors/` et non en `data:` URI : la CSP de l'application n'autorise les images que depuis `'self'`, et un curseur compte comme une image.
- **22 chaînes françaises supplémentaires traduites.** Mon premier balayage cherchait des accents; « Ce clip n'existe plus dans la timeline » n'en contient aucun et était donc passé au travers. Le second scan cherche des mots-outils français — deux dans une même chaîne ne peuvent pas être de l'anglais. Il ne reste rien.

- **Tous les messages d'erreur passent en anglais.** 58 messages distincts, 89 occurrences, répartis sur les huit fichiers Rust — chacun d'eux s'affichait dans le bandeau rouge d'un programme censé être unilingue anglophone.
  - Ce ne sont pas des traductions mot à mot : un message doit dire ce qui s'est passé et quoi faire. « Analyse le BPM de ce morceau avant de l'ajouter à la timeline » garde son impératif; « La piste demandée doit être comprise entre 1 et 3 » devient « The track has to be A, B or C », puisque c'est ainsi que les pistes s'appellent à l'écran — l'ancien message était faux dans les deux systèmes de numérotation.
  - Le doublon d'`add_clip` est rattrapé au passage : `move_clip` refusait un chevauchement avec son propre message, resté en français. Les deux disent maintenant la même chose.
  - L'orthographe suit celle déjà en place dans l'interface (`analyze`, `initialize`). Les commentaires du code restent en français : ce sont des notes internes, pas ce que voit l'utilisateur.

- **« Impossible d'ajouter un morceau par-dessus un autre » sur un dépôt automatique.** La rotation était **aveugle** : elle avançait d'une piste à chaque clip ajouté sans jamais vérifier que la piste visée était libre à cet endroit. Au quatrième dépôt à la même position, elle revenait sur A — occupée — et refusait le clip sans avoir essayé B ni C.
  - La rotation ne fixe plus que **le point de départ de la recherche**. `add_clip` essaie A, B, C dans cet ordre et prend la première réellement libre à l'ancre. L'erreur ne survient que si les trois sont occupées, et elle le dit.
  - Une piste **nommée** par l'appelant reste la décision de l'appelant : un dépôt glissé sur une piste précise échoue toujours si elle est occupée, avec un message distinct.
  - La règle vivait **en double** — `timelineLaneRotation.ts` côté frontend et `unwrap_or(0)` côté Rust, qui ne tournait pas du tout. Le bouton « + » imposait donc une piste que le backend ne pouvait plus refuser d'accepter. La décision revient au backend, là où vit la géométrie; le module TypeScript et son test sont supprimés, la couverture reportée en Rust.
- **Le bandeau d'erreur était illisible** : un lavis rouge à 30 % d'opacité avec un texte rose poussière. Ce qui se trouvait derrière transparaissait, et sur un panneau clair cela donnait du rouge sur rouge. Il devient une plaque **opaque** rubis foncé à texte blanc — près de 10:1 de contraste. Un message qui vaut la peine d'interrompre doit être lisible où qu'il apparaisse.
- Les messages de `add_clip` passent en anglais.

- **`B` coupait un clip au hasard.** Le raccourci prenait le **premier clip du tableau** qui enjambait la tête de lecture — avec trois pistes actives en même temps, celui que la base renvoyait en premier, c'est-à-dire arbitraire vu de l'utilisateur. La piste décide désormais : pointer n'importe où dedans (bande de filtre, forme d'onde, ou l'espace entre les deux) l'arme, et `B` coupe le clip de **cette** piste.
  - Un repère rubis apparaît à côté de la colonne M/S, plutôt qu'un voile sur la piste : il doit se lire d'un coup d'œil sans concurrencer la forme d'onde qu'on est en train de regarder. La piste A est armée au départ, pour qu'un raccourci ait toujours une cible définie.
  - L'armement se fait en **capture**, donc avant que le pinceau de filtre ou un glissé de clip ne réclame le geste, et sans en annuler aucun.
- **Deux nouveaux raccourcis, même logique** : `Shift+S` bascule le solo de la piste armée, `Shift+M` son mute. Shift est le modificateur de piste, ce qui laisse `s` et `m` libres à la frappe et rappelle les boutons S et M qu'ils commandent. `Ctrl+S` et `Ctrl+M` ne nous appartiennent pas et sont rendus au système.
  - `resolveLaneShortcut` et `clipToSplit` sont testés séparément — neuf cas, dont celui qui reproduit exactement la régression : trois pistes actives au même beat, chacune doit couper la sienne.

- **Le VU-mètre redessiné**, à dimensions inchangées (357 × 64, 48 lentilles de 9,5 px, module OL de 26 × 54). Aucune propriété de boîte n'a été touchée : seule la matière change.
  - **Le vrai défaut était le contraste.** Des lentilles gris clair (`#cbd5e1`) dans des logements gris clair (`#c8d2e0`) sur un panneau clair : rien ne pouvait ressortir, quel que soit le niveau. Le boîtier devient une **fenêtre sombre encastrée** dans la console, du même type de creux que le rail de transport voisin — une lampe a besoin de noir derrière elle avant de pouvoir paraître allumée. Le filet clair sous la fenêtre est l'arête du panneau qui accroche la lumière.
  - **Une seule progression, du froid au chaud**, dans des couleurs que l'interface possède déjà : neutre tant que le niveau va bien, ambre quand il monte, rubis là où ça compte. L'ancienne allait bleu marine → orange → rouge, dont le bleu marine disparaissait purement et simplement.
  - **Les classes mentaient** : `--green`, `--amber`, `--red` alors que la feuille de style les allumait en bleu marine, orange et rubis. Elles deviennent `--normal`, `--caution`, `--peak`, nommées d'après ce que le niveau signifie et non d'après une couleur. Les seuils (14 / 20 sur 24) ne bougent pas.
  - Éteinte, une lentille garde un léger dégradé : c'est de l'acrylique moulé, pas un trou. La lampe OL s'allume dans le même vocabulaire que celles des touches de transport, pour que la console n'ait qu'une seule façon de dire « ceci est actif ».

- **Les quatre touches de transport redessinées** (PLAY, PAUSE, COMP, LIMIT), à dimensions inchangées.
  - **Les emoji sont partis.** `▶ Ⅱ 🎚 ⚡` étaient rendus par la police du système, deux d'entre eux en couleur : aucun réglage ne pouvait les harmoniser, parce qu'ils n'étaient pas à nous. `TransportGlyph` les redessine en géométrie; ils prennent l'encre de la touche et tiennent leur graisse à toute échelle. COMP et LIMIT partagent une idée — la courbe de transfert : le compresseur se cintre là où il commence à travailler, le limiteur rencontre un plafond et file à plat le long. Côte à côte, la paire dit ce que les deux commandes font au signal.
  - **La LED seule dit l'état.** L'ancienne version basculait toute la touche en bleu nuit; une piste rayée en diagonale a été essayée puis écartée. Une lampe allumée suffit, et c'est la réponse la plus sobre : la touche ne bouge pas, seule la lampe s'allume — cœur incandescent, halo, et la lumière qu'elle répand sur la touche autour d'elle. Elle passe de 11 px à 6 px, parce que rien d'autre n'a besoin de changer.
  - **Le relief est refait** : lumière unique venue d'en haut, arête supérieure vive, dégradé qui retombe sur la face, corps extrudé et ombre de contact. L'enfoncement fait descendre la touche et effondre son corps avec elle, au lieu de simplement la décaler.
  - Les quatre blocs `.is-active` identiques (PLAY, PAUSE, LIMIT, COMP) fondent en une règle, et les huit `!important` qu'ils traînaient disparaissent.

- **Le programme s'appelle BeatForge.** Toute occurrence de l'ancien nom a été remplacée : manifestes (`package.json`, `Cargo.toml`, `tauri.conf.json`), nom de la caisse Rust (`ez_dj_lib` → `beatforge_lib`), titre de fenêtre, en-tête du panneau d'aide, préfixes des fichiers temporaires de test, documentation. L'icône passe d'un lettrage « EZ » mauve et menthe à un monogramme « BF » en fer chaud sur des barres d'acier, dans la palette actuelle.
  - **L'identifiant de paquet devient `ca.beatforge.app`.** C'est de lui que Tauri déduit le dossier de données : renommer sans plus revenait à pointer vers un dossier vide, et une installation existante aurait démarré comme si toute la bibliothèque avait disparu. `adopt_legacy_library` reprend au premier lancement la base laissée sous `ca.ezdj.app`.
    - Le journal d'écriture anticipée (`-wal`) et son index (`-shm`) voyagent avec la base. Emporter la base seule perdrait toutes les transactions non encore intégrées — sur la bibliothèque actuelle, 4,5 Mo.
    - La base est copiée **en dernier**, car c'est elle que teste la garde : une copie interrompue laisse simplement l'opération à refaire au lancement suivant, au lieu d'exposer une base privée de son journal. Un test verrouille cet ordre.
    - La copie ne s'exécute que si le nouveau dossier n'a pas déjà sa propre base, et l'ancien dossier reste intact : un échec ne coûte rien. À supprimer quand plus aucune installation ne précède le changement de nom.

- **Étiquette de dépôt restée à l'ancien design** : la piste visée s'entourait de vert menthe `#4cd7ae` et affichait « Déposer ici » — la dernière chaîne française d'un programme unilingue anglais —, tandis que l'étiquette qui suit le curseur était mauve `#8f78ff` sur violet foncé. Les deux adoptent le vocabulaire actuel : la piste s'arme d'un liseré rubis `#dc2626`, l'accent que l'interface réserve déjà à ce qui est vivant (tête de lecture, nœuds de volume, LED allumées), et la pastille dit « Drop here ». L'étiquette du curseur devient une plaquette aluminium comme les menus contextuels, puis s'allume en rubis dès qu'elle survole une piste, pour que le curseur et sa cible disent la même chose.
  - La piste conserve son propre fond : c'est le liseré qui l'arme, pas une teinte qui viendrait concurrencer la forme d'onde en dessous.

- **Nœuds de volume qui sautaient à −∞ avant de suivre le curseur** : le dessin et le glisser employaient deux géométries différentes. Le rendu place 0 dB à 110 unités dans la paire de pistes, avec une course de ±23 unités; le glisser mesurait depuis le haut de la **sous-piste de filtre** et étalait linéairement +12…−60 dB sur 100 unités. Attraper un nœud à 0 dB revenait donc à lire 110/100 = 1,10, clampé à 1, au-delà du seuil de silence de 0,97 : −∞ au premier pixel de mouvement. Même à +12 dB le nœud sautait à −50,6 dB. `src/lib/volumeCurve.ts` réunit la courbe et son inverse, avec un test qui relit chaque gain à la position où il est dessiné.
  - Le silence devient le plancher de la course plutôt qu'une zone morte séparée : `volumeNodeY(lane, null)` et `volumeNodeY(lane, -60)` donnent le même point, donc un nœud qui bascule en −∞ ne bouge pas.

### Ajouté

## 2026-07-25

- **Compresseur de collage master et teinte de console (bouton `COMP`)** : compresseur stéréo-lié placé avant le limiteur, de caractère fixe — seuil `−12 dBFS`, rapport `2:1`, genou doux de 6 dB, attaque 10 ms, retombée 120 ms, makeup `+2 dB`. Les pistes reposant à `−6 dB`, il travaille sur la moitié forte du matériel et laisse les passages calmes tranquilles; l'attaque n'est pas plus rapide afin que le transitoire du kick passe intact, et la retombée est plus courte qu'un temps à tout tempo de club, ce qui donne le pompage audible recherché.
  - **Détecteur passe-haut à 120 Hz** : alimenté par le signal complet, le kick s'approprierait toute la réduction de gain et ferait plonger le morceau à chaque temps. En écoutant au-delà du grave, le compresseur répond à l'ensemble du mix et le kick garde son poids.
  - **Teinte de console** engagée par le même bouton : plateau grave de `+2 dB` sous 90 Hz, plateau aigu de `+2 dB` au-dessus de 10 kHz, puis une **saturation** — parce que des plateaux ne déplacent qu'un équilibre, alors que ce qui colore un son, ce sont des harmoniques. Écrêteur cubique doux, mélangé à 30 % : sur un 200 Hz à 0,8 le fondamental ne perd que 0,36 dB et l'harmonique trois arrive à −35 dB.
    - La saturation est **bandée sous 5 kHz**. L'harmonique trois de cette bande retombe sous 15 kHz et reste donc dans le spectre, ce qui évite le repliement sans suréchantillonnage; et c'est là que la chaleur a sa place, la même courbe sur des cymbales n'ajoutant que de la friture. Un test envoie un 12 kHz et vérifie que rien n'apparaît à 8,1 kHz, la fréquence où son harmonique trois se replierait.
    - Les plateaux passent **avant** la courbe, pour que le relief sous le kick attaque le saturateur plus fort que le reste. Fondu de 8 ms pour commuter sans clic.
  - Le rapport `2:1` se calcule par une seule racine carrée, sans logarithme sur le thread temps réel. Le biquad accueille deux nouveaux types de filtre en plateau, qui profitent du cache de coefficients existant.
  - État persisté au schéma 18 (`project_settings.compressor_enabled`), **désactivé par défaut** : l'activer d'office aurait changé le son de tous les projets existants sans qu'on le demande. Appliqué atomiquement comme `LIMIT`, donc audible immédiatement pendant la lecture.
- **Limiteur master** : le bus de sortie était simplement écrêté à `0,98`. Un limiteur stéréo-lié occupe désormais cette place, avec une attaque de 2 ms et une retombée de 120 ms; la borne physique demeure en dernier recours pour le bref dépassement qu'une attaque finie ne peut pas rattraper. Le gain de réduction est commun aux deux canaux, donc l'image stéréo ne bouge pas.
- Le bouton `LIMIT` pilote réellement ce limiteur. Son état est persisté dans `project_settings.limiter_enabled` (schéma 17) et partagé atomiquement avec la source déjà placée dans la file audio, comme Mute et Solo : il prend effet pendant la lecture sans reconstruction ni redécodage.

- **L'affichage BPM de l'en-tête devient un afficheur, plus un champ** : c'était un `input[type=number]`, donc il portait les flèches natives du navigateur — qui proposaient une édition dont la carte de tempo n'a pas besoin, chaque clip posant déjà sa propre cible sur la courbe, et qui décentraient la valeur de la largeur qu'elles réservaient. La valeur est désormais centrée dans son écran et affiche en permanence le tempo au playhead, alors qu'à l'arrêt elle montrait le BPM de projet brut. La chaîne d'édition devenue inutile est retirée avec : l'état local du champ, sa validation, et le module `timelinePlayback` dont l'unique rôle était de valider une saisie en attente avant Play. La commande Rust `set_project_bpm` reste en place — `add_clip` l'utilise pour le premier clip et l'historique la réécrit.
- **Règles CSS mortes supprimées** : `.track-note`, `.timeline-tap-button`, `.row-preview-button` et `.preview-toggle-button` n'étaient rendues nulle part, mais portaient encore l'ancienne palette mauve et l'auraient réintroduite au premier usage. Leurs dix règles disparaissent, ainsi que leurs mentions dans les sélecteurs groupés et le `.transport-button` resté dans ces mêmes groupes. La feuille perd un kilo-octet et ne décrit plus que des éléments réellement affichés.
- **Bandeau de message de la Library harmonisé** : les confirmations — « 3 tracks analyzed », « … now uses 126.000 BPM with its first downbeat at … » — s'affichaient en vert-sarcelle `#8fcdbb` sur `rgba(35, 104, 84, …)`, de l'ancienne palette. Le bandeau conserve un accent turquoise, celui que le design emploie déjà pour les cibles de tempo, mais porte son texte dans le crème du panneau : le contraste passe de 9,5:1 à 14,9:1, ce qui compte pour une phrase longue en 11 px. La police devient Cascadia comme le reste du panneau, avec un interligne qui rend le retour à la ligne lisible.
- **Message de bibliothèque vide harmonisé** : « Your library is empty » et sa pastille ♫ étaient restés en mauve `#9b87ff`, dernière trace visible de l'ancienne palette. Le panneau Library ayant un corps sombre — contrairement aux commandes aluminium qu'il contient —, le bloc adopte sa propre palette crème sur anthracite : titre `#f5f2eb` à 16,8:1 de contraste, sous-texte à 6,1:1, pastille en logement encastré. Les valeurs par défaut de `.eyebrow` et `.status-label` passent à `color: inherit` plutôt qu'à une couleur fixe : les panneaux ne sont pas tous clairs, donc toute valeur figée serait illisible sur la moitié d'entre eux. `.primary-button` reprend le traitement bleu nuit sur blanc déjà utilisé partout, lisible sur les deux fonds. Deux blocs entièrement morts qui portaient encore l'ancienne palette — `.preview-status-dot` et `.transport-button` — sont supprimés.
- **Menus contextuels harmonisés** : les deux menus de clic droit portaient chacun une palette héritée d'une refonte antérieure — olive-brun sur `#24211c` pour la timeline, CNC sombre sur `#181b22` pour la Library — et différaient aussi par leur typographie, leur taille de texte et leurs rayons. Ils partagent désormais un seul bloc de règles, sur le style aluminium anodisé du transport et des afficheurs. Une action qui détruit du travail se signale par un survol rubis, les autres par un survol bleu nuit; l'état désactivé est visible au lieu d'être identique à l'état normal.
- **LED du VU-mètre allongées verticalement** : les lentilles remplissent la hauteur du boîtier au lieu d'y flotter centrées, ce qui les rend rectangulaires et aligne la rampe sur le témoin `OL` à sa droite.
- **Hauteur des trois voies audio rendue identique** : la sous-piste Filter de la piste A mesurait `43/50` de son tiers, celles de B et C `46,5/50`. Ce surplus compensait le bandeau séparateur de 6 px que A n'affiche pas, étant la voie du haut — mais une compensation exprimée en pourcentage ne peut pas suivre un bandeau exprimé en pixels fixes, si bien que l'écart variait avec la taille de la fenêtre et que B et C perdaient la différence sur leur espace audio. Les trois paires adoptent la valeur de A. Les décalages compensatoires qui en découlaient disparaissent avec : la ligne de bypass revient au milieu exact de la bande (`50 %` au lieu de `50 % + 3px` et `50 % − 1px`), et `filterNodeY` calcule une seule position au lieu de trois valeurs ajustées à l'œil (`21,5` / `24` / `25`).
- **Le bandeau séparateur ne recouvre plus les courbes de filtre** : dessiné au sommet de la sous-piste Filter, il empiétait sur les courbes qu'il surplombait — un passe-haut complet sur B ou C venait buter dedans. Il appartient désormais au bord bas de la voie audio du dessus, où rien n'est tracé : un clip s'arrête 14 px avant, et la ligne de volume n'y descend jamais. La séparation visuelle est au même pixel qu'avant, mais la bande de fréquences est intégralement utilisable sur les trois pistes.
- **Bande de titre des clips amincie** : la barre portant le titre, le bouton `EQ` et le bouton `×` passe de 26 à 18 pixels, et ses deux boutons de 18 à 14. Chaque pixel de cette bande est un pixel où un clic saisit le clip au lieu d'atteindre la piste en dessous; il y a donc davantage de place pour se déplacer dans la timeline sans accrocher un clip, et la forme d'onde y gagne huit pixels. La hauteur devient la variable CSS `--clip-heading-height` : la forme d'onde et la ligne rouge du premier temps s'en déduisent, au lieu de répéter la même valeur en dur à trois endroits où elle pouvait diverger.
- **Redimensionnement horizontal des courbes de filtre** : jusqu'ici un geste sur une courbe existante ne pouvait qu'ajuster sa hauteur. Saisir l'un de ses bords — à moins de huit pixels, donc une cible de taille constante quel que soit le zoom — l'allonge ou la raccourcit. Le bord opposé reste immobile, l'intensité et la forme sont conservées, et le bord déplacé se cale sur la grille du quart de beat. Le redimensionnement s'arrête à la courbe voisine au lieu de l'écraser, et ne peut ni s'inverser ni sortir des bornes de largeur. Le curseur passe en `ew-resize` au survol d'un bord, sans quoi la zone de saisie serait invisible. La commande `draw_timeline_filter_bubble` accepte désormais la plage remplacée, afin que l'ancienne étendue et la nouvelle soient écrites dans la même transaction : une courbe raccourcie ne laisse pas sa queue derrière elle, et rien ne s'entend s'ouvrir pendant la lecture.
- **Courbes de filtre beaucoup plus longues** : la largeur maximale d'un Filter Brush passe de 128 à 4 096 beats, soit de 32 à 1 024 mesures — d'environ une minute à environ une demi-heure à 128 BPM. Pour que cela reste sans coût, l'espacement des échantillons persistés s'adapte : un quart de beat jusqu'à 512 échantillons, puis un pas plus large qui conserve ce plafond. Une courbe d'une demi-heure occupe donc autant de place qu'une courbe de deux mesures, et les courbes de 128 beats ou moins gardent exactement leur résolution d'origine. Le pas reste un multiple du quart de beat, donc chaque échantillon tombe toujours sur la grille attendue.
- **Suppression d'une courbe de filtre au clic droit** : un clic droit sur une bulle de la sous-piste Filter ouvre `Delete Filter Curve`. Une courbe étant persistée comme une suite dense d'échantillons plutôt que comme un nœud unique, la commande Rust `clear_timeline_filter_range` efface toute la plage écrite par le geste, points de bypass compris. Un clic droit hors d'une courbe n'ouvre aucun menu, et la sous-piste Filter ne laisse jamais passer l'événement vers le menu Volume de la voie en dessous.

### Ajouté

- **Compression sidechain (clip-clé)** : une icône de clé apparaît dans l'en-tête de chaque clip, à côté de `EQ`. Elle ne devient cliquable que si le clip en chevauche un autre — sur un clip isolé elle ne pomperait rien et ne masquerait rien. Le clip désigné devient la source de déclenchement : il se tait là où il en recouvre d'autres, et y impose son pompage. Ailleurs il joue normalement, ce qui permet d'utiliser un vrai morceau comme clé plutôt qu'une boucle de kick muette.
  - **Le détecteur écoute l'audio réel de la clé**, à travers un passe-bas à 150 Hz : la clé peut donc être un morceau complet et déclencher sur son kick plutôt que sur ses charleys. Le pompage suit le rythme réel de la clé, y compris syncopé, sans dépendre de la grille de temps.
  - **Déclenchement sur transitoire, pas sur niveau** : le passe-bas seul ne suffisait pas. Une ligne de basse occupe exactement la bande du kick et s'y maintient, si bien qu'un détecteur de niveau la lit comme un seul coup interminable et réduit le gain en continu — ce qui s'entend comme un compresseur qui se comporte mal, et non comme un pompage par kick. Le déclenchement compare désormais deux enveloppes d'**énergie** du grave, une rapide de 15 ms et une lente de 300 ms : un kick fait décoller la première au-dessus de la seconde, une note tenue les fait monter ensemble et ne déclenche rien. Mesurer l'énergie et non l'amplitude est le point qui compte — comparer une enveloppe de crête à une de moyenne donne, pour une sinusoïde stable, un rapport voisin de 1,57 quel que soit le niveau, assez près de tout seuil raisonnable pour se déclencher. Seuil retenu : deux fois et demie l'énergie, environ 4 dB. Une fenêtre réfractaire d'un demi-temps, dérivée du BPM, empêche la queue d'un kick ou une note de basse suivante de redéclencher.
  - **La retombée est liée au tempo** : le gain remonte sur neuf dixièmes de temps, donc il arrive à l'unité juste avant le kick suivant. Elle est linéaire en décibels et non exponentielle — c'est ce qui donne le gonflement droit qu'on entend comme du pompage, là où une retombée à un pôle remonterait d'un coup puis traînerait. Profondeur fixe de `−15 dB`.
  - **Une seule clé à la fois** : en désigner une libère la précédente dans la même transaction, pour que le projet ne soit jamais brièvement pompé deux fois.
  - Ordre de la chaîne master : **duck → compresseur → limiteur**, l'ordre console. À noter en mixant : un compresseur relève ce que le duck vient de creuser, donc activer `COMP` adoucit un peu le pompage.
  - **Pas d'interrupteur global** : le bouton `DUCK` faisait doublon avec la clé — un clip la porte ou ne la porte pas, et c'est déjà la commande. Il est retiré, ainsi que son état persisté; le schéma 20 supprime la colonne que le schéma 19 avait ajoutée. Un test vérifie qu'une colonne posée par une migration puis retirée par une suivante a bien disparu au bout de la chaîne.
  - Schéma 19 ajoute `timeline_clips.is_sidechain_key`.
  - **Correction du premier essai** : désigner une clé à l'arrêt ne produisait aucun effet, et la clé restait audible. `playback_signature` ne prenait pas en compte `is_sidechain_key`, si bien que la signature du plan ne bougeait pas et que le moteur réutilisait au Play le mix déjà en cache, construit sans clé. Ce hachage décide si un mix en cache peut resservir : tout ce qui change ce qui est rendu doit l'y faire figurer. Un test vérifie désormais que nommer une clé invalide le cache, et qu'à l'inverse les interrupteurs master ne le font pas — ils sont partagés par atomique avec la source déjà en file.
  - L'icône de clé est horizontale : anneau à gauche, tige à droite, dents vers le bas.

### Corrigé

- **Le zoom extérieur maximal ne montrait pas tout le projet** : le calcul de la borne de zoom était pourtant juste, le contenu tenait exactement dans la fenêtre. Mais le suivi du playhead s'appliquait à tous les niveaux de zoom — une marge virtuelle de demi-fenêtre et un décalage de `−playhead × zoom` — si bien qu'au zoom minimal le contenu était poussé pour centrer le playhead et que la moitié sortait de l'écran. Le suivi ne s'applique désormais que tant que le contenu déborde de la fenêtre; dès que le projet entier y tient, il est centré. La borne de zoom extérieure descend par ailleurs un cran plus bas que l'ajustement parfait : le projet occupe 88 % de la largeur, encadré de deux marges égales, de sorte qu'atteindre la limite se lise comme une limite plutôt que comme une commande bloquée.
- **La barre d'espace pilotait la timeline depuis l'intérieur du Beatgrid Editor** : le défaut allait plus loin qu'un raccourci mal dirigé, puisque démarrer la timeline libère la sortie Preview. Appuyer sur Espace dans l'éditeur coupait donc l'écoute en cours pour lancer le morceau derrière la fenêtre. Une fenêtre ouverte possède désormais le transport : l'éditeur pilote son propre lecteur, et la fenêtre Clip EQ ne démarre rien.

### Ajouté

- **La correction manuelle du premier temps est calée sur la grille analysée** : personne ne clique exactement sur un temps, et un downbeat décalé de cent millisecondes fait dériver tous les clips construits dessus. L'analyse sait déjà où sont les temps; la correction n'a donc qu'à désigner *quel* temps est le temps 1, et la position est relue sur la grille. La grille est prolongée arithmétiquement plutôt que cherchée parmi les temps stockés, afin qu'un downbeat placé avant le premier temps analysé — précisément l'usage quand un morceau ouvre sans batterie — tombe sur le bon temps au lieu de sauter vers l'ancre. Corriger le tempo est un autre geste : il décrit une grille que l'analyse n'a jamais produite, la position saisie est alors prise telle quelle. Le message de confirmation indique la valeur réellement enregistrée et signale le calage.

### Modifié

- **Détection du premier temps revue** : la barre rouge tombait trop souvent à côté. Trois causes distinctes, toutes corrigées, et la version d'algorithme passe à 2 pour que les morceaux déjà en bibliothèque soient réanalysés une fois au démarrage, corrections manuelles conservées.
  - **Le downbeat se décidait sur l'enveloppe large bande.** En musique électronique le premier temps est le kick, mais un clap ou une caisse claire sur les temps 2 et 4 produit une attaque large bande plus grande que le kick — plus brillante, spectre plus étendu, saut d'énergie plus marqué. L'algorithme se calait donc régulièrement sur le contretemps, soit une demi-mesure d'écart. Une seconde enveloppe est maintenant extraite pendant le même décodage, filtrée à 120 Hz sur deux pôles, et c'est elle qui décide du premier temps. Un test reproduit le cas et vérifie que l'ancien chemin large bande échoue là où le nouveau réussit.
  - **La recherche de phase était bornée à ±18 % d'un temps** autour de la première attaque significative, ce qui supposait que cette attaque tombait elle-même sur un temps. Dans une intro électronique c'est souvent faux — un riser, une nappe, un souffle de vinyle ou un contretemps arrive en premier — et la bonne phase devenait alors inatteignable, décalant toute la grille. La phase étant périodique, la recherche couvre désormais un temps complet.
  - **La grille était ancrée sur un point extrapolé dans une intro sans batterie.** Mesuré sur `Jestruepp`, la bande grave est strictement nulle pendant les 46 premières secondes et le kick n'entre qu'à 61,72 s; le premier temps était pourtant annoncé à 9 089 ms, là où rien ne joue. Le premier temps est désormais le premier temps de la grille sur lequel le kick tape réellement, ce qui donne 61 946 ms sur ce morceau. La recherche se fait sur la grille elle-même — force du kick échantillonnée à chaque temps, moyennée sur deux mesures — et non sur le niveau brut, afin qu'un kick d'intro plus discret que le drop compte quand même. Un morceau sans grave exploitable retombe sur la comparaison des quatre phases.
  - **Un biais arbitraire favorisait la phase 0**, c'est-à-dire simplement l'endroit où la première attaque était tombée : le résultat dépendait donc du premier transitoire de l'intro. Il est retiré au profit d'un vote par mesure combiné à l'accentuation moyenne, pour qu'un unique passage fort ne décide pas de tout le morceau.
- **Le témoin `OL` signale désormais un écrêtage réellement subi.** Les deux points de mesure du bus master sont séparés : les aiguilles du VU observent toujours le master avant le limiteur, puisqu'elles indiquent le niveau que le mix produit; `OL` est mesuré après le limiteur et avant la borne physique, donc il ne s'allume que lorsque cette borne a effectivement dû rogner le signal. Limiteur activé, il reste éteint sauf si un transitoire devance l'attaque; limiteur contourné, il se comporte comme avant. Un témoin qui s'allume sur un dépassement déjà absorbé n'indique pas un défaut du son produit, seulement un niveau élevé — c'est le rôle du VU.

### Corrigé

- **Suite de tests Rust réparée** : `cargo test` ne compilait plus depuis l'ajout des champs `trim_start_beats`, `trim_end_beats` et `eq_settings` (4 erreurs `E0063`/`E0061` dans `src-tauri/src/timeline.rs` et `src-tauri/src/audio/timeline.rs`). `check.cmd` échouait donc à l'étape `cargo test`; il passe de nouveau intégralement, formatage et Clippy compris.
- **Boucle de sauvegarde infinie du Clip EQ** : l'effet « Live EQ » de `ClipEqModal.tsx` dépendait de `onSave`, que `App.tsx` reconstruisait à chaque snapshot de timeline. Le premier mouvement de curseur déclenchait donc une boucle sans fin d'écritures SQLite, de reconstructions du plan audio et d'entrées d'historique. Le callback est désormais lu par `useRef` et l'écriture est débouncée à 200 ms.
- **`-∞ dB` du Clip EQ sans effet** : `-Infinity` devient `null` en JSON et l'IPC Tauri le livrait au moteur comme « aucun gain défini », qui ignorait la coupure. Le silence est maintenant une valeur finie partagée, `CLIP_EQ_SILENCE_DB` (`-60 dB`) dans `src/lib/clipEq.ts` et `src-tauri/src/audio/timeline.rs`. L'interface clampait par ailleurs à `-48 dB` alors que le moteur coupait dès `-36 dB`; les deux bornes coïncident désormais.
- **Cartes de tempo divergentes** : `project_timing` construisait ses cibles depuis `anchor_beat` tandis que `snapshot` et `render_plan` utilisaient `tempo_anchor_beat`. Déplacer un nœud BPM turquoise faisait diverger les signatures, `matches_timing` rejetait le cache du moteur, et le Seek comme le suivi du transport cessaient silencieusement de fonctionner. Les deux chemins partagent maintenant `tempo_targets` et `project_end_beat`.
- **Longueur de projet ignorant le rognage** : `project_timing` calculait la fin depuis la durée complète du MP3. Après une scission suivie de la suppression du sous-clip de droite, elle divergeait de `render_plan` et provoquait la même panne silencieuse.
- **`CLEAR TIMELINE` pendant la lecture** : la commande renvoyait une erreur, car `refresh_live_timeline_after_edit` appelait `render_plan` sur une timeline devenue vide. La base était pourtant vidée, mais l'interface affichait « Unable to continue » et gardait les clips à l'écran. Une édition qui ne laisse plus rien à jouer arrête maintenant le transport au lieu d'échouer; une erreur de rendu réelle reste signalée.
- **Écritures non atomiques** : `split_timeline_clip`, `restore_snapshot` et `draw_filter_bubble` enchaînaient plusieurs instructions sans transaction. Une scission interrompue tronquait le clip gauche en perdant sa partie droite, un Undo interrompu vidait la timeline, et une bulle de filtre exécutait jusqu'à 513 `INSERT` dans autant de transactions implicites. Les trois passent par une transaction unique, la bulle réutilisant un `INSERT` préparé.
- **`restore_snapshot` sans validation** : le snapshot d'Undo transite par le frontend et était écrit tel quel. Piste, bornes de beat, BPM, gains, valeurs et tensions de filtre sont désormais contrôlés comme pour une édition normale.
- **Versionnement SQLite figé** : `CURRENT_DATABASE_SCHEMA` estampillait `user_version = 14` alors que les migrations vont jusqu'à 16, et il était réexécuté après migration. La version retombait donc à 14 et les migrations 14→15→16 se rejouaient à chaque démarrage. Le schéma porte maintenant `LATEST_SCHEMA_VERSION`, et les colonnes ajoutées par `ensure_column` reçoivent les mêmes contraintes `CHECK` qu'une base neuve.
- **Historique Undo/Redo** : l'état précédent était empilé avant l'appel au backend, si bien qu'une édition refusée laissait une étape qui n'annulait rien; deux Undo rapides empilaient deux fois le même état dans Redo; le transport n'était pas rafraîchi après restauration. L'historique est extrait dans `src/lib/undoRedo.ts` et n'enregistre plus qu'après succès.
- **Bulle de filtre dessinée ≠ entendue** : React fermait la bulle à `+0.001` beat et Rust à `+0.01`. La constante est partagée entre `src/lib/filterShape.ts` et `src-tauri/src/timeline.rs`.

### Modifié

- Les tests `splitClip.test.ts`, `undoRedo.test.ts` et `filterTriangle.test.ts` n'importaient aucun code de production : ils réimplémentaient la logique dans le fichier de test et l'assertaient contre leur propre copie. `undoRedo.test.ts` testait même une classe absente de l'application. Ils sont remplacés par `src/lib/undoRedo.ts`, `src/lib/filterShape.ts` et leurs tests, plus deux tests Rust couvrant réellement la scission et l'accord entre `project_timing` et `render_plan`.
- `sanitizeClipEq` et `DEFAULT_CLIP_EQ` n'étaient utilisés que par leur propre test; `ClipEqModal` réécrivait ses défauts en dur. Toutes les lectures et sauvegardes du Clip EQ passent désormais par ces fonctions.
- Les callbacks d'édition de timeline d'`App.tsx` passent par `runTimelineEdit` et deviennent référentiellement stables, ce qui supprime aussi une cascade de rendus de `TimelinePanel` à chaque snapshot.
- L'interface devient unilingue anglaise. Les deux dernières chaînes françaises visibles — la confirmation de `CLEAR TIMELINE` et le libellé d'accessibilité du VU — sont traduites. Les messages d'erreur du backend Rust, qui remontent dans la bannière `Unable to continue`, restent à traduire.
- **Coefficients de biquad mis en cache** : `sin`, `cos` et `powf` étaient recalculés à chaque échantillon, pour chaque filtre, chaque canal et chaque clip, sur le thread temps réel. Ils ne sont plus conçus que lorsque la forme change réellement : un Clip EQ statique ne recalcule jamais, et un sweep de lane recalcule une fois par trame au lieu d'une fois par échantillon.
- **Scroll horizontal pendant la lecture** : `Shift + molette` était effacé par le rafraîchissement du transport toutes les 50 ms, donc invisible pendant Play. La vue reste maintenant là où l'utilisateur l'a laissée, puis revient suivre le playhead après 2,5 s d'inactivité; un clic de positionnement ou un changement de transport la rend immédiatement.
- Les boutons `DUCK` et `COMP` étaient des interrupteurs locaux branchés sur rien, dont les infobulles promettaient un sidechain et un compresseur absents du moteur. Ils sont désactivés explicitement en attendant les modules DSP correspondants.

## 2026-07-23

### Ajouté

- **Fonctionnalité "Clip EQ" (Égaliseur Paramétrique par Morceau)** :
  - Intégration du bouton **"EQ"** et harmonisation du bouton de fermeture **"×"** (`.clip-remove-btn`) avec la même hauteur (18px) et la même finition aluminium anodisé 3D (`src/components/TimelinePanel.tsx` & `src/app.css`).
  - Création du composant fenêtre modale **[ClipEqModal.tsx](file:///Y:/MixCanvas/src/components/ClipEqModal.tsx)** avec écran graphique LCD Bleu Nuit (`#152c4e`) affichant la réponse en fréquence log (20 Hz - 20 kHz) et 2 poignées interactives paramétriques (High Pass / Cutoff bleu & Low Pass rouge).
  - **Architecture Égaliseur 3 Bandes (HPF, LPF & 3e Bande Paramétrique Bell)** :
    - Filtres **Passe-Haut (HPF)** et **Passe-Bas (LPF)** restaurés avec atténuation standard jusqu'-> -36 dB.
    - Ajout de la **3e Bande EQ Paramétrique (Bell / Peaking)** dotée d'une fréquence réglable (20 Hz - 20 kHz), d'une plage de gain s'étendant d'une **réduction totale jusqu'à moins infini (-∞ dB / Notch Cut)** jusqu'à un **boost d'amplification de +6 dB**, et d'un facteur d'amplitude **Q (0.1 à 10.0)**.
    - Restructuration des cartes de contrôle de la modale en **disposition verticale superposée** (`.clip-eq-controls-row` en `flex-direction: column` dans `src/app.css`) : les cartes **PASSE-HAUT (HPF)**, **EQ PARAMÉTRIQUE (BELL)** et **PASSE-BAS (LPF)** sont empilées les unes sous les autres sans le moindre débordement à droite.
  - **Repositionnement et Rotation du Boîtier `OL` à Droite des LED (`StereoVuMeter.tsx`)** :
    - Nouveauté dans `src/components/StereoVuMeter.tsx` et `src/app.css` : Déplacement du boîtier de surcharge **`OL`** directement **à la droite des rampes de LED** (`display: flex; flex-direction: row` dans `.stereo-vu-meter`).
    - Orientation verticale du boîtier (`.vu-overload-container`) : Le label **`OL`** est positionné en HAUT, et la voyant LED rouge de surcharge est placé juste en DESSOUS du texte.
    - 66/66 tests Vitest et `tsc --noEmit` validés.
  - **Ajustement de la Hauteur des Filtres Existants (`TimelinePanel.tsx`)** :
    - Détection de la forme existante (`peakRatio` dans `existingFilterBubbleAt`) lors du clic sur un filtre déjà en place (`ramp_up`, `ramp_down`, `triangle`).
    - Verrouillage de la position (`startBeat`), de la largeur (`widthBeats`) et de la forme (`shape`) du filtre existant lors du glisser.
    - Seule la hauteur / fréquence de coupure (`value`) est ajustée vers le haut ou vers le bas en temps réel.
    - 63/63 tests Vitest validés.
  - **Correction du Saut / Snapping Visuel des Subclips Découpés (`TimelinePanel.tsx`)** :
    - Correction du calcul de `visualStartBeat` dans React (`TimelinePanel.tsx`). Le composant utilisait par erreur `anchorBeat - preRollBeats` (ce qui valait `0` pour le subclip de droite et le faisait sauter instantanément au tout début de la timeline).
    - `visualStartBeat` utilise désormais `clip.visualStartBeat` calculé par le backend Rust (prenant en compte `trimStartBeats`). Lors d'une scission, les deux subclips restent **parfaitement immobiles** à leur position exacte sans le moindre saut visuel.
  - **Correction du Découpage de Waveform des Subclips (`ClipWaveform.tsx`)** :
    - Découpage précis du tableau de crêtes de forme d'onde (`leftMin`, `leftMax`, `leftRms`, `rightMin`, `rightMax`, `rightRms`) selon les ratios `trimStartBeats` et `trimEndBeats`.
    - Chaque subclip affiche uniquement la portion de forme d'onde correspondant à sa plage audio réelle sans étirement ni décalage visuel.
  - **Interdiction Stricte des Superpositions de Clips (Anti-Overlap Engine)** :
    - Ajout de la validation `clips_overlap` dans `src-tauri/src/timeline.rs` pour l'ajout (`add_clip`) et le déplacement (`move_clip`).
    - Aucune superposition n'est autorisée sur une même piste (clip sur clip, subclip sur clip ou subclip sur subclip). En cas de tentative de chevauchement, l'action est rejetée avec un message explicite.
    - **Correction du Rendu Visuel des Subclips (`TimelinePanel.tsx` & `timeline.rs`)** :
      - Suppression du plancher de largeur forcée `Math.max(88, ...)` sur les clips qui étirait visuellement les subclips courts et les faisait déborder au-delà de leur fin réelle.
      - `clipWidth` est désormais strictement proportionnel à la durée en beats du clip (`clip.durationBeats * pixelsPerBeat`), garantissant une démarcation parfaite et sans aucun chevauchement au point de scission.
      - Calage automatique de `right_tempo_anchor_beat` au minimum à `target_beat` pour le subclip de droite dans `split_timeline_clip` Rust.
    - 60/60 tests Vitest validés.

## 2026-07-22

### Modifié

- **Correction du chevauchement des cartouches de morceaux sur les bandes de filtres (`src/components/TimelinePanel.tsx` & `src/app.css`)** :
  - Remplacement du décalage en pixels fixes (`+ 57px`) par un calcul dynamique en pourcentage (`top: calc(lane * 33.333% + (100% / 9) + 7px)`).
  - La cartouche du morceau (`.timeline-clip`) est désormais confinée de manière responsive et étanche dans la piste de forme d'onde (`.timeline-lane`), à exactement 7px en-dessous de la ligne de démarcation de la bande de filtre supérieur, quelle que soit la hauteur de la fenêtre.
  - **Ligne d'automation de volume à 0 dB** : Recalcul de `volumeNodeY` pour aligner l'axe du volume 0 dB sur l'axe central horizontal exact des formes d'onde audio (`Y = 110px`).
  - **Rétablissement de la trame hachurée 45° sur l'espace d'amorce à gauche (`src/app.css`)** : Restauration du motif dégradé hachuré à 45° sur aluminium anodisé (`#dce0e6`) dans le fond de `.timeline-scroll`, redonnant son aspect industriel d'origine à la zone d'amorce et aux commandes à gauche.
  - **Suppression de l'étiquette de sous-piste de filtre (`src/components/TimelinePanel.tsx`)** : Retrait du libellé texte (`FILTER BRUSH`) dans le coin supérieur gauche des bandes de filtres pour un rendu encore plus épuré.
  - **Dégradé vertical thématique sur le fond des bandes de filtres (`src/app.css`)** : Application d'un dégradé vertical bicolore élégant fading vers un bleu cian en haut (zone High Pass Filter) et un rouge corail en bas (zone Low Pass Filter) autour de la ligne centrale d'automation.
  - **Alignement vertical parfait des boutons flottants M et S (`src/app.css`)** : Remplacement du `margin-top: %` (qui se basait sur la largeur) par un positionnement relatif direct (`top: 66.666%; transform: translateY(-50%)`), plaçant l'axe des boutons M et S exactement au centre vertical des pistes audio.
  - **Passage effectif à un fond presque blanc ultra-lumineux (`#f4f7fa`) pour toutes les pistes A, B et C (`src/app.css`)** : Suppression de l'ancien sélecteur de spécificité `.timeline-lane:nth-child(2)` qui forçait un fond sombre `#0f1118`, permettant au fond blanc anodisé ultra-clair (`#f4f7fa`) de s'appliquer réellement sur l'ensemble des pistes.
- Validation à 100 % des 53 tests Vitest.

## 2026-07-21

### Ajouté

- Début du jalon `0.0.19` Smart Filter : sous-pistes Filter visibles sous A/B/C, ligne centrale Bypass, moitié haute HP et moitié basse LP.
- Clic pour poser un Filter Node, glisser-déposer pour l'éditer, avec menu contextuel pour ajouter, réinitialiser ou supprimer.
- Le script de développement limite Cargo à un job afin de rester fiable sur une machine dont la mémoire disponible est faible pendant la reconstruction Tauri.
- Correction du transport en pause : le polling ne resynchronise plus le playhead sur la position du flux audio mis en pause; la position exacte de pause est conservée.
- Correction du jump au toggle Play/Pause : le transport musical devient l'unique source du playhead; l'horloge interne du lecteur audio ne peut plus le recaler pendant le playback.
- Le playhead reste désormais centré aussi en pause : la marge virtuelle et le défilement de suivi ne sont plus ajoutés ou retirés au toggle Play/Pause.
- Correction du glitch de zoom : le défilement ancré au playhead est désormais appliqué avant l'affichage de la nouvelle échelle, sans position intermédiaire à l'extrémité de la timeline.
- Désactivation de l'ancrage automatique du navigateur dans la zone Timeline afin qu'il ne concurrence pas le recentrage de zoom.
- Les impulsions de molette et les touches `R`/`T` sont désormais regroupées à une cadence d'image, avec une amplitude bornée, afin d'empêcher les échelles intermédiaires visibles pendant un zoom.
- Le défilement horizontal natif du viewport Timeline est neutralisé tandis que son `scrollLeft` programmatique reste actif. Cela supprime la frame observée dans la capture où WebView2 déplaçait d'abord tout le contenu — playhead compris — dans le sens opposé avant l'application du zoom.
- Après analyse de la capture 60 fps, le recentrage programmatique par `scrollLeft` est entièrement retiré du zoom et du suivi du transport. La largeur musicale et la translation `translate3d` centrée sont maintenant publiées ensemble par React; aucune position calculée avec l'ancienne échelle ne peut être composée séparément par WebView2.
- La capture suivante révèle que WebView2 pouvait encore actualiser le calque GPU `translate3d` avant le layout de ses enfants. Cette transformation est remplacée par la propriété de layout `left`, forçant la grille, les clips, les waveforms et le playhead à être repeints dans la même frame.
- Les sous-pistes Filter dessinent maintenant de vraies courbes de sweep, au lieu de segments droits. `Shift + molette` sur un segment règle sa tension, persistée dans SQLite et appliquée avec la même formule dans React et le DSP Rust.
- Ajout du schéma SQLite 14 et de `timeline_filter_nodes` : valeur bipolaire persistante, snapping au quart de beat et aimantation à zéro dans une zone de ±5 %.
- Ajout du premier moteur de filtre par lane après le time-stretch : biquad Butterworth Q fixe, mapping logarithmique 20 Hz–20 kHz, mix sec/filtré et lissage 8 ms.
- Les sweeps Filter utilisent maintenant des bornes musicales (LP 18 kHz→90 Hz, HP 50 Hz→12 kHz) et un gain de compensation après filtrage. Ce gain progresse linéairement en dB jusqu'à +6 dB en LP ou +4,5 dB en HP, pour conserver une sensation de niveau plus stable sans masquer les vrais dépassements `OL`.
- Correction du calcul Filter au frame exact d'un node : sa valeur est désormais appliquée immédiatement au lieu d'un bypass ponctuel.
- Les Filter Nodes et leurs menus disparaissent de l'interface au profit du **Filter Brush** : un drag vertical dessine une bulle lissée, avec retour automatique à la bande complète. Sa largeur est de deux mesures par défaut et se règle horizontalement pendant le geste; les échantillons invisibles sont persistés atomiquement afin que le rendu audio corresponde immédiatement à la forme affichée.
- La bande Filter au repos est assombrie et les bulles dessinées reçoivent un remplissage or lumineux, un contour crème et une lueur plus marquée : la forme active domine désormais clairement la grille de fréquences par défaut.
- Les bulles Filter différencient maintenant leur direction sans perdre leur highlight : bleu profond pour un passe-haut, rouge profond pour un passe-bas, avec le même contour crème lumineux.
- Le point où commence un Filter Brush est désormais son vrai début temporel, plutôt que le centre de la bulle. La durée par défaut reste deux mesures; un drag horizontal vers la droite règle directement sa fin.
- Un geste dans une bulle Filter existante l'édite maintenant directement : sa durée est préservée et le drag vertical règle son intensité, au lieu d'écraser la zone avec une nouvelle bulle.
- Les waveforms Timeline abandonnent leur palette brune : fond noir/gris, crêtes blanc cassé et RMS gris neutre pour une lecture plus nette de type DAW.
- Les blocs de clips et leurs en-têtes passent eux aussi en gris anthracite neutre, légèrement plus clair que le fond waveform : aucun brun ne subsiste dans cette zone, sans rendre les tracks inutilement sombres.
- Correction des Volume Nodes : le clic droit est maintenant capté par la surface complète A/B/C, peut créer une node à travers un clip, et identifie correctement B ou C au lieu de toujours retomber sur A.
- Passage au jalon `0.0.17` avec remplacement du raccord granulaire fixe par un moteur WSOLA maison stéréo-lié.
- Ajout d'une recherche de corrélation normalisée autour de chaque position source cible; le décalage retenu est commun aux canaux gauche et droit afin de préserver leur phase relative.
- Ajout de régressions DSP couvrant le fondu complémentaire et la capacité de la recherche WSOLA à rapprocher deux formes d'onde déphasées.
- Ajout de la lecture des tags ID3 artiste/titre à l'import des MP3 : ID3v2.2, v2.3 et v2.4 pour `TP1`/`TPE1` et `TT2`/`TIT2`, avec repli ID3v1.
- Ajout du schéma SQLite 12 (`artist`, `title`, `id3_scanned`) et d'un rattrapage automatique des morceaux déjà indexés et encore accessibles, sans exiger de réimportation.
- Ajout du schéma SQLite 13 et de `timeline_clips.tempo_anchor_beat`, initialisé depuis l'ancre existante afin que les cibles BPM deviennent indépendantes des clips audio.

### Modifié

- Pendant Play, le playhead reste au centre de la zone timeline et le contenu défile continuellement sous lui. Une marge virtuelle de demi-viewport de chaque côté conserve ce comportement dès le premier beat et jusqu'à la fin du projet.
- Le zoom à la molette respecte ce nouveau suivi pendant la lecture; hors Play, il conserve son recentrage existant autour du playhead.
- La ligne d'automation affichée et le gain audio de toute piste sans node passent de `0 dB` à `−6 dB`.
- Chaque nouveau clip crée une node `−6 dB` à son début et à sa fin audio; une node existante à la même position est préservée.
- Les nodes situées dans l'intervalle audio d'un clip suivent son déplacement horizontal et vertical. Les nodes extérieures restent en place, et une collision externe annule le déplacement sans perte d'automation.
- Le hop de synthèse passe de 1 024 à 512 frames et les raccords utilisent maintenant un fondu cosinus à bords doux plutôt qu'un crossfade linéaire entre grains arbitrairement déphasés.
- L'interpolation cubique à quatre points remplace l'interpolation linéaire lors de la lecture d'une position source fractionnaire.
- La timeline adopte la fréquence réelle du périphérique de sortie au lieu de déclarer systématiquement 44,1 kHz; cela évite la seconde conversion effectuée auparavant lorsque Windows fonctionne à 48 kHz.
- Les coefficients du VU sont calculés depuis leurs constantes de temps et la fréquence de sortie réelle, ce qui conserve les mêmes ballistiques à 44,1 et 48 kHz.
- La fenêtre PCM réserve dès son ouverture la capacité requise par le WSOLA afin d'éviter ses réallocations récurrentes.
- Les scripts `dev.ps1` et `check.ps1` invoquent désormais les exécutables verrouillés de `node_modules/.bin` directement; ils ne dépendent plus de la résolution imbriquée de `pnpm exec`.
- Le bouton global `Analyze` et les boutons `Analyze`/`Retry` de la Library sont retirés. L'import reste automatiquement analysé; toute vérification ou réanalyse manuelle se fait désormais dans Beatgrid.
- La Library affiche désormais `Artist - Trackname` lorsqu'ils sont présents dans les tags ID3. En l'absence partielle ou totale de tags, elle utilise le titre seul, l'artiste seul, puis le nom de fichier comme repli.
- Une entrée de Library déjà utilisée sur la timeline est maintenant repérée par une bordure verte et le badge `IN USE`; elle reste ajoutable afin de ne pas empêcher un doublage volontaire.
- Ajout d'un tri local compact `Artist`, `Track`, `BPM` et `In Use`; un second clic sur le critère actif inverse le sens. La Library reçoit une finition visuelle inspirée d'une console analogique, avec nuances crème, laiton et vert de contrôle.
- Le bouton `×` est retiré des lignes de Library. `Remove Track` est maintenant disponible au clic droit, ce qui libère l'espace pour les titres; le bouton d'ajout à la timeline devient un disque vinyle avec un repère `+`.
- La Library est éclaircie vers une façade crème/laiton et ses boutons `Add Folder`, `+ MP3`, Preview, BPM et tri adoptent le même langage analogique.
- Le badge `IN USE`, le pourcentage de confiance et l'indication textuelle `Manual` sont retirés. L'utilisation reste visible par un overlay bleu-gris discret, et une correction manuelle reçoit un encadré crème autour du BPM.
- Les contrôles BPM et Preview sont inversés : BPM est maintenant adjacent au titre, Preview est à droite dans la cellule compacte dédiée.
- La sortie audio commune des mini-players Library et Beatgrid demande désormais un tampon de 4 096 frames. Cela réduit les sous-alimentations pendant le décodage MP3 en continu, qui peuvent être entendues comme une granularité ou une vitesse instable; le VBR n'est pas une variation de pitch.
- Un clic dans la timeline avec une Preview ouverte bascule automatiquement vers l'audio timeline au beat cliqué. Il ne tente plus de Seek un cache dont la sortie a été libérée par la Preview, ce qui supprimait le message `Unable to continue`.
- Les raccourcis `R` et `T` effectuent désormais respectivement un zoom arrière et avant de la timeline, selon la même mécanique de recentrage que la molette.
- Les nœuds turquoise BPM déplacent maintenant uniquement leur cible de tempo : le clip et ses Volume Nodes restent immobiles. La cible conserve le snapping à la mesure et ne peut pas sortir de la longueur visuelle de son clip; un déplacement de clip translate toutefois sa cible du même delta.
- Toute la hauteur du ruler BPM est une zone de drag permissive : elle choisit la cible de clip la plus proche, sans demander de viser un point de quelques pixels, et affiche le curseur main uniquement dans cette zone.
- La colonne fixe de pistes A/B/C est retirée. Les seules commandes Mute/Solo sont désormais des boutons ronds M/S flottants, ancrés à gauche de chaque voie sans consommer de largeur musicale ni disparaître au zoom. La timeline adopte aussi la surface crème/laiton, les pistes olive et les clips brun-or de la Library.
- Le Beatgrid Editor reçoit la même finition crème, laiton et noyer que la Library et la timeline. À son ouverture, son mini-player charge automatiquement le MP3 sélectionné, à zéro et en pause; le bouton `Load Preview` n'est donc plus nécessaire.
- Le VU master à aiguilles est remplacé par deux rangées de grandes LED circulaires horizontales stéréo, directement posées sur le panneau sans boîtier brun, graduées vert/orange/rouge selon la même calibration VU. Le témoin de surcharge devient un voyant circulaire `OL` distinct. Les niveaux master sont maintenant remontés même si Rodio signale brièvement un état Play ambigu, ce qui évite un affichage figé pendant une lecture valide.

### Corrigé

- Suppression de la principale cause du trémolo, de la granulation et du filtrage en peigne : l'ancien moteur fondait périodiquement deux segments non alignés toutes les 23 ms.
- Le témoin `OL` s'allume maintenant dès que le signal pré-sortie dépasse réellement la borne de protection de `0,98`, et non seulement à partir de `1,0`.
- La conversion MP3/PCM vers float32 est confirmée comme non responsable des artéfacts : elle demeure le format interne normal et fournit le headroom voulu.

### Validation

- Les cinquante tests Rust réussissent, incluant les régressions WSOLA, le gain de repos à `−6 dB`, les nodes automatiques, leur déplacement sélectif avec un clip, la lecture ID3v2, la bascule Preview→timeline et la borne persistante d'une cible BPM indépendante.
- Les cinquante-trois tests TypeScript, la vérification des types et le build Vite de production réussissent; ils couvrent notamment le libellé `Artist - Trackname`, ses replis, les tris de Library et les raccourcis de zoom.
- Le modèle de tempo demeure linéaire en BPM dans l'espace musical des beats et ses conversions beat↔temps restent exactes; l'impression de gradation irrégulière provenait principalement des raccords audio quantifiés et déphasés.

## 2026-07-19

### Ajouté

- Ajout d'une horloge de transport de timeline dans Rust, exprimée en beats et fondée sur une horloge monotone.
- Activation du bouton Play/Pause principal lorsque la timeline contient au moins un clip.
- Ajout d'un playhead visible traversant la règle et la piste.
- Ajout du repositionnement du playhead par clic sur la règle ou dans la piste.
- Ajout du raccourci Espace pour basculer Play/Pause hors des champs et boutons interactifs.
- Ajout de cinq tests TypeScript pour le zoom global et de deux tests Rust pour l'horloge du transport.
- Ajout de la lecture audible de la timeline et passage au jalon `0.0.6`.
- Ajout d'un décodeur de rendu MP3 vers stéréo float32 à 44,1 kHz.
- Ajout d'un time-stretch temporel maison conservant la tonalité, sans varispeed.
- Ajout du mixage float32 des clips superposés et d'une protection de sortie à 0,98.
- Ajout d'une fenêtre PCM circulaire alimentée en continu pour les reprises, changements de BPM et repositionnements instantanés.
- Ajout de cinq tests DSP et d'un test de synchronisation du transport sur l'audio.
- Ajout de trois tests garantissant que la barre d'espace contrôle la timeline sans réactiver un bouton ciblé.
- Passage au jalon `0.0.7` avec remplacement des waveforms décoratives par les crêtes réelles des MP3.
- Ajout de quatre enveloppes min/max float32 sur 2 048 colonnes pour préserver séparément les canaux gauche et droit.
- Ajout du cache SQLite `track_waveforms` et de la migration automatique du schéma 4 vers le schéma 5.
- Ajout d'un rattrapage asynchrone pour tous les anciens morceaux présents dans la bibliothèque, même s'ils n'ont jamais été placés dans la timeline.
- Ajout d'un composant SVG mémorisé et de sept tests supplémentaires couvrant les crêtes, le mono, leur sérialisation, leur sélection depuis la bibliothèque et leur géométrie visuelle.
- Passage au jalon `0.0.8` avec trois pistes stéréo visibles et persistantes.
- Ajout du dépôt direct d'un morceau de la bibliothèque sur chacune des trois pistes.
- Ajout du déplacement vertical et horizontal d'un clip dans un seul geste, avec snapping du beat conservé.
- Ajout de la migration SQLite du schéma 5 vers le schéma 6, qui conserve les clips existants sur la première piste.
- Ajout de trois tests TypeScript couvrant la sélection verticale et ses bornes.
- Ajout d'un test TypeScript supplémentaire couvrant le centrage du playhead aux deux extrémités de la timeline.
- Passage au jalon `0.0.9` avec scrub Preview, Mute/Solo et validation explicite du beatmatching.
- Ajout d'une barre de Preview cliquable et déplaçable qui conserve l'état Play/Pause.
- Ajout de boutons Mute et Solo minimalistes sur la gauche des trois pistes.
- Ajout de la persistance des états de piste et de la migration SQLite du schéma 6 vers le schéma 7.
- Ajout de deux tests TypeScript sur l'ordre BPM-avant-Play et de trois tests Rust sur la conversion 125↔120 BPM, le partage temps réel du masque et la logique Mute/Solo.
- Passage au jalon `0.0.10` avec snapping 4/4, détection du premier temps et beatgrid source uniforme.
- Ajout d'une détection du downbeat qui compare l'accent moyen des quatre phases de mesure.
- Ajout d'une colonne fixe réservée aux commandes Mute/Solo des trois pistes.
- Ajout de trois tests TypeScript pour le snapping par mesure et de trois tests Rust pour le downbeat, l'uniformité de la grille et la migration des ancres.
- Passage au jalon `0.0.11` avec raffinement BPM à longue portée et cache d'analyse versionné.
- Ajout de corrélations successives à 8, 16, 32 et 64 beats pour mesurer la dérive de phase sur toute la chanson.
- Ajout d'une optimisation globale de l'origine de pulsation autour de la première attaque significative.
- Ajout d'un outil de diagnostic local qui exécute le même analyseur que l'application sur des MP3 réels.
- Ajout de deux régressions Rust couvrant la correction `127,60 → 128,00 BPM` sur six minutes et la récupération de phase globale.
- Passage au jalon `0.0.12` avec alternance automatique des pistes et édition des clips pendant Play.
- Ajout de la rotation persistante P1 → P2 → P3 au clic sur le bouton ↦ de la bibliothèque.
- Ajout d'une surbrillance de la piste ciblée pendant un glisser-déposer depuis la bibliothèque.
- Ajout d'un test TypeScript de trois scénarios pour la rotation des pistes et d'un test Rust garantissant que l'actualisation d'un plan ne pré-décode pas les MP3.
- Ajout d'un drag-and-drop interne fondé sur Pointer Events, avec capture de la souris, seuil de 6 px et badge flottant indépendant du mécanisme HTML refusé par WebView2.
- Ajout de trois tests couvrant la distinction clic/drag, la piste et la mesure sous le pointeur, ainsi que les limites du viewport visible.
- Passage au jalon `0.0.13` avec waveform DAW haute définition et zoom de timeline atomique.
- Passage de 2 048 à un maximum de 16 384 colonnes min/max stéréo par morceau.
- Ajout des enveloppes RMS gauche/droite afin de distinguer le corps sonore des crêtes transitoires.
- Ajout d'une pyramide de niveaux de détail qui conserve les extrema et combine l'énergie RMS quadratique.
- Ajout de deux tests TypeScript pour la pyramide, d'un test de tracé RMS et d'une régression Rust pour la migration du cache waveform.
- Passage au jalon `0.0.14` avec carte de tempo progressive partagée par le transport et le moteur audio.
- Ajout automatique d'une cible BPM à chaque ancre turquoise, égale au BPM effectif du clip.
- Ajout de l'interpolation linéaire du BPM entre les cibles et des conversions exactes beat↔temps à travers chaque rampe.
- Ajout d'une rampe turquoise dans la règle avec un point et un libellé BPM pour chaque cible de clip.
- Ajout de quatre tests Rust de carte de tempo, d'un test d'horloge variable, d'un test de position source pendant une rampe et de deux tests TypeScript du tracé.
- Passage au jalon `0.0.15` avec VU-mètre master stéréo analogique au centre de l'interface.
- Ajout de deux enveloppes VU temps réel alimentées par les canaux gauche et droit du vrai bus master float32.
- Ajout de deux cadrans crème encadrés de bois sombre et laiton, avec échelle −20 à +3 dB et aiguilles rouges indépendantes.
- Ajout de deux tests Rust pour les ballistiques et l'indépendance stéréo, ainsi que trois tests TypeScript de calibration du cadran.
- Passage au jalon `0.0.16` avec automation de volume indépendante sur les pistes A, B et C.
- Ajout des menus contextuels `Add Volume Node` et `Delete Volume Node`, du déplacement horizontal/vertical et de la plage `−∞ à +12 dB`.
- Ajout de la table SQLite `timeline_volume_nodes`, de la migration automatique du schéma 10 vers le schéma 11 et de l'actualisation pendant Play.
- Ajout d'un témoin vintage rouge `OL` avec rémanence, piloté par le master float32 avant la protection de sortie.
- Ajout d'un mini-transport Preview avec scrub sous les commandes de Library et dans l'éditeur Beatgrid.
- Ajout d'un test Rust de persistance des Volume Nodes et d'une régression garantissant qu'une correction du premier downbeat est adoptée par un clip déjà présent.

### Modifié

- Le gain automatique fondé sur le nombre maximal de clips superposés est retiré : à `0 dB`, la timeline conserve désormais le niveau nominal de la Preview et le mixage float32 fournit le headroom interne.
- Chaque piste interpole ses Volume Nodes linéairement en dB avant la sommation master; l'absence de point conserve un gain unitaire.
- Le transport principal utilise deux boutons mécaniques distincts, Play vert et Pause orangé, avec rétroéclairage d'état.
- L'en-tête affiche maintenant `Tracks`, `Total Time` et le BPM courant mis à jour pendant la lecture; Tap et les boutons Zoom sont retirés.
- Les pistes P1/P2/P3 deviennent A/B/C et la colonne `4/4` devient `BPM`.
- Library, ses commandes et l'intégralité de l'éditeur Beatgrid sont traduits en anglais.
- Le Preview est déplacé sous `Analyze`, `Add Folder` et `+ MP3` afin de libérer la hauteur centrale.

- La borne minimale de zoom dépend maintenant de la largeur visible et de la longueur complète du mix; le zoom extérieur maximal montre tous les clips.
- Les libellés de mesure s'espacent automatiquement lorsque le nombre de pixels par beat devient très faible.
- La Preview disparaît entièrement tant qu'aucun morceau n'est chargé et devient ensuite un indicateur compact avec Play/Pause seulement.
- Chaque morceau de la bibliothèque tient maintenant sur une seule ligne compacte.
- Les commandes Preview et Retirer sont placées directement à côté du titre du morceau.
- Le polling du playhead utilise une requête SQLite agrégée limitée au BPM et à la fin du projet.
- Play vérifie uniquement l'ouverture des MP3; leur contenu est décodé progressivement pendant la lecture.
- Le BPM, le Tap Tempo, la beatgrid et la suppression de clips sont verrouillés pendant la lecture principale; Pause, Seek, ajout et déplacement de clips demeurent disponibles.
- Démarrer la Preview met la timeline en pause, et démarrer la timeline met la Preview en pause.
- Le playhead est recadré sur la position réelle du lecteur audio dès qu'un rendu existe.
- L'analyse BPM collecte maintenant la waveform durant son passage de décodage existant, sans second décodage pour les nouveaux morceaux.
- Le cache est maintenant préparé dès qu'un morceau appartient à la bibliothèque; l'ajout ultérieur du clip demeure immédiat et trouve sa waveform déjà prête.
- L'ajout automatique à la suite utilise maintenant la fin de la piste choisie plutôt que la fin globale des trois pistes.
- Le titre de la timeline et chaque clip indiquent maintenant clairement leur piste stéréo.
- La molette et les boutons de zoom recentrent maintenant automatiquement la timeline sur le playhead.
- Chaque clip indique maintenant son BPM source et le BPM projet réellement ciblé par le time-stretch.
- Mute et Solo peuvent être modifiés pendant la lecture sans remettre le lecteur à zéro.
- Le premier temps source se cale maintenant sur un multiple de quatre beats dans React et dans l'autorité Rust.
- Les positions automatiques de beats utilisent désormais une période globale uniforme au lieu de suivre et cumuler les déplacements d'attaques locales.
- Les harmoniques ×2 et ×4 contribuent aussi au raffinement sous-frame de la période BPM.
- L'éditeur de beatgrid nomme explicitement le « Premier temps (1) » et conserve la correction manuelle comme autorité.
- Le libellé de piste, Mute et Solo sont maintenant empilés verticalement; la colonne fixe passe de 76 à 42 px.
- L'indice de confiance combine maintenant périodicité courte, cohérence à longue portée et séparation des candidats.
- Les anciens caches sont réanalysés automatiquement une seule fois en arrière-plan; les nouveaux imports utilisent directement l'algorithme courant.
- Le bouton ↦ indique la prochaine piste automatique, alterne selon le clip créé le plus récemment et ajoute maintenant le morceau sur la mesure la plus proche du playhead.
- Le dépôt utilise maintenant toute la surface des pistes, y compris la zone occupée par un autre clip.
- La rangée ciblée et le badge de déplacement deviennent verts lorsque le pointeur entre dans une destination valide.
- Un ajout ou un déplacement pendant Play reconstruit seulement les relations temporelles, conserve la position audio courante et resynchronise le transport sur le nouveau plan.
- La waveform choisit maintenant le niveau le plus léger qui couvre encore la largeur rendue du clip.
- Le rendu stéréo superpose une crête fine, un corps RMS plus dense et un axe zéro par canal.
- Les événements de molette rapides calculent chaque facteur depuis la dernière valeur effective plutôt que depuis une fermeture React périmée.
- Le recentrage du zoom est appliqué après la nouvelle mise en page et avant son affichage à l'écran.
- Le champ BPM projet et le Tap Tempo pilotent maintenant le BPM de départ; les ancres de clips pilotent les cibles suivantes.
- Le time-stretch d'un clip n'utilise plus un ratio fixe : le début de chaque grain retrouve sa position source depuis le BPM courant de la carte globale.
- Un ajout ou déplacement pendant Play conserve le beat musical courant même si la nouvelle cible modifie la durée écoulée de la timeline.
- Chaque clip affiche maintenant son propre BPM comme « BPM cible » au lieu d'annoncer un master constant.
- Le snapshot de transport transporte maintenant les niveaux master L/R dans sa boucle existante de 50 ms.
- L'en-tête réserve son centre au VU-mètre et adapte sa disposition sur les fenêtres plus étroites.

### Décisions d'architecture

- `NULL` représente `−∞ dB` dans SQLite; `−60 dB` sert uniquement de borne d'interpolation et non de silence exact.
- La mesure VU et `OL` précède la borne de sortie à 0,98 afin que l'interface révèle une surcharge que la protection physique masque nécessairement.
- Le zoom de la molette publie la largeur React et recalcule `scrollLeft` dans le même événement DOM non passif, sans phase de recentrage différée.

- Rust demeure l'autorité du transport; après préparation, le lecteur audio natif est son horloge et React ne fait qu'interroger et afficher sa position.
- La lecture 0.0.6 est calculée à la demande à partir de petites fenêtres PCM; elle ne produit plus de rendu monolithique du projet et ne pré-décode plus les chansons complètes.
- Le changement de BPM reconstruit uniquement les relations temporelles des clips et ne recalcule aucun fichier audio.
- La limite de sécurité du projet passe de 20 minutes à quatre heures; les ratios de time-stretch demeurent temporairement de 0,5× à 2×.
- Le BPM et les corrections de beatgrid ne peuvent pas être modifiés depuis l'interface pendant la lecture; un ajout ou déplacement de clip remplace explicitement le plan actif afin qu'il ne devienne jamais périmé.
- Les waveforms sont un cache d'analyse reproductible, séparé du projet et entièrement absent du chemin audio temps réel.
- Le schéma 10 conserve six enveloppes float32 min/max/RMS comme blocs little-endian compacts; aucun PCM et aucun nouveau moteur audio ne traversent l'IPC Tauri.
- La pyramide dérivée reste en mémoire dans React et n'est jamais persistée comme donnée créative.
- Les trois pistes partagent une seule grille et un seul tempo global; leur contenu actif est sommé dans le bus stéréo float32 existant.
- Le numéro de piste participe à la signature du plan audio afin qu'un déplacement vertical invalide correctement l'état préparé.
- Les états Mute/Solo sont réduits à un masque atomique partagé avec les clones déjà placés dans la file audio.
- La position d'un clip est la composition d'une seule carte beat→temps globale et de sa grille source; le recouvrement granulaire conserve la hauteur pendant que le ratio de tempo varie.
- Les cibles BPM sont dérivées des ancres et des analyses existantes plutôt que persistées séparément; déplacer un clip ou corriger son BPM met donc automatiquement la courbe à jour.
- Si plusieurs clips partagent une ancre, le clip dont l'identifiant est le plus récent devient la cible déterministe à ce point.
- Le meter observe le signal après sommation, Mute/Solo, protection de headroom et borne de sortie; il demeure entièrement absent du traitement sonore.
- Les enveloppes sont calculées dans la source audio, publiées par atomiques et lues sans verrou supplémentaire par l'interface.
- Le VU-mètre est la première expérience du langage « studio vintage »; aucun restylage global n'est engagé avant son évaluation visuelle.
- La Preview demeure un chemin cue séparé et ne fait pas bouger le VU du master de timeline.
- Le transport se met automatiquement en pause à la fin du dernier clip et repart du début si Play est demandé depuis cette borne.
- L'ancre d'un clip représente le temps 1 de la source sur une frontière de mesure 4/4 du projet; le snapping sur un beat arbitraire n'est plus utilisé dans ce prototype.
- La détection automatique du downbeat demeure heuristique; les intros ambiguës se corrigent sans modifier le MP3 depuis la Preview.

### Corrigé

- Suppression de l'atténuation statique qui rendait le Preview nettement plus fort que la timeline.
- Le témoin `OL` signale maintenant une somme master supérieure ou égale à 0 dBFS même si la sortie entendue est ensuite bornée.
- La molette ne dépend plus d'un `useLayoutEffect` différé susceptible d'afficher une frame dans la direction opposée.
- La sauvegarde d'un premier downbeat est désormais couverte par une régression sur un clip déjà présent et par le rafraîchissement immédiat du snapshot React.

- Suppression du rendu PCM couvrant toute la timeline, qui pouvait consommer plus de 500 Mo et bloquer perceptiblement l'application sur un mix de 18 minutes.
- Une occurrence ne décode maintenant que la zone MP3 qu'elle est en train de lire au lieu de produire une copie audio time-stretchée complète.
- La Preview et la timeline libèrent leur périphérique avant de céder la lecture à l'autre, évitant l'ouverture simultanée de deux sorties Windows.
- La barre d'espace pilote maintenant Play/Pause même lorsqu'un bouton conserve le focus et empêche la répétition de l'action sélectionnée.
- Play attend maintenant qu'une modification du BPM projet déclenchée par la perte de focus soit enregistrée; l'ancien tempo ne peut plus gagner cette course.
- Les clips dont les ancres avaient des phases différentes modulo quatre sont recalés automatiquement par la migration SQLite 7 vers 8.
- Les commandes Mute/Solo ne défilent plus avec le contenu et ne peuvent plus cacher le nom d'un clip, quel que soit le zoom.
- Le drag depuis ↦ ne déclenche plus le curseur « interdit » du WebView : aucun drag natif n'est lancé.
- La molette ne produit plus un flash bref dans la direction opposée avant son zoom réel.
- Le transport de secours ne suppose plus un BPM constant entre deux lectures du périphérique audio; il utilise l'inverse exacte de la carte de tempo.
- Les aiguilles reviennent au repos sur Pause, Seek, remplacement du plan et fin de timeline au lieu de conserver une ancienne mesure.

### Diagnostic

- Les deux clips d'exemple sont correctement ancrés sur des mesures : `4 modulo 4 = 0` et `28 modulo 4 = 0`; le snapping n'explique donc plus leur dérive progressive.
- Gutes Nitzwerk est analysé à 127,60 BPM avec seulement 24,2 % de confiance, contre un tempo de référence de 128 BPM; l'écart de 0,40 BPM produit environ un beat de dérive en 150 secondes.
- Jestrüpp est analysé à 125,92 BPM contre un tempo de référence de 126 BPM; après adaptation au master 127,60, sa pulsation réelle devient environ 127,68 BPM.
- Le moteur granulaire utilise des positions absolues par grain : son écart cyclique reste inférieur à un grain et ne s'accumule pas. Avec ces deux estimations, la dérive relative prévue est d'environ 0,32 BPM, soit un beat toutes les 188 secondes, ce qui correspond au symptôme observé.
- Le diagnostic a établi qu'il fallait raffiner le BPM par cohérence de phase sur toute la chanson; rendre simplement les intervalles persistés uniformes ne corrigeait pas une période globale erronée.
- Le nouvel analyseur exécuté directement sur les deux fichiers donne maintenant `128,00 BPM / 371 ms / 781 beats` et `126,00 BPM / 9 565 ms / 888 beats`.

### Validation

- Les trente tests TypeScript et les trente-six tests Rust réussissent.
- Le formatage Rust et Clippy réussissent avec tous les avertissements traités comme des erreurs.
- Le cadrage sans Preview a été vérifié : la timeline récupère toute la hauteur de travail et aucun défilement vertical global n'apparaît.
- La compilation frontend de production réussit et l'application native redémarre après migration du schéma SQLite 6 vers le schéma 7.
- La base utilisateur réelle conserve ses dix morceaux, leurs dix waveforms et ses deux clips; les trois états Mute/Solo sont initialisés sans modifier le projet existant.
- MixCanvas 0.0.9 redémarre depuis les dépendances locales du dépôt et sa fenêtre native demeure réactive.
- La compilation frontend 0.0.10 réussit; le formatage, Clippy et l'ensemble des tests réussissent avec les dépendances locales verrouillées.
- La base utilisateur réelle migre au schéma 8 et ses deux ancres deviennent `4` et `620`, toutes deux congruentes à `0 modulo 4`.
- À 1 280 × 720, la vérification visuelle mesure six boutons Mute/Solo visibles dans une colonne de 76 px entièrement séparée du viewport horizontal, sans défilement du document.
- MixCanvas 0.0.10 redémarre dans sa fenêtre native après la migration et demeure réactive.
- La colonne verticale compacte a été revérifiée à 1 280 × 720 : elle mesure 42 px, les six boutons restent visibles et le viewport de timeline gagne 34 px sans défilement du document.
- La compilation frontend 0.0.11, le formatage Rust et Clippy réussissent avec tous les avertissements traités comme des erreurs.
- La base utilisateur réelle migre au schéma 9 et réanalyse automatiquement ses deux anciens caches : les résultats persistés deviennent 128,00 et 126,00 BPM avec `analysis_version = 1`.
- L'application native 0.0.11 redémarre après la migration, ne relance pas les caches déjà courants et demeure réactive.
- Les trente-six tests TypeScript et les trente-sept tests Rust de la version 0.0.12 réussissent.
- La compilation frontend de production, le formatage Rust et Clippy réussissent pour la version 0.0.12.
- L'application native 0.0.12 redémarre depuis les dépendances locales du dépôt et demeure prête pour l'essai musical du déplacement pendant Play.
- Les trente-neuf tests TypeScript et les trente-huit tests Rust de la version 0.0.13 réussissent.
- La compilation frontend de production, le formatage Rust et Clippy réussissent pour la version 0.0.13.
- La base utilisateur migre au schéma 10 et régénère ses deux waveforms à 16 380 et 16 382 colonnes; les blocs RMS ont exactement la même longueur que les blocs de crêtes.
- L'application native 0.0.13 redémarre depuis les dépendances locales du dépôt avec les nouveaux caches chargés.
- Les quarante-et-un tests TypeScript et les quarante-trois tests Rust de la version 0.0.14 réussissent, dont les régressions de rampe, de Seek et de time-stretch variable.
- La compilation frontend de production, le formatage Rust et Clippy réussissent pour la version 0.0.14.
- L'application native 0.0.14 redémarre depuis les dépendances locales du dépôt avec la carte de tempo chargée depuis le projet existant.
- Les quarante-quatre tests TypeScript et les quarante-cinq tests Rust de la version 0.0.15 réussissent.
- La compilation frontend de production, le formatage Rust et Clippy réussissent pour la version 0.0.15.
- L'application native 0.0.15 redémarre depuis les dépendances locales du dépôt avec les deux cadrans master montés au centre.
- Les quarante-six tests TypeScript et les quarante-cinq tests Rust de la version 0.0.16 réussissent.
- La compilation frontend de production, le formatage Rust et Clippy réussissent pour la version 0.0.16.
- La base utilisateur réelle migre au schéma 11 sans modifier ses sept morceaux ni ses six clips existants.
- La disposition native 0.0.16 est vérifiée à 1 296 × 799 : Play/Pause vintage, VU/OL, BPM, Tracks/Total Time, A/B/C et Library demeurent visibles sans défilement du document.
- Le menu contextuel `Add Volume Node` et l'éditeur Beatgrid anglais sont vérifiés dans la fenêtre native; aucun MP3 ni point créatif n'est modifié pendant cette QA.

## 2026-07-18

### Ajouté

- Création du document vivant `architecture.md`.
- Création du journal quotidien `changelog.md`.
- Ajout du fichier `LICENSE` contenant le texte officiel de la GNU Affero General Public License version 3.
- Création du squelette de l'application desktop MixCanvas 0.0.1.
- Ajout d'une interface React et TypeScript avec identité visuelle initiale.
- Ajout d'un sélecteur natif limité aux fichiers MP3.
- Ajout d'une Preview native avec Play, Pause et retour au début.
- Ajout d'une barre de progression interactive permettant d'avancer ou de reculer dans le MP3.
- Ajout de la commande Rust de déplacement audio, avec conservation de l'état Play/Pause et remise en file d'une piste terminée.
- Passage du projet au jalon `0.0.2`.
- Ajout d'une bibliothèque MP3 persistante fondée sur SQLite embarqué.
- Ajout simultané de plusieurs fichiers MP3.
- Ajout récursif d'un dossier musical et de ses sous-dossiers.
- Ajout d'une liste affichant le chemin, la durée, le format audio, l'état du fichier et le futur état BPM.
- Ajout d'un bouton Preview sur chaque ligne avec Play/Pause de la piste active.
- Ajout du retrait non destructif d'une entrée sans suppression du MP3 original.
- Détection des fichiers déplacés, supprimés ou autrement introuvables.
- Prévention des doublons par une clé de chemin normalisée.
- Ajout d'un résumé d'importation indiquant les ajouts, doublons et fichiers illisibles.
- Passage du projet au jalon `0.0.3`.
- Ajout d'un analyseur BPM et beatgrid maison entièrement écrit en Rust.
- Ajout d'une enveloppe d'énergie compacte, d'une détection d'attaques à seuil adaptatif et d'une autocorrélation entre 70 et 190 BPM.
- Ajout d'un suivi local des attaques pour éviter la dérive cumulative de la beatgrid.
- Ajout d'un indice de confiance, du premier beat détecté et de toutes les positions de beats.
- Ajout des commandes « Analyser », « Réessayer », « Réanalyser » et « Tout analyser » dans la bibliothèque.
- Déclenchement automatique de l'analyse BPM pour chaque nouveau MP3 importé.
- Ajout d'états visuels pour une analyse en cours, réussie ou échouée.
- Ajout d'une exclusion empêchant deux séries d'analyses simultanées.
- Passage du projet au jalon `0.0.4`.
- Ajout d'un éditeur de correction de beatgrid ouvert depuis la valeur BPM d'un morceau.
- Ajout de la saisie manuelle du BPM et des commandes ×2 et ÷2.
- Ajout d'un Tap Tempo conservant neuf frappes, fondé sur la médiane et réinitialisé après deux secondes.
- Ajout de la capture du premier beat à partir de la position courante de la Preview.
- Ajout de la restauration instantanée du BPM, du premier beat et de la beatgrid automatiques.
- Ajout d'un indicateur « Manuel » dans la bibliothèque pour les morceaux corrigés.
- Passage du projet au jalon `0.0.5`.
- Ajout d'une première piste stéréo sur une timeline musicale horizontale et zoomable.
- Ajout d'une règle en mesures de quatre beats et d'une grille visuelle par beat.
- Ajout d'un bouton de bibliothèque pouvant glisser un morceau analysé sur la timeline ou l'ajouter automatiquement à la suite.
- Ajout de clips stéréo affichant le nom, le BPM, la position musicale et une ligne distincte sur le premier beat source.
- Ajout du déplacement horizontal des clips avec snapping sur les beats entiers.
- Ajout de la suppression non destructive d'un clip de la timeline.
- Ajout d'un tempo global constant du projet, initialisé par le BPM du premier clip.
- Ajout de la saisie et du Tap Tempo directement dans l'en-tête de la timeline.
- Ajout de la restauration automatique des clips et du tempo du projet au redémarrage.
- Réorganisation de toute l'interface en poste de travail plein écran centré sur la timeline.
- Déplacement de la bibliothèque dans un panneau latéral droit avec défilement vertical interne.
- Conversion de la Preview en transport compact sous la timeline.
- Ajout du zoom continu de 4 à 96 pixels par beat avec la molette, centré sous le pointeur.
- Conservation des boutons − et + comme commandes de zoom accessibles au clavier.
- Ajout de l'emplacement Play/Pause au transport principal de timeline.
- Affichage de l'éditeur de beatgrid dans une superposition bornée à la fenêtre.
- Affichage du nom, du chemin, de la durée, de la progression, de la fréquence d'échantillonnage et du nombre de canaux.
- Ajout d'une icône MixCanvas et génération des formats desktop nécessaires.
- Ajout d'un README avec les prérequis, l'installation, le lancement et les vérifications.
- Ajout des scripts PowerShell de développement et de validation.
- Ajout des commandes racine `install.ps1`, `dev.ps1` et `check.ps1`, utilisables sans installation globale de pnpm.
- Ajout de lanceurs `.cmd` compatibles avec une politique PowerShell qui bloque l'exécution directe des scripts.
- Ajout d'une amorce Corepack dont le cache pnpm demeure dans `.corepack` sous le dépôt.
- Initialisation du dépôt Git local sur la branche `main`, sans publication distante.
- Définition du périmètre fonctionnel initial de la version 0.1.
- Ajout conceptuel d'un bouton Tap Tempo pour saisir ou modifier un marqueur de tempo.

### Décisions d'architecture

- La timeline comporte trois pistes audio stéréo et une piste globale de tempo.
- Le projet utilise une courbe de tempo globale partagée par toutes les pistes.
- La version 0.1 prend en charge les tempos constants et les progressions linéaires entre deux marqueurs.
- Les positions éditoriales principales sont exprimées en mesures et en beats, puis converties en positions audio par le moteur.
- Les clips suivent le tempo du projet par time-stretch avec conservation de la tonalité, et non par varispeed.
- Le traitement et le mixage internes utilisent des échantillons float32.
- L'édition reste non destructive : les MP3 originaux ne sont jamais modifiés.
- Le moteur audio est l'autorité temporelle; l'horloge de l'interface ne pilote pas la synchronisation.
- Les données du projet et le cache d'analyse restent séparés.
- Le moteur sera préparé pour accueillir des modules DSP internes sans faire des effets une exigence de la version 0.1.
- Les effets maison utiliseront des blocs stéréo float32 et respecteront les contraintes du traitement audio temps réel.
- Tous les effets seront des modules DSP maison compilés directement dans MixCanvas.
- L'hébergement de VST3, Audio Units ou de tout autre plugin tiers est explicitement exclu du produit.
- La sélection des dépendances tiendra compte d'une distribution gratuite et open source sur GitHub.
- Les moteurs d'analyse et de time-stretch resteront remplaçables afin de limiter le couplage à leurs licences.
- Adoption de la licence `AGPL-3.0-only` pour MixCanvas.
- Adoption d'une stack Windows-first composée de Tauri 2, React, TypeScript et Rust.
- La Preview utilise Rodio au-dessus de CPAL et Symphonia; cette couche reste isolée du futur moteur de timeline.
- L'IPC Tauri transporte uniquement des commandes et des états de haut niveau, jamais des blocs PCM.
- Les dépendances JavaScript et Rust, leurs caches et les artefacts de compilation restent dans le dossier du projet.
- Les versions sont verrouillées par `pnpm-lock.yaml`, `Cargo.lock` et `rust-toolchain.toml`.
- Le store virtuel global de pnpm est désactivé afin d'éviter toute dépendance applicative hors du projet.
- Les scripts ne dépendent plus de la présence de pnpm dans le `PATH` de l'utilisateur.
- Les commandes de préparation frontend appelées par Tauri passent elles aussi par l'amorce Corepack locale.
- SQLite est compilé directement dans MixCanvas avec les fonctionnalités rusqlite par défaut désactivées.
- La base utilisateur est enregistrée dans le dossier de données de l'application sous `library.sqlite3`.
- L'importation est exécutée hors du thread principal de l'interface et les insertions SQLite sont regroupées dans une transaction.
- Le schéma initial de bibliothèque est versionné avec `PRAGMA user_version = 1`.
- Migration automatique du schéma SQLite 1 vers le schéma 2 sans perte des morceaux existants.
- Ajout de la table `track_beats` avec suppression en cascade lorsque sa piste est retirée.
- L'analyse audio s'exécute hors du thread principal et ne garde en mémoire que l'enveloppe temporelle compacte.
- Aucun nouveau moteur DSP ni service externe n'est nécessaire pour l'analyse BPM 0.0.3.
- Migration automatique du schéma SQLite 2 vers le schéma 3 avec ajout de `manual_bpm` et `manual_first_beat_ms`.
- Les corrections demeurent séparées des résultats automatiques et ne dupliquent pas les positions de beats en base.
- La grille corrigée est dérivée du BPM et du premier beat manuels pour les morceaux à tempo source constant.
- Les corrections acceptent de 40 à 300 BPM et bornent le premier beat à la durée du morceau.
- Migration automatique du schéma SQLite 3 vers le schéma 4 avec ajout de `project_settings` et `timeline_clips`.
- Le premier beat source est l'ancre persistante d'un clip; le début physique, le pré-roll et la durée musicale sont dérivés du BPM effectif.
- Rust vérifie et arrondit chaque position demandée sur un beat entier, même si elle provient déjà d'un geste calé dans l'interface.
- Le pré-roll d'un morceau est conservé et l'ancre minimale empêche son audio physique de précéder le début du projet.
- Une correction de beatgrid recalcule la géométrie des clips sans réécrire leurs positions persistantes.
- La suppression d'une piste de bibliothèque supprime ses clips en cascade sans supprimer le fichier audio.
- Le projet courant du prototype reste temporairement dans `library.sqlite3`; ce stockage ne constitue pas encore le futur format de projet exportable.
- Le jalon 0.0.5 expose une seule piste et n'ajoute pas encore de lecture de timeline ni de time-stretch.
- La fenêtre principale ne défile plus; seuls la bibliothèque verticalement et l'axe de timeline horizontalement possèdent leur propre zone de défilement.
- Le bouton Play/Pause principal reste désactivé jusqu'au branchement du véritable moteur de timeline afin de ne pas simuler une synchronisation audio avec l'horloge de l'interface.

### Exigences précisées

- L'analyse doit produire plus qu'un BPM : elle doit également déterminer une beatgrid et le premier temps.
- La version 0.1 doit permettre de corriger manuellement le BPM, de le doubler ou le diviser par deux, et de repositionner le premier beat.
- Le Preview lit le morceau à son tempo et à sa tonalité d'origine par un chemin d'écoute distinct.
- Les trois pistes doivent demeurer synchronisées pendant une progression de tempo.
- L'évolution prévue comprend un compresseur, un limiteur et un compresseur avec entrée sidechain.
- Une future chaîne d'effets pourra exister sur chaque piste et sur le bus master.
- Le routage sidechain transmettra un signal de contrôle au détecteur d'un effet sans détourner automatiquement le signal audible de sa piste source.
- Le logiciel maison est destiné à être rendu disponible gratuitement dans un dépôt GitHub public.

### Documentation

- Description des composants : bibliothèque, analyse, courbe de tempo, Tap Tempo, timeline, Preview, moteur audio, persistance et cache.
- Ajout de critères techniques de réussite et d'une liste explicite des fonctions hors portée de la version 0.1.
- Ajout de la liste des décisions technologiques et ergonomiques restant à prendre.
- Ajout de l'architecture prévue pour les effets internes, les chaînes de traitement et le routage sidechain.
- Ajout des contraintes de licence liées à Rubber Band et Essentia.
- Inscription de l'AGPLv3 comme licence définitive du projet.
- Documentation de la stack adoptée, de l'isolation des dépendances et du flux de Preview 0.0.1.
- Documentation du stockage SQLite, du schéma versionné et du flux de bibliothèque 0.0.2.
- Documentation de l'algorithme, de la persistance de beatgrid et des limites du jalon 0.0.3.
- Documentation du flux de correction, du schéma 3 et du comportement réutilisable de Tap Tempo.
- Documentation du modèle d'ancrage musical, des formules de géométrie, du schéma 4 et des limites d'édition du jalon 0.0.5.

### État de l'implémentation

- Le jalon 0.0.1 est implémenté et l'application desktop démarre correctement sous Windows.
- Le jalon 0.0.2 est implémenté et sa base persistante est créée correctement au démarrage.
- Le jalon 0.0.3 démarre correctement et a migré la bibliothèque réelle du schéma 1 au schéma 2 en conservant ses neuf entrées.
- Les neuf MP3 réels ont terminé leur analyse sans erreur et 5 543 positions de beats ont été persistées; la précision musicale doit encore être comparée aux BPM connus par l'utilisateur.
- Le jalon 0.0.4 démarre correctement et a migré la bibliothèque réelle vers le schéma 3 en conservant les neuf analyses.
- Le jalon 0.0.5 démarre correctement et a migré la bibliothèque réelle vers le schéma 4 en conservant les neuf morceaux, les neuf analyses et les 5 543 positions de beats.
- La table de timeline réelle est initialisée vide et le tempo du projet est initialisé à 120 BPM, sans modification des MP3 ni des analyses existantes.
- La fenêtre native `MixCanvas` a été lancée depuis le dépôt et confirmée comme réactive.
- Le frontend compile pour la production.
- Les dix tests TypeScript réussissent, dont la stabilisation et la réinitialisation de Tap Tempo.
- Les quinze tests unitaires Rust réussissent, notamment les grilles synthétiques à 120 et 128 BPM, les migrations SQLite, la persistance des analyses et corrections, le snapping, la géométrie musicale et la restauration d'une timeline après réouverture.
- Le formatage Rust est conforme.
- Clippy réussit avec tous les avertissements traités comme des erreurs.
- Le script de validation interrompt maintenant correctement son exécution lorsqu'une commande Cargo échoue.
- La Preview, le transport et le déplacement dans un véritable MP3 ont été validés par l'utilisateur.
- L'ajout et la persistance de fichiers réels dans la bibliothèque 0.0.2 ont été validés par l'utilisateur.
- Le prochain essai utilisateur doit valider le glisser-déposer, le déplacement calé et la lisibilité des longs clips réels avant d'engager la lecture time-stretchée.
- Le nouveau cadrage a été vérifié à 1280 × 720 et à la taille minimale 900 × 620 : aucune hauteur de document supplémentaire n'est produite et la molette modifie effectivement le zoom.
