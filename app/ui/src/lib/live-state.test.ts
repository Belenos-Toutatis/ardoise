import { afterEach, describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveEvent, LivePoint, LiveSnapshot, LiveStroke, LiveViewState } from './live';
import { DEFAULT_VIEW, LiveState, VIEW_SEND_MS, subscribeLive } from './live-state.svelte';
import type { Note, Stroke } from './note';

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const RED = { r: 0xd8, g: 0, b: 0, a: 255 };

function page(id: string, strokes: Stroke[] = [], extra: Partial<Note> = {}): Note {
  return {
    id,
    title: '',
    created: 1_792_000_000_000,
    modified: 1_792_000_000_000,
    page: { width: 29700, height: 21600 },
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes,
    timing: 'recorded',
    origin: { kind: 'live' },
    ...extra,
  };
}

function stroke(x: number, n = 3, layer = 0): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer,
    points: Array.from({ length: n }, (_, i) => ({ x: x + 10 * i, y: 500, pressure: 0.5, t: 5 * i })),
  };
}

function snapshot(extra: Partial<LiveSnapshot> = {}): LiveSnapshot {
  return {
    state: 'live',
    page: page('p1'),
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { ...DEFAULT_VIEW, x: 14850, y: 10800 },
    hover: null,
    presentation: null,
    saved: [],
    ...extra,
  };
}

let clock = 1_792_000_000_000;
function points(n: number, x0 = 1000): LivePoint[] {
  return Array.from({ length: n }, (_, i) => ({ x: x0 + 10 * i, y: 700, p: 0.6, t: 5 * i, recv: (clock += 5) }));
}

function liveStroke(pts: LivePoint[], extra: Partial<LiveStroke> = {}): LiveStroke {
  return { tool: 'feltTip', color: RED, width: 120, layer: 0, points: pts, ...extra };
}

const batch = (...events: LiveEvent[]): LiveBatch => ({ events, sentAt: clock });

function ready(extra: Partial<LiveSnapshot> = {}): LiveState {
  const live = new LiveState();
  live.initial(snapshot(extra));
  return live;
}

afterEach(() => {
  vi.useRealTimers();
});

describe('live state: strokes', () => {
  it('applies the snapshot, then strokes as they are drawn', () => {
    const live = ready({ page: page('p1', [stroke(100)]), saved: ['C:\\a.ardoise'] });
    expect(live.ready).toBe(true);
    expect(live.strokes).toHaveLength(1);
    expect(live.strokeCount).toBe(1);
    expect(live.saved).toEqual(['C:\\a.ardoise']);
    const changes = vi.fn();
    live.subscribe(changes);

    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(points(2)) }), 1);
    expect(live.current?.points).toHaveLength(2);
    expect(live.current?.points[0]).toEqual({ x: 1000, y: 700, pressure: 0.6, t: 0 });
    expect(live.current?.tool).toBe('feltTip');
    expect(live.strokeCount).toBe(2);
    live.receive(batch({ type: 'points', pageIndex: 0, points: points(3, 1020) }), 2);
    expect(live.current?.points).toHaveLength(5);
    live.receive(batch({ type: 'strokeEnd', pageIndex: 0 }), 3);
    expect(live.current).toBeNull();
    expect(live.strokes).toHaveLength(2);
    expect(live.strokes[1].points).toHaveLength(5);
    expect(live.strokeCount).toBe(2);
    expect(changes).toHaveBeenCalledTimes(3);
    // Appended in place: the canvases add the stroke to their cache.
    expect(live.generation).toBe(1);
  });

  it('keeps the receipt and arrival of each point until a frame paints it', () => {
    const live = ready();
    const pts = points(3);
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(pts) }), 4242);
    const pending = live.takeUnpainted();
    expect(pending).toEqual(pts.map((p) => ({ recv: p.recv, arrival: 4242 })));
    expect(live.takeUnpainted()).toEqual([]);
  });

  it('ignores strokes of another page and starts a missing stroke from the style', () => {
    const live = ready({ layer: 2, page: page('p1', [], { layers: [0, 1, 2].map((i) => ({ name: `L${i}`, visible: true })) }) });
    live.receive(batch({ type: 'strokeStart', pageIndex: 3, stroke: liveStroke(points(2)) }));
    expect(live.current).toBeNull();
    // Subscribed in the middle of a stroke: its points still show.
    live.receive(batch({ type: 'points', pageIndex: 0, points: points(2) }));
    expect(live.current).toMatchObject({ tool: 'ballpoint', layer: 2 });
    expect(live.strokeCount).toBe(1);
    live.receive(batch({ type: 'strokeEnd', pageIndex: 1 }));
    expect(live.current).not.toBeNull();
    // A new stroke without the end of the previous one: the previous one is finished.
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(points(1)) }));
    expect(live.strokes).toHaveLength(1);
    expect(live.strokeCount).toBe(2);
  });

  it('does not draw twice a stroke in progress that a snapshot already holds', () => {
    const live = ready({ page: page('p1', [stroke(100)]) });
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(points(2)) }));
    const inProgress = live.current;
    const generation = live.generation;
    // A style change mid-stroke: the snapshot's page holds the finished stroke…
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ page: page('p1', [stroke(100)]) }) }));
    expect(live.current).toBe(inProgress);
    expect(live.strokes).toHaveLength(1);
    expect(live.generation).toBe(generation);
    // …or also the one in progress (with the points it had then).
    const partial: Stroke = { ...inProgress!, points: inProgress!.points.slice(0, 1) };
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ page: page('p1', [stroke(100), partial]) }) }));
    expect(live.current).toBe(inProgress);
    expect(live.strokes).toHaveLength(1);
    expect(live.strokeCount).toBe(2);
  });
});

