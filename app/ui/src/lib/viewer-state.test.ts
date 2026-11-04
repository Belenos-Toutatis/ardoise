import { describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveEvent, LiveMark, LivePoint, LiveSnapshot } from './live';
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

  it('shows the marks written over the presentation, the whole list each time, and drops them with the page', () => {
    const red = { r: 216, g: 0, b: 0, a: 255 };
    const mark = (x: number, n: number): LiveMark => ({
      tool: 'feltTip',
      color: red,
      width: 140,
      points: Array.from({ length: n }, (_, i) => ({ x: x + 10 * i, y: 500, p: 0.5 })),
    });
    const v = new ViewerState();
    // A viewer arriving late: the marks follow the snapshot of its first batch.
    v.handle({
      type: 'live',
      reset: true,
      batch: batch({ type: 'snapshot', snapshot: snapshot('p1') }, { type: 'marks', marks: [mark(0, 3)] }),
    });
    const live = v.live;
    expect(live.marks).toHaveLength(1);
    expect(live.marks[0]).toMatchObject({ tool: 'feltTip', layer: 0, color: red, width: 140 });
    expect(live.marks[0].points).toHaveLength(3);
    // Nothing of the note: no stroke, and the count of strokes does not change.
    expect(live.strokes).toHaveLength(0);
    expect(live.strokeCount).toBe(0);

    // The list grows: the marks that did not change are the same objects.
    const first = live.marks[0];
    const told = vi.fn();
    live.subscribe(told);
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'marks', marks: [mark(0, 3), mark(100, 2)] }) });
    expect(live.marks).toHaveLength(2);
    expect(live.marks[0]).toBe(first);
    expect(told).toHaveBeenCalled();

    // A style change of the snapshot (same page) keeps them; another page, or an empty list, removes them.
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'snapshot', snapshot: snapshot('p1', [], { style: { tool: 'chalk', color: red, width: 90 } }) }) });
    expect(live.marks).toHaveLength(2);
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'snapshot', snapshot: snapshot('p2', [], { pageIndex: 1, pageCount: 2 }) }) });
    expect(live.marks).toHaveLength(0);
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'marks', marks: [mark(5, 2)] }) });
    expect(live.marks).toHaveLength(1);
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'marks', marks: [] }) });
    expect(live.marks).toHaveLength(0);

    // A new reset starts without marks, and a note has none.
    v.handle({ type: 'live', reset: false, batch: batch({ type: 'marks', marks: [mark(5, 2)] }) });
    v.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: snapshot('p2') }) });
    expect(v.live.marks).toHaveLength(0);
    v.handle({ type: 'note', page: shared('n1', [stroke(1)]), background: null });
    expect(v.live.marks).toHaveLength(0);
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

