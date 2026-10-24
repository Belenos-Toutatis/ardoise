/**
 * Browser stand-in for the milestone M5 commands (`export_notes`, `copy_note_image`,
 * `split_note`, `merge_notes`, `set_auto_export`), over the demo library of `mock-data.ts`.
 * Exports write nothing (they return plausible paths); split and merge really change the demo
 * store, with the semantics of `ardoise_core::ops` (docs/design/exports-and-operations.md).
 */
import { applySplit } from './editor/split';
import type {
  AutoExportSettings,
  ExportFormat,
  ExportOptions,
  ExportSource,
  FileStamp,
  MergeLayout,
  Note,
  PageSize,
  SplitResult,
  SplitSpec,
  Stroke,
} from './note';
import { dpiFits } from './export-options';
import { UNHANDLED, fileName, freshStamp, mockLibrary, normalizeTags, notifyLibraryChanged, sameStamp } from './mock-data';
import { baseName, failure, freePath, newId, summary } from './mock-library';

/** Tests switch the clipboard off to see how the interface reports it. */
export const mockClipboard = { available: true };

/**
 * How long a browser export takes, milliseconds. 0 (the default) answers at once; a test (or a
 * developer) sets a delay to see the dialog under way and to cancel it.
 */
export const mockExport = { delayMs: 0 };

/** Settles the export under way with `cancelled`; null while none is running. */
let cancelRunning: (() => void) | null = null;

/** Runs `result` after the mock delay, unless `cancel_export` comes first (`cancelled`). */
function afterDelay<T>(result: () => T): T | Promise<T> {
  if (mockExport.delayMs <= 0) return result();
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      cancelRunning = null;
      try {
        resolve(result());
      } catch (error) {
        reject(error);
      }
    }, mockExport.delayMs);
    cancelRunning = () => {
      clearTimeout(timer);
      cancelRunning = null;
      reject({ code: 'cancelled', message: 'cancelled' });
    };
  });
}

/** Notes are plain JSON: a JSON round trip is a deep copy. */
function copy<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/** A page side above this is refused: 100 m in device units (10 µm). */
const MAX_SIDE = 10_000_000;
/** Gap between two merged notes in the replay, milliseconds. */
const MERGE_GAP_MS = 1000;

const EXTENSIONS: Record<ExportFormat, string> = { png: 'png', jpg: 'jpg', svg: 'svg', pdf: 'pdf' };

/** The note an export source stands for: its unsaved `note`, else the one stored at `path`. */
function sourceNote(source: ExportSource): Note {
  return source.note ?? (mockLibrary().notes.get(source.path)?.note ?? failure('noteNotFound', source.path));
}

/** Like the renderer, a raster image over `MAX_IMAGE_SIDE` pixels on a side is refused. */
function checkImageSize(pages: PageSize[], dpi: number): void {
  if (!dpiFits(pages, dpi)) failure('imageTooLarge', `dpi ${dpi}`);
}

function checkOptions(options: ExportOptions, format: ExportFormat): void {
  if (!(options.dpi >= 72 && options.dpi <= 600)) failure('exportFailed', `dpi ${options.dpi}`);
  if (format === 'jpg' && !(options.quality >= 1 && options.quality <= 100)) {
    failure('exportFailed', `quality ${options.quality}`);
  }
}

/** The files an export writes: the destination itself, or one file per note inside a folder. */
export function exportedPaths(sources: ExportSource[], format: ExportFormat, destination: string): string[] {
  if (format === 'pdf' || sources.length === 1) return [destination];
  const folder = destination.replace(/[\\/]+$/, '');
  const used = new Set<string>();
  return sources.map((source) => {
    const stem = baseName(source.path).replace(/\.ardoise$/i, '');
    let name = stem;
    for (let n = 2; used.has(name.toLowerCase()); n++) name = `${stem} (${n})`;
    used.add(name.toLowerCase());
    return `${folder}\\${name}.${EXTENSIONS[format]}`;
  });
}

// --- Merge ----------------------------------------------------------------------------------

/** First and last sample time of a note; null when it has no sample. */
function sampleSpan(note: Note): { first: number; last: number } | null {
  let first = Infinity;
  let last = -Infinity;
  for (const stroke of note.strokes) {
    for (const p of stroke.points) {
      if (p.t < first) first = p.t;
      if (p.t > last) last = p.t;
    }
  }
  return first <= last ? { first, last } : null;
}

/**
 * Assembles `notes` (in the order given) into one new note, `id` and `title` empty. Overlay: one
 * page as large as the largest, every note at the top-left, layers appended. End to end: a long
 * page, notes stacked, layers merged by index. Sample times are made sequential.
 */
