/**
 * Browser stand-in for the background image commands (`background.rs`): a file whose name ends
 * in `.pdf` is a PDF of three A4 portrait pages, anything else a 1600 × 1200 photo. Staged images
 * are only keys and sizes (the browser draws a placeholder where the image goes, `mock-notes.ts`).
 */
import type { BackgroundSource, StagedBackground } from './background';
import { fitRect } from './background';
import { UNHANDLED, mockLibrary, notifyLibraryChanged } from './mock-data';
import { mockLibraryInvoke } from './mock-library';
import type { NoteSummary } from './note';

/** A4 portrait in points. */
const A4_POINTS = { width: 595.28, height: 841.89 };
const PDF_PAGES = 3;
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

function inspect(path: string): BackgroundSource {
  if (!/\.(pdf|png|jpe?g)$/i.test(path)) throw { code: 'backgroundUnsupported', message: path };
  return {
    name: nameOf(path),
    kind: isPdf(path) ? 'pdf' : 'image',
    pages: isPdf(path) ? Array.from({ length: PDF_PAGES }, () => ({ ...A4_POINTS })) : [],
  };
}

function stage(path: string, pdfPage?: number): StagedBackground {
  const source = inspect(path);
  if (source.kind === 'pdf') {
    const page = pdfPage ?? 1;
    if (page < 1 || page > PDF_PAGES) throw { code: 'pdfNoSuchPage', message: String(page) };
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

function createFromFile(binder: string, path: string, pages?: number[]): NoteSummary[] {
  const source = inspect(path);
  const stem = source.name.replace(/\.[^.]*$/, '');
  const chosen = source.kind === 'pdf' ? (pages ?? Array.from({ length: PDF_PAGES }, (_, i) => i + 1)) : [0];
  const made: NoteSummary[] = [];
  for (const page of chosen) {
    if (source.kind === 'pdf' && (page < 1 || page > PDF_PAGES)) throw { code: 'pdfNoSuchPage', message: String(page) };
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
  }
  notifyLibraryChanged();
  return made;
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
      return undefined;
    case 'create_notes_from_file':
      return createFromFile(args.binder as string, args.path as string, args.pages as number[] | undefined);
    case 'background_image_names':
      return (args.paths as string[]).map((path) => mockLibrary().notes.get(path)?.note.backgroundImage?.name ?? null);
    default:
      return UNHANDLED;
  }
}
