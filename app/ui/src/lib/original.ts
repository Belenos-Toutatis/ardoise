/**
 * "Back to the original" (SPEC §4.4): the ink a note can go back to, as `note_original` returns
 * it (`app/src-tauri/src/original.rs`). The page size, layers, strokes and page objects come back
 * (none at all for a blank sheet that live mode wrote on; a tablet page has no objects, so the text
 * frames added since go); the title, tags, favourite, paper and narration stay
 * as they are, except the slate of a sheet written on under the blackboard. The background image
 * stays too, placed for the restored page (`lib/editor/original.ts`).
 */
import type { BackgroundImage, Layer, PageObject, PageSize, Paper, Stroke } from './note';

export type NoteOriginal =
  /** Nothing to go back to: never edited, and not from the tablet. */
  | { source: 'none' }
  /** As synced from the tablet (`original.bin`), or as before the first edit (`original.ardoise`). */
  | {
      source: 'tablet' | 'firstVersion';
      page: PageSize;
      layers: Layer[];
      strokes: Stroke[];
      /** The page objects (text frames) it had: always empty for a tablet page; absent from older answers. */
      objects?: PageObject[];
      /** The paper the first version had; never given for a tablet page. */
      paper?: Paper;
      /**
       * The background image the first version had, where it was (its pixels are not in
       * `original.ardoise`); never given for a tablet page.
       */
      backgroundImage?: BackgroundImage;
    };
