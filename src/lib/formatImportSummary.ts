import type { AnalysisBatchResult, LibraryImportResult } from "../library/types";

type ImportCounts = Pick<LibraryImportResult, "addedCount" | "duplicateCount" | "failedCount">;

export function formatImportSummary(counts: ImportCounts): string {
  const details: string[] = [];

  if (counts.addedCount > 0) {
    details.push(`${counts.addedCount} MP3 added`);
  }

  if (counts.duplicateCount > 0) {
    details.push(`${counts.duplicateCount} already present`);
  }

  if (counts.failedCount > 0) {
    details.push(`${counts.failedCount} unreadable file${counts.failedCount > 1 ? "s" : ""}`);
  }

  return details.length > 0 ? details.join(" · ") : "No new MP3 files found.";
}

type AnalysisCounts = Pick<AnalysisBatchResult, "analyzedCount" | "failedCount">;

export function formatAnalysisSummary(counts: AnalysisCounts): string {
  const details: string[] = [];

  if (counts.analyzedCount > 0) {
    details.push(`${counts.analyzedCount} track${counts.analyzedCount > 1 ? "s" : ""} analyzed`);
  }

  if (counts.failedCount > 0) {
    details.push(`${counts.failedCount} failure${counts.failedCount > 1 ? "s" : ""}`);
  }

  return details.length > 0 ? details.join(" · ") : "No tracks to analyze.";
}
