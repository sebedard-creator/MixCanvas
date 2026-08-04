# Architecture de MixCanvas

## Rôle de ce document

Ce document vivant décrit la mécanique sous le capot de MixCanvas : les responsabilités des différentes parties du logiciel, la circulation des données et les règles techniques qui garantissent la synchronisation audio.

Il ne sert pas à décrire l'apparence de l'interface ni à imposer prématurément un langage ou un framework. Toute décision qui modifie le fonctionnement interne doit être reflétée ici. Chaque modification matérielle du projet doit également être inscrite dans le journal de développement le jour où elle est faite. Jusqu'à la 1.0 ce journal est `prereleasechangelog.md`, qui reste sur la machine de l'auteur : il consigne des décisions au jour le jour, utiles pour reprendre le contexte et sans intérêt pour qui clone le programme.

Dernière mise à jour : 2026-08-03

## Vision du produit

MixCanvas est un éditeur de mix DJ volontairement simple. Il doit permettre de construire un mix sur une timeline musicale sans accumuler les fonctions d'une station audio complète.

Le projet est destiné à devenir un logiciel maison distribué gratuitement avec son code source dans un dépôt GitHub public. Le dépôt devra porter une licence open source explicite; le simple fait de rendre le code visible publiquement ne définit pas les droits de réutilisation et de distribution.

La version 0.1 vise les fonctions suivantes :

- bibliothèque locale de fichiers MP3;
- analyse automatique du BPM, des beats et du premier temps;
- correction manuelle minimale de l'analyse;
- bouton Preview pour chaque morceau;
- timeline de trois pistes stéréo;
- déplacement de clips avec snapping musical;
- courbe globale de tempo avec segments constants ou progressifs;
- outil manuel Tap 1 sur plusieurs mesures;
- time-stretch conservant la tonalité;
- transport Play/Pause;
- traitement audio interne en virgule flottante 32 bits;
- sauvegarde et réouverture d'un projet.

## Principes fondamentaux

### Édition non destructive

MixCanvas ne modifie jamais les MP3 originaux. Un projet conserve des références vers les fichiers ainsi que des instructions de lecture : piste, position, point d'entrée, durée et paramètres de synchronisation.

### Temps musical et temps audio

L'interface manipule principalement des mesures et des beats. Le moteur audio convertit ensuite ces positions musicales en temps réel et en positions d'échantillons.

Cette séparation est essentielle : une position exprimée seulement en secondes ne resterait pas musicalement stable après une modification de la courbe de tempo.

### Tempo global du projet

Le projet possède une seule courbe de tempo globale. Toutes les pistes audibles suivent cette courbe simultanément. Les clips ne possèdent pas de courbes de tempo indépendantes, car elles pourraient les désynchroniser lorsqu'ils jouent ensemble.

### Time-stretch sans changement de tonalité

Chaque clip est adapté au tempo du projet par time-stretch. Sa hauteur musicale doit demeurer stable. Le comportement de type varispeed, où le tempo et la tonalité changent ensemble, ne fait pas partie du fonctionnement normal de MixCanvas.

À un instant donné, le facteur de lecture dépend du BPM cible du projet et du BPM source du morceau. Ce facteur peut évoluer continuellement pendant une rampe de tempo.

### Traitement interne float32

Le décodage, le time-stretch, le mixage et les gains utilisent des échantillons en virgule flottante 32 bits. Cette représentation procure du headroom pendant les calculs internes. Le bus master doit malgré tout posséder un contrôle de gain, un indicateur de niveau et une protection explicite contre l'écrêtage à la sortie.

### Effets internes modulaires

L'évolution prévue du produit comprend des effets maison simples, notamment un compresseur, un limiteur et un compresseur avec entrée sidechain. Ils seront conçus comme des modules DSP internes plutôt que comme des plugins VST chargés dynamiquement.

Le premier de ces modules est en place : un limiteur master occupe la fin du bus, à l'endroit où la sortie était auparavant simplement écrêtée. Sa réduction de gain est stéréo-liée, dérivée du pic de la trame plutôt que de chaque canal séparément, afin qu'une limitation ne puisse jamais déplacer l'image stéréo. La borne physique subsiste après le limiteur, en dernier recours pour le bref dépassement qu'une attaque finie ne rattrape pas. Son activation suit le même mécanisme que Mute et Solo — un atomique partagé avec la source déjà en file — donc elle est audible immédiatement sans reconstruire le plan.

Les deux points de mesure du bus master sont distincts et le restent :

- les aiguilles du VU observent le master **après** le compresseur et sa teinte, mais **avant** le limiteur. Le compresseur et la teinte façonnent le mix et appartiennent à son son, donc le VU en montre le résultat; le limiteur est une protection, donc l'aiguille reste en amont de lui;
- le témoin `OL` est mesuré **après** le limiteur et avant la borne physique. Il ne s'allume donc que lorsque la borne a effectivement dû rogner le signal, autrement dit sur un écrêtage réellement subi. Limiteur activé, il reste éteint sauf si un transitoire devance l'attaque; limiteur contourné, il signale chaque crête écrêtée comme auparavant.

Un témoin qui s'allumerait sur un dépassement déjà absorbé n'indiquerait pas un défaut du son produit, seulement un niveau élevé — c'est le rôle du VU.

Le second module est un compresseur de collage placé avant le limiteur, avec un caractère fixe plutôt que des réglages exposés : un seul bouton doit donner un résultat utilisable. Seuil à −12 dBFS, rapport 2:1, genou doux de 6 dB, attaque de 10 ms, retombée de 120 ms et makeup de +2 dB. Les pistes reposant à −4 dB, une passe dense somme quelques décibels sous la pleine échelle : le compresseur travaille donc sur la moitié forte du matériel et laisse les passages calmes tranquilles. L'attaque n'est délibérément pas plus rapide, afin que le transitoire du kick passe intact.

Son détecteur écoute le mix à travers un passe-haut à 120 Hz. C'est le choix qui compte le plus pour ce répertoire : alimenté par le signal complet, le kick s'approprie toute la réduction de gain et fait plonger le morceau à chaque temps. En écoutant au-delà du grave, le compresseur répond à l'ensemble du mix et le kick conserve son poids; la compression devient dépendante de la fréquence plutôt que pilotée par les basses.

Le même bouton engage une teinte de console en deux temps. D'abord un relief : plateau grave de +2 dB sous 90 Hz, plateau aigu de +2 dB au-dessus de 10 kHz. Ensuite une saturation, car des plateaux ne font que déplacer l'équilibre d'un signal — ce qui le colore vraiment, ce sont des harmoniques.

La saturation est un écrêteur cubique doux, la courbe la plus économique ayant une forme musicale : rigoureusement linéaire à l'origine, elle se cintre progressivement et s'aplatit à ±2/3. Étant cubique, elle ne produit d'une sinusoïde que le fondamental et une harmonique trois — le contenu harmonique est borné par construction, et c'est ce qui rend le suréchantillonnage inutile ici.

Elle ne s'applique qu'au corps du mixage, sous 5 kHz. Deux raisons, qui pointent dans le même sens. L'harmonique trois de tout ce qui vit dans cette bande retombe sous 15 kHz et reste donc à l'intérieur du spectre : rien ne se replie. Et c'est de toute façon là que la chaleur a sa place; la même courbe appliquée aux cymbales n'ajouterait que de la friture. Le haut du spectre est séparé par un passe-bas puis rendu intact.

L'ordre compte : les plateaux passent en premier, pour que le relief sous le kick attaque le saturateur plus fort que le reste — c'est de là que vient le poids d'une grande console. Saturer d'abord et égaliser ensuite ne ferait qu'égaliser une distorsion déjà commise.

La profondeur est réglée par un mélange à 30 % et non par un gain d'entrée : la plage utile du cubique est exactement ±1, qui est aussi la plage d'un échantillon, donc pousser davantage ne ferait qu'enfouir les crêtes dans la partie plate de la courbe, là où elle cesse d'être musicale pour devenir un écrêteur. Mesuré sur un 200 Hz à 0,8 : le fondamental perd 0,36 dB et l'harmonique trois arrive à −35 dB. De la matière, pas de la distorsion.

L'ensemble tourne en permanence afin que l'état des filtres reste chaud; un fondu de 8 ms commute la teinte sans clic.

Le rapport 2:1 est calculé sans logarithme sur le thread temps réel : à ce rapport, la sortie compressée vaut `sqrt(seuil × pic)`, donc le gain se réduit à une seule racine carrée, à laquelle le genou est mêlé par un `smoothstep`.

Le troisième module est la compression à entrée sidechain. Elle n'a pas d'interrupteur global : un clip porte la clé ou ne la porte pas, et cette désignation est déjà la commande — un second contrôle par-dessus n'aurait fait que dupliquer le premier. Un clip de la timeline peut être désigné comme clé; il se tait alors partout où il en recouvre d'autres et y impose son pompage, tout en jouant normalement là où il est seul. Cette dernière propriété est ce qui permet d'employer un morceau entier comme clé plutôt qu'une boucle de kick muette.

Le détecteur écoute l'audio réel de la clé à travers un passe-bas à 150 Hz, et non la grille de temps : le pompage suit donc le rythme effectivement joué, y compris syncopé, et reste juste même si le BPM est mal estimé. Écouter au-delà du grave serait ici la même erreur que pour la détection du downbeat — les charleys d'un morceau complet déclencheraient le duck.

Mais un passe-bas ne suffit pas, et l'erreur mérite d'être consignée : une ligne de basse occupe exactement la bande du kick et s'y maintient. Un détecteur de **niveau** la lit donc comme un seul coup interminable et réduit le gain en continu — ce qui s'entend comme un compresseur qui se comporte mal, pas comme un pompage par kick. Ce qui distingue les deux est le **transitoire**, non le niveau.

Le déclenchement compare donc deux enveloppes d'énergie de la bande grave : une rapide de 15 ms, qui suit l'attaque d'un coup, et une lente de 300 ms, qui suit le niveau que le grave tient depuis un moment. Un kick fait décoller la première au-dessus de la seconde; une note de basse tenue les fait monter ensemble et ne déclenche rien. Les deux mesurent l'énergie et non l'amplitude, ce qui importe : comparer une enveloppe de crête à une enveloppe de moyenne donne, pour une sinusoïde stable, un rapport voisin de 1,57 quel que soit le niveau — assez près de tout seuil raisonnable pour se déclencher. Deux énergies lissées convergent en revanche vers la même valeur sur un signal stable, si bien que leur rapport se pose à 1 et que seule une hausse réelle peut le soulever.

Le seuil est de deux fois et demie l'énergie, environ 4 dB. Un kick plus faible que la basse qui l'accompagne ne le franchira pas; un tel kick fait de toute façon une mauvaise source de déclenchement, et abaisser encore la barre laisserait passer de simples notes de basse.

Le tempo intervient deux fois : il minute la retombée, et il fixe une fenêtre réfractaire d'un demi-temps pendant laquelle rien ne peut redéclencher. Un kick ne peut pas être suivi d'un autre plus tôt, et cette fenêtre empêche la queue d'un kick, ou une note de basse tombant juste après, de déclencher une seconde fois. Le détecteur ne se cale volontairement pas sur les *positions* de la grille : l'effet dépendrait alors de l'exactitude de la beatgrid, et une grille fausse ferait taire de vrais kicks.

La retombée est la forme même de l'effet. Elle est linéaire en décibels, obtenue en multipliant le gain par un facteur constant à chaque trame : c'est le gonflement droit que l'oreille lit comme du pompage, là où une retombée à un pôle remonterait l'essentiel du chemin d'un coup puis traînerait. Sa durée vaut neuf dixièmes d'un temps du projet, de sorte que le gain revienne à l'unité juste avant le kick suivant; ce quasi-rendez-vous est ce que l'on entend respirer. La profondeur est fixe, comme le caractère des autres modules.

Une seule clé peut exister à la fois : en désigner une libère la précédente dans la même transaction, sans quoi le projet serait brièvement creusé deux fois. Contrairement aux interrupteurs master, changer de clé modifie ce qui est audible et reconstruit donc le plan de lecture au lieu de basculer un atomique.

L'ordre de la chaîne master est **duck, puis compresseur, puis limiteur** — l'ordre d'une console, le sidechain appartenant au mixage et les processeurs master à ce qui en sort. La conséquence mérite d'être connue en mixant : un compresseur relève ce que le duck vient de creuser, donc activer `COMP` adoucit le pompage.

La version 0.1 n'a pas à exposer ces effets, mais le moteur ne doit pas empêcher leur ajout. Le chemin audio doit donc pouvoir accueillir ultérieurement une chaîne d'effets par piste et une chaîne d'effets sur le bus master.

Un module DSP interne devra :

- recevoir et produire des blocs audio stéréo float32;
- exposer des paramètres identifiés et sauvegardables;
- offrir un état Bypass;
- déclarer sa latence lorsque celle-ci n'est pas nulle;
- traiter l'audio sans allocation, verrou, accès disque ni attente dans le chemin temps réel;
- pouvoir recevoir une entrée d'analyse distincte pour un éventuel sidechain.

MixCanvas n'hébergera pas de plugins externes. Aucun support VST3, Audio Unit ou autre format de plugin tiers n'est prévu. Le mot « plugin » désigne uniquement, lorsque nécessaire dans les discussions, un effet maison compilé directement avec le logiciel. Le terme privilégié dans l'architecture est « module DSP interne ».

### Licence et dépendances

Les dépendances doivent être sélectionnées en fonction d'une distribution gratuite et open source. Leur licence, leurs avis de droits d'auteur et les obligations liées aux binaires distribués doivent être consignés avant leur intégration.

MixCanvas est distribué sous la GNU Affero General Public License version 3 uniquement, identifiée par l'expression SPDX `AGPL-3.0-only`. Le texte intégral officiel est conservé dans le fichier `LICENSE`.

Ce choix est compatible avec l'utilisation envisagée de Rubber Band sous GPL version 2 ou ultérieure et d'Essentia sous AGPLv3. Le jalon 0.0.3 utilise toutefois un analyseur BPM maison écrit en Rust et n'intègre pas Essentia. L'architecture doit malgré tout garder le moteur d'analyse interchangeable et isoler le moteur de time-stretch derrière une interface interne afin de limiter le couplage technique aux dépendances.

### Stack logicielle adoptée

La première plateforme de développement est Windows. Les choix doivent demeurer portables afin de permettre une prise en charge ultérieure de macOS, mais la version 0.1 sera validée d'abord sur Windows.

La stack initiale est la suivante :

- Tauri 2 pour l'application desktop et l'intégration avec la WebView système;
- React et TypeScript pour l'interface;
- Rust pour le moteur audio, l'accès aux fichiers et l'état applicatif faisant autorité;
- Rodio comme couche de Preview initiale au-dessus de CPAL et Symphonia;
- CPAL pour la sortie audio native;
- Symphonia pour le décodage MP3 vers des échantillons float32;
- SQLite embarqué par rusqlite pour la bibliothèque persistante, sans serveur ni DLL SQLite système.

L'interface TypeScript n'est jamais l'autorité temporelle audio. Elle transmet seulement des commandes de haut niveau au moteur Rust et reçoit des états destinés à l'affichage. Aucun bloc PCM ne doit traverser l'IPC de Tauri.

Rodio permet de valider rapidement la Preview et le périphérique audio. Son utilisation dans le jalon 0.0.1 ne signifie pas que le futur moteur de timeline reposera sur son modèle de lecture. La timeline, le time-stretch variable et le mixage à trois pistes devront rester derrière nos propres abstractions audio.

