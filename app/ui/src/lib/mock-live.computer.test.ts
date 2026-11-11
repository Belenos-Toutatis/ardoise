// Milestone M14: the browser stand-in follows package B's rules for the computer's pen (`live_computer_pen`), the layers
// (`live_layers`), the per-page layers of both pens, the undo steps of computer strokes and the eraser's groups
// (docs/design/tablet-live-classroom.md, "M14", and its "As built (B)").
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ComputerSample, LiveBatch, LiveComputerPenInput, LiveEvent, LiveLayerOp, LiveSnapshot, LiveStyle } from './live';
import { wallClock } from './latency';
import { LiveState } from './live-state.svelte';
import { resetMockLibrary } from './mock-data';
import {
  MAX_COMPUTER_POINTS,
  MAX_LAYERS,
  MAX_PEN_SAMPLES,
  mockLiveConfig,
  mockLiveInvoke,
  mockLiveSubscribe,
  mockLiveTick,
  resetMockLive,
  setMockLiveOrientation,
} from './mock-live';

const RED = { r: 216, g: 0, b: 0, a: 255 };
const STYLE: LiveStyle = { tool: 'feltTip', color: RED, width: 120 };
const invoke = (command: string, args: Record<string, unknown> = {}) => mockLiveInvoke(command, args);

/** A window on a running session whose tablet is driven step by step. */
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
  const events = () => batches.flatMap((batch) => batch.events);
  const since = (count: number) => events().slice(count);
  const page = () => (invoke('live_start') as LiveSnapshot).page;
  const state = () => invoke('live_start') as LiveSnapshot;
  const layers = (op: LiveLayerOp, pageIndex = live.pageIndex) => invoke('live_layers', { pageIndex, op });
  const pen = (input: LiveComputerPenInput) => invoke('live_computer_pen', { input }) as boolean;
  let ms = 0;
  const samples = (n: number, x0 = 5000, y = 15000): ComputerSample[] =>
    Array.from({ length: n }, (_, i) => ({ x: x0 + 40 * i, y, p: 0.5, ms: (ms += 4) }));
  const now = () => ms + 1;
  /** A whole computer stroke of `n` samples as gesture `id`. */
  const write = (id: number, n = 6, x0 = 5000) => {
    pen({ kind: 'begin', stroke: id, pageIndex: live.pageIndex, style: STYLE, samples: samples(2, x0), now: now() });
    pen({ kind: 'points', stroke: id, samples: samples(n - 2, x0 + 80), now: now() });
    return pen({ kind: 'end', stroke: id, samples: [], now: now() });
  };
  /** The tablet writes until the page holds `count` finished strokes. */
  const tabletUntil = (count: number) => {
    for (let i = 0; live.strokes.length < count; i++) {
      if (i > 5000) throw new Error('the pen does not write');
      mockLiveTick();
    }
  };
  return { live, batches, events, since, page, state, layers, pen, samples, now, write, tabletUntil };
}

type Session = Awaited<ReturnType<typeof open>>;

function refused(call: () => unknown): unknown {
  try {
    call();
  } catch (error) {
    return (error as { code?: unknown }).code;
  }
  return null;
}

const types = (events: readonly LiveEvent[]) => events.map((e) => ('pen' in e && e.pen ? `${e.type}:${e.pen}` : e.type));

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(Date.UTC(2026, 10, 11, 9, 0));
  resetMockLive();
  resetMockLibrary();
});

afterEach(() => {
  resetMockLive();
  vi.useRealTimers();
});

