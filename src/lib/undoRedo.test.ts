import { describe, expect, it } from "vitest";
import { UNDO_HISTORY_LIMIT, UndoRedoHistory } from "./undoRedo";

describe("UndoRedoHistory", () => {
  it("restores the state recorded before an edit, then replays it", () => {
    const history = new UndoRedoHistory<string>();
    expect(history.canUndo).toBe(false);
    expect(history.undo("A")).toBeNull();

    history.push("A");
    expect(history.canUndo).toBe(true);

    expect(history.undo("B")).toBe("A");
    expect(history.canUndo).toBe(false);
    expect(history.canRedo).toBe(true);

    expect(history.redo("A")).toBe("B");
    expect(history.canRedo).toBe(false);
    expect(history.canUndo).toBe(true);
  });

  it("walks back several steps without repeating the state it left", () => {
    const history = new UndoRedoHistory<string>();
    history.push("A");
    history.push("B");
    history.push("C");

    expect(history.undo("D")).toBe("C");
    expect(history.undo("C")).toBe("B");
    expect(history.undo("B")).toBe("A");
    expect(history.undo("A")).toBeNull();

    expect(history.redoCount).toBe(3);
    expect(history.redo("A")).toBe("B");
    expect(history.redo("B")).toBe("C");
    expect(history.redo("C")).toBe("D");
  });

  it("drops the redo branch as soon as a new edit is recorded", () => {
    const history = new UndoRedoHistory<string>();
    history.push("A");
    history.undo("B");
    expect(history.canRedo).toBe(true);

    history.push("A");
    expect(history.canRedo).toBe(false);
    expect(history.redo("X")).toBeNull();
  });

  it("keeps at most the configured number of steps", () => {
    const history = new UndoRedoHistory<number>();
    for (let index = 0; index < UNDO_HISTORY_LIMIT + 25; index += 1) {
      history.push(index);
    }
    expect(history.undoCount).toBe(UNDO_HISTORY_LIMIT);
    // The oldest steps are the ones dropped: the newest push is undone first.
    expect(history.undo(-1)).toBe(UNDO_HISTORY_LIMIT + 24);
  });

  it("clears both branches", () => {
    const history = new UndoRedoHistory<string>();
    history.push("A");
    history.undo("B");
    history.clear();
    expect(history.canUndo).toBe(false);
    expect(history.canRedo).toBe(false);
  });
});
