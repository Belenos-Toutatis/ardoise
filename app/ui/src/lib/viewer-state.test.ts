import { describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveEvent, LivePoint, LiveSnapshot } from './live';
import { DEFAULT_VIEW } from './live-state.svelte';
import type { Layer, Note, Stroke } from './note';
import type { SharedPage } from './share';
import { ViewerState, noteSnapshot } from './viewer-state.svelte';

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const A4 = { width: 29700, height: 21600 };
const LAYERS: Layer[] = [{ name: 'Calque 1', visible: true }];

function stroke(x: number, n = 3, extra: Partial<Stroke> = {}): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer: 0,
    points: Array.from({ length: n }, (_, i) => ({ x: x + 10 * i, y: 500, pressure: 0.5, t: 5 * i })),
    ...extra,
  };
}

function shared(id: string, strokes: Stroke[] = [], extra: Partial<SharedPage> = {}): SharedPage {
  return {
    id,
    page: A4,
    background: { paper: 'white', template: 'lined', texture: false },
    layers: LAYERS,
    strokes,
    ...extra,
  };
}

function livePage(id: string, strokes: Stroke[] = []): Note {
  return {
    id,
    title: '',
    created: 0,
    modified: 0,
    page: A4,
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: LAYERS,
    strokes,
    timing: 'recorded',
    origin: { kind: 'live' },
  };
}

function snapshot(id = 'p1', strokes: Stroke[] = [], extra: Partial<LiveSnapshot> = {}): LiveSnapshot {
  return {
    state: 'live',
    page: livePage(id, strokes),
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

let clock = 0;
function points(n: number, x0 = 1000): LivePoint[] {
  return Array.from({ length: n }, (_, i) => ({ x: x0 + 10 * i, y: 700, p: 0.6, t: 5 * i, recv: (clock += 5) }));
}

const batch = (...events: LiveEvent[]): LiveBatch => ({ events, sentAt: clock });
const start = (pts: LivePoint[]): LiveEvent => ({
  type: 'strokeStart',
  pageIndex: 0,
  stroke: { tool: 'feltTip', color: BLACK, width: 120, layer: 0, points: pts },
});

describe('viewer state: connection', () => {
  it('starts connecting, with an empty sheet', () => {
    const v = new ViewerState();
    expect(v.screen).toBe('connecting');
    expect(v.shown).toBeNull();
    expect(v.reconnecting).toBe(false);
    expect(v.paused).toBe(false);
    expect(v.background).toBeNull();
    expect(v.live.ready).toBe(false);
  });

  it('asks the code on 401, forgetting what was shown', () => {
    const v = new ViewerState();
    v.handle({ type: 'note', page: shared('n1', [stroke(100)]), background: 'data:image/png;base64,AA' });
    v.handle({ type: 'pause', paused: true });
    v.status('needsCode');
    expect(v.screen).toBe('code');
    expect(v.paused).toBe(false);
    expect(v.background).toBeNull();
    expect(v.live.strokes).toHaveLength(0);
  });

  it('flags reconnecting and too many screens, and clears them when the stream opens', () => {
    const v = new ViewerState();
    v.status('tooMany');
    expect(v.tooMany).toBe(true);
    v.status('reconnecting');
    expect(v.reconnecting).toBe(true);
    expect(v.tooMany).toBe(false);
    v.status('open');
    expect(v.reconnecting).toBe(false);
    expect(v.tooMany).toBe(false);
  });

  it('keeps the last image while reconnecting', () => {
    const v = new ViewerState();
    v.handle({ type: 'note', page: shared('n1', [stroke(100)]), background: null });
    v.status('reconnecting');
    expect(v.screen).toBe('note');
    expect(v.live.strokes).toHaveLength(1);
    expect(v.reconnecting).toBe(true);
  });
});

describe('viewer state: waiting and end', () => {
  it('shows the waiting screen and empties the sheet', () => {
    const v = new ViewerState();
    v.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: snapshot('p1', [stroke(1)]) }) });
    v.handle({ type: 'waiting' });
    expect(v.screen).toBe('waiting');
    expect(v.shown).toBeNull();
    expect(v.live.ready).toBe(false);
  });

  it('shows "ended" on the end message, and again on the status that follows; a new share asks for the code', () => {
    const v = new ViewerState();
    v.handle({ type: 'note', page: shared('n1', [stroke(100)]), background: null });
    v.handle({ type: 'pause', paused: true });
    v.handle({ type: 'end' });
    expect(v.screen).toBe('ended');
    expect(v.paused).toBe(false);
    const sheet = v.live;
    v.status('ended');
    v.status('ended');
    expect(v.screen).toBe('ended');
    expect(v.live).toBe(sheet);
    v.status('needsCode');
    expect(v.screen).toBe('code');
  });

  it('ends a reconnection too: "ended" is not "connection lost"', () => {
    const v = new ViewerState();
    v.status('reconnecting');
    v.handle({ type: 'end' });
    expect(v.reconnecting).toBe(false);
  });
});

