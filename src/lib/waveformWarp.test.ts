import { expect, it } from "vitest";
import { warpWaveform } from "./waveformWarp";

it("preserves peaks and RMS while warping source time onto the musical grid", () => {
  const waveform = { leftMin: [-1, -.5, -.25, 0], leftMax: [1, .5, .25, 0], leftRms: [1, .5, .25, 0],
    rightMin: [-1, -.5, -.25, 0], rightMax: [1, .5, .25, 0], rightRms: [1, .5, .25, 0] };
  expect(warpWaveform(waveform, (progress) => progress)).toEqual(waveform);
  const warped = warpWaveform(waveform, (progress) => progress ** 2);
  expect(warped.leftMax).toEqual([1, 1, .5, .25]);
  expect(warped.leftMin).toEqual([-1, -1, -.5, -.25]);
  expect(warped.leftRms.every(Number.isFinite)).toBe(true);
});
