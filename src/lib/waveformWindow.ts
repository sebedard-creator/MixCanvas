/**
 * Quelle part d'un clip vaut la peine d'être dessinée.
 *
 * La waveform était construite pour **toute** la largeur du clip. Mesuré sur un
 * morceau de six minutes au zoom ordinaire : seize mille colonnes, un million
 * six cent mille caractères de chemin, vingt-six millisecondes par clip et par
 * cran de zoom — pour neuf cents pixels réellement à l'écran. Quatre-vingt-
 * treize pour cent de ce travail ne se voit jamais.
 *
 * Un écran ne montre qu'un nombre fini de colonnes : au-delà, on ne gagne pas
 * du détail, on gagne du travail. Ce module dit quelle tranche du clip est
 * visible, et combien de colonnes elle mérite.
 */

/** La tranche d'un clip à dessiner, en pixels locaux au clip. */
export interface WaveformWindow {
  /** Où commence la tranche dans le clip. */
  offsetPx: number;
  /** Sa largeur. */
  widthPx: number;
}

/**
 * De combien la fenêtre avance d'un coup.
 *
 * Sans ce pas, la tranche changerait à chaque pixel de défilement — donc à
 * chaque image pendant la lecture, et l'on aurait remplacé un gros calcul rare
 * par un petit calcul permanent. Deux cent cinquante-six pixels : assez large
 * pour que la lecture n'en franchisse qu'un toutes les quelques secondes au
 * zoom courant, assez fin pour que la marge reste modeste.
 */
export const WAVEFORM_WINDOW_QUANTUM_PX = 256;

/**
 * La tranche visible d'un clip, arrondie à un pas et débordée d'une marge.
 *
 * `visibleFromPx` est en coordonnées **locales au clip** : négatif quand le
 * clip commence avant le bord gauche de la fenêtre.
 *
 * Renvoie `null` quand le clip est entièrement hors champ — il n'a alors aucune
 * géométrie à produire, ce qui est le second gain après la réduction.
 */
export function waveformWindow(
  clipWidthPx: number,
  visibleFromPx: number,
  visibleWidthPx: number,
  quantumPx: number = WAVEFORM_WINDOW_QUANTUM_PX,
): WaveformWindow | null {
  const usable =
    Number.isFinite(clipWidthPx)
    && Number.isFinite(visibleFromPx)
    && Number.isFinite(visibleWidthPx)
    && clipWidthPx > 0
    && visibleWidthPx > 0;
  if (!usable) return null;

  const quantum = Math.max(1, Math.floor(quantumPx));
  // Une marge d'un pas de chaque côté : la tranche est prête avant d'être
  // demandée, donc franchir une frontière ne montre jamais de vide.
  const wantedFrom = visibleFromPx - quantum;
  const wantedTo = visibleFromPx + visibleWidthPx + quantum;
  if (wantedTo <= 0 || wantedFrom >= clipWidthPx) return null;

  // Un bloc de largeur fixe qui glisse, plutôt que deux bords qui s'alignent
  // chacun pour soi : ceux-ci franchissaient la grille à des moments
  // différents, et la tranche changeait donc **deux** fois par pas au lieu
  // d'une. La largeur ne varie plus qu'au bout du clip, là où elle se coupe.
  const spanQuanta = Math.ceil((wantedTo - wantedFrom) / quantum);
  const offsetPx = Math.max(0, Math.floor(wantedFrom / quantum) * quantum);
  const widthPx = Math.min(clipWidthPx - offsetPx, spanQuanta * quantum);
  if (!(widthPx > 0)) return null;

  return { offsetPx, widthPx };
}

/**
 * Les bornes de cette tranche dans une série de `bucketCount` échantillons.
 *
 * Les deux bornes sont élargies vers l'extérieur : une colonne à cheval doit
 * être dessinée entièrement, faute de quoi le bord de la tranche laisse
 * apparaître une marche d'un pixel à chaque frontière.
 */
export function windowBucketRange(
  window: WaveformWindow,
  clipWidthPx: number,
  bucketCount: number,
): { from: number; to: number } {
  if (!(clipWidthPx > 0) || bucketCount <= 0) {
    return { from: 0, to: bucketCount };
  }
  const from = Math.max(0, Math.floor((window.offsetPx / clipWidthPx) * bucketCount));
  const to = Math.min(
    bucketCount,
    Math.ceil(((window.offsetPx + window.widthPx) / clipWidthPx) * bucketCount),
  );
  return { from, to: Math.max(from + 1, to) };
}