describe('viewer state: pause', () => {
  it('shows and hides the badge', () => {
    const v = new ViewerState();
    v.handle({ type: 'waiting' });
    v.handle({ type: 'pause', paused: true });
    expect(v.paused).toBe(true);
    expect(v.screen).toBe('waiting');
    v.handle({ type: 'pause', paused: false });
    expect(v.paused).toBe(false);
  });
});

describe('viewer state: a stream opening again', () => {
  it('drops a pause badge left from before the cut (a paused share says so again)', () => {
    const v = new ViewerState();
    v.handle({ type: 'waiting' });
    v.handle({ type: 'pause', paused: true });
    v.status('reconnecting');
    v.status('open');
    expect(v.paused).toBe(false);
    v.handle({ type: 'pause', paused: true });
    expect(v.paused).toBe(true);
  });
});

describe('viewer state: a note', () => {
  it('keeps the paper shown when an update of the page comes without one', () => {
    const v = new ViewerState();
    v.handle({ type: 'note', page: shared('n1', [stroke(100), stroke(400)]), background: 'data:image/png;base64,AA' });
    v.handle({ type: 'note', page: shared('n1', [stroke(100)]) });
    expect(v.background).toBe('data:image/png;base64,AA');
    expect(v.live.strokes).toHaveLength(1);
    // A note given with plain paper drops the picture; one after the waiting screen has none.
    v.handle({ type: 'note', page: shared('n1'), background: null });
    expect(v.background).toBeNull();
    v.handle({ type: 'note', page: shared('n2'), background: 'data:image/png;base64,BB' });
    v.handle({ type: 'waiting' });
    v.handle({ type: 'note', page: shared('n2') });
    expect(v.background).toBeNull();
  });

  it('becomes a snapshot of one page, the whole page in view, idle, no laser', () => {
    const v = new ViewerState();
    const strokes = [stroke(100), stroke(400)];
    v.handle({ type: 'note', page: shared('n1', strokes), background: 'data:image/png;base64,AA' });
    expect(v.screen).toBe('note');
    expect(v.shown).toBe('note');
    expect(v.background).toBe('data:image/png;base64,AA');
    const live = v.live;
    expect(live.ready).toBe(true);
    expect(live.state).toBe('idle');
    expect(live.pageIndex).toBe(0);
    expect(live.pageCount).toBe(1);
    expect(live.pageSize).toEqual(A4);
    expect(live.view).toEqual({ zoom: 1, x: 14850, y: 10800, board: false, laser: false });
    expect(live.hover).toBeNull();
    expect(live.presentation).toBeNull();
    expect(live.saved).toEqual([]);
    expect(live.noteId).toBe('n1');
    expect(live.background).toEqual({ paper: 'white', template: 'lined', texture: false });
    expect(live.layers).toEqual(LAYERS);
    expect(live.strokes).toEqual(strokes);
    expect(live.strokeCount).toBe(2);
    expect(live.current).toBeNull();
  });

  it('holds a portrait page as a portrait', () => {
    const snap = noteSnapshot(shared('n1', [], { page: { width: 21600, height: 29700 } }));
    expect(snap.orientation).toBe('portraitLeft');
    expect(snap.view).toEqual({ zoom: 1, x: 10800, y: 14850, board: false, laser: false });
    expect(noteSnapshot(shared('n1')).orientation).toBe('landscape');
  });

  it('has no private field of a note', () => {
    const snap = noteSnapshot(shared('n1'));
    expect(snap.page.title).toBe('');
    expect(snap.page.tags).toEqual([]);
    expect(snap.page.origin).toEqual({ kind: 'ardoise' });
  });

  it('appends strokes as finished strokes: the strokes grow in place, same generation, listeners told', () => {
    const v = new ViewerState();
    v.handle({ type: 'note', page: shared('n1', [stroke(100)]), background: null });
    const live = v.live;
    const array = live.strokes;
    const generation = live.generation;
    const changed = vi.fn();
    live.subscribe(changed);

    v.handle({ type: 'noteStrokes', strokes: [stroke(500, 4), stroke(900, 2, { tool: 'highlighter' })] });
    expect(v.live).toBe(live);
    expect(live.strokes).toBe(array);
    expect(live.strokes).toHaveLength(3);
    expect(live.generation).toBe(generation);
    expect(live.strokeCount).toBe(3);
    expect(live.current).toBeNull();
    expect(live.strokes[1].points).toEqual(stroke(500, 4).points);
    expect(live.strokes[2].tool).toBe('highlighter');
    expect(changed).toHaveBeenCalled();
    // Nothing waits for a frame forever.
    expect(live.takeUnpainted().length).toBeGreaterThan(0);
    expect(live.takeUnpainted()).toEqual([]);
  });

  it('draws again a note with the same id: a new sheet and a new array, even when only a middle point moved', () => {
    const v = new ViewerState();
    const before = stroke(100, 5);
    v.handle({ type: 'note', page: shared('n1', [before]), background: null });
    const first = v.live;
    const edited = stroke(100, 5);
    edited.points[2] = { ...edited.points[2], y: 9999 };
    v.handle({ type: 'note', page: shared('n1', [edited]), background: null });
    expect(v.live).not.toBe(first);
    expect(v.live.strokes).not.toBe(first.strokes);
    expect(v.live.strokes[0].points[2].y).toBe(9999);
    expect(v.live.generation).toBeGreaterThan(0);
  });

  it('stops the old sheet and tells nothing to its listeners afterwards', () => {
    const v = new ViewerState();
    v.handle({ type: 'note', page: shared('n1'), background: null });
    const old = v.live;
    const changed = vi.fn();
    old.subscribe(changed);
    v.handle({ type: 'note', page: shared('n1', [stroke(1)]), background: null });
    old.receive(batch({ type: 'hover', x: 1, y: 2 }));
    expect(changed).not.toHaveBeenCalled();
  });

  it('ignores strokes added while no note is shown', () => {
    const v = new ViewerState();
    v.handle({ type: 'noteStrokes', strokes: [stroke(1)] });
    expect(v.live.strokes).toHaveLength(0);
    v.handle({ type: 'waiting' });
    v.handle({ type: 'noteStrokes', strokes: [stroke(1)] });
    expect(v.live.strokes).toHaveLength(0);
    expect(v.screen).toBe('waiting');
  });

  it('shows the paper of the message, and none for plain paper', () => {
    const v = new ViewerState();
    v.handle({ type: 'note', page: shared('n1'), background: 'data:image/png;base64,AA' });
    expect(v.background).toBe('data:image/png;base64,AA');
    v.handle({ type: 'note', page: shared('n1'), background: null });
    expect(v.background).toBeNull();
  });
});

