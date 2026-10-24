import { beforeEach, describe, expect, it } from 'vitest';
import { UNHANDLED, freshStamp, mockLibrary, mockOnLibraryChanged, resetMockLibrary, sameStamp } from './mock-data';
import { backgroundDataUrl, mockNotesInvoke } from './mock-notes';
import type { FileStamp, OpenedNote } from './note';

const path = () => [...mockLibrary().notes.keys()].find((p) => p.includes('Liste de courses'))!;

function decode(url: string): string {
  return decodeURIComponent(url.slice(url.indexOf(',') + 1));
}

describe('editor commands in the browser stand-in', () => {
  beforeEach(() => {
    resetMockLibrary();
  });

  it('opens a deep copy of a stored note', () => {
    const opened = mockNotesInvoke('open_note', { path: path() }) as OpenedNote;
    expect(opened.note.title).toBe('Liste de courses');
    opened.note.strokes[0].points[0].x = -1;
    opened.note.title = 'changed';
    expect(mockLibrary().notes.get(path())!.note.strokes[0].points[0].x).not.toBe(-1);
    expect(mockLibrary().notes.get(path())!.note.title).toBe('Liste de courses');
    expect(() => mockNotesInvoke('open_note', { path: 'C:\\nope.ardoise' })).toThrow();
  });

  it('saves with the current stamp, notifies, and refuses a stale one', () => {
    const { note, stamp } = mockNotesInvoke('open_note', { path: path() }) as OpenedNote;
    let notified = 0;
    const stop = mockOnLibraryChanged(() => notified++);
    note.strokes = [];
    const next = mockNotesInvoke('save_note', { path: path(), note, stamp }) as FileStamp;
    stop();
    expect(sameStamp(next, stamp)).toBe(false);
    expect(mockLibrary().notes.get(path())!.note.strokes).toEqual([]);
    expect(notified).toBe(1);
    // The old stamp is stale now.
    expect(() => mockNotesInvoke('save_note', { path: path(), note, stamp })).toThrow(
      expect.objectContaining({ code: 'noteConflict' }),
    );
    expect(() => mockNotesInvoke('save_note', { path: path(), note, stamp: freshStamp() })).toThrow();
    expect(mockNotesInvoke('save_note', { path: path(), note, stamp: next })).toBeTruthy();
  });

  it('renders backgrounds as SVG data URLs with the paper and template', () => {
    const page = { width: 29700, height: 21600 };
    const url = mockNotesInvoke('render_background', {
      page,
      background: { paper: 'cream', template: 'seyes', texture: false },
      widthPx: 1024,
    }) as string;
    expect(url.startsWith('data:image/svg+xml')).toBe(true);
    const svg = decode(url);
    expect(svg).toContain('width="1024"');
    expect(svg).toContain('viewBox="0 0 297 216"');
    expect(svg).toContain('#faf4e2');
    expect((svg.match(/<line /g) ?? []).length).toBeGreaterThan(100);
    const blank = decode(backgroundDataUrl(page, { paper: 'slate', template: 'blank', texture: true }, 512));
    expect(blank).toContain('#2a3230');
    expect(blank).toContain('feTurbulence');
    expect(blank).not.toContain('<line');
    for (const template of ['lined', 'grid5mm', 'millimetric', 'dotted', 'music'] as const) {
      const custom = { custom: { r: 10, g: 20, b: 30, a: 255 } };
      expect(decode(backgroundDataUrl(page, { paper: custom, template, texture: false }, 512))).toMatch(
        /<line |<pattern /,
      );
    }
  });

  it('leaves other commands to the other stand-ins', () => {
    expect(mockNotesInvoke('library_list', {})).toBe(UNHANDLED);
  });
});
