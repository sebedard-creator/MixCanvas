/**
 * Ce que l'écran des effets partage avec le reste du programme.
 *
 * Le nombre de pistes et la façon de les nommer existaient déjà en plusieurs
 * exemplaires — `TIMELINE_LANES` dans le panneau, `String.fromCharCode(65 + …)`
 * recopié partout. C'est la forme exacte du défaut qui revient dans ce projet :
 * une règle écrite à deux endroits, qui finit par diverger.
 */

/** Trois pistes stéréo : A, B et C. */
export const TIMELINE_LANE_COUNT = 3;

/**
 * Les effets qu'on joue à la main, dans l'ordre où ils se présentent.
 *
 * Le même nom exactement que côté Rust, où `PlayedEffect` le reçoit tel quel :
 * c'est un contrat entre deux langages, et un effet mal orthographié serait
 * refusé par la commande plutôt que joué sur le mauvais bus.
 */
export const PLAYED_EFFECTS = ["reverb", "flanger", "bitcrush", "delay"] as const;
export type PlayedEffect = (typeof PLAYED_EFFECTS)[number];

/**
 * La couleur de chaque effet sur la timeline.
 *
 * Aucune autre ligne ne les emploie — le volume est ambre, le panoramique
 * pointillé, le filtre bleu, le tempo turquoise — donc la couleur seule dit de
 * quel effet il s'agit, sans étiquette à lire pendant qu'on mixe.
 */
export const EFFECT_TINTS: Record<PlayedEffect, string> = {
  reverb: "#7c5cd6",
  flanger: "#2fa87a",
  bitcrush: "#cf4593",
  delay: "#e2622b",
};

/** Le nom écrit sur la pastille, en majuscules comme le reste de la console. */
export const EFFECT_LABELS: Record<PlayedEffect, string> = {
  reverb: "REVERB",
  flanger: "FLANGE",
  bitcrush: "CRUSH",
  delay: "DELAY",
};

/** La lettre d'une piste, telle qu'elle s'affiche partout. */
export function laneLetter(lane: number): string {
  return String.fromCharCode(65 + lane);
}

export interface ReverbNode {
  beat: number;
  value: number;
}

/**
 * Les tranches où une voie envoie dans la reverb, en beats.
 *
 * Les nœuds décrivent une courbe; la timeline, elle, veut des **régions** à
 * teinter. Une région va du premier nœud non nul au dernier, rampes comprises :
 * c'est exactement ce qu'on a joué.
 *
 * Séparé du composant et vérifié : c'est une conversion de données, et la
 * relire dans du JSX ne dirait pas si elle est juste.
 */
export interface ReverbSpan {
  startBeat: number;
  endBeat: number;
  /** Le sommet atteint dans la tranche, pour doser la teinte. */
  peak: number;
  /**
   * Les nœuds de la tranche, bornes comprises.
   *
   * Ils sont conservés parce que la teinte doit être **la courbe**, et non une
   * approximation de sa silhouette. Une région ne s'est longtemps dessinée
   * qu'avec ses deux bords et un dégradé fixe aux extrémités, à douze pour cent
   * de sa largeur; or les rampes écrites sont absolues — un huitième de temps à
   * la montée, trois quarts à la descente. Sur une longue passe le mauve
   * montait quinze fois trop lentement, sur une courte il retombait trop vite,
   * et l'écart changeait de sens avec la durée. Avec les nœuds, ce qui est
   * imprimé se déduit de ce qui est joué.
   */
  nodes: ReverbNode[];
}