### Isolation et reproductibilité des dépendances

Les outils de compilation — Node.js, pnpm, Rustup, Microsoft C++ Build Tools et WebView2 — sont des prérequis système de développement. Ils ne seront pas requis sur la machine d'un utilisateur qui installe une version compilée.

Les dépendances du projet et leurs artefacts restent sous la racine du dépôt :

- `.corepack` pour la version de pnpm verrouillée par le projet;
- `.pnpm-store` et `node_modules` pour JavaScript;
- `.cargo-home` pour le registre et le cache des crates Rust utilisés par les scripts du projet;
- `src-tauri/target` pour les artefacts Rust;
- `dist` pour le frontend compilé.

Ces dossiers sont exclus de Git. `pnpm-lock.yaml`, `Cargo.lock` et `rust-toolchain.toml` fixent les versions nécessaires pour reconstruire le projet sur une autre machine. Le store virtuel global de pnpm est explicitement désactivé. Les scripts racine utilisent le Corepack système avec `COREPACK_HOME` redirigé dans le dépôt : une installation globale de pnpm et une modification du `PATH` ne sont donc pas nécessaires. `scripts/dev.ps1` fixe aussi `CARGO_BUILD_JOBS=1` afin de limiter la mémoire utilisée par une reconstruction Tauri sur une machine de développement déjà chargée. Les lanceurs `.cmd` démarrent les scripts PowerShell avec une exception d'exécution limitée au processus courant, sans modifier la politique de sécurité globale de Windows.

Le transport musical est la source de vérité du playhead. Les actions Play, Pause et Seek agissent d'abord sur sa position en beats; le moteur audio reçoit ensuite le seek correspondant. Les snapshots de l'interface ne réécrivent jamais le transport depuis l'horloge interne du lecteur, qui peut être momentanément imprécise autour d'un seek ou d'une pause. Le playhead reste physiquement au centre de la fenêtre tant que le contenu déborde de celle-ci; la timeline possède alors une marge virtuelle de demi-fenêtre et défile autour de lui. Dès que le projet entier tient dans la fenêtre — au zoom extérieur maximal — ce décalage cesserait d'avoir un sens et repousserait la moitié du mix hors de l'écran : le contenu est alors centré, encadré de deux marges égales, ce qui est précisément l'objet d'un zoom arrière complet.

Un clic de seek est converti depuis le rectangle de la règle ou de la voie réellement cliquée. Ce rectangle est déjà déplacé par le `scrollLeft` courant et déjà après la marge virtuelle : `x local / pixelsParBeat` est donc directement le beat demandé. La marge ne doit jamais être soustraite une seconde fois, sans quoi les clics d'une timeline centrée sont artificiellement ramenés vers le début du projet.

La borne de zoom extérieure s'arrête volontairement un cran avant l'ajustement parfait : le projet occupe une fraction de la largeur, si bien qu'atteindre la limite se lit comme une limite plutôt que comme une commande bloquée, et que les deux extrémités du mix sont visibles d'un coup d'œil. Cela garantit un playhead continu, stable et aligné avec l'axe musical de la timeline.

Le zoom de la timeline est ancré au playhead. Les événements de molette sont regroupés en une seule variation bornée par image affichée, ce qui évite les valeurs d'échelle transitoires lors d'une rafale de micro-événements. La largeur musicale et la translation qui place le beat courant au centre sont dérivées du même état et publiées dans le même commit DOM.

Le suivi de lecture passe par le **défilement natif écrit sans lecture**. La surface musicale reçoit une demi-largeur de viewport vide avant et après le projet; écrire `scrollLeft = beat × pixelsPerBeat` garde donc ce beat sous la ligne centrale. Le playhead n'appartient plus à cette surface : c'est une superposition fixe au-dessus du viewport. La trace du 2 août a montré que translater le conteneur complet en rendu logiciel repeignait `timeline-content` sur une zone non bornée, jusqu'à 15,7 ms pour une seule opération. Le défilement conserve le centrage sans déplacer ce sous-arbre de clips, courbes et waveforms à chaque tick.

Le viewport `.timeline-scroll` est une frontière explicite de layout et de peinture (`contain: layout paint`). Il a déjà une hauteur définie et un débordement masqué; cette isolation ne change donc ni sa géométrie ni ce qui est visible, mais interdit à une invalidation de scroll de remonter inutilement jusqu'au document complet en rendu logiciel. Une tentative supplémentaire avec `will-change: scroll-position` n'a pas réduit la rasterisation dans les traces comparables et n'est pas conservée : Chrome possédait déjà sa couche de scroll dédiée.

La trace de validation sur 23,896 s confirme l'effet attendu : `Layerize` passe de 194,8 à **11,6 ms/s**, `Paint` de 178,0 à **93,7 ms/s** et `Layout` de 23,4 à **15,5 ms/s**. Les trois rangées musicales sont encore peintes vingt fois par seconde — le mode logiciel doit copier les pixels exposés par le défilement — mais une passe coûte au plus 3,19 ms et ne porte plus sur une surface infinie. C'est un coût borné, compatible avec le déplacement continu du timeline.

Le zoom ne possède toujours ni aperçu intermédiaire par `scaleX`, ni conteneur forcé en calque par `will-change` : grille, clips, courbes, waveforms et playhead n'existent qu'à une seule échelle validée par image. Le viewport conserve un `overflow-x: hidden`; son `scrollLeft` est l'unique déplacement vivant de la lecture et il n'est jamais lu dans le cycle de rendu.

Les clips audio sont virtualisés côté interface : seule la fenêtre visible, augmentée d'une marge supérieure au seuil de rafraîchissement de vue, possède des titres, boutons et conteneurs de waveform dans le DOM. La timeline audio et son plan de lecture gardent tous les clips : cette sélection ne touche qu'au rendu React. Les numéros de clips et la recherche des morceaux de bibliothèque sont pré-indexés, afin qu'une session longue ne transforme pas chaque rendu en recherche quadratique.

L'afficheur BPM suit le playhead hors du cycle React. Une rampe de tempo peut changer ses deux décimales à chaque poll audio; le texte du readout est donc écrit directement dans son `span`, comme le playhead et les VU-mètres. Un changement de BPM vivant ne doit jamais reconstruire la timeline, ses clips ni ses waveforms.

Les gestes Draw Pan et Volume sont des groupes persistants. Les points précis restent dans les tables d'automation pour le DSP, mais portent un `draw_group_id`; `timeline_draw_groups` conserve la voie, l'étendue, le type (`step`, `sine`, `triangle`) et la période du geste. La timeline les dessine comme **un seul chemin SVG par Draw**, sans poignée intermédiaire, avec un échantillonnage visuel borné par la largeur réelle du viewport. Les points audio originaux ne sont ni supprimés ni modifiés par cette simplification. Les nodes manuels n'ont pas de groupe et restent éditables. Un clic droit dans la plage d'une courbe Draw propose `Delete Draw` — ou les deux suppressions nommées quand Pan et Volume se recouvrent — et efface atomiquement le groupe concerné sans toucher aux nodes manuels. Les groupes suivent un clip déplacé et font partie des instantanés Undo/Redo ainsi que des fichiers de projet.

Le mode de rendu se choisit **au lancement**, et non plus à la compilation. La variante `disable-gpu` a disparu : `RenderMode` lit `--no-gpu`, `--gpu-safe` ou `--gpu` et ajuste `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` avant la création du premier WebView, en préservant les arguments déjà présents. Le dessin logiciel reste le défaut, parce que certains pilotes font déchirer le compositeur matériel de WebView2 pendant un zoom et qu'aucun soin de notre côté n'y change rien — la justesse l'emporte, à un coût que cette interface 2D peut payer. Une seule feature de compilation subsiste, `embed-resources`, qui produit le portable d'un seul fichier.

Un portable ne se construit jamais avec `cargo build` seul : Rust y produit correctement l'exécutable, mais Tauri ne reçoit pas son contexte `build` et l'application peut conserver `devUrl` (`127.0.0.1:1420`) au lieu de charger `frontendDist`. La commande de distribution est `node_modules\\.bin\\tauri.cmd build --no-bundle -f embed-resources`; elle exécute d'abord le build Vite puis embarque `dist` dans l'exécutable.

**Le GPU a ensuite été mis hors de cause par la mesure.** Le relevé du 2026-07-31, inspecteur ouvert sur la machine de développement, donne 535 contre 541 ms de rendu par seconde entre `--gpu-safe` et `--gpu` : moins de deux pour cent d'écart. Le moteur n'était pas le problème, et remettre l'accélération n'aurait rien gagné. Le flash observé le 2026-07-29 était réel, mais il ne représentait pas le coût, seulement un artefact visible; la charge venait d'ailleurs, et la section du 2026-08-02 dit d'où.

## Le tempo change sur les temps — 2026-08-03

La rampe entre deux cibles de BPM était **continue** : `bpm_at_beat` renvoyait
une interpolation linéaire évaluée à n'importe quel endroit, donc le tempo
différait d'un échantillon au suivant. C'était juste au sens mathématique, et
faux au sens musical.

**Ce que ça produisait.** Le ratio d'étirement de chaque clip bougeait en
permanence. Sur un seul morceau, on entendait la vitesse glisser — un
accelerando que la musique enregistrée ne fait jamais. Sur deux clips
superposés, chacun était étiré par un ratio différent et mouvant : leurs
transitoires dérivaient l'une contre l'autre au lieu de rester verrouillées,
ce qui est exactement le contraire de ce qu'on cherche en mixant.

**Le principe qui corrige.** Un changement de tempo au milieu d'un temps est à
nu. Un changement à la **frontière** d'un temps est masqué par la transitoire
qui s'y trouve. Le tempo est donc désormais constant à l'intérieur d'un temps et
ne change qu'à son bord : la rampe est échantillonnée au début de chaque temps
entier et tient jusqu'au suivant.

Une ancre posée sur un temps entier est honorée exactement. Une ancre posée
entre deux temps voit son changement prendre effet à la frontière suivante — ce
qui est le comportement voulu, et non une approximation subie.

**Ce que ça coûte en calcul, et pourquoi une table.** Un temps dure exactement
`60 / bpm`, mais ce BPM change d'un temps à l'autre le long d'une rampe : la
somme des durées n'a pas de forme close, contrairement à l'intégrale
logarithmique qu'elle remplace. Elle est donc accumulée **une fois par édition**
dans `beat_seconds`, et relue par dichotomie.

Ce détail n'est pas un raffinement. `beat_at_seconds` est appelée depuis
`source_position_at_timeline_frame`, sur le chemin audio, une fois par grain
WSOLA — quelques centaines de fois par seconde et par clip. Une boucle sur les
trente mille temps d'un long mix y aurait coûté ce que la recherche WSOLA a déjà
coûté une fois cette semaine : une régression audible, trouvée après coup.

**Les trois fonctions doivent changer ensemble.** `bpm_at_beat`,
`seconds_at_beat` et `beat_at_seconds` décrivent un seul modèle. Quantifier la
première en laissant les autres intégrer une rampe continue ferait diverger la
cadence réellement jouée de la position calculée — une panne silencieuse, de la
même famille que les deux cartes de tempo divergentes du handoff.

## Ce que coûte une image — mesures du 2026-07-31 au 2026-08-02

Trois jours de relevés sur la machine de développement, inspecteur ouvert,
parce qu'une interface jugée « inacceptable » avant une v1.0 ne se corrige pas
au raisonnement. La leçon de méthode précède les correctifs : **les deux
hypothèses formulées sans mesure étaient fausses toutes les deux**, et les cinq
défauts trouvés l'ont été par le profileur, jamais par déduction.

### Une propriété personnalisée invalide tout son sous-arbre

C'est le mécanisme central, et il s'est présenté **trois fois** sous des formes
différentes. Changer une propriété personnalisée sur un élément oblige le
navigateur à recalculer le style de chacun de ses descendants, puisque
n'importe lequel pourrait la lire. Une propriété ordinaire — `transform`,
`background-size`, `left` — n'invalide que l'élément qui la porte.

| propriété | posée sur | changeait | coût mesuré |
|---|---|---|---|
| `--timeline-follow-offset` | l'ancêtre de toute la timeline | 20 fois/s | **36,4 %** du fil |
| `--measure-width` | chaque voie, ancêtre de ses clips | à chaque zoom | non isolé |
| `--anchor-offset` | chaque clip, ancêtre de sa waveform | à chaque zoom | non isolé |

Aucune n'est remplacée par une astuce : la première devient une transformation,
la deuxième un `background-size` sur une tuile large d'une mesure, la troisième
un `left` écrit sur l'ancre elle-même — le seul descendant qui la lisait.
`--beat-width`, écrite sur deux éléments par voie à chaque zoom, n'était lue par
aucune règle.

**Plus aucune propriété personnalisée n'est écrite depuis React**, et c'est une
invariante à tenir. Elle se vérifie d'une commande :

```
grep -rn '"--[a-z-]*":' src/
```

### Lire la mise en page force le navigateur à la recalculer

Un `useLayoutEffect` sans dépendances vérifiait à chaque rendu que le
`scrollLeft` natif de la timeline valait bien zéro. Lire `scrollLeft` oblige le
navigateur à recalculer style et mise en page sur-le-champ, avant de pouvoir
répondre — et comme le rendu venait d'écrire des styles, il n'avait rien en
réserve. **188 ms par seconde**, soit un cinquième du fil principal, pour
vérifier une valeur qui ne bouge presque jamais.

L'`overflow` étant déjà `hidden` des deux côtés, il ne reste qu'un défilement
provoqué par le navigateur lui-même quand il met un enfant au premier plan, et
l'évènement `scroll` dit exactement quand cela arrive. Un zoom remet le décalage
à zéro par une **écriture seule**, qui n'interroge pas la mise en page.

La même vérification exhaustive s'impose ici :

```
grep -rn "getBoundingClientRect\|offsetWidth\|clientWidth\|scrollLeft" src/
```

Les lectures situées dans un gestionnaire de pointeur sont saines — une par
geste, et il faut bien convertir un pointeur en beat. Ce sont celles qui vivent
sur le chemin de chaque rendu qui coûtent.

### Ne construire que ce qui se voit

Trois structures étaient bâties sur toute la largeur du mix, qui peut atteindre
soixante-dix mille pixels, pour un écran qui en montre neuf cents.

- **Les waveforms.** Mesuré sur un morceau de six minutes au zoom ordinaire :
  seize mille colonnes, un million six cent mille caractères de chemin,
  vingt-six millisecondes par clip et par cran de zoom. Un bloc de largeur fixe
  glisse par pas de 256 px — deux bords s'alignant chacun pour soi franchissaient
  la grille à des moments différents, et la tranche changeait deux fois par pas.
  Le niveau de la pyramide reste choisi sur la largeur **totale** du clip : le
  choisir sur la tranche donnait un escalier.
- **Les marqueurs de mesure**, coupés à la fenêtre élargie d'un pas.
- **Les courbes d'automation** — volume, panoramique, filtre. Le découpage garde
  **un nœud de chaque côté** de la fenêtre : couper au bord donnerait une pente
  calculée depuis le bord, et la ligne ne passerait plus où elle passe. Les
  extrémités restent ancrées à 0 et à la largeur totale, un segment plat ne
  coûtant rien.

### Le transport hors du cycle de rendu

