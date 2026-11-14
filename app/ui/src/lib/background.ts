/**
 * Background images under a note (format 1.2, SPEC §4.4, `docs/design/extras.md`): where an image
 * goes on the page, and how page operations move it with the strokes. The interface never holds
 * the pixels: Rust imports them and keeps them under a key (`background.rs`, its staging area),
 * which the editor's history keeps beside the note (`Doc.backgroundPixels`).
 */
import type { BackgroundImage, ImageRect, PageSize } from './note';

/** How an image is placed (`ImageFit` in `note.rs`). */
export type ImageFit = 'contain' | 'cover' | 'stretch';

export const IMAGE_FITS: readonly ImageFit[] = ['contain', 'cover', 'stretch'];

/** Size of an image's stored pixels. */
export interface PixelSize {
  width: number;
  height: number;
}

/** An image imported by Rust and kept in memory under `key` (`StagedBackground` in `background.rs`). */
export interface StagedBackground extends PixelSize {
  key: string;
  /** The imported file's name. */
  name: string;
  /** Page of the PDF, from 1; absent for an image. */
  pdfPage?: number;
}

/** The pixels of a doc's background image: their key in Rust's staging area and their size. */
export interface PixelsRef extends PixelSize {
  key: string;
}

/** A file chosen to become a background (`BackgroundSource` in `background.rs`). */
export interface BackgroundSource {
  name: string;
  kind: 'image' | 'pdf';
  /** For a PDF, each page's size in points (1/72 inch). */
  pages: { width: number; height: number }[];
}

/** How far the creation of notes from a file is (`NotesProgress` in `background.rs`). */
export interface NotesProgress {
  /** Notes made so far. */
  done: number;
  /** Notes to make. */
  total: number;
}

/** The size as displayed: width and height swapped by an odd number of quarter turns. */
export function displayedSize(pixels: PixelSize, quarterTurns = 0): PixelSize {
  return quarterTurns % 2 === 1 ? { width: pixels.height, height: pixels.width } : pixels;
}

/**
 * Where an image of `pixels` (as displayed) goes on `page` (`ImageRect::fit` in `note.rs`):
 * whole and centred (contain), covering the page and centred (cover), or stretched to the page.
 * An empty image gets the whole page.
 */
export function fitRect(page: PageSize, pixels: PixelSize, fit: ImageFit): ImageRect {
  const whole = { x: 0, y: 0, width: page.width, height: page.height };
  if (pixels.width <= 0 || pixels.height <= 0 || fit === 'stretch') return whole;
  const byWidth = page.width / pixels.width;
  const byHeight = page.height / pixels.height;
  const scale = fit === 'contain' ? Math.min(byWidth, byHeight) : Math.max(byWidth, byHeight);
  const width = pixels.width * scale;
  const height = pixels.height * scale;
  return { x: (page.width - width) / 2, y: (page.height - height) / 2, width, height };
}

/** The placement that gives the image's rectangle on `page`, or null when it was moved since (a crop). */
export function currentFit(
  image: BackgroundImage,
  page: PageSize,
  pixels: PixelSize,
  // Stretch first: an image of the page's proportions gives the same rectangle for every fit.
  order: readonly ImageFit[] = ['stretch', 'contain', 'cover'],
): ImageFit | null {
  const shown = displayedSize(pixels, image.quarterTurns ?? 0);
  const near = (a: number, b: number) => Math.abs(a - b) <= Math.max(1, Math.abs(b) * 1e-4);
  for (const fit of order) {
    const r = fitRect(page, shown, fit);
    const i = image.rect;
    if (near(i.x, r.x) && near(i.y, r.y) && near(i.width, r.width) && near(i.height, r.height)) return fit;
  }
  return null;
}

/** The image placed again on `page` with `fit`. */
export function refit(image: BackgroundImage, page: PageSize, pixels: PixelSize, fit: ImageFit): BackgroundImage {
  return { ...image, rect: fitRect(page, displayedSize(pixels, image.quarterTurns ?? 0), fit) };
}

/**
 * The image after the page (`page`, its size before the turn) turned a quarter turn, clockwise
 * or not: its rectangle turned like the strokes' points (`rotateQuarter` in `editor/ops.ts`),
 * its pixels one more quarter turn.
 */
