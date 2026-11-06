/**
 * A revision, a retouched copy of another note made by an assistant (SPEC §4.9): where its source
 * stands in a fresh library listing.
 */
import type { NoteSummary, RevisionOfNote, RevisionSource } from '../note';

/**
 * The note of the listing with this id, as Rust finds it (`revision::find_source`): a sync
 * client's conflict copy has the id of the note it copies, so the original file is preferred.
 */
export function sourceIn(notes: readonly NoteSummary[], id: string): RevisionSource | null {
  const found = notes.filter((n) => n.id === id);
  const best = found.find((n) => n.conflict === null) ?? found[0];
  return best ? { path: best.path, title: best.title } : null;
}

/** Whether two sources are the same place and name. */
export function sameSource(a: RevisionSource | null, b: RevisionSource | null): boolean {
  return a === b || (a !== null && b !== null && a.path === b.path && a.title === b.title);
}

/** `revision` with its source as the listing shows it now; the same object when nothing moved. */
export function withSourceIn(revision: RevisionOfNote, notes: readonly NoteSummary[]): RevisionOfNote {
  const source = sourceIn(notes, revision.of);
  return sameSource(source, revision.source) ? revision : { ...revision, source };
}