La position de lecture est interrogée vingt fois par seconde. Tant qu'elle
vivait dans un `useState`, chacune de ces réponses re-rendait l'application
entière — pour déplacer deux éléments. Elle passe par `liveTransport`
(`src/lib/liveTransport.ts`), un émetteur auquel on s'abonne : la translation du
contenu, la tête de lecture et le curseur de la barre sont écrits directement
dans le DOM. Un rendu n'est redemandé qu'au franchissement de 192 px, le pas en
deçà de la marge que les waveforms se gardent.

Effet de bord qui vaut d'être noté : une action déclenchée au clavier lit
désormais la position **au moment du geste** et non celle du dernier rendu. Elle
était juste par accident, parce que l'état se rafraîchissait vingt fois par
seconde; elle l'est maintenant par construction.

### Un objet neuf empêche React de s'arrêter

`updateSmartCursor` écrivait deux objets neufs dans l'état à chaque mouvement du
pointeur. Le pointeur bouge plus de cent fois par seconde, l'outil ne change
qu'en franchissant une frontière — mais un objet neuf n'est jamais égal au
précédent, donc le panneau entier était re-rendu à la fréquence de la souris, et
les trois jeux de courbes reconstruits avec lui. **43 % du fil principal.** Rien
n'est écrit tant que rien ne change, et les courbes sont mémorisées.

### Ce que le profileur peut dire, et ce qu'il faut pour l'entendre

Le paquet de production est minifié : le premier poste s'affichait comme
`(anonymous)`, ce qui ne nomme rien. Un build de diagnostic non minifié le
nomme, au prix d'un frontend plus lourd :

```
npx vite build --minify false --sourcemap
npx tauri build --no-bundle -f embed-resources --config <config vidant beforeBuildCommand>
```

sans quoi `beforeBuildCommand` refait un frontend minifié par-dessus.

Deux pièges de lecture méritent d'être consignés. **Un nom peut désigner son
voisin** : le profileur a un jour attribué 43 % à `panNodeY`, trois opérations
arithmétiques, parce que V8 l'avait inlinée dans la fonction qui l'appelait.
**Et l'attribution à une position dans le paquet vaut mieux qu'un total** :
c'est elle, et rien d'autre, qui a trouvé la lecture de `scrollLeft`, invisible
autrement puisqu'elle ne ressemble pas à du travail.

Enfin, les totaux ne se comparent qu'entre scénarios semblables. Plusieurs
relevés de cette série ont été faits sur des sessions différentes et ne
permettent aucune conclusion — c'est la limite qu'il a fallu reconnaître avant
de pouvoir avancer.

## État de référence technique — audit du 2026-08-03

Le projet porte maintenant le nom **MixCanvas**. Il s'est auparavant appelé
EZ-DJ, puis BeatForge. Les trois manifestes applicatifs portent la version
**1.0.0** depuis le 2026-08-04. La numérotation des jalons ci-dessous est
restée celle du développement — elle raconte l'ordre dans lequel les choses ont
été construites, et n'a jamais suivi la version du paquet.

L'application demeure organisée en deux moitiés :

- React 19, TypeScript 7 et Vite 8 pour l'interface;
- Tauri 2 et Rust 2024 pour la persistance, l'analyse, le transport et tout le
  chemin audio;
- SQLite embarqué, en schéma **27**, pour la session courante;
- JSON versionné, au format `mixcanvas-project` version **1**, pour les fichiers
  `.mixcanvas`;
- cinquante-quatre commandes Tauri enregistrées comme frontière IPC.

Les responsabilités principales sont les suivantes :

- `src/App.tsx` orchestre l'état de l'application et les appels IPC;
- `src/components/TimelinePanel.tsx` porte les gestes et la géométrie
  interactive de la timeline;
- `src/lib/` contient les règles TypeScript pures et testables;
- `src-tauri/src/timeline.rs` est l'autorité sur la géométrie persistée des
  clips et des automations;
- `src-tauri/src/audio/timeline.rs` construit et joue le plan audio en temps
  réel;
- `src-tauri/src/library.rs` détient le schéma, ses migrations et le cache
  d'analyse;
- `src-tauri/src/project.rs` sérialise et restaure les fichiers de projet;
- `src-tauri/src/audio/stems.rs` réalise la séparation voix/instrumental.

La concentration de code est le principal risque structurel actuel :
`TimelinePanel.tsx`, `timeline.rs`, `audio/timeline.rs` et `app.css` portent
chacun plusieurs milliers de lignes. Une modification doit donc d'abord
chercher la règle existante et sa contrepartie de l'autre côté de l'IPC. Il ne
faut pas extraire ces fichiers mécaniquement pendant une correction audio ou
visuelle; les scissions devront suivre des frontières de responsabilités
testables.

État vérifié le 2026-07-28, application fermée :

- `tsc --noEmit` passe;
- Vitest passe : 23 fichiers, 179 tests;
- `cargo test` passe : 139 tests, 4 tests ONNX ignorés explicitement;
- `cargo fmt --check` et Clippy avec `-D warnings` passent;
- la production frontend Vite se construit;
- aucun commit Git n'existe encore, donc cet audit décrit une photographie et
  non une évolution vérifiable par historique.

## État d'implémentation

### Jalon 0.0.1 — Preview audio native

Le premier flux vertical est implémenté :

1. l'utilisateur ouvre le sélecteur natif de fichiers;
2. seuls les fichiers MP3 sont proposés et acceptés;
3. Rust ouvre le fichier et Symphonia le décode en streaming;
4. le moteur initialise la sortie audio Windows à la demande;
5. Rodio transmet les échantillons float32 à CPAL;
6. l'interface pilote Play, Pause, le retour au début et le déplacement dans le fichier avec des commandes Tauri;
7. une barre de progression interactive transmet une position bornée à la durée du morceau;
8. l'interface affiche la durée, la position, la fréquence d'échantillonnage, le nombre de canaux et l'état de lecture.

La Preview demeure séparée du futur transport de la timeline. Le fichier n'est jamais modifié et l'audio décodé au complet n'est pas conservé en mémoire.

### Jalon 0.0.2 — Bibliothèque MP3 persistante

Le second flux vertical est implémenté :

1. l'utilisateur peut sélectionner plusieurs MP3 ou un dossier;
2. les sous-dossiers sont explorés récursivement sans suivre les liens symboliques;
3. chaque MP3 valide est décodé suffisamment pour obtenir sa durée, sa fréquence d'échantillonnage et son nombre de canaux;
4. les entrées sont enregistrées dans une base SQLite locale avec un chemin normalisé empêchant les doublons;
5. la liste est restaurée automatiquement au prochain démarrage;
6. l'existence de chaque fichier est revérifiée lors de la lecture de la bibliothèque;
7. chaque ligne pilote la Preview existante sans créer un second moteur audio;
8. retirer une entrée supprime uniquement la référence SQLite et ne touche jamais au MP3.

L'exploration des dossiers, le décodage des métadonnées et l'écriture en lot s'exécutent dans une tâche bloquante dédiée afin de ne pas immobiliser le thread principal de l'interface. Une seule importation peut modifier la bibliothèque à la fois.

La base est enregistrée sous le nom `library.sqlite3` dans `MixCanvas Files`, le dossier posé **à côté de l'exécutable** qui reçoit tout ce que le programme écrit. Un exécutable placé là où il n'a pas le droit d'écrire se replie sur le dossier de données applicatives de l'identifiant `ca.mixcanvas.app`, parce que refuser de démarrer serait pire. Les dépendances de compilation restent dans le dépôt; cette base constitue une donnée utilisateur produite à l'exécution.

### Jalon 0.0.3 — Analyse BPM et beatgrid

Le troisième flux vertical est implémenté :

1. chaque nouveau morceau est analysé automatiquement après son importation; l'utilisateur peut aussi lancer une réanalyse ou analyser toute l'ancienne bibliothèque;
2. le MP3 est décodé en streaming sans conserver tout le signal PCM;
3. le moteur calcule une enveloppe d'énergie à 100 Hz puis une enveloppe d'attaques avec seuil adaptatif;
4. une autocorrélation normalisée cherche une périodicité entre 70 et 190 BPM;
5. les harmoniques de la période contribuent au classement afin de stabiliser l'estimation;
6. dans l'algorithme initial, chaque beat prédit était rapproché d'une attaque locale; le jalon 0.0.10 remplace cette série déformable par une période globale uniforme;
7. le BPM final, la confiance, le premier beat détecté, toutes les positions et les crêtes stéréo sont enregistrés dans SQLite;
8. l'interface affiche les états Non analysé, Analyse, Analysé ou Erreur et permet une réanalyse.

Une seule série d'analyses peut s'exécuter à la fois. Le calcul est envoyé dans une tâche bloquante dédiée; la Preview et l'interface demeurent disponibles. Les modifications SQLite sont courtes et protégées séparément du décodage audio.

La version 0.0.3 vise les morceaux électroniques à tempo stable. Elle détecte une origine de pulsation, pas encore le premier temps d'une mesure de quatre temps. La distinction downbeat/mesure et les corrections manuelles sont donc explicitement reportées au prochain jalon.

### Jalon 0.0.4 — Correction non destructive de la beatgrid

Le quatrième flux vertical est implémenté :

1. l'utilisateur ouvre l'éditeur en cliquant sur le BPM d'un morceau analysé;
2. il peut saisir un BPM, le diviser ou le multiplier par deux;
3. Tap 1 enregistre le premier temps de mesures successives et ajuste une grille rigide à partir de quatre mesures au minimum;
4. la Preview du morceau sert de repère auditif pour placer le premier beat;
5. la position courante, idéalement après une Pause sur l'attaque, peut être capturée en millisecondes;
6. la correction est enregistrée séparément du BPM, du premier beat et des positions automatiques;
7. la grille effective est calculée à partir du BPM manuel et du premier beat manuel;
8. « Restaurer l'automatique » supprime uniquement la correction et réactive immédiatement l'analyse originale.

Le schéma 3 ajoute `manual_bpm` et `manual_first_beat_ms`. Il ne duplique pas les milliers de positions corrigées : puisque la version 0.1 suppose un tempo source constant, elles peuvent être dérivées de ces deux valeurs. La future timeline devra demander la grille effective à une abstraction interne plutôt que lire directement `track_beats`.

Le flux manuel sépare maintenant trois responsabilités. Tap 1 donne à la fois la période approximative et la phase musicale : chaque pression désigne le premier temps de la mesure suivante, donc les indices de beat `0, 4, 8, 12…` sont connus sans demander à l'utilisateur de compter ou saisir le nombre de mesures. « Snap to beat » relance ensuite l'ajustement audio dans une fenêtre étroite autour de ce BPM et produit une grille rigide précise; la position de premier temps issue des taps est calée sur le beat le plus proche de cette nouvelle grille. L'origine fournie par le modèle n'est utilisée que comme phase de pulsation : le beat choisi par l'utilisateur demeure le `1`, même si le modèle classe une autre phase de mesure comme downbeat. Enfin, « Save Correction » enregistre littéralement le BPM et le premier temps affichés, sans les recaler une seconde fois sur l'ancienne analyse automatique. Cette séparation est essentielle lorsque l'analyse à corriger est précisément celle qui s'est trompée.

Les corrections sont bornées entre 40 et 300 BPM et le premier beat ne peut pas dépasser la durée du morceau. Une réanalyse remplace le résultat automatique, mais conserve la correction manuelle jusqu'à sa restauration explicite.

### Jalon 0.0.5 — Première timeline musicale persistante

Le cinquième flux vertical est implémenté :

1. un morceau analysé peut être glissé de la bibliothèque vers une première piste stéréo, ou ajouté à la suite par un clic;
2. la position déposée est convertie en beat entier et vérifiée de nouveau par Rust avant son enregistrement;
3. le BPM du premier clip initialise le tempo global du projet;
4. chaque clip est dessiné selon sa durée musicale source, avec une ligne distincte indiquant son premier beat;
5. le clip peut être déplacé horizontalement et reste calé sur des beats entiers;
6. le pré-roll situé avant le premier beat est conservé et ne peut pas être déplacé avant le début du projet;
7. le BPM du projet peut être saisi ou obtenu avec Tap Tempo;
8. les clips, leur piste et leur beat d'ancrage sont restaurés automatiquement au redémarrage.

Le schéma 4 ajoute `project_settings` et `timeline_clips`. Le champ `anchor_beat` représente le beat du projet où tombe le premier beat de la source; il ne représente pas le début physique du MP3. La géométrie affichée est dérivée à la lecture avec les formules suivantes :

- `pre_roll_beats = first_beat_ms × bpm_source / 60 000`;
- `visual_start_beat = anchor_beat - pre_roll_beats`;
- `duration_beats = duration_ms × bpm_source / 60 000`.

Les valeurs effectives corrigées de BPM et de premier beat sont utilisées. Une correction de beatgrid met donc à jour la géométrie sans réécrire les clips. La suppression d'une référence de bibliothèque supprime en cascade ses clips, mais ne touche toujours pas au MP3.

Le jalon 0.0.5 a d'abord validé l'édition musicale et une horloge de transport silencieuse. Une piste est exposée maintenant afin de valider la mécanique avant sa généralisation aux trois pistes prévues pour la version 0.1.

L'interface desktop 0.0.5 est organisée comme un poste de travail occupant exactement la fenêtre :

- la timeline est la surface centrale et absorbe l'espace disponible;
- la bibliothèque demeure dans un panneau latéral droit de largeur bornée;
- la Preview est un transport compact sous la timeline;
- l'éditeur de beatgrid apparaît en superposition et ne modifie pas les dimensions du poste de travail;
- le document principal ne défile jamais; la bibliothèque possède son propre défilement vertical et la timeline son propre défilement horizontal.

Le zoom de timeline accepte la molette jusqu'à 96 pixels par beat. Sa borne minimale est calculée avec `largeur_visible / longueur_totale_en_beats`, sans plancher visuel arbitraire. Le niveau de zoom maximal vers l'extérieur peut ainsi montrer tous les clips du mix, même pour un projet de plusieurs heures. Sous un pixel par beat, la grille détaillée est masquée et seuls les repères de mesure suffisamment espacés demeurent lisibles. Chaque changement d'échelle utilise le beat courant du playhead comme point d'ancrage et le replace au centre de la largeur visible, dans les limites du début et de la fin du contenu. La molette et les boutons − et + partagent exactement ce comportement.

Le transport principal Play/Pause est fonctionnel depuis le 19 juillet 2026. Avant qu'un rendu audio soit chargé, son état fait autorité dans Rust et utilise une horloge monotone pour convertir le temps écoulé en beats selon le BPM du projet. Un clic sur la règle ou la piste repositionne le playhead; la barre d'espace bascule Play/Pause lorsque le focus n'est pas dans un champ ou un bouton. Le transport revient au début lorsqu'on relance après la fin et se met automatiquement en pause à la borne finale du dernier clip.

Depuis le jalon 0.0.6, la sortie audio remplace l'estimation monotone comme autorité dès qu'un rendu est chargé. Le frontend interroge seulement sa position d'affichage; il ne calcule jamais lui-même l'avancement. Le polling lit directement le BPM et la borne finale par une requête SQLite agrégée, puis recadre le transport sur la position réelle du lecteur natif.

### Jalon 0.0.6 — Première lecture time-stretchée

Le sixième flux vertical rend la première piste audible :

