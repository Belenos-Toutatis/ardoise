/**
 * Browser stand-in for the organising commands of milestone M10 (`rename_binder`,
 * `delete_binder`, `tag_colors`, `set_tag_color`; Rust: `organize.rs`), over the demo library of
 * `mock-data.ts`.
 */
import type { BinderDeleted } from './api';
import { UNHANDLED, mockLibrary, notePath, notifyLibraryChanged, type MockLibrary } from './mock-data';
import { baseName, failure, mockLibraryInvoke } from './mock-library';
import type { TrashEntry } from './note';
import { tagKey } from './tag-colors';

/** Binders that also hold files other than notes (tests fill it): deleting them keeps their folder. */
export const mockForeignFiles = new Set<string>();

/** Tag colours of each demo library (`_ardoise/tags.json`), forgotten with it. */
const colorsOf = new WeakMap<MockLibrary, Map<string, string>>();

function colors(library: MockLibrary): Map<string, string> {
  let found = colorsOf.get(library);
  if (!found) {
    found = new Map();
    colorsOf.set(library, found);
  }
  return found;
}

const within = (binder: string, parent: string) => binder === parent || binder.startsWith(`${parent}/`);

function checkName(name: string): string {
  const trimmed = name.trim();
  if (!trimmed || trimmed === '.' || trimmed === '..' || /[\\/:*?"<>|]/.test(trimmed) || trimmed.toLowerCase() === '_ardoise') {
    failure('invalidName', name);
  }
  return trimmed;
}

function knownBinder(library: MockLibrary, binder: string): void {
  const exists = library.binders.has(binder) || [...library.notes.values()].some((s) => within(s.binder, binder));
  if (!binder || !exists) failure(binder ? 'binderNotFound' : 'invalidName', binder);
}

export function mockOrganizeInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'rename_binder': {
      const library = mockLibrary();
      const binder = args.binder as string;
      const name = checkName(args.name as string);
      knownBinder(library, binder);
      const segments = binder.split('/');
      const target = [...segments.slice(0, -1), name].join('/');
      if (target === binder) return binder;
      const caseOnly = target.toLowerCase() === binder.toLowerCase();
      const taken = [...library.binders].some((b) => b.toLowerCase() === target.toLowerCase());
      if (taken && !caseOnly) failure('binderExists', target);
      const moved = (path: string) => target + path.slice(binder.length);
      library.binders = new Set([...library.binders].map((b) => (within(b, binder) ? moved(b) : b)));
      for (const [path, stored] of [...library.notes]) {
        if (!within(stored.binder, binder)) continue;
        library.notes.delete(path);
        stored.binder = moved(stored.binder);
        library.notes.set(notePath(library.root, stored.binder, baseName(path)), stored);
      }
      notifyLibraryChanged();
      return target;
    }

    case 'delete_binder': {
      const library = mockLibrary();
      const binder = args.binder as string;
      knownBinder(library, binder);
      const paths = [...library.notes].filter(([, s]) => within(s.binder, binder)).map(([path]) => path).sort();
      const ids = paths.length > 0 ? (mockLibraryInvoke('trash_notes', { paths }) as string[]) : [];
      const trashed = ids.map((id): TrashEntry => {
        const entry = library.trash.get(id) ?? failure('notInTrash', id);
        const { stored, ...rest } = entry;
        void stored;
        return rest;
      });
      // Folders holding other files stay, with every folder above them up to the binder.
      const kept = new Set<string>();
      for (const foreign of mockForeignFiles) {
        if (!within(foreign, binder)) continue;
        const segments = foreign.split('/');
        for (let i = segments.length; i >= binder.split('/').length; i--) kept.add(segments.slice(0, i).join('/'));
      }
      library.binders = new Set([...library.binders].filter((b) => !within(b, binder) || kept.has(b)));
      notifyLibraryChanged();
      const deepestFirst = [...kept].sort((a, b) => b.split('/').length - a.split('/').length || a.localeCompare(b));
      return { trashed, failed: [], kept: deepestFirst } satisfies BinderDeleted;
    }

    case 'tag_colors':
      return Object.fromEntries(colors(mockLibrary()));

    case 'set_tag_color': {
      const key = tagKey(args.tag as string);
      if (!key) failure('invalidName', args.tag as string);
      const color = (args.color as string | null) ?? null;
      if (color !== null && !/^[a-z][a-z0-9-]{0,31}$/.test(color)) failure('invalidTagColor', color);
      const map = colors(mockLibrary());
      if (color === null) map.delete(key);
      else map.set(key, color);
      return Object.fromEntries(map);
    }

    default:
      return UNHANDLED;
  }
}