describe('browser stand-in: the layers of both pens', () => {
  it('starts both pens on the first layer; « Ajouter un calque » takes the computer there, the tablet stays', async () => {
    const s = await open();
    expect([s.live.layer, s.live.computerLayer]).toEqual([0, 0]);
    s.layers({ kind: 'add' });
    expect(s.live.layers.map((l) => l.name)).toEqual(['Calque 1', 'Calque 2']);
    expect([s.live.layer, s.live.computerLayer]).toEqual([0, 1]);
    // The tablet's button (or `newLayer`): the tablet goes to a new top layer, the computer stays.
    invoke('live_command', { command: { kind: 'newLayer' } });
    expect([s.live.layer, s.live.computerLayer, s.live.layers.length]).toEqual([2, 1, 3]);
  });

  it('chooses the layers of the computer and of the tablet; the same choice sends nothing', async () => {
    const s = await open();
    s.layers({ kind: 'add' });
    s.layers({ kind: 'tablet', layer: 1 });
    s.layers({ kind: 'computer', layer: 0 });
    expect([s.live.layer, s.live.computerLayer]).toEqual([1, 0]);
    const count = s.events().length;
    s.layers({ kind: 'computer', layer: 0 });
    s.layers({ kind: 'tablet', layer: 1 });
    s.layers({ kind: 'rename', layer: 0, name: '  Calque 1 ' });
    s.layers({ kind: 'visible', layer: 0, visible: true });
    expect(s.events().length).toBe(count);
    // The tablet then writes on its layer.
    s.tabletUntil(1);
    expect(s.live.strokes[0].layer).toBe(1);
  });

  it('renames (trimmed), shows and hides; refuses what makes no sense, as a whole', async () => {
    const s = await open();
    s.layers({ kind: 'rename', layer: 0, name: '  Élève  ' });
    expect(s.live.layers[0].name).toBe('Élève');
    s.layers({ kind: 'visible', layer: 0, visible: false });
    expect(s.live.layers[0].visible).toBe(false);
    expect(refused(() => s.layers({ kind: 'rename', layer: 0, name: '   ' }))).toBe('liveLayerInvalid');
    expect(refused(() => s.layers({ kind: 'rename', layer: 0, name: 'x'.repeat(201) }))).toBe('liveLayerInvalid');
    expect(refused(() => s.layers({ kind: 'visible', layer: 3, visible: true }))).toBe('liveLayerInvalid');
    expect(refused(() => s.layers({ kind: 'computer', layer: 0.5 }))).toBe('liveLayerInvalid');
    expect(refused(() => s.layers({ kind: 'merge', layer: 0 }))).toBe('liveLayerInvalid');
    expect(refused(() => s.layers({ kind: 'add' }, 3))).toBe('liveEditStale');
    expect(s.live.layers).toEqual([{ name: 'Élève', visible: false }]);
    for (let i = 1; i < MAX_LAYERS; i++) s.layers({ kind: 'add' });
    expect(refused(() => s.layers({ kind: 'add' }))).toBe('liveLayerInvalid');
    // The tablet's button does nothing at all there.
    invoke('live_command', { command: { kind: 'newLayer' } });
    expect(s.state().page.layers).toHaveLength(MAX_LAYERS);
  });

  it('moves a layer: strokes and both pens follow it, places and revision unchanged', async () => {
    const s = await open();
    s.tabletUntil(1);
    s.layers({ kind: 'add' });
    s.write(1);
    s.layers({ kind: 'add' });
    const revision = s.live.revision;
    expect(s.live.strokes.map((st) => st.layer)).toEqual([0, 1]);
    // Calque 1 to the top: Calque 2 and Calque 3 go down.
    s.layers({ kind: 'move', from: 0, to: 2 });
    expect(s.live.layers.map((l) => l.name)).toEqual(['Calque 2', 'Calque 3', 'Calque 1']);
    expect(s.live.strokes.map((st) => st.layer)).toEqual([2, 0]);
    expect([s.live.layer, s.live.computerLayer]).toEqual([2, 1]);
    expect(s.live.revision).toBe(revision);
    expect(refused(() => s.layers({ kind: 'move', from: 1, to: 1 }))).toBe('liveLayerInvalid');
    // The computer stroke's undo still takes it off, at its place, after the move.
    expect(invoke('live_undo_edit', { pageIndex: 0 })).toBe(true);
    expect(s.live.strokes.map((st) => st.layer)).toEqual([2]);
    expect(invoke('live_redo_edit', { pageIndex: 0 })).toBe(true);
    expect(s.live.strokes.map((st) => st.layer)).toEqual([2, 0]);
  });

  it("merges a layer into the one below: its strokes come after the others, the page's history goes", async () => {
    const s = await open();
    s.layers({ kind: 'add' });
    s.write(1);
    s.tabletUntil(2);
    s.layers({ kind: 'visible', layer: 0, visible: false });
    expect(s.live.canUndo).toBe(true);
    const revision = s.live.revision;
    s.layers({ kind: 'merge', layer: 1 });
    expect(s.live.layers).toEqual([{ name: 'Calque 1', visible: true }]);
    // The tablet's stroke first, then the merged computer stroke.
    expect(s.live.strokes.map((st) => [st.layer, st.tool])).toEqual([
      [0, 'ballpoint'],
      [0, 'feltTip'],
    ]);
    expect([s.live.layer, s.live.computerLayer]).toEqual([0, 0]);
    expect(s.live.revision).not.toBe(revision);
    expect([s.live.canUndo, s.live.canRedo]).toEqual([false, false]);
    expect(invoke('live_undo_edit', { pageIndex: 0 })).toBe(false);
  });

  it('keeps the layers of a page when it is left; a new page starts both on its first layer', async () => {
    const s = await open();
    s.layers({ kind: 'add' });
    s.layers({ kind: 'tablet', layer: 1 });
    s.layers({ kind: 'computer', layer: 0 });
    invoke('live_command', { command: { kind: 'nextPage' } });
    expect([s.live.pageIndex, s.live.layer, s.live.computerLayer]).toEqual([1, 0, 0]);
    invoke('live_command', { command: { kind: 'previousPage' } });
    expect([s.live.pageIndex, s.live.layer, s.live.computerLayer]).toEqual([0, 1, 0]);
  });

  it('refuses every layer operation once the session is stopped', async () => {
    const s = await open();
    invoke('live_stop');
    for (const op of [{ kind: 'add' }, { kind: 'computer', layer: 0 }] as LiveLayerOp[]) {
      expect(refused(() => s.layers(op))).toBe('liveNotRunning');
    }
  });
});

