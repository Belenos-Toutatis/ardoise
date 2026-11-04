// Milestone M12: what the control window asks of the session for the selection (delete, recolour,
// undo, redo), against the browser stand-in, which follows the rules of the Rust session.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveStrokesEdit } from './live';
import { LiveSelection } from './live-select.svelte';
import { LiveState } from './live-state.svelte';
import { deleteSelected, recolourSelected, stepHistory, type TouchUp, type TouchUpCommands } from './live-touch-up';
import { resetMockLibrary } from './mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveSubscribe, mockLiveTick, resetMockLive } from './mock-live';
import type { Rgba } from './note';

const RED: Rgba = { r: 216, g: 0, b: 0, a: 255 };
const invoke = (command: string, args: Record<string, unknown> = {}) => mockLiveInvoke(command, args);

const commands: TouchUpCommands = {
  liveEditStrokes: async (pageIndex, revision, edit: LiveStrokesEdit) =>
    invoke('live_edit_strokes', { pageIndex, revision, edit }) as number,
  liveUndoEdit: async (pageIndex) => invoke('live_undo_edit', { pageIndex }) as boolean,
  liveRedoEdit: async (pageIndex) => invoke('live_redo_edit', { pageIndex }) as boolean,
};

async function open() {
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 0;
  const live = new LiveState();
  const batches: LiveBatch[] = [];
  live.initial(
    await mockLiveSubscribe((batch) => {
      batches.push(batch);
      live.receive(batch);
    }),
  );
  invoke('live_start');
  vi.advanceTimersByTime(1);
  const selection = new LiveSelection();
  const errors: unknown[] = [];
  const onhistory = vi.fn();
  const touch: TouchUp = { live, selection, commands, onerror: (error) => errors.push(error), onhistory };
  const writeUntil = (count: number) => {
    for (let i = 0; live.strokes.length < count; i++) {
      if (i > 5000) throw new Error('the pen does not write');
      mockLiveTick();
    }
  };
  const choose = (indices: number[]) => selection.choose(live, indices, 'replace');
  return { live, touch, selection, errors, onhistory, writeUntil, choose };
}

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

describe('touching up the selection', () => {
  it('deletes the chosen strokes, and the selection goes with them', async () => {
    const { live, touch, selection, errors, writeUntil, choose } = await open();
    writeUntil(5);
    const starts = live.strokes.map((s) => s.points[0].x);
    choose([3, 1]);
    await deleteSelected(touch);
    expect(errors).toEqual([]);
    expect(live.strokes.map((s) => s.points[0].x)).toEqual([starts[0], starts[2], starts[4]]);
    expect(selection.indices).toEqual([]);
    expect(live.canUndo).toBe(true);
  });

  it('does nothing without a selection', async () => {
    const { live, touch, writeUntil } = await open();
    writeUntil(2);
    const edit = vi.spyOn(commands, 'liveEditStrokes');
    await deleteSelected(touch);
    await recolourSelected(touch, RED);
    expect(edit).not.toHaveBeenCalled();
    expect(live.strokes).toHaveLength(2);
    edit.mockRestore();
  });

  it('recolours and keeps the selection, then undo and redo give the colours back and forth', async () => {
    const { live, touch, selection, errors, writeUntil, choose } = await open();
    writeUntil(4);
    const before = live.strokes.map((s) => ({ ...s.color }));
    choose([0, 2]);
    const revision = live.revision;
    await recolourSelected(touch, RED);
    expect(errors).toEqual([]);
    expect(live.strokes.map((s) => s.color)).toEqual([RED, before[1], RED, before[3]]);
    expect(selection.indices).toEqual([0, 2]);
    // A change of colour keeps the revision: the selection is still the page's.
    expect(live.revision).toBe(revision);
    selection.follow(live);
    expect(selection.indices).toEqual([0, 2]);

    await stepHistory(touch);
    expect(live.strokes.map((s) => s.color)).toEqual(before);
    expect(live.canRedo).toBe(true);
    await stepHistory(touch, true);
    expect(live.strokes[0].color).toEqual(RED);
    expect(touch.onhistory).toHaveBeenCalledTimes(2);
  });

  it('undoes a delete (the revision changes: the old indices are dropped) and redoes it', async () => {
    const { live, touch, selection, writeUntil, choose } = await open();
    writeUntil(3);
    choose([1]);
    await deleteSelected(touch);
    expect(live.strokes).toHaveLength(2);
    choose([0]);
    await stepHistory(touch);
    expect(live.strokes).toHaveLength(3);
    selection.follow(live);
    expect(selection.indices).toEqual([]);
    await stepHistory(touch, true);
    expect(live.strokes).toHaveLength(2);
  });

  it('undoes « Clear the page » too, but nothing of what the tablet wrote', async () => {
    const { live, touch, onhistory, writeUntil } = await open();
    writeUntil(3);
    // Nothing was touched up: there is nothing to undo, whatever the tablet wrote.
    expect(live.canUndo).toBe(false);
    await stepHistory(touch);
    expect(onhistory).not.toHaveBeenCalled();
    expect(live.strokes).toHaveLength(3);

    invoke('live_command', { command: { kind: 'clearPage' } });
    expect(live.strokes).toHaveLength(0);
    expect(live.canUndo).toBe(true);
    await stepHistory(touch);
    expect(live.strokes).toHaveLength(3);
    expect(onhistory).toHaveBeenCalledOnce();
  });

  it('a stroke the tablet writes meanwhile does not move the indices', async () => {
    const { live, touch, selection, writeUntil, choose } = await open();
    writeUntil(3);
    const chosen = live.strokes[1];
    choose([1]);
    const revision = live.revision;
    writeUntil(5);
    expect(live.revision).toBe(revision);
    selection.follow(live);
    expect(selection.indices).toEqual([1]);
    expect(live.strokes[1]).toBe(chosen);
    await recolourSelected(touch, RED);
    expect(live.strokes[1].color).toEqual(RED);
    expect(live.strokes[4].color).not.toEqual(RED);
  });

  it('a stale selection is refused, dropped, and reported', async () => {
    const { live, touch, selection, errors, writeUntil, choose } = await open();
    writeUntil(4);
    choose([0, 1]);
    // Something else deleted a stroke after the selection was made (another window, the tablet's button...).
    invoke('live_edit_strokes', { pageIndex: 0, revision: live.revision, edit: { kind: 'delete', indices: [3] } });
    expect(live.revision).not.toBe(selection.revision);
    await deleteSelected(touch);
    expect(live.strokes).toHaveLength(3);
    expect(errors).toEqual([expect.objectContaining({ code: 'liveEditStale' })]);
    expect(selection.indices).toEqual([]);
  });

  it('reports a refused edit that is not stale, and keeps the selection to try again', async () => {
    const { touch, selection, errors, writeUntil, choose } = await open();
    writeUntil(2);
    choose([0, 7]);
    await deleteSelected(touch);
    expect(errors).toEqual([expect.objectContaining({ code: 'liveEditInvalid' })]);
    expect(selection.indices).toEqual([0, 7]);
  });

  it('reports an undo that is refused as stale and drops the selection', async () => {
    const { touch, selection, errors, writeUntil, choose } = await open();
    writeUntil(2);
    choose([0]);
    const stale: TouchUp = {
      ...touch,
      commands: { ...commands, liveUndoEdit: () => Promise.reject({ code: 'liveEditStale', message: 'gone' }) },
    };
    await stepHistory(stale);
    expect(errors).toEqual([expect.objectContaining({ code: 'liveEditStale' })]);
    expect(selection.indices).toEqual([]);
  });
});