export function reverbSpans(nodes: readonly ReverbNode[]): ReverbSpan[] {
  const sorted = [...nodes].sort((left, right) => left.beat - right.beat);
  const spans: ReverbSpan[] = [];
  let open: ReverbSpan | null = null;

  for (const node of sorted) {
    const point = { beat: node.beat, value: node.value };
    if (node.value > 0) {
      if (open === null) {
        open = { startBeat: node.beat, endBeat: node.beat, peak: node.value, nodes: [] };
      }
      open.endBeat = node.beat;
      open.peak = Math.max(open.peak, node.value);
      open.nodes.push(point);
      continue;
    }
    // Un nœud à zéro ferme la tranche **sur lui** : c'est le bas de la rampe
    // de sortie, donc la fin de ce qu'on entend.
    if (open !== null) {
      open.endBeat = node.beat;
      open.nodes.push(point);
      spans.push(open);
      open = null;
      continue;
    }
    // Un zéro isolé avant toute montée est le bas de la rampe d'entrée : il
    // ouvre la tranche plutôt que de la fermer.
    open = { startBeat: node.beat, endBeat: node.beat, peak: 0, nodes: [point] };
  }

  if (open !== null) spans.push(open);
  // Une tranche sans sommet n'a rien à montrer : deux nœuds à zéro qui se
  // suivent ouvrent puis referment un vide, et il ne doit pas se teinter.
  return spans.filter((span) => span.peak > 0 && span.endBeat > span.startBeat);
}

/** Un arrêt du dégradé qui teinte une région : sa place, et son opacité. */
export interface ReverbGradientStop {
  /** Sa place dans la région, de zéro à un. */
  offset: number;
  /** La valeur de l'automation à cet endroit, de zéro à un. */
  opacity: number;
}

/**
 * Le dégradé d'une région, déduit de ses nœuds.
 *
 * Un arrêt par nœud, à sa place exacte. C'est ce qui fait de la teinte une
 * copie de la courbe et non une silhouette : entre deux nœuds, l'automation
 * interpole linéairement en beats, et un dégradé SVG interpole linéairement sur
 * la largeur — la région couvrant exactement la plage des nœuds, les deux
 * interpolations sont la même.
 */
export function reverbGradientStops(span: ReverbSpan): ReverbGradientStop[] {
  const width = span.endBeat - span.startBeat;
  if (!(width > 0)) return [{ offset: 0, opacity: span.peak }];
  return span.nodes.map((node) => ({
    // Bornées : un nœud hors de la plage viendrait d'une base abîmée, et un
    // arrêt négatif ferait disparaître le dégradé entier plutôt que ce nœud.
    offset: Math.min(1, Math.max(0, (node.beat - span.startBeat) / width)),
    opacity: Math.min(1, Math.max(0, node.value)),
  }));
}

/** Une région posée sur une voie, avec l'effet qui la porte. */
export interface EffectRegion extends ReverbSpan {
  lane: number;
  effect: PlayedEffect;
}

/**
 * Les régions qui touchent la fenêtre visible.
 *
 * Toutes les autres couches de la timeline sont déjà fenêtrées; celle-ci est
 * arrivée après et ne l'était pas. Chaque région porte son propre dégradé, avec
 * un arrêt par nœud : une séance chargée en laissait donc des centaines dans le
 * document, reconstruits à chaque rendu, pour une poignée de visibles.
 *
 * Le test est un simple **recouvrement**, sans le voisin de chaque côté que
 * `nodesAcross` conserve. La différence est réelle : une courbe est une
 * polyligne, dont le segment qui entre dans la fenêtre a besoin du point d'avant
 * pour avoir la bonne pente. Une région, elle, est un rectangle qui se suffit —
 * ce qui est hors du cadre ne change rien à ce qui est dedans.
 */
export function regionsAcross<T extends { startBeat: number; endBeat: number }>(
  regions: readonly T[],
  fromBeat: number,
  toBeat: number,
): T[] {
  return regions.filter((region) => region.endBeat >= fromBeat && region.startBeat <= toBeat);
}

/**
 * Une plage où plusieurs effets jouent en même temps sur la même voie.
 *
 * Deux teintes translucides superposées donnent une troisième couleur, qui ne
 * dit ni l'une ni l'autre : du mauve sous du vert n'est plus ni de la reverb ni
 * du flanger, c'est une couleur qu'aucune légende n'explique. Les bandes
 * hachurées en diagonale, elles, gardent les deux visibles — on lit les deux
 * effets plutôt qu'un mélange.
 */
