import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveSnapshot, LiveStrokesEdit, LiveStrokesEdited } from './live';
import { LiveState } from './live-state.svelte';
import { resetMockLibrary } from './mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveSubscribe, mockLiveTick, resetMockLive, setMockLiveOrientation } from './mock-live';
import type { Rgba, Stroke } from './note';

const RED: Rgba = { r: 216, g: 0, b: 0, a: 255 };
const invoke = (command: string, args: Record<string, unknown> = {}) => mockLiveInvoke(command, args);

/** A window (`LiveState`) on a running session, whose pen is driven step by step. */
async function open() {
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 0;
  const live = new LiveState();
  const batches: LiveBatch[] = [];
  const snapshot = await mockLiveSubscribe((batch) => {
    batches.push(batch);
    live.receive(batch);
  });
  live.initial(snapshot);
  invoke('live_start');
  vi.advanceTimersByTime(1);
  const edits = () =>
    batches.flatMap((batch) => batch.events).filter((event): event is LiveStrokesEdited => event.type === 'strokesEdited');
  /** The page as Rust would give it. */
  const page = () => (invoke('live_start') as LiveSnapshot).page;
  const edit = (edit: LiveStrokesEdit, revision = live.revision, pageIndex = live.pageIndex) =>
    invoke('live_edit_strokes', { pageIndex, revision, edit }) as number;
  const undo = () => invoke('live_undo_edit', { pageIndex: live.pageIndex }) as boolean;
  const redo = () => invoke('live_redo_edit', { pageIndex: live.pageIndex }) as boolean;
  /** The pen writes until the page holds `count` finished strokes. */
  const writeUntil = (count: number) => {
    for (let i = 0; live.strokes.length < count; i++) {
      if (i > 5000) throw new Error('the pen does not write');
      mockLiveTick();
    }
  };
  return { live, batches, edits, page, edit, undo, redo, writeUntil };
}

const startsOf = (strokes: readonly Stroke[]) => strokes.map((s) => s.points[0].x);

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(Date.UTC(2026, 10, 4, 9, 0));
  resetMockLive();
  resetMockLibrary();
});

afterEach(() => {
  resetMockLive();
  vi.useRealTimers();
});