1. Rust construit un plan de lecture immuable à partir des clips et des corrections de beatgrid effectives;
2. chaque occurrence ouvre un décodeur MP3 uniquement lorsqu'elle devient audible;
3. une petite fenêtre PCM circulaire conserve les quelques milliers d'échantillons nécessaires autour de la position source courante;
4. le facteur `bpm_source / bpm_projet` détermine la relation entre les positions source et projet;
5. une source temporelle maison par grains recouvrants calcule les échantillons demandés sans utiliser le varispeed de Rodio;
6. seuls les clips actifs à la position courante sont sommés dans le bus stéréo float32;
7. un gain de protection est déduit des pics possibles des clips qui se chevauchent et la sortie est bornée à 0,98;
8. Rodio consomme cette source à la volée et fournit la position faisant autorité pour Play, Pause et Seek.

La préparation synchrone vérifie seulement que chaque fichier distinct peut être ouvert; elle ne parcourt pas son contenu. Le décodeur alimente ensuite la fenêtre PCM pendant que Rodio consomme la source, comme il le fait déjà pour la Preview. Lors d'un Seek, le décodeur saute directement près de la position source requise et reconstruit uniquement cette petite fenêtre. Une signature couvre le BPM du projet, la géométrie des clips, les corrections source et les métadonnées des fichiers. Modifier le BPM ou déplacer une occurrence reconstruit une liste compacte de relations de lecture, sans redécoder le MP3 et sans recalculer les minutes audio antérieures. La Preview et la timeline ne jouent jamais simultanément et une seule sortie Windows demeure ouverte : démarrer l'une libère la sortie de l'autre tout en conservant sa position.

La lecture 0.0.6 ne rend plus la timeline en mémoire et sa consommation PCM ne dépend plus de la durée des chansons ou du projet. Elle accepte un projet constant de quatre heures maximum, une sortie interne stéréo à 44,1 kHz et des facteurs de time-stretch de 0,5× à 2×. Le futur tempo progressif fournira un ratio variant par bloc et intégrera cette variation pour conserver une correspondance déterministe entre temps source et temps projet. L'algorithme granulaire actuel privilégie une première validation fonctionnelle du beatmatching; sa qualité devra être évaluée avec les morceaux réels de l'utilisateur avant de figer le DSP. Le rendu complet de la timeline est réservé à un éventuel export audio, jamais à la lecture interactive.

### Jalon 0.0.7 — Waveforms stéréo réelles

Le septième flux vertical remplace le motif CSS des clips par le contenu réel de chaque MP3 :

1. le passage de décodage déjà requis par l'analyse BPM collecte simultanément 2 048 colonnes de crêtes;
2. chaque colonne conserve un minimum et un maximum signés pour le canal gauche et pour le canal droit;
3. les quatre séries sont normalisées par rapport au plus grand pic absolu du morceau;
4. les valeurs float32 sont encodées en blocs binaires little-endian dans SQLite afin de garder un cache compact;
5. la réponse de timeline transmet uniquement ces crêtes simplifiées à React, jamais des blocs PCM;
6. un composant SVG mémorisé dessine les deux canaux et n'est pas recalculé pendant l'avancement du playhead;
7. les morceaux analysés avant ce jalon sont tous rattrapés en arrière-plan dès leur présence dans la bibliothèque;
8. l'ajout et le déplacement des clips restent immédiats puisque le cache est préparé avant leur arrivée dans la timeline.

Le cache est reproductible et ne fait pas partie du projet créatif. Il ne participe ni au time-stretch, ni au mixage, ni à l'horloge audio. Une réanalyse remplace atomiquement la beatgrid et la waveform du morceau. Un nouvel import obtient sa waveform durant le même décodage que son analyse BPM automatique; au démarrage, une tâche distincte complète seulement les anciens morceaux dont le cache manque. Le jalon n'ajoute aucune dépendance : le décodeur MP3, SQLite et SVG déjà présents suffisent.

### Jalon 0.0.8 — Trois pistes stéréo

Le huitième flux vertical généralise la piste initiale sans modifier le modèle temporel :

1. la timeline expose trois rangées stéréo partageant la même règle, la même grille et le même playhead;
2. un morceau de la bibliothèque peut être déposé directement sur l'une des trois pistes;
3. un clip se déplace horizontalement par beats entiers et verticalement entre les pistes dans un seul geste;
4. Rust valide de nouveau une piste comprise entre 0 et 2 ainsi que le beat d'ancrage avant toute écriture;
5. l'ajout automatique à la suite cherche la fin du contenu de la piste choisie plutôt que la fin globale du projet;
6. SQLite sauvegarde ensemble `lane` et `anchor_beat`, et la migration 5 vers 6 conserve les clips existants sur la piste 1;
7. le plan audio transporte le numéro de piste dans sa signature afin que tout déplacement invalide correctement l'état préparé;
8. les clips actifs des trois pistes sont sommés à la demande dans le bus stéréo float32 existant.

Les pistes ne possèdent pas encore de gain ou de chaîne DSP. Leur numéro est néanmoins une donnée persistante dès maintenant afin que ces fonctions puissent être ajoutées sans changer la sémantique des clips.

### Jalon 0.0.9 — Contrôle des pistes et beatmatching vérifiable

Le neuvième flux vertical rend les fonctions de contrôle immédiatement testables :

1. chaque piste possède des boutons Mute et Solo minimalistes dont l'état est persisté;
2. les trois états sont réduits à un masque de pistes audibles, où Mute demeure prioritaire sur Solo;
3. ce masque est partagé atomiquement avec la source déjà placée dans la file audio;
4. Mute et Solo prennent donc effet pendant la lecture sans reconstruction ni redécodage;
5. chaque clip affiche explicitement `BPM source → BPM projet`;
6. le ratio de durée du time-stretch est calculé par `BPM source / BPM projet`;
7. Play attend désormais la fin d'une modification de BPM déclenchée par la perte de focus avant de préparer l'audio;
8. la barre compacte de Preview est un véritable contrôle de scrub qui conserve l'état Play/Pause pendant un déplacement.

Pour une source à 125 BPM et un projet à 120 BPM, le ratio de durée vaut `125 / 120 = 1,04166…`; chaque intervalle source de `60 / 125` seconde devient donc un intervalle de `60 / 120` seconde à la sortie. L'opération inverse est vérifiée de la même manière. Cette conversion ne corrige toutefois pas une mauvaise estimation du BPM source ou du premier beat : la beatgrid du morceau doit demeurer corrigible séparément.

### Jalon 0.0.10 — Mesures 4/4 et grille source stable

Le dixième flux vertical corrige la différence entre suivre une pulsation et effectuer un beatmatching musical :

1. l'autocorrélation estime une période globale et raffine cette période avec ses harmoniques ×2 et ×4;
2. les positions persistées sont régénérées à intervalle uniforme à partir de cette période; une attaque locale ne peut plus déplacer un beat puis entraîner les suivants;
3. les accents des quatre phases possibles sont comparés pour estimer le downbeat, c'est-à-dire le temps 1 d'une mesure 4/4;
4. le champ historique `first_beat_ms` représente désormais ce premier temps automatique ou sa correction manuelle;
5. le premier temps d'un clip se cale sur une frontière de mesure du projet, soit un multiple de quatre beats;
6. React affiche immédiatement ce snapping pendant le geste et Rust recalcule la même contrainte avant la sauvegarde;
7. la migration SQLite 7 vers 8 recale les ancres existantes sur la mesure la plus proche tout en réservant assez de place au pré-roll;
8. Mute et Solo résident dans une colonne fixe extérieure à la surface horizontale; ni le zoom ni le défilement des clips ne peuvent les masquer ou les superposer à un titre. Le libellé `Px`, Mute et Solo sont empilés verticalement dans 42 px afin de réserver le maximum de largeur à la timeline.

Depuis la version d'algorithme 2, le temps 1 n'est plus choisi sur l'enveloppe large bande mais sur une enveloppe de grave extraite pendant le même décodage, filtrée à 120 Hz sur deux pôles. La raison est propre au répertoire : en musique électronique le premier temps est le kick, mais un clap ou une caisse claire sur les temps 2 et 4 produit une attaque large bande plus grande que le kick — plus brillante, spectre plus étendu, saut d'énergie plus marqué. Décider sur le mix complet revenait donc à se caler sur le contretemps. Un morceau sans grave exploitable retombe sur le mix complet, qui reste alors la meilleure preuve disponible.

Le premier temps annoncé est le premier temps de la grille sur lequel le kick tape réellement. Un morceau électronique ouvre souvent sur une minute de nappes, de souffle ou de montée avant l'entrée de la batterie; ancrer la grille sur un point extrapolé en arrière dans cette intro donne un premier temps où rien ne joue, sans usage pour le beatmatching. La recherche se fait sur la grille elle-même — la force du kick est échantillonnée à chaque temps puis moyennée sur deux mesures — et non sur le niveau brut, afin qu'un kick d'intro plus discret que le drop compte quand même. Le seuil se mesure toutefois par rapport au niveau que le morceau tient ensuite : une montée porte couramment un kick très filtré, et un seuil relatif au plancher de bruit s'ancrerait dessus plutôt que sur le temps à partir duquel un DJ compte.

Cette règle remplace aussi la comparaison des quatre phases lorsqu'elle s'applique : quand la batterie entre après une intro, elle entre sur le temps 1, ce qui est un indice bien plus fort. Avec un kick sur les quatre temps, le temps 1 et le temps 3 portent le même accent grave et aucun vote ne peut les distinguer. La comparaison des quatre phases demeure pour les morceaux qui groovent dès leur premier temps.

La phase de la pulsation est cherchée sur un temps entier. Elle est périodique, et la borner autour de la première attaque significative supposait que cette attaque tombait elle-même sur un temps; une intro électronique commence souvent par autre chose. Le choix parmi les quatre phases combine l'accentuation moyenne et un vote par mesure, afin qu'un seul passage fort ne décide pas du morceau entier.

Depuis la version d'algorithme 3, cette autocorrélation maison n'est plus la
source principale. Un petit modèle Beat This! observe le morceau complet à
22,05 kHz et produit deux suites d'événements : beats et downbeats. Le modèle
n'est toutefois **pas** l'horloge du logiciel. Il peut manquer un beat dans un
breakdown, prendre une subdivision pour un beat supplémentaire ou résumer la
suite par une médiane imprécise; copier directement ses timestamps rendrait la
timeline non uniforme et ferait dériver tout ce qui suit une omission.

MixCanvas ajuste donc sa propre grille rigide aux observations :

1. le BPM médian du modèle — ou le Tap 1 multi-mesures dans l'éditeur — fournit seulement
   l'ordre de grandeur;
2. toutes les paires d'événements séparées de 2 à 16 secondes votent pour une
   période, leur écart étant divisé par le nombre entier de beats le plus
   plausible;
3. une médiane pondérée donne la période initiale sans laisser une omission
   isolée dominer;
4. les résidus `temps modulo période` choisissent la phase à la milliseconde;
5. chaque événement reçoit indépendamment son entier de grille le plus proche;
   pour un même entier, seul le plus proche est gardé;
6. une régression linéaire robuste retire successivement les écarts de plus de
   50, 30 et 20 ms, puis recommence jusqu'à stabilisation;
7. la timeline persistée est enfin régénérée à pas **strictement uniforme** à
   partir de la période et de la phase ajustées.

Une omission ne décale ainsi jamais les beats suivants. Le BPM est conservé au
millième plutôt qu'au centième : quelques centièmes paraissent négligeables
sur une mesure mais deviennent audibles après plusieurs minutes.

Les downbeats du modèle ne sont pas non plus copiés tels quels. Chacun vote
pour l'une des quatre phases de la grille rigide, avec un poids qui diminue
selon sa distance à cette grille. La phase gagnante définit le `1`; l'enveloppe
de grave existante choisit ensuite le premier `1` réellement musical après
l'introduction. Si les modèles sont absents ou si leurs observations ne
forment aucune grille stable, l'analyseur par autocorrélation de version 2
reste le chemin de secours.

Les modèles `beat_this_small.onnx` (environ 10 Mo) et
`mel_spectrogram.onnx` (environ 270 ko) font partie des ressources de
MixCanvas. L'inférence utilise RTen en Rust pur : aucun Python, serveur, VST ou
DLL additionnelle n'est requis. Le modèle complet de 83 Mo a été testé sur le
corpus de diagnostic et n'a pas amélioré les BPM; il n'est donc pas distribué.
Les licences et empreintes SHA-256 sont consignées dans
`THIRD_PARTY_NOTICES.md`.

`ANALYSIS_ALGORITHM_VERSION` existe des deux côtés — dans `src-tauri/src/analysis.rs` et dans `src/library/types.ts` — et les deux doivent être incrémentés ensemble, sans quoi un algorithme amélioré n'atteint jamais les morceaux déjà indexés.

La détection du temps 1 reste une estimation fondée sur les accents et peut être ambiguë dans une intro sans kick ou un arrangement syncopé. L'éditeur de beatgrid nomme donc explicitement « Premier temps (1) » et permet de le capturer depuis la Preview; cette valeur manuelle demeure prioritaire. Depuis 0.0.11, les analyses mises en cache sont relancées automatiquement lorsqu'une nouvelle version d'algorithme l'exige.

Le test musical de Gutes Nitzwerk et Jestrüpp a révélé une seconde limite distincte du snapping. Les ancres étaient bien congruentes à `0 modulo 4` et le moteur appliquait exactement le ratio calculé, mais l'analyse avait enregistré respectivement 127,60 BPM avec 24,2 % de confiance et 125,92 BPM avec 56,9 % de confiance. Les tempos de référence de ces éditions sont 128 et 126 BPM. Une grille uniforme fondée sur un BPM légèrement faux dérive nécessairement; le jalon 0.0.11 corrige cette période globale.

### Jalon 0.0.11 — Raffinement BPM à longue portée

Le onzième flux vertical rend l'analyse suffisamment précise pour que le time-stretch puisse conserver un beatmatching de plusieurs minutes :

1. l'autocorrélation courte entre 70 et 190 BPM fournit toujours une hypothèse robuste contre les erreurs de demi-tempo et double-tempo;
2. cette période initiale est ensuite confrontée aux attaques séparées de 8, 16, 32 et 64 beats;
3. à chaque échelle, une recherche bornée autour du retard prédit trouve le maximum de corrélation et applique une interpolation parabolique sous-frame;
4. la période raffinée à une échelle devient l'hypothèse de l'échelle suivante, ce qui mesure la cohérence de phase sur plusieurs minutes sans arrondir le BPM à un entier;
5. l'origine de pulsation est optimisée séparément autour de la première attaque significative en maximisant l'alignement pondéré de toute la grille;
6. la détection des accents 4/4 choisit ensuite le premier temps, puis les positions persistées sont régénérées avec l'unique période globale;
7. l'indice de confiance combine désormais la corrélation courte, la meilleure corrélation à longue portée et la séparation des candidats;
8. un outil de diagnostic local réutilise exactement l'analyseur de production afin de vérifier de vrais MP3 sans dupliquer l'algorithme.

Sur les fichiers de régression réels, la même exécution qui produisait 127,60 et 125,92 BPM produit maintenant 128,00 et 126,00 BPM. Gutes Nitzwerk passe de 779 à 781 positions de beats et conserve un premier temps à 371 ms. Jestrüpp obtient un premier temps à 9 565 ms; ce déplacement d'environ un beat corrige aussi sa phase de mesure automatique.

