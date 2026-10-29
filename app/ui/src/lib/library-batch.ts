/**
 * Operations on several notes of the library, one note after the other (milestone M10): a
 * failure is recorded with its translated message and does not stop the others, and the run can
 * be stopped between two notes. Plain functions, tested without the interface.
 */
import { api, errorMessage } from './api';
import { rotateQuarter } from './editor/ops';
import type { FileStamp } from './note';

export type TurnDirection = 'cw' | 'ccw';

/** A note to work on: its path, and the title shown if it fails. */
export interface BatchTarget {
  path: string;
  title: string;
}

/** A note the operation could not handle, with the translated reason. */
export interface BatchFailure extends BatchTarget {
  message: string;
}

export interface BatchOutcome<R> {
  /** The notes handled, in order, with what the work returned for each. */
  done: (BatchTarget & { result: R })[];
  failed: BatchFailure[];
  /** True when the run was stopped before the last note. */
  stopped: boolean;
}

export interface BatchHooks {
  /** Called before the first note and after each one. */
  onprogress?: (done: number, total: number) => void;
  /** Asked before each note: true stops the run there. */
  shouldStop?: () => boolean;
}

/** Runs `work` on each target in turn, collecting results and failures. */
export async function runBatch<R>(
  targets: readonly BatchTarget[],
  work: (target: BatchTarget) => Promise<R>,
  hooks: BatchHooks = {},
): Promise<BatchOutcome<R>> {
  const outcome: BatchOutcome<R> = { done: [], failed: [], stopped: false };
  hooks.onprogress?.(0, targets.length);
  for (const [index, target] of targets.entries()) {
    if (hooks.shouldStop?.()) {
      outcome.stopped = true;
      break;
    }
    try {
      outcome.done.push({ ...target, result: await work(target) });
    } catch (error) {
      outcome.failed.push({ ...target, message: errorMessage(error) });
    }
    hooks.onprogress?.(index + 1, targets.length);
  }
  return outcome;
}

export function sameStamp(a: FileStamp, b: FileStamp): boolean {
  return (
    a.len === b.len &&
    a.modified.secs_since_epoch === b.modified.secs_since_epoch &&
    a.modified.nanos_since_epoch === b.modified.nanos_since_epoch
  );
}

export function opposite(direction: TurnDirection): TurnDirection {
  return direction === 'cw' ? 'ccw' : 'cw';
}

/**
 * Turns the note at `path` a quarter turn exactly as the editor's « Tourner la feuille avec
 * l'écriture » does (the same `rotateQuarter`: page, ink and background image together), then
 * saves it with the stamp check: a note changed between the reading and the writing is refused
 * (`noteConflict`) and left as it was. With `expected`, a note that is no longer that version (it
 * changed since an earlier turn, which is being undone) is refused the same way. Returns the new
 * stamp.
 */
export async function turnNote(path: string, direction: TurnDirection, expected?: FileStamp): Promise<FileStamp> {
  const opened = await api.openNote(path);
  if (expected && !sameStamp(opened.stamp, expected)) throw { code: 'noteConflict', message: path };
  return api.saveNote(path, rotateQuarter(opened.note, direction), opened.stamp);
}
