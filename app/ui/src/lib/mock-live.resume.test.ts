// Resuming live mode on an existing sheet (M12): the browser stand-in follows the Rust side
// (docs/design/tablet-live-classroom.md, "M12").
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { recoveryOf } from './editor/drafts';
import type { LiveBatch, LiveEvent, LiveSnapshot, LiveStopped } from './live';
import { freshStamp, mockLibrary, resetMockLibrary } from './mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveSubscribe, resetMockLive, setMockLiveOrientation } from './mock-live';

/** Subscribes and records every batch. */
async function subscribe() {
  const batches: LiveBatch[] = [];
  const first = await mockLiveSubscribe((b) => batches.push(b));
  const events = () => batches.flatMap((b) => b.events);
  const snapshots = () => events().flatMap((e) => (e.type === 'snapshot' ? [e.snapshot] : []));
  const last = () => snapshots().at(-1);
  return { first, batches, events, snapshots, last };
}

const invoke = (command: string, args: Record<string, unknown> = {}) => mockLiveInvoke(command, args);
type Forked = Extract<LiveEvent, { type: 'sheetForked' }>;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(Date.UTC(2026, 9, 24, 9, 0));
  resetMockLive();
  resetMockLibrary();
});

afterEach(() => {
  resetMockLive();
  vi.useRealTimers();
});

const sheetPath = () => [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];
const resume = (path = sheetPath()) => invoke('live_resume', { path }) as LiveSnapshot;
const stop = () => invoke('live_stop') as LiveStopped;

