/**
 * Browser stand-in for the drafts and the end of the session (`draft_*`, `set_unsaved_work`;
 * `drafts.rs`, `session_end.rs`). Drafts are kept per note id in memory, for the demo library
 * they were written in (a reset of the library forgets them). `startNewMockSession` makes the
 * drafts written so far look left over from an earlier run, as after a crash.
 */
import { binderOf } from './editor/disk';
import type { Draft, DraftSummary } from './editor/drafts';
import type { StagedBackground } from './background';
import { UNHANDLED, fileName, freshStamp, mockLibrary, notifyLibraryChanged, type MockLibrary, type StoredNote } from './mock-data';
import { failure, freePath, newId, summary } from './mock-library';
import type { FileStamp, Note } from './note';

interface Kept {
  draft: Draft;
  session: number;
  imageKey: string | null;
}

/** What tests look at: the reason last given for Windows' shutdown screen (null: nothing unsaved). */
export const mockDrafts = {
  unsavedReason: null as string | null,
  /** Draft writes so far (tests count them). */
  writes: 0,
};

let kept = new Map<string, Kept>();
let library: MockLibrary | null = null;
let session = 1;

/** The drafts of the demo library in use (a new demo library has none). */
function store(): Map<string, Kept> {
  const current = mockLibrary();
  if (current !== library) {
    library = current;
    kept = new Map();
  }
  return kept;
}

/** The drafts written so far now look left over from an earlier run of the app. */
export function startNewMockSession(): void {
  session += 1;
}

/** Forgets every draft and the reason (tests). */
export function resetMockDrafts(): void {
  kept = new Map();
  library = null;
  mockDrafts.unsavedReason = null;
  mockDrafts.writes = 0;
}

const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

function summaryOf(draft: Draft): DraftSummary {
  return { id: draft.id, path: draft.path, title: draft.title, savedAt: draft.savedAt };
}

function restoreCopy(id: string, title: string) {
  const entry = store().get(id);
  if (!entry) failure('draftNotFound', id);
  const lib = mockLibrary();
  const now = Date.now();
  const note: Note = { ...copy(entry.draft.note), id: newId(), title: title.trim(), created: now, modified: now, origin: { kind: 'ardoise' } };
  if (!entry.imageKey) delete note.backgroundImage;
  const wanted = binderOf(entry.draft.path, lib.root);
  const binder = wanted === '' || lib.binders.has(wanted) ? wanted : '';
  const path = freePath(lib, binder, fileName(note));
  const stored: StoredNote = { note, stamp: freshStamp(JSON.stringify(note).length), binder };
  lib.notes.set(path, stored);
  store().delete(id);
  notifyLibraryChanged();
  return summary(path, stored);
}

export function mockDraftsInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'draft_write': {
      const note = copy(args.note as Note);
      const id = args.noteId as string;
      mockDrafts.writes += 1;
      store().set(id, {
        session,
        imageKey: note.backgroundImage ? ((args.imageKey as string | null) ?? null) : null,
        draft: {
          id,
          path: args.path as string,
          title: note.title,
          savedAt: Date.now(),
          baseStamp: copy((args.baseStamp as FileStamp | null) ?? null),
          hasImage: !!note.backgroundImage && !!args.imageKey,
          note,
        },
      });
      return undefined;
    }
    case 'draft_find': {
      const entry = store().get(args.noteId as string);
      return entry ? copy(entry.draft) : null;
    }
    case 'draft_list':
      return [...store().values()]
        .filter((entry) => entry.session !== session)
        .map((entry) => summaryOf(entry.draft))
        .sort((a, b) => b.savedAt - a.savedAt);
    case 'draft_discard':
      store().delete(args.noteId as string);
      return undefined;
    case 'draft_stage_image': {
      const entry = store().get(args.noteId as string);
      if (!entry) failure('draftNotFound', args.noteId as string);
      const image = entry.draft.note.backgroundImage;
      if (!image || !entry.imageKey) return null;
      return { key: entry.imageKey, name: image.name, pdfPage: image.pdfPage, width: 1600, height: 1200 } satisfies StagedBackground;
    }
    case 'draft_restore_copy':
      return restoreCopy(args.noteId as string, args.title as string);
    case 'set_unsaved_work':
      mockDrafts.unsavedReason = (args.reason as string | null) ?? null;
      return undefined;
    default:
      return UNHANDLED;
  }
}
