// @vitest-environment jsdom
// The browser stand-in of the editor's projection (M17, docs/design/paper-projection-narration.md §2.3): Rust's rules
// (the screen, the scene, live's events, the refusals), and the feed, the stand-in and a window's `LiveState` agreeing.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { LiveBatch, LiveEvent } from './live';
import { LiveState } from './live-state.svelte';
import { mockLiveConfig, mockLiveInvoke, mockLiveSubscribe, resetMockLive } from './mock-live';
import {
  mockProjectionClosedByWindow,
  mockProjectionInvoke,
  mockProjectionState,
  mockProjectionSubscribe,
  mockProjectionWatch,
  resetMockProjection,
} from './mock-projection';
import type { Note, Stroke } from './note';
import type { ProjectionStatus, ProjectionUpdate } from './projection';
import { ProjectionFeed, type InProgress } from './projection-feed';

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const VIEW = { zoom: 1, x: 14850, y: 10800 };

let made = 0;
function stroke(layer = 0): Stroke {
  const t = (made += 100);
  return { tool: 'ballpoint', color: BLACK, width: 60, layer, points: [0, 1, 2].map((i) => ({ x: 1000 + t + i, y: 2000, pressure: 0.5, t: t + i * 10 })) };
}

function note(strokes: Stroke[], over: Partial<Note> = {}): Note {
  return {
    id: 'note-p',
    title: '',
    created: 0,
    modified: 0,
    page: { width: 29700, height: 21600 },
    background: { paper: 'cream', template: 'seyes', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes,
    timing: 'recorded',
    origin: { kind: 'live' },
    ...over,
  };
}

const update = (u: ProjectionUpdate) => mockProjectionInvoke('projection_update', { update: u });
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

function refusal(fn: () => unknown): string | null {
  try {
    fn();
    return null;
  } catch (error) {
    return (error as { code: string }).code;
  }
}

/** A window subscribed to the stand-in: the batches it got. */
async function subscribed() {
  const batches: LiveBatch[] = [];
  const snapshot = await mockProjectionSubscribe((batch) => batches.push(batch));
  await flush();
  const events = () => batches.flatMap((b) => b.events);
  return { snapshot, batches, events, types: () => events().map((e) => e.type) };
}

beforeEach(() => {
  resetMockLive();
  resetMockProjection();
  mockLiveConfig.pen = false;
});
afterEach(() => {
  resetMockProjection();
  resetMockLive();
});

describe('the screen', () => {
  it('opens on the first screen that is not the primary one, tells the watcher, closes live’s presentation', async () => {
    const statuses: ProjectionStatus[] = [];
    expect(await mockProjectionWatch((s) => statuses.push(s))).toEqual({ open: null });
    mockLiveInvoke('open_presentation', { monitor: null });
    const shown = mockProjectionInvoke('projection_open', { monitor: null }) as { key: string; primary: boolean };
    expect(shown.primary).toBe(false);
    expect(statuses.map((s) => s.open?.key ?? null)).toEqual([shown.key]);
    expect((await mockLiveSubscribe(() => {})).presentation).toBeNull();
    // The screen asked for wins.
    mockProjectionInvoke('projection_close', {});
    const list = mockLiveInvoke('list_monitors', {}) as { key: string }[];
    expect((mockProjectionInvoke('projection_open', { monitor: list[0].key }) as { key: string }).key).toBe(list[0].key);
  });

  it('refuses without any screen', () => {
    mockLiveConfig.monitors = 0;
    expect(refusal(() => mockProjectionInvoke('projection_open', { monitor: null }))).toBe('noMonitor');
  });

  it('closed by any means: the scene, the stroke and the subscriber go, the watcher hears it', async () => {
    const statuses: ProjectionStatus[] = [];
    await mockProjectionWatch((s) => statuses.push(s));
    mockProjectionInvoke('projection_open', { monitor: null });
    await mockProjectionSubscribe(() => {});
    update({ kind: 'page', note: note([stroke()]), imageKey: null, view: VIEW });
    update({ kind: 'strokeBegin', id: 1, tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [] });
    mockProjectionClosedByWindow();
    expect(mockProjectionState()).toMatchObject({ open: null, note: null, current: null, subscribed: false, watched: true });
    expect(statuses.at(-1)).toEqual({ open: null });
    // Closing again tells nothing new.
    mockProjectionInvoke('projection_close', {});
    expect(statuses).toHaveLength(2);
  });
});

describe('while closed', () => {
  it('refuses every update and keeps nothing', () => {
    const page = note([stroke()]);
    expect(refusal(() => update({ kind: 'page', note: page, imageKey: null, view: VIEW }))).toBe('projectionStale');
    expect(refusal(() => update({ kind: 'strokeCancel', id: 1 }))).toBe('projectionStale');
    expect(mockProjectionState().note).toBeNull();
    mockProjectionInvoke('projection_open', { monitor: null });
    update({ kind: 'page', note: page, imageKey: null, view: VIEW });
    mockProjectionInvoke('projection_close', {});
    expect(refusal(() => update({ kind: 'strokes', keep: 1, tail: [], ended: null }))).toBe('projectionStale');
    expect(mockProjectionState().note).toBeNull();
  });
});

describe('the scene and its events', () => {
  beforeEach(() => {
    mockProjectionInvoke('projection_open', { monitor: null });
  });

  it('answers an idle empty page before any page, then the snapshot of a one-page session', async () => {
    const w = await subscribed();
    expect(w.snapshot.state).toBe('idle');
    expect(w.snapshot.page.strokes).toEqual([]);
    const page = note([stroke(), stroke()], { backgroundImage: { name: 'fiche.pdf', pdfPage: 1, rect: { x: 0, y: 0, width: 100, height: 100 }, quarterTurns: 0, opacity: 1 } });
    update({ kind: 'page', note: page, imageKey: 'staged-7', view: { zoom: 2, x: 9000, y: 8000 } });
    const snapshot = w.events().find((e) => e.type === 'snapshot' && e.snapshot.state === 'live');
    if (snapshot?.type !== 'snapshot') throw new Error('no snapshot of the page');
    expect(snapshot.snapshot).toMatchObject({
      state: 'live',
      pageIndex: 0,
      pageCount: 1,
      canTurnPage: false,
      layer: 0,
      computerLayer: 0,
      view: { zoom: 2, x: 9000, y: 8000, board: false, laser: false },
      canUndo: false,
      canRedo: false,
      pagePaper: 'cream',
      imagePixels: { key: 'staged-7', width: 2480, height: 3508 },
    });
    expect(snapshot.snapshot.page.strokes).toHaveLength(2);
  });

  it('strokes: the old tail removed, the new one inserted, and ended swaps the stroke in progress in the same batch', async () => {
    const w = await subscribed();
    const [a, b, c] = [stroke(), stroke(), stroke()];
    update({ kind: 'page', note: note([a, b, c]), imageKey: null, view: VIEW });
    update({ kind: 'strokeBegin', id: 5, tool: 'feltTip', color: BLACK, width: 80, layer: 0, points: [{ x: 1, y: 2, p: 0.5, t: 10, wall: 1234.5 }] });
    w.batches.length = 0;
    const d = stroke();
    update({ kind: 'strokes', keep: 1, tail: [d], ended: 5 });
    expect(w.batches).toHaveLength(1);
    const [edited, cancel] = w.batches[0].events;
    expect(edited).toMatchObject({ type: 'strokesEdited', pageIndex: 0, removed: [1, 2], inserted: [{ index: 1, stroke: d }], recoloured: [], canUndo: false, canRedo: false });
    expect(cancel).toEqual({ type: 'strokeCancel', pageIndex: 0, pen: 'computer', id: 5 });
    expect(mockProjectionState().note?.strokes).toHaveLength(2);
    expect(mockProjectionState().current).toBeNull();
  });

  it('the stroke in progress as the computer’s, each point received at its pointer sample’s wall time', async () => {
    const w = await subscribed();
    update({ kind: 'page', note: note([]), imageKey: null, view: VIEW });
    w.batches.length = 0;
    update({ kind: 'strokeBegin', id: 2, tool: 'feltTip', color: BLACK, width: 80, layer: 0, points: [{ x: 1, y: 2, p: 0.5, t: 10, wall: 1000 }] });
    update({ kind: 'strokePoints', id: 2, points: [{ x: 3, y: 4, p: 0.6, t: 18, wall: 1008 }] });
    // Another id's points, and another's cancel: ignored.
    update({ kind: 'strokePoints', id: 9, points: [{ x: 3, y: 4, p: 0.6, t: 18, wall: 1008 }] });
    update({ kind: 'strokeCancel', id: 9 });
    update({ kind: 'strokeCancel', id: 2 });
    const events = w.events();
    expect(events.map((e) => e.type)).toEqual(['strokeStart', 'points', 'strokeCancel']);
    expect(events[0]).toMatchObject({ pen: 'computer', id: 2, stroke: { tool: 'feltTip', points: [{ x: 1, y: 2, p: 0.5, t: 10, recv: 1000 }] } });
    expect(events[1]).toMatchObject({ pen: 'computer', id: 2, points: [{ x: 3, y: 4, p: 0.6, t: 18, recv: 1008 }] });
  });

  it('a late subscriber gets the snapshot and the stroke in progress; updates without one are kept', async () => {
    update({ kind: 'page', note: note([stroke()]), imageKey: null, view: VIEW });
    update({ kind: 'strokes', keep: 1, tail: [stroke()], ended: null });
    update({ kind: 'strokeBegin', id: 3, tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [{ x: 1, y: 1, p: 1, t: 1, wall: 1 }] });
    const w = await subscribed();
    expect(w.snapshot.page.strokes).toHaveLength(2);
    expect(w.types()).toEqual(['snapshot', 'strokeStart']);
  });

  it('refuses stale and invalid updates', () => {
    expect(refusal(() => update({ kind: 'strokes', keep: 0, tail: [], ended: null }))).toBe('projectionStale');
    expect(refusal(() => update({ kind: 'view', view: VIEW }))).toBe('projectionStale');
    expect(refusal(() => update({ kind: 'strokeBegin', id: 1, tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [] }))).toBe('projectionStale');
    expect(refusal(() => update({ kind: 'strokePoints', id: 1, points: [] }))).toBe('projectionStale');
    // A cancel without a page does nothing.
    expect(refusal(() => update({ kind: 'strokeCancel', id: 1 }))).toBeNull();
    expect(refusal(() => update({ kind: 'page', note: note([], { page: { width: 0, height: 100 } }), imageKey: null, view: VIEW }))).toBe('projectionInvalid');
    expect(refusal(() => update({ kind: 'page', note: note([], { layers: [] }), imageKey: null, view: VIEW }))).toBe('projectionInvalid');
    update({ kind: 'page', note: note([stroke()]), imageKey: null, view: VIEW });
    expect(refusal(() => update({ kind: 'strokes', keep: 2, tail: [], ended: null }))).toBe('projectionStale');
    expect(refusal(() => update({ kind: 'strokes', keep: 0.5, tail: [], ended: null }))).toBe('projectionInvalid');
    expect(refusal(() => update({ kind: 'strokes', keep: 0, tail: [], ended: -1 }))).toBe('projectionInvalid');
    expect(refusal(() => update({ kind: 'strokeBegin', id: 1.5, tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [] }))).toBe('projectionInvalid');
    expect(
      refusal(() => update({ kind: 'strokeBegin', id: 1, tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [{ x: Number.NaN, y: 0, p: 0, t: 0, wall: 0 }] })),
    ).toBe('projectionInvalid');
    expect(refusal(() => update({ kind: 'strokeBegin', id: 1, tool: 'ballpoint', color: BLACK, width: 60, layer: -1, points: [] }))).toBe('projectionInvalid');
    expect(refusal(() => update({ kind: 'strokePoints', id: 1, points: Array.from({ length: 4097 }, () => ({ x: 0, y: 0, p: 0, t: 0, wall: 0 })) }))).toBe('projectionInvalid');
    // A view is kept on the page, never refused.
    update({ kind: 'view', view: { zoom: 20, x: -500, y: 1e9 } });
    expect(mockProjectionState().view).toEqual({ zoom: 8, x: 0, y: 21600 });
  });

  it('sends nothing for the same view again, nor for a strokes update that changes nothing', async () => {
    update({ kind: 'page', note: note([stroke()]), imageKey: null, view: VIEW });
    const w = await subscribed();
    w.batches.length = 0;
    update({ kind: 'view', view: { zoom: 2, x: 10000, y: 9000 } });
    update({ kind: 'view', view: { zoom: 2, x: 10000, y: 9000 } });
    update({ kind: 'strokes', keep: 1, tail: [], ended: null });
    expect(w.types()).toEqual(['view']);
  });

  it('a begin while a stroke is in progress cancels that one first; ended for another id is ignored', async () => {
    update({ kind: 'page', note: note([]), imageKey: null, view: VIEW });
    const w = await subscribed();
    w.batches.length = 0;
    update({ kind: 'strokeBegin', id: 1, tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [] });
    update({ kind: 'strokeBegin', id: 2, tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [] });
    expect(w.batches.at(-1)!.events).toEqual([
      { type: 'strokeCancel', pageIndex: 0, pen: 'computer', id: 1 },
      expect.objectContaining({ type: 'strokeStart', id: 2 }),
    ]);
    update({ kind: 'strokes', keep: 0, tail: [stroke()], ended: 1 });
    expect(w.batches.at(-1)!.events.map((e) => e.type)).toEqual(['strokesEdited']);
    expect(mockProjectionState().current?.id).toBe(2);
  });

  it('keeps and sends at most 100 000 points of the stroke in progress', async () => {
    update({ kind: 'page', note: note([]), imageKey: null, view: VIEW });
    update({ kind: 'strokeBegin', id: 1, tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [] });
    const batch = Array.from({ length: 4096 }, (_, i) => ({ x: i, y: 0, p: 0.5, t: i, wall: i }));
    for (let n = 0; n < 24; n++) update({ kind: 'strokePoints', id: 1, points: batch });
    expect(mockProjectionState().current?.points).toBe(24 * 4096);
    const w = await subscribed();
    w.batches.length = 0;
    update({ kind: 'strokePoints', id: 1, points: batch });
    update({ kind: 'strokePoints', id: 1, points: batch });
    expect(mockProjectionState().current?.points).toBe(100_000);
    const sent = w.events().flatMap((e) => (e.type === 'points' ? e.points : []));
    expect(sent).toHaveLength(100_000 - 24 * 4096);
  });
});

describe('the feed, the stand-in and the window agree', () => {
  beforeEach(() => {
    mockProjectionInvoke('projection_open', { monitor: null });
  });

  it('the window draws what the editor shows, the stroke in progress included', async () => {
    const live = new LiveState();
    live.initial(await mockProjectionSubscribe((batch) => live.receive(batch)));
    const feed = new ProjectionFeed(async (u) => {
      update(u);
    });
    const strokes = Array.from({ length: 6 }, () => stroke());
    feed.show(note(strokes), null);
    await flush();
    expect(live.strokes).toHaveLength(6);
    expect(live.background.paper).toBe('cream');
    // A stroke being drawn shows; lifted, it is one of the page's.
    const points = [{ x: 5, y: 5, pressure: 0.5, t: 100 }];
    const begin: InProgress = { kind: 'begin', id: 1, tool: 'feltTip', color: BLACK, width: 80, layer: 0, points, wallAtBase: 1000, base: 100 };
    feed.stroke(begin);
    await flush();
    expect(live.computerCurrent?.points).toHaveLength(1);
    points.push({ x: 6, y: 6, pressure: 0.5, t: 108 });
    feed.stroke({ kind: 'grow', id: 1, points });
    await flush();
    expect(live.computerCurrent?.points).toHaveLength(2);
    expect(live.strokeCount).toBe(7);
    feed.stroke({ kind: 'end', id: 1, committed: true });
    const finished = { ...stroke(), points: points.map((p) => ({ ...p })) };
    const after = [...strokes, finished];
    feed.show(note(after), null);
    await flush();
    expect(live.computerCurrent).toBeNull();
    expect(live.strokes).toHaveLength(7);
    expect(live.strokeCount).toBe(7);
    // Undo, then the middle changed, then the paper: the window follows each.
    feed.show(note(strokes), null);
    await flush();
    expect(live.strokes).toHaveLength(6);
    const moved = [...strokes.slice(0, 4), { ...strokes[4], color: { r: 200, g: 0, b: 0, a: 255 } }, strokes[5]];
    feed.show(note(moved), null);
    await flush();
    expect(live.strokes[4].color.r).toBe(200);
    expect(live.strokes[5]).toEqual(strokes[5]);
    feed.show(note(moved, { background: { paper: 'white', template: 'lined', texture: false } }), null);
    await flush();
    expect(live.background.template).toBe('lined');
    expect(live.strokes).toHaveLength(6);
    expect(feed.rejections).toBe(0);
  });

  it('repairs after a stale refusal: the page again', async () => {
    const live = new LiveState();
    live.initial(await mockProjectionSubscribe((batch) => live.receive(batch)));
    const sent: ProjectionUpdate['kind'][] = [];
    const feed = new ProjectionFeed(async (u) => {
      sent.push(u.kind);
      update(u);
    });
    const strokes = Array.from({ length: 4 }, () => stroke());
    feed.show(note(strokes), null);
    await flush();
    // The window was closed and opened again behind the feed's back: Rust has no scene.
    mockProjectionClosedByWindow();
    mockProjectionInvoke('projection_open', { monitor: null });
    live.initial(await mockProjectionSubscribe((batch) => live.receive(batch)));
    feed.show(note([...strokes, stroke()]), null);
    await flush();
    expect(sent).toEqual(['page', 'strokes', 'page']);
    expect(feed.rejections).toBe(1);
    expect(live.strokes).toHaveLength(5);
  });

  it('every batch the window gets is one of live’s own events', async () => {
    const w = await subscribed();
    const feed = new ProjectionFeed(async (u) => {
      update(u);
    });
    feed.show(note([stroke()]), null);
    feed.view({ zoom: 2, x: 10000, y: 9000 });
    await flush();
    const known: LiveEvent['type'][] = ['snapshot', 'strokeStart', 'points', 'strokeCancel', 'strokesEdited', 'view'];
    expect(w.types().every((type) => known.includes(type))).toBe(true);
  });
});