Le schéma SQLite 9 ajoute `analysis_version`. Chaque résultat enregistré porte la version de l'algorithme qui l'a produit. Au premier démarrage après une mise à niveau, React demande automatiquement une nouvelle analyse uniquement pour les morceaux dont le cache est ancien; le traitement reste dans la tâche bloquante existante, conserve les corrections manuelles et ne se répète pas aux démarrages suivants. Un nouvel import utilise directement la version courante.

### Jalon 0.0.12 — Édition de clips pendant la lecture

Le douzième flux vertical rend la construction du mix fluide sans abandonner l'autorité du moteur audio :

1. le bouton ↦ de la bibliothèque déduit sa prochaine destination du clip créé le plus récemment et alterne ainsi P1 → P2 → P3 de façon persistante; son ancre est la mesure 4/4 la plus proche du playhead plutôt qu'une fin de piste calculée automatiquement;
2. un glisser-déposer demeure explicite : le bouton capture le pointeur et MixCanvas mesure lui-même son déplacement, sans utiliser le drag-and-drop HTML que WebView2 refusait; après un seuil de 6 px, la coordonnée verticale choisit la piste et la rangée ciblée est surlignée;
3. la surface visible entière des trois pistes reçoit le dépôt, y compris au-dessus d'un clip existant; la position horizontale tient compte du défilement avant d'être convertie en mesure;
4. l'ajout et le déplacement horizontal ou vertical restent permis pendant Play, tandis que le BPM projet, le Tap Tempo, la beatgrid et la suppression structurelle demeurent verrouillés;
5. avant une écriture, Rust mémorise le BPM et la fin du plan audio actuellement actif; après l'écriture SQLite, il produit le nouveau plan compact;
6. le lecteur capture sa position audio courante juste avant de remplacer la source mise en file, puis reprend le nouveau plan à cette même position;
7. cette actualisation rapide ne vérifie ni ne décode de nouveau tous les MP3 : chaque occurrence conserve l'ouverture paresseuse et ne reconstruit qu'une petite fenêtre PCM lorsqu'elle devient audible;
8. le transport Rust est resynchronisé avec la nouvelle durée et la même position en beats; React reçoit ensuite le snapshot persisté pour l'affichage.

Le remplacement de la source active peut provoquer une discontinuité très brève au moment du geste, mais il ne produit aucun rendu intermédiaire et ne recalcule pas les minutes audio déjà jouées. Une future file de commandes audio par blocs pourra rendre cette permutation strictement sans coupure si les essais musicaux montrent que cela est nécessaire.

### Jalon 0.0.13 — Waveform DAW et zoom atomique

Le treizième flux vertical remplace l'empreinte globale simplifiée par une représentation destinée à l'édition précise :

1. le passage de décodage existant collecte jusqu'à 16 384 colonnes au lieu de 2 048, soit huit fois plus de précision sans conserver le PCM;
2. chaque colonne conserve les minimums, maximums et niveaux RMS des canaux gauche et droit;
3. les crêtes déterminent la normalisation commune; le corps RMS demeure donc visuellement comparable aux transients au lieu d'être normalisé séparément;
4. SQLite 10 ajoute les deux enveloppes RMS, invalide uniquement les anciennes waveforms et les régénère en arrière-plan depuis les MP3;
5. React construit une pyramide par réductions successives de moitié et conserve les extrema ainsi que l'énergie RMS quadratique;
6. chaque clip sélectionne le niveau le moins coûteux qui possède encore au moins autant de colonnes que sa largeur rendue;
7. le SVG dessine un corps RMS dense, une crête min/max fine et deux axes zéro stéréo, ce qui rend les attaques courtes plus lisibles dans une piste fortement compressée;
8. ce cache visuel demeure reproductible, absent du transport et du chemin DSP temps réel.

Le zoom de timeline est également rendu atomique. Chaque cran de molette multiplie la dernière valeur effective par son facteur; plusieurs événements rapides ne peuvent plus repartir d'une valeur périmée. Les unités pixel, ligne et page sont normalisées et bornées. La largeur et la position de layout du contenu sont calculées avec le même `pixelsPerBeat`; aucune écriture de `scrollLeft` ni transformation composite ne participe au zoom.

### Jalon 0.0.14 — Carte de tempo progressive

Le quatorzième flux vertical remplace le BPM maître constant par une unique carte de tempo partagée :

1. le BPM saisi ou produit par Tap Tempo crée le point de départ virtuel à `beat = 0`;
2. chaque clip analysé crée automatiquement une cible à son ancre turquoise, égale à son BPM source effectif;
3. les points sont triés par position musicale; si plusieurs clips partagent exactement une ancre, le clip ajouté le plus récemment devient l'autorité à cet endroit;
4. le BPM progresse vers la cible suivante par **paliers d'un temps** — la rampe est échantillonnée au début de chaque temps entier et tient jusqu'au suivant —, puis demeure constant après la dernière cible;
5. la conversion beat vers secondes intègre exactement `60 / BPM(beat)`; sa fonction inverse retrouve le beat depuis le temps audio avec l'inverse exponentielle du même segment;
6. le transport, le Seek, le playhead et les trois pistes utilisent cette même paire de conversions, ce qui empêche des horloges indépendantes de diverger;
7. le moteur de time-stretch recalcule la position source cible à chaque hop depuis la carte de tempo, puis le WSOLA aligne le raccord sur la waveform locale tout en convergeant vers cette autorité temporelle;
8. un ajout ou déplacement pendant Play capture le beat courant dans l'ancienne carte, construit la nouvelle courbe, puis reprend ce même beat dans la nouvelle carte sans rendu intermédiaire;
9. la règle affiche la rampe turquoise, ses cibles et leur BPM; le déplacement provisoire d'un clip déplace aussi sa cible avant la sauvegarde;
10. les cibles sont dérivées de `project_bpm`, `anchor_beat` et du BPM effectif des morceaux : aucune donnée créative dupliquée ni migration SQLite n'est nécessaire.

Le calcul coûteux beat↔temps n'est pas exécuté à chaque échantillon. Une occurrence mémorise les positions sources du hop courant et du précédent; la courbe n'est donc consultée qu'au changement de hop, tandis que l'interpolation PCM stéréo reste locale à la petite fenêtre de décodage existante.

### Jalon 0.0.15 — VU-mètre master stéréo

Le quinzième flux vertical introduit la première composante visuelle « studio vintage » sans séparer l'affichage du vrai moteur audio :

1. le point de mesure se trouve sur le bus master stéréo après la sommation des pistes et les états Mute/Solo; il précède le limiteur, afin d'indiquer le niveau réellement produit par le mix. Le témoin `OL` a été séparé de ce point depuis l'ajout du limiteur : il est mesuré après lui, sur l'écrêtage effectivement subi;
2. les canaux gauche et droit conservent chacun une enveloppe indépendante fondée sur la valeur absolue du signal;
3. l'attaque atteint environ 99 % d'un niveau constant en 300 ms, conformément au comportement attendu d'un VU-mètre, tandis que la retombée utilise une constante d'environ 300 ms;
4. le moteur publie les enveloppes toutes les 128 frames, soit environ 2,9 ms à 44,1 kHz, dans deux atomiques float32 partagées avec la source actuellement placée dans Rodio;
5. la commande de transport déjà interrogée toutes les 50 ms transporte aussi les deux niveaux : aucun nouveau polling IPC n'est créé;
6. Pause, Seek, changement de source et fin de timeline remettent les valeurs à zéro; un compteur de génération force également la source audio suspendue à réinitialiser ses enveloppes lors de la reprise;
7. React transforme le niveau linéaire en échelle VU de −20 à +3 dB, avec `0 VU` calibré à 0,35 du niveau float32 maximal;
8. deux rangées de grandes LED circulaires horizontales flottent directement sur le panneau de l'en-tête, sans boîtier ni cadre : leurs lampes suivent la même échelle VU non linéaire, de vert à orange puis rouge, et un voyant circulaire `OL` séparé signale la surcharge. Les niveaux sont publiés même si Rodio rapporte transitoirement un état de lecture ambigu, afin que l'affichage reste fidèle au signal master produit;
9. le meter est strictement en lecture seule : il n'applique aucun gain, aucune normalisation et aucun traitement au signal observé;

Les LED du VU changent d'état directement et sans ombre portée dynamique. La lecture les actualise fréquemment; `box-shadow` ne peut pas être composé par Chromium et une trace le signalait encore comme animation à chaque changement de diode, même après retrait de la transition explicite. La couleur et le relief de la lentille suffisent à lire le niveau, sans transformer le VU en source continue de peinture et de layerisation.
10. aucune dépendance, donnée persistée ou migration SQLite n'est ajoutée.

Le style crème, bois sombre, laiton et aiguille rouge sert de prototype au futur langage visuel. Le reste de l'interface demeure inchangé afin d'évaluer ce choix avant de propager une esthétique vintage aux transports, contrôles et panneaux.

La Preview demeure un chemin d'écoute de repérage séparé et ne pilote pas le VU master de la timeline. Un éventuel meter de cue devra être présenté comme tel afin de ne pas confondre pré-écoute et sortie du mix.

### Jalon 0.0.16 — automation de volume et surcharge master

Le seizième flux vertical introduit une automation de gain non destructive sur chacune des pistes A, B et C :

1. un Volume Node contient un identifiant, une piste, une position en beats et un gain de `−∞` à `+12 dB`;
2. les positions sont calées au quart de beat et les points d'une même piste sont ordonnés dans le temps;
3. avant le premier point et après le dernier, le gain du point terminal est maintenu; sans point, la piste reste à `0 dB`;
4. entre deux points, l'interpolation est linéaire dans le domaine des décibels, puis convertie en amplitude par `10^(dB/20)`;
5. `−∞` est persisté par `NULL`, affiché comme une coupure complète et utilise `−60 dB` uniquement comme extrémité mathématique d'une rampe;
6. le gain est calculé séparément pour A/B/C après le time-stretch et avant la sommation master float32;
7. le mélange interne n'est plus abaissé automatiquement selon le nombre de clips superposés : le niveau de la timeline correspond donc au niveau nominal de la Preview à gain unitaire;
8. le VU observe le master avant sa borne de sortie, tandis qu'un état `OL` atomique s'allume dès que la valeur absolue non écrêtée dépasse la borne physique de `0,98` et conserve une brève rémanence visuelle;
9. la sortie physique demeure bornée à `0,98`; cette protection n'empêche ni le calcul float32 au-dessus de 0 dBFS ni le diagnostic fidèle d'une surcharge;
10. une édition pendant Play reconstruit seulement le plan d'automation compact et reprend au même beat, selon le mécanisme de rafraîchissement temps réel existant.

Le schéma SQLite 11 ajoute `timeline_volume_nodes` avec unicité de `(lane, beat)`. Les automations font partie des données créatives du projet, contrairement aux waveforms reproductibles. La signature du plan audio inclut tous les points afin qu'un changement de gain invalide correctement la source mise en file.

Le zoom à la molette utilise un écouteur DOM non passif unique. React publie simultanément la nouvelle largeur du contenu et sa position horizontale autour du playhead. Le suivi du transport emploie la même propriété de layout `left`; `scrollLeft` ne sert plus d'horloge visuelle et reste à zéro.

### Jalon 0.0.17 — time-stretch WSOLA et fréquence native

Le dix-septième flux vertical remplace le prototype de raccord granulaire dont les segments étaient fondus sans tenir compte de leur phase. Cette mécanique produisait un filtrage en peigne cyclique audible comme un trémolo ou une granulation, même lorsque le bus restait très loin de la saturation.

1. la position source nominale de chaque hop de 512 frames demeure dérivée de la carte globale de tempo; elle reste donc l'autorité de synchronisation et ne peut pas dériver librement;
2. la continuation naturelle du hop précédent sert de référence, et une recherche de corrélation normalisée examine une région bornée autour de la position nominale suivante;
3. la largeur de recherche dépend de la correction temporelle requise et demeure plus petite que celle-ci; le raccord gagne en cohérence de phase sans annuler progressivement le time-stretch demandé;
4. une seule position corrélée est choisie à partir de la somme mono d'analyse et appliquée aux deux canaux, ce qui préserve la phase stéréo;
5. le recouvrement utilise un fondu cosinus dont les gains sont complémentaires; les bords ont une pente nulle et ne créent plus les marches d'amplitude du fondu linéaire;
6. les positions fractionnaires sont lues par interpolation cubique à quatre points, plus fidèle dans le haut du spectre que l'interpolation linéaire précédente;
7. la fenêtre `VecDeque` réserve sa capacité de travail dès l'ouverture du clip; la recherche utilise des tableaux de pile fixes et n'alloue pas à chaque hop;
8. le moteur demande la fréquence d'échantillonnage réelle au périphérique Rodio/CPAL et construit la timeline à cette fréquence; une sortie Windows à 48 kHz ne reçoit donc plus une source artificiellement déclarée à 44,1 kHz;
9. les conversions secondes↔frames, la durée, le Seek et les constantes de temps du VU utilisent tous cette même fréquence réelle;
10. le décodage et tout le chemin DSP restent en float32. Un échantillon PCM 16 bits est représentable exactement en float32; cette conversion n'est ni une modulation, ni une source de granulation.
11. pendant Play, la zone visible reçoit une demi-largeur de viewport virtuelle avant et après son contenu. Le playhead reste alors au centre physique, tandis que `scrollLeft` suit sa position musicale à chaque état de transport; les deux extrémités conservent ainsi ce centrage sans coordonnée de défilement négative.
12. le gain de repos d'une piste sans automation est `DEFAULT_TRACK_GAIN_DB`, une constante unique que lisent le moteur float32, le node posé automatiquement et la ligne affichée. Elle valait `−6 dB` — la réserve classique pour deux platines, puisque deux morceaux beatmatchés ont leurs kicks en phase et s'additionnent de +6 dB dans le pire cas. Ce chiffre datait d'une sortie bornée en dur, où tout dépassement s'entendait; le limiteur occupant désormais cette place et travaillant par défaut, elle passe à `−4 dB`. Le moteur en tenait autrefois sa propre copie en dur, que changer la première aurait laissée derrière;
13. l'insertion d'un clip écrit deux Volume Nodes persistants à cette même valeur, calés au début et à la fin visuels de son audio. Une node déjà présente exactement à cette position reste l'autorité et n'est jamais écrasée;
14. un déplacement de clip sélectionne les nodes de son ancienne piste dont la position appartient à son intervalle audio, puis les translate du même nombre de beats et les place sur la nouvelle piste avec le clip;
15. les nodes hors de cet intervalle restent immobiles. Une collision avec une node externe à la destination annule l'opération plutôt que de supprimer silencieusement une automation existante.
16. l'import MP3 lit aussi les champs texte ID3 `TPE1` (artiste) et `TIT2` (titre), puis les enregistre avec l'entrée de bibliothèque; les variantes ID3v2.2, v2.3 et v2.4 sont prises en charge, avec repli ID3v1;
17. le schéma SQLite 12 ajoute `artist`, `title` et `id3_scanned`. Au premier démarrage après migration, les fichiers existants et accessibles sont relus une fois afin de compléter leurs tags sans réimportation; un fichier absent est laissé à analyser lorsqu'il redevient disponible.
18. l'interface dérive l'état « IN USE » de la liste des clips du projet : une entrée de Library déjà référencée reçoit un repère visuel, sans être désactivée, car le doublage intentionnel d'un morceau doit rester possible.
19. le tri de Library reste une vue locale React, donc il ne modifie ni l'ordre stocké dans SQLite ni le projet. Il peut ordonner les entrées par artiste, titre, BPM ou état « IN USE »; un second clic inverse la direction, et les valeurs absentes restent après les valeurs connues.
20. la suppression d'une entrée de Library est désormais une action contextuelle : un clic droit sur sa ligne ouvre `Remove Track`. Cette action conserve sa sémantique existante — la référence SQLite et les clips associés sont supprimés, mais le MP3 source n'est jamais effacé.
21. l'état « IN USE » est volontairement porté uniquement par l'overlay de la ligne, dans une teinte bleu-gris à faible opacité distincte des commandes d'ajout. Le BPM est présenté près du titre et Preview occupe la cellule de droite; une correction manuelle est signalée par un encadré crème, sans texte ni score de confiance.
22. Preview et l'éditeur Beatgrid utilisent une unique sortie Rodio, distincte du moteur de timeline. Les MP3 sont lus à leur taux d'échantillonnage déclaré puis convertis au taux du périphérique par le mixer; le VBR influe sur l'indexation et le Seek, pas sur la hauteur. La sortie Preview demande un tampon de 4 096 frames afin de privilégier une lecture stable pendant le décodage continu; si ce format n'est pas accepté par le périphérique, l'ouverture standard Rodio est conservée comme repli.
23. un clic de positionnement dans la timeline alors qu'une Preview est chargée est une bascule explicite vers la timeline : il enregistre d'abord le nouveau beat, libère la sortie Preview puis prépare et démarre le moteur timeline à cette position. Un cache timeline sans sortie active n'est jamais Seeké directement; il sera recréé par cette préparation.
24. `R` et `T` utilisent la même primitive de zoom que la molette, respectivement avec un delta de zoom arrière et avant équivalent à 120 px. La primitive conserve l'ancrage au playhead et son suivi centré pendant Play; ces raccourcis sont ignorés dans les champs texte et lorsqu'un modificateur système est enfoncé.
25. les nœuds BPM turquoise représentent des cibles de tempo persistantes, attachées chacune à un clip par `tempo_anchor_beat`, mais distinctes de l'ancre audio `anchor_beat`. Leur glisser-déposer ne déplace donc jamais le clip ni son automation; il recale seulement la cible sur une mesure complète, entre le début et la fin visuels du clip associé. Lorsqu'un clip est lui-même déplacé, sa cible est translatée du même delta afin de conserver leur relation.

