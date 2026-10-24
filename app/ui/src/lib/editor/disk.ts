/**
 * The open note's file on disk: where it sits in the library, and whether it changed under the
 * editor (another PC through Nextcloud, the library screen…).
 */
import type { FileStamp, NoteSummary } from '../note';

export type DiskChange = 'changed' | 'missing';

function normalise(path: string): string {
  // Windows paths: either separator, any case.
  return path.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
}

export function samePath(a: string, b: string): boolean {
  return normalise(a) === normalise(b);
}

export function sameStamp(a: FileStamp, b: FileStamp): boolean {
  return (
    a.len === b.len &&
    a.modified.secs_since_epoch === b.modified.secs_since_epoch &&
    a.modified.nanos_since_epoch === b.modified.nanos_since_epoch
  );
}

/**
 * The binder (folder relative to the library, `/`-separated, empty for the root) holding the
 * note at `path`; empty when the path is not inside `library`.
 */
export function binderOf(path: string, library: string | null): string {
  if (!library) return '';
  const segments = path.split(/[\\/]+/).filter(Boolean);
  const root = library.split(/[\\/]+/).filter(Boolean);
  const inside = root.length < segments.length && root.every((s, i) => s.toLowerCase() === segments[i].toLowerCase());
  return inside ? segments.slice(root.length, -1).join('/') : '';
}

/**
 * How the note at `path`, last seen with `stamp`, stands in a fresh library listing: changed
 * (another stamp), missing (deleted, moved or renamed elsewhere), or null when unchanged.
 */
export function diskChange(notes: readonly NoteSummary[], path: string, stamp: FileStamp): DiskChange | null {
  const entry = notes.find((n) => samePath(n.path, path));
  if (!entry) return 'missing';
  return sameStamp(entry.stamp, stamp) ? null : 'changed';
}
