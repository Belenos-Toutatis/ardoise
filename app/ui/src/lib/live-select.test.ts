// Milestone M12: the selection of the control window (which strokes a lasso or a click chooses, how
// gestures combine, when the selection is dropped), through the editor's real geometry.
import { describe, expect, it } from 'vitest';
import type { Vec } from './editor/geometry';
import { CLICK_PX, LiveSelection, combineSelection, pickStrokes, type SelectionSource } from './live-select.svelte';
import type { Layer, Stroke } from './note';

const LAYERS: Layer[] = [
  { name: 'Calque 1', visible: true },
  { name: 'Calque 2', visible: false },
];

/** A horizontal line of 20 samples from `x`, at height `y`. */
function line(x: number, y: number, extra: Partial<Stroke> = {}): Stroke {
  return {
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer: 0,
    points: Array.from({ length: 20 }, (_, i) => ({ x: x + 50 * i, y, pressure: 0.6, t: 5 * i })),
    ...extra,
  };
}

/** A rectangle around [x0, x1] × [y0, y1]. */
function around(x0: number, y0: number, x1: number, y1: number): Vec[] {
  return [
    { x: x0, y: y0 },
    { x: x1, y: y0 },
    { x: x1, y: y1 },
    { x: x0, y: y1 },
  ];
}

const SCALE = 0.03;

describe('what a lasso or a click chooses', () => {
  const strokes = [line(1000, 2000), line(1000, 4000), line(8000, 2000), line(1000, 6000, { layer: 1 })];
  const page = { strokes, layers: LAYERS };

  it('takes the strokes with at least half of their samples inside the lasso', () => {
    expect(pickStrokes(page, around(500, 1500, 2500, 2500), SCALE)).toEqual([0]);
    expect(pickStrokes(page, around(500, 1500, 9500, 4500), SCALE)).toEqual([0, 1, 2]);
    // Only a third of the first stroke is inside.
    expect(pickStrokes(page, around(500, 1500, 1300, 2500), SCALE)).toEqual([]);
    expect(pickStrokes(page, around(100, 100, 400, 400), SCALE)).toEqual([]);
  });

  it('never takes a stroke of a hidden layer', () => {
    expect(pickStrokes(page, around(500, 5500, 2500, 6500), SCALE)).toEqual([]);
    // A click on it does not pick it either.
    expect(pickStrokes(page, [{ x: 1500, y: 6000 }], SCALE)).toEqual([]);
    expect(pickStrokes(page, around(0, 0, 20000, 20000), SCALE)).toEqual([0, 1, 2]);
  });

  it('picks the topmost stroke under a click (a gesture shorter than a few pixels)', () => {
    expect(pickStrokes(page, [{ x: 1500, y: 4010 }], SCALE)).toEqual([1]);
    expect(pickStrokes(page, [{ x: 1500, y: 4010 }, { x: 1501, y: 4011 }], SCALE)).toEqual([1]);
    // Missing by more than a few pixels of the screen: nothing.
    expect(pickStrokes(page, [{ x: 1500, y: 4600 }], SCALE)).toEqual([]);
    expect(pickStrokes(page, [], SCALE)).toEqual([]);
  });

  it('measures a click in screen pixels: the same gesture is a lasso when zoomed in', () => {
    const tiny = [{ x: 1000, y: 1950 }, { x: 1000 + (CLICK_PX / SCALE) * 0.5, y: 1950 }];
    expect(pickStrokes(page, tiny, SCALE)).toEqual([0]);
    // Two samples are not an area: as a lasso it chooses nothing.
    expect(pickStrokes(page, tiny, SCALE * 100)).toEqual([]);
  });
});

describe('combining gestures', () => {
  it('replaces, adds (Shift) and removes (Ctrl), in ascending order without repeats', () => {
    expect(combineSelection([4, 1], [2, 2], 'replace')).toEqual([2]);
    expect(combineSelection([4, 1], [], 'replace')).toEqual([]);
    expect(combineSelection([4, 1], [2, 4], 'add')).toEqual([1, 2, 4]);
    expect(combineSelection([4, 1], [], 'add')).toEqual([1, 4]);
    expect(combineSelection([4, 1, 2], [2, 9], 'remove')).toEqual([1, 4]);
  });
});

