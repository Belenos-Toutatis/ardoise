/**
 * "Back to the original" (SPEC §4.4): the ink a note can go back to, as `note_original` returns
 * it (`app/src-tauri/src/original.rs`). Only the page size, layers and strokes come back; the
 * title, tags, favourite, background and narration stay as they are.
 */
import type { Layer, PageSize, Stroke } from './note';

export type NoteOriginal =
  /** Nothing to go back to: never edited, and not from the tablet. */
  | { source: 'none' }
  /** As synced from the tablet (`original.bin`), or as before the first edit (`original.ardoise`). */
  | { source: 'tablet' | 'firstVersion'; page: PageSize; layers: Layer[]; strokes: Stroke[] };
