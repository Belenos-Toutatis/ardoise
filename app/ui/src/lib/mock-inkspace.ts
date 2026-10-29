/**
 * Browser stand-in for the Inkspace import commands (`import.rs`): a fake Inkspace on this PC
 * holding five notes, one of them unreadable. An import makes one empty, untitled note per
 * readable Inkspace note (no ink: the browser has none to convert) and remembers which Inkspace
 * notes are in the demo library, so that a second import skips them.
 */
import type { InkspaceFailure, InkspaceProgress, InkspaceReport, InkspaceSource } from './inkspace';
import { UNHANDLED, mockLibrary, notifyLibraryChanged, type MockLibrary } from './mock-data';
import { cleanBinder, failure, mockLibraryInvoke } from './mock-library';

const LOCAL_STATE = 'C:\\Users\\demo\\AppData\\Local\\Packages\\D91E29CF.InkspaceApp_38kynpdw5g1aw\\LocalState';

interface FakeNote {
  id: string;
  /** Unix ms; null for the note that cannot be read. */
  created: number | null;
  readable: boolean;
}

const NOTES: FakeNote[] = [
  { id: '3f2a9c1e-5b7d-4e0a-9a31-6c1d2e8b4f01', created: Date.UTC(2025, 8, 2, 8, 15), readable: true },
  { id: '8b61d4a7-0c2e-4f93-b5a8-17e9d3c06a22', created: Date.UTC(2025, 10, 18, 13, 40), readable: true },
  { id: 'c7e05b39-91fa-4d68-8e2c-5a40b7d1e933', created: Date.UTC(2026, 0, 12, 9, 5), readable: true },
  { id: 'd91e29cf-7a34-4b1c-a6f0-2e85c93b7d44', created: null, readable: false },
  { id: 'e4a8f160-3d95-47c2-b017-9f6a2c5e8d55', created: Date.UTC(2026, 5, 29, 15, 30), readable: true },
];

/** What the stand-in does; tests and developers may change it. */
export const mockInkspace = {
  /** The folder `inkspace_detect` finds; null for a PC without Inkspace. */
  detected: LOCAL_STATE as string | null,
  /** Pause before each note, in milliseconds (a calm pace in the dev browser); tests set it to 0. */
  stepMs: 300,
  /** Holds the import before note `before` (from 0) of those to write until `until` settles; tests use it as a gate. */
  hold: null as { before: number; until: Promise<void> } | null,
};

/** Inkspace ids imported into each demo library (a new library starts empty). */
const imported = new WeakMap<MockLibrary, Set<string>>();

let running = false;
let cancelRequested = false;

function importedIds(): Set<string> {
  const library = mockLibrary();
  let ids = imported.get(library);
  if (!ids) imported.set(library, (ids = new Set()));
  return ids;
}

/** Back to the start: Inkspace found, nothing imported, default pace (tests). */
export function resetMockInkspace(): void {
  mockInkspace.detected = LOCAL_STATE;
  mockInkspace.stepMs = 300;
  mockInkspace.hold = null;
  imported.delete(mockLibrary());
  running = false;
  cancelRequested = false;
}

/** Accepts the folders Inkspace keeps its data in (or a copy of them), as the Rust side does. */
function inspect(path: string): InkspaceSource {
  const end = path.indexOf('LocalState');
  if (end < 0 && !/InkspaceApp|WacomCloudDb/.test(path)) throw { code: 'inkspaceNotFound', message: path };
  const ids = importedIds();
  return {
    path: end >= 0 ? path.slice(0, end + 'LocalState'.length) : LOCAL_STATE,
    notes: NOTES.length,
    alreadyImported: NOTES.filter((n) => ids.has(n.id)).length,
    unreadable: NOTES.filter((n) => !n.readable).length,
  };
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

async function importNotes(
  path: string,
  binder: string,
  onProgress?: (progress: InkspaceProgress) => void,
): Promise<InkspaceReport> {
  if (running) failure('inkspaceBusy', 'an import is running');
  inspect(path);
  const target = cleanBinder(binder);
  running = true;
  cancelRequested = false;
  const ids = importedIds();
  // As on the Rust side: the notes that cannot be read are reported first, those imported
  // before are skipped, and the progress counts the notes to write.
  const readable = NOTES.filter((n) => n.readable && !ids.has(n.id));
  const report: InkspaceReport = {
    binder: target,
    total: NOTES.length,
    imported: 0,
    skipped: NOTES.filter((n) => n.readable && ids.has(n.id)).length,
    failed: NOTES.filter((n) => !n.readable).map(
      (note): InkspaceFailure => ({
        inkspaceId: note.id,
        created: note.created,
        code: 'inkspaceNoteDamaged',
        message: 'unknown stroke format',
      }),
    ),
    cancelled: false,
  };
  try {
    onProgress?.({ done: 0, total: readable.length });
    for (const [index, note] of readable.entries()) {
      if (mockInkspace.hold?.before === index) await mockInkspace.hold.until;
      await sleep(mockInkspace.stepMs);
      if (cancelRequested) {
        report.cancelled = true;
        break;
      }
      // Empty title, like Inkspace's notes.
      mockLibraryInvoke('create_note', { binder: target, title: '' });
      ids.add(note.id);
      report.imported += 1;
      onProgress?.({ done: index + 1, total: readable.length });
    }
    return report;
  } finally {
    running = false;
    cancelRequested = false;
    notifyLibraryChanged();
  }
}

export function mockInkspaceInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'inkspace_detect':
      return mockInkspace.detected;
    case 'inkspace_inspect':
      return inspect(args.path as string);
    case 'inkspace_import':
      return importNotes(
        args.path as string,
        args.binder as string,
        args.onProgress as ((progress: InkspaceProgress) => void) | undefined,
      );
    case 'inkspace_cancel':
      if (running) cancelRequested = true;
      return undefined;
    default:
      return UNHANDLED;
  }
}
