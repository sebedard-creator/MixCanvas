/**
 * La position de lecture, hors du cycle de rendu de React.
 *
 * Le transport est interrogé vingt fois par seconde. Tant que sa position
 * vivait dans un `useState`, chacune de ces réponses re-rendait l'application
 * entière — mesuré sur cinq secondes de lecture : trente-huit pour cent du fil
 * principal passés dans du script, pour déplacer deux éléments.
 *
 * Rien n'oblige une valeur qui change vingt fois par seconde à passer par un
 * rendu. Ce qui la lit peut s'y abonner et écrire directement dans le DOM ; le
 * rendu, lui, ne reste nécessaire que lorsque la vue a bougé assez pour qu'il
 * y ait de nouveaux marqueurs ou de nouvelles waveforms à construire.
 *
 * La valeur reste également **lisible à tout instant** : une action déclenchée
 * au clavier veut la position d'maintenant, pas celle du dernier rendu. C'est
 * ce que `read()` donne, et c'est plus juste que ce que l'état React donnait.
 */

import type { TimelineTransportSnapshot } from "../timeline/types";

type Listener = (snapshot: TimelineTransportSnapshot) => void;

export interface LiveTransport {
  /** L'état le plus récent, quel que soit le moment du cycle de rendu. */
  read(): TimelineTransportSnapshot;
  /** Publie un nouvel état et prévient les abonnés, sans passer par React. */
  publish(snapshot: TimelineTransportSnapshot): void;
  /** S'abonne ; la fonction rendue se désabonne. */
  subscribe(listener: Listener): () => void;
}

export function createLiveTransport(initial: TimelineTransportSnapshot): LiveTransport {
  let current = initial;
  const listeners = new Set<Listener>();

  return {
    read: () => current,
    publish(snapshot) {
      current = snapshot;
      // Une copie : un abonné qui se désabonne en réagissant ne doit pas
      // faire sauter le suivant.
      for (const listener of [...listeners]) {
        listener(snapshot);
      }
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
