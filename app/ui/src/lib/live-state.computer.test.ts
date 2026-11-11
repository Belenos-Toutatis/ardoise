// Milestone M14: the live state with two pens. The computer's stroke in progress (`pen: 'computer'`) lives beside
// the tablet's, neither ends the other; `strokeCancel` drops it; a snapshot drops both until the session sends them
// again; `history` keeps undo and redo up to date; `computerLayer` and `lastComputerDone` say what the control window
// needs; the latency lists keep the two pens apart (the control window's figure stays the tablet's).
import { describe, expect, it } from 'vitest';
import type { LiveBatch, LiveEvent, LivePoint, LiveSnapshot, LiveStroke } from './live';
import { DEFAULT_VIEW, LiveState } from './live-state.svelte';
import type { Note, Stroke } from './note';

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const RED = { r: 0xd8, g: 0, b: 0, a: 255 };

function page(id: string, strokes: Stroke[] = []): Note {
  return {
    id,
    title: '',
    created: 1_792_000_000_000,
    modified: 1_792_000_000_000,
    page: { width: 29700, height: 21600 },
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [
      { name: 'Calque 1', visible: true },
      { name: 'Correction', visible: true },
    ],
    strokes,
    timing: 'recorded',
    origin: { kind: 'live' },
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
    computerLayer: 1,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { ...DEFAULT_VIEW, x: 14850, y: 10800 },
    hover: null,
    presentation: null,
    saved: [],
    revision: 7,
    canUndo: false,
    canRedo: false,
    ...extra,
  };
}

let clock = 1_792_000_000_000;
function points(n: number, x0 = 1000): LivePoint[] {
  return Array.from({ length: n }, (_, i) => ({ x: x0 + 10 * i, y: 700, p: 0.6, t: 5 * i, recv: (clock += 5) }));
}

function tabletStroke(pts: LivePoint[]): LiveStroke {
  return { tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: pts };
}

function computerStroke(pts: LivePoint[]): LiveStroke {
  return { tool: 'feltTip', color: RED, width: 120, layer: 1, points: pts };
}

const batch = (...events: LiveEvent[]): LiveBatch => ({ events, sentAt: clock });

function ready(extra: Partial<LiveSnapshot> = {}): LiveState {
  const live = new LiveState();
  live.initial(snapshot(extra));
  return live;
}

