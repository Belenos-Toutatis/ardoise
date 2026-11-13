/**
 * Browser stand-in for the library commands (`library_list`, `note_thumbnail`, `create_note`,
 * binders, tags, favourites, trash, `tablet_info`…), over the demo library of `mock-data.ts`.
 */
import type { FileStamp, LibraryListing, Note, NoteSummary, Stroke, TabletInfo, TagCount, TrashEntry } from './note';
import {
  UNHANDLED,
  fileName,
  freshStamp,
  mockAutoExportSettings,
  mockLibrary,
  normalizeTags,
  notePath,
  notifyLibraryChanged,
  sameStamp,
  type MockLibrary,
  type StoredNote,
} from './mock-data';

/** Files the demo listing reports as unreadable; tests and developers may fill it. */
export const mockProblems: { path: string; code: string; message: string }[] = [];

const DEMO_TABLET: TabletInfo = {
  name: 'Bamboo Slate',
  address: 'AA:BB:CC:DD:EE:FF',
  registered: '2026-10-12T09:30:00+02:00',
  width: 29700,
  height: 21600,
};

/** The registered tablet follows the demo library: a reset library registers it again. */
let tablet: { library: MockLibrary; info: TabletInfo | null } | null = null;

function tabletState(library: MockLibrary) {
  if (!tablet || tablet.library !== library) tablet = { library, info: { ...DEMO_TABLET } };
  return tablet;
}

/** The latest creation date accepted, as the Rust library's (`MAX_CREATED_MS`: 9999-12-31 00:00 UTC). */
const MAX_CREATED = 253_402_214_400_000;

/** `title (word)`, then `(word 2)`, `(word 3)`… as `copy_title` of the Rust library. */
export function copyTitle(title: string, word: string): string {
  const trimmed = title.trim();
  const name = word.trim();
  const open = trimmed.endsWith(')') ? trimmed.lastIndexOf('(') : -1;
  if (open >= 0) {
    const inside = trimmed.slice(open + 1, -1);
    const before = trimmed.slice(0, open).trimEnd();
    let n = 0;
    if (inside.toLowerCase() === name.toLowerCase()) n = 1;
    else {
      const space = inside.lastIndexOf(' ');
      const k = Number(inside.slice(space + 1));
      if (space > 0 && inside.slice(0, space).toLowerCase() === name.toLowerCase() && Number.isInteger(k) && k >= 2) n = k;
    }
    if (n > 0) return before ? `${before} (${name} ${n + 1})` : `(${name} ${n + 1})`;
  }
  return trimmed ? `${trimmed} (${name})` : `(${name})`;
}

let counter = 0;

export function newId(): string {
  counter += 1;
  const random = globalThis.crypto?.randomUUID?.();
  return random ?? `00000000-0000-4000-8000-${String(counter).padStart(12, '0')}`;
}

export function failure(code: string, message: string): never {
  throw { code, message };
}

