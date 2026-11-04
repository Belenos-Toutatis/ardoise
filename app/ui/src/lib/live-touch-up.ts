/**
 * What the control window does with the selection (docs/design/tablet-live-classroom.md, "M12"):
 * delete it, recolour it, and undo or redo the last edit of the page (a delete, a change of colour,
 * « Clear the page »: never what the tablet wrote). Rust does the edit and every window gets a
 * `strokesEdited` event (`LiveState` applies it); this only asks, with the revision the selection was
 * chosen at, and looks after the selection afterwards.
 */
import type { api as Api } from './api';
import type { LiveSelection } from './live-select.svelte';
import type { LiveState } from './live-state.svelte';
import type { Rgba } from './note';

/** The commands used (the real `api`, or a stand-in in tests). */
export type TouchUpCommands = Pick<typeof Api, 'liveEditStrokes' | 'liveUndoEdit' | 'liveRedoEdit'>;

export interface TouchUp {
  live: Pick<LiveState, 'pageIndex'>;
  selection: LiveSelection;
  commands: TouchUpCommands;
  /** An edit was refused (the error is translated by `errorMessage`). */
  onerror: (error: unknown) => void;
  /** An undo or a redo went through: the offer to undo the clearing is no longer true. */
  onhistory?: () => void;
}

function codeOf(error: unknown): unknown {
  return (error as { code?: unknown } | null)?.code;
}

/** A refused edit: a page or a revision that moved leaves nothing to select any more. */
function refused(touch: TouchUp, error: unknown): void {
  if (codeOf(error) === 'liveEditStale') touch.selection.clear();
  touch.onerror(error);
}

/** Deletes the selected strokes; the selection is gone with them. */
export async function deleteSelected(touch: TouchUp): Promise<void> {
  const { selection } = touch;
  if (!selection.any) return;
  const indices = [...selection.indices];
  try {
    await touch.commands.liveEditStrokes(selection.pageIndex, selection.revision, { kind: 'delete', indices });
    selection.clear();
  } catch (error) {
    refused(touch, error);
  }
}

/** Gives the selected strokes `color`; the selection stays (the revision does not change). */
export async function recolourSelected(touch: TouchUp, color: Rgba): Promise<void> {
  const { selection } = touch;
  if (!selection.any) return;
  const indices = [...selection.indices];
  try {
    await touch.commands.liveEditStrokes(selection.pageIndex, selection.revision, { kind: 'recolour', indices, color });
  } catch (error) {
    refused(touch, error);
  }
}

/** Undoes (or, with `again`, redoes) the last edit of the page on screen. */
export async function stepHistory(touch: TouchUp, again = false): Promise<void> {
  try {
    const { pageIndex } = touch.live;
    const done = again ? await touch.commands.liveRedoEdit(pageIndex) : await touch.commands.liveUndoEdit(pageIndex);
    if (done) touch.onhistory?.();
  } catch (error) {
    refused(touch, error);
  }
}
