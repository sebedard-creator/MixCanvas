import { describe, expect, it } from "vitest";

import { formatDuration } from "./formatDuration";

describe("formatDuration", () => {
  it("formats a regular track position", () => {
    expect(formatDuration(125_900)).toBe("02:05");
  });

  it("includes hours for long mixes", () => {
    expect(formatDuration(3_725_000)).toBe("1:02:05");
  });

  it("handles invalid and negative values", () => {
    expect(formatDuration(Number.NaN)).toBe("00:00");
    expect(formatDuration(-500)).toBe("00:00");
  });
});
