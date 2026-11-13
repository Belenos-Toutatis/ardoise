/**
 * Browser stand-in for the background image commands (`background.rs`, `pdf_preview.rs`): a file whose name ends
 * in `.pdf` is a PDF of three A4 portrait pages (`fiche-12p.pdf`: twelve, every fifth one landscape), anything else a
 * 1600 × 1200 photo. Staged images are only keys and sizes (the browser draws a placeholder where the image goes,
 * `mock-notes.ts`); the thumbnails of a PDF are drawn as small SVG sheets.
 */
import type { BackgroundSource, NotesProgress, StagedBackground } from './background';
import { fitRect } from './background';
import { UNHANDLED, mockLibrary, notifyLibraryChanged } from './mock-data';
import { mockLibraryInvoke } from './mock-library';
import type { NoteSummary } from './note';

/** A4 portrait in points. */
const A4_POINTS = { width: 595.28, height: 841.89 };
const PDF_PAGES = 3;
const MOCK_PAGES_LIMIT = 10_000;
/** A page at 300 dpi; a photo. */
const PAGE_PIXELS = { width: 2480, height: 3508 };
const PHOTO_PIXELS = { width: 1600, height: 1200 };

let staged = 0;

function nameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function isPdf(path: string): boolean {
  return path.toLowerCase().endsWith('.pdf');
}

/** Pages of the mock PDF at `path`: three, or `N` for a name ending in `-Np.pdf`. */
function pagesOf(path: string): number {
  const named = /-(\d+)p\.pdf$/i.exec(path);
  return named ? Math.min(Math.max(Number(named[1]), 1), MOCK_PAGES_LIMIT) : PDF_PAGES;
}

/** A4 portrait, every fifth page of a long PDF landscape (so that the thumbnails differ in proportions). */
function pageSizeOf(page: number): { width: number; height: number } {
  return page % 5 === 0 && page > 3 ? { width: A4_POINTS.height, height: A4_POINTS.width } : { ...A4_POINTS };
}

function inspect(path: string): BackgroundSource {
  if (!/\.(pdf|png|jpe?g)$/i.test(path)) throw { code: 'backgroundUnsupported', message: path };
  return {
    name: nameOf(path),
    kind: isPdf(path) ? 'pdf' : 'image',
    pages: isPdf(path) ? Array.from({ length: pagesOf(path) }, (_, i) => pageSizeOf(i + 1)) : [],
  };
}

function stage(path: string, pdfPage?: number): StagedBackground {
  const source = inspect(path);
  if (source.kind === 'pdf') {
    const page = pdfPage ?? 1;
    if (page < 1 || page > pagesOf(path)) throw { code: 'pdfNoSuchPage', message: String(page) };
    return { key: `mock-${++staged}`, name: source.name, pdfPage: page, ...PAGE_PIXELS };
  }
  return { key: `mock-${++staged}`, name: source.name, ...PHOTO_PIXELS };
}

function stagedOfNote(path: string): StagedBackground | null {
  const image = mockLibrary().notes.get(path)?.note.backgroundImage;
  if (!image) return null;
  const pixels = image.pdfPage === undefined ? PHOTO_PIXELS : PAGE_PIXELS;
  return { key: `mock-note-${++staged}`, name: image.name, pdfPage: image.pdfPage, ...pixels };
}

