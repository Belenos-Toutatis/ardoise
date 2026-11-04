/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { instantiateInk, type Ink } from './ink';
import type { LiveBatch, LiveEvent, LivePoint, LiveSnapshot, LiveStrokesEdited } from './live';
import { insertStrokes, recolourStrokes, removeStrokes } from './live-edits';
import { FakeContext, FakeSurface } from './live-fake-canvas';
import { LiveRenderer, buildScene } from './live-render';
import { DEFAULT_VIEW, LiveState } from './live-state.svelte';
import type { Layer, Note, Rgba, Stroke } from './note';
import { ViewerState } from './viewer-state.svelte';

const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 255 };
const RED: Rgba = { r: 216, g: 0, b: 0, a: 255 };
const BLUE: Rgba = { r: 0, g: 0, b: 216, a: 255 };

function stroke(x: number, extra: Partial<Stroke> = {}): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer: 0,
    points: Array.from({ length: 12 }, (_, i) => ({ x: x + 50 * i, y: 3000 + 40 * Math.sin(i), pressure: 0.6, t: 5 * i })),
    ...extra,
  };
}

function page(strokes: Stroke[], extra: Partial<Note> = {}): Note {
  return {
    id: 'p1',
    title: '',
    created: 0,
    modified: 0,
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

function snapshot(strokes: Stroke[], extra: Partial<LiveSnapshot> = {}, note: Partial<Note> = {}): LiveSnapshot {
  return {
    state: 'live',
    page: page(strokes, note),
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { ...DEFAULT_VIEW, x: 14850, y: 10800 },
    hover: null,
    presentation: null,
    saved: [],
    revision: 100,
    canUndo: false,
    canRedo: false,
    ...extra,
  };
}

function ready(strokes: Stroke[], extra: Partial<LiveSnapshot> = {}, note: Partial<Note> = {}): LiveState {
  const live = new LiveState();
  live.initial(snapshot(strokes, extra, note));
  return live;
}

function edited(parts: Partial<LiveStrokesEdited> = {}): LiveStrokesEdited {
  return { type: 'strokesEdited', pageIndex: 0, revision: 101, removed: [], inserted: [], recoloured: [], canUndo: true, canRedo: false, ...parts };
}

const batch = (...events: LiveEvent[]): LiveBatch => ({ events, sentAt: 1 });
const starts = (strokes: readonly Stroke[]) => strokes.map((s) => s.points[0].x);
const point = (x: number): LivePoint => ({ x, y: 700, p: 0.6, t: 5, recv: 1 });

describe('applying an edit to a list of strokes', () => {
  const list = (...xs: number[]) => xs.map((x) => stroke(x));

  it('takes out the strokes named, wherever they are', () => {
    const strokes = list(0, 1, 2, 3, 4, 5);
    const kept = strokes[3];
    removeStrokes(strokes, [0, 2, 5]);
    expect(starts(strokes)).toEqual([1, 3, 4]);
    expect(strokes[1]).toBe(kept);
    removeStrokes(strokes, []);
    expect(strokes).toHaveLength(3);
    removeStrokes(strokes, [2]);
    expect(starts(strokes)).toEqual([1, 3]);
    removeStrokes(strokes, [0, 1]);
    expect(strokes).toEqual([]);
  });

  it('puts strokes back at their places, whatever came at the tail', () => {
    const strokes = list(1, 3, 4, 6, 7);
    const kept = strokes[0];
    insertStrokes(strokes, [
      { index: 0, stroke: stroke(0) },
      { index: 2, stroke: stroke(2) },
      { index: 5, stroke: stroke(5) },
    ]);
    expect(starts(strokes)).toEqual([0, 1, 2, 3, 4, 5, 6, 7]);
    expect(strokes[1]).toBe(kept);
    insertStrokes(strokes, []);
    expect(strokes).toHaveLength(8);
    // At the very end, and in an empty list.
    insertStrokes(strokes, [{ index: 8, stroke: stroke(8) }]);
    expect(starts(strokes).at(-1)).toBe(8);
    const empty: Stroke[] = [];
    insertStrokes(empty, [
      { index: 0, stroke: stroke(0) },
      { index: 1, stroke: stroke(1) },
    ]);
    expect(starts(empty)).toEqual([0, 1]);
  });

  it('gives colours on the same objects and shares no change between strokes', () => {
    const strokes = list(0, 1, 2);
    const objects = [...strokes];
    recolourStrokes(strokes, [
      { color: RED, indices: [0, 2] },
      { color: BLUE, indices: [1] },
    ]);
    expect(strokes.map((s) => s.color)).toEqual([RED, BLUE, RED]);
    strokes.forEach((s, i) => expect(s).toBe(objects[i]));
    // An index that is not there changes nothing (and throws nothing).
    recolourStrokes(strokes, [{ color: BLACK, indices: [9] }]);
    expect(BLACK).toEqual({ r: 0, g: 0, b: 0, a: 255 });
    expect(strokes).toHaveLength(3);
  });
});

describe('live state: strokes edited', () => {
  it('takes them out, keeps the objects of the others and asks for a redraw', () => {
    const live = ready([stroke(1), stroke(2), stroke(3), stroke(4)]);
    const objects = [...live.strokes];
    const generation = live.generation;
    const changes = vi.fn();
    live.subscribe(changes);

    live.receive(batch(edited({ removed: [0, 2] })));
    expect(starts(live.strokes)).toEqual([2, 4]);
    expect(live.strokes[0]).toBe(objects[1]);
    // What the canvases repaint: the strokes that left.
    expect(live.edits.at(-1)).toEqual({ generation: live.generation, touched: [objects[0], objects[2]] });
    expect(live.strokes[1]).toBe(objects[3]);
    expect(live.generation).toBe(generation + 1);
    expect(live.strokeCount).toBe(2);
    expect(live.revision).toBe(101);
    expect(live.canUndo).toBe(true);
    expect(changes).toHaveBeenCalledTimes(1);
  });

  it('gives strokes back at their places, under what the tablet wrote since', () => {
    const live = ready([stroke(1), stroke(3)]);
    const [first, second] = live.strokes;
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: { tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [point(9)] } }));
    live.receive(batch({ type: 'strokeEnd', pageIndex: 0 }));
    const written = live.strokes[2];
    live.receive(
      batch(
        edited({
          inserted: [
            { index: 0, stroke: stroke(0) },
            { index: 2, stroke: stroke(2) },
          ],
          canUndo: false,
          canRedo: true,
        }),
      ),
    );
    expect(starts(live.strokes)).toEqual([0, 1, 2, 3, 9]);
    expect(live.strokes[1]).toBe(first);
    expect(live.strokes[3]).toBe(second);
    expect(live.strokes[4]).toBe(written);
    expect(live.strokeCount).toBe(5);
    expect(live.canUndo).toBe(false);
    expect(live.canRedo).toBe(true);
  });

  it('recolours on the same object, the revision as the event says', () => {
    const live = ready([stroke(1), stroke(2)]);
    const objects = [...live.strokes];
    const generation = live.generation;
    live.receive(batch(edited({ revision: 100, recoloured: [{ color: RED, indices: [1] }] })));
    expect(live.strokes[1]).toBe(objects[1]);
    expect(live.strokes[1].color).toEqual(RED);
    expect(live.strokes[0].color).toEqual(BLACK);
    expect(live.generation).toBe(generation + 1);
    expect(live.revision).toBe(100);
  });

  it('leaves the stroke in progress alone: it joins at the tail when it ends', () => {
    const live = ready([stroke(1), stroke(2)]);
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: { tool: 'feltTip', color: RED, width: 120, layer: 0, points: [point(5)] } }));
    const current = live.current;
    live.receive(batch(edited({ removed: [0] }), { type: 'points', pageIndex: 0, points: [point(6)] }));
    expect(live.current).toBe(current);
    expect(live.current?.points).toHaveLength(2);
    expect(live.strokeCount).toBe(2);
    live.receive(batch(edited({ inserted: [{ index: 0, stroke: stroke(1) }] })));
    expect(live.strokeCount).toBe(3);
    live.receive(batch({ type: 'strokeEnd', pageIndex: 0 }));
    expect(starts(live.strokes)).toEqual([1, 2, 5]);
    expect(live.strokes[2].points).toHaveLength(2);
    expect(live.strokeCount).toBe(3);
  });

  it('ignores an edit of another page', () => {
    const live = ready([stroke(1)]);
    const generation = live.generation;
    live.receive(batch(edited({ pageIndex: 3, removed: [0], revision: 999 })));
    expect(live.strokes).toHaveLength(1);
    expect(live.generation).toBe(generation);
    expect(live.revision).toBe(100);
  });

  it('applies events in the order they came, early batches too', () => {
    const live = new LiveState();
    // Tauri may deliver a batch before the answer that carries the snapshot.
    live.receive(batch(snapshotEvent([stroke(1), stroke(2)]), edited({ removed: [0], revision: 101 })));
    live.receive(batch(edited({ inserted: [{ index: 1, stroke: stroke(3) }], revision: 102 })));
    expect(live.ready).toBe(false);
    live.initial(snapshot([stroke(1), stroke(2)]));
    expect(starts(live.strokes)).toEqual([2, 3]);
    expect(live.revision).toBe(102);
  });

  it('takes the revision and the history from each snapshot, which keeps equal strokes', () => {
    const live = ready([stroke(1), stroke(2)], { revision: 7, canUndo: true, canRedo: true });
    expect(live.revision).toBe(7);
    expect(live.canUndo).toBe(true);
    expect(live.canRedo).toBe(true);
    live.receive(batch(edited({ recoloured: [{ color: RED, indices: [0] }], revision: 7 })));
    const objects = [...live.strokes];
    const generation = live.generation;
    // A style changes, say: the page comes again as Rust has it, the same strokes.
    const same = [stroke(1, { color: RED }), stroke(2)];
    live.receive(batch(snapshotEvent(same, { revision: 7, canUndo: false, canRedo: false })));
    expect(live.strokes[0]).toBe(objects[0]);
    expect(live.generation).toBe(generation);
    expect(live.canUndo).toBe(false);
    expect(live.canRedo).toBe(false);
    // A snapshot of another revision with other strokes replaces them.
    live.receive(batch(snapshotEvent([], { revision: 8 })));
    expect(live.strokes).toEqual([]);
    expect(live.revision).toBe(8);
    expect(live.generation).toBe(generation + 1);
  });

  it('runs without revision nor history on a viewer of the share', () => {
    const viewer = new LiveState();
    const without: LiveSnapshot = snapshot([stroke(1), stroke(2)]);
    delete without.revision;
    delete without.canUndo;
    delete without.canRedo;
    viewer.initial(without);
    expect(viewer.revision).toBe(0);
    const generation = viewer.generation;
    viewer.receive(
      batch({ type: 'strokesEdited', pageIndex: 0, removed: [1], inserted: [], recoloured: [{ color: RED, indices: [0] }] }),
    );
    expect(starts(viewer.strokes)).toEqual([1]);
    expect(viewer.strokes[0].color).toEqual(RED);
    expect(viewer.generation).toBe(generation + 1);
    expect(viewer.revision).toBe(0);
    expect(viewer.canUndo).toBe(false);
  });

  it('costs a pass over the list on a page of many strokes, without touching an outline', () => {
    const count = 50_000;
    const strokes = Array.from({ length: count }, (_, i) => ({ ...stroke(i), points: [] as Stroke['points'] }));
    const live = ready(strokes);
    const objects = [...live.strokes];
    const chosen = Array.from({ length: 1000 }, (_, i) => i * 50);
    let begin = performance.now();
    live.receive(batch(edited({ removed: chosen })));
    const removal = performance.now() - begin;
    const back = chosen.map((index) => ({ index, stroke: objects[index] }));
    begin = performance.now();
    live.receive(batch(edited({ inserted: back })));
    const insertion = performance.now() - begin;
    const all = Array.from({ length: count }, (_, i) => i);
    begin = performance.now();
    live.receive(batch(edited({ recoloured: [{ color: RED, indices: all }] })));
    const recolouring = performance.now() - begin;
    console.info(
      `applying an edit on ${count} strokes: delete 1000 ${removal.toFixed(2)} ms, ` +
        `give them back ${insertion.toFixed(2)} ms, recolour all ${recolouring.toFixed(2)} ms`,
    );
    live.strokes.forEach((s, i) => {
      if (s !== objects[i]) throw new Error(`stroke ${i} changed object`);
    });
    // Tens of microseconds in the app; the bound only catches something out of proportion.
    expect(removal + insertion + recolouring).toBeLessThan(250);
  });
});