describe('live state: pages, layers, clearing', () => {
  it('changes page: the strokes are replaced and the stroke in progress dropped', () => {
    const live = ready({ page: page('p1', [stroke(100), stroke(200)]) });
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(points(2)) }));
    const generation = live.generation;
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ page: page('p2'), pageIndex: 1, pageCount: 2 }) }));
    expect(live.pageIndex).toBe(1);
    expect(live.pageCount).toBe(2);
    expect(live.noteId).toBe('p2');
    expect(live.strokes).toEqual([]);
    expect(live.current).toBeNull();
    expect(live.strokeCount).toBe(0);
    expect(live.generation).toBe(generation + 1);
    // Back to the first page.
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ page: page('p1', [stroke(100), stroke(200)]), pageCount: 2 }) }));
    expect(live.strokes).toHaveLength(2);
    expect(live.generation).toBe(generation + 2);
  });

  it('clears the page', () => {
    const live = ready({ page: page('p1', [stroke(100), stroke(200)]) });
    const generation = live.generation;
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ page: page('p1') }) }));
    expect(live.strokes).toEqual([]);
    expect(live.strokeCount).toBe(0);
    expect(live.generation).toBe(generation + 1);
  });

  it('keeps the strokes (and their outlines) when a snapshot does not change them', () => {
    const live = ready({ page: page('p1', [stroke(100)]) });
    const kept = live.strokes[0];
    const generation = live.generation;
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ page: page('p1', [stroke(100)]), style: { tool: 'chalk', color: RED, width: 90 } }) }));
    expect(live.strokes[0]).toBe(kept);
    expect(live.generation).toBe(generation);
    expect(live.style.tool).toBe('chalk');
  });

  it('follows the layer receiving new strokes', () => {
    const live = ready();
    const layers = [
      { name: 'Calque 1', visible: true },
      { name: 'Calque 2', visible: true },
    ];
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ page: page('p1', [], { layers }), layer: 1 }) }));
    expect(live.layer).toBe(1);
    expect(live.layers).toEqual(layers);
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(points(2), { layer: 1 }) }));
    expect(live.current?.layer).toBe(1);
  });

  it('merges the saved notes, whichever way Rust reports them', () => {
    const live = ready({ saved: ['a'] });
    live.receive(batch({ type: 'saved', paths: ['a', 'b'] }));
    live.receive(batch({ type: 'saved', paths: ['c'] }));
    expect(live.saved).toEqual(['a', 'b', 'c']);
  });

  it('keeps the pages that could not be written until a save succeeds', () => {
    const live = ready();
    expect(live.saveErrors).toEqual([]);
    live.receive(batch({ type: 'saveFailed', pageIndex: 1, message: 'disk full' }));
    live.receive(batch({ type: 'saveFailed', pageIndex: 0, message: 'access denied' }));
    expect(live.saveErrors).toEqual([
      { pageIndex: 0, message: 'access denied' },
      { pageIndex: 1, message: 'disk full' },
    ]);
    // The same page failing again shows its latest message, once.
    live.receive(batch({ type: 'saveFailed', pageIndex: 1, message: 'disk still full' }));
    expect(live.saveErrors).toHaveLength(2);
    expect(live.saveErrors[1]).toEqual({ pageIndex: 1, message: 'disk still full' });
    // Other events, even a snapshot, do not clear it.
    live.receive(batch({ type: 'hover', x: 1, y: 1 }, { type: 'snapshot', snapshot: snapshot() }));
    expect(live.saveErrors).toHaveLength(2);
    // A successful save means that everything is written.
    live.receive(batch({ type: 'saved', paths: ['a'] }));
    expect(live.saveErrors).toEqual([]);
  });
});

