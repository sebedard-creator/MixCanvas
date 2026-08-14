import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { LiveTransport } from "../lib/liveTransport";
import {
  EFFECT_LABELS,
  PLAYED_EFFECTS,
  TIMELINE_LANE_COUNT,
  laneLetter,
  lanesPlayingAt,
  reverbSpans,
} from "../lib/mixEffects";
import type { PlayedEffect } from "../lib/mixEffects";
import type { TransportGlyphName } from "./TransportGlyph";
import type { TimelineReverbNode } from "../timeline/types";
import { TransportGlyph } from "./TransportGlyph";

interface MixFxBayProps {
  /** Le masque des pistes dont le bouton d'un effet est enfoncé, un bit par piste. */
  onSetKeys: (effect: PlayedEffect, keys: number) => void;
  /**
   * Le masque des pistes sous la gomme, pour que le moteur les taise.
   *
   * Le balayage ne s'écrit qu'au relâchement; sans ce masque on entendrait la
   * passe continuer pendant qu'on l'efface. Un seul masque pour tous les
   * effets : la gomme est un seul bouton, et elle emporte tout.
   */
  onSetErasing: (lanes: number) => void;
  /** Sert à éteindre les pastilles à l'arrêt : à l'arrêt, on n'entend rien. */
  transportStatus: "paused" | "playing";
  /**
   * Signale une passe **en cours**, pour que la timeline la dessine en direct.
   *
   * Le beat de départ à l'appui, `null` au relâchement — c'est à ce moment que
   * la vraie région prend le relais. Sans cela, on jouait à l'aveugle : rien
   * n'apparaissait avant qu'on ne lève le doigt.
   */
  onLivePass: (effect: PlayedEffect, lane: number, startBeat: number | null) => void;
  /** Écrit une passe jouée sur la timeline, au relâchement. */
  onWriteSpan: (effect: PlayedEffect, lane: number, startBeat: number, endBeat: number) => void;
  /** Retire l'automation de **tous** les effets balayés par la gomme. */
  onEraseSpan: (lane: number, startBeat: number, endBeat: number) => void | Promise<void>;
  /** Les passes déjà écrites, pour que la pastille les suive à la lecture. */
  nodesByEffect: Record<PlayedEffect, readonly TimelineReverbNode[]>;
  liveTransport: LiveTransport;
  busy: boolean;
}

/**
 * Les effets joués, à demeure dans la console.
 *
 * Ils ont d'abord vécu dans un panneau qu'une touche `MIX FX` ouvrait par
 * dessus la bibliothèque. Le panneau est dissous ici, et les deux touches qui
 * le commandaient — `MIX FX` et `AUTO` — ont rendu leur place : la barre du
 * haut avait 201 px morts entre le VU-mètre et `BOUNCE MIX`, et ces deux
 * touches en libèrent 138 de plus. Les 339 px qui en résultent tiennent la
 * grille entière.
 *
 * Ce qu'on y gagne dépasse la place : un panneau qu'il faut ouvrir est un
 * panneau qu'on oublie de fermer, et il masquait la colonne qu'il recouvrait.
 * Une baie posée dans la console ne cache rien et ne se range pas.
 *
 * Ce qu'on y perd est la taille des pastilles : 59 × 22 au lieu de 52 carrés.
 * La hauteur de la barre est le facteur qui limite — trois pistes dans 82 px —
 * et elle ne bouge pas, sinon c'est la timeline qui rétrécit. La marque de
 * 12 px tient; il n'y a plus de place pour l'écrire en toutes lettres, d'où
 * l'infobulle et `aria-label` sur chaque pastille.
 *
 * **Aucun raccourci clavier**, et c'est un retrait volontaire d'avant le
 * déménagement. Il y en avait quinze — quatre effets et une gomme sur trois
 * pistes — qui empiétaient sur ceux de la timeline : il a fallu museler tous
 * les raccourcis de la timeline pendant l'ouverture du panneau, puis ignorer
 * les frappes modifiées parce que `Ctrl+Z` jouait la reverb, puis reprendre la
 * barre d'espace qu'on venait de perdre. Trois correctifs pour un confort que
 * personne n'utilisait. Ce qu'on y perd est réel et vaut d'être dit : à la
 * souris, on ne tient qu'un bouton à la fois.
 */

