import { describe, expect, it } from "vitest";

import { formatAnalysisSummary, formatImportSummary } from "./formatImportSummary";

describe("formatImportSummary", () => {
  it("describes a mixed import", () => {
    expect(formatImportSummary({ addedCount: 4, duplicateCount: 2, failedCount: 1 })).toBe(
      "4 MP3 added · 2 already present · 1 unreadable file",
    );
  });

  it("describes an empty folder", () => {
    expect(formatImportSummary({ addedCount: 0, duplicateCount: 0, failedCount: 0 })).toBe(
      "No new MP3 files found.",
    );
  });
});

describe("formatAnalysisSummary", () => {
  it("reports successes and failures", () => {
    expect(formatAnalysisSummary({ analyzedCount: 3, failedCount: 1 })).toBe(
      "3 tracks analyzed · 1 failure",
    );
  });
});