Toute la bande verticale du ruler BPM constitue une zone de prise SVG invisible : un drag y sélectionne la cible BPM de clip la plus proche, sans seuil horizontal à deviner. Son identité React dépend uniquement du clip, jamais de sa position provisoire, afin que la capture de souris demeure active pendant un déplacement sur plusieurs mesures.

Les commandes Mute et Solo ne réservent plus une colonne de piste : deux boutons ronds M/S flottent au bord gauche de chaque voie, dans un calque non défilant au-dessus de la timeline. Elles restent donc accessibles à tout niveau de zoom sans réduire la largeur musicale disponible; seul le bouton lui-même intercepte le pointeur.

À l'ouverture du Beatgrid Editor, l'application charge automatiquement le MP3 sélectionné dans l'unique moteur Preview, le remet à zéro et le laisse en pause. Cette préparation réutilise la même bascule sûre Preview↔timeline que les autres previews : la sortie timeline est suspendue avant de réserver la sortie Preview, mais aucun son ne démarre avant l'action Play de l'utilisateur.

### Jalon 0.0.19 — Smart Filter bipolaire

Le premier jalon Smart Filter ajoute une sous-piste visible sous chaque voie audio. Elle est bipolaire : sa ligne médiane représente `0.0` et le bypass exact, la moitié haute le passe-haut et la moitié basse le passe-bas. Son interaction principale est un Filter Brush sans node visible : le beat où le drag commence devient exactement le début d'une bulle lissée, avec un retour automatique au bypass à sa fin. La bulle dure deux mesures par défaut; le déplacement horizontal pendant le geste règle sa largeur de 2 à 4 096 beats, soit 1 024 mesures — environ une demi-heure à 128 BPM. Un balayage long se dessine donc en une fois, après un zoom arrière suffisant pour que le geste couvre la durée voulue. Un geste sur une bulle existante fait l'une de deux choses selon l'endroit saisi. Près d'un de ses bords — à moins de huit pixels, une distance à l'écran plutôt qu'en beats, afin que la cible garde la même taille à tous les niveaux de zoom — le geste allonge ou raccourcit la courbe : le bord opposé reste immobile, l'intensité et la forme sont conservées, et le bord déplacé se cale sur la même grille du quart de beat que le reste du pinceau. Ailleurs dans la bulle, le geste modifie son intensité verticalement sans déplacer ni remplacer sa durée. Un redimensionnement s'arrête à la courbe voisine plutôt que de l'écraser, et ne peut ni s'inverser ni sortir des bornes de largeur.

Un redimensionnement nomme la plage qu'il remplace afin que l'ancienne étendue et la nouvelle soient écrites dans la même transaction : sans cela, une courbe raccourcie laisserait derrière elle son ancienne queue, et une courbe effacée avant d'être réécrite s'entendrait s'ouvrir en pleine lecture. Les valeurs situées à `±5 %` de la ligne centrale sont aimantées à zéro. La bande de fréquences complète reste volontairement discrète, tandis qu'une bulle dessinée devient bleu profond en passe-haut ou rouge profond en passe-bas; son contour crème lumineux conserve le highlight de l'action active.

L'espacement des échantillons persistés s'adapte à la longueur du geste : un quart de beat jusqu'à 512 échantillons, puis un pas plus large qui conserve ce plafond. Une courbe d'une demi-heure ne coûte donc ni plus de stockage, ni plus d'IPC, ni plus de hachage de signature qu'une courbe de deux mesures. Le pas demeure un multiple du quart de beat, afin que chaque échantillon tombe là où `validate_volume_beat` l'aurait calé et que la plage effacée lors d'un redessin ou d'une suppression les couvre tous. L'élargissement n'a pas d'effet audible : le moteur interpole entre les échantillons et lisse la coupure sur 8 ms, et un balayage aussi long progresse lentement.

Les échantillons internes du pinceau sont persistés dans `timeline_filter_nodes` avec la piste, le beat, la valeur normalisée `−1.0…+1.0` et une tension réservée pour la compatibilité des anciens projets. Ils appartiennent à la lane globale et ne suivent pas les clips lorsqu'ils sont déplacés. Leur écriture remplace atomiquement la zone de la bulle et ajoute des points de bypass à ses bornes : une transaction unique garantit que le DSP ne peut jamais lire un geste partiellement dessiné. La distance entre le bord d'une bulle et son point de bypass est une constante partagée : `FILTER_BUBBLE_BYPASS_EPSILON_BEATS` existe des deux côtés, dans `src/lib/filterShape.ts` et dans `src-tauri/src/timeline.rs`, afin que la courbe dessinée soit exactement celle qui est persistée.

Les trois paires voie audio / sous-piste Filter sont géométriquement identiques. Le bandeau séparateur de 6 px entre deux paires appartient au bord bas de la voie audio supérieure, et non au sommet de la bande de filtre inférieure : placé là, il empiétait sur les courbes qu'il surplombait, et un passe-haut complet venait buter dedans. Le bord bas d'une voie audio est en revanche toujours vide, un clip s'arrêtant 14 px avant et la ligne de volume n'y descendant jamais. La dernière piste ne l'affiche pas, n'ayant rien après elle dont la séparer.

Ce bandeau ne doit jamais être compensé en agrandissant une voie : une compensation en pourcentage ne peut pas suivre une hauteur en pixels fixes, et l'écart varierait alors avec la taille de la fenêtre. La ligne de bypass tombe au milieu exact de la bande, position que `filterNodeY` reproduit dans le SVG d'automation à partir de la même fraction.

Les clips et leurs waveforms forment un module visuel distinct du panneau vintage : surfaces gris anthracite légèrement éclaircies, fond waveform noir/gris, crêtes blanc cassé, corps RMS gris et axes neutres. Ils n'emploient aucun brun afin de préserver la lecture précise des transitoires sans devenir illisibles dans un panneau trop sombre.

Les clics droits Volume sont résolus au niveau du conteneur des trois paires de pistes, plutôt qu'au niveau d'une voie audio individuelle. La coordonnée verticale est ainsi toujours convertie contre la hauteur complète A/B/C, y compris lorsque le clic cible un clip superposé; un contrôle de Volume Node ou une sous-piste Filter bloque explicitement cette propagation.

Le moteur applique le filtre après le time-stretch, avant l'automation de volume et avant la sommation master. Ce jalon utilise un biquad Butterworth à Q fixe (`0,707`) : la valeur positive pilote un passe-haut de 50 Hz à 12 kHz, la valeur négative un passe-bas de 18 kHz à 90 Hz. Ces bornes évitent des extrêmes peu musicaux. Un mix sec/filtré maintient le bypass exact; un gain de compensation progressif est appliqué ensuite pour limiter la sensation de perte de niveau : 0 à +4,5 dB en passe-haut et 0 à +6 dB en passe-bas. La rampe est linéaire en dB, donc proche d'une progression régulière de niveau perçu. La valeur audio est lissée sur 8 ms pour éviter un pop lors d'une automation abrupte ou d'un passage de LP à HP. La tension du node de départ d'un segment est persistante et s'édite avec `Shift + molette` sur ce segment. React échantonne la courbe de puissance et Rust applique exactement la même fonction : `progress ^ (2 ^ (tension × 2))`. La courbe dessinée est donc celle entendue, sans segments droits d'automation.

La courbe globale reste définie en BPM dans l'espace des beats — l'axe musical de la timeline —, mais elle est **échantillonnée par temps** et non évaluée en continu : voir « Le tempo change sur les temps » plus bas. Toute autre définition de la courbe devra modifier ensemble `bpm_at_beat`, `seconds_at_beat`, leur inverse et le tracé React; elle ne doit jamais être simulée dans le seul time-stretcher.

Le WSOLA réduit fortement les artéfacts du prototype pour les écarts de tempo usuels d'un mix DJ. L'architecture conserve néanmoins le moteur derrière sa frontière interne : si les essais musicaux montrent que les ratios extrêmes ou les sources harmoniques soutenues exigent un phase-vocoder à verrouillage de phase et gestion des transitoires, ce remplacement ne devra toucher ni la carte de tempo, ni le projet SQLite, ni l'interface.

### Après 0.0.19 — dynamique master, sidechain, rognage et renommage

Ce bloc n'est pas numéroté : la numérotation des jalons avait divergé de la version du paquet, et tout ce qui suit a été livré dans la **1.0.0**.

La **chaîne master** est complète et commutable depuis deux boutons. `LIMIT` engage un limiteur stéréo-lié à la place de l'écrêtage brut. `COMP` engage un compresseur de collage à caractère fixe, sa teinte de console et sa saturation. Le détail de chaque module — seuils, constantes de temps, ordre et raisons — est décrit dans « Effets internes modulaires ». Le **sidechain** n'a pas d'interrupteur : un clip porte la clé ou ne la porte pas, ce qui est déjà la commande.

Le **rognage de clip** est décrit dans « Timeline et clips ». Il réutilise les colonnes que la scission avait introduites, sans nouveau schéma.

Trois **raccourcis** agissent sur une piste armée au pointeur : `B` scinde, `Shift+S` bascule le solo, `Shift+M` le mute. Shift est le modificateur de piste, ce qui laisse les lettres libres à la frappe; `Ctrl+S` et `Ctrl+M` sont rendus au système. Trois autres reprennent le rail d'affichage sans modificateur — `E` pour `VIEW`, `S` pour la forme du crayon, `D` pour sa période —, et suivent les mêmes conditions que les boutons : ce qui est grisé à la souris ne s'arme pas au clavier.

Le programme s'est appelé **EZ-DJ**, puis **BeatForge**. L'identifiant de paquet passant de `ca.ezdj.app` à `ca.beatforge.app`, puis à `ca.mixcanvas.app`, et Tauri en déduisant le dossier de données, une installation existante aurait démarré comme si sa bibliothèque avait disparu. `adopt_legacy_library` reprend au premier lancement la base laissée sous l'ancien identifiant, journal d'écriture anticipée compris — la base est copiée en dernier, car c'est elle que teste la garde, de sorte qu'une copie interrompue se refait au lancement suivant au lieu d'exposer une base privée de son journal.

L'interface est **unilingue anglaise**. Cinquante-huit messages d'erreur y sont passés en une fois. Un balayage par accents ne suffit pas à les trouver toutes — « Ce clip n'existe plus dans la timeline » n'en contient aucun; il faut chercher des mots-outils français, dont deux dans une même chaîne ne peuvent pas être de l'anglais.


### Après le renommage — fichier de projet, bounce, panoramique et crayon

Le **fichier de projet** `.mixcanvas` sort le travail de la base : un JSON qui porte la bibliothèque entière, les corrections de beatgrid, la carte de tempo et toute la timeline. Le cache d'analyse y est inclus plutôt que référencé, faute de quoi ouvrir un projet sur une machine neuve imposerait de tout réanalyser avant de voir quoi que ce soit. Charger **remplace** la bibliothèque au lieu de s'y ajouter : mélanger deux sessions donnait une bibliothèque qui n'appartenait à aucun des deux projets. Les champs ajoutés après coup — le panoramique, par exemple — sont lus avec `#[serde(default)]`, de sorte qu'un projet plus ancien s'ouvre encore.

Le **bounce** rend la timeline hors ligne, en 16 bits / 44,1 kHz stéréo entrelacé. Il traverse exactement la même chaîne que la lecture, chaîne master comprise, avec l'état des boutons `COMP` et `LIMIT` au moment du rendu : un fichier qui ne sonnerait pas comme ce qu'on vient d'entendre n'aurait aucun intérêt. Il n'est pas temps réel et prend le temps qu'il faut. Le silence de tête est sauté — un mix qui commence à la mesure 9 ne doit pas livrer huit mesures de rien —, la conversion en entier passe par un **dither TPDF**, et l'en-tête RIFF est réécrit à la fin quand la taille est connue. Une fenêtre de progression suit le rendu, qui tourne sur `spawn_blocking` pour ne pas geler l'interface.

L'**automation de panoramique** double celle du volume : une ligne par piste, centrée par défaut, avec sa propre couleur et son propre pointillé. Nœud vers le haut, la piste part à gauche; vers le bas, à droite. La loi est à **puissance constante** — √2/2 sur chaque côté au centre —, sans quoi le centre paraîtrait plus fort que les extrêmes et un balayage s'entendrait comme une bosse. Un clip déposé pose ses **deux ancres au centre** aux bouts de sa durée visible, comme il pose celles de volume : sans elles, une automation écrite plus loin sur la voie remonterait jusqu'à son début, la ligne rampant entre ses nœuds. Comme celle de volume, elle **suit le clip qui la contient** quand il se déplace, sur la même voie ou sur une autre : les deux tables passent par la même fonction, désignée par leur nom, faute de quoi l'une des deux finirait par cesser de suivre en silence. Ce qui est hors du clip ne bouge pas. Le bouton `VIEW` fait défiler ce qui est montré : panoramique, volume, les deux, rien. C'est un réglage d'affichage, donc local à la vue et non persisté : rien de ce que le moteur rend n'en dépend.