/**
 * La marque de chaque effet.
 *
 * Une image plutôt qu'un nom : sur une pastille de cette taille, une marque
 * figurative se reconnaît d'un coup d'œil là où trois lettres demandent d'être
 * lues — et l'œil est occupé ailleurs, sur la piste qui défile.
 */
const EFFECT_GLYPHS: Record<PlayedEffect, TransportGlyphName> = {
  reverb: "fx-reverb",
  flanger: "fx-flanger",
  bitcrush: "fx-bitcrush",
  delay: "fx-delay",
};

/** Un masque par effet, tous à zéro. */
function noLanes(): Record<PlayedEffect, number> {
  return { reverb: 0, flanger: 0, bitcrush: 0, delay: 0 };
}

/** Un départ de passe par piste et par effet. */
function noStarts(): Record<PlayedEffect, (number | null)[]> {
  return {
    reverb: Array(TIMELINE_LANE_COUNT).fill(null),
    flanger: Array(TIMELINE_LANE_COUNT).fill(null),
    bitcrush: Array(TIMELINE_LANE_COUNT).fill(null),
    delay: Array(TIMELINE_LANE_COUNT).fill(null),
  };
}

export function MixFxBay({
  onSetKeys,
  onSetErasing,
  transportStatus,
  onLivePass,
  onWriteSpan,
  onEraseSpan,
  nodesByEffect,
  liveTransport,
  busy,
}: MixFxBayProps) {
  /**
   * Quelles pistes sont tenues, par effet, ici et maintenant.
   *
   * Gardé aussi dans une référence : les écouteurs de pointeur vivent hors du
   * rendu et doivent lire l'état courant, pas celui qu'ils ont capturé en
   * s'installant.
   */
  const [heldLanes, setHeldLanes] = useState(noLanes);
  const heldRef = useRef(noLanes());

  /**
   * Le beat où chaque passe en cours a commencé, ou `null`.
   *
   * Relevé à l'appui et relu au relâchement : c'est ce couple qui devient une
   * courbe sur la timeline. Dans une référence, parce que le relâchement peut
   * venir d'un écouteur installé bien avant le rendu courant.
   */
  const spanStarts = useRef(noStarts());

  const publish = useCallback(
    (effect: PlayedEffect, next: number) => {
      if (next === heldRef.current[effect]) return;
      heldRef.current = { ...heldRef.current, [effect]: next };
      setHeldLanes((current) => ({ ...current, [effect]: next }));
      onSetKeys(effect, next);
    },
    [onSetKeys],
  );

  const hold = useCallback(
    (effect: PlayedEffect, lane: number) => {
      if (spanStarts.current[effect][lane] === null) {
        const startBeat = liveTransport.read().positionBeat;
        spanStarts.current[effect][lane] = startBeat;
        onLivePass(effect, lane, startBeat);
      }
      publish(effect, heldRef.current[effect] | (1 << lane));
    },
    [liveTransport, onLivePass, publish],
  );

  const release = useCallback(
    (effect: PlayedEffect, lane: number) => {
      const start = spanStarts.current[effect][lane];
      spanStarts.current[effect][lane] = null;
      onLivePass(effect, lane, null);
      publish(effect, heldRef.current[effect] & ~(1 << lane));
      // La passe ne s'écrit que si la tête de lecture a réellement avancé.
      // Tenir le bouton à l'arrêt s'entend, mais n'a pas de durée à inscrire.
      if (start === null) return;
      const end = liveTransport.read().positionBeat;
      if (end - start > 1.0e-6) onWriteSpan(effect, lane, start, end);
    },
    [liveTransport, onLivePass, onWriteSpan, publish],
  );

  /**
   * La gomme, tenue au-dessus d'une piste.
   *
   * Le balayage ne s'applique à la base qu'au relâchement — sa longueur n'est
   * connue qu'à la fin — mais le moteur est prévenu **à l'appui** : sans cela
   * on entendait la passe continuer sous la gomme, et l'on ne savait ce qu'on
   * avait retiré qu'après avoir levé le doigt. Effacer doit s'entendre pendant
   * le geste, comme jouer.
   */
  const [erasingLanes, setErasingLanes] = useState(0);
  const erasingRef = useRef(0);
  const eraseStarts = useRef<(number | null)[]>(Array(TIMELINE_LANE_COUNT).fill(null));

  const publishErasing = useCallback(
    (next: number) => {
      if (next === erasingRef.current) return;
      erasingRef.current = next;
      setErasingLanes(next);
      onSetErasing(next);
    },
    [onSetErasing],
  );

  const holdEraser = useCallback(
    (lane: number) => {
      if (eraseStarts.current[lane] !== null) return;
      eraseStarts.current[lane] = liveTransport.read().positionBeat;
      publishErasing(erasingRef.current | (1 << lane));
    },
    [liveTransport, publishErasing],
  );

  const releaseEraser = useCallback(
    (lane: number) => {
      const start = eraseStarts.current[lane];
      eraseStarts.current[lane] = null;
      const lift = () => publishErasing(erasingRef.current & ~(1 << lane));

      const end = start === null ? null : liveTransport.read().positionBeat;
      if (start === null || end === null || end - start <= 1.0e-6) {
        lift();
        return;
      }
      /* Le masque ne se lève qu'une fois l'effacement écrit et le plan
         reconstruit. Le lever tout de suite rendrait la parole à la passe
         qu'on vient de retirer, le temps de l'aller-retour — un dernier
         soubresaut de ce qu'on croyait avoir effacé. */
      void Promise.resolve(onEraseSpan(lane, start, end)).finally(lift);
    },
    [liveTransport, onEraseSpan, publishErasing],
  );

  /**
   * Les voies dont une passe **enregistrée** est en train de jouer.
   *
   * La pastille doit s'allumer quand la timeline joue ce qu'on y a écrit, pas
   * seulement quand un doigt appuie : sans cela la console ment sur ce qu'on
   * entend. Même source de vérité que les teintes de la timeline — les mêmes
   * tranches.
   */
  const [playingLanes, setPlayingLanes] = useState(noLanes);
  const spans = useMemo(
    () =>
      Object.fromEntries(
        PLAYED_EFFECTS.map((effect) => [
          effect,
          Array.from({ length: TIMELINE_LANE_COUNT }, (_, lane) =>
            reverbSpans(nodesByEffect[effect].filter((node) => node.lane === lane)),
          ),
        ]),
      ) as Record<PlayedEffect, ReturnType<typeof reverbSpans>[]>,
    [nodesByEffect],
  );
  const spansRef = useRef(spans);
  spansRef.current = spans;

  useEffect(() => {
    /* À l'arrêt, aucune pastille ne s'allume depuis la timeline : elle montre
       ce qu'on **entend**, et à l'arrêt on n'entend rien.

       Le transport n'est interrogé que pendant la lecture, donc le dernier
       masque publié survivrait à la pause : mettre en pause à l'intérieur
       d'une passe laisserait sa pastille allumée pour de bon, et rien ne
       pourrait plus la contredire puisque plus aucun instantané n'arrive. */
    if (transportStatus !== "playing") {
      setPlayingLanes(noLanes);
      return undefined;
    }
    /* Un abonnement plutôt qu'un rendu : la position avance vingt fois par
       seconde, mais une pastille ne change d'état qu'en franchissant le bord
       d'une passe. On ne réveille React que là. */
    return liveTransport.subscribe((snapshot) => {
      setPlayingLanes((current) => {
        let changed = false;
        const next = { ...current };
        for (const effect of PLAYED_EFFECTS) {
          const mask = lanesPlayingAt(spansRef.current[effect], snapshot.positionBeat);
          if (mask !== current[effect]) {
            next[effect] = mask;
            changed = true;
          }
        }
        return changed ? next : current;
      });
    });
  }, [liveTransport, transportStatus]);

  /** Abandonne les gestes en cours sans rien écrire ni effacer. */
  const abandon = useCallback(() => {
    for (const effect of PLAYED_EFFECTS) {
      spanStarts.current[effect].forEach((start, lane) => {
        if (start !== null) onLivePass(effect, lane, null);
      });
    }
    spanStarts.current = noStarts();
    eraseStarts.current.fill(null);
    publishErasing(0);
    for (const effect of PLAYED_EFFECTS) publish(effect, 0);
  }, [onLivePass, publish, publishErasing]);

  /* Perdre le focus relâche tout. Un bouton tenu à la souris pendant qu'on
     change de fenêtre ne verra jamais son relâchement, et l'effet resterait
     ouvert sans rien pour le refermer.

     C'est le seul écouteur qui reste. La baie ne se ferme plus — elle fait
     partie de la console — donc il n'y a ni `Échap` ni démontage à couvrir. */
  useEffect(() => {
    const clear = () => abandon();
    window.addEventListener("blur", clear);
    return () => window.removeEventListener("blur", clear);
  }, [abandon]);

  return (
    <div className="analog-transport analog-fx-bay" role="group" aria-label="Mix Effects">
      <div className="analog-fx-grid">
        {Array.from({ length: TIMELINE_LANE_COUNT }, (_, lane) => {
          const erasing = (erasingLanes & (1 << lane)) !== 0;
          return (
            <div className="analog-fx-row" key={lane}>
              <span className="analog-fx-lane" aria-hidden="true">
                {laneLetter(lane)}
              </span>

              {PLAYED_EFFECTS.map((effect) => {
                const held = (heldLanes[effect] & (1 << lane)) !== 0;
                // Tenue à la main **ou** jouée depuis la timeline : dans les
                // deux cas l'effet est ouvert, et la pastille doit le montrer.
                // Mais `aria-pressed` reste sur le seul appui : un lecteur
                // d'écran doit dire si le bouton est enfoncé, pas si la piste
                // sonne.
                const lit = held || (playingLanes[effect] & (1 << lane)) !== 0;
                return (
                  <button
                    key={effect}
                    type="button"
                    className={`analog-fx-pad analog-fx-pad--${effect}${lit ? " is-held" : ""}`}
                    aria-pressed={held}
                    /* Le nom quitte la pastille pour l'infobulle : elle ne
                       porte plus qu'une marque, mais reste nommée pour qui la
                       lit autrement qu'à l'œil. */
                    aria-label={`${EFFECT_LABELS[effect]} on track ${laneLetter(lane)}`}
                    title={`Hold for ${EFFECT_LABELS[effect]} on track ${laneLetter(lane)}`}
                    /* `aria-disabled` et non `disabled` : un bouton désactivé
                       cesse de recevoir les évènements du pointeur, si bien
                       qu'une édition déclenchée pendant qu'on tient la
                       pastille lui volait son relâchement et laissait l'effet
                       ouvert. Le refus se fait dans la poignée, où il peut
                       laisser passer le lever de doigt. */
                    aria-disabled={busy}
                    /* Le pointeur est capturé : un doigt qui glisse hors du
                       bouton doit quand même le relâcher, sinon l'effet
                       resterait ouvert sur un geste rapide. */
                    onPointerDown={(event) => {
                      if (busy) return;
                      event.currentTarget.setPointerCapture(event.pointerId);
                      hold(effect, lane);
                    }}
                    onPointerUp={() => release(effect, lane)}
                    onPointerCancel={() => release(effect, lane)}
                  >
                    <TransportGlyph name={EFFECT_GLYPHS[effect]} />
                  </button>
                );
              })}

              {/* La gomme, à droite des effets qu'elle retire. */}
              <button
                type="button"
                className={`analog-fx-pad analog-fx-eraser${erasing ? " is-held" : ""}`}
                aria-pressed={erasing}
                aria-label={`Erase all effect automation on track ${laneLetter(lane)}`}
                title={`Hold to wipe every effect as it passes on track ${laneLetter(lane)}`}
                aria-disabled={busy}
                onPointerDown={(event) => {
                  if (busy) return;
                  event.currentTarget.setPointerCapture(event.pointerId);
                  holdEraser(lane);
                }}
                onPointerUp={() => releaseEraser(lane)}
                onPointerCancel={() => releaseEraser(lane)}
              >
                <TransportGlyph name="fx-eraser" />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
