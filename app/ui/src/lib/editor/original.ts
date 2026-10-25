/** "Revenir à l'original" in the editor: the original's ink in the note being edited. */
import type { NoteOriginal } from '../original';
import { nearestVisibleLayer, type Doc } from './state';

/**
 * The doc with the original's page size, layers and strokes (one undoable step); the title,
 * tags, favourite, background and everything else of the note stay. Nothing is selected
 * afterwards, and new ink goes on the top visible layer. `doc` itself without an original.
 */
export function revertToOriginal(doc: Doc, original: NoteOriginal): Doc {
  if (original.source === 'none' || original.layers.length === 0) return doc;
  const note = { ...doc.note, page: original.page, layers: original.layers, strokes: original.strokes };
  const top = note.layers.length - 1;
  // The rest of the document (the background image's pixels) stays.
  return { ...doc, note, selection: [], activeLayer: nearestVisibleLayer(note, top) ?? top };
}
