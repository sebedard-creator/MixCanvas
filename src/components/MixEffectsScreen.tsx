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
import { BEATS_PER_MEASURE } from "../lib/timelineSnap";
import { TransportGlyph } from "./TransportGlyph";

interface MixEffectsScreenProps {
  isOpen: boolean;
  onClose: () => void;
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
  /** Le transport, pour rejouer une passe sans quitter l'écran. */
  transportStatus: "paused" | "playing";
  onTogglePlayback: () => void;
  onSeek: (beat: number) => void;
  /** Le défilement rapide, tant que `FFWD` est tenu. */
  onScrubForward: (scrubbing: boolean) => void;
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
  canPlay: boolean;
  busy: boolean;
}

/**
 * Le panneau n'a **plus aucun raccourci**, et c'est un retrait volontaire.
 *
 * Il en portait quinze — quatre effets et une gomme sur trois pistes — qui
 * réclamaient chacun une lettre. Elles empiétaient sur celles de la timeline,
 * ce qui a demandé de museler tous ses raccourcis pendant que le panneau était
 * ouvert, puis d'ignorer les frappes modifiées parce que `Ctrl+Z` jouait la
 * reverb, puis de reprendre la barre d'espace qu'on venait de perdre. Trois
 * correctifs pour un confort que personne n'utilisait.
 *
 * Ce qu'on y perd est réel et vaut d'être dit : à la souris, on ne tient qu'un
 * bouton à la fois. Tenir deux effets ensemble demandait le clavier. Le
 * compromis a été tranché du côté de l'usage réel.
 *
 * `Échap` reste : fermer une fenêtre par `Échap` n'appartient à personne
 * d'autre, et rien dans le programme ne le réclame.
 */

