import { memo, useMemo } from "react";

import { waveformChannelPath, waveformRmsPath } from "../lib/waveformPath";
import { buildWaveformPyramid, selectWaveformLevel } from "../lib/waveformPyramid";
import { waveformWindow, windowBucketRange } from "../lib/waveformWindow";
import type { WaveformPeaks } from "../timeline/types";

interface ClipWaveformProps {
  waveform: WaveformPeaks | null;
  displayWidth: number;
  trimStartBeats?: number;
  trimEndBeats?: number;
  durationBeats?: number;
  /**
   * Où commence la fenêtre à l'écran, en pixels comptés depuis le début de la
   * partie dessinée du clip. Négatif quand le clip déborde à gauche.
   */
  visibleFromPx: number;
  /** La largeur visible, en pixels. */
  visibleWidthPx: number;
}

function sliceSeries(values: number[], from: number, to: number): number[] {
  return values.slice(Math.min(from, values.length), Math.min(to, values.length));
}

export const ClipWaveform = memo(function ClipWaveform({
  waveform,
  displayWidth,
  trimStartBeats = 0,
  trimEndBeats = 0,
  durationBeats,
  visibleFromPx,
  visibleWidthPx,
}: ClipWaveformProps) {
  const slicedWaveform = useMemo(() => {
    if (!waveform) return null;
    if (trimStartBeats === 0 && trimEndBeats === 0) return waveform;

    const totalBeats = trimStartBeats + (durationBeats ?? 0) + trimEndBeats;
    if (totalBeats <= 0) return waveform;

    const len = waveform.leftMin.length;
    const startIndex = Math.floor((trimStartBeats / totalBeats) * len);
    const endIndex = Math.min(len, Math.ceil(((trimStartBeats + (durationBeats ?? 0)) / totalBeats) * len));

    if (endIndex <= startIndex) return waveform;

    return {
      leftMin: waveform.leftMin.slice(startIndex, endIndex),
      leftMax: waveform.leftMax.slice(startIndex, endIndex),
      leftRms: waveform.leftRms.slice(startIndex, endIndex),
      rightMin: waveform.rightMin.slice(startIndex, endIndex),
      rightMax: waveform.rightMax.slice(startIndex, endIndex),
      rightRms: waveform.rightRms.slice(startIndex, endIndex),
    };
  }, [waveform, trimStartBeats, trimEndBeats, durationBeats]);

  const pyramid = useMemo(
    () => (slicedWaveform ? buildWaveformPyramid(slicedWaveform) : []),
    [slicedWaveform],
  );

  /**
   * La tranche à dessiner, et rien qu'elle.
   *
   * Le niveau était choisi sur la largeur **entière** du clip : un morceau de
   * six minutes au zoom ordinaire faisait seize mille colonnes et un million
   * six cent mille caractères de chemin, pour neuf cents pixels à l'écran.
   * Quatre-vingt-treize pour cent de ce travail ne se voyait jamais.
   *
   * La fenêtre est arrondie à un pas, donc elle ne bouge qu'une fois tous les
   * deux cent cinquante-six pixels de défilement : autrement on aurait troqué
   * un gros calcul rare contre un petit calcul à chaque image.
   */
  const window = useMemo(
    () => waveformWindow(displayWidth, visibleFromPx, visibleWidthPx),
    [displayWidth, visibleFromPx, visibleWidthPx],
  );

  const paths = useMemo(() => {
    if (!slicedWaveform || !window || pyramid.length === 0) return null;
    // Le niveau se choisit toujours sur la largeur **entière** du clip, et
    // c'est voulu : un niveau est indexé sur toute la durée du morceau, donc
    // c'est cette largeur-là qui dit combien de colonnes il faut pour avoir une
    // colonne par pixel. Le viser sur la tranche donnait quatre-vingt-sept
    // colonnes pour quatorze cents pixels au zoom serré — une waveform en
    // escalier.
    //
    // Le gain ne vient pas du niveau mais du **découpage** juste en dessous :
    // on ne construit le chemin que pour les colonnes visibles.
    const level = selectWaveformLevel(pyramid, displayWidth);
    if (!level) return null;

    const { from, to } = windowBucketRange(window, displayWidth, level.leftMin.length);
    const leftMin = sliceSeries(level.leftMin, from, to);
    if (leftMin.length === 0) return null;

    return {
      leftPeak: waveformChannelPath(leftMin, sliceSeries(level.leftMax, from, to), 24, 21),
      leftRms: waveformRmsPath(sliceSeries(level.leftRms, from, to), 24, 21),
      rightPeak: waveformChannelPath(
        sliceSeries(level.rightMin, from, to),
        sliceSeries(level.rightMax, from, to),
        76,
        21,
      ),
      rightRms: waveformRmsPath(sliceSeries(level.rightRms, from, to), 76, 21),
      width: Math.max(1, leftMin.length - 1),
    };
  }, [displayWidth, pyramid, slicedWaveform, window]);

  if (!paths || !window) {
    return <div className="clip-waveform clip-waveform--pending" aria-hidden="true" />;
  }

  return (
    <svg
      className="clip-waveform"
      viewBox={`0 0 ${paths.width} 100`}
      preserveAspectRatio="none"
      aria-hidden="true"
      /* Le dessin n'occupe plus toute la largeur du clip : il est posé à
         l'endroit de sa tranche, et suit la fenêtre par bonds d'un pas. */
      style={{ left: window.offsetPx, width: window.widthPx, right: "auto" }}
    >
      <line className="clip-waveform-zero" x1="0" x2={paths.width} y1="24" y2="24" />
      <line className="clip-waveform-zero" x1="0" x2={paths.width} y1="76" y2="76" />
      <path className="clip-waveform-peak" d={paths.leftPeak} />
      <path className="clip-waveform-peak" d={paths.rightPeak} />
      <path className="clip-waveform-rms" d={paths.leftRms} />
      <path className="clip-waveform-rms" d={paths.rightRms} />
    </svg>
  );
});