describe('browser stand-in: resuming a sheet', () => {
  it('opens the note as the first page, never turned, and starts a new note at the next page', async () => {
    mockLiveConfig.waitMs = 0;
    const sub = await subscribe();
    const stored = mockLibrary().notes.get(sheetPath())!;
    const snapshot = resume();
    expect(snapshot.state).toBe('waiting');
    expect(snapshot.page.id).toBe(stored.note.id);
    expect(snapshot.page.strokes).toHaveLength(stored.note.strokes.length);
    expect(snapshot.pageCount).toBe(1);
    expect(snapshot.canTurnPage).toBe(false);
    expect(snapshot.orientation).toBe('landscape');
    // A resumed sheet is not a written page: nothing is counted as saved yet.
    expect(snapshot.saved).toEqual([]);

    invoke('live_command', { command: { kind: 'nextPage' } });
    const next = sub.last()!;
    expect(next.pageIndex).toBe(1);
    expect(next.page.id).not.toBe(stored.note.id);
    expect(next.canTurnPage).toBe(true);
    invoke('live_command', { command: { kind: 'previousPage' } });
    expect(sub.last()?.canTurnPage).toBe(false);
  });

  it('writes into the same note, keeps the older ink and continues after the last stroke', () => {
    mockLiveConfig.waitMs = 0;
    const path = sheetPath();
    const before = mockLibrary().notes.get(path)!;
    const strokes = before.note.strokes.length;
    const lastT = Math.max(...before.note.strokes.flatMap((s) => s.points.map((p) => p.t)));
    const notes = mockLibrary().notes.size;
    const stamp = before.stamp;
    resume();
    // Untouched, the sheet is not written.
    expect(mockLibrary().notes.get(path)!.stamp).toBe(stamp);
    vi.advanceTimersByTime(15 * 200);
    expect(stop()).toEqual({ saved: [path], forks: [], narration: null });
    const after = mockLibrary().notes.get(path)!;
    expect(mockLibrary().notes.size).toBe(notes);
    expect(after.stamp).not.toBe(stamp);
    expect(after.note.strokes.length).toBeGreaterThan(strokes);
    expect(after.note.strokes.slice(0, strokes)).toEqual(before.note.strokes);
    expect(after.note.strokes[strokes].points[0].t).toBeGreaterThan(lastT);
    expect(after.note.title).toBe('Cours de physique');
    expect(after.binder).toBe(before.binder);
  });

  it('leaves the sheet as it is when it stops without writing, and counts nothing', () => {
    const path = sheetPath();
    const stamp = mockLibrary().notes.get(path)!.stamp;
    resume();
    expect(stop()).toEqual({ saved: [], forks: [], narration: null });
    expect(mockLibrary().notes.get(path)!.stamp).toBe(stamp);
  });

  it('leaves the sheet as it is when only the blackboard was pressed', () => {
    const path = sheetPath();
    const stored = mockLibrary().notes.get(path)!;
    stored.note.background.paper = 'kraft';
    const stamp = stored.stamp;
    const snapshot = resume();
    invoke('live_set_view', { view: { ...snapshot.view, board: true } });
    expect(stop().saved).toEqual([]);
    expect(mockLibrary().notes.get(path)!.stamp).toBe(stamp);
    expect(mockLibrary().notes.get(path)!.note.background.paper).toBe('kraft');
  });

  it('gives a sheet its own paper back, and writes slate only with ink under the blackboard', () => {
    mockLiveConfig.waitMs = 0;
    const path = sheetPath();
    mockLibrary().notes.get(path)!.note.background.paper = 'kraft';
    const snapshot = resume();
    invoke('live_set_view', { view: { ...snapshot.view, board: true } });
    invoke('live_set_view', { view: { ...snapshot.view, board: false } });
    vi.advanceTimersByTime(15 * 200);
    stop();
    expect(mockLibrary().notes.get(path)!.note.background.paper).toBe('kraft');

    const again = resume();
    invoke('live_set_view', { view: { ...again.view, board: true } });
    vi.advanceTimersByTime(15 * 200);
    stop();
    expect(mockLibrary().notes.get(path)!.note.background.paper).toBe('slate');
  });

  it('keeps a blank sheet as the original of what is written on it', () => {
    mockLiveConfig.waitMs = 0;
    const path = sheetPath();
    const stored = mockLibrary().notes.get(path)!;
    stored.note.strokes = [];
    stored.original = undefined;
    resume();
    vi.advanceTimersByTime(15 * 200);
    stop();
    const written = mockLibrary().notes.get(path)!;
    expect(written.note.strokes.length).toBeGreaterThan(0);
    expect(written.original).toMatchObject({ source: 'firstVersion', strokes: [] });
  });

  it('holds a portrait sheet on the same side and keeps its ink when the position is switched', async () => {
    const stored = mockLibrary().notes.get(sheetPath())!;
    stored.note.page = { width: 21600, height: 29700 };
    const sub = await subscribe();
    expect(resume().orientation).toBe('portraitLeft');
    setMockLiveOrientation('landscapeFlipped');
    expect(sub.last()?.orientation).toBe('portraitRight');
    expect(sub.last()?.page.page).toEqual({ width: 21600, height: 29700 });
    expect(sub.last()?.page.strokes).toEqual(stored.note.strokes);
    expect(sub.last()?.canTurnPage).toBe(false);
    // The setting now agrees with the sheet: it is held as the setting says.
    setMockLiveOrientation('portraitRight');
    expect(sub.last()?.orientation).toBe('portraitRight');
    expect(sub.last()?.page.strokes).toEqual(stored.note.strokes);
  });

  it('refuses a note it cannot read, and a running session goes on', async () => {
    const sub = await subscribe();
    invoke('live_start');
    expect(() => invoke('live_resume', { path: `${mockLibrary().root}\\Nulle part.ardoise` })).toThrow(
      expect.objectContaining({ code: 'noteNotFound' }),
    );
    expect(() => invoke('live_resume', { path: 'D:\\ailleurs\\x.ardoise' })).toThrow(expect.objectContaining({ code: 'notInLibrary' }));
    mockLiveConfig.resumeError = 'noteNewerVersion';
    expect(() => resume()).toThrow(expect.objectContaining({ code: 'noteNewerVersion' }));
    mockLiveConfig.resumeError = 'noteDamaged';
    expect(() => resume()).toThrow(expect.objectContaining({ code: 'noteDamaged' }));
    expect(sub.last()?.state).toBe('waiting');
    expect(sub.last()?.canTurnPage).toBe(true);
  });

  it('ends a running session as live_stop does before it resumes', () => {
    mockLiveConfig.waitMs = 0;
    const before = mockLibrary().notes.size;
    invoke('live_start');
    vi.advanceTimersByTime(15 * 150);
    const snapshot = resume();
    expect(mockLibrary().notes.size).toBe(before + 1);
    expect(snapshot.pageCount).toBe(1);
    expect(snapshot.canTurnPage).toBe(false);
  });

  it('starts the blackboard only on a slate sheet', () => {
    const path = sheetPath();
    expect(resume(path).view.board).toBe(false);
    invoke('live_stop');
    mockLibrary().notes.get(path)!.note.background.paper = 'slate';
    const snapshot = resume(path);
    expect(snapshot.view.board).toBe(true);
    expect(snapshot.style.tool).toBe('chalk');
  });

  it('forks when the sheet changed elsewhere: a new note beside it, told once, even to a late window', async () => {
    mockLiveConfig.waitMs = 0;
    const path = sheetPath();
    const binder = mockLibrary().notes.get(path)!.binder;
    const sub = await subscribe();
    resume();
    // Another PC saves the note (Nextcloud brought it here).
    const outside = mockLibrary().notes.get(path)!;
    mockLibrary().notes.set(path, { ...outside, note: { ...outside.note, title: 'Autre' }, stamp: freshStamp(2000) });
    const outsideStamp = mockLibrary().notes.get(path)!.stamp;
    vi.advanceTimersByTime(15 * 200);
    const forks = sub.events().filter((e): e is Forked => e.type === 'sheetForked');
    expect(forks).toHaveLength(1);
    const fork = forks[0];
    expect(fork).toMatchObject({ pageIndex: 0, reason: 'changed', original: path });
    expect(fork.path).not.toBe(path);
    // The original is as the other PC left it; the new note holds the writing, in the same binder.
    expect(mockLibrary().notes.get(path)!.stamp).toBe(outsideStamp);
    expect(mockLibrary().notes.get(path)!.note.title).toBe('Autre');
    const forked = mockLibrary().notes.get(fork.path)!;
    expect(forked.binder).toBe(binder);
    expect(forked.note.strokes.length).toBeGreaterThan(outside.note.strokes.length);
    // The next writes go on in the new note, and nothing is told again.
    vi.advanceTimersByTime(15 * 100);
    invoke('live_command', { command: { kind: 'newPage' } });
    expect(sub.events().filter((e) => e.type === 'sheetForked')).toHaveLength(1);
    expect(mockLibrary().notes.get(path)!.stamp).toBe(outsideStamp);

    // A window that subscribes later is told too.
    const late: LiveBatch[] = [];
    await mockLiveSubscribe((b) => late.push(b));
    await Promise.resolve();
    expect(late.flatMap((b) => b.events).filter((e) => e.type === 'sheetForked')).toEqual([fork]);

    // The stop says so as well: the view closes on it.
    const { type: _type, ...told } = fork;
    expect(stop().forks).toEqual([told]);

    // The view of the next lesson is not told.
    const next: LiveBatch[] = [];
    await mockLiveSubscribe((b) => next.push(b));
    await Promise.resolve();
    expect(next.flatMap((b) => b.events).filter((e) => e.type === 'sheetForked')).toEqual([]);
  });

  it('forks into a new note when the sheet was deleted meanwhile', async () => {
    mockLiveConfig.waitMs = 0;
    const path = sheetPath();
    const binder = mockLibrary().notes.get(path)!.binder;
    const sub = await subscribe();
    resume();
    mockLibrary().notes.delete(path);
    vi.advanceTimersByTime(15 * 200);
    const fork = sub.events().find((e): e is Forked => e.type === 'sheetForked')!;
    expect(fork).toMatchObject({ reason: 'deleted', original: path });
    // Written again, in the binder it was in (under its old name when that one is free).
    expect(mockLibrary().notes.get(fork.path)!.note.strokes.length).toBeGreaterThan(0);
    expect(mockLibrary().notes.get(fork.path)!.binder).toBe(binder);
  });

  it('a draft left for the note is offered only as a copy once live mode wrote it', () => {
    mockLiveConfig.waitMs = 0;
    const path = sheetPath();
    const stored = mockLibrary().notes.get(path)!;
    const draft = {
      id: stored.note.id,
      path,
      title: stored.note.title,
      savedAt: 1,
      baseStamp: stored.stamp,
      hasImage: false,
      note: { ...stored.note, title: 'Brouillon' },
    };
    expect(recoveryOf(draft, { note: stored.note, stamp: stored.stamp })).toBe('restore');
    resume();
    vi.advanceTimersByTime(15 * 200);
    invoke('live_stop');
    const written = mockLibrary().notes.get(path)!;
    expect(recoveryOf(draft, { note: written.note, stamp: written.stamp })).toBe('copy');
  });
});