describe('browser stand-in: touching up during live mode', () => {
  it('deletes, and a window has the same list; the tablet does not change the revision', async () => {
    const { live, edits, page, edit, writeUntil } = await open();
    writeUntil(4);
    const atStart = live.revision;
    expect(atStart).toBeGreaterThan(0);
    writeUntil(5);
    expect(live.revision).toBe(atStart);

    const before = startsOf(live.strokes);
    const objects = [...live.strokes];
    const generation = live.generation;
    const revision = edit({ kind: 'delete', indices: [3, 0] });
    expect(revision).not.toBe(atStart);
    expect(live.revision).toBe(revision);
    expect(edits().at(-1)).toMatchObject({ removed: [0, 3], inserted: [], recoloured: [], canUndo: true, canRedo: false });
    expect(startsOf(live.strokes)).toEqual([before[1], before[2], before[4]]);
    expect(live.strokes[0]).toBe(objects[1]);
    expect(live.generation).toBe(generation + 1);
    expect(page().strokes).toEqual(live.strokes);
  });

  it('recolours without changing the revision, and a colour already there is no step', async () => {
    const { live, edits, page, edit, undo, writeUntil } = await open();
    writeUntil(3);
    const revision = live.revision;
    expect(edit({ kind: 'recolour', indices: [1], color: RED })).toBe(revision);
    expect(live.strokes[1].color).toEqual(RED);
    expect(live.canUndo).toBe(true);
    // Twice in a row on the swatches: not stale, no second step.
    const count = edits().length;
    expect(edit({ kind: 'recolour', indices: [1], color: RED }, revision)).toBe(revision);
    expect(edits()).toHaveLength(count);
    expect(undo()).toBe(true);
    expect(undo()).toBe(false);
    expect(live.strokes[1].color).toEqual({ r: 0, g: 0, b: 0, a: 255 });
    expect(page().strokes).toEqual(live.strokes);
  });

  it('undoes and redoes, what the tablet wrote since staying after the places given back', async () => {
    const { live, page, edit, undo, redo, writeUntil } = await open();
    writeUntil(4);
    const first = startsOf(live.strokes);
    edit({ kind: 'delete', indices: [0, 1] });
    writeUntil(4);
    const written = startsOf(live.strokes).slice(2);
    expect(undo()).toBe(true);
    expect(live.canRedo).toBe(true);
    expect(startsOf(live.strokes)).toEqual([...first, ...written]);
    writeUntil(7);
    const more = startsOf(live.strokes).slice(6);
    expect(redo()).toBe(true);
    expect(startsOf(live.strokes)).toEqual([first[2], first[3], ...written, ...more]);
    expect(redo()).toBe(false);
    expect(page().strokes).toEqual(live.strokes);
  });

  it('refuses a stale page or revision, and whatever makes no sense, changing nothing', async () => {
    const { live, edit, undo, writeUntil } = await open();
    writeUntil(3);
    const revision = live.revision;
    edit({ kind: 'delete', indices: [0] });
    const stale = expect.objectContaining({ code: 'liveEditStale' });
    const invalid = expect.objectContaining({ code: 'liveEditInvalid' });
    expect(() => edit({ kind: 'delete', indices: [0] }, revision)).toThrow();
    try {
      edit({ kind: 'delete', indices: [0] }, revision);
    } catch (error) {
      expect(error).toEqual(stale);
    }
    const refused = (edited: LiveStrokesEdit) => {
      let error: unknown;
      try {
        edit(edited);
      } catch (e) {
        error = e;
      }
      expect(error).toEqual(invalid);
    };
    const count = live.strokes.length;
    refused({ kind: 'delete', indices: [] });
    refused({ kind: 'delete', indices: [count] });
    refused({ kind: 'delete', indices: [-1] });
    refused({ kind: 'delete', indices: [0.5] });
    refused({ kind: 'delete', indices: [0, 0] });
    refused({ kind: 'recolour', indices: [0], color: { r: 300, g: 0, b: 0, a: 255 } });
    refused({ kind: 'recolour', indices: [0], color: { r: 0, g: 0, b: 0, a: 0 } });
    expect(live.strokes).toHaveLength(count);

    // Another page: refused, and its history waits for it.
    invoke('live_command', { command: { kind: 'newPage' } });
    let error: unknown;
    try {
      invoke('live_undo_edit', { pageIndex: 0 });
    } catch (e) {
      error = e;
    }
    expect(error).toEqual(stale);
    expect(live.canUndo).toBe(false);
    expect(undo()).toBe(false);
    invoke('live_command', { command: { kind: 'previousPage' } });
    expect(live.canUndo).toBe(true);
    expect(undo()).toBe(true);

    // Stopped: as for the other commands.
    invoke('live_stop');
    expect(() => invoke('live_undo_edit', { pageIndex: 0 })).toThrow();
  });

  it('puts « Clear the page » in the history, its button and Ctrl+Z alike', async () => {
    const { live, edit, undo, redo, writeUntil } = await open();
    writeUntil(3);
    const first = startsOf(live.strokes);
    const before = live.revision;
    invoke('live_command', { command: { kind: 'clearPage' } });
    expect(live.strokes).toEqual([]);
    expect(live.revision).not.toBe(before);
    expect(live.canUndo).toBe(true);
    writeUntil(1);
    const written = startsOf(live.strokes);
    expect(undo()).toBe(true);
    expect(startsOf(live.strokes)).toEqual([...first, ...written]);
    expect(redo()).toBe(true);
    expect(startsOf(live.strokes)).toEqual(written);
    // The button goes through the history too, what came after the clearing first.
    edit({ kind: 'recolour', indices: [0], color: RED });
    expect(invoke('live_restore_cleared')).toBe(true);
    expect(startsOf(live.strokes)).toEqual([...first, ...written]);
    expect(live.strokes.at(-1)?.color).toEqual({ r: 0, g: 0, b: 0, a: 255 });
    expect(invoke('live_restore_cleared')).toBe(false);
    expect(live.canRedo).toBe(true);
  });

  it('turns what can come back with the page', async () => {
    const { live, edit, undo, page, writeUntil } = await open();
    writeUntil(2);
    const first = live.strokes[0].points[0];
    edit({ kind: 'delete', indices: [0] });
    setMockLiveOrientation('portraitLeft');
    expect(undo()).toBe(true);
    // A quarter turn clockwise: (x, y) -> (height - y, x).
    expect(live.strokes[0].points[0].x).toBe(21600 - first.y);
    expect(live.strokes[0].points[0].y).toBe(first.x);
    expect(page().strokes).toEqual(live.strokes);
  });

  it('keeps a stroke in progress out of the list while edits arrive', async () => {
    const { live, edit, undo, page, writeUntil } = await open();
    writeUntil(2);
    // Writing again: stops on a tick where the pen is down.
    for (let i = 0; !live.current; i++) {
      if (i > 500) throw new Error('the pen does not write');
      mockLiveTick();
    }
    const current = live.current;
    edit({ kind: 'delete', indices: [0] });
    expect(live.current).toBe(current);
    expect(undo()).toBe(true);
    expect(live.current).toBe(current);
    expect(page().strokes).toEqual(live.strokes);
    writeUntil(3);
    expect(page().strokes).toEqual(live.strokes);
  });

  it('gives a window that subscribes later the page, its revision and its history', async () => {
    const { live, edit, writeUntil } = await open();
    writeUntil(2);
    const revision = edit({ kind: 'delete', indices: [0] });
    const late = new LiveState();
    late.initial(await mockLiveSubscribe((batch) => late.receive(batch)));
    expect(late.revision).toBe(revision);
    expect(late.canUndo).toBe(true);
    expect(late.strokes).toEqual(live.strokes);
    invoke('live_undo_edit', { pageIndex: 0 });
    expect(late.strokes).toEqual(live.strokes);
    expect(late.revision).toBe(live.revision);
  });
});
