/**
 * The pure parts of undoing a move of notes to another binder (`LibraryState.undoMove`, `docs/design/interface.md`,
 * "Undoing in the library"): what a note was and where it stood in its binder's free order, and where it goes back
 * in that order. A move does not touch the order files (the note leaves the binder it came from, which keeps its id,
 * and lands among the notes of the target that the order does not list: after the listed ones); so the place the
 * note had is, as a rule, still written in the source binder's file when the undo comes, and the undo has nothing to
 * write. When something dropped the id from the file meanwhile (a rearrangement of that binder), the place is found
 * again from the neighbours the note had, as long as the other notes were not rearranged; otherwise the note goes
 * to the end, and the undo says so.
 */
import { inBinderOrder } from './library-logic';
import type { FileStamp, NoteSummary } from './note';

/** A note a move took out of its binder, as the undo needs it. */
export interface MovedNote {
  id: string;
  /** Its title as the screen showed it (for the notices). */
  title: string;
  /** The binder it came from (`''` is the root). */
  binder: string;
  /** Its path before the move, and its path after it. */
  from: string;
  to: string;
  /** The stamp of its file after the move (a move does not rewrite it): the undo goes only over that file. */
  stamp: FileStamp;
}

/** What the free order of a binder held when notes left it. */
export interface OrderMemory {
  /** The ids the order file listed. */
  listed: string[];
  /** The notes of the binder as the grid showed them (the listed ones as the file lists them, then the others by date). */
  shown: string[];
}

/**
 * What each binder that has a free order held, for the binders `moved` notes come from (the others have no place to
 * give back). `all` is every note of the library, `orders` the free orders the listing carries.
 */
export function rememberOrders(
  moved: readonly NoteSummary[],
  all: readonly NoteSummary[],
  orders: Readonly<Record<string, string[]>>,
  locale: string,
): Map<string, OrderMemory> {
  const memory = new Map<string, OrderMemory>();
  for (const binder of new Set(moved.map((note) => note.binder))) {
    const listed = orders[binder];
    if (!listed) continue;
    const shown = inBinderOrder(
      all.filter((note) => note.binder === binder),
      listed,
      locale,
    ).map((note) => note.id);
    memory.set(binder, { listed: [...listed], shown });
  }
  return memory;
}

/** What to do about the place of notes that go back into a binder. */
export type Placement =
  /** Nothing to write: the binder has no free order (any more), the notes were not listed, or the file still lists them. */
  | { kind: 'none' }
  /** The order to write (over the one the library holds), and the ids of the notes it puts at the end for lack of a place. */
  | { kind: 'write'; ids: string[]; atEnd: string[] };

/**
 * Where `returned` (ids of notes that came back to a binder) stand in its free order. `held` is the order the library
 * holds for the binder now (undefined: none), `now` the notes of the binder now, returned ones included.
 *
 * A returned note the file still lists has its place. One that was listed when it left and is no longer (the binder was
 * rearranged meanwhile, or the file rewritten without it) goes back after the neighbour it had, but only if the notes
 * that remain are in the relative order they had: that order is then the one the user knew, and the place is not a
 * guess. If they were rearranged, it goes to the end of the binder.
 */
export function placeBack(
  memory: OrderMemory | undefined,
  returned: readonly string[],
  held: readonly string[] | undefined,
  now: readonly NoteSummary[],
  locale: string,
): Placement {
  if (!memory || !held) return { kind: 'none' };
  const listed = new Set(memory.listed);
  const heldSet = new Set(held);
  const missing = memory.shown.filter((id) => returned.includes(id) && listed.has(id) && !heldSet.has(id));
  if (missing.length === 0) return { kind: 'none' };
  const lost = new Set(missing);
  const others = inBinderOrder(
    now.filter((note) => !lost.has(note.id)),
    held,
    locale,
  ).map((note) => note.id);
  const othersSet = new Set(others);
  const knew = new Set(memory.shown);
  const was = memory.shown.filter((id) => othersSet.has(id));
  const is = others.filter((id) => knew.has(id));
  if (was.length !== is.length || was.some((id, i) => id !== is[i])) return { kind: 'write', ids: [...others, ...missing], atEnd: missing };
  const ids = [...others];
  for (const id of missing) {
    const at = memory.shown.indexOf(id);
    const present = (candidate: string | undefined) => candidate !== undefined && ids.includes(candidate);
    const before = memory.shown.slice(0, at).reverse().find(present);
    const after = memory.shown.slice(at + 1).find(present);
    if (before !== undefined) ids.splice(ids.indexOf(before) + 1, 0, id);
    else if (after !== undefined) ids.splice(ids.indexOf(after), 0, id);
    else ids.push(id);
  }
  return { kind: 'write', ids, atEnd: [] };
}

/** The ids of `held` without those of `gone`; null when none of them was listed (nothing to write). */
export function withoutIds(held: readonly string[] | undefined, gone: readonly string[]): string[] | null {
  if (!held || !held.some((id) => gone.includes(id))) return null;
  return held.filter((id) => !gone.includes(id));
}

/** The last part of a path, with either separator. */
export function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