export interface HatchBand {
  lane: number;
  startBeat: number;
  endBeat: number;
  /** Les effets présents, dans l'ordre de `PLAYED_EFFECTS`. Toujours au moins deux. */
  effects: PlayedEffect[];
}

export function hatchBands(regions: readonly EffectRegion[]): HatchBand[] {
  const bands: HatchBand[] = [];
  const lanes = new Set(regions.map((region) => region.lane));

  for (const lane of [...lanes].sort((left, right) => left - right)) {
    const onLane = regions.filter((region) => region.lane === lane);
    // Chaque bord d'une région est un endroit où la réponse peut changer;
    // entre deux bords consécutifs, elle ne change pas.
    const edges = [...new Set(onLane.flatMap((r) => [r.startBeat, r.endBeat]))]
      .sort((left, right) => left - right);

    for (let index = 0; index + 1 < edges.length; index += 1) {
      const [from, to] = [edges[index], edges[index + 1]];
      if (!(to > from)) continue;
      const middle = (from + to) / 2;
      const effects = PLAYED_EFFECTS.filter((effect) =>
        onLane.some(
          (region) =>
            region.effect === effect && middle >= region.startBeat && middle <= region.endBeat,
        ),
      );
      if (effects.length < 2) continue;

      // Deux tranches voisines portant les mêmes effets sont une seule bande :
      // les recoller évite une couture visible là où rien ne change.
      const previous = bands.at(-1);
      const sameSet =
        previous !== undefined
        && previous.lane === lane
        && previous.endBeat === from
        && previous.effects.length === effects.length
        && previous.effects.every((effect, at) => effect === effects[at]);
      if (sameSet && previous !== undefined) {
        previous.endBeat = to;
        continue;
      }
      bands.push({ lane, startBeat: from, endBeat: to, effects: [...effects] });
    }
  }

  return bands;
}

/**
 * L'identifiant du dégradé d'une région.
 *
 * Chaque région a le sien, puisque ses arrêts lui sont propres. Ici plutôt que
 * dans le composant parce que la valeur est écrite à deux endroits — sur le
 * `<linearGradient>` et dans le `url(#…)` qui le cherche — et que deux endroits
 * qui doivent s'accorder valent une seule fonction.
 */
export function reverbGradientId(
  effect: PlayedEffect,
  lane: number,
  startBeat: number,
): string {
  // Un beat est fractionnaire et peut être négatif dans un projet abîmé : ni le
  // point ni le signe n'ont leur place dans un identifiant que `url(#…)` doit
  // ensuite retrouver.
  return `fx-fade-${effect}-${lane}-${String(startBeat).replace(/[^0-9]+/g, "_")}`;
}

/**
 * L'identifiant du motif hachuré d'une combinaison d'effets.
 *
 * Un motif par combinaison, pas par bande : deux endroits où la reverb et le
 * flanger se recouvrent portent exactement les mêmes rayures, et n'ont donc
 * aucune raison d'avoir chacun leur définition.
 */
export function hatchPatternId(effects: readonly PlayedEffect[]): string {
  return `fx-hatch-${effects.join("-")}`;
}

/**
 * Les voies dont une passe est en train de jouer à ce beat, en masque.
 *
 * La pastille de l'écran s'allume là-dessus autant que sur l'appui : quand la
 * timeline rejoue ce qu'on y a écrit, l'écran doit le montrer, sinon il ment
 * sur ce qu'on entend. Bornes **incluses** des deux côtés — le bord d'une
 * passe fait partie de la passe.
 */
export function lanesPlayingAt(
  spansByLane: readonly (readonly ReverbSpan[])[],
  beat: number,
): number {
  if (!Number.isFinite(beat)) return 0;
  return spansByLane.reduce((mask, spans, lane) => {
    const inside = spans.some((span) => beat >= span.startBeat && beat <= span.endBeat);
    return inside ? mask | (1 << lane) : mask;
  }, 0);
}