La bande de filtre garde son propre geste, séparé du crayon d'automation : `Ctrl` enfoncé, elle se dessine **à main levée**, la valeur pointée peinte sur chaque quart de temps parcouru. Sans `Ctrl`, le pinceau à bulle et sa pente. Une enveloppe préétablie décrit mal un balayage qu'on entend avant de savoir le nommer, et le filtre est la seule ligne toujours affichée : elle mérite son geste plutôt qu'un partage avec les autres.

Le **crayon** dessine une forme préétablie d'un seul glissé — carré, sinus, triangle, sur ½, 1, 2 ou 4 temps —, l'étendue et la hauteur du geste donnant la longueur et l'amplitude. Armé, c'est un **mode** : le déplacement de clip est suspendu et le curseur devient un crayon, sans quoi tout trait commencé sur de l'audio partirait en déplacement. Il ne peut pas s'armer sans ligne affichée, et se désarme si `VIEW` les masque toutes : dessiner sur ce qu'on ne voit pas revient à écrire dans le vide. La courbe s'écrit pendant le geste, par les mêmes fonctions que l'écriture définitive. Chaque trait est **ancré au repos à ses deux bouts** — le centre en panoramique, le niveau en place en volume — pour ne pas créer d'automation *vers* le dessin; l'ancrage est posé et non déduit, puisque selon la forme le premier point tombe sur une crête ou sur un creux. La géométrie vit dans `src/lib/automationShapes.ts`, testée seule, et le serveur revalide le plafond de nœuds qu'elle respecte déjà.

Le **sidechain lit le clip-clé tel qu'il arrive dans le mix** : son enveloppe de volume et sa place dans le champ stéréo, et non son signal brut. Baisser la clé allège le pompage, ce qui permet d'en écrire une progression; la pousser sur un côté l'allège aussi de trois décibels au maximum, comme une console dont le départ est pris après le panoramique. Les deux passent par la profondeur du ducking et non par le niveau du détecteur, dont le déclenchement compare deux énergies et ignore donc le niveau qu'on lui donne. Enfin, la **correction de beatgrid** se fait en deux temps : `Tap 1` ajuste une grille à plusieurs premiers temps choisis musicalement, puis `Snap to beat` laisse l'audio en raffiner la période sans remplacer ce choix de phase.


### Séparation en stems

Un clip porte deux touches, `VOX` et `MUS`, à gauche de la chaîne du sidechain. Trois états pour deux touches : rien d'allumé, le morceau entier. Le premier clic lance un rendu hors ligne avec la fenêtre de progression du bounce; les suivants sont instantanés.

**Seule la fenêtre que le clip fait entendre est séparée**, marge de quatre secondes comprise. Séparer un morceau entier pour huit mesures utilisées coûterait vingt fois le travail nécessaire, ce qui est insoutenable sur une longue timeline. La marge donne au modèle le contexte dont il a besoin pour décider, et laisse de quoi repousser un rognage de quelques mesures sans retomber dans le silence.

La difficulté est la géométrie : un clip calcule sa position depuis le fichier source, et un stem qui ne couvre qu'une fenêtre ne commence pas au même endroit. Le stem retient donc l'instant de la source où il commence, et le plan de rendu recule le premier temps d'autant — le clip ne bouge pas en basculant. C'est ce qui rend la fonction utilisable en mix : l'acapella d'un morceau se dépose sur l'instrumental d'un autre en étant déjà calée.

Le modèle est **open-unmix**, licence MIT, 18 Mo en demi-précision. Il reçoit un spectrogramme d'amplitude et rend un masque : la transformée de Fourier vit dans `src-tauri/src/audio/stems.rs`, jamais dans le graphe. Ce n'était pas un choix mais une contrainte — la branche spectrale de Demucs, meilleure à l'oreille, est bâtie sur des nombres complexes qu'ONNX ne sait pas représenter, et trois tentatives d'export l'ont confirmé. Seule la cible « voix » est apprise; l'instrumental est `mix − voix` dans le domaine temporel, ce qui divise le modèle par deux et garantit que les deux stems se resomment exactement en l'original.

Le travail se fait **par tranches de 256 trames** : un spectrogramme entier pèserait deux cents mégaoctets par canal. Un test vérifie que le découpage rend le même signal qu'un traitement d'un bloc, faute de quoi le raccord s'entendrait toutes les six secondes.

Les sources doivent être à **44,1 kHz** : un spectre analysé ailleurs range les mêmes sons dans d'autres bandes, et le modèle chercherait une voix là où il n'y en a pas.

## Composants du système

### 1. Bibliothèque musicale

La bibliothèque indexe les fichiers sans les dupliquer. Pour chaque fichier, elle conserve au minimum :

- chemin du fichier;
- titre et artiste lorsqu'ils sont disponibles;
- durée;
- BPM analysé et BPM corrigé, s'il y a lieu;
- beatgrid et position du premier temps;
- état de l'analyse;
- données simplifiées nécessaires à l'affichage de la forme d'onde.

Un fichier déplacé ou manquant doit être signalé et pouvoir être relié à son nouvel emplacement.

Le schéma SQLite conserve l'identifiant interne, le chemin original, une clé de chemin normalisée, le nom de fichier, l'artiste et le titre ID3 optionnels, la durée, la fréquence d'échantillonnage, le nombre de canaux, le BPM, la confiance, le premier beat, le nombre de beats, l'état de l'analyse, une erreur éventuelle et la date d'ajout. La table `track_beats` conserve chaque position en millisecondes avec une clé étrangère vers le morceau. Le schéma 5 ajoute `track_waveforms`, initialement composée de quatre blocs min/max stéréo. Le schéma 6 élargit la contrainte de `timeline_clips.lane` de la seule piste 0 aux pistes 0 à 2. Le schéma 7 ajoute `timeline_lanes` et ses états booléens Mute/Solo pour les trois pistes. Le schéma 8 ne crée aucune nouvelle colonne : il constitue une migration de données qui recale les anciennes ancres sur les mesures 4/4. Le schéma 9 ajoute la version du cache d'analyse. Le schéma 10 remplace les anciennes waveforms par six blocs min/max/RMS stéréo haute définition avec suppression en cascade. Le schéma 11 ajoute les Volume Nodes persistants des trois pistes. Le schéma 12 ajoute `artist`, `title` et le marqueur `id3_scanned` pour le rattrapage sûr des entrées existantes. Le schéma 13 ajoute `timeline_clips.tempo_anchor_beat`, initialisé à l'ancienne ancre de chaque clip afin de séparer la cible BPM de sa position audio. Le schéma 14 ajoute `timeline_filter_nodes` et ses valeurs bipolaires de filtre par lane. Le schéma 15 ajoute `timeline_clips.eq_settings`, l'égaliseur par clip sérialisé en JSON. Le schéma 16 ajoute `timeline_clips.trim_start_beats` et `trim_end_beats`, qui permettent à deux sous-clips issus d'une scission de référencer le même MP3 sans le dupliquer. Le schéma 17 ajoute `project_settings.limiter_enabled`. Le schéma 18 ajoute `project_settings.compressor_enabled`. Le schéma 19 ajoute `timeline_clips.is_sidechain_key`. Le schéma 20 supprime `project_settings.ducking_enabled`, que le schéma 19 avait introduit avant que l'interrupteur global ne se révèle redondant : les migrations forment un journal, on n'en réécrit pas les entrées passées. Le schéma 21 ajoute l'automation de panoramique par piste. Le schéma 22 introduit d'abord des stems rattachés au morceau et le sélecteur de stem du clip. Le schéma 23 remplace cette première géométrie par `clip_stems`, car seule la fenêtre réellement utilisée par chaque clip est séparée; `source_from_ms` conserve son origine dans le fichier. Le schéma 24 ajoute aux stems leurs six blocs de waveform stéréo min/max/RMS. Le schéma 25 ajoute `clip_bakes`, la cuisson d'un clip : le fichier rendu, son origine dans la source, ses six blocs de waveform, et surtout la colonne `removed`, qui conserve telle quelle l'automation retirée au moment de la cuisson. C'est elle qui rend l'opération réversible; sans elle, cuire un effet serait un aller simple, et un bouton sans retour finit par ne plus être cliqué. Le schéma 26 ajoute `timeline_draw_groups` et rattache les points de Volume/Pan générés par un geste Draw avec `draw_group_id`; le moteur conserve ainsi tous les échantillons audio tandis que l'interface peut représenter et supprimer le geste comme une seule courbe. Le schéma 27 ajoute `timeline_clips.tempo_target_bpm`, nullable : le tempo que la courbe globale vise à l'ancre de ce clip. `NULL` veut dire « la vitesse native du morceau », le cas ordinaire. Cette colonne sépare deux idées que le programme confondait — corriger une analyse fausse, qui appartient à la bibliothèque, et décider d'une vitesse de lecture, qui appartient au clip.

`CURRENT_DATABASE_SCHEMA` estampille toujours `LATEST_SCHEMA_VERSION`, car il est également rejoué après une migration afin de vérifier la présence des tables et des index. Une valeur périmée à cet endroit ramènerait la base à une version antérieure et rejouerait les dernières migrations à chaque démarrage. Les colonnes ajoutées par `ensure_column` portent les mêmes contraintes `CHECK` que le schéma neuf, afin qu'une base migrée et une base créée de zéro acceptent exactement les mêmes valeurs.

`PRAGMA user_version` versionne le schéma afin que les futures modifications puissent être migrées explicitement. Le jalon 0.0.3 migre automatiquement le schéma 1 vers le schéma 2; le jalon 0.0.4 migre ensuite le schéma 2 vers le schéma 3; le jalon 0.0.5 migre le schéma 3 vers le schéma 4; le jalon 0.0.7 migre le schéma 4 vers le schéma 5; le jalon 0.0.8 migre le schéma 5 vers le schéma 6; le jalon 0.0.9 migre le schéma 6 vers le schéma 7; le jalon 0.0.10 migre le schéma 7 vers le schéma 8; le jalon 0.0.11 migre le schéma 8 vers le schéma 9; le jalon 0.0.13 migre le schéma 9 vers le schéma 10; le jalon 0.0.16 migre le schéma 10 vers le schéma 11; le jalon 0.0.17 migre le schéma 11 vers le schéma 12; le jalon 0.0.18 migre le schéma 12 vers le schéma 13; le jalon 0.0.19 migre le schéma 13 vers le schéma 14, puis 14 vers 15 et 15 vers 16. Les schémas 17 et 18 ajoutent les interrupteurs `limiter_enabled` et `compressor_enabled`, le schéma 19 la colonne `is_sidechain_key`, et le schéma 20 retire `ducking_enabled`. Les schémas 21 à 25 ajoutent successivement le panoramique, les stems, leur portée par clip, leurs waveforms et la cuisson réversible d'un clip; le schéma 26 ajoute les groupes Draw persistants, et le schéma 27 la cible de tempo propre à chaque clip. Les migrations peuvent être enchaînées depuis le schéma 1 et conservent les entrées, leurs analyses, leurs corrections, leurs clips, leurs états de piste et leurs automations. Seul le cache waveform reproductible est invalidé au passage vers le schéma 10.

Le jalon 0.0.2 signale les fichiers manquants et permet de retirer leur référence. La fonction de reliaison vers un nouvel emplacement demeure à ajouter lorsque le format de projet utilisera lui aussi ces identifiants.

### 2. Analyse audio

L'analyse s'exécute en arrière-plan afin de ne pas bloquer l'interface. Elle décode le MP3 pour produire les informations nécessaires sans conserver inutilement tout l'audio décodé en mémoire.

L'analyseur initial est un module Rust interne sans nouvelle dépendance DSP. Il travaille sur une représentation temporelle compacte et conserve une interface de résultat indépendante de l'algorithme. Pendant le même décodage, il calcule aussi les six enveloppes min/max/RMS nécessaires à l'affichage stéréo, sans conserver le PCM complet. Un autre moteur pourra ainsi remplacer l'estimation BPM si les tests sur une collection musicale représentative montrent que sa précision est insuffisante.

La version 0.1 cherche :

- le BPM source;
- les positions des beats;
- le premier temps de la mesure;
- la durée;
- les données de forme d'onde.

La version 0.1 peut supposer que le tempo interne d'un morceau est constant. Les morceaux live, les vieux enregistrements et les chansons comportant des variations de tempo nécessiteront ultérieurement une beatgrid source à plusieurs marqueurs.

Puisque l'analyse automatique peut se tromper, les corrections minimales suivantes sont disponibles depuis le jalon 0.0.4 :

- saisir un BPM manuellement;
- doubler ou diviser le BPM par deux;
- repositionner le premier beat ou le premier temps.

Les résultats automatiques et les corrections sont stockés séparément dans le schéma 3 afin que l'analyse originale puisse être restaurée.

### 3. Courbe de tempo

La courbe est composée de marqueurs placés à des positions musicales sur la timeline. Un marqueur contient au minimum une position et une valeur BPM. Depuis le jalon 0.0.14, le point de départ provient du BPM du projet et chaque ancre turquoise produit automatiquement une cible égale au BPM effectif de son clip.

Une cible est portée par `tempo_anchor_beat`, distinct de l'ancre audio `anchor_beat` depuis le jalon 0.0.18. Cette distinction impose une règle stricte : tous les chemins qui construisent une `TempoMap` doivent la dériver de la même fonction. Le moteur audio compare la signature de la carte mise en cache à celle que le transport lui présente; deux cartes bâties sur des colonnes différentes produisent des signatures différentes, le moteur rejette alors son propre cache et le Seek cesse silencieusement de fonctionner. `tempo_targets` et `project_end_beat` sont donc les seules sources de vérité, partagées par `project_timing`, `snapshot` et `render_plan`. La longueur du projet doit pour la même raison tenir compte du rognage des sous-clips, et non de la durée complète du fichier source.

Entre deux marqueurs, le moteur offre :

- un tempo constant;
- une progression linéaire du BPM courant vers le BPM du marqueur suivant.

Exemple : un marqueur à 124 BPM suivi huit mesures plus tard d'un marqueur à 128 BPM produit une accélération graduelle sur ces huit mesures. La grille du projet, le transport et les trois pistes audio utilisent tous la même courbe.

Les marqueurs de clips se calent sur leurs ancres de mesure. Les courbes libres, les formes d'accélération complexes, l'édition manuelle de cibles intermédiaires et les automations de tempo par clip sont reportées après la version 0.1.

### 4. Tap 1 multi-mesures

Le bouton Tap 1 ne demande pas des pulsations quelconques : chaque pression désigne le premier temps de la mesure suivante. La position est lue directement dans `preview_snapshot`, donc dans l'horloge source du moteur audio. Ni l'heure de l'événement navigateur, ni le rafraîchissement périodique du mini-player ne peuvent quantifier le geste à 50 ms ou confondre une latence d'interface avec le temps du MP3.

Quatre premiers temps consécutifs sont requis, huit sont recommandés et la série s'arrête à seize sans faire glisser son premier repère. Pour le tap d'indice `i`, MixCanvas connaît le beat `4i`. Une régression linéaire des positions source sur ces indices ajuste simultanément la durée d'une mesure et l'interception de la droite : la première donne `BPM = 240 000 / millisecondes_par_mesure`, la seconde raffine la position du premier temps. Toutes les pressions contribuent, plutôt que les deux extrémités seulement, ce qui répartit l'erreur humaine sur la durée observée. L'interface expose l'erreur quadratique moyenne en millisecondes comme retour de stabilité.

