/**
 * The browser stand-in's library (`npm run dev`, component tests): a few demo notes kept in
 * memory, shared by `mock-library.ts` (listing, binders, tags, trash) and `mock-notes.ts`
 * (opening and saving notes).
 */
import type { InkPoint, Tool } from './ink';
import type { FileStamp, Note, Rgba, Stroke, TrashEntry } from './note';

/** Returned by a mock module for a command it does not handle. */
export const UNHANDLED = Symbol('unhandled');

export interface StoredNote {
  note: Note;
  stamp: FileStamp;
  /** Relative, `/`-separated; empty for the root. */
  binder: string;
}

export interface MockLibrary {
  root: string;
  /** Path → note. Paths are `root\binder\file name`. */
  notes: Map<string, StoredNote>;
  binders: Set<string>;
  trash: Map<string, TrashEntry & { stored: StoredNote }>;
  /** Thumbnails are generated SVG data URLs keyed by note id. */
}

let clock = 0;

/** A new stamp, different from every previous one. */
export function freshStamp(len = 1000): FileStamp {
  clock += 1;
  return { modified: { secs_since_epoch: 1_792_843_200 + clock, nanos_since_epoch: 0 }, len };
}

export function sameStamp(a: FileStamp, b: FileStamp): boolean {
  return (
    a.len === b.len &&
    a.modified.secs_since_epoch === b.modified.secs_since_epoch &&
    a.modified.nanos_since_epoch === b.modified.nanos_since_epoch
  );
}

function wave(x0: number, y0: number, length: number, amplitude: number, tool: Tool, color: Rgba): Stroke {
  const points: InkPoint[] = Array.from({ length: 60 }, (_, i) => ({
    x: x0 + (length * i) / 59,
    y: y0 + amplitude * Math.sin(i / 6),
    pressure: 0.4 + 0.5 * Math.abs(Math.sin(i / 9)),
    t: i * 5,
  }));
  return { tool, color, width: tool === 'highlighter' ? 500 : 60, layer: 0, points };
}

function demoNote(id: string, title: string, created: number, strokes: Stroke[], extra: Partial<Note> = {}): Note {
  return {
    id,
    title,
    created,
    modified: created,
    page: { width: 29700, height: 21600 },
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes,
    timing: 'recorded',
    origin: { kind: 'tablet' },
    ...extra,
  };
}

const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 255 };
const BLUE: Rgba = { r: 0x51, g: 0xa7, b: 0xf9, a: 255 };
const RED: Rgba = { r: 0xd8, g: 0, b: 0, a: 255 };

/** File name like the Rust library: `YYYY-MM-DD Title.ardoise`. */
export function fileName(note: Note): string {
  const date = new Date(note.created).toISOString().slice(0, 10);
  return `${date}${note.title ? ' ' + note.title : ''}.ardoise`;
}

export function notePath(root: string, binder: string, name: string): string {
  return [root, ...binder.split('/').filter(Boolean), name].join('\\');
}

function seed(root: string): MockLibrary {
  const library: MockLibrary = { root, notes: new Map(), binders: new Set(['Classe', 'Classe/3e B', 'Perso']), trash: new Map() };
  const add = (binder: string, note: Note) => {
    library.notes.set(notePath(root, binder, fileName(note)), { note, stamp: freshStamp(), binder });
  };
  add(
    'Classe/3e B',
    demoNote(
      'b1f0c6c2-0000-4000-8000-000000000001',
      'Cours de physique',
      Date.UTC(2026, 9, 20, 8, 30),
      [wave(2000, 3000, 20000, 600, 'ballpoint', BLACK), wave(2000, 6000, 18000, 400, 'fountainPen', BLUE)],
      { tags: ['physique', '3e B'] },
    ),
  );
  add(
    '',
    demoNote(
      'b1f0c6c2-0000-4000-8000-000000000002',
      'Liste de courses',
      Date.UTC(2026, 9, 22, 18, 0),
      [wave(3000, 4000, 9000, 300, 'feltTip', RED)],
      { favorite: true },
    ),
  );
  add(
    'Perso',
    demoNote(
      'b1f0c6c2-0000-4000-8000-000000000003',
      'Réunion parents',
      Date.UTC(2026, 9, 23, 17, 15),
      [wave(1500, 2500, 25000, 800, 'pencil', BLACK)],
      { tags: ['réunion'] },
    ),
  );
  return library;
}

let library: MockLibrary | null = null;

/** The demo library (created on first use, rooted at `root`). */
export function mockLibrary(root = 'C:\\demo\\Ardoise'): MockLibrary {
  library ??= seed(root);
  return library;
}

/** Forgets every change (tests). */
export function resetMockLibrary(root?: string): MockLibrary {
  library = seed(root ?? 'C:\\demo\\Ardoise');
  return library;
}

const listeners = new Set<() => void>();

/** What `onLibraryChanged` does in the browser. */
export function mockOnLibraryChanged(handler: () => void): () => void {
  listeners.add(handler);
  return () => listeners.delete(handler);
}

/** Mock modules call this after changing notes, like the Rust watcher would. */
export function notifyLibraryChanged(): void {
  for (const listener of [...listeners]) listener();
}
