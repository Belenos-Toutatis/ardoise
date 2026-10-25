/** "Revenir à l'original" in the editor: the original's ink in the note being edited. */
import { currentFit, displayedSize, fitRect, type PixelSize } from '../background';
import type { BackgroundImage, PageSize } from '../note';
import type { NoteOriginal } from '../original';
import { nearestVisibleLayer, type Doc } from './state';

/**
 * The doc with the original's page size, layers and strokes (one undoable step); the title,
 * tags, favourite, paper and everything else of the note stay, the background image too, placed
 * for the restored page (`imageForOriginal`). Nothing is selected afterwards, and new ink goes
 * on the top visible layer. `doc` itself without an original.
 */
export function revertToOriginal(doc: Doc, original: NoteOriginal): Doc {
  if (original.source === 'none' || original.layers.length === 0) return doc;
  const note = { ...doc.note, page: original.page, layers: original.layers, strokes: original.strokes };
  const image = doc.note.backgroundImage;
  if (image) note.backgroundImage = imageForOriginal(image, doc.note.page, original, doc.backgroundPixels);
  const top = note.layers.length - 1;
  // The rest of the document (the background image's pixels) stays.
  return { ...doc, note, selection: [], activeLayer: nearestVisibleLayer(note, top) ?? top };
}

/** The same image: the same file, and the same page of it for a PDF. */
function sameImage(a: BackgroundImage, b: BackgroundImage): boolean {
  return a.name === b.name && (a.pdfPage ?? null) === (b.pdfPage ?? null);
}

/**
 * Where the note's current background image goes once the original's page comes back
 * (`page` is the page before the revert; `pixels` the size of the image's stored pixels, when
 * known). The rule (`docs/design/extras.md`, "Back to the original"):
 *
 * - The original had this very image (same file name and PDF page): it goes back where it was
 *   then (rectangle and quarter turns); only its opacity stays as it is now.
 * - Otherwise (another image, an original without one, a tablet page, which never has one):
 *   page turns are the only thing that turn an image, and the revert undoes them, so the image
 *   comes back upright, as its file shows it (no quarter turn). When it was turned or the page
 *   size changes, it is placed again on the restored page with the placement it has now
 *   (contain, cover or stretch, as `currentFit` recognises it), or contained when it was moved
 *   by a crop. Otherwise it stays where it is.
 */
export function imageForOriginal(
  image: BackgroundImage,
  page: PageSize,
  original: Exclude<NoteOriginal, { source: 'none' }>,
  pixels?: PixelSize,
): BackgroundImage {
  const before = original.backgroundImage;
  if (before && sameImage(before, image)) {
    return { ...image, rect: { ...before.rect }, quarterTurns: before.quarterTurns ?? 0 };
  }
  const turns = image.quarterTurns ?? 0;
  const samePage = page.width === original.page.width && page.height === original.page.height;
  if (turns % 4 === 0 && samePage) return image;
  // Without the pixels' size, the rectangle (turned back) stands for it.
  const stored = pixels ?? displayedSize({ width: image.rect.width, height: image.rect.height }, turns % 2);
  const fit = currentFit(image, page, stored) ?? 'contain';
  return { ...image, rect: fitRect(original.page, stored, fit), quarterTurns: 0 };
}