describe('browser stand-in: the computer pen', () => {
  it('writes beside the tablet: events with its pen and number, then a step of the history', async () => {
    const s = await open();
    s.layers({ kind: 'add' });
    const start = s.events().length;
    expect(s.pen({ kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: s.samples(3), now: s.now() })).toBe(true);
    s.tabletUntil(1);
    expect(s.pen({ kind: 'points', stroke: 1, samples: s.samples(2, 6000), now: s.now() })).toBe(true);
    expect(s.live.computerCurrent?.points).toHaveLength(5);
    expect(s.live.strokeCount).toBe(2);
    expect(s.pen({ kind: 'end', stroke: 1, samples: s.samples(1, 7000), now: s.now() })).toBe(true);
    const computerEvents = s.since(start).filter((e) => 'pen' in e && e.pen === 'computer');
    expect(computerEvents.map((e) => e.type)).toEqual(['strokeStart', 'points', 'points', 'strokeEnd']);
    expect(computerEvents.every((e) => 'id' in e && e.id === 1)).toBe(true);
    const end = s.since(start).findIndex((e) => e.type === 'strokeEnd' && e.pen === 'computer');
    expect(s.since(start)[end + 1]).toEqual({ type: 'history', pageIndex: 0, canUndo: true, canRedo: false });
    // At the tail, after the tablet's stroke finished while it was written, on the computer's layer, in its style.
    const last = s.live.strokes[s.live.strokes.length - 1];
    expect(last).toMatchObject({ tool: 'feltTip', width: 120, color: RED, layer: 1 });
    expect(last.points).toHaveLength(6);
    expect(s.live.lastComputerDone).toBe(1);
    expect(s.page().strokes).toEqual(s.live.strokes);
  });

  it("gives its points times from the page's origin and the wall clock, never going back", async () => {
    const s = await open();
    const before = wallClock();
    s.write(1, 12);
    const stroke = s.live.strokes[0];
    const times = stroke.points.map((p) => p.t);
    expect(times.every((t, i) => t >= 0 && (i === 0 || t >= times[i - 1]))).toBe(true);
    const recv = s.events().flatMap((e) => (e.type === 'strokeStart' || e.type === 'points') && e.pen === 'computer' ? (e.type === 'points' ? e.points : e.stroke.points) : []);
    expect(recv).toHaveLength(12);
    expect(recv.every((p) => p.recv <= wallClock() && p.recv >= before - 10_000)).toBe(true);
    // A sample far in the past still gets a time no earlier than 10 s before its call.
    s.pen({ kind: 'begin', stroke: 2, pageIndex: 0, style: STYLE, samples: [{ x: 1, y: 1, p: 1, ms: -1e9 }], now: 0 });
    expect(s.live.computerCurrent!.points[0].t).toBeGreaterThanOrEqual(0);
  });

  it('checks the samples: none finite, too many; non-finite pressure counts as 1; coordinates kept near the page', async () => {
    const s = await open();
    const begin = (samples: ComputerSample[], stroke = 1) => s.pen({ kind: 'begin', stroke, pageIndex: 0, style: STYLE, samples, now: 1 });
    expect(refused(() => begin([{ x: NaN, y: 1, p: 1, ms: 0 }]))).toBe('liveEditInvalid');
    expect(refused(() => begin([]))).toBe('liveEditInvalid');
    expect(refused(() => begin(s.samples(MAX_PEN_SAMPLES + 1)))).toBe('liveEditInvalid');
    expect(begin([{ x: Infinity, y: 0, p: 1, ms: 0 }, { x: -1e9, y: 1e9, p: NaN, ms: 0 }, { x: 10, y: 10, p: 7, ms: 0 }])).toBe(true);
    const points = s.live.computerCurrent!.points;
    expect(points).toHaveLength(2);
    expect(points[0]).toMatchObject({ x: -29700, y: 2 * 21600, pressure: 1 });
    expect(points[1].pressure).toBe(1);
    expect(refused(() => s.pen({ kind: 'points', stroke: 1, samples: s.samples(MAX_PEN_SAMPLES + 1), now: 1 }))).toBe('liveEditInvalid');
  });

  it('refuses a begin on another page or on a hidden layer, and leaves the stroke in progress as it was', async () => {
    const s = await open();
    s.pen({ kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: s.samples(2), now: s.now() });
    expect(refused(() => s.pen({ kind: 'begin', stroke: 2, pageIndex: 1, style: STYLE, samples: s.samples(1), now: s.now() }))).toBe('liveEditStale');
    s.layers({ kind: 'add' });
    s.layers({ kind: 'visible', layer: 1, visible: false });
    expect(refused(() => s.pen({ kind: 'begin', stroke: 2, pageIndex: 0, style: STYLE, samples: s.samples(1), now: s.now() }))).toBe('liveLayerHidden');
    // Stroke 1 goes on (on « Calque 1 », where it began).
    expect(s.pen({ kind: 'points', stroke: 1, samples: s.samples(1), now: s.now() })).toBe(true);
    expect(s.live.computerCurrent?.points).toHaveLength(3);
  });

  it('answers false for another number or no stroke; cancel drops it with strokeCancel', async () => {
    const s = await open();
    expect(s.pen({ kind: 'points', stroke: 1, samples: s.samples(1), now: s.now() })).toBe(false);
    expect(s.pen({ kind: 'end', stroke: 1, samples: [], now: s.now() })).toBe(false);
    expect(s.pen({ kind: 'cancel', stroke: 1 })).toBe(false);
    s.pen({ kind: 'begin', stroke: 3, pageIndex: 0, style: STYLE, samples: s.samples(2), now: s.now() });
    expect(s.pen({ kind: 'points', stroke: 2, samples: s.samples(1), now: s.now() })).toBe(false);
    const start = s.events().length;
    expect(s.pen({ kind: 'cancel', stroke: 3 })).toBe(true);
    expect(s.since(start)).toEqual([{ type: 'strokeCancel', pageIndex: 0, pen: 'computer', id: 3 }]);
    expect(s.live.strokes).toHaveLength(0);
    expect(s.live.lastComputerDone).toBe(3);
    expect(s.live.canUndo).toBe(false);
  });

  it('a begin while a stroke is in progress ends that one first, kept', async () => {
    const s = await open();
    s.pen({ kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: s.samples(2), now: s.now() });
    const start = s.events().length;
    s.pen({ kind: 'begin', stroke: 2, pageIndex: 0, style: STYLE, samples: s.samples(2, 9000), now: s.now() });
    expect(types(s.since(start))).toEqual(['strokeEnd:computer', 'history', 'saved', 'strokeStart:computer']);
    expect(s.live.strokes).toHaveLength(1);
    expect(s.pen({ kind: 'points', stroke: 1, samples: s.samples(1), now: s.now() })).toBe(false);
  });

  it('is ended, kept, by a page change and a quarter turn; dropped by « Clear the page », told before the snapshot', async () => {
    const s = await open();
    s.pen({ kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: s.samples(2), now: s.now() });
    let start = s.events().length;
    invoke('live_command', { command: { kind: 'nextPage' } });
    let after = types(s.since(start));
    expect(after.indexOf('strokeEnd:computer')).toBeLessThan(after.indexOf('snapshot'));
    expect(s.pen({ kind: 'points', stroke: 1, samples: s.samples(1), now: s.now() })).toBe(false);
    invoke('live_command', { command: { kind: 'previousPage' } });
    expect(s.live.strokes).toHaveLength(1);

    s.pen({ kind: 'begin', stroke: 2, pageIndex: 0, style: STYLE, samples: s.samples(2), now: s.now() });
    setMockLiveOrientation('portraitLeft');
    expect(s.live.strokes).toHaveLength(2);
    expect(s.pen({ kind: 'end', stroke: 2, samples: [], now: s.now() })).toBe(false);

    s.pen({ kind: 'begin', stroke: 3, pageIndex: 0, style: STYLE, samples: s.samples(2), now: s.now() });
    start = s.events().length;
    invoke('live_command', { command: { kind: 'clearPage' } });
    after = types(s.since(start));
    expect(after.slice(0, 2)).toEqual(['strokeCancel:computer', 'snapshot']);
    expect(s.live.computerCurrent).toBeNull();
    expect(s.live.lastComputerDone).toBe(3);
    expect(s.pen({ kind: 'points', stroke: 3, samples: s.samples(1), now: s.now() })).toBe(false);
  });

  it('follows a snapshot with both strokes in progress, the tablet first', async () => {
    const s = await open();
    s.pen({ kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: s.samples(2), now: s.now() });
    for (let i = 0; !s.live.current; i++) {
      if (i > 500) throw new Error('the tablet does not write');
      mockLiveTick();
    }
    const start = s.events().length;
    s.layers({ kind: 'add' });
    expect(types(s.since(start))).toEqual(['snapshot', 'strokeStart', 'strokeStart:computer']);
    expect(s.live.current).not.toBeNull();
    expect(s.live.computerCurrent?.points).toHaveLength(2);
  });

  it('ends a stroke that reaches the limit of points, kept, and answers false', async () => {
    const s = await open();
    s.pen({ kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: s.samples(1), now: s.now() });
    let goesOn = true;
    let calls = 0;
    while (goesOn) {
      goesOn = s.pen({ kind: 'points', stroke: 1, samples: s.samples(MAX_PEN_SAMPLES), now: s.now() });
      if (++calls > 100) throw new Error('no limit');
    }
    expect(s.live.strokes).toHaveLength(1);
    expect(s.live.strokes[0].points).toHaveLength(MAX_COMPUTER_POINTS);
    expect(s.live.canUndo).toBe(true);
  });

  it('after a stop, a begin is refused and the rest answers false; before any start, everything is refused', async () => {
    expect(refused(() => invoke('live_computer_pen', { input: { kind: 'cancel', stroke: 1 } }))).toBe('liveNotRunning');
    const s = await open();
    s.pen({ kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: s.samples(2), now: s.now() });
    invoke('live_stop');
    // The stop kept the stroke (it is in the note), and nothing more goes in.
    expect(refused(() => s.pen({ kind: 'begin', stroke: 2, pageIndex: 0, style: STYLE, samples: s.samples(1), now: s.now() }))).toBe('liveNotRunning');
    expect(s.pen({ kind: 'points', stroke: 1, samples: s.samples(1), now: s.now() })).toBe(false);
    expect(s.pen({ kind: 'cancel', stroke: 1 })).toBe(false);
  });

  it('writes while the tablet is away (the session waits for it)', async () => {
    mockLiveConfig.pen = false;
    mockLiveConfig.waitMs = 10_000;
    invoke('live_start');
    expect((invoke('live_start') as LiveSnapshot).state).toBe('waiting');
    const input = { kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: [{ x: 1, y: 1, p: 0.5, ms: 0 }], now: 1 };
    expect(invoke('live_computer_pen', { input })).toBe(true);
  });
});