describe('viewer state: live mode', () => {
  it('starts from the snapshot of a reset batch, then the stroke in progress', () => {
    const v = new ViewerState();
    v.handle({
      type: 'live',
      reset: true,
      batch: batch({ type: 'snapshot', snapshot: snapshot('p1', [stroke(100)]) }, start(points(2))),
    });
    expect(v.screen).toBe('live');
    expect(v.shown).toBe('live');
    expect(v.background).toBeNull();
    expect(v.live.ready).toBe(true);
    expect(v.live.state).toBe('live');
    expect(v.live.strokes).toHaveLength(1);
    expect(v.live.current?.points).toHaveLength(2);
    expect(v.live.strokeCount).toBe(2);
  });

  it('adds the increments of the following batches to the same sheet', () => {
    const v = new ViewerState();
    v.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: snapshot() }) });
    const live = v.live;
    v.handle({ type: 'live', reset: false, batch: batch(start(points(2))) });
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'points', pageIndex: 0, points: points(3, 1100) }) });
    expect(v.live).toBe(live);
    expect(live.current?.points).toHaveLength(5);
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'strokeEnd', pageIndex: 0 }) });
    expect(live.current).toBeNull();
    expect(live.strokes).toHaveLength(1);
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'view', view: { ...DEFAULT_VIEW, zoom: 2, board: true, x: 5000, y: 5000 } }) });
    expect(live.view.zoom).toBe(2);
    expect(live.view.board).toBe(true);
  });

  it('starts again from a fresh sheet on a new reset (reconnection): no stroke of the old one is left', () => {
    const v = new ViewerState();
    v.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: snapshot('p1') }, start(points(3))) });
    const old = v.live;
    expect(old.current).not.toBeNull();
    v.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: snapshot('p1', [stroke(100)]) }) });
    expect(v.live).not.toBe(old);
    expect(v.live.current).toBeNull();
    expect(v.live.strokes).toHaveLength(1);
    expect(v.live.strokeCount).toBe(1);
  });

  it('goes from a note to live mode and back', () => {
    const v = new ViewerState();
    v.handle({ type: 'note', page: shared('n1', [stroke(1)]), background: 'data:image/png;base64,AA' });
    v.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: snapshot('p9') }) });
    expect(v.screen).toBe('live');
    expect(v.background).toBeNull();
    expect(v.live.noteId).toBe('p9');
    expect(v.live.strokes).toHaveLength(0);
    v.handle({ type: 'note', page: shared('n1', [stroke(1)]), background: null });
    expect(v.screen).toBe('note');
    expect(v.live.noteId).toBe('n1');
  });

  it('ignores increments that have no sheet to join, and a reset without a snapshot', () => {
    const v = new ViewerState();
    v.handle({ type: 'live', reset: false, batch: batch(start(points(2))) });
    expect(v.screen).toBe('connecting');
    v.handle({ type: 'live', reset: true, batch: batch(start(points(2))) });
    expect(v.screen).toBe('connecting');
    v.handle({ type: 'note', page: shared('n1', [stroke(1)]), background: null });
    v.handle({ type: 'live', reset: false, batch: batch(start(points(2))) });
    expect(v.screen).toBe('note');
    expect(v.live.strokes).toHaveLength(1);
    expect(v.live.current).toBeNull();
  });

  it('does not keep what the PC strips, and follows the blackboard and the laser', () => {
    const v = new ViewerState();
    v.handle({
      type: 'live',
      reset: true,
      batch: batch({ type: 'snapshot', snapshot: snapshot('p1', [], { view: { zoom: 1, x: 14850, y: 10800, board: true, laser: true }, hover: { x: 5, y: 6 } }) }),
    });
    expect(v.live.view.board).toBe(true);
    expect(v.live.view.laser).toBe(true);
    expect(v.live.hover).toEqual({ x: 5, y: 6 });
    expect(v.live.saved).toEqual([]);
  });
});
