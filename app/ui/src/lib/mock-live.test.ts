import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveEvent, LiveSnapshot, MonitorInfo } from './live';
import { mockLibrary, resetMockLibrary } from './mock-data';
import {
  mockLiveConfig,
  mockLiveEmit,
  mockLiveInvoke,
  mockLiveReconnect,
  mockLiveSubscribe,
  mockLiveSubscribers,
  mockLiveTick,
  resetMockLive,
} from './mock-live';

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

describe('browser stand-in for live mode', () => {
  it('starts idle, waits for the tablet, then streams batches of 3 points every 15 ms', async () => {
    const sub = await subscribe();
    expect(sub.first.state).toBe('idle');
    const started = invoke('live_start') as LiveSnapshot;
    expect(started.state).toBe('waiting');
    expect(started.pageCount).toBe(1);
    expect(started.page.origin).toEqual({ kind: 'live' });
    expect(sub.last()?.state).toBe('waiting');

    vi.advanceTimersByTime(mockLiveConfig.waitMs);
    expect(sub.last()?.state).toBe('live');
    const before = sub.batches.length;
    vi.advanceTimersByTime(15 * 200);
    const batches = sub.batches.slice(before);
    // One batch per tick, except while the pen is away (nothing to send).
    expect(batches.length).toBeGreaterThan(150);
    expect(batches.length).toBeLessThanOrEqual(200);
    for (const batch of batches) {
      const points = batch.events.flatMap((e) =>
        e.type === 'points' ? e.points : e.type === 'strokeStart' ? e.stroke.points : [],
      );
      expect(points.length).toBeLessThanOrEqual(3);
      for (const p of points) {
        // Received a little before the batch left Rust.
        expect(p.recv).toBeLessThan(batch.sentAt);
        expect(batch.sentAt - p.recv).toBeLessThan(5);
        expect(p.p).toBeGreaterThan(0);
        expect(p.p).toBeLessThanOrEqual(1);
        expect(Number.isInteger(p.t)).toBe(true);
      }
      // The hovering pen at most once per batch.
      expect(batch.events.filter((e) => e.type === 'hover').length).toBeLessThanOrEqual(1);
    }
    const events = batches.flatMap((b) => b.events);
    const starts = events.filter((e) => e.type === 'strokeStart').length;
    const ends = events.filter((e) => e.type === 'strokeEnd').length;
    expect(starts).toBeGreaterThan(3);
    expect(Math.abs(starts - ends)).toBeLessThanOrEqual(1);
    expect(events.some((e) => e.type === 'hover')).toBe(true);
    expect(events.some((e) => e.type === 'hoverEnd')).toBe(true);
    // Points follow each other 5 ms apart within a stroke.
    const stroke = events.find((e) => e.type === 'strokeStart') as Extract<LiveEvent, { type: 'strokeStart' }>;
    if (stroke.stroke.points.length > 1) expect(stroke.stroke.points[1].t - stroke.stroke.points[0].t).toBe(5);
  });

  it('writes the same strokes every time (seeded pen)', async () => {
    const run = async () => {
      resetMockLive();
      resetMockLibrary();
      mockLiveConfig.waitMs = 0;
      const sub = await subscribe();
      invoke('live_start');
      vi.advanceTimersByTime(15 * 120);
      return sub
        .events()
        .filter((e) => e.type !== 'snapshot')
        .map((e) => JSON.stringify(e, (k, v) => (k === 'recv' ? undefined : v)));
    };
    const a = await run();
    const b = await run();
    expect(a.length).toBeGreaterThan(50);
    expect(b).toEqual(a);
  });

  it('turns pages, adds pages and layers, clears the page', async () => {
    mockLiveConfig.pen = false;
    const sub = await subscribe();
    invoke('live_start');
    vi.advanceTimersByTime(mockLiveConfig.waitMs);
    expect(() => invoke('live_command', { command: { kind: 'previousPage' } })).not.toThrow();
    expect(sub.last()?.pageIndex).toBe(0);
    invoke('live_command', { command: { kind: 'nextPage' } });
    expect(sub.last()).toMatchObject({ pageIndex: 1, pageCount: 2 });
    invoke('live_command', { command: { kind: 'previousPage' } });
    expect(sub.last()).toMatchObject({ pageIndex: 0, pageCount: 2 });
    invoke('live_command', { command: { kind: 'newPage' } });
    expect(sub.last()).toMatchObject({ pageIndex: 2, pageCount: 3 });
    invoke('live_command', { command: { kind: 'button' } });
    expect(sub.last()).toMatchObject({ pageIndex: 3, pageCount: 4 });
    invoke('live_command', { command: { kind: 'newLayer' } });
    expect(sub.last()?.layer).toBe(1);
    expect(sub.last()?.page.layers).toHaveLength(2);

    mockLiveConfig.pen = true;
    mockLiveTick(300);
    const strokes = () => sub.last()?.page.strokes.length ?? 0;
    invoke('live_command', { command: { kind: 'newLayer' } });
    expect(strokes()).toBeGreaterThan(0);
    invoke('live_command', { command: { kind: 'clearPage' } });
    expect(strokes()).toBe(0);
    expect(() => invoke('live_command', { command: { kind: 'jump' } })).toThrow();
  });

  it('sets the style and the view; the blackboard brings slate paper and white chalk', async () => {
    mockLiveConfig.pen = false;
    const sub = await subscribe();
    invoke('live_start');
    const red = { tool: 'feltTip', color: { r: 216, g: 0, b: 0, a: 255 }, width: 120 };
    invoke('live_set_style', { style: red });
    expect(sub.last()?.style).toEqual(red);
    const view = { zoom: 12, x: 1000, y: 2000, board: false, laser: true };
    invoke('live_set_view', { view });
    const echoed = sub.events().at(-1);
    expect(echoed).toEqual({ type: 'view', view: { ...view, zoom: 8 } });
    invoke('live_set_view', { view: { ...view, board: true } });
    expect(sub.last()?.page.background.paper).toBe('slate');
    expect(sub.last()?.style).toMatchObject({ tool: 'chalk', color: { r: 255, g: 255, b: 255, a: 255 } });
    // New pages are slate too while the blackboard is on.
    invoke('live_command', { command: { kind: 'newPage' } });
    expect(sub.last()?.page.background.paper).toBe('slate');
    invoke('live_set_view', { view: { ...view, board: false } });
    expect(sub.last()?.page.background.paper).toBe('white');
    expect(sub.last()?.style).toEqual(red);
  });

  it('refuses commands when live mode is not running', () => {
    expect(() => invoke('live_command', { command: { kind: 'nextPage' } })).toThrow();
    expect(() => invoke('live_set_view', { view: {} })).toThrow();
    expect(() => invoke('live_set_style', { style: {} })).toThrow();
    expect(invoke('live_stop')).toEqual([]);
    expect(invoke('unknown_command')).toBeTypeOf('symbol');
  });

  it('offers two screens and opens the presentation on the projector by default', async () => {
    const sub = await subscribe();
    const screens = invoke('list_monitors') as MonitorInfo[];
    expect(screens.map((m) => [m.primary, m.chosen])).toEqual([
      [true, false],
      [false, false],
    ]);
    const shown = invoke('open_presentation', { monitor: null }) as MonitorInfo;
    expect(shown.primary).toBe(false);
    expect(shown.chosen).toBe(true);
    expect(sub.last()?.presentation?.key).toBe(shown.key);
    // The choice is remembered.
    expect((invoke('list_monitors') as MonitorInfo[]).find((m) => m.chosen)?.key).toBe(shown.key);
    invoke('close_presentation');
    expect(sub.last()?.presentation).toBeNull();
    const primary = invoke('open_presentation', { monitor: screens[0].key }) as MonitorInfo;
    expect(primary.primary).toBe(true);
    mockLiveConfig.monitors = 0;
    expect(() => invoke('open_presentation', { monitor: null })).toThrow();
  });

  it('saves each page with strokes as a note of the library and reports it', async () => {
    mockLiveConfig.waitMs = 0;
    const sub = await subscribe();
    const before = mockLibrary().notes.size;
    invoke('live_start');
    vi.advanceTimersByTime(15 * 150);
    const saved = sub.events().filter((e) => e.type === 'saved');
    expect(saved.length).toBeGreaterThan(0);
    invoke('live_command', { command: { kind: 'nextPage' } });
    vi.advanceTimersByTime(15 * 150);
    invoke('live_command', { command: { kind: 'newPage' } });
    const paths = invoke('live_stop') as string[];
    expect(paths).toHaveLength(2);
    expect(new Set(paths).size).toBe(2);
    expect(mockLibrary().notes.size).toBe(before + 2);
    const note = mockLibrary().notes.get(paths[0])!.note;
    expect(note.origin).toEqual({ kind: 'live' });
    expect(note.strokes.length).toBeGreaterThan(0);
    expect(note.strokes.every((s) => s.points.every((p) => Number.isInteger(p.t)))).toBe(true);
    expect(sub.last()?.state).toBe('idle');
    // Stopped: the pen is silent.
    const count = sub.batches.length;
    vi.advanceTimersByTime(1000);
    expect(sub.batches.length).toBe(count);
  });

  it('reconnects after a dropped link, and lets tests send their own events', async () => {
    mockLiveConfig.waitMs = 0;
    const sub = await subscribe();
    invoke('live_start');
    vi.advanceTimersByTime(1);
    mockLiveReconnect(500);
    expect(sub.last()?.state).toBe('reconnecting');
    const count = sub.batches.length;
    vi.advanceTimersByTime(400);
    expect(sub.batches.length).toBe(count);
    vi.advanceTimersByTime(100);
    expect(sub.last()?.state).toBe('live');
    mockLiveEmit([{ type: 'hoverEnd' }]);
    expect(sub.events().at(-1)).toEqual({ type: 'hoverEnd' });
    expect(mockLiveSubscribers()).toBe(1);
    resetMockLive();
    expect(mockLiveSubscribers()).toBe(0);
  });
});
