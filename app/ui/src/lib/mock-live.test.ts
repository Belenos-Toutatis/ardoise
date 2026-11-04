import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveEvent, LiveSnapshot, MonitorInfo } from './live';
import { LiveState } from './live-state.svelte';
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
  setMockLiveOrientation,
} from './mock-live';
import { orientPoint, quarterTurns, turnPoint } from './orientation';

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
  it('renders the paper of a page, like the session', () => {
    const url = mockLiveInvoke('live_page_background', {
      pageIndex: 0,
      page: { width: 29700, height: 21600 },
      background: { paper: 'slate', template: 'blank', texture: false },
      widthPx: 512,
    });
    expect(String(url)).toMatch(/^data:image\/svg\+xml/);
  });

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

  it('gives the ink back after « Clear the page », only on the page that was cleared', async () => {
    mockLiveConfig.pen = true;
    const sub = await subscribe();
    invoke('live_start');
    vi.advanceTimersByTime(mockLiveConfig.waitMs + 1500);
    invoke('live_command', { command: { kind: 'clearPage' } });
    expect(sub.last()!.page.strokes).toHaveLength(0);
    expect(invoke('live_restore_cleared')).toBe(true);
    // Through the page's history: an event with the strokes, not the whole page again.
    expect(sub.events().at(-1)).toMatchObject({ type: 'strokesEdited', removed: [] });
    expect((invoke('live_start') as LiveSnapshot).page.strokes.length).toBeGreaterThan(0);
    // Only once, and not from another page.
    expect(invoke('live_restore_cleared')).toBe(false);
    invoke('live_command', { command: { kind: 'clearPage' } });
    invoke('live_command', { command: { kind: 'newPage' } });
    expect(invoke('live_restore_cleared')).toBe(false);
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

  it('follows a snapshot with the stroke in progress, so a window never holds a partial stroke', async () => {
    const live = new LiveState();
    const sub = await subscribe();
    const snapshot = invoke('live_start') as LiveSnapshot;
    live.initial(snapshot);
    const feed = sub.batches.length;
    const fed = () => sub.batches.slice(feed).forEach((b) => live.receive(b));
    vi.advanceTimersByTime(mockLiveConfig.waitMs);
    // Until the pen is down for a while: a stroke in progress.
    let guard = 0;
    while ((live.current?.points.length ?? 0) < 4 && guard++ < 1000) {
      vi.advanceTimersByTime(15);
      fed();
      sub.batches.splice(feed);
    }
    expect(live.current).not.toBeNull();
    const finished = live.strokes.length;
    const red = { tool: 'feltTip', color: { r: 216, g: 0, b: 0, a: 255 }, width: 120 };
    invoke('live_set_style', { style: red });
    fed();
    expect(live.strokes).toHaveLength(finished);
    expect(live.current?.points.length ?? 0).toBeGreaterThanOrEqual(4);
    expect(live.strokeCount).toBe(finished + 1);
  });

  it('holds the tablet landscape by default and portrait notes follow the position', async () => {
    mockLiveConfig.pen = false;
    const sub = await subscribe();
    expect(sub.first.orientation).toBe('landscape');
    setMockLiveOrientation('portraitLeft');
    const started = invoke('live_start') as LiveSnapshot;
    expect(started.orientation).toBe('portraitLeft');
    expect(started.page.page).toEqual({ width: 21600, height: 29700 });
    // The simulated pen writes in the tablet's frame; the points arrive turned onto the page.
    vi.advanceTimersByTime(mockLiveConfig.waitMs);
    mockLiveTick(200);
    const points = sub
      .events()
      .flatMap((e) => (e.type === 'points' ? e.points : e.type === 'strokeStart' ? e.stroke.points : []));
    expect(points.length).toBeGreaterThan(100);
    for (const p of points) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(21600);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(29700);
    }
  });

  it('turns the page in progress, with its ink, when the position is switched', async () => {
    mockLiveConfig.pen = false;
    const sub = await subscribe();
    invoke('live_start');
    vi.advanceTimersByTime(mockLiveConfig.waitMs);
    mockLiveConfig.pen = true;
    mockLiveTick(300);
    // Snapshots only carry the strokes finished when they were taken: ask for a fresh one.
    const before = await mockLiveSubscribe(() => {});
    expect(before.page.strokes.length).toBeGreaterThan(1);
    const old = structuredClone(before.page.strokes);

    const count = sub.snapshots().length;
    setMockLiveOrientation('portraitRight');
    // One snapshot: the page is portrait and every point turned three quarter turns (the
    // stroke in progress ended first, so there may be one stroke more).
    expect(sub.snapshots().length).toBe(count + 1);
    const after = sub.last()!;
    expect(after.orientation).toBe('portraitRight');
    expect(after.page.page).toEqual({ width: 21600, height: 29700 });
    expect(after.page.strokes.length).toBeGreaterThanOrEqual(old.length);
    old.forEach((stroke, i) => {
      stroke.points.forEach((p, j) => {
        const [x, y] = turnPoint(p.x, p.y, 29700, 21600, quarterTurns('portraitRight'));
        expect(after.page.strokes[i].points[j]).toMatchObject({ x, y, pressure: p.pressure, t: p.t });
      });
    });
    expect(after.view).toMatchObject({ zoom: 1, x: 10800, y: 14850 });
    // Same position again: nothing happens.
    setMockLiveOrientation('portraitRight');
    expect(sub.snapshots().length).toBe(count + 1);
  });

  it('shows a switch made while the session still waits for the tablet', async () => {
    mockLiveConfig.pen = false;
    const sub = await subscribe();
    const started = invoke('live_start') as LiveSnapshot;
    expect(started.state).toBe('waiting');
    setMockLiveOrientation('portraitLeft');
    const waiting = sub.last()!;
    expect(waiting.state).toBe('waiting');
    expect(waiting.orientation).toBe('portraitLeft');
    expect(waiting.page.page).toEqual({ width: 21600, height: 29700 });
    // The tablet then streams: same position, the page is not turned again.
    vi.advanceTimersByTime(mockLiveConfig.waitMs);
    const live = sub.last()!;
    expect(live.state).toBe('live');
    expect(live.orientation).toBe('portraitLeft');
    expect(live.page.page).toEqual({ width: 21600, height: 29700 });
  });

  it('turns an earlier page only when it is shown again, and a blank page at once', async () => {
    mockLiveConfig.pen = false;
    const sub = await subscribe();
    invoke('live_start');
    vi.advanceTimersByTime(mockLiveConfig.waitMs);
    mockLiveConfig.pen = true;
    mockLiveTick(120);
    mockLiveConfig.pen = false;
    invoke('live_command', { command: { kind: 'newPage' } });
    expect(sub.last()?.pageIndex).toBe(1);
    setMockLiveOrientation('landscapeFlipped');
    // Page 2 is blank: it follows, and stays landscape.
    expect(sub.last()?.page.page).toEqual({ width: 29700, height: 21600 });
    setMockLiveOrientation('portraitLeft');
    expect(sub.last()?.page.page).toEqual({ width: 21600, height: 29700 });
    // Page 1 has ink and was left in landscape: it turns when shown again.
    invoke('live_command', { command: { kind: 'previousPage' } });
    const shown = sub.last()!;
    expect(shown.pageIndex).toBe(0);
    expect(shown.page.page).toEqual({ width: 21600, height: 29700 });
    expect(shown.page.strokes.length).toBeGreaterThan(0);
    for (const stroke of shown.page.strokes) {
      for (const p of stroke.points) {
        expect(p.x).toBeGreaterThanOrEqual(0);
        expect(p.x).toBeLessThanOrEqual(21600);
        expect(p.y).toBeGreaterThanOrEqual(0);
        expect(p.y).toBeLessThanOrEqual(29700);
      }
    }
  });

  it('turns the tablet-frame corners onto the page like the Rust side', () => {
    expect(orientPoint(0, 0, 29700, 21600, 'landscape')).toEqual([0, 0]);
    expect(orientPoint(0, 0, 29700, 21600, 'landscapeFlipped')).toEqual([29700, 21600]);
    expect(orientPoint(0, 0, 29700, 21600, 'portraitLeft')).toEqual([21600, 0]);
    expect(orientPoint(29700, 0, 29700, 21600, 'portraitLeft')).toEqual([21600, 29700]);
    expect(orientPoint(0, 0, 29700, 21600, 'portraitRight')).toEqual([0, 29700]);
    expect(orientPoint(29700, 21600, 29700, 21600, 'portraitRight')).toEqual([21600, 0]);
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
    // The assistant's bridge projects without changing the saved choice.
    invoke('close_presentation');
    const bridged = invoke('open_presentation', { monitor: screens[0].key, remember: false }) as MonitorInfo;
    expect(bridged.primary).toBe(true);
    expect((invoke('list_monitors') as MonitorInfo[]).find((m) => m.chosen)?.key).toBe(shown.key);
    invoke('close_presentation');
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

  it('reports a page that cannot be written once, keeps it in memory and writes it with the next success', async () => {
    mockLiveConfig.waitMs = 0;
    mockLiveConfig.saveError = 'The library is read-only';
    const sub = await subscribe();
    const before = mockLibrary().notes.size;
    invoke('live_start');
    vi.advanceTimersByTime(15 * 150);
    const failures = () => sub.events().flatMap((e) => (e.type === 'saveFailed' ? [e] : []));
    expect(failures()).toEqual([{ type: 'saveFailed', pageIndex: 0, code: 'libraryAccess', message: 'The library is read-only' }]);
    expect(sub.events().some((e) => e.type === 'saved')).toBe(false);
    expect(mockLibrary().notes.size).toBe(before);

    // The next page fails too: one report per page, not one per stroke.
    invoke('live_command', { command: { kind: 'nextPage' } });
    vi.advanceTimersByTime(15 * 150);
    expect(failures().map((e) => e.pageIndex)).toEqual([0, 1]);

    // The error goes away: the next save writes both pages and says that everything is written.
    mockLiveConfig.saveError = null;
    invoke('live_command', { command: { kind: 'previousPage' } });
    vi.advanceTimersByTime(15);
    const saved = sub.events().filter((e) => e.type === 'saved');
    expect(saved.length).toBeGreaterThan(0);
    expect(mockLibrary().notes.size).toBe(before + 2);
    expect(failures()).toHaveLength(2);
    const paths = invoke('live_stop') as string[];
    expect(paths).toHaveLength(2);
  });

  it('does not say that everything is saved when stopping with a page that failed', async () => {
    mockLiveConfig.waitMs = 0;
    mockLiveConfig.saveError = 'disk full';
    const sub = await subscribe();
    invoke('live_start');
    vi.advanceTimersByTime(15 * 150);
    const count = sub.events().length;
    invoke('live_stop');
    const after = sub.events().slice(count);
    expect(after.some((e) => e.type === 'saved')).toBe(false);
    expect(after.some((e) => e.type === 'snapshot')).toBe(true);
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
