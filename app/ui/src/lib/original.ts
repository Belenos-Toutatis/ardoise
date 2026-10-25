/**
 * "Back to the original" (SPEC §4.4): the ink a note can go back to, as `note_original` returns
 * it (`app/src-tauri/src/original.rs`). The page size, layers and strokes come back; the title,
 * tags, favourite, paper and narration stay as they are. The background image stays too, placed
 * for the restored page (`lib/editor/original.ts`).
 */
import type { BackgroundImage, Layer, PageSize, Stroke } from './note';

export type NoteOriginal =
  /** Nothing to go back to: never edited, and not from the tablet. */
  | { source: 'none' }
  /** As synced from the tablet (`original.bin`), or as before the first edit (`original.ardoise`). */
  | {
      source: 'tablet' | 'firstVersion';
      page: PageSize;
      layers: Layer[];
      strokes: Stroke[];
      /**
       * The background image the first version had, where it was (its pixels are not in
       * `original.ardoise`); never given for a tablet page.
       */
      backgroundImage?: BackgroundImage;
    };
