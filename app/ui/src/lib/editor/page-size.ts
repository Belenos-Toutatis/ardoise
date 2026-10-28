/**
 * Page orientation and paper sizes of a note being edited (milestone M8). Sizes are device
 * units (10 µm): 1 mm is 100 units. The presets are given in portrait; a page of the other
 * orientation has them swapped.
 */
import type { PageSize } from '../note';

export type Orientation = 'portrait' | 'landscape';

/** Paper sizes the Page panel offers besides a custom one. */
export type PaperSizeId = 'a4' | 'a5' | 'letter' | 'tablet';

export const PAPER_SIZE_IDS: readonly PaperSizeId[] = ['a4', 'a5', 'letter', 'tablet'];

/** Portrait size of each preset. `tablet` is the page of a note from the Bamboo Slate (216 × 297 mm). */
export const PAPER_SIZES: Readonly<Record<PaperSizeId, PageSize>> = {
  a4: { width: 21_000, height: 29_700 },
  a5: { width: 14_800, height: 21_000 },
  letter: { width: 21_590, height: 27_940 },
  tablet: { width: 21_600, height: 29_700 },
};

/** Device units in a millimetre. */
export const UNITS_PER_MM = 100;

/** Smallest and largest side of a custom page, millimetres. */
export const MIN_SIDE_MM = 10;
export const MAX_SIDE_MM = 2000;

/** Landscape when wider than tall; a square page counts as portrait. */
export function orientationOf(page: PageSize): Orientation {
  return page.width > page.height ? 'landscape' : 'portrait';
}

/** The page of `size` (given in portrait) in `orientation`. */
export function inOrientation(size: PageSize, orientation: Orientation): PageSize {
  const long = Math.max(size.width, size.height);
  const short = Math.min(size.width, size.height);
  return orientation === 'landscape' ? { width: long, height: short } : { width: short, height: long };
}

/** The preset `page` has, in either orientation; null for any other size. */
export function presetOf(page: PageSize): PaperSizeId | null {
  const long = Math.max(page.width, page.height);
  const short = Math.min(page.width, page.height);
  for (const id of PAPER_SIZE_IDS) {
    if (PAPER_SIZES[id].width === short && PAPER_SIZES[id].height === long) return id;
  }
  return null;
}

/** The size of preset `id` in the orientation `page` has now. */
export function presetPage(id: PaperSizeId, page: PageSize): PageSize {
  return inOrientation(PAPER_SIZES[id], orientationOf(page));
}

/** The same page turned the other way up (width and height swapped). */
export function swappedPage(page: PageSize): PageSize {
  return { width: page.height, height: page.width };
}

/** A length in device units as millimetres, to a tenth. */
export function toMillimetres(units: number): number {
  return Math.round(units / (UNITS_PER_MM / 10)) / 10;
}

/** A length in millimetres typed by the user (comma or point as decimal mark) in device units; null if not a valid side. */
export function parseMillimetres(text: string): number | null {
  const value = Number(text.trim().replace(',', '.'));
  if (!Number.isFinite(value) || value < MIN_SIDE_MM || value > MAX_SIDE_MM) return null;
  return Math.round(value * UNITS_PER_MM);
}
