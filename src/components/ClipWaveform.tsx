import { memo, useMemo } from "react";

import { waveformChannelPath, waveformRmsPath } from "../lib/waveformPath";
import { buildWaveformPyramid, selectWaveformLevel } from "../lib/waveformPyramid";
import type { WaveformPeaks } from "../timeline/types";

interface ClipWaveformProps {
  waveform: WaveformPeaks | null;
  displayWidth: number;
  trimStartBeats?: number;
  trimEndBeats?: number;
  durationBeats?: number;
}

export const ClipWaveform = memo(function ClipWaveform({
  waveform,
  displayWidth,
  trimStartBeats = 0,
  trimEndBeats = 0,
  durationBeats,
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
  const level = useMemo(
    () => selectWaveformLevel(pyramid, displayWidth),
    [displayWidth, pyramid],
  );
  const paths = useMemo(() => {
    if (!level) {
      return null;
    }

    return {
      leftPeak: waveformChannelPath(level.leftMin, level.leftMax, 24, 21),
      leftRms: waveformRmsPath(level.leftRms, 24, 21),
      rightPeak: waveformChannelPath(level.rightMin, level.rightMax, 76, 21),
      rightRms: waveformRmsPath(level.rightRms, 76, 21),
      width: Math.max(1, level.leftMin.length - 1),
    };
  }, [level]);

  if (!paths) {
    return <div className="clip-waveform clip-waveform--pending" aria-hidden="true" />;
  }

  return (
    <svg
      className="clip-waveform"
      viewBox={`0 0 ${paths.width} 100`}
      preserveAspectRatio="none"
      aria-hidden="true"
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