describe('a viewer of the share', () => {
  it('follows the edits through the increments of its stream', () => {
    const viewer = new ViewerState();
    // What `feed.rs` sends: no revision and no history; a hidden stroke is not in its list.
    const first = snapshot([stroke(1), stroke(2), stroke(3)]);
    delete first.revision;
    delete first.canUndo;
    delete first.canRedo;
    viewer.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: first }) });
    const objects = [...viewer.live.strokes];
    const generation = viewer.live.generation;
    viewer.handle({
      type: 'live',
      reset: false,
      batch: batch({ type: 'strokesEdited', pageIndex: 0, removed: [1], inserted: [], recoloured: [{ color: RED, indices: [1] }] }),
    });
    expect(starts(viewer.live.strokes)).toEqual([1, 3]);
    expect(viewer.live.strokes[0]).toBe(objects[0]);
    expect(viewer.live.strokes[1]).toBe(objects[2]);
    expect(viewer.live.strokes[1].color).toEqual(RED);
    expect(viewer.live.generation).toBe(generation + 1);
    viewer.close();
  });
});

function snapshotEvent(strokes: Stroke[], extra: Partial<LiveSnapshot> = {}): LiveEvent {
  return { type: 'snapshot', snapshot: snapshot(strokes, extra) };
}

describe('the canvas after an edit', () => {
  let ink: Ink;
  beforeAll(async () => {
    ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
  });

  function setup() {
    const surfaces: FakeSurface[] = [];
    const contours = vi.fn((s: Parameters<Ink['contours']>[0]) => ink.contours(s));
    const counting: Ink = { ...ink, contours };
    const renderer = new LiveRenderer(() => {
      const surface = new FakeSurface();
      surfaces.push(surface);
      return surface;
    }, counting);
    const target = new FakeContext();
    const paint = (live: LiveState, background = live.background) => {
      const report = renderer.paint(
        target,
        buildScene(
          { ...sourceOf(live), background },
          { mode: 'presentation', viewport: { width: 1280, height: 720 }, pixelRatio: 1, aspect: 16 / 10, backgroundImage: null },
        ),
      );
      return report;
    };
    return { paint, contours, cache: () => surfaces[0].ctx };
  }

  function sourceOf(live: LiveState) {
    return {
      pageSize: live.pageSize,
      background: live.background,
      layers: live.layers,
      strokes: live.strokes,
      generation: live.generation,
      current: live.current,
      hover: live.hover,
      view: live.view,
    };
  }

  const fills = (ctx: FakeContext) => ctx.ops('fill').map((c) => c.fillStyle);

  it('draws the cache again after a delete, reusing the outlines of what stays', () => {
    const { paint, contours, cache } = setup();
    const live = ready([stroke(1000), stroke(5000), stroke(9000)]);
    paint(live);
    expect(contours).toHaveBeenCalledTimes(3);
    cache().reset();
    contours.mockClear();
    live.receive(batch(edited({ removed: [1] })));
    const report = paint(live);
    expect(report.full).toBe(true);
    expect(cache().ops('fill')).toHaveLength(2);
    expect(contours).not.toHaveBeenCalled();
    // A stroke added then: only that one, as usual, and not on top of what was there twice.
    cache().reset();
    live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: { tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [point(100), point(200)] } }));
    live.receive(batch({ type: 'strokeEnd', pageIndex: 0 }));
    const added = paint(live);
    expect(added).toMatchObject({ full: false, appended: 1 });
    expect(cache().ops('fill')).toHaveLength(1);
  });

  it('computes an outline only for the strokes that come back', () => {
    const { paint, contours, cache } = setup();
    const live = ready([stroke(1000), stroke(9000)]);
    paint(live);
    contours.mockClear();
    cache().reset();
    live.receive(batch(edited({ inserted: [{ index: 1, stroke: stroke(5000) }] })));
    expect(paint(live).full).toBe(true);
    expect(cache().ops('fill')).toHaveLength(3);
    expect(contours).toHaveBeenCalledTimes(1);
  });

  it('shows a new colour at once, the look of the tool kept: highlighter, felt tip, chalk', () => {
    const { paint, contours, cache } = setup();
    const marker = stroke(1000, { tool: 'highlighter', width: 500, color: { r: 255, g: 230, b: 0, a: 128 } });
    const felt = stroke(5000, { tool: 'feltTip', width: 120 });
    const chalk = stroke(9000, { tool: 'chalk', width: 150 });
    const live = ready([marker, felt, chalk]);
    paint(live);
    contours.mockClear();
    cache().reset();

    const pink: Rgba = { r: 255, g: 0, b: 128, a: 128 };
    live.receive(batch(edited({ recoloured: [{ color: pink, indices: [0] }, { color: RED, indices: [1, 2] }] })));
    expect(paint(live).full).toBe(true);
    expect(contours).not.toHaveBeenCalled();
    const [one, two, three] = cache().ops('fill');
    expect(fills(cache())).toEqual(['rgb(255, 0, 128)', 'rgb(216, 0, 0)', 'rgb(216, 0, 0)']);
    expect(one.composite).toBe('multiply');
    expect(one.alpha).toBeCloseTo(128 / 255, 6);
    expect(two.composite).toBe('source-over');
    expect(two.alpha).toBeCloseTo(0.9, 6);
    expect(three.alpha).toBeCloseTo(0.95, 6);

    // On the blackboard, the highlighter stays screened on the dark paper.
    cache().reset();
    paint(live, { paper: 'slate', template: 'blank', texture: false });
    expect(cache().ops('fill')[0].composite).toBe('screen');
    // Undone: the colour it had.
    cache().reset();
    live.receive(batch(edited({ recoloured: [{ color: marker.color, indices: [0] }] })));
    paint(live);
    expect(fills(cache())[0]).toBe('rgb(255, 0, 128)');
  });

  it('keeps hidden strokes out of the cache, whatever is edited around them', () => {
    const { paint, contours, cache } = setup();
    const layers: Layer[] = [
      { name: 'Réponses', visible: false },
      { name: 'Calque 2', visible: true },
    ];
    const secret = stroke(1000, { layer: 0, color: BLUE });
    const shown = stroke(5000, { layer: 1 });
    const other = stroke(9000, { layer: 1 });
    const live = ready([secret, shown, stroke(3000, { layer: 0, color: BLUE }), other], {}, { layers });
    expect(live.strokes).toHaveLength(4);
    paint(live);
    expect(cache().ops('fill')).toHaveLength(2);
    cache().reset();
    contours.mockClear();

    live.receive(batch(edited({ removed: [1], recoloured: [{ color: RED, indices: [2] }] })));
    expect(starts(live.strokes)).toEqual([1000, 3000, 9000]);
    expect(paint(live).full).toBe(true);
    expect(fills(cache())).toEqual(['rgb(216, 0, 0)']);
    expect(contours).not.toHaveBeenCalled();
    cache().reset();
    live.receive(batch(edited({ inserted: [{ index: 1, stroke: shown }] })));
    expect(paint(live).full).toBe(true);
    expect(cache().ops('fill')).toHaveLength(2);
    expect(fills(cache())).not.toContain('rgb(0, 0, 216)');
  });
});
