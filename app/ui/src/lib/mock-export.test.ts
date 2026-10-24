// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { hline, note } from './editor/test-fixtures';
import { freshStamp, mockLibrary, mockOnLibraryChanged, resetMockLibrary } from './mock-data';
import { checkAutoExport, exportedPaths, mergeNotesLogic, mockClipboard, mockExport } from './mock-export';
import type { ExportOptions, Note } from './note';

const OPTIONS: ExportOptions = { dpi: 150, background: true, quality: 90, pdfPage: 'actual' };

beforeEach(() => {
  resetMockLibrary();
});

afterEach(() => {
  mockClipboard.available = true;
});

function pathOf(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

/** The `code` of the error a synchronous function throws. */
function thrownCode(run: () => unknown): string | undefined {
  try {
    run();
  } catch (error) {
    return (error as { code?: string }).code;
  }
  return undefined;
}

async function code(promise: Promise<unknown>): Promise<string | undefined> {
  try {
    await promise;
  } catch (error) {
    return (error as { code?: string }).code;
  }
  return undefined;
}

describe('mock export', () => {
  it('writes one file for one note and for a PDF, one file per note in a folder otherwise', async () => {
    const one = [{ path: pathOf('Liste de courses') }];
    const three = ['Liste de courses', 'Cours de physique', 'Réunion parents'].map((title) => ({ path: pathOf(title) }));
    expect(await api.exportNotes(one, 'png', OPTIONS, 'C:\\out\\a.png')).toEqual(['C:\\out\\a.png']);
    expect(await api.exportNotes(three, 'pdf', OPTIONS, 'C:\\out\\all.pdf')).toEqual(['C:\\out\\all.pdf']);
    const files = await api.exportNotes(three, 'svg', OPTIONS, 'C:\\out\\');
    expect(files).toHaveLength(3);
    expect(files[0]).toMatch(/^C:\\out\\2026-10-22 Liste de courses\.svg$/);
  });

  it('never gives two notes the same file name', () => {
    const sources = [{ path: 'C:\\lib\\a\\Note.ardoise' }, { path: 'C:\\lib\\b\\Note.ardoise' }];
    expect(exportedPaths(sources, 'jpg', 'D:\\x')).toEqual(['D:\\x\\Note.jpg', 'D:\\x\\Note (2).jpg']);
  });

  it('exports the editor note without it being in the library, and refuses unknown notes', async () => {
    const unsaved = { path: 'C:\\demo\\Ardoise\\New.ardoise', note: note([]) };
    expect(await api.exportNotes([unsaved], 'png', OPTIONS, 'C:\\out\\n.png')).toEqual(['C:\\out\\n.png']);
    expect(await code(api.exportNotes([{ path: 'C:\\nope.ardoise' }], 'png', OPTIONS, 'C:\\out\\n.png'))).toBe('noteNotFound');
  });

  it('refuses options and sources the desktop app would refuse', async () => {
    const source = [{ path: pathOf('Liste de courses') }];
    expect(await code(api.exportNotes([], 'png', OPTIONS, 'C:\\x'))).toBe('exportFailed');
    expect(await code(api.exportNotes(source, 'png', { ...OPTIONS, dpi: 10 }, 'C:\\x'))).toBe('exportFailed');
    expect(await code(api.exportNotes(source, 'jpg', { ...OPTIONS, quality: 0 }, 'C:\\x'))).toBe('exportFailed');
  });

  it('refuses an image over 16 384 pixels on a side, for PNG, JPG and the clipboard', async () => {
    const wide = { path: 'C:\\demo\\Ardoise\\Wide.ardoise', note: { ...note([]), page: { width: 300_000, height: 21600 } } };
    // 3 000 mm at 150 dpi is 17 717 px; at 96 dpi 11 339 px.
    expect(await code(api.exportNotes([wide], 'png', OPTIONS, 'C:\\out\\w.png'))).toBe('imageTooLarge');
    expect(await code(api.exportNotes([wide], 'jpg', OPTIONS, 'C:\\out\\w.jpg'))).toBe('imageTooLarge');
    expect(await code(api.copyNoteImage(wide, 150))).toBe('imageTooLarge');
    await expect(api.exportNotes([wide], 'png', { ...OPTIONS, dpi: 96 }, 'C:\\out\\w.png')).resolves.toEqual(['C:\\out\\w.png']);
    // Vector formats have no such limit.
    await expect(api.exportNotes([wide], 'pdf', OPTIONS, 'C:\\out\\w.pdf')).resolves.toEqual(['C:\\out\\w.pdf']);
    await expect(api.exportNotes([wide], 'svg', OPTIONS, 'C:\\out\\w.svg')).resolves.toEqual(['C:\\out\\w.svg']);
  });

  it('can cancel an export under way: it rejects with `cancelled`', async () => {
    mockExport.delayMs = 1000;
    try {
      const running = api.exportNotes([{ path: pathOf('Liste de courses') }], 'png', OPTIONS, 'C:\\out\\a.png');
      await api.cancelExport();
      expect(await code(running)).toBe('cancelled');
      // With nothing running, cancelling does nothing; the next export is not affected.
      await expect(api.cancelExport()).resolves.toBeUndefined();
      mockExport.delayMs = 5;
      await expect(api.exportNotes([{ path: pathOf('Liste de courses') }], 'png', OPTIONS, 'C:\\out\\b.png')).resolves.toEqual([
        'C:\\out\\b.png',
      ]);
    } finally {
      mockExport.delayMs = 0;
    }
  });

  it('copies an image, unless the clipboard is unavailable', async () => {
    const source = { path: pathOf('Liste de courses') };
    await expect(api.copyNoteImage(source, 150)).resolves.toBeUndefined();
    mockClipboard.available = false;
    expect(await code(api.copyNoteImage(source, 150))).toBe('clipboardUnavailable');
  });
});

describe('mock split', () => {
  async function open(title: string) {
    const path = pathOf(title);
    const opened = await api.openNote(path);
    return { path, ...opened };
  }

  it('cuts in time: the new note takes what came after, in the same binder, and listeners hear of it', async () => {
    const { path, note, stamp } = await open('Cours de physique');
    const heard = vi.fn();
    const stop = mockOnLibraryChanged(heard);
    const result = await api.splitNote(path, note, stamp, { kind: 'time', at: 150 }, 'Cours de physique (suite)');
    stop();

    expect(heard).toHaveBeenCalled();
    // Each of the two strokes has 60 samples, 5 ms apart: 30 before 150 ms, 30 from then on.
    expect(result.note.strokes.map((s) => s.points.length)).toEqual([30, 30]);
    expect(result.created.title).toBe('Cours de physique (suite)');
    expect(result.created.binder).toBe('Classe/3e B');
    const created = mockLibrary().notes.get(result.created.path)!.note;
    expect(created.strokes.map((s) => s.points.length)).toEqual([30, 30]);
    expect(created.strokes[0].points[0].t).toBe(0);
    expect(created.created).toBe(note.created + 150);
    expect(created.id).not.toBe(note.id);
    // The original keeps its identity and takes the new stamp.
    const kept = mockLibrary().notes.get(path)!;
    expect(kept.note.id).toBe(note.id);
    expect(kept.stamp).toEqual(result.stamp);
    expect(kept.stamp).not.toEqual(stamp);
  });

  it('cuts by zone like the lasso does', async () => {
    const { path, note, stamp } = await open('Cours de physique');
    const result = await api.splitNote(
      path,
      note,
      stamp,
      {
        kind: 'zone',
        polygon: [
          [1000, 2000],
          [23000, 2000],
          [23000, 4500],
          [1000, 4500],
        ],
      },
      'Extrait',
    );
    expect(result.note.strokes).toHaveLength(1);
    expect(result.note.strokes[0].tool).toBe('fountainPen');
    expect(mockLibrary().notes.get(result.created.path)!.note.strokes[0].tool).toBe('ballpoint');
  });

  it('refuses an empty side, a stale stamp and a missing note without changing anything', async () => {
    const { path, note, stamp } = await open('Cours de physique');
    const before = mockLibrary().notes.size;
    expect(await code(api.splitNote(path, note, stamp, { kind: 'time', at: 0 }, 'X'))).toBe('nothingToSplit');
    expect(await code(api.splitNote(path, note, freshStamp(), { kind: 'time', at: 150 }, 'X'))).toBe('noteConflict');
    expect(await code(api.splitNote('C:\\nope.ardoise', note, stamp, { kind: 'time', at: 150 }, 'X'))).toBe('noteNotFound');
    expect(mockLibrary().notes.size).toBe(before);
    expect(mockLibrary().notes.get(path)!.stamp).toEqual(stamp);
  });

  it('keeps a title with forbidden characters (trimmed) and cleans only the file name', async () => {
    const { path, note, stamp } = await open('Cours de physique');
    const result = await api.splitNote(path, note, stamp, { kind: 'time', at: 150 }, '  Suite: a/b "c"?  ');
    expect(result.created.title).toBe('Suite: a/b "c"?');
    expect(mockLibrary().notes.get(result.created.path)!.note.title).toBe('Suite: a/b "c"?');
    expect(result.created.fileName).toBe("2026-10-20 Suite- a-b 'c'.ardoise");
  });

  it('splits the editor note, not the stored one: unsaved changes are saved with the cut', async () => {
    const { path, note, stamp } = await open('Cours de physique');
    const edited: Note = { ...note, strokes: [...note.strokes, hline(1000, 1900, 9000)] };
    const result = await api.splitNote(path, edited, stamp, { kind: 'time', at: 150 }, 'Suite');
    expect(result.note.strokes).toHaveLength(3);
  });
});

describe('mock merge', () => {
  const a: Note = { ...note([hline(1000, 1500, 1000)], 1, { width: 10000, height: 5000 }), id: 'a', title: 'Alpha', tags: ['x'] };
  const b: Note = {
    ...note([hline(2000, 2500, 3000)], 2, { width: 8000, height: 7000 }),
    id: 'b',
    title: 'Beta',
    tags: ['y', 'x'],
    timing: 'synthesized',
  };

  it('overlays: one page as large as the largest, layers appended and named after the notes', () => {
    const merged = mergeNotesLogic([a, b], 'overlay');
    expect(merged.page).toEqual({ width: 10000, height: 7000 });
    expect(merged.layers.map((l) => l.name)).toEqual(['Alpha', 'Beta - L1', 'Beta - L2']);
    expect(merged.strokes.map((s) => s.layer)).toEqual([0, 1]);
    expect(merged.strokes[1].points[0].y).toBe(3000);
    expect(merged.tags).toEqual(['x', 'y']);
    expect(merged.timing).toBe('synthesized');
    expect(merged.origin).toEqual({ kind: 'ardoise' });
  });

  it('stacks end to end: a long page, strokes shifted down, layers merged by index', () => {
    const merged = mergeNotesLogic([a, b], 'endToEnd');
    expect(merged.page).toEqual({ width: 10000, height: 12000 });
    expect(merged.layers.map((l) => l.name)).toEqual(['L1', 'L2']);
    expect(merged.strokes[1].points[0].y).toBe(5000 + 3000);
  });

  it('makes the times sequential, one second between notes', () => {
    const merged = mergeNotesLogic([a, b], 'overlay');
    const last = (i: number) => Math.max(...merged.strokes[i].points.map((p) => p.t));
    const first = (i: number) => Math.min(...merged.strokes[i].points.map((p) => p.t));
    expect(first(0)).toBe(0);
    expect(first(1)).toBe(last(0) + 1000);
  });

  it('does not let a note without samples move the times of the next ones', () => {
    const empty: Note = { ...note([]), id: 'e', title: 'Vide' };
    const shifted = (s: Note): Note => ({
      ...s,
      strokes: s.strokes.map((st) => ({ ...st, points: st.points.map((p) => ({ ...p, t: p.t + 500 })) })),
    });
    const firstTimes = (merged: Note) => merged.strokes.map((s) => s.points[0].t);
    // An empty first note: the next one keeps its own times.
    expect(firstTimes(mergeNotesLogic([empty, shifted(a)], 'overlay'))).toEqual([500]);
    // An empty note in the middle: the third note still starts 1 s after the first one ends.
    const withHole = mergeNotesLogic([a, empty, b], 'overlay');
    const withoutHole = mergeNotesLogic([a, b], 'overlay');
    expect(firstTimes(withHole)).toEqual(firstTimes(withoutHole));
  });

  it('normalises the tags of the merged notes (spaces, case-insensitive duplicates, first spelling)', () => {
    const tagged = (tags: string[]): Note => ({ ...a, tags });
    const merged = mergeNotesLogic([tagged(['  Maths  ', 'a   b']), tagged(['maths', 'A B', 'Physique', ''])], 'overlay');
    expect(merged.tags).toEqual(['Maths', 'a b', 'Physique']);
  });

  it('accepts a title with forbidden characters and trims it', async () => {
    const paths = [pathOf('Cours de physique'), pathOf('Liste de courses')];
    const created = await api.mergeNotes(paths, 'overlay', '  A/B: "C"  ');
    expect(created.title).toBe('A/B: "C"');
    expect(created.fileName).toBe("2026-10-20 A-B- 'C'.ardoise");
  });

  it('needs two notes and refuses pages above 100 m', () => {
    expect(thrownCode(() => mergeNotesLogic([a], 'overlay'))).toBe('nothingToMerge');
    const huge: Note = { ...a, page: { width: 6_000_000, height: 6_000_000 } };
    expect(thrownCode(() => mergeNotesLogic([huge, huge], 'overlay'))).toBeUndefined();
    expect(thrownCode(() => mergeNotesLogic([huge, huge], 'endToEnd'))).toBe('pageTooLarge');
  });

  it('creates the note in the first one\'s binder and leaves the sources alone', async () => {
    const paths = [pathOf('Cours de physique'), pathOf('Liste de courses')];
    const before = structuredClone([...mockLibrary().notes.values()].map((s) => s.note));
    const heard = vi.fn();
    const stop = mockOnLibraryChanged(heard);
    const created = await api.mergeNotes(paths, 'endToEnd', 'Les deux');
    stop();
    expect(heard).toHaveBeenCalled();
    expect(created.title).toBe('Les deux');
    expect(created.binder).toBe('Classe/3e B');
    const stored = mockLibrary().notes.get(created.path)!.note;
    expect(stored.strokes).toHaveLength(3);
    expect(stored.page.height).toBe(43200);
    expect([...mockLibrary().notes.values()].slice(0, 3).map((s) => s.note)).toEqual(before);
    expect(await code(api.mergeNotes([paths[0]], 'overlay', 'Seule'))).toBe('nothingToMerge');
    expect(await code(api.mergeNotes([paths[0], 'C:\\nope.ardoise'], 'overlay', 'X'))).toBe('noteNotFound');
  });
});

describe('mock automatic export', () => {
  it('stores the format and a normalised folder, and returns the new state', async () => {
    const state = await api.setAutoExport({ format: 'pdf', folder: ' /Exports\\PDF/ ' });
    expect(state.autoExport).toEqual({ format: 'pdf', folder: 'Exports/PDF' });
    expect((await api.appState()).autoExport.format).toBe('pdf');
    await api.setAutoExport({ format: 'off', folder: 'Exports' });
  });

  it('refuses folders outside the library or inside its own _ardoise folder', async () => {
    for (const folder of ['', ' / ', '..', 'a/../b', 'C:/x', '_ardoise', '_Ardoise/x', '.ardoise']) {
      expect(await code(api.setAutoExport({ format: 'png', folder })), folder).toBe('invalidFolder');
    }
    expect(checkAutoExport({ format: 'off', folder: 'My _ardoise' }).folder).toBe('My _ardoise');
    expect((await api.appState()).autoExport.folder).toBe('Exports');
  });
});
