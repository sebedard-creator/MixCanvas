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

  /* Ce que l'application en fait : retirer les waveforms des clips avant de
     les empiler. Le crochet doit valoir aux **trois** entrées, sinon la moitié
     des états gardés resteraient lourds sans que rien ne le signale. */
  it("keeps only what the store hook returns, at every entrance", () => {
    const seen: string[] = [];
    const history = new UndoRedoHistory<{ tag: string; heavy: number[] | null }>(
      50,
      (state) => {
        seen.push(state.tag);
        return { ...state, heavy: null };
      },
    );

    history.push({ tag: "pushed", heavy: [1, 2, 3] });
    // `undo` bascule l'état courant vers la pile redo : deuxième entrée.
    const undone = history.undo({ tag: "undone", heavy: [4, 5, 6] });
    // `redo` le rebascule vers la pile undo : troisième entrée.
    const redone = history.redo({ tag: "redone", heavy: [7, 8, 9] });

    expect(undone).toEqual({ tag: "pushed", heavy: null });
    expect(redone).toEqual({ tag: "undone", heavy: null });
    expect(seen).toEqual(["pushed", "undone", "redone"]);
  });

  it("leaves states untouched when no hook is given", () => {
    const history = new UndoRedoHistory<{ heavy: number[] }>();
    const state = { heavy: [1, 2, 3] };
    history.push(state);
    expect(history.undo({ heavy: [] })).toBe(state);
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
