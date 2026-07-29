export function waveformChannelPath(
  minimum: number[],
  maximum: number[],
  centerY: number,
  amplitude: number,
): string {
  const count = Math.min(minimum.length, maximum.length);
  if (count === 0) {
    return "";
  }

  const point = (index: number, value: number) => {
    const finiteValue = Number.isFinite(value) ? value : 0;
    const clampedValue = Math.max(-1, Math.min(1, finiteValue));
    return `${index},${(centerY - clampedValue * amplitude).toFixed(2)}`;
  };
  const upper = Array.from({ length: count }, (_, index) => point(index, maximum[index]));
  const lower = Array.from(
    { length: count },
    (_, offset) => {
      const index = count - 1 - offset;
      return point(index, minimum[index]);
    },
  );

  return `M${upper.join(" L")} L${lower.join(" L")} Z`;
}

export function waveformRmsPath(
  rms: number[],
  centerY: number,
  amplitude: number,
): string {
  if (rms.length === 0) {
    return "";
  }
  const magnitude = (value: number) => Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
  const upper = rms.map(
    (value, index) => `${index},${(centerY - magnitude(value) * amplitude).toFixed(2)}`,
  );
  const lower = Array.from({ length: rms.length }, (_, offset) => {
    const index = rms.length - 1 - offset;
    return `${index},${(centerY + magnitude(rms[index]) * amplitude).toFixed(2)}`;
  });
  return `M${upper.join(" L")} L${lower.join(" L")} Z`;
}