describe('live state: hover, view, presentation', () => {
  it('follows the hovering pen, and forgets it when the pen writes or leaves', () => {
    const live = ready();
    live.receive(batch({ type: 'hover', x: 300, y: 400 }));
    expect(live.hover).toEqual({ x: 300, y: 400 });
    live.receive(batch({ type: 'hoverEnd' }));
    expect(live.hover).toBeNull();
    live.receive(batch({ type: 'hover', x: 1, y: 2 }));
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(points(1)) }));
    expect(live.hover).toBeNull();
  });

  it('applies views and presentation changes from any window', () => {
    const live = ready();
    const view: LiveViewState = { zoom: 2, x: 9000, y: 7000, board: true, laser: true };
    live.receive(batch({ type: 'view', view }));
    expect(live.view).toEqual(view);
    const presentation = { key: 'k', name: 'n', x: 0, y: 0, width: 1280, height: 800, scale: 1, primary: false, chosen: true };
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ presentation, view }) }));
    expect(live.presentation).toEqual(presentation);
  });

  it('shows a local view at once and ignores the older echoes of a drag', async () => {
    vi.useFakeTimers();
    const live = ready();
    const sent: LiveViewState[] = [];
    const send = async (v: LiveViewState) => {
      sent.push(v);
    };
    const at = (x: number): LiveViewState => ({ zoom: 2, x, y: 5000, board: false, laser: false });
    live.changeView(at(1000.4), send);
    live.changeView(at(2000), send);
    live.changeView(at(3000), send);
    // Shown at once (rounded as sent), sent at most once per interval.
    expect(live.view).toEqual(at(3000));
    expect(sent).toEqual([at(1000)]);
    // The echo of the first view comes back while the third waits: the third stays.
    live.receive(batch({ type: 'view', view: at(1000) }));
    expect(live.view).toEqual(at(3000));
    await vi.advanceTimersByTimeAsync(VIEW_SEND_MS);
    expect(sent).toEqual([at(1000), at(3000)]);
    live.receive(batch({ type: 'view', view: at(3000) }));
    expect(live.view).toEqual(at(3000));
    // Another window moves the view afterwards: followed.
    live.receive(batch({ type: 'view', view: at(7000) }));
    expect(live.view).toEqual(at(7000));
  });

  it('shows Rust’s view again when a change is refused', async () => {
    const live = ready();
    const before = live.view;
    live.changeView({ ...before, laser: true }, () => Promise.reject(new Error('refused')));
    expect(live.view.laser).toBe(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(live.view).toEqual(before);
  });

  it('shows a style at once, keeps it over older echoes, restores it when refused', async () => {
    const live = ready();
    const chalk = { tool: 'chalk' as const, color: RED, width: 150 };
    let answer!: () => void;
    const pending = live.changeStyle(chalk, () => new Promise<void>((resolve) => (answer = resolve)));
    expect(live.style).toEqual(chalk);
    // A snapshot sent before the change was applied still has the old style.
    live.receive(batch({ type: 'snapshot', snapshot: snapshot() }));
    expect(live.style).toEqual(chalk);
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ style: chalk }) }));
    answer();
    await pending;
    expect(live.style).toEqual(chalk);
    await expect(live.changeStyle({ ...chalk, width: 300 }, () => Promise.reject(new Error('no')))).rejects.toThrow('no');
    expect(live.style).toEqual(chalk);
    live.previewStyle({ ...chalk, width: 400 });
    expect(live.style.width).toBe(400);
  });
});

describe('live state: subscription', () => {
  it('applies batches that arrive before the subscription snapshot after it', async () => {
    const live = new LiveState();
    let push!: (b: LiveBatch) => void;
    let answer!: (s: LiveSnapshot) => void;
    const subscribed = subscribeLive(live, (onBatch) => {
      push = onBatch;
      return new Promise<LiveSnapshot>((resolve) => (answer = resolve));
    });
    push(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(points(2)) }));
    expect(live.ready).toBe(false);
    expect(live.current).toBeNull();
    answer(snapshot({ page: page('p1', [stroke(100)]) }));
    await subscribed;
    expect(live.ready).toBe(true);
    expect(live.strokes).toHaveLength(1);
    expect(live.current?.points).toHaveLength(2);
  });

  it('takes the start snapshot only when nothing newer came', () => {
    const live = new LiveState();
    live.adoptStart(snapshot({ state: 'waiting' }));
    expect(live.state).toBe('waiting');
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ state: 'live' }) }));
    live.adoptStart(snapshot({ state: 'waiting' }));
    expect(live.state).toBe('live');
    const idle = ready({ state: 'idle' });
    idle.adoptStart(snapshot({ state: 'waiting' }));
    expect(idle.state).toBe('waiting');
  });

  it('ignores everything once closed', () => {
    const live = ready();
    const changes = vi.fn();
    live.subscribe(changes);
    live.close();
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(points(2)) }));
    live.changeView({ ...live.view, zoom: 3 }, () => Promise.resolve());
    expect(live.current).toBeNull();
    expect(live.view.zoom).toBe(1);
    expect(changes).not.toHaveBeenCalled();
  });
});