Une série dont un intervalle s'écarte de plus de 40 % de l'intervalle médian est refusée : une mesure oubliée vaut presque exactement deux intervalles et ne doit jamais être interprétée comme un ralentissement réel. Revenir en arrière dans la Preview démarre automatiquement une nouvelle série; `Clear` la retire explicitement et remet les champs à la correction enregistrée. Une saisie manuelle du BPM ou du premier temps retire aussi la série afin que l'interface ne présente pas comme mesuré un nombre qui vient d'être remplacé.

Tap 1 remplit directement le BPM et le premier temps, mais ne persiste rien. Dès quatre mesures cohérentes, `Snap to beat` peut utiliser ce BPM comme fenêtre de recherche étroite et recaler la phase choisie sur le beat audio le plus proche. `Save Correction` demeure l'unique confirmation qui écrit la correction.

Le mini-player du Beatgrid Editor offre `½ SPEED`. Il s'agit volontairement d'un varispeed Rodio à `0.5`, et non du time-stretch musical de la timeline : le pitch descend, mais les attaques restent franches pour être pointées à la main. Rodio exprime toutefois `Player::get_pos()` dans le temps transformé par le varispeed : à demi-vitesse, cette coordonnée avance deux fois plus loin que la position équivalente dans le MP3 source. `PreviewEngine` multiplie donc la position Rodio par la vitesse avant de la publier, et divise inversement toute cible de Seek par cette vitesse. Tap 1 reçoit ainsi des positions source et calcule le BPM natif — donc deux fois le BPM brut observé à demi-vitesse. Lors d'un changement de vitesse, le moteur capture d'abord la position source avec l'ancien facteur, puis se recale avec le nouveau afin d'éviter tout saut. La commande IPC n'accepte que `0.5` et `1.0`. Charger un autre morceau, céder la sortie à la timeline ou fermer l'éditeur remet la Preview à vitesse normale.

La rangée de correction ne contient plus `÷2` ni `×2`. Le bouton `Clear` est rendu en permanence, désactivé tant que la série est vide : sa place est réservée avant la première pression et Tap 1 ne peut donc plus se déplacer sous la souris au milieu d'une mesure. L'accuracy RMS n'est affichée qu'une fois les quatre prises minimales obtenues; son nombre seul devient vert lorsqu'elle est strictement inférieure à 20 ms. Avant quatre prises, aucune couleur ne prétend qu'une précision a déjà été mesurée.

### 5. Timeline et clips

La timeline contient trois pistes stéréo et une piste dédiée à la courbe de tempo. Un clip représente une région d'un fichier de la bibliothèque.

Un clip conserve conceptuellement :

- le fichier source;
- la piste de destination;
- sa position de départ sur la grille du projet;
- son point d'entrée dans la beatgrid source;
- sa longueur;
- ses réglages d'égalisation propres (**Clip EQ**) et ses fenêtres de découpage non destructif (**subclips**);
- ses éventuels paramètres de gain et d'automation de volume;
- les corrections de synchronisation qui lui sont propres.

Lorsqu'un clip est déplacé :
  - `src/components/TimelinePanel.tsx` : Vue principale de la timeline multipiste (pistes audio, grilles de tempo, enveloppes de volume et filtres dynamiques).
  - `src/components/ClipEqModal.tsx` : Fenêtre modale d'Égaliseur de morçeau (Clip EQ 3 bandes + Trim Gain).
  - `src/components/HelpModal.tsx` : Fenêtre modale de guide des raccourcis clavier et contrôles d'utilisation en anglais.

Chaque clip dispose de sa propre chaîne d'égalisation paramétrique stéréo 3 bandes, entièrement traitée en temps réel par le moteur DSP Rust (`src-tauri/src/audio/timeline.rs`) :
- **Passe-Haut (HPF)** : Filtre Biquad passe-haut avec fréquence de coupure ajustable de 20 Hz à 20 000 Hz.
- **Égaliseur Bell Paramétrique (EQ3)** : Filtre Biquad peaking/notch permettant une atténuation progressive jusqu'à **-∞ dB** (coupure totale / notch) et une amplification jusqu'à **+6 dB**, avec un facteur de résonance Q ajustable de 0,1 à 10,0.
- **Passe-Bas (LPF)** : Filtre Biquad passe-bas avec fréquence de coupure ajustable de 20 Hz à 20 000 Hz.
- **Interface & Temps Réel Direct** : La fenêtre modale `ClipEqModal.tsx` et les menus d'aide contextuelle au survol (tooltips) sont intégralement en **anglais**. Tout ajustement graphique sur l'écran LCD ou via les curseurs s'applique **instantanément sur la lecture audio en cours** et persiste dans SQLite (colonne `eq_settings` via la migration v15).

#### Scission de Clip (Touche "B" / Subclips Autonomes & Anti-Chevauchement)
Appuyer sur la touche **"B"** scinde le clip de la **piste armée** au curseur de lecture. La piste est armée en pointant n'importe où dedans — bande de filtre, forme d'onde, ou l'espace entre les deux — et un repère rubis l'indique à côté de sa colonne M/S. Sans cette désignation, le raccourci prenait le premier clip du tableau enjambant le playhead, c'est-à-dire un clip arbitraire dès que plusieurs pistes jouent ensemble. `Shift+S` et `Shift+M` agissent sur la même piste armée. La scission produit :
- Le clip original est découpé de manière non destructive en deux subclips indépendants dans SQLite (migration v16 ajoutant `trim_start_beats` et `trim_end_beats`).
- Chaque subclip hérite de son propre identifiant, de son propre `ClipEqSettings`, de ses propres nœuds d'enveloppes de volume et peut être déplacé ou édité séparément sur n'importe quelle piste.
- **Affichage Précis des Waveforms** : Le composant `ClipWaveform.tsx` tronque le tableau de crêtes de forme d'onde (`leftMin`, `leftMax`, `leftRms`, etc.) exactement selon l'intervalle `[trimStartBeats, durationBeats]`. Les subclips n'affichent plus la forme d'onde entière étirée, mais uniquement leur section audio dédiée.
- **Moteur Anti-Chevauchement (Anti-Overlap)** : Le backend Rust (`clips_overlap` dans `src-tauri/src/timeline.rs`) interdit formellement toute superposition de deux clips ou subclips sur la même piste. Si un déplacement ou un ajout créerait un chevauchement (`start_a < end_b && end_a > start_b`), l'opération est refusée.
- Les migrations de la base de données utilisent la fonction résiliente `ensure_column` (inspectant `PRAGMA table_info`) afin de garantir l'idempotence des opérations de schéma et d'éviter tout conflit de nom de colonne existante (`duplicate column name`).

#### Rognage de clip (trim)

Approcher une extrémité de clip change le curseur en bracket — `[` au début, `]` à la fin — et le glissé masque ou redonne du matériel, calé sur le quart de temps. L'ancre ne bouge jamais : un rognage change ce qui est entendu du morceau, pas où le clip se trouve, de sorte que tout ce qui reste audible garde sa place sur la grille. C'est ce qui le distingue d'un déplacement.

Le rognage est stocké comme le nombre de temps masqués à chaque bout, dans les colonnes que la scission utilisait déjà. Il est donc réversible : ré-étirer est le même geste avec un nombre plus petit, jusqu'à zéro où le morceau entier revient. `set_clip_trim` revalide côté Rust ce que l'interface a déjà borné — un demi-temps de clip au minimum, et aucune croissance dans un voisin de la même piste.

La géométrie vivante du geste est calculée une seule fois, par `clipWithTrim`, et la boîte du clip comme la fenêtre de forme d'onde en descendent. Les séparer fait comprimer une tranche fixe d'échantillons dans une boîte qui rétrécit, ce qui se lit comme un time-stretch et non comme un rognage.

Le prototype expose les trois pistes, leurs états Mute/Solo et la rampe du tempo global. SQLite conserve `library_track_id`, `lane`, `anchor_beat`, `eq_settings`, `trim_start_beats` et `trim_end_beats`. La cible BPM est dérivée du morceau afin qu'une correction de beatgrid demeure l'unique autorité. La durée visuelle, le pré-roll et les extrémités du clip sont aussi des valeurs dérivées plutôt que des données dupliquées. Le clic d'ajout automatique part de la piste suivant le clip le plus récent, puis retient la première des trois qui soit réellement libre à cet endroit — la rotation ne fixe que l'ordre d'essai, car avancer d'une piste ne dit rien sur sa disponibilité. Le premier temps du nouveau clip se place sur la mesure la plus proche du playhead. L'erreur de chevauchement n'est renvoyée que si les trois pistes sont occupées. Un dépôt choisit plutôt sa piste et sa mesure sous le pointeur. Le dépôt et le déplacement restent disponibles pendant Play, et Rust remplace alors le plan audio compact au même beat musical.

### 6. Preview

Le Preview utilise un chemin d'écoute distinct de la timeline. Il lit le morceau à son tempo et à sa tonalité d'origine afin de permettre une audition fidèle avant de l'ajouter au projet.

Une seule preview peut jouer à la fois dans la version 0.1. Son indicateur compact n'est monté dans l'interface que lorsqu'un morceau a été chargé depuis la bibliothèque; il affiche le morceau, la progression et uniquement la commande Play/Pause. Depuis le jalon 0.0.6, démarrer une Preview met la timeline en pause et démarrer la timeline met la Preview en pause, ce qui évite deux périphériques concurrents audibles.

La progression de l'indicateur compact est de nouveau interactive depuis le jalon 0.0.9. Un clic ou un glissement transmet une position en millisecondes au moteur natif, qui la borne à la durée et conserve l'état de lecture précédent. Le contrôle est désactivé pendant la lecture de la timeline afin de ne pas rouvrir un second périphérique audio concurrent.

### 7. Transport et moteur audio

Le moteur audio est l'autorité temporelle pendant la lecture. L'horloge de l'interface ne doit jamais servir à synchroniser les pistes.

Pour chaque portion audio traitée, le moteur doit :

1. déterminer la position exacte du transport;
2. obtenir le BPM cible dans la courbe globale;
3. déterminer quels clips sont actifs;
4. lire leurs données sources;
5. appliquer le time-stretch requis sans changer leur tonalité;
6. aligner leurs beats sur la grille du projet;
7. mixer les trois pistes en float32;
8. appliquer le traitement du bus master;
9. envoyer le résultat vers la sortie stéréo.

Play, Pause et le repositionnement de la tête de lecture doivent tous reprendre à une position déterministe et alignée à l'échantillon près. La fluidité visuelle de la tête de lecture est secondaire à la stabilité de l'audio.

Dans le jalon 0.0.14, `TempoMap` est l'autorité commune. Elle a d'abord intégré une rampe **continue** : le BPM variant linéairement dans l'espace musical, le temps exact d'un beat venait d'une intégrale logarithmique et son inverse d'une exponentielle. Ces deux formes closes ont disparu avec la quantification par temps — un temps durant exactement `60 / bpm`, la somme s'accumule une fois pour toutes dans une table et se relit par dichotomie. Voir « Le tempo change sur les temps ». La source Rodio demande toujours les échantillons en ordre; chaque clip actif met en cache les débuts sources de deux grains, effectue l'interpolation PCM et leur fondu croisé, puis les trois pistes sont additionnées en `f32`. La protection de niveau et la sortie restent elles aussi en float32. Aucun buffer du mix complet, resampling varispeed ou changement de hauteur n'est produit.

### 8. Effets et routage

Le chemin de signal est aujourd'hui le suivant : source du clip, time-stretch, égaliseur du clip, filtre de la sous-piste, automation de volume de la voie, sommation, puis la chaîne master — sidechain, compresseur, teinte de console, mesure, limiteur, borne de sortie.

Cette section décrivait autrefois cette chaîne comme un projet. Elle est implémentée : le détail de chaque module et le raisonnement derrière son caractère se trouvent dans « Effets internes modulaires », plus haut dans ce document. Le sidechain n'a pas d'interrupteur global — un clip porte la clé ou ne la porte pas —, et son détecteur écoute le clip-clé réel plutôt qu'une copie interne.

Ce qui reste effectivement à faire : un graphe de routage général, dans lequel une voie pourrait alimenter le détecteur d'un module sans changer sa destination audible. Un tel graphe devra interdire les boucles de rétroaction dès sa première version. La compensation automatique de latence et l'automation des paramètres d'effet ne seront ajoutées que lorsqu'un effet les rendra nécessaires; aucun module actuel n'introduit de latence à compenser.

### 9. Persistance et cache

Deux catégories de données doivent rester séparées :

- le projet, qui contient les choix créatifs de l'utilisateur;
- le cache d'analyse, qui contient des résultats reproductibles associés aux fichiers audio.

La suppression d'un cache ne doit pas détruire un projet. Le logiciel doit pouvoir reconstruire les formes d'onde et les analyses manquantes.

L'unique projet courant reste enregistré dans `library.sqlite3` : c'est l'état qu'on retrouve au lancement, sans rien demander. Le format exportable existe depuis `Save` / `Load` — un fichier `.mixcanvas` décrit plus haut — mais il ne remplace pas ce stockage, il le sérialise. Plusieurs projets ouverts en même temps restent hors portée.

## Flux principal

1. L'utilisateur ajoute un dossier ou des fichiers à la bibliothèque.
2. L'analyse produit le BPM, la beatgrid, le premier temps et la forme d'onde.
3. L'utilisateur peut écouter le fichier avec Preview et corriger l'analyse.
4. Il glisse le fichier sur l'une des trois pistes.
5. Le clip s'aligne sur la grille musicale globale.
6. La courbe de tempo indique le BPM cible à chaque position.
7. Le moteur adapte continuellement chaque clip actif à ce BPM sans changer sa tonalité.
8. Le mix float32 est envoyé vers le bus master puis vers la sortie stéréo.

## Critères techniques de réussite de la version 0.1

- Deux morceaux correctement analysés restent alignés pendant une rampe de tempo.
- La tonalité perçue ne change pas lorsque le tempo varie.
- Le déplacement d'un clip conserve son alignement musical.
- Play/Pause et le repositionnement ne produisent pas de dérive entre les pistes.
- Une analyse incorrecte peut être corrigée sans modifier le fichier source.
- Les travaux d'analyse n'interrompent pas la lecture.
- Le mixage interne reste en float32 jusqu'au traitement de sortie.
- Un projet sauvegardé se rouvre avec les mêmes clips, marqueurs et alignements.

## Hors portée de la version 0.1

- vidéo;
- détection et harmonisation de tonalité;
- hébergement de plugins audio externes;
- graphe de routage général et compensation automatique de latence;
- support complet des morceaux à tempo source variable;
- intégrations avec des bibliothèques DJ externes;
- courbes de tempo libres;
- ouverture simultanée de plusieurs projets;
- fonctions de performance DJ en direct.

## Non-objectifs du produit

- héberger des plugins VST3, Audio Unit ou tout autre plugin tiers;
- fournir une API binaire permettant de charger des effets externes;
- devenir une station audio généraliste ou un environnement de développement de plugins.

## Décisions encore ouvertes

- moteur de time-stretch définitif après l'évaluation musicale de l'algorithme maison 0.0.6;
- format du fichier de projet;
- limites définitives de time-stretch avant d'afficher un avertissement;
- présence ou non d'un export audio dans la version 0.1.
- emplacement des chaînes d'effets dans l'interface et ordre de traitement modifiable ou fixe;
- procédure de distribution des binaires, du code source correspondant et des avis de licences tierces;
