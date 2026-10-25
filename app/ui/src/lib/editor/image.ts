/**
 * The background image of the note being edited (SPEC §4.4, P2): put one under the strokes,
 * place it, change its opacity, remove it. Each is one undoable step on the `Doc`, which keeps
 * the image (in the note) and the key of its pixels (`backgroundPixels`) together: undo and redo
 * always pair an image with its own pixels, saving passes that key to Rust, and Rust writes
 * those pixels into the note file only then (`docs/design/extras.md`, "Background image or PDF").
 */
import { IMAGE_FITS, fitRect, refit, type ImageFit, type PixelsRef, type StagedBackground } from '../background';
import type { BackgroundImage, Note, PageSize } from '../note';
import type { Doc } from './state';

/** The image an import makes on `page`: placed with `fit` (all of it visible by default), opaque. */
export function importedImage(page: PageSize, staged: StagedBackground, fit: ImageFit = 'contain'): BackgroundImage {
  const image: BackgroundImage = {
    name: staged.name,
    rect: fitRect(page, staged, fit),
    quarterTurns: 0,
    opacity: 1,
  };
  if (staged.pdfPage !== undefined) image.pdfPage = staged.pdfPage;
  return image;
}

/** The pixels reference of a staged image. */
export function pixelsOf(staged: StagedBackground): PixelsRef {
  return { key: staged.key, width: staged.width, height: staged.height };
}

/** Puts `image`, whose pixels are `pixels`, under the strokes (replacing any other). */
export function setBackgroundImage(doc: Doc, image: BackgroundImage, pixels: PixelsRef): Doc {
  return { ...doc, note: { ...doc.note, backgroundImage: image }, backgroundPixels: pixels };
}

/** Takes the image away (its pixels stay in Rust's memory for an undo). */
export function removeBackgroundImage(doc: Doc): Doc {
  if (!doc.note.backgroundImage) return doc;
  const note: Note = { ...doc.note };
  delete note.backgroundImage;
  const next: Doc = { ...doc, note };
  delete (next as { backgroundPixels?: PixelsRef }).backgroundPixels;
  return next;
}

/** The image's opacity, 0 to 1. */
export function setImageOpacity(doc: Doc, opacity: number): Doc {
  const image = doc.note.backgroundImage;
  const value = Math.min(1, Math.max(0, opacity));
  if (!image || (image.opacity ?? 1) === value) return doc;
  return { ...doc, note: { ...doc.note, backgroundImage: { ...image, opacity: value } } };
}

/** The image placed again on the page with `fit` (needs the size of its pixels). */
export function setImageFit(doc: Doc, fit: ImageFit): Doc {
  const image = doc.note.backgroundImage;
  const pixels = doc.backgroundPixels;
  if (!image || !pixels || !IMAGE_FITS.includes(fit)) return doc;
  const placed = refit(image, doc.note.page, pixels, fit);
  const same = JSON.stringify(placed.rect) === JSON.stringify(image.rect);
  return same ? doc : { ...doc, note: { ...doc.note, backgroundImage: placed } };
}

/** What `render_background` needs to draw the doc's image, or undefined without one. */
export function placedImage(doc: Doc): { image: BackgroundImage; key: string } | undefined {
  const image = doc.note.backgroundImage;
  const pixels = doc.backgroundPixels;
  return image && pixels ? { image, key: pixels.key } : undefined;
}