describe('browser stand-in: undo and redo of the computer', () => {
  async function withTablet(): Promise<Session> {
    const s = await open();
    s.tabletUntil(2);
    return s;
  }

  it("undoes a computer stroke and redoes it at its place while the tablet appended after it; never the tablet's", async () => {
    const s = await withTablet();
    s.write(1);
    s.tabletUntil(4);
    const tablet = s.live.strokes.filter((st) => st.tool === 'ballpoint').length;
    expect(s.live.strokes[2].tool).toBe('feltTip');
    expect(invoke('live_undo_edit', { pageIndex: 0 })).toBe(true);
    expect(s.live.strokes.map((st) => st.tool)).not.toContain('feltTip');
    expect(s.live.strokes).toHaveLength(tablet);
    expect([s.live.canUndo, s.live.canRedo]).toEqual([false, true]);
    // Nothing more to undo: the tablet's writing is no step.
    expect(invoke('live_undo_edit', { pageIndex: 0 })).toBe(false);
    expect(s.live.strokes).toHaveLength(tablet);
    // The tablet's writing does not empty the redo steps.
    s.tabletUntil(tablet + 1);
    expect(invoke('live_redo_edit', { pageIndex: 0 })).toBe(true);
    expect(s.live.strokes[2].tool).toBe('feltTip');
    expect(s.page().strokes).toEqual(s.live.strokes);
  });

  it('a new computer stroke empties the redo steps', async () => {
    const s = await withTablet();
    s.write(1);
    invoke('live_undo_edit', { pageIndex: 0 });
    expect(s.live.canRedo).toBe(true);
    s.write(2);
    expect(s.live.canRedo).toBe(false);
    expect(invoke('live_redo_edit', { pageIndex: 0 })).toBe(false);
  });

  it("undoes an eraser's gesture, every delete of its group, in one command and one batch; redoes it whole", async () => {
    const s = await withTablet();
    s.tabletUntil(4);
    const before = [...s.live.strokes];
    const del = (indices: number[], group?: number) =>
      invoke('live_edit_strokes', { pageIndex: 0, revision: s.live.revision, edit: { kind: 'delete', indices, ...(group === undefined ? {} : { group }) } });
    del([3]);
    del([0], 77);
    del([1], 77);
    expect(s.live.strokes).toEqual([before[1]]);
    const batches = s.batches.length;
    expect(invoke('live_undo_edit', { pageIndex: 0 })).toBe(true);
    expect(s.batches.length).toBe(batches + 1);
    expect(s.batches.at(-1)!.events.map((e) => e.type)).toEqual(['strokesEdited', 'strokesEdited']);
    expect(s.live.strokes).toEqual([before[0], before[1], before[2]]);
    // The delete without a group is a step of its own.
    expect(invoke('live_undo_edit', { pageIndex: 0 })).toBe(true);
    expect(s.live.strokes).toEqual(before);
    expect(invoke('live_redo_edit', { pageIndex: 0 })).toBe(true);
    expect(invoke('live_redo_edit', { pageIndex: 0 })).toBe(true);
    expect(s.live.strokes).toEqual([before[1]]);
    expect(refused(() => del([0], -1))).toBe('liveEditInvalid');
    expect(refused(() => del([0], 1.5))).toBe('liveEditInvalid');
  });

  it('« Undo the clearing » goes through the groups too', async () => {
    const s = await withTablet();
    invoke('live_command', { command: { kind: 'clearPage' } });
    s.tabletUntil(1);
    invoke('live_edit_strokes', { pageIndex: 0, revision: s.live.revision, edit: { kind: 'delete', indices: [0], group: 5 } });
    expect(invoke('live_restore_cleared')).toBe(true);
    expect(s.live.strokes).toHaveLength(3);
  });
});
