// SPEC §4.2 « Retoucher pendant le direct »: moving and resizing the chosen strokes (`transform`) against the browser
// stand-in, which follows the rules of the Rust session: the points and the width change, nothing else; one
// `strokesEdited` per edit (removed, then inserted at the same places); a change of nothing is no step; the revision
// moves on; the refusals; undo and redo, one step per transform.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveStrokesEdit, LiveStrokesEdited } from './live';
import { LiveState } from './live-state.svelte';
import { resetMockLibrary } from './mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveSubscribe, mockLiveTick, resetMockLive, setMockLiveOrientation } from './mock-live';
import type { Stroke } from './note';

const invoke = (command: string, args: Record<string, unknown> = {}) => mockLiveInvoke(command, args);

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
  const events = () =>
    batches.flatMap((batch) => batch.events).filter((event): event is LiveStrokesEdited => event.type === 'strokesEdited');
  const edit = (edited: LiveStrokesEdit, revision = live.revision) =>
    invoke('live_edit_strokes', { pageIndex: live.pageIndex, revision, edit: edited }) as number;
  const transform = (indices: number[], scale: number, x: number, y: number, revision = live.revision) =>
    edit({ kind: 'transform', indices, scale, x, y }, revision);
  const undo = () => invoke('live_undo_edit', { pageIndex: live.pageIndex }) as boolean;
  const redo = () => invoke('live_redo_edit', { pageIndex: live.pageIndex }) as boolean;
  const writeUntil = (count: number) => {
    for (let i = 0; live.strokes.length < count; i++) {
      if (i > 5000) throw new Error('the pen does not write');
      mockLiveTick();
    }
  };
  return { live, events, edit, transform, undo, redo, writeUntil };
}

/** What a transform does to a stroke, as the contract says. */
const moved = (stroke: Stroke, scale: number, x: number, y: number): Stroke => ({
  ...stroke,
  width: stroke.width * scale,
  points: stroke.points.map((p) => ({ ...p, x: p.x * scale + x, y: p.y * scale + y })),
});

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(Date.UTC(2026, 10, 12, 9, 0));
  resetMockLive();
  resetMockLibrary();
});

afterEach(() => {
  resetMockLive();
  vi.useRealTimers();
});