/** Nextcloud conflict copies look like `name (conflicted copy 2026-10-24).ardoise`. */
function conflictOf(name: string): { originalName: string } | null {
  const match = /^(.*) \([^()]*(?:conflict|konflikt|conflit)[^()]*\)(\.ardoise)$/i.exec(name);
  return match ? { originalName: match[1] + match[2] } : null;
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function summary(path: string, stored: StoredNote): NoteSummary {
  const { note } = stored;
  const name = baseName(path);
  return {
    path,
    binder: stored.binder,
    fileName: name,
    id: note.id,
    title: note.title,
    created: note.created,
    modified: note.modified,
    tags: [...note.tags],
    favorite: note.favorite ?? false,
    stamp: stored.stamp,
    conflict: conflictOf(name),
  };
}

/** A relative binder path checked segment by segment, as the Rust library does. */
export function cleanBinder(binder: string): string {
  const segments = binder.split('/').map((s) => s.trim());
  if (binder.trim() === '') return '';
  for (const segment of segments) {
    if (!segment || segment === '.' || segment === '..' || /[\\:*?"<>|]/.test(segment) || segment === '_ardoise') {
      failure('invalidName', binder);
    }
  }
  return segments.join('/');
}

function addBinder(library: MockLibrary, binder: string): void {
  const segments = binder.split('/').filter(Boolean);
  for (let i = 1; i <= segments.length; i++) library.binders.add(segments.slice(0, i).join('/'));
}

/** `base.ardoise` → `base (2).ardoise`… until the path is free. */
export function freePath(library: MockLibrary, binder: string, name: string, ignore?: string): string {
  let candidate = notePath(library.root, binder, name);
  for (let n = 2; library.notes.has(candidate) && candidate !== ignore; n++) {
    candidate = notePath(library.root, binder, name.replace(/\.ardoise$/, ` (${n}).ardoise`));
  }
  return candidate;
}

function find(library: MockLibrary, path: string): StoredNote {
  return library.notes.get(path) ?? failure('noteNotFound', path);
}

/** The stored note at `path`, which must still carry `stamp` (otherwise `noteConflict`). */
function findFresh(library: MockLibrary, path: string, stamp: FileStamp): StoredNote {
  const stored = find(library, path);
  if (!sameStamp(stored.stamp, stamp)) failure('noteConflict', path);
  return stored;
}

function thumbnail(note: Note): string {
  const polylines = note.strokes
    .map((stroke: Stroke) => {
      const { r, g, b, a } = stroke.color;
      const points = stroke.points.map((p) => `${Math.round(p.x)},${Math.round(p.y)}`).join(' ');
      const opacity = stroke.tool === 'highlighter' ? 0.4 : a / 255;
      return `<polyline points="${points}" fill="none" stroke="rgb(${r},${g},${b})" stroke-opacity="${opacity}" stroke-width="${Math.max(stroke.width, 90)}" stroke-linecap="round" stroke-linejoin="round"/>`;
    })
    .join('');
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${note.page.width} ${note.page.height}"><rect width="100%" height="100%" fill="#ffffff"/>${polylines}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/**
 * The binders the interface lists: while the automatic export is on, its folder and sub-folders
 * (PDF and PNG files, not notes) are left out, unless they hold notes (`hide_export_folder`).
 */
function visibleBinders(binders: Set<string>, notes: NoteSummary[]): string[] {
  const { format, folder } = mockAutoExportSettings();
  const all = [...binders].sort();
  if (format === 'off') return all;
  const hidden = folder.toLowerCase();
  const within = (binder: string, parent: string) => binder === parent || binder.startsWith(`${parent}/`);
  const noteBinders = notes.map((n) => n.binder.toLowerCase());
  return all.filter((binder) => {
    const lower = binder.toLowerCase();
    return !within(lower, hidden) || noteBinders.some((n) => within(n, lower));
  });
}

function listing(library: MockLibrary): LibraryListing {
  const notes = [...library.notes].map(([path, stored]) => summary(path, stored));
  const binders = new Set(library.binders);
  for (const note of notes) {
    const segments = note.binder.split('/').filter(Boolean);
    for (let i = 1; i <= segments.length; i++) binders.add(segments.slice(0, i).join('/'));
  }
  const counts = new Map<string, number>();
  for (const note of notes) for (const tag of note.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  const tags: TagCount[] = [...counts].map(([tag, count]) => ({ tag, count })).sort((a, b) => a.tag.localeCompare(b.tag));
  const orders: Record<string, string[]> = {};
  for (const [binder, ids] of library.orders) orders[binder] = [...ids];
  return { notes, binders: visibleBinders(binders, notes), tags, problems: [...mockProblems], orders };
}

function modify(stored: StoredNote, change: (note: Note) => void): FileStamp {
  change(stored.note);
  stored.note.modified = Date.now();
  stored.stamp = freshStamp();
  return stored.stamp;
}

export function mockLibraryInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'library_list':
      return listing(mockLibrary());

    case 'note_thumbnail': {
      const stored = mockLibrary().notes.get(args.path as string);
      return stored ? thumbnail(stored.note) : null;
    }

    case 'create_note': {
      const library = mockLibrary();
      const binder = cleanBinder(args.binder as string);
      const now = Date.now();
      const note: Note = {
        id: newId(),
        title: (args.title as string).trim(),
        created: now,
        modified: now,
        page: { width: 29700, height: 21600 },
        background: { paper: 'white', template: 'blank', texture: false },
        tags: [],
        layers: [{ name: 'Calque 1', visible: true }],
        strokes: [],
        timing: 'synthesized',
        origin: { kind: 'ardoise' },
      };
      addBinder(library, binder);
      const path = freePath(library, binder, fileName(note));
      const stored: StoredNote = { note, stamp: freshStamp(), binder };
      library.notes.set(path, stored);
      notifyLibraryChanged();
      return summary(path, stored);
    }

    case 'create_binder': {
      const binder = cleanBinder(args.binder as string);
      addBinder(mockLibrary(), binder);
      notifyLibraryChanged();
      return undefined;
    }

    case 'move_notes': {
      const library = mockLibrary();
      const binder = cleanBinder(args.binder as string);
      const paths = args.paths as string[];
      paths.forEach((path) => find(library, path));
      addBinder(library, binder);
      const moved = paths.map((path) => {
        const stored = find(library, path);
        library.notes.delete(path);
        const target = freePath(library, binder, baseName(path));
        stored.binder = binder;
        library.notes.set(target, stored);
        return target;
      });
      notifyLibraryChanged();
      return moved;
    }

    case 'trash_notes': {
      const library = mockLibrary();
      const paths = args.paths as string[];
      const trashed: { path: string; id: string }[] = [];
      const failed: { path: string; error: { code: string; message: string } }[] = [];
      for (const path of paths) {
        const stored = library.notes.get(path);
        if (!stored) {
          failed.push({ path, error: { code: 'noteNotFound', message: path } });
          continue;
        }
        library.notes.delete(path);
        const id = newId();
        const relative = [...stored.binder.split('/').filter(Boolean), baseName(path)].join('/');
        library.trash.set(id, {
          id,
          title: stored.note.title.trim() || null,
          original: relative,
          deleted: Date.now(),
          noteId: stored.note.id,
          stored,
        });
        trashed.push({ path, id });
      }
      notifyLibraryChanged();
      return { trashed, failed };
    }

    case 'trash_list':
      return [...mockLibrary().trash.values()]
        .map(({ stored, ...entry }): TrashEntry => {
          void stored;
          return entry;
        })
        .sort((a, b) => b.deleted - a.deleted);

    case 'trash_restore': {
      const library = mockLibrary();
      const entry = library.trash.get(args.id as string) ?? failure('notInTrash', args.id as string);
      addBinder(library, entry.stored.binder);
      const path = freePath(library, entry.stored.binder, fileName(entry.stored.note));
      library.trash.delete(entry.id);
      library.notes.set(path, entry.stored);
      notifyLibraryChanged();
      return path;
    }

    case 'trash_purge': {
      const library = mockLibrary();
      if (!library.trash.delete(args.id as string)) failure('notInTrash', args.id as string);
      notifyLibraryChanged();
      return undefined;
    }

    case 'trash_empty': {
      const library = mockLibrary();
      const count = library.trash.size;
      library.trash.clear();
      notifyLibraryChanged();
      return count;
    }

    case 'set_note_tags': {
      const stored = findFresh(mockLibrary(), args.path as string, args.stamp as FileStamp);
      const tags = normalizeTags(args.tags as string[]);
      const stamp = modify(stored, (note) => (note.tags = tags));
      notifyLibraryChanged();
      return stamp;
    }

    case 'set_note_favorite': {
      const stored = findFresh(mockLibrary(), args.path as string, args.stamp as FileStamp);
      const stamp = modify(stored, (note) => (note.favorite = args.favorite as boolean));
      notifyLibraryChanged();
      return stamp;
    }

    case 'retitle_note': {
      const library = mockLibrary();
      const path = args.path as string;
      const stored = findFresh(library, path, args.stamp as FileStamp);
      // Any title goes: only the file name is cleaned (`fileName`).
      const title = (args.title as string).trim();
      const stamp = modify(stored, (note) => (note.title = title));
      library.notes.delete(path);
      const target = freePath(library, stored.binder, fileName(stored.note));
      library.notes.set(target, stored);
      notifyLibraryChanged();
      return { path: target, stamp };
    }

    case 'duplicate_note': {
      const library = mockLibrary();
      const source = find(library, args.path as string);
      const copy: Note = JSON.parse(JSON.stringify(source.note)) as Note;
      copy.id = newId();
      copy.title = copyTitle(copy.title, args.copyWord as string);
      copy.favorite = false;
      copy.modified = Date.now();
      const stored: StoredNote = { ...source, note: copy, stamp: freshStamp(), binder: source.binder };
      // The original's tablet page and sound are the copy's too (docs/design/library-order.md).
      if (source.original) stored.original = JSON.parse(JSON.stringify(source.original));
      const path = freePath(library, source.binder, fileName(copy));
      library.notes.set(path, stored);
      // A copy goes right after its original in the binder's order, when the binder has one.
      const order = library.orders.get(source.binder);
      const at = order?.indexOf(source.note.id) ?? -1;
      if (order && at >= 0) order.splice(at + 1, 0, copy.id);
      notifyLibraryChanged();
      return summary(path, stored);
    }

    case 'set_note_created': {
      const library = mockLibrary();
      const path = args.path as string;
      const stored = findFresh(library, path, args.stamp as FileStamp);
      const created = args.created as number;
      if (!Number.isFinite(created) || created < 0 || created > MAX_CREATED) failure('invalidDate', String(created));
      const stamp = modify(stored, (note) => (note.created = created));
      library.notes.delete(path);
      const target = freePath(library, stored.binder, fileName(stored.note));
      library.notes.set(target, stored);
      notifyLibraryChanged();
      return { path: target, stamp };
    }

    case 'set_binder_order': {
      const library = mockLibrary();
      const binder = cleanBinder(args.binder as string);
      if (binder !== '' && !library.binders.has(binder)) failure('binderNotFound', binder);
      library.orders.set(binder, [...new Set((args.ids as string[]).filter(Boolean))]);
      notifyLibraryChanged();
      return undefined;
    }

    case 'tablet_info':
      return tabletState(mockLibrary()).info;

    case 'forget_tablet':
      tabletState(mockLibrary()).info = null;
      return undefined;

    default:
      return UNHANDLED;
  }
}