/**
 * La marque de chaque effet.
 *
 * Une image plutôt qu'un nom : sur une pastille carrée, une marque figurative
 * se reconnaît d'un coup d'œil là où trois lettres demandent d'être lues — et
 * l'œil est occupé ailleurs, sur la piste qui défile.
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

export function MixEffectsScreen({
  isOpen,
  onClose,
  onSetKeys,
  onSetErasing,
  transportStatus,
  onTogglePlayback,
  onSeek,
  onScrubForward,
  onLivePass,
  onWriteSpan,
  onEraseSpan,
  nodesByEffect,
  liveTransport,
  canPlay,
  busy,
}: MixEffectsScreenProps) {
  /**
   * Quelles pistes sont tenues, par effet, ici et maintenant.
   *
   * Gardé aussi dans une référence : les écouteurs de clavier et de pointeur
   * vivent hors du rendu et doivent lire l'état courant, pas celui qu'ils ont
   * capturé en s'installant.
   */
  const [heldLanes, setHeldLanes] = useState(noLanes);
  const heldRef = useRef(noLanes());
  /* Il y avait ici une seconde marque : les pistes qui avaient **déjà reçu**
     chaque effet depuis l'ouverture du programme, signalées par un point de la
     couleur de l'effet. Elle est retirée.

     Une pastille ne doit porter qu'un seul message, et c'est celui-ci :
     **l'effet sonne en ce moment sur cette piste**. Un point qui reste allumé
     une fois la passe jouée disait autre chose au même endroit — « tu as déjà
     utilisé ceci » — et l'œil ne fait pas le tri entre deux signaux posés sur
     le même bouton. La règle demandée est nette : la pastille et la teinte de
     la timeline doivent dire la même chose au même instant. */

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
   * seulement quand un doigt appuie : sans cela l'écran ment sur ce qu'on
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

       C'était le défaut signalé. Le transport n'est interrogé que pendant la
       lecture, donc le dernier masque publié survivait à la pause : mettre en
       pause à l'intérieur d'une passe laissait sa pastille allumée pour de
       bon, et comme la reverb est celle qu'on joue en premier, elle semblait
       coincée sur « on ». Rien ne pouvait plus l'éteindre, puisque plus aucun
       instantané n'arrivait pour la contredire. */
    if (!isOpen || transportStatus !== "playing") {
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
  }, [isOpen, liveTransport, transportStatus]);

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

  /* Fermer l'écran relâche tout. Un bouton resté enfoncé parce qu'on a quitté
     la fenêtre laisserait l'effet ouvert sans rien pour le refermer. */
  useEffect(() => {
    if (isOpen) return;
    abandon();
    onScrubForward(false);
  }, [abandon, isOpen, onScrubForward]);

  useEffect(() => {
    if (!isOpen) return undefined;

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };

    /* Perdre le focus relâche tout. Un bouton tenu à la souris pendant qu'on
       change de fenêtre ne verra jamais son relâchement, et l'effet resterait
       ouvert sans rien pour le refermer. */
    const clear = () => abandon();

    window.addEventListener("keydown", closeOnEscape);
    window.addEventListener("blur", clear);
    return () => {
      window.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("blur", clear);
    };
  }, [abandon, isOpen, onClose]);

  if (!isOpen) return null;

  return (
    /* Un panneau posé **sur** la timeline, et non un écran qui la remplace.
       Le geste consiste à jouer un effet en regardant la piste défiler : tout
       ce que ce panneau cache est du travail qu'on ne voit pas. D'où le fond
       transparent, l'absence de texte, et des pastilles carrées à la taille
       des touches du transport plutôt qu'un rail de noms à lire.

       `aria-modal` est retiré avec le fond : la fenêtre n'est plus modale au
       sens visuel, et l'annoncer comme telle mentirait à un lecteur d'écran.
       Le clavier, lui, reste bien à ce panneau — ses touches sont des lettres
       que le reste du programme emploie aussi. */
    <div className="mix-effects-overlay">
      <div className="mix-effects-screen" role="dialog" aria-label="Mix Effects">
        <div className="mix-effects-header">
          <span className="mix-effects-badge">
            <span className="mix-effects-badge-dot" />
            MIX FX
          </span>
          <button
            className="mix-effects-close"
            type="button"
            onClick={onClose}
            aria-label="Close Mix Effects"
            title="Close · Esc"
          >
            ✕
          </button>
        </div>

        <div className="mix-effects-lanes">
          {Array.from({ length: TIMELINE_LANE_COUNT }, (_, lane) => {
            const erasing = (erasingLanes & (1 << lane)) !== 0;
            return (
              <div className="mix-effects-lane" key={lane}>
                <span className="mix-effects-lane-name">{laneLetter(lane)}</span>

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
                      className={`mix-effects-pad mix-effects-pad--${effect}${lit ? " is-held" : ""}`}
                      aria-pressed={held}
                      /* Le nom quitte la pastille pour l'étiquette et
                         l'infobulle : elle ne porte plus qu'une marque, mais
                         reste nommée pour qui la lit autrement qu'à l'œil. */
                      aria-label={`${EFFECT_LABELS[effect]} on track ${laneLetter(lane)}`}
                      title={`Hold for ${EFFECT_LABELS[effect]}`}
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
                  className={`mix-effects-pad mix-effects-eraser${erasing ? " is-held" : ""}`}
                  aria-pressed={erasing}
                  aria-label={`Erase all effect automation on track ${laneLetter(lane)}`}
                  title="Hold to wipe every effect as it passes"
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

        {/* Le transport, sur place. Rejouer une passe qu'on vient de rater est
            le geste le plus fréquent ici, et sortir du panneau pour le faire
            casserait le fil. */}
        <div className="mix-effects-transport" role="group" aria-label="Transport">
          <button
            type="button"
            className="mix-effects-pad mix-effects-transport-button"
            disabled={busy}
            onClick={() => onSeek(Math.max(0, liveTransport.read().positionBeat - BEATS_PER_MEASURE))}
            aria-label="Back one bar"
            title="Back one bar"
          >
            <TransportGlyph name="rewind" />
          </button>
          <button
            type="button"
            className={`mix-effects-pad mix-effects-transport-button${transportStatus === "playing" ? " is-playing" : ""}`}
            disabled={busy || !canPlay}
            onClick={onTogglePlayback}
            aria-label={transportStatus === "playing" ? "Pause" : "Play"}
            title="Play / Pause"
          >
            <TransportGlyph name={transportStatus === "playing" ? "pause" : "play"} />
          </button>
          <button
            type="button"
            className="mix-effects-pad mix-effects-transport-button"
            disabled={busy}
            onClick={() => onSeek(liveTransport.read().positionBeat + BEATS_PER_MEASURE)}
            onPointerDown={(event) => {
              event.currentTarget.setPointerCapture(event.pointerId);
              onScrubForward(true);
            }}
            onPointerUp={() => onScrubForward(false)}
            onPointerCancel={() => onScrubForward(false)}
            onPointerLeave={() => onScrubForward(false)}
            aria-label="Forward one bar, hold for double speed"
            title="Click: forward one bar · Hold: 2x"
          >
            <TransportGlyph name="forward" />
          </button>
        </div>
      </div>
    </div>
  );
}
