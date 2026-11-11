// Milestone M14: the control window's queue, the computer's pen and its eraser end (docs/design/tablet-live-classroom.md,
// "The window's queue, pen and eraser"). The commands are stand-ins whose answers the tests release one by one, so that
// what is in flight, what waits and in which order the calls leave can be seen.
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ComputerSample, LiveComputerPenInput, LiveStrokesEdit, LiveStyle } from './live';
import {
  ComputerEraser,
  ComputerPen,
  ComputerQueue,
  LOCAL_GRACE_MS,
  LiveComputer,
  MAX_PEN_SAMPLES,
  STALE_RETRIES,
  STALE_WAIT_MS,
  nextGroup,
  type EraserSource,
} from './live-computer';
import type { Layer, Stroke } from './note';

const RED = { r: 216, g: 0, b: 0, a: 255 };
const STYLE: LiveStyle = { tool: 'feltTip', color: RED, width: 120 };

/** A promise the test settles. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

/** Lets the queue's promises move on. */
async function settle(): Promise<void> {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

/** `live_computer_pen` and `live_edit_strokes` whose answers wait for the test. */
function commands() {
  const calls: { input: LiveComputerPenInput; answer: ReturnType<typeof deferred<boolean>> }[] = [];
  const edits: { pageIndex: number; revision: number; edit: LiveStrokesEdit; answer: ReturnType<typeof deferred<number>> }[] = [];
  return {
    calls,
    edits,
    api: {
      liveComputerPen: (input: LiveComputerPenInput) => {
        const answer = deferred<boolean>();
        calls.push({ input: structuredClone(input), answer });
        return answer.promise;
      },
      liveEditStrokes: (pageIndex: number, revision: number, edit: LiveStrokesEdit) => {
        const answer = deferred<number>();
        edits.push({ pageIndex, revision, edit: structuredClone(edit), answer });
        return answer.promise;
      },
    },
  };
}

let clock = 1000;
const sample = (x: number, y = 500, ms = (clock += 4)): ComputerSample => ({ x, y, p: 0.5, ms });
const place = { pageIndex: 0, noteId: 'p1', layer: 1 };

function pen(extra: { onerror?: (error: unknown) => void; now?: () => number } = {}) {
  const c = commands();
  const queue = new ComputerQueue();
  const onerror = vi.fn(extra.onerror ?? (() => {}));
  const onchange = vi.fn();
  const computerPen = new ComputerPen({ queue, commands: c.api, style: () => STYLE, onerror, onchange, now: extra.now });
  return { ...c, queue, pen: computerPen, onerror, onchange };
}

afterEach(() => {
  vi.useRealTimers();
});

describe('ComputerQueue', () => {
  it('runs one task at a time, in the order asked, and an error does not stop the next one', async () => {
    const queue = new ComputerQueue();
    const started: string[] = [];
    const a = deferred<string>();
    const b = deferred<string>();
    const ra = queue.run(() => {
      started.push('a');
      return a.promise;
    });
    const rb = queue.run(() => {
      started.push('b');
      return b.promise;
    });
    const rc = queue.run(async () => {
      started.push('c');
      return 'c';
    });
    await settle();
    expect(started).toEqual(['a']);
    expect(queue.pending).toBe(3);
    a.reject(new Error('refused'));
    await expect(ra).rejects.toThrow('refused');
    await settle();
    expect(started).toEqual(['a', 'b']);
    b.resolve('b');
    await expect(rb).resolves.toBe('b');
    await expect(rc).resolves.toBe('c');
    await queue.idle();
    expect(queue.pending).toBe(0);
  });
});

describe('ComputerPen', () => {
  it('begins at once, then one call in flight: the samples that come meanwhile go in the next one', async () => {
    const t = pen();
    t.pen.down(place, sample(100));
    await settle();
    expect(t.pen.active).toBe(true);
    expect(t.calls.length).toBe(1);
    expect(t.calls[0].input).toMatchObject({ kind: 'begin', stroke: 1, pageIndex: 0, style: STYLE, samples: [{ x: 100 }] });
    expect(typeof (t.calls[0].input as { now: number }).now).toBe('number');
    t.pen.move([sample(110)]);
    t.pen.move([sample(120), sample(130)]);
    await settle();
    // Nothing more leaves while `begin` is in flight.
    expect(t.calls).toHaveLength(1);
    t.calls[0].answer.resolve(true);
    await settle();
    expect(t.calls).toHaveLength(2);
    expect(t.calls[1].input).toMatchObject({ kind: 'points', stroke: 1 });
    expect((t.calls[1].input as { samples: ComputerSample[] }).samples.map((s) => s.x)).toEqual([110, 120, 130]);
    // Lifted while `points` is in flight: `end` waits for it, with the last samples.
    t.pen.move([sample(140)]);
    t.pen.up();
    expect(t.pen.active).toBe(false);
    await settle();
    expect(t.calls).toHaveLength(2);
    t.calls[1].answer.resolve(true);
    await settle();
    expect(t.calls).toHaveLength(3);
    expect(t.calls[2].input).toMatchObject({ kind: 'end', stroke: 1 });
    expect((t.calls[2].input as { samples: ComputerSample[] }).samples.map((s) => s.x)).toEqual([140]);
    t.calls[2].answer.resolve(true);
    await t.queue.idle();
    expect(t.onerror).not.toHaveBeenCalled();
  });

  it('puts the end before a command asked for after the pen was lifted (a Ctrl+Z undoes that stroke)', async () => {
    const t = pen();
    t.pen.down(place, sample(100));
    await settle();
    t.pen.move([sample(110)]);
    t.pen.up();
    const order: string[] = [];
    const undo = t.queue.run(async () => {
      order.push('undo');
      return true;
    });
    t.calls[0].answer.resolve(true);
    await settle();
    expect(t.calls.map((call) => call.input.kind)).toEqual(['begin', 'end']);
    expect(order).toEqual([]);
    t.calls[1].answer.resolve(true);
    await undo;
    expect(order).toEqual(['undo']);
    // `end` carried the sample that came while `begin` was in flight.
    expect((t.calls[1].input as { samples: ComputerSample[] }).samples.map((s) => s.x)).toEqual([110]);
  });

  it('numbers the gestures from 1 and sends at most MAX_PEN_SAMPLES samples a call', async () => {
    const t = pen();
    t.pen.down(place, sample(0));
    t.pen.move(Array.from({ length: MAX_PEN_SAMPLES + 10 }, (_, i) => sample(i + 1)));
    t.pen.up();
    // `begin` took what was there when it left: the first MAX_PEN_SAMPLES.
    await settle();
    expect((t.calls[0].input as { samples: ComputerSample[] }).samples).toHaveLength(MAX_PEN_SAMPLES);
    t.calls[0].answer.resolve(true);
    await settle();
    expect(t.calls[1].input).toMatchObject({ kind: 'end', stroke: 1 });
    expect((t.calls[1].input as { samples: ComputerSample[] }).samples).toHaveLength(11);
    t.calls[1].answer.resolve(true);
    await settle();
    t.pen.down(place, sample(0));
    await settle();
    expect(t.calls[2].input).toMatchObject({ kind: 'begin', stroke: 2 });
  });

  it('draws its own stroke until the session says it is done, then hands it over', async () => {
    const t = pen();
    t.pen.down(place, sample(100));
    t.pen.move([sample(110), sample(120)]);
    const view = { pageIndex: 0, noteId: 'p1', lastComputerDone: 0 };
    const local = t.pen.locals(view);
    expect(local).toHaveLength(1);
    expect(local![0]).toMatchObject({ tool: 'feltTip', width: 120, layer: 1, color: RED });
    expect(local![0].points.map((p) => p.x)).toEqual([100, 110, 120]);
    expect(local![0].points.map((p) => p.t)).toEqual([0, 4, 8]);
    t.pen.up();
    // Lifted: still drawn while the session has not ended it.
    expect(t.pen.locals(view)).toHaveLength(1);
    // A second gesture before the session ended the first: both are drawn.
    t.pen.down(place, sample(300));
    expect(t.pen.locals(view)).toHaveLength(2);
    expect(t.pen.locals({ ...view, lastComputerDone: 1 })).toHaveLength(1);
    t.pen.up();
    expect(t.pen.locals({ ...view, lastComputerDone: 2 })).toBeUndefined();
  });

  it('drops its stroke on another page or note', () => {
    const t = pen();
    t.pen.down(place, sample(100));
    expect(t.pen.locals({ pageIndex: 1, noteId: 'p2', lastComputerDone: 0 })).toBeUndefined();
    expect(t.pen.active).toBe(false);
    t.pen.down(place, sample(100));
    expect(t.pen.locals({ pageIndex: 0, noteId: 'other', lastComputerDone: 0 })).toBeUndefined();
  });

  it(`stops drawing a stroke ${LOCAL_GRACE_MS} ms after its end left, should the answer be lost`, async () => {
    let now = 0;
    const t = pen({ now: () => now });
    t.pen.down(place, sample(100));
    await settle();
    t.calls[0].answer.resolve(true);
    t.pen.up();
    await settle();
    expect(t.calls[1].input.kind).toBe('end');
    const view = { pageIndex: 0, noteId: 'p1', lastComputerDone: 0 };
    now = LOCAL_GRACE_MS - 1;
    expect(t.pen.locals(view)).toHaveLength(1);
    now = LOCAL_GRACE_MS + 1;
    expect(t.pen.locals(view)).toBeUndefined();
  });

  it("cancel: the gesture's last call is `cancel`, and Rust's copy stays hidden until the session drops it", async () => {
    const t = pen();
    t.pen.down(place, sample(100));
    await settle();
    t.pen.move([sample(110)]);
    expect(t.pen.cancel()).toBe(true);
    expect(t.pen.active).toBe(false);
    expect(t.onchange).toHaveBeenCalled();
    const view = { pageIndex: 0, noteId: 'p1', lastComputerDone: 0 };
    expect(t.pen.locals(view)).toEqual([]);
    t.calls[0].answer.resolve(true);
    await settle();
    expect(t.calls.map((call) => call.input)).toEqual([expect.objectContaining({ kind: 'begin' }), { kind: 'cancel', stroke: 1 }]);
    t.calls[1].answer.resolve(true);
    await settle();
    expect(t.pen.locals({ ...view, lastComputerDone: 1 })).toBeUndefined();
    expect(t.pen.cancel()).toBe(false);
  });

  it('a gesture cancelled before its begin left sends nothing at all', async () => {
    const t = pen();
    const blocker = deferred<void>();
    void t.queue.run(() => blocker.promise);
    t.pen.down(place, sample(100));
    t.pen.cancel();
    blocker.resolve();
    await t.queue.idle();
    expect(t.calls).toHaveLength(0);
    expect(t.pen.locals({ pageIndex: 0, noteId: 'p1', lastComputerDone: 0 })).toBeUndefined();
  });

  it('a false answer stops the gesture: nothing more is sent for it, its stroke goes', async () => {
    const t = pen();
    t.pen.down(place, sample(100));
    await settle();
    t.calls[0].answer.resolve(true);
    t.pen.move([sample(110)]);
    await settle();
    t.calls[1].answer.resolve(false);
    await settle();
    expect(t.pen.locals({ pageIndex: 0, noteId: 'p1', lastComputerDone: 0 })).toBeUndefined();
    t.pen.move([sample(120)]);
    t.pen.up();
    await t.queue.idle();
    expect(t.calls).toHaveLength(2);
    expect(t.onerror).not.toHaveBeenCalled();
  });

  it('a refused begin drops the stroke: stale is silent, the other refusals are told', async () => {
    const t = pen();
    t.pen.down(place, sample(100));
    await settle();
    t.calls[0].answer.reject({ code: 'liveEditStale', message: 'stale' });
    await settle();
    expect(t.onerror).not.toHaveBeenCalled();
    expect(t.pen.locals({ pageIndex: 0, noteId: 'p1', lastComputerDone: 0 })).toBeUndefined();
    t.pen.move([sample(110)]);
    t.pen.up();
    await t.queue.idle();
    expect(t.calls).toHaveLength(1);

    t.pen.down(place, sample(100));
    await settle();
    t.calls[1].answer.reject({ code: 'liveLayerHidden', message: 'hidden' });
    await settle();
    expect(t.onerror).toHaveBeenCalledExactlyOnceWith({ code: 'liveLayerHidden', message: 'hidden' });
  });
});

/** A page as the eraser sees it: three strokes on two layers, the computer on layer 1. */
class Source implements EraserSource {
  strokes: Stroke[];
  layers: Layer[] = [
    { name: 'Calque 1', visible: true },
    { name: 'Correction', visible: true },
  ];
  computerLayer = 1;
  revision = 10;
  pageIndex = 0;
  noteId = 'p1';
  lastComputerDone = 0;
  private listeners = new Set<() => void>();

  constructor() {
    this.strokes = [line(1000, 0), line(1000, 1), line(5000, 1)];
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  change(): void {
    for (const listener of [...this.listeners]) listener();
  }
}

/** A horizontal line at y = 2000 from `x`, 1000 units long. */
function line(x: number, layer: number): Stroke {
  return {
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer,
    points: Array.from({ length: 11 }, (_, i) => ({ x: x + 100 * i, y: 2000, pressure: 0.6, t: 5 * i })),
  };
}

function eraser(groups = [101, 102, 103]) {
  const c = commands();
  const queue = new ComputerQueue();
  const live = new Source();
  const onerror = vi.fn();
  let g = 0;
  const computerEraser = new ComputerEraser({ queue, commands: c.api, live, onerror, group: () => groups[g++] });
  return { ...c, queue, live, eraser: computerEraser, onerror };
}

describe('ComputerEraser', () => {
  it('deletes whole strokes of the chosen layer it touches, each once, one group per gesture', async () => {
    const t = eraser();
    // Across x = 1500: a stroke of layer 0 and one of layer 1; only the latter goes.
    t.eraser.down({ x: 1500, y: 1500 }, 100);
    t.eraser.move({ x: 1500, y: 2500 });
    expect(t.eraser.active).toBe(true);
    await settle();
    expect(t.edits).toHaveLength(1);
    expect(t.edits[0]).toMatchObject({ pageIndex: 0, revision: 10, edit: { kind: 'delete', indices: [1], group: 101 } });
    // Back over the same stroke: nothing more; then over the third one: same group.
    t.eraser.move({ x: 1500, y: 1500 });
    t.eraser.move({ x: 5500, y: 2000 });
    t.edits[0].answer.resolve(11);
    await settle();
    expect(t.edits).toHaveLength(2);
    expect(t.edits[1].edit).toMatchObject({ kind: 'delete', group: 101 });
    t.eraser.up();
    expect(t.eraser.active).toBe(false);
    t.eraser.down({ x: 1500, y: 2000 }, 100);
    t.eraser.up();
    await settle();
    expect(t.edits).toHaveLength(2);
  });

  it('reads the indices and the revision when the call leaves the queue (a delete moves indices)', async () => {
    const t = eraser();
    t.eraser.down({ x: 1500, y: 2000 }, 100);
    t.eraser.move({ x: 5500, y: 2000 });
    await settle();
    expect(t.edits[0].edit.indices).toEqual([1]);
    // The session applies the first delete before the second call leaves: the third stroke is now at 1.
    t.live.strokes.splice(1, 1);
    t.live.revision = 11;
    t.edits[0].answer.resolve(11);
    await settle();
    expect(t.edits[1]).toMatchObject({ revision: 11, edit: { indices: [1], group: 101 } });
  });

  it('finds nothing on a hidden chosen layer, and on other layers', async () => {
    const t = eraser();
    t.live.layers = [t.live.layers[0], { ...t.live.layers[1], visible: false }];
    t.eraser.down({ x: 1500, y: 2000 }, 100);
    t.eraser.move({ x: 5500, y: 2000 });
    t.live.computerLayer = 0;
    t.live.layers = [t.live.layers[0], { ...t.live.layers[1], visible: true }];
    t.eraser.move({ x: 1500, y: 2000 });
    await settle();
    expect(t.edits.map((call) => call.edit.indices)).toEqual([[0]]);
  });

  it(`tries a stale delete again after the next change, at most ${STALE_RETRIES} times, skipping strokes gone`, async () => {
    const t = eraser();
    t.eraser.down({ x: 1500, y: 2000 }, 100);
    t.eraser.move({ x: 5500, y: 2000 });
    await settle();
    t.edits[0].answer.reject({ code: 'liveEditStale', message: 'stale' });
    await settle();
    // The gesture's other delete leaves meanwhile; the stale one waits for the window's list to catch up.
    expect(t.edits.map((call) => call.edit.indices)).toEqual([[1], [2]]);
    t.edits[1].answer.resolve(11);
    await settle();
    expect(t.edits).toHaveLength(2);
    t.live.revision = 12;
    t.live.change();
    await settle();
    expect(t.edits).toHaveLength(3);
    expect(t.edits[2]).toMatchObject({ revision: 12, edit: { kind: 'delete', indices: [1], group: 101 } });
    // Stale again and again: tried STALE_RETRIES times after the first, then given up, silently.
    t.edits[2].answer.reject({ code: 'liveEditStale', message: 'stale' });
    for (let i = 1; i < STALE_RETRIES; i++) {
      await settle();
      t.live.change();
      await settle();
      t.edits[t.edits.length - 1].answer.reject({ code: 'liveEditStale', message: 'stale' });
    }
    await settle();
    t.live.change();
    await t.queue.idle();
    await settle();
    expect(t.edits).toHaveLength(2 + STALE_RETRIES);
    expect(t.onerror).not.toHaveBeenCalled();

    // A stroke gone meanwhile (an undo, a clearing) is skipped; none left: no call.
    const u = eraser();
    u.eraser.down({ x: 5500, y: 2000 }, 100);
    await settle();
    u.edits[0].answer.reject({ code: 'liveEditStale', message: 'stale' });
    await settle();
    u.live.strokes.splice(2, 1);
    u.live.change();
    await u.queue.idle();
    expect(u.edits).toHaveLength(1);
  });

  it('tells a refusal other than stale, and stops on another page', async () => {
    const t = eraser();
    t.eraser.down({ x: 1500, y: 2000 }, 100);
    await settle();
    t.edits[0].answer.reject({ code: 'liveNotRunning', message: 'stopped' });
    await settle();
    expect(t.onerror).toHaveBeenCalledOnce();
    t.live.pageIndex = 1;
    t.eraser.move({ x: 5500, y: 2000 });
    await t.queue.idle();
    expect(t.edits).toHaveLength(1);
  });

  it('keeps its ring where the eraser end is, and drops it when cancelled', () => {
    const t = eraser();
    t.eraser.hover({ x: 10, y: 20 });
    expect(t.eraser.ring).toEqual({ x: 10, y: 20 });
    t.eraser.down({ x: 30, y: 40 }, 100);
    t.eraser.hover(null);
    expect(t.eraser.ring).toEqual({ x: 30, y: 40 });
    t.eraser.move({ x: 50, y: 60 });
    expect(t.eraser.ring).toEqual({ x: 50, y: 60 });
    expect(t.eraser.cancel()).toBe(true);
    expect(t.eraser.ring).toBeNull();
    expect(t.eraser.active).toBe(false);
  });
});

describe('group numbers', () => {
  it('are whole u32 numbers that follow each other from a value drawn at load, not from 1', () => {
    const a = nextGroup();
    const b = nextGroup();
    expect(Number.isInteger(a) && a > 0 && a <= 0xffff_ffff).toBe(true);
    expect(b).toBe(a + 1);
    // (supposé) Drawn in 1..2^31: the chance that a test run starts at 1 is nil.
    expect(a).toBeGreaterThan(1);
  });
});

describe('LiveComputer', () => {
  it('is busy while a gesture of the pen or of the eraser is under way, and Esc drops it', async () => {
    const c = commands();
    const live = new Source();
    const computer = new LiveComputer({ live, commands: c.api, style: () => STYLE, onerror: () => {} });
    const onchange = vi.fn();
    computer.onchange = onchange;
    expect(computer.busy).toBe(false);
    computer.pen.down(place, sample(1));
    expect(computer.busy).toBe(true);
    expect(computer.locals()).toHaveLength(1);
    computer.pen.up();
    expect(computer.busy).toBe(false);
    computer.eraser.down({ x: 1500, y: 2000 }, 100);
    expect(computer.busy).toBe(true);
    expect(computer.cancel()).toBe(true);
    expect(computer.busy).toBe(false);
    expect(onchange).toHaveBeenCalled();
    expect(computer.cancel()).toBe(false);
    computer.dispose();
  });
});

// --- Review fixes (M14) -------------------------------------------------------------------------------------------

describe('ComputerPen: what the review found', () => {
  it('queues every chunk still pending and the end when the pen is lifted, before a Ctrl+Z asked for after', async () => {
    const t = pen();
    t.pen.down(place, sample(0));
    await settle();
    t.calls[0].answer.resolve(true);
    t.pen.move([sample(1)]);
    await settle();
    // `points` is in flight (an IPC call that takes long); the samples pile up meanwhile.
    expect(t.calls.map((call) => call.input.kind)).toEqual(['begin', 'points']);
    t.pen.move(Array.from({ length: MAX_PEN_SAMPLES + 10 }, (_, i) => sample(i + 2)));
    t.pen.up();
    const order: string[] = [];
    const undo = t.queue.run(async () => {
      order.push(`undo after ${t.calls.length} calls`);
      return true;
    });
    t.calls[1].answer.resolve(true);
    await settle();
    t.calls[2].answer.resolve(true);
    await settle();
    expect(t.calls.map((call) => call.input.kind)).toEqual(['begin', 'points', 'points', 'end']);
    const sizes = t.calls.map((call) => ('samples' in call.input ? call.input.samples.length : 0));
    expect(sizes).toEqual([1, 1, MAX_PEN_SAMPLES, 10]);
    t.calls[3].answer.resolve(true);
    await undo;
    expect(order).toEqual(['undo after 4 calls']);
  });

  it('says a lifted stroke is ending until the session recorded it, or dropped it', async () => {
    const t = pen();
    const view = { pageIndex: 0, noteId: 'p1', lastComputerDone: 0 };
    expect(t.pen.ending(view)).toBe(false);
    t.pen.down(place, sample(1));
    expect(t.pen.ending(view)).toBe(false);
    t.pen.up();
    expect(t.pen.ending(view)).toBe(true);
    expect(t.pen.ending({ ...view, lastComputerDone: 1 })).toBe(false);
    // A refused begin: no step will come.
    await settle();
    t.calls[0].answer.reject({ code: 'liveLayerHidden', message: 'hidden' });
    await settle();
    expect(t.pen.ending(view)).toBe(false);
  });

  it('cancels a gesture under way when the window goes; a lifted stroke still ends', async () => {
    const t = pen();
    t.pen.down(place, sample(1));
    await settle();
    t.calls[0].answer.resolve(true);
    t.pen.move([sample(2)]);
    t.pen.dispose();
    await settle();
    expect(t.calls.map((call) => call.input)).toEqual([expect.objectContaining({ kind: 'begin' }), { kind: 'cancel', stroke: 1 }]);
    t.calls[1].answer.resolve(true);
    await t.queue.idle();

    // Lifted, then the window goes: its `end` leaves, no cancel.
    const u = pen();
    u.pen.down(place, sample(1));
    u.pen.up();
    u.pen.dispose();
    await settle();
    u.calls[0].answer.resolve(true);
    await settle();
    expect(u.calls.map((call) => call.input.kind)).toEqual(['begin', 'end']);
    u.calls[1].answer.resolve(true);
    await u.queue.idle();

    // Gone before its begin left: nothing at all.
    const v = pen();
    const blocker = deferred<void>();
    void v.queue.run(() => blocker.promise);
    v.pen.down(place, sample(1));
    v.pen.dispose();
    blocker.resolve();
    await v.queue.idle();
    expect(v.calls).toEqual([]);
  });
});

describe('ComputerEraser: what the review found', () => {
  it("makes the gesture's next delete at the revision the session answered, on the list less what it deleted", async () => {
    const t = eraser();
    const [a, , c] = t.live.strokes;
    t.eraser.down({ x: 1500, y: 2000 }, 100);
    t.eraser.move({ x: 5500, y: 2000 });
    await settle();
    expect(t.edits[0]).toMatchObject({ revision: 10, edit: { indices: [1] } });
    // The session went to 11; the window's list has not caught up (its strokesEdited is on its way).
    t.edits[0].answer.resolve(11);
    await settle();
    expect(t.edits[1]).toMatchObject({ revision: 11, edit: { indices: [1], group: 101 } });
    t.edits[1].answer.resolve(12);
    await settle();
    // The window catches up with the first delete only: still the session's revision, without the second stroke.
    t.live.strokes = [a, c];
    t.live.revision = 11;
    t.eraser.down({ x: 1500, y: 2000 }, 100);
    expect(t.edits).toHaveLength(2);
    t.live.strokes.push(line(9000, 1));
    t.eraser.move({ x: 9500, y: 2000 });
    await settle();
    expect(t.edits[2]).toMatchObject({ revision: 12, edit: { indices: [1], group: 102 } });
    t.edits[2].answer.resolve(13);
    await settle();
    // Something else moved the revision (an undo): the window's own revision and list again.
    t.live.revision = 40;
    t.live.strokes.push(line(13_000, 1));
    t.eraser.move({ x: 13_500, y: 2000 });
    await settle();
    expect(t.edits[3]).toMatchObject({ revision: 40, edit: { indices: [3] } });
    expect(t.onerror).not.toHaveBeenCalled();
  });

  it('retries at once when the window already caught up before the stale answer came', async () => {
    const t = eraser();
    t.eraser.down({ x: 1500, y: 2000 }, 100);
    await settle();
    // The window learns of a change meanwhile, then the answer comes: stale.
    t.live.revision = 12;
    t.edits[0].answer.reject({ code: 'liveEditStale', message: 'stale' });
    await settle();
    // No change notification is needed: it leaves again at once, at the new revision.
    expect(t.edits).toHaveLength(2);
    expect(t.edits[1]).toMatchObject({ revision: 12, edit: { indices: [1], group: 101 } });
  });

  it('counts its retries as work under way, drops them when an undo is queued, and gives up when nothing changes', async () => {
    const c = commands();
    const live = new Source();
    const computer = new LiveComputer({ live, commands: c.api, style: () => STYLE, onerror: () => {}, group: () => 7 });
    computer.eraser.down({ x: 1500, y: 2000 }, 100);
    computer.eraser.up();
    await settle();
    c.edits[0].answer.reject({ code: 'liveEditStale', message: 'stale' });
    await settle();
    // Lifted, but a retry waits for the window's list: Ctrl+Z must wait too.
    expect(computer.busy).toBe(true);
    // An undo is queued (the gesture's group given back): the retry would delete the stroke again after it.
    computer.historyQueued();
    expect(computer.busy).toBe(false);
    live.revision = 11;
    live.change();
    await computer.queue.idle();
    await settle();
    expect(c.edits).toHaveLength(1);

    // A retry already queued when the undo is asked for is dropped as well.
    computer.eraser.down({ x: 5500, y: 2000 }, 100);
    computer.eraser.up();
    await settle();
    const blocker = deferred<void>();
    void computer.queue.run(() => blocker.promise);
    c.edits[1].answer.reject({ code: 'liveEditStale', message: 'stale' });
    await settle();
    live.revision = 12;
    live.change();
    await settle();
    computer.historyQueued();
    blocker.resolve();
    await computer.queue.idle();
    expect(c.edits).toHaveLength(2);

    // Nothing changes at all: after STALE_WAIT_MS the retry gives up, and Ctrl+Z is free again.
    vi.useFakeTimers();
    computer.eraser.down({ x: 1500, y: 2000 }, 100);
    computer.eraser.up();
    await settle();
    c.edits[2].answer.reject({ code: 'liveEditStale', message: 'stale' });
    await settle();
    expect(computer.busy).toBe(true);
    vi.advanceTimersByTime(STALE_WAIT_MS + 1);
    await settle();
    expect(computer.busy).toBe(false);
    expect(c.edits).toHaveLength(3);
  });
});

describe('LiveComputer: the stroke just written', () => {
  it('says a stroke is ending once lifted, until lastComputerDone reaches it', () => {
    const c = commands();
    const live = new Source();
    const computer = new LiveComputer({ live, commands: c.api, style: () => STYLE, onerror: () => {} });
    computer.pen.down(place, sample(1));
    expect(computer.strokeEnding).toBe(false);
    computer.pen.up();
    expect(computer.strokeEnding).toBe(true);
    live.lastComputerDone = 1;
    expect(computer.strokeEnding).toBe(false);
  });

  it('says from which hand a refusal comes', async () => {
    const c = commands();
    const live = new Source();
    const onerror = vi.fn();
    const computer = new LiveComputer({ live, commands: c.api, style: () => STYLE, onerror });
    computer.pen.down(place, sample(1));
    await settle();
    c.calls[0].answer.reject({ code: 'liveEditInvalid', message: 'x' });
    computer.eraser.down({ x: 1500, y: 2000 }, 100);
    await settle();
    c.edits[0].answer.reject({ code: 'liveEditInvalid', message: 'y' });
    await settle();
    expect(onerror.mock.calls).toEqual([
      [{ code: 'liveEditInvalid', message: 'x' }, 'pen'],
      [{ code: 'liveEditInvalid', message: 'y' }, 'eraser'],
    ]);
  });
});
