import { memo, useLayoutEffect, useMemo, useRef } from "react";

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

interface WaveformRaster {
  leftMin: number[];
  leftMax: number[];
  leftRms: number[];
  rightMin: number[];
  rightMax: number[];
  rightRms: number[];
  width: number;
}

function drawEnvelope(
  context: CanvasRenderingContext2D,
  minimum: readonly number[],
  maximum: readonly number[],
  centerY: number,
  amplitude: number,
  scaleX: number,
) {
  const count = Math.min(minimum.length, maximum.length);
  if (count === 0) return;
  const valueY = (value: number) => {
    const finite = Number.isFinite(value) ? value : 0;
    return centerY - Math.max(-1, Math.min(1, finite)) * amplitude;
  };
  context.beginPath();
  context.moveTo(0, valueY(maximum[0]));
  for (let index = 1; index < count; index += 1) {
    context.lineTo(index * scaleX, valueY(maximum[index]));
  }
  for (let index = count - 1; index >= 0; index -= 1) {
    context.lineTo(index * scaleX, valueY(minimum[index]));
  }
  context.closePath();
  context.fill();
  context.stroke();
}

function drawRms(
  context: CanvasRenderingContext2D,
  rms: readonly number[],
  centerY: number,
  amplitude: number,
  scaleX: number,
) {
  if (rms.length === 0) return;
  const magnitude = (value: number) => (
    Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0)) * amplitude
  );
  context.beginPath();
  context.moveTo(0, centerY - magnitude(rms[0]));
  for (let index = 1; index < rms.length; index += 1) {
    context.lineTo(index * scaleX, centerY - magnitude(rms[index]));
  }
  for (let index = rms.length - 1; index >= 0; index -= 1) {
    context.lineTo(index * scaleX, centerY + magnitude(rms[index]));
  }
  context.closePath();
  context.fill();
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
  const canvas = useRef<HTMLCanvasElement | null>(null);
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

  const level = useMemo(
    () => selectWaveformLevel(pyramid, displayWidth),
    [displayWidth, pyramid],
  );
  const bucketWindow = level && window
    ? windowBucketRange(window, displayWidth, level.leftMin.length)
    : null;
  const bucketFrom = bucketWindow?.from ?? -1;
  const bucketTo = bucketWindow?.to ?? -1;

  const raster = useMemo<WaveformRaster | null>(() => {
    if (!level || bucketFrom < 0 || bucketTo <= bucketFrom) return null;
    // Le niveau se choisit toujours sur la largeur **entière** du clip, et
    // c'est voulu : un niveau est indexé sur toute la durée du morceau, donc
    // c'est cette largeur-là qui dit combien de colonnes il faut pour avoir une
    // colonne par pixel. Le viser sur la tranche donnait quatre-vingt-sept
    // colonnes pour quatorze cents pixels au zoom serré — une waveform en
    // escalier.
    //
    // Le gain ne vient pas du niveau mais du **découpage** juste en dessous :
    // on ne construit le chemin que pour les colonnes visibles.
    const leftMin = sliceSeries(level.leftMin, bucketFrom, bucketTo);
    if (leftMin.length === 0) return null;

    return {
      leftMin,
      leftMax: sliceSeries(level.leftMax, bucketFrom, bucketTo),
      leftRms: sliceSeries(level.leftRms, bucketFrom, bucketTo),
      rightMin: sliceSeries(level.rightMin, bucketFrom, bucketTo),
      rightMax: sliceSeries(level.rightMax, bucketFrom, bucketTo),
      rightRms: sliceSeries(level.rightRms, bucketFrom, bucketTo),
      width: Math.max(1, leftMin.length),
    };
  }, [bucketFrom, bucketTo, level]);

  /**
   * La largeur du bitmap, en pixels réellement affichés.
   *
   * Elle valait le **nombre de colonnes**, ce qui allait tant qu'il y en avait
   * une par pixel — le cas d'un clip entier, puisque le niveau de pyramide est
   * choisi pour ça. Une courte tranche casse l'équivalence : quatre temps pris
   * dans un morceau de six minutes ne gardent que quatre-vingt-onze colonnes,
   * étirées par le CSS sur cent soixante pixels. Le navigateur ré-échantillonne
   * alors l'image, et comme le contenu défile par fractions de pixel pendant la
   * lecture, la phase change à chaque trame : la waveform scintille. Plusieurs
   * tours de boucle côte à côte, chacun à sa propre fraction, et ça se voit
   * comme une danse.
   *
   * Le bitmap fait donc exactement la taille à laquelle il sera posé, et le
   * dessin s'y étale lui-même.
   */
  const pixelWidth = Math.max(1, Math.round(window?.widthPx ?? 0));

  useLayoutEffect(() => {
    const element = canvas.current;
    if (!element || !raster) return;
    const context = element.getContext("2d", { alpha: true });
    if (!context) return;
    context.clearRect(0, 0, pixelWidth, 100);
    // Une colonne occupe cette largeur. Le trait, lui, garde la sienne : c'est
    // pourquoi l'échelle est portée par les coordonnées et non par une
    // transformation du contexte, qui aurait épaissi les verticales.
    const scaleX = raster.width > 1 ? (pixelWidth - 1) / (raster.width - 1) : 1;

    context.strokeStyle = "#000000";
    context.lineWidth = 1.5;
    context.fillStyle = "rgba(0, 0, 0, 0.18)";
    drawEnvelope(context, raster.leftMin, raster.leftMax, 24, 21, scaleX);
    drawEnvelope(context, raster.rightMin, raster.rightMax, 76, 21, scaleX);

    context.fillStyle = "#000000";
    drawRms(context, raster.leftRms, 24, 21, scaleX);
    drawRms(context, raster.rightRms, 76, 21, scaleX);

    context.strokeStyle = "rgba(0, 0, 0, 0.4)";
    context.lineWidth = 1;
    context.beginPath();
    context.moveTo(0, 24.5);
    context.lineTo(pixelWidth, 24.5);
    context.moveTo(0, 76.5);
    context.lineTo(pixelWidth, 76.5);
    context.stroke();
  }, [pixelWidth, raster]);

  if (!raster || !window) {
    return <div className="clip-waveform clip-waveform--pending" aria-hidden="true" />;
  }

  return (
    <canvas
      ref={canvas}
      className="clip-waveform"
      width={pixelWidth}
      height={100}
      aria-hidden="true"
      /* The bitmap is repainted only when its source bucket range changes.
         Ordinary zoom steps resize this cached DAW-style image instead of
         asking SVG to tessellate four long paths again. */
      style={{ left: window.offsetPx, width: pixelWidth, right: "auto" }}
    />
  );
});