function createFromFile(
  binder: string,
  path: string,
  pages?: number[],
  onProgress?: (progress: NotesProgress) => void,
): NoteSummary[] {
  const source = inspect(path);
  const stem = source.name.replace(/\.[^.]*$/, '');
  const chosen = source.kind === 'pdf' ? (pages ?? Array.from({ length: pagesOf(path) }, (_, i) => i + 1)) : [0];
  const made: NoteSummary[] = [];
  onProgress?.({ done: 0, total: chosen.length });
  for (const page of chosen) {
    if (source.kind === 'pdf' && (page < 1 || page > pagesOf(path))) throw { code: 'pdfNoSuchPage', message: String(page) };
    const title = source.kind === 'pdf' ? `${stem} - page ${page}` : stem;
    const summary = mockLibraryInvoke('create_note', { binder, title }) as NoteSummary;
    const stored = mockLibrary().notes.get(summary.path);
    if (stored) {
      const size = source.kind === 'pdf' ? { width: 21000, height: 29700 } : { width: 28000, height: 21000 };
      const pixels = source.kind === 'pdf' ? PAGE_PIXELS : PHOTO_PIXELS;
      stored.note.page = size;
      stored.note.backgroundImage = {
        name: source.name,
        ...(source.kind === 'pdf' ? { pdfPage: page } : {}),
        rect: fitRect(size, pixels, 'contain'),
        quarterTurns: 0,
        opacity: 1,
      };
    }
    made.push(summary);
    onProgress?.({ done: made.length, total: chosen.length });
  }
  notifyLibraryChanged();
  return made;
}

// --- The thumbnails of the question « which page? » --------------------------------------------------------

/** The PDFs open for their thumbnails, by token. */
const previews = new Map<number, string>();
let previewToken = 0;

/** How many previews are open (a test checks that closing the question frees them). */
export function openPdfPreviews(): number {
  return previews.size;
}

/** A small sheet with the page's number, as an SVG data URL (`width` pixels wide, in the page's proportions). */
function thumbnailOf(path: string, page: number, width: number): string {
  const size = pageSizeOf(page);
  const height = Math.round((width * size.height) / size.width);
  const lines = Array.from({ length: 9 }, (_, i) => {
    const y = Math.round(height * (0.42 + i * 0.06));
    const end = Math.round(width * (i % 3 === 2 ? 0.55 : 0.82));
    return `<path d="M${Math.round(width * 0.12)} ${y}H${end}" stroke="#9aa4b2" stroke-width="${Math.max(1, width / 90)}"/>`;
  }).join('');
  const hue = (page * 47) % 360;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">` +
    `<rect width="${width}" height="${height}" fill="#fff"/>` +
    `<rect x="${width * 0.12}" y="${height * 0.08}" width="${width * 0.5}" height="${height * 0.1}" fill="hsl(${hue} 55% 55%)"/>` +
    `<text x="${width * 0.12}" y="${height * 0.33}" font-family="sans-serif" font-size="${height * 0.14}" fill="#1d232b">${page}</text>` +
    lines +
    `<title>${nameOf(path)}</title></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function openPreview(path: string): number {
  if (!isPdf(path)) throw { code: 'backgroundUnsupported', message: path };
  inspect(path);
  previews.set(++previewToken, path);
  return previewToken;
}

function renderThumbnail(token: number, page: number, width: number): string {
  const path = previews.get(token);
  if (path === undefined) throw { code: 'pdfPreviewClosed', message: String(token) };
  if (page < 1 || page > pagesOf(path)) throw { code: 'pdfNoSuchPage', message: String(page) };
  return thumbnailOf(path, page, Math.min(Math.max(Math.round(width), 32), 512));
}

export function mockBackgroundInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'inspect_background_file':
      return inspect(args.path as string);
    case 'stage_background_image':
      return stage(args.path as string, args.pdfPage as number | undefined);
    case 'stage_note_background':
      return stagedOfNote(args.path as string);
    case 'release_background_images':
    case 'cancel_create_notes':
      return undefined;
    case 'open_pdf_preview':
      return openPreview(args.path as string);
    case 'render_pdf_thumbnail':
      return renderThumbnail(args.token as number, args.page as number, args.width as number);
    case 'close_pdf_preview':
      previews.delete(args.token as number);
      return undefined;
    case 'create_notes_from_file':
      return createFromFile(
        args.binder as string,
        args.path as string,
        args.pages as number[] | undefined,
        args.onProgress as ((progress: NotesProgress) => void) | undefined,
      );
    case 'background_image_names':
      return (args.paths as string[]).map((path) => mockLibrary().notes.get(path)?.note.backgroundImage?.name ?? null);
    default:
      return UNHANDLED;
  }
}
