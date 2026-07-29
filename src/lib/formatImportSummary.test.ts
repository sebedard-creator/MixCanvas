import { describe, expect, it } from "vitest";

import {
  formatAnalysisProgress,
  formatAnalysisSummary,
  formatImportSummary,
} from "./formatImportSummary";

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

describe("formatAnalysisProgress", () => {
  it("names the track being worked on, not the one just finished", () => {
    expect(formatAnalysisProgress(0, 87)).toBe("Analyzing 1 of 87...");
    expect(formatAnalysisProgress(11, 87)).toBe("Analyzing 12 of 87...");
  });

  it("stops at the total on the last track", () => {
    // Le dernier événement annonce 87 sur 87 finies; sans borne l'affichage
    // aurait dit « 88 sur 87 » juste avant que le résumé le remplace.
    expect(formatAnalysisProgress(87, 87)).toBe("Analyzing 87 of 87...");
  });

  it("says nothing when there is nothing to analyse", () => {
    expect(formatAnalysisProgress(0, 0)).toBe("");
  });
});

describe("formatAnalysisSummary", () => {
  it("reports successes and failures", () => {
    expect(formatAnalysisSummary({ analyzedCount: 3, failedCount: 1 })).toBe(
      "3 tracks analyzed · 1 failure",
    );
  });
});
