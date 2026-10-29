import { beforeEach, describe, expect, it } from 'vitest';
import type { InkspaceProgress, InkspaceReport, InkspaceSource } from './inkspace';
import { mockLibrary, resetMockLibrary } from './mock-data';
import { mockInkspace, mockInkspaceInvoke, resetMockInkspace } from './mock-inkspace';

const FOUND = mockInkspace.detected!;

const inspect = (path: string) => mockInkspaceInvoke('inkspace_inspect', { path }) as InkspaceSource;
const importNotes = (binder: string, onProgress?: (progress: InkspaceProgress) => void) =>
  mockInkspaceInvoke('inkspace_import', { path: FOUND, binder, onProgress }) as Promise<InkspaceReport>;

beforeEach(() => {
  resetMockLibrary();
  resetMockInkspace();
  mockInkspace.stepMs = 0;
});

describe('Inkspace import in the browser stand-in', () => {
  it('detects a fake Inkspace and counts its notes', () => {
    expect(mockInkspaceInvoke('inkspace_detect', {})).toBe(FOUND);
    expect(inspect(FOUND)).toEqual({ path: FOUND, notes: 5, alreadyImported: 0, unreadable: 1 });
    mockInkspace.detected = null;
    expect(mockInkspaceInvoke('inkspace_detect', {})).toBeNull();
  });

  it('accepts the folders around LocalState and refuses any other', () => {
    expect(inspect('D:\\Copie\\LocalState\\WacomCloudDb').path).toBe('D:\\Copie\\LocalState');
    expect(inspect('E:\\Sauvegarde\\WacomCloudDb\\_').path).toBe(FOUND);
    expect(() => inspect('C:\\Users\\demo\\Documents')).toThrow(
      expect.objectContaining({ code: 'inkspaceNotFound', message: 'C:\\Users\\demo\\Documents' }),
    );
  });

  it('imports the readable notes with progress, reports the damaged one, and skips them the second time', async () => {
    const before = mockLibrary().notes.size;
    const steps: InkspaceProgress[] = [];
    const first = await importNotes('Inkspace', (p) => steps.push(p));
    expect(steps.map((s) => s.done)).toEqual([0, 1, 2, 3, 4]);
    expect(steps.every((s) => s.total === 4)).toBe(true);
    expect(first).toMatchObject({ binder: 'Inkspace', total: 5, imported: 4, skipped: 0, cancelled: false });
    expect(first.failed).toHaveLength(1);
    expect(first.failed[0]).toMatchObject({ code: 'inkspaceNoteDamaged' });
    expect(mockLibrary().notes.size).toBe(before + 4);
    expect(mockLibrary().binders.has('Inkspace')).toBe(true);
    expect(inspect(FOUND).alreadyImported).toBe(4);

    const second = await importNotes('Inkspace');
    expect(second).toMatchObject({ imported: 0, skipped: 4, cancelled: false });
    expect(second.failed).toHaveLength(1);
    expect(mockLibrary().notes.size).toBe(before + 4);
  });

  it('starts afresh with a new library', async () => {
    await importNotes('');
    expect(inspect(FOUND).alreadyImported).toBe(4);
    resetMockLibrary();
    expect(inspect(FOUND).alreadyImported).toBe(0);
  });

  it('stops before the next note, keeps those imported, refuses a second import meanwhile', async () => {
    let release!: () => void;
    mockInkspace.hold = { before: 2, until: new Promise<void>((resolve) => (release = resolve)) };
    let reached!: () => void;
    const twoDone = new Promise<void>((resolve) => (reached = resolve));
    const running = importNotes('Inkspace', (p) => p.done === 2 && reached());
    await expect(importNotes('Inkspace')).rejects.toMatchObject({ code: 'inkspaceBusy' });
    await twoDone;
    mockInkspaceInvoke('inkspace_cancel', {});
    release();
    const report = await running;
    expect(report.cancelled).toBe(true);
    expect(report.imported).toBe(2);
    expect(inspect(FOUND).alreadyImported).toBe(2);
    // Imports can run again once stopped.
    const rest = await importNotes('Inkspace');
    expect(rest).toMatchObject({ imported: 2, skipped: 2, cancelled: false });
  });

  it('refuses an unusable binder name', async () => {
    await expect(importNotes('a:b')).rejects.toMatchObject({ code: 'invalidName' });
    await expect(importNotes('')).resolves.toMatchObject({ binder: '', imported: 4 });
  });
});