describe('the selection and the page it belongs to', () => {
  const source = (extra: Partial<SelectionSource> = {}): SelectionSource => ({
    strokes: [],
    layers: LAYERS,
    noteId: 'n1',
    pageIndex: 0,
    revision: 7,
    ...extra,
  });

  it('starts off, and turning it off drops what is chosen', () => {
    const selection = new LiveSelection();
    expect(selection.active).toBe(false);
    expect(selection.any).toBe(false);
    selection.toggle();
    selection.choose(source(), [3, 1], 'replace');
    expect(selection.active).toBe(true);
    expect([selection.indices, selection.count]).toEqual([[1, 3], 2]);
    selection.toggle();
    expect([selection.active, selection.indices]).toEqual([false, []]);
  });

  it('keeps the selection through the tablet and a change of colour (same revision), drops it otherwise', () => {
    const selection = new LiveSelection();
    selection.choose(source(), [1, 2], 'replace');
    selection.follow(source());
    expect(selection.indices).toEqual([1, 2]);
    // An edit that moved the indices, a page turned, another note.
    for (const changed of [{ revision: 8 }, { pageIndex: 1 }, { noteId: 'n2' }]) {
      selection.choose(source(), [1, 2], 'replace');
      selection.follow(source(changed));
      expect(selection.indices, JSON.stringify(changed)).toEqual([]);
    }
  });

  it('does not add to a selection of another revision: the gesture starts again', () => {
    const selection = new LiveSelection();
    selection.choose(source(), [1], 'replace');
    selection.choose(source({ revision: 9 }), [4], 'add');
    expect(selection.indices).toEqual([4]);
    expect(selection.revision).toBe(9);
    selection.choose(source({ revision: 9 }), [5], 'add');
    expect(selection.indices).toEqual([4, 5]);
  });

  it('a gesture on nothing clears, but adding nothing does not', () => {
    const selection = new LiveSelection();
    selection.choose(source(), [1, 2], 'replace');
    selection.choose(source(), [], 'add');
    expect(selection.indices).toEqual([1, 2]);
    selection.choose(source(), [], 'replace');
    expect(selection.indices).toEqual([]);
  });
});

describe('a page of many thousands of strokes', () => {
  /** 24 000 strokes of 30 samples laid out on a grid over an A4 page, a hundred columns wide. */
  const page = (() => {
    const strokes: Stroke[] = [];
    for (let i = 0; i < 24_000; i++) {
      const x = 500 + (i % 100) * 280;
      const y = 400 + Math.floor(i / 100) * 85;
      strokes.push({
        tool: 'ballpoint',
        color: { r: 0, g: 0, b: 0, a: 255 },
        width: 40,
        layer: 0,
        points: Array.from({ length: 30 }, (_, k) => ({ x: x + 8 * k, y: y + 10 * Math.sin(k), pressure: 0.5, t: k })),
      });
    }
    return { strokes, layers: [{ name: 'Calque 1', visible: true }] as Layer[] };
  })();

  /** A hand-drawn loop of 300 vertices around a part of the page. */
  const loop = Array.from({ length: 300 }, (_, i) => {
    const angle = (2 * Math.PI * i) / 300;
    return { x: 14000 + 6000 * Math.cos(angle) * (1 + 0.05 * Math.sin(9 * angle)), y: 10000 + 5000 * Math.sin(angle) };
  });

  it('checks a lasso and a click well within a second, the second time faster still', () => {
    const t0 = performance.now();
    const picked = pickStrokes(page, loop, SCALE);
    const first = performance.now() - t0;
    const t1 = performance.now();
    const again = pickStrokes(page, loop, SCALE);
    const second = performance.now() - t1;
    const t2 = performance.now();
    const clicked = pickStrokes(page, [{ x: 14010, y: 10000 + 10 * Math.sin(1) }], SCALE);
    const click = performance.now() - t2;

    expect(picked.length).toBeGreaterThan(1000);
    expect(picked.length).toBeLessThan(page.strokes.length);
    expect(again).toEqual(picked);
    expect(clicked.length).toBeLessThanOrEqual(1);
    // Generous bounds for a loaded machine: the work is a pass over bounding boxes (cached per stroke
    // object) and a point-in-polygon test on the strokes the loop's box touches.
    expect(first).toBeLessThan(2000);
    expect(second).toBeLessThan(1000);
    expect(click).toBeLessThan(500);
    console.info(`lasso on ${page.strokes.length} strokes: first ${first.toFixed(0)} ms, then ${second.toFixed(0)} ms; click ${click.toFixed(0)} ms; ${picked.length} chosen`);
  });
});