export function rotateImage(image: BackgroundImage, page: PageSize, direction: 'cw' | 'ccw'): BackgroundImage {
  const r = image.rect;
  const turns = image.quarterTurns ?? 0;
  const rect =
    direction === 'cw'
      ? { x: page.height - (r.y + r.height), y: r.x, width: r.height, height: r.width }
      : { x: r.y, y: page.width - (r.x + r.width), width: r.height, height: r.width };
  return { ...image, rect, quarterTurns: (turns + (direction === 'cw' ? 1 : 3)) % 4 };
}

/** The image after a crop whose top-left corner was at (`x`, `y`): moved like the strokes. */
export function shiftImage(image: BackgroundImage, x: number, y: number): BackgroundImage {
  return { ...image, rect: { ...image.rect, x: image.rect.x - x, y: image.rect.y - y } };
}

/** The image after the page's ink was scaled by `scale` and then moved by (`dx`, `dy`): moved like the strokes. */
export function mapImage(image: BackgroundImage, scale: number, dx: number, dy: number): BackgroundImage {
  const r = image.rect;
  return {
    ...image,
    rect: { x: r.x * scale + dx, y: r.y * scale + dy, width: r.width * scale, height: r.height * scale },
  };
}

/**
 * The image alone turned a quarter turn (the page and the strokes stay): its pixels turn, and it
 * is placed again with the fit it had (contain, cover or stretch; contain when its proportions
 * made them all the same), or, when it was moved by hand or cropped, turned in place around the
 * centre of its rectangle. Needs the size of the stored pixels for the first case.
 */
export function turnImageAlone(
  image: BackgroundImage,
  page: PageSize,
  pixels: PixelSize | undefined,
  direction: 'cw' | 'ccw',
): BackgroundImage {
  const turned: BackgroundImage = {
    ...image,
    quarterTurns: ((image.quarterTurns ?? 0) + (direction === 'cw' ? 1 : 3)) % 4,
  };
  const fit = pixels ? currentFit(image, page, pixels, ['contain', 'cover', 'stretch']) : null;
  if (fit && pixels) return refit(turned, page, pixels, fit);
  const r = image.rect;
  const cx = r.x + r.width / 2;
  const cy = r.y + r.height / 2;
  return { ...turned, rect: { x: cx - r.height / 2, y: cy - r.width / 2, width: r.height, height: r.width } };
}

/** Opacity of an image (absent means 1). */
export function opacityOf(image: BackgroundImage): number {
  return image.opacity ?? 1;
}

/**
 * The pages of a page list typed by the user (`1-3, 5`), in order and without repeats, each
 * within 1..`count`; null when the text is not a valid list.
 */
export function parsePageList(text: string, count: number): number[] | null {
  const pages: number[] = [];
  const parts = text
    .split(/[,;]/)
    .map((p) => p.trim())
    .filter((p) => p !== '');
  if (parts.length === 0) return null;
  for (const part of parts) {
    const match = /^(\d+)\s*(?:[-–]\s*(\d+))?$/.exec(part);
    if (!match) return null;
    const from = Number(match[1]);
    const to = match[2] === undefined ? from : Number(match[2]);
    if (from < 1 || to < from || to > count) return null;
    for (let page = from; page <= to; page++) if (!pages.includes(page)) pages.push(page);
  }
  return pages;
}

/** `pages` (from 1) as a page list the field takes back (`parsePageList`): runs as `1-3`, then `5`; sorted, no repeat. */
export function formatPageList(pages: readonly number[]): string {
  const sorted = [...new Set(pages)].sort((a, b) => a - b);
  const parts: string[] = [];
  for (let i = 0; i < sorted.length; ) {
    let end = i;
    while (end + 1 < sorted.length && sorted[end + 1] === sorted[end] + 1) end += 1;
    parts.push(end > i ? `${sorted[i]}-${sorted[end]}` : String(sorted[i]));
    i = end + 1;
  }
  return parts.join(', ');
}

/** A page's size in points as millimetres, rounded (`210 × 297`). */
export function pageMillimetres(page: { width: number; height: number }): { width: number; height: number } {
  const mm = (pt: number) => Math.round((pt * 25.4) / 72);
  return { width: mm(page.width), height: mm(page.height) };
}
