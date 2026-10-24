/**
 * Unlimited undo/redo over immutable snapshots. Each user action commits one snapshot (a drag
 * commits once, when it ends); snapshots share every unchanged stroke object, so a step costs
 * one array of references.
 */

export interface History<T> {
  readonly past: readonly T[];
  readonly present: T;
  readonly future: readonly T[];
}

export function createHistory<T>(present: T): History<T> {
  return { past: [], present, future: [] };
}

/** A new undoable step; clears the redo list. Committing the present itself changes nothing. */
export function commit<T>(history: History<T>, next: T): History<T> {
  if (next === history.present) return history;
  return { past: [...history.past, history.present], present: next, future: [] };
}

/** Replaces the present without a new step (selection changes, for instance). */
export function replacePresent<T>(history: History<T>, next: T): History<T> {
  return next === history.present ? history : { ...history, present: next };
}

export function canUndo<T>(history: History<T>): boolean {
  return history.past.length > 0;
}

export function canRedo<T>(history: History<T>): boolean {
  return history.future.length > 0;
}

export function undo<T>(history: History<T>): History<T> {
  if (history.past.length === 0) return history;
  return {
    past: history.past.slice(0, -1),
    present: history.past[history.past.length - 1],
    future: [history.present, ...history.future],
  };
}

export function redo<T>(history: History<T>): History<T> {
  if (history.future.length === 0) return history;
  return {
    past: [...history.past, history.present],
    present: history.future[0],
    future: history.future.slice(1),
  };
}
