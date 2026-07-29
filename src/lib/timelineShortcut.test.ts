import { describe, expect, it } from "vitest";

import {
  resolveSpaceTarget,
  shouldCaptureTimelineSpace,
  shouldCaptureTimelineZoom,
} from "./timelineShortcut";

describe("timeline Space shortcut", () => {
  it("captures Space while a button remains focused", () => {
    expect(shouldCaptureTimelineSpace("Space", "BUTTON")).toBe(true);
  });

  it("does not capture typing targets", () => {
    expect(shouldCaptureTimelineSpace("Space", "INPUT")).toBe(false);
    expect(shouldCaptureTimelineSpace("Space", "TEXTAREA")).toBe(false);
    expect(shouldCaptureTimelineSpace("Space", "DIV", true)).toBe(false);
  });

  it("ignores every key other than Space", () => {
    expect(shouldCaptureTimelineSpace("Enter", "BUTTON")).toBe(false);
  });

  it("drives the timeline when nothing is open on top of it", () => {
    expect(resolveSpaceTarget({ beatgridEditor: false, clipEq: false })).toBe("timeline");
  });

  it("drives the Beatgrid Editor's own preview while it is open", () => {
    // Starting the timeline releases the Preview output, so reaching the
    // timeline from behind the editor would cut off what it is auditioning.
    expect(resolveSpaceTarget({ beatgridEditor: true, clipEq: false })).toBe(
      "beatgrid-preview",
    );
    expect(resolveSpaceTarget({ beatgridEditor: true, clipEq: true })).toBe(
      "beatgrid-preview",
    );
  });

  it("starts nothing from behind the Clip EQ window", () => {
    expect(resolveSpaceTarget({ beatgridEditor: false, clipEq: true })).toBe("none");
  });

  it("captures R and T for zoom outside text entry fields", () => {
    expect(shouldCaptureTimelineZoom("KeyR", "DIV")).toBe(true);
    expect(shouldCaptureTimelineZoom("KeyT", "BUTTON")).toBe(true);
    expect(shouldCaptureTimelineZoom("KeyR", "INPUT")).toBe(false);
    expect(shouldCaptureTimelineZoom("KeyT", "DIV", true)).toBe(false);
  });
});
