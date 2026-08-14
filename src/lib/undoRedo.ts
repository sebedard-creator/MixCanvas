export const UNDO_HISTORY_LIMIT = 50;

/**
 * Bounded undo/redo history.
 *
 * A snapshot is only recorded once the operation that produced it succeeded,
 * so a rejected edit never leaves a step that undoes to the current state.
 * The caller passes the state it is leaving to `undo`/`redo`, which keeps the
 * two stacks consistent even when several steps are taken back to back.
 *
 * `store` décide **ce qui est gardé** d'un état, et s'applique au seul endroit
 * où un état entre dans une pile. Un appelant qui allégerait ses états
 * lui-même devrait le faire aux trois entrées — `push`, et les deux bascules
 * de `undo` et `redo` — et une liste tenue à la main finit toujours par
 * s'écarter de ce qu'elle décrit.
 */
export class UndoRedoHistory<T> {
  private undoStack: T[] = [];
  private redoStack: T[] = [];

  constructor(
    private readonly limit: number = UNDO_HISTORY_LIMIT,
    private readonly store: (state: T) => T = (state) => state,
  ) {}

  /** Records the state that preceded a successful edit. */
  push(previousState: T): void {
    this.undoStack = [this.store(previousState), ...this.undoStack].slice(0, this.limit);
    this.redoStack = [];
  }

  /** Returns the state to restore, or null when there is nothing to undo. */
  undo(currentState: T): T | null {
    const previous = this.undoStack.shift();
    if (previous === undefined) {
      return null;
    }
    this.redoStack = [this.store(currentState), ...this.redoStack].slice(0, this.limit);
    return previous;
  }

  /** Returns the state to restore, or null when there is nothing to redo. */
  redo(currentState: T): T | null {
    const next = this.redoStack.shift();
    if (next === undefined) {
      return null;
    }
    this.undoStack = [this.store(currentState), ...this.undoStack].slice(0, this.limit);
    return next;
  }

  clear(): void {
    this.undoStack = [];
    this.redoStack = [];
  }

  get canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  get canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  get undoCount(): number {
    return this.undoStack.length;
  }

  get redoCount(): number {
    return this.redoStack.length;
  }
}
