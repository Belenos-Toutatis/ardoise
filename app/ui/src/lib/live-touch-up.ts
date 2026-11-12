/**
 * What the control window does with the selection (docs/design/tablet-live-classroom.md, "M12"):
 * delete it, recolour it, move or resize it, and undo or redo the last edit of the page (a delete, a change of colour,
 * « Clear the page »: never what the tablet wrote). Rust does the edit and every window gets a
 * `strokesEdited` event (`LiveState` applies it); this only asks, with the revision the selection was
 * chosen at, and looks after the selection afterwards. M14: the calls go through the window's queue
 * (`ComputerQueue`), after the computer's pen and eraser calls asked for before them: a Ctrl+Z pressed just
 * after the pen is lifted undoes that stroke. One edit at a time: while one is under way, a
 * second key press (or click) is ignored, because it would be refused as stale and say so in red.
 */
import type { api as Api } from './api';
import type { ComputerQueue } from './live-computer';
import type { LiveSelection } from './live-select.svelte';
import type { LiveState } from './live-state.svelte';
import type { Rgba } from './note';
import type { PageTransform } from './selection-grab';

/** The commands used (the real `api`, or a stand-in in tests). */
export type TouchUpCommands = Pick<typeof Api, 'liveEditStrokes' | 'liveUndoEdit' | 'liveRedoEdit'>;

export interface TouchUp {
  live: Pick<LiveState, 'pageIndex' | 'noteId' | 'revision' | 'strokes' | 'layers'>;
  selection: LiveSelection;
  commands: TouchUpCommands;
  /** The window's queue of computer commands (M14); absent: each call leaves at once. */
  queue?: ComputerQueue;
  /** An edit was refused (the error is translated by `errorMessage`). */
  onerror: (error: unknown) => void;
  /** An undo or a redo went through: the offer to undo the clearing is no longer true. */
  onhistory?: () => void;
}

function codeOf(error: unknown): unknown {
  return (error as { code?: unknown } | null)?.code;
}

/** The controls with an edit under way. */
const editing = new WeakSet<TouchUp>();

/** Runs `edit` unless another is under way for `touch`; what it answers, or undefined when it did not run. */
async function alone<T>(touch: TouchUp, edit: () => Promise<T>): Promise<T | undefined> {
  if (editing.has(touch)) return undefined;
  editing.add(touch);
  try {
    return await edit();
  } finally {
    editing.delete(touch);
  }
}

/** Runs `call` after the queue's earlier commands, if there is a queue. */
function queued<T>(touch: TouchUp, call: () => Promise<T>): Promise<T> {
  return touch.queue ? touch.queue.run(call) : call();
}

/** A refused edit: a page or a revision that moved leaves nothing to select any more. */
function refused(touch: TouchUp, error: unknown): void {
  if (codeOf(error) === 'liveEditStale') touch.selection.clear();
  touch.onerror(error);
}

/** Deletes the selected strokes; the selection is gone with them. */
export function deleteSelected(touch: TouchUp): Promise<void> {
  return alone(touch, async () => {
    const { selection } = touch;
    if (!selection.any) return;
    const indices = [...selection.indices];
    try {
      await queued(touch, () => touch.commands.liveEditStrokes(selection.pageIndex, selection.revision, { kind: 'delete', indices }));
      selection.clear();
    } catch (error) {
      refused(touch, error);
    }
  });
}

/** Gives the selected strokes `color`; the selection stays (the revision does not change). */
export function recolourSelected(touch: TouchUp, color: Rgba): Promise<void> {
  return alone(touch, async () => {
    const { selection } = touch;
    if (!selection.any) return;
    const indices = [...selection.indices];
    try {
      await queued(touch, () => touch.commands.liveEditStrokes(selection.pageIndex, selection.revision, { kind: 'recolour', indices, color }));
    } catch (error) {
      refused(touch, error);
    }
  });
}

/**
 * Moves or resizes the selected strokes (`transform`, SPEC §4.2): the selection stays, ready for another move or a
 * resize. The strokes keep their places in the list, but the revision moves on, and the call's answer and the
 * `strokesEdited` event may come in either order: the selection is held while the call is under way (the revision
 * the session reports meanwhile does not drop it) and then takes the revision the call returns; one ahead of the
 * session's waits for the event (`LiveSelection.follow`). True when the edit went through; false when it was
 * refused (a stale page clears the selection, the error is told) or another edit was under way.
 */
export async function transformSelected(touch: TouchUp, transform: PageTransform): Promise<boolean> {
  const done = await alone(touch, async () => {
    const { selection } = touch;
    if (!selection.any) return false;
    const indices = [...selection.indices];
    const { scale, x, y } = transform;
    selection.hold();
    let revision: number | undefined;
    try {
      revision = await queued(touch, () => touch.commands.liveEditStrokes(selection.pageIndex, selection.revision, { kind: 'transform', indices, scale, x, y }));
      return true;
    } catch (error) {
      refused(touch, error);
      return false;
    } finally {
      selection.release(revision);
      selection.follow(touch.live);
    }
  });
  return done === true;
}

/** Undoes (or, with `again`, redoes) the last edit of the page on screen. */
export function stepHistory(touch: TouchUp, again = false): Promise<void> {
  return alone(touch, async () => {
    try {
      const { pageIndex } = touch.live;
      const done = await queued(touch, () => (again ? touch.commands.liveRedoEdit(pageIndex) : touch.commands.liveUndoEdit(pageIndex)));
      if (done) touch.onhistory?.();
    } catch (error) {
      refused(touch, error);
    }
  });
}