describe('browser stand-in: moving and resizing strokes', () => {
  it('moves the chosen strokes only, in one event, at the same places, with a new revision', async () => {
    const { live, events, transform, writeUntil } = await open();
    writeUntil(4);
    const before = structuredClone(live.strokes);
    const revision = live.revision;
    const generation = live.generation;
    const answer = transform([2, 0], 1, 300, -150);
    expect(answer).not.toBe(revision);
    expect(live.revision).toBe(answer);
    expect(live.generation).toBe(generation + 1);
    expect(live.strokes[0]).toEqual(moved(before[0], 1, 300, -150));
    expect(live.strokes[2]).toEqual(moved(before[2], 1, 300, -150));
    expect(live.strokes[1]).toEqual(before[1]);
    expect(live.strokes[3]).toEqual(before[3]);
    expect(live.strokes).toHaveLength(4);
    // One event: the chosen indices out, the moved strokes in at the same indices; no colour.
    const last = events().at(-1);
    expect(last).toMatchObject({ removed: [0, 2], recoloured: [], revision: answer, canUndo: true, canRedo: false });
    expect(last?.inserted.map((placed) => placed.index)).toEqual([0, 2]);
  });

  it('resizes: the points and the width follow the scale, pressure, times, colour and layer do not', async () => {
    const { live, transform, writeUntil } = await open();
    writeUntil(2);
    const before = structuredClone(live.strokes[1]);
    transform([1], 0.5, 1000, 2000);
    const after = live.strokes[1];
    expect(after).toEqual(moved(before, 0.5, 1000, 2000));
    expect(after.width).toBe(before.width * 0.5);
    expect(after.points.map((p) => [p.pressure, p.t])).toEqual(before.points.map((p) => [p.pressure, p.t]));
    expect(after.color).toEqual(before.color);
    expect(after.tool).toBe(before.tool);
    expect(after.layer).toBe(before.layer);
  });

  it('a transform that changes nothing is no step: same revision, no event', async () => {
    const { live, events, transform, writeUntil } = await open();
    writeUntil(2);
    const revision = live.revision;
    const count = events().length;
    expect(transform([0, 1], 1, 0, 0)).toBe(revision);
    expect(events()).toHaveLength(count);
    expect(live.canUndo).toBe(false);
  });

  it('refuses a scale or an offset out of range, or not a number, changing nothing', async () => {
    const { live, transform, writeUntil } = await open();
    writeUntil(2);
    const before = structuredClone(live.strokes);
    const refused = (scale: number, x: number, y: number) => {
      let error: unknown;
      try {
        transform([0], scale, x, y);
      } catch (e) {
        error = e;
      }
      expect(error).toEqual(expect.objectContaining({ code: 'liveEditInvalid' }));
    };
    refused(0.049, 0, 0);
    refused(0, 0, 0);
    refused(-1, 0, 0);
    refused(20.01, 0, 0);
    refused(Number.NaN, 0, 0);
    refused(Number.POSITIVE_INFINITY, 0, 0);
    refused(1, 1_000_000_001, 0);
    refused(1, 0, -1_000_000_001);
    refused(1, Number.NaN, 0);
    expect(live.strokes).toEqual(before);
    expect(live.canUndo).toBe(false);
    // The limits themselves are allowed.
    expect(() => transform([0], 0.05, 1e9, -1e9)).not.toThrow();
    expect(() => transform([1], 20, 0, 0)).not.toThrow();
  });

  it('refuses a stale revision or page, an empty or wrong list and a hidden stroke, as the other edits', async () => {
    const { live, transform, writeUntil } = await open();
    writeUntil(3);
    const revision = live.revision;
    transform([0], 1, 10, 10);
    const code = (run: () => unknown) => {
      try {
        run();
      } catch (e) {
        return (e as { code?: string }).code;
      }
      return undefined;
    };
    expect(code(() => transform([1], 1, 10, 10, revision))).toBe('liveEditStale');
    expect(code(() => transform([], 1, 10, 10))).toBe('liveEditInvalid');
    expect(code(() => transform([3], 1, 10, 10))).toBe('liveEditInvalid');
    expect(code(() => transform([1, 1], 1, 10, 10))).toBe('liveEditInvalid');
    expect(code(() => invoke('live_edit_strokes', { pageIndex: 5, revision: live.revision, edit: { kind: 'transform', indices: [1], scale: 1, x: 1, y: 1 } }))).toBe(
      'liveEditStale',
    );
    // A stroke of a hidden layer cannot be touched.
    invoke('live_layers', { pageIndex: live.pageIndex, op: { kind: 'visible', layer: 0, visible: false } });
    expect(code(() => transform([1], 1, 10, 10))).toBe('liveEditInvalid');
  });

  it('undoes and redoes a transform in one step each, with the strokes as before and as after in the event', async () => {
    const { live, events, transform, undo, redo, writeUntil } = await open();
    writeUntil(3);
    const before = structuredClone(live.strokes);
    transform([0, 1], 2, -40, 25);
    const after = structuredClone(live.strokes);
    const afterRevision = live.revision;

    expect(undo()).toBe(true);
    expect(live.strokes).toEqual(before);
    expect(live.revision).toBeGreaterThan(afterRevision);
    expect(events().at(-1)).toMatchObject({ removed: [0, 1], recoloured: [], canUndo: false, canRedo: true });
    expect(events().at(-1)?.inserted.map((placed) => placed.index)).toEqual([0, 1]);
    expect(undo()).toBe(false);

    const undone = live.revision;
    expect(redo()).toBe(true);
    expect(live.strokes).toEqual(after);
    expect(live.revision).toBeGreaterThan(undone);
    expect(events().at(-1)).toMatchObject({ removed: [0, 1], canUndo: true, canRedo: false });
    expect(redo()).toBe(false);
  });

  it('two transforms are two steps, and what the tablet wrote since stays where it is', async () => {
    const { live, transform, undo, writeUntil } = await open();
    writeUntil(2);
    const first = structuredClone(live.strokes);
    transform([0], 1, 100, 0);
    transform([0], 0.5, 0, 0);
    writeUntil(3);
    const written = structuredClone(live.strokes[2]);
    expect(live.strokes[0].width).toBe(first[0].width * 0.5);
    expect(undo()).toBe(true);
    expect(live.strokes[0]).toEqual(moved(first[0], 1, 100, 0));
    expect(undo()).toBe(true);
    expect(live.strokes[0]).toEqual(first[0]);
    expect(live.strokes[2]).toEqual(written);
    expect(undo()).toBe(false);
  });

  it('a window that subscribes later has the moved strokes, and the same list', async () => {
    const { live, transform, writeUntil } = await open();
    writeUntil(2);
    const answer = transform([1], 1.5, 70, 80);
    const late = new LiveState();
    late.initial(await mockLiveSubscribe((batch) => late.receive(batch)));
    expect(late.revision).toBe(answer);
    expect(late.canUndo).toBe(true);
    expect(late.strokes).toEqual(live.strokes);
  });

  it('the steps keep strokes of their own: a layer moved renumbers the page and the history once each', async () => {
    const { live, transform, undo, redo, writeUntil } = await open();
    writeUntil(2);
    invoke('live_layers', { pageIndex: live.pageIndex, op: { kind: 'add' } });
    expect(live.layers).toHaveLength(2);
    transform([0], 2, 10, 10);
    expect(live.strokes[0].layer).toBe(0);
    // Layer 0 goes above layer 1: its strokes are on layer 1 now, those of the page and those the steps hold.
    invoke('live_layers', { pageIndex: live.pageIndex, op: { kind: 'move', from: 0, to: 1 } });
    expect(live.strokes.map((s) => s.layer)).toEqual([1, 1]);
    expect(undo()).toBe(true);
    expect(live.strokes.map((s) => s.layer)).toEqual([1, 1]);
    expect(redo()).toBe(true);
    expect(live.strokes.map((s) => s.layer)).toEqual([1, 1]);
  });

  it('a page that turns moves its revision on, so a selection chosen before is dropped and its edit is refused as stale', async () => {
    const { live, transform, writeUntil } = await open();
    writeUntil(2);
    const before = live.revision;
    setMockLiveOrientation('portraitLeft');
    expect(live.revision).not.toBe(before);
    let error: unknown;
    try {
      transform([0], 1, 5, 5, before);
    } catch (e) {
      error = e;
    }
    expect(error).toEqual(expect.objectContaining({ code: 'liveEditStale' }));
    // At the new revision it goes through.
    expect(() => transform([0], 1, 5, 5)).not.toThrow();
  });

  it('a page that turns turns what can come back: an undone move returns turned, where it would be had the page stayed', async () => {
    const { live, transform, undo, redo, writeUntil } = await open();
    writeUntil(2);
    const first = structuredClone(live.strokes[0].points[0]);
    transform([0], 2, 100, 50);
    expect(undo()).toBe(true);
    setMockLiveOrientation('portraitLeft');
    // A quarter turn clockwise: (x, y) -> (height - y, x), the page being 21600 high before it.
    expect(live.strokes[0].points[0].x).toBeCloseTo(21600 - first.y, 6);
    expect(redo()).toBe(true);
    const moved = { x: first.x * 2 + 100, y: first.y * 2 + 50 };
    expect(live.strokes[0].points[0].x).toBeCloseTo(21600 - moved.y, 6);
    expect(live.strokes[0].points[0].y).toBeCloseTo(moved.x, 6);
    // And the undo turned too.
    expect(undo()).toBe(true);
    expect(live.strokes[0].points[0].x).toBeCloseTo(21600 - first.y, 6);
    expect(live.strokes[0].points[0].y).toBeCloseTo(first.x, 6);
  });
});
