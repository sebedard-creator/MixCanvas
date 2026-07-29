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

/**
 * Où en est le lot, pendant qu'il tourne.
 *
 * Sur un dossier entier l'analyse dure des minutes; sans compte qui avance,
 * une interface immobile se lit comme une panne. Le numéro est celui de la
 * piste **en cours**, pas de la dernière finie : c'est ce qu'on cherche des
 * yeux quand on attend, et l'affichage doit arriver à `n / n`.
 */
export function formatAnalysisProgress(done: number, total: number): string {
  if (total <= 0) {
    return "";
  }
  const current = Math.min(done + 1, total);
  return `Analyzing ${current} of ${total}...`;
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
