/** The export dialog's choices, remembered on this computer between two exports. */
import type { ExportFormat, ExportOptions, PageSize } from './note';

export const DPI_CHOICES = [96, 150, 300] as const;
export const FORMATS: readonly ExportFormat[] = ['png', 'jpg', 'webp', 'svg', 'pdf'];

/** Largest side of an exported image, in pixels (the renderer refuses more: `imageTooLarge`). */
export const MAX_IMAGE_SIDE = 16_384;
/** Largest side of a WebP image: the format stores sizes on 14 bits. */
export const MAX_WEBP_SIDE = 16_383;
/** Device units (10 µm) in an inch. */
const UNITS_PER_INCH = 2540;

/** Side in pixels of a page side of `units` device units at `dpi`, as the renderer rounds it. */
export function imageSide(units: number, dpi: number): number {
  return Math.round((units / UNITS_PER_INCH) * dpi);
}

/** Whether the image of every page at `dpi` stays within the largest side of `format` on both sides. */
export function dpiFits(pages: readonly PageSize[], dpi: number, format?: ExportFormat): boolean {
  const max = format === 'webp' ? MAX_WEBP_SIDE : MAX_IMAGE_SIDE;
  return pages.every((page) => imageSide(page.width, dpi) <= max && imageSide(page.height, dpi) <= max);
}

/** The formats made of pixels: they have a resolution. */
export function isRaster(format: ExportFormat): boolean {
  return format === 'png' || format === 'jpg' || format === 'webp';
}

/** The first of `candidates` (best first) whose image fits; null when none does. */
export function largestFittingDpi(
  pages: readonly PageSize[],
  candidates: readonly number[],
  format?: ExportFormat,
): number | null {
  return candidates.find((dpi) => dpiFits(pages, dpi, format)) ?? null;
}

export interface ExportChoice {
  format: ExportFormat;
  options: ExportOptions;
}

export const DEFAULT_CHOICE: ExportChoice = {
  format: 'png',
  options: { dpi: 150, background: true, quality: 90, pdfPage: 'actual' },
};

const KEY = 'ardoise.export';

function defaults(): ExportChoice {
  return { format: DEFAULT_CHOICE.format, options: { ...DEFAULT_CHOICE.options } };
}

/** The remembered choice, or the default when nothing valid is stored (or storage is blocked). */
export function loadChoice(): ExportChoice {
  try {
    const stored = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Partial<ExportChoice> | null;
    const o = stored?.options;
    if (!stored || !o || !FORMATS.includes(stored.format as ExportFormat)) return defaults();
    const fallback = DEFAULT_CHOICE.options;
    return {
      format: stored.format as ExportFormat,
      options: {
        dpi: (DPI_CHOICES as readonly number[]).includes(o.dpi) ? o.dpi : fallback.dpi,
        background: typeof o.background === 'boolean' ? o.background : fallback.background,
        quality: Number.isInteger(o.quality) && o.quality >= 1 && o.quality <= 100 ? o.quality : fallback.quality,
        pdfPage: o.pdfPage === 'a4' ? 'a4' : 'actual',
      },
    };
  } catch {
    return defaults();
  }
}

export function saveChoice(choice: ExportChoice): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(choice));
  } catch {
    // Nothing to remember it with: the next export starts from the defaults.
  }
}

/** File name of a note's export: the note file's name without `.ardoise`. */
export function exportStem(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path;
  return name.replace(/\.ardoise$/i, '') || name;
}