describe('viewer state: the paper of a live page', () => {
  const PICTURE = 'data:image/png;base64,QUJD';
  const OTHER = 'data:image/png;base64,REVG';

  function live(snap: LiveSnapshot, reset = true) {
    return { type: 'live', reset, batch: batch({ type: 'snapshot', snapshot: snap }) } as const;
  }
  const sheet = (extra: Partial<LiveSnapshot> = {}) => snapshot('sheet', [], { paperKey: 'k1', ...extra });
  const plain = () => snapshot('next', [], { pageIndex: 1, pageCount: 2 });

  it('has none on an ordinary page, as before', () => {
    const v = new ViewerState();
    v.handle(live(snapshot()));
    expect(v.live.paperKey).toBeNull();
    expect(v.paper).toBeNull();
    expect(v.background).toBeNull();
  });

  it('shows the picture the page asks for, in whichever order they arrive', () => {
    const v = new ViewerState();
    v.handle(live(sheet()));
    expect(v.paper).toBeNull();
    v.handle({ type: 'livePaper', key: 'k1', picture: PICTURE });
    expect(v.paper).toBe(PICTURE);

    const before = new ViewerState();
    before.handle({ type: 'livePaper', key: 'k1', picture: PICTURE });
    expect(before.paper).toBeNull();
    before.handle(live(sheet()));
    expect(before.paper).toBe(PICTURE);
  });

  it('does not touch the sheet or its strokes: the picture goes apart', () => {
    const v = new ViewerState();
    v.handle(live(sheet({ page: { ...snapshot('sheet').page, strokes: [stroke(100)] } })));
    const { live: state, live: { strokes, generation } } = v;
    v.handle({ type: 'livePaper', key: 'k1', picture: PICTURE });
    expect(v.live).toBe(state);
    expect(v.live.strokes).toBe(strokes);
    expect(v.live.generation).toBe(generation);
  });

  it('clears at once on a plain page, and comes back with the sheet without anything sent', () => {
    const v = new ViewerState();
    v.handle(live(sheet()));
    v.handle({ type: 'livePaper', key: 'k1', picture: PICTURE });
    v.handle(live(plain(), false));
    expect(v.paper).toBeNull();
    v.handle(live(sheet(), false));
    expect(v.paper).toBe(PICTURE);
  });

  it('never shows a picture for another paper: the blackboard, a page not yet drawn', () => {
    const v = new ViewerState();
    v.handle(live(sheet()));
    v.handle({ type: 'livePaper', key: 'k1', picture: PICTURE });
    v.handle(live(sheet({ paperKey: 'k2', view: { ...DEFAULT_VIEW, board: true } }), false));
    expect(v.paper).toBeNull();
    v.handle({ type: 'livePaper', key: 'k2', picture: OTHER });
    expect(v.paper).toBe(OTHER);
    // A later picture replaces it; a picture for a key nobody asks for shows nothing.
    v.handle({ type: 'livePaper', key: 'k3', picture: PICTURE });
    expect(v.paper).toBeNull();
  });

  it('survives what replaces the sheet: a new subscription, a pause, a reconnection', () => {
    const v = new ViewerState();
    v.handle(live(sheet()));
    v.handle({ type: 'livePaper', key: 'k1', picture: PICTURE });
    const first = v.live;
    // Pause: the frozen image, then the badge; resume: a fresh snapshot.
    v.handle(live(sheet()));
    expect(v.live).not.toBe(first);
    v.handle({ type: 'pause', paused: true });
    expect(v.paper).toBe(PICTURE);
    v.handle({ type: 'pause', paused: false });
    v.handle(live(sheet()));
    expect(v.paper).toBe(PICTURE);
    // The connection is cut: the last image stays, and the new stream brings the picture anyway.
    v.status('reconnecting');
    expect(v.paper).toBe(PICTURE);
    v.status('open');
    v.handle(live(sheet()));
    expect(v.paper).toBe(PICTURE);
  });

  it('leaves the note its own paper, and comes back after it', () => {
    const v = new ViewerState();
    v.handle(live(sheet()));
    v.handle({ type: 'livePaper', key: 'k1', picture: PICTURE });
    v.handle({ type: 'note', page: shared('n1'), background: OTHER });
    expect(v.paper).toBe(OTHER);
    v.handle({ type: 'note', page: shared('n2'), background: null });
    expect(v.paper).toBeNull();
    // The PC remembers having sent it: back in live mode it does not send it again.
    v.handle(live(sheet()));
    expect(v.paper).toBe(PICTURE);
    expect(v.background).toBeNull();
    v.handle({ type: 'waiting' });
    expect(v.paper).toBeNull();
    v.handle(live(sheet()));
    expect(v.paper).toBe(PICTURE);
  });

  it('forgets it when the share ends or asks the code again: a new share sends it again', () => {
    for (const leave of [
      (v: ViewerState) => v.handle({ type: 'end' }),
      (v: ViewerState) => v.status('needsCode'),
      (v: ViewerState) => v.status('ended'),
    ]) {
      const v = new ViewerState();
      v.handle(live(sheet()));
      v.handle({ type: 'livePaper', key: 'k1', picture: PICTURE });
      leave(v);
      expect(v.paper).toBeNull();
      v.handle(live(sheet()));
      expect(v.paper).toBeNull();
    }
  });
});