export function mergeNotesLogic(notes: Note[], layout: MergeLayout): Note {
  if (notes.length < 2) failure('nothingToMerge', String(notes.length));
  const width = Math.max(...notes.map((n) => n.page.width));
  const height =
    layout === 'overlay' ? Math.max(...notes.map((n) => n.page.height)) : notes.reduce((sum, n) => sum + n.page.height, 0);
  if (width > MAX_SIDE || height > MAX_SIDE) failure('pageTooLarge', `${width} × ${height}`);

  const layers: Note['layers'] = [];
  const strokes: Stroke[] = [];
  let y = 0;
  // Where the previous note with samples ends; a note without samples moves nothing.
  let previousEnd: number | null = null;
  notes.forEach((note) => {
    const span = sampleSpan(note);
    const dt = span && previousEnd !== null ? previousEnd + MERGE_GAP_MS - span.first : 0;
    if (span) previousEnd = span.last + dt;
    const base = layout === 'overlay' ? layers.length : 0;
    if (layout === 'overlay') {
      for (const layer of note.layers) {
        const several = note.layers.length > 1;
        const name = several && note.title ? `${note.title} - ${layer.name}` : note.title || layer.name;
        layers.push({ name, visible: layer.visible });
      }
    } else {
      note.layers.forEach((layer, i) => {
        if (!layers[i]) layers[i] = { ...layer };
      });
    }
    const top = Math.max(0, note.layers.length - 1);
    for (const stroke of note.strokes) {
      strokes.push({
        ...stroke,
        layer: base + Math.min(stroke.layer, top),
        points: stroke.points.map((p) => ({ ...p, y: p.y + y, t: Math.max(0, p.t + dt) })),
      });
    }
    if (layout === 'endToEnd') y += note.page.height;
  });

  const first = notes[0];
  return {
    ...copy(first),
    id: '',
    title: '',
    modified: Date.now(),
    page: { width, height },
    tags: normalizeTags(notes.flatMap((n) => n.tags)),
    favorite: false,
    layers,
    strokes,
    timing: notes.some((n) => n.timing === 'synthesized') ? 'synthesized' : first.timing,
    origin: { kind: 'ardoise' },
  };
}

// --- Automatic export --------------------------------------------------------------------------

/**
 * The folder as `AutoExport::relative_folder` accepts it: `/`-separated, no `.` or `..`, no
 * drive letter or stream (`:`), not the library's own `_ardoise` folder. Throws `invalidFolder`.
 */
export function checkAutoExport(settings: AutoExportSettings): AutoExportSettings {
  const parts: string[] = [];
  for (const raw of settings.folder.split(/[\\/]/)) {
    const part = raw.trim();
    if (part === '') continue;
    if (part === '.' || part === '..' || part.includes(':')) failure('invalidFolder', settings.folder);
    if (parts.length === 0 && ['_ardoise', '.ardoise'].includes(part.toLowerCase())) failure('invalidFolder', settings.folder);
    parts.push(part);
  }
  if (parts.length === 0) failure('invalidFolder', settings.folder);
  return { format: settings.format, folder: parts.join('/') };
}

// --- Commands ---------------------------------------------------------------------------------

export function mockExportInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'export_notes': {
      const sources = args.sources as ExportSource[];
      const format = args.format as ExportFormat;
      if (sources.length === 0) failure('exportFailed', 'no note');
      const notes = sources.map(sourceNote);
      const options = args.options as ExportOptions;
      checkOptions(options, format);
      if (format === 'png' || format === 'jpg') {
        checkImageSize(
          notes.map((n) => n.page),
          options.dpi,
        );
      }
      return afterDelay(() => exportedPaths(sources, format, args.destination as string));
    }

    case 'cancel_export':
      cancelRunning?.();
      return undefined;

    case 'copy_note_image': {
      const note = sourceNote(args.source as ExportSource);
      checkImageSize([note.page], args.dpi as number);
      if (!mockClipboard.available) failure('clipboardUnavailable', 'browser');
      return undefined;
    }

    case 'split_note': {
      const library = mockLibrary();
      const path = args.path as string;
      const entry = library.notes.get(path) ?? failure('noteNotFound', path);
      if (!sameStamp(entry.stamp, args.stamp as FileStamp)) failure('noteConflict', path);
      // Any title goes: only the file name is cleaned (`fileName`).
      const title = (args.title as string).trim();
      const parts = applySplit(args.note as Note, args.split as SplitSpec) ?? failure('nothingToSplit', path);

      // The new note first, in the same binder; then the kept part over the original.
      const extracted: Note = { ...copy(parts.extracted), id: newId(), title };
      const created = { note: extracted, stamp: freshStamp(JSON.stringify(extracted).length), binder: entry.binder };
      const createdPath = freePath(library, entry.binder, fileName(extracted));
      library.notes.set(createdPath, created);

      const kept: Note = { ...copy(parts.kept), id: entry.note.id, modified: Date.now() };
      entry.note = kept;
      entry.stamp = freshStamp(JSON.stringify(kept).length);
      notifyLibraryChanged();
      return { note: copy(kept), stamp: copy(entry.stamp), created: summary(createdPath, created) } satisfies SplitResult;
    }

    case 'merge_notes': {
      const library = mockLibrary();
      const paths = args.paths as string[];
      const sources = paths.map((path) => library.notes.get(path) ?? failure('noteNotFound', path));
      const title = (args.title as string).trim();
      const merged: Note = { ...mergeNotesLogic(sources.map((s) => s.note), args.layout as MergeLayout), id: newId(), title };
      const binder = sources[0].binder;
      const stored = { note: merged, stamp: freshStamp(JSON.stringify(merged).length), binder };
      const path = freePath(library, binder, fileName(merged));
      library.notes.set(path, stored);
      notifyLibraryChanged();
      return summary(path, stored);
    }

    default:
      return UNHANDLED;
  }
}