describe('the computer stroke in progress', () => {
  it('lives beside the tablet stroke: interleaved events go to their own pen, neither ends the other', () => {
    const live = ready();
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: tabletStroke(points(2)) }));
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: computerStroke(points(3, 5000)), pen: 'computer', id: 4 }));
    expect(live.current?.points).toHaveLength(2);
    expect(live.computerCurrent?.points).toHaveLength(3);
    expect(live.strokeCount).toBe(2);
    live.receive(
      batch(
        { type: 'points', pageIndex: 0, points: points(2), pen: 'computer', id: 4 },
        { type: 'points', pageIndex: 0, points: points(4) },
        { type: 'points', pageIndex: 0, points: points(1), pen: 'computer', id: 4 },
      ),
    );
    expect(live.current?.points).toHaveLength(6);
    expect(live.computerCurrent?.points).toHaveLength(6);
    // The tablet's stroke ends first: the computer's goes on, and the list has the tablet's alone.
    live.receive(batch({ type: 'strokeEnd', pageIndex: 0 }));
    expect(live.strokes).toHaveLength(1);
    expect(live.strokes[0].tool).toBe('ballpoint');
    expect(live.current).toBeNull();
    expect(live.computerCurrent?.points).toHaveLength(6);
    expect(live.strokeCount).toBe(2);
    // A new tablet stroke does not end the computer's either.
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: tabletStroke(points(1)) }));
    expect(live.computerCurrent).not.toBeNull();
    expect(live.strokeCount).toBe(3);
    // The computer's ends: at the tail, on its layer, in its style; its number is done.
    expect(live.lastComputerDone).toBe(0);
    live.receive(batch({ type: 'strokeEnd', pageIndex: 0, pen: 'computer', id: 4 }));
    expect(live.strokes.map((s) => s.tool)).toEqual(['ballpoint', 'feltTip']);
    expect(live.strokes[1]).toMatchObject({ layer: 1, width: 120, color: RED });
    expect(live.computerCurrent).toBeNull();
    expect(live.current?.points).toHaveLength(1);
    expect(live.lastComputerDone).toBe(4);
    expect(live.strokeCount).toBe(3);
  });

  it('a computer strokeStart while one is in progress keeps the first one', () => {
    const live = ready();
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: computerStroke(points(3)), pen: 'computer', id: 1 }));
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: computerStroke(points(2, 9000)), pen: 'computer', id: 2 }));
    expect(live.strokes).toHaveLength(1);
    expect(live.computerCurrent?.points).toHaveLength(2);
  });

  it('strokeCancel drops the computer stroke, not the tablet one, and says its number is done', () => {
    const live = ready();
    live.receive(
      batch(
        { type: 'strokeStart', pageIndex: 0, stroke: tabletStroke(points(2)) },
        { type: 'strokeStart', pageIndex: 0, stroke: computerStroke(points(3)), pen: 'computer', id: 9 },
      ),
    );
    live.receive(batch({ type: 'strokeCancel', pageIndex: 0, pen: 'computer', id: 9 }));
    expect(live.computerCurrent).toBeNull();
    expect(live.current?.points).toHaveLength(2);
    expect(live.strokes).toHaveLength(0);
    expect(live.lastComputerDone).toBe(9);
    expect(live.strokeCount).toBe(1);
  });

  it('a snapshot drops both strokes in progress; the strokeStarts that follow it bring them back', () => {
    const live = ready();
    live.receive(
      batch(
        { type: 'strokeStart', pageIndex: 0, stroke: tabletStroke(points(2)) },
        { type: 'strokeStart', pageIndex: 0, stroke: computerStroke(points(3)), pen: 'computer', id: 2 },
      ),
    );
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ computerLayer: 0 }) }));
    expect(live.current).toBeNull();
    expect(live.computerCurrent).toBeNull();
    expect(live.strokes).toHaveLength(0);
    expect(live.strokeCount).toBe(0);
    expect(live.computerLayer).toBe(0);
    live.receive(
      batch(
        { type: 'snapshot', snapshot: snapshot() },
        { type: 'strokeStart', pageIndex: 0, stroke: tabletStroke(points(4)) },
        { type: 'strokeStart', pageIndex: 0, stroke: computerStroke(points(5)), pen: 'computer', id: 2 },
      ),
    );
    expect(live.current?.points).toHaveLength(4);
    expect(live.computerCurrent?.points).toHaveLength(5);
    expect(live.strokes).toHaveLength(0);
    expect(live.strokeCount).toBe(2);
  });

  it('computer points without a strokeStart (subscribed mid-stroke) start one on the computer layer', () => {
    const live = ready();
    live.receive(batch({ type: 'points', pageIndex: 0, points: points(3), pen: 'computer', id: 3 }));
    expect(live.computerCurrent?.layer).toBe(1);
    expect(live.current).toBeNull();
    live.receive(batch({ type: 'strokeEnd', pageIndex: 0, pen: 'computer', id: 3 }));
    expect(live.strokes).toHaveLength(1);
  });

  it('ignores the strokes of another page but still learns which gesture is done', () => {
    const live = ready();
    live.receive(batch({ type: 'strokeStart', pageIndex: 1, stroke: computerStroke(points(3)), pen: 'computer', id: 5 }));
    expect(live.computerCurrent).toBeNull();
    live.receive(batch({ type: 'strokeEnd', pageIndex: 1, pen: 'computer', id: 5 }));
    expect(live.strokes).toHaveLength(0);
    expect(live.lastComputerDone).toBe(5);
  });
});

describe('the computer layer and the history', () => {
  it('reads computerLayer from the snapshot, else the tablet layer (the viewers)', () => {
    const live = ready({ layer: 0, computerLayer: 1 });
    expect(live.layer).toBe(0);
    expect(live.computerLayer).toBe(1);
    const viewer = new LiveState();
    const shared = snapshot({ layer: 1 });
    delete shared.computerLayer;
    viewer.initial(shared);
    expect(viewer.computerLayer).toBe(1);
  });

  it('history sets canUndo and canRedo for the page shown, and keeps the revision', () => {
    const live = ready();
    live.receive(batch({ type: 'history', pageIndex: 0, canUndo: true, canRedo: false }));
    expect([live.canUndo, live.canRedo, live.revision]).toEqual([true, false, 7]);
    live.receive(batch({ type: 'history', pageIndex: 3, canUndo: false, canRedo: true }));
    expect([live.canUndo, live.canRedo]).toEqual([true, false]);
  });
});

describe('latency', () => {
  it("keeps the tablet's points and the computer's apart", () => {
    const live = ready();
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: tabletStroke(points(2)) }), 100);
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: computerStroke(points(3)), pen: 'computer', id: 1 }), 200);
    live.receive(batch({ type: 'points', pageIndex: 0, points: points(4), pen: 'computer', id: 1 }), 300);
    live.receive(batch({ type: 'points', pageIndex: 0, points: points(1) }), 400);
    const tablet = live.takeUnpainted();
    expect(tablet.map((p) => p.arrival)).toEqual([100, 100, 400]);
    const computer = live.takeUnpaintedComputer();
    expect(computer.map((p) => p.arrival)).toEqual([200, 200, 200, 300, 300, 300, 300]);
    expect(live.takeUnpainted()).toEqual([]);
    expect(live.takeUnpaintedComputer()).toEqual([]);
  });
});
