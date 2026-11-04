/// <reference types="node" />
// Milestone M12: the repaint of an area of the cache after an edit against a full redraw of the same state, in real
// pixels (`live-raster-canvas.ts`): the state of `LiveState`, fed real `strokesEdited` events, and the real outlines of
// the ink module. The journeys compare the same on real canvases (`app/e2e/journeys-m12-touchup.mjs`).
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink, type Tool } from './ink';
import type { LiveEvent, LiveSnapshot, LiveStrokesEdited, PlacedStroke, RecolouredStrokes } from './live';
import { FakeContext } from './live-fake-canvas';
import { RasterContext, RasterPicture, RasterSurface } from './live-raster-canvas';
import { LiveRenderer, buildScene, type PaintReport } from './live-render';
import { DEFAULT_VIEW, LiveState } from './live-state.svelte';
import type { Background, Layer, Note, Rgba, Stroke } from './note';

let ink: Ink;
beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
});

const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 255 };
const RED: Rgba = { r: 216, g: 0, b: 0, a: 255 };
const GREEN: Rgba = { r: 132, g: 189, b: 0, a: 255 };
const YELLOW: Rgba = { r: 255, g: 220, b: 0, a: 160 };
const WHITE_PAPER: Background = { paper: 'white', template: 'blank', texture: false };
const ONE_LAYER: Layer[] = [{ name: 'Calque 1', visible: true }];
const VIEWPORT = { width: 360, height: 250 };
const PHOTO = new RasterPicture((u, v) => [u, v, 0.5, 0.6 + 0.4 * u]) as unknown as CanvasImageSource;

/** A diagonal stroke from `(x, y)`, with enough samples to be a real outline. */
function line(x: number, y: number, extra: Partial<Stroke> & { dx?: number; dy?: number } = {}): Stroke {
  const { dx = 2400, dy = 900, ...rest } = extra;
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 200,
    layer: 0,
    points: Array.from({ length: 14 }, (_, i) => ({ x: x + (dx * i) / 13, y: y + (dy * i) / 13 + 90 * Math.sin(i), pressure: 0.6, t: 5 * i })),
    ...rest,
  };
}

function snapshot(strokes: Stroke[], note: Partial<Note> = {}, extra: Partial<LiveSnapshot> = {}): LiveSnapshot {
  return {
    state: 'live',
    page: {
      id: 'p1',
      title: '',
      created: 0,
      modified: 0,
      page: { width: 29700, height: 21600 },
      background: WHITE_PAPER,
      tags: [],
      layers: ONE_LAYER,
      strokes,
      timing: 'recorded',
      origin: { kind: 'live' },
      ...note,
    },
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { ...DEFAULT_VIEW, x: 14850, y: 10800 },
    hover: null,
    presentation: null,
    saved: [],
    revision: 1,
    canUndo: false,
    canRedo: false,
    ...extra,
  };
}

interface RigOptions {
  mode?: 'preview' | 'presentation';
  pixelRatio?: number;
  backgroundImage?: CanvasImageSource | null;
  note?: Partial<Note>;
  view?: Partial<LiveSnapshot['view']>;
}

/** A window: `LiveState`, the renderer with its cache in pixels, and a full redraw of the same state to compare with. */
class Rig {
  readonly live = new LiveState();
  readonly target = new FakeContext();
  private readonly surfaces: RasterSurface[] = [];
  private readonly renderer: LiveRenderer;
  backgroundImage: CanvasImageSource | null;

  constructor(
    strokes: Stroke[],
    private readonly options: RigOptions = {},
  ) {
    this.backgroundImage = options.backgroundImage ?? null;
    this.live.initial(snapshot(strokes, options.note));
    if (options.view) this.live.view = { ...this.live.view, ...options.view };
    this.renderer = new LiveRenderer(() => {
      const surface = new RasterSurface();
      this.surfaces.push(surface);
      return surface;
    }, ink);
  }

  private scene() {
    return buildScene(this.live, {
      mode: this.options.mode ?? 'preview',
      viewport: VIEWPORT,
      pixelRatio: this.options.pixelRatio ?? 1,
      aspect: 16 / 9,
      backgroundImage: this.backgroundImage,
    });
  }

  paint(): PaintReport {
    return this.renderer.paint(this.target, this.scene());
  }

  /** The events reach the window; nothing is painted yet. */
  receive(...events: LiveEvent[]): void {
    this.live.receive({ events, sentAt: 1 });
  }

  edit(parts: { removed?: number[]; inserted?: PlacedStroke[]; recoloured?: RecolouredStrokes[] }): void {
    const event: LiveStrokesEdited = { type: 'strokesEdited', pageIndex: 0, removed: [], inserted: [], recoloured: [], ...parts };
    this.receive(event);
  }

  /** The tablet ends a stroke. */
  write(stroke: Stroke): void {
    const points = stroke.points.map((p) => ({ x: p.x, y: p.y, p: p.pressure, t: p.t, recv: 1 }));
    this.receive({ type: 'strokeStart', pageIndex: 0, stroke: { ...stroke, points } }, { type: 'strokeEnd', pageIndex: 0 });
  }

  cache(): RasterContext {
    return this.surfaces[0].ctx;
  }

  /** The cache as a full redraw of this very state would make it. */
  fullRedraw(): RasterContext {
    const surface = new RasterSurface();
    new LiveRenderer(() => surface, ink).paint(new FakeContext(), this.scene());
    return surface.ctx;
  }
}

/** Where two canvases differ: the number of pixels and the first of them. */
function differences(a: RasterContext, b: RasterContext): { pixels: number; first: string | null } {
  expect([a.width, a.height]).toEqual([b.width, b.height]);
  let pixels = 0;
  let first: string | null = null;
  for (let i = 0; i < a.pixels.length; i += 4) {
    if (a.pixels[i] !== b.pixels[i] || a.pixels[i + 1] !== b.pixels[i + 1] || a.pixels[i + 2] !== b.pixels[i + 2] || a.pixels[i + 3] !== b.pixels[i + 3]) {
      pixels += 1;
      first ??= `pixel (${(i / 4) % a.width}, ${Math.floor(i / 4 / a.width)}): ${[...a.pixels.subarray(i, i + 4)]} against ${[...b.pixels.subarray(i, i + 4)]}`;
    }
  }
  return { pixels, first };
}

/** Paints, and the cache must be what a full redraw makes. */
function expectSamePixels(rig: Rig, what: string): PaintReport {
  const report = rig.paint();
  expect(differences(rig.cache(), rig.fullRedraw()), what).toEqual({ pixels: 0, first: null });
  return report;
}

/** Paints: an edit repainted an area, and the pixels are those of a full redraw. */
function expectPartial(rig: Rig, what: string): PaintReport {
  const report = expectSamePixels(rig, what);
  expect(report.full, `${what}: a full redraw`).toBe(false);
  expect(report.repainted, `${what}: no repaint of an area`).toBeTypeOf('number');
  return report;
}

const pageStrokes = () => {
  const strokes: Stroke[] = [];
  for (let row = 0; row < 6; row++) {
    for (let column = 0; column < 7; column++) strokes.push(line(1500 + column * 3800, 1500 + row * 3200));
  }
  return strokes;
};

describe('the test canvas', () => {
  it('tells apart what an edit changes, so that the comparison can fail', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    const before = rig.fullRedraw();
    rig.edit({ recoloured: [{ color: RED, indices: [3] }] });
    expect(differences(before, rig.fullRedraw()).pixels).toBeGreaterThan(20);
    rig.edit({ removed: [4] });
    expect(differences(before, rig.fullRedraw()).pixels).toBeGreaterThan(40);
  });
});

describe('repainting the area of an edit', () => {
  it('a delete: only the strokes around it are drawn again, the paper and the others stay', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    rig.edit({ removed: [8] });
    const report = expectPartial(rig, 'delete');
    // The neighbours that the area reaches, at most: none of the far strokes.
    expect(report.repainted).toBeLessThan(5);
  });

  it('a recolour keeps every outline: the same objects are filled, none is computed again', () => {
    const counted = { contours: 0 };
    const spy: Ink = {
      ...ink,
      contours: (stroke) => {
        counted.contours += 1;
        return ink.contours(stroke);
      },
    };
    const strokes = pageStrokes();
    const live = new LiveState();
    live.initial(snapshot(strokes));
    const surface = new RasterSurface();
    const renderer = new LiveRenderer(() => surface, spy);
    const scene = () => buildScene(live, { mode: 'preview', viewport: VIEWPORT, pixelRatio: 1, aspect: 16 / 9, backgroundImage: null });
    renderer.paint(new FakeContext(), scene());
    expect(counted.contours).toBe(strokes.length);
    live.receive({ events: [{ type: 'strokesEdited', pageIndex: 0, removed: [], inserted: [], recoloured: [{ color: RED, indices: [10, 11] }] }], sentAt: 1 });
    expect(renderer.paint(new FakeContext(), scene()).repainted).toBeTypeOf('number');
    expect(counted.contours).toBe(strokes.length);
  });

  it('a recolour, an undo and a redo give the pixels of a full redraw', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    rig.edit({ recoloured: [{ color: RED, indices: [9, 10] }] });
    expectPartial(rig, 'recolour');
    const removed = rig.live.strokes.slice(9, 11);
    rig.edit({ removed: [9, 10] });
    expectPartial(rig, 'delete');
    rig.edit({ inserted: removed.map((stroke, k) => ({ index: 9 + k, stroke: { ...stroke, points: [...stroke.points] } })) });
    expectPartial(rig, 'undo of the delete');
    rig.edit({ recoloured: [{ color: BLACK, indices: [9, 10] }] });
    expectPartial(rig, 'undo of the recolour');
    rig.edit({ recoloured: [{ color: RED, indices: [9, 10] }] });
    expectPartial(rig, 'redo of the recolour');
    rig.edit({ removed: [9, 10] });
    expectPartial(rig, 'redo of the delete');
  });

  it('overlapping strokes: what lies above an edited stroke stays above, what lies under shows again', () => {
    const under = line(5000, 5000, { dx: 3000, dy: 0, width: 500, color: GREEN });
    const middle = line(6000, 4000, { dx: 0, dy: 3000, width: 500, color: BLACK });
    const over = line(5000, 6000, { dx: 3000, dy: -2500, width: 400, color: RED });
    const rig = new Rig([...pageStrokes().slice(30), under, middle, over]);
    const at = rig.live.strokes.indexOf(middle);
    rig.paint();
    rig.edit({ recoloured: [{ color: YELLOW, indices: [at] }] });
    expectPartial(rig, 'recolour of the middle stroke');
    rig.edit({ removed: [at] });
    expectPartial(rig, 'delete of the middle stroke');
    rig.edit({ inserted: [{ index: at, stroke: { ...middle, points: [...middle.points] } }] });
    const report = expectPartial(rig, 'undo: it comes back between the two');
    expect(report.repainted).toBeGreaterThanOrEqual(3);
  });

  it('a highlighter under ink: multiplied on light paper, the ink over it, whichever is touched', () => {
    const marker = line(4000, 4000, { dx: 5000, dy: 300, width: 900, tool: 'highlighter', color: YELLOW });
    const ink1 = line(4500, 3900, { dx: 4000, dy: 500 });
    const rig = new Rig([marker, ink1, ...pageStrokes().slice(25)]);
    rig.paint();
    rig.edit({ recoloured: [{ color: RED, indices: [1] }] });
    expectPartial(rig, 'the ink over the highlighter recoloured');
    rig.edit({ recoloured: [{ color: { r: 100, g: 220, b: 255, a: 200 }, indices: [0] }] });
    expectPartial(rig, 'the highlighter recoloured');
    rig.edit({ removed: [1] });
    expectPartial(rig, 'the ink gone');
    rig.edit({ removed: [0] });
    expectPartial(rig, 'the highlighter gone');
  });

  it('a hidden layer stays hidden, and a stroke without a layer stays on top', () => {
    const layers: Layer[] = [
      { name: 'bas', visible: true },
      { name: 'caché', visible: false },
      { name: 'haut', visible: true },
    ];
    const strokes = [
      line(5000, 5000, { layer: 2, width: 500, color: RED }),
      line(5200, 5000, { layer: 1, width: 500, color: GREEN }),
      line(5400, 5000, { layer: 0, width: 500 }),
      line(5600, 5000, { layer: 9, width: 500, color: { r: 0, g: 0, b: 200, a: 255 } }),
      line(16000, 15000, { layer: 0 }),
    ];
    const rig = new Rig(strokes, { note: { layers } });
    rig.paint();
    rig.edit({ recoloured: [{ color: GREEN, indices: [2] }] });
    expectPartial(rig, 'recolour under a stroke of a higher layer');
    rig.edit({ removed: [0] });
    expectPartial(rig, 'delete on the top layer');
    rig.edit({ removed: [3] });
    expectPartial(rig, 'delete of the stroke without a layer');
  });

  it('the blackboard: slate paper, a screened highlighter', () => {
    const marker = line(4000, 4000, { dx: 5000, dy: 300, width: 900, tool: 'highlighter', color: YELLOW });
    const chalk = line(4500, 3900, { dx: 4000, dy: 500, tool: 'chalk', color: { r: 255, g: 255, b: 255, a: 255 } });
    const rig = new Rig([marker, chalk, ...pageStrokes().slice(30)], { view: { board: true } });
    rig.paint();
    rig.edit({ recoloured: [{ color: RED, indices: [1] }] });
    expectPartial(rig, 'chalk recoloured on the board');
    rig.edit({ removed: [0] });
    expectPartial(rig, 'highlighter removed on the board');
    // Coming back from the board changes the paper: everything again.
    rig.live.view = { ...rig.live.view, board: false };
    expect(expectSamePixels(rig, 'the board goes').full).toBe(true);
  });

  it('a background image, partly transparent, on a page whose edge falls between pixels', () => {
    const rig = new Rig([line(300, 300, { dx: 2000, dy: 500, width: 600 }), ...pageStrokes()], { backgroundImage: PHOTO, pixelRatio: 1.25 });
    rig.paint();
    rig.edit({ recoloured: [{ color: RED, indices: [0] }] });
    expectPartial(rig, 'recolour at the corner of the page');
    rig.edit({ removed: [0] });
    expectPartial(rig, 'delete at the corner of the page');
  });

  it('a zoomed presentation: a surround, and strokes off the canvas', () => {
    const view = { zoom: 3, x: 9000, y: 7000 };
    const rig = new Rig(pageStrokes(), { mode: 'presentation', view, pixelRatio: 1.5 });
    rig.paint();
    rig.edit({ recoloured: [{ color: RED, indices: [8] }] });
    expectPartial(rig, 'an edit on screen');
    // Far off the canvas: no pixel changes and nothing is drawn.
    rig.edit({ removed: [41] });
    const off = expectPartial(rig, 'an edit off the canvas');
    expect(off.repainted).toBe(0);
  });

  it('a stroke the tablet finishes while an edit is drawn is drawn once, over the area', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    // In the same frame: it crosses the strokes edited, with a highlighter that would show being drawn twice.
    rig.write(line(5000, 5000, { dx: 3000, dy: 2500, width: 700, tool: 'highlighter', color: YELLOW }));
    rig.edit({ recoloured: [{ color: RED, indices: [8, 9] }] });
    rig.write(line(5500, 4800, { dx: 2500, dy: 2500, width: 500, tool: 'feltTip', color: GREEN }));
    const report = expectPartial(rig, 'edit and two new strokes in a frame');
    expect(report.appended).toBe(2);
    // And the frames that follow add nothing more.
    expect(rig.paint()).toEqual({ full: false, appended: 0, currentPoints: 0 });
    expect(differences(rig.cache(), rig.fullRedraw()).pixels).toBe(0);
    rig.write(line(5600, 4800));
    expect(rig.paint().appended).toBe(1);
    expect(differences(rig.cache(), rig.fullRedraw()).pixels).toBe(0);
  });

  it('a stroke finished and edited before any frame shows it is drawn once, in its place', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    rig.write(line(5000, 5000, { width: 500 }));
    const last = rig.live.strokes.length - 1;
    rig.edit({ recoloured: [{ color: RED, indices: [last] }] });
    expectPartial(rig, 'written and recoloured in one frame');
    rig.write(line(5000, 5200, { width: 500 }));
    rig.edit({ removed: [rig.live.strokes.length - 1] });
    expectPartial(rig, 'written and deleted in one frame');
  });

  it('several edits between two frames are repainted together', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    rig.edit({ recoloured: [{ color: RED, indices: [8] }] });
    rig.edit({ removed: [8] });
    rig.edit({ recoloured: [{ color: GREEN, indices: [8] }] });
    expectPartial(rig, 'three edits in a frame');
  });
});

describe('falling back to a full redraw', () => {
  const full = (report: PaintReport) => {
    expect(report.full).toBe(true);
    expect(report.repainted).toBeUndefined();
  };

  it('when the strokes of the edit cover most of the canvas', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    rig.edit({ recoloured: [{ color: RED, indices: [0, 41] }] });
    full(expectSamePixels(rig, 'two far corners'));
  });

  it('when most strokes cross the area', () => {
    const strokes = Array.from({ length: 40 }, (_, k) => line(9000 + k * 10, 8000));
    const rig = new Rig(strokes);
    rig.paint();
    rig.edit({ removed: [20] });
    full(expectSamePixels(rig, 'a heap of strokes'));
  });

  it('after a snapshot, even of the same page', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    rig.edit({ removed: [5] });
    rig.receive({ type: 'snapshot', snapshot: snapshot(pageStrokes().slice(0, 20)) });
    full(expectSamePixels(rig, 'a snapshot after an edit'));
    rig.edit({ removed: [5] });
    expectPartial(rig, 'the next edit');
  });

  it('when another page comes', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    rig.receive({ type: 'snapshot', snapshot: snapshot(pageStrokes().slice(10), {}, { pageIndex: 1 }) });
    full(expectSamePixels(rig, 'page change'));
  });

  it('when the view or the background image change', () => {
    const rig = new Rig(pageStrokes(), { mode: 'presentation', view: { zoom: 2, x: 9000, y: 7000 } });
    rig.paint();
    rig.edit({ removed: [5] });
    rig.live.view = { ...rig.live.view, zoom: 2.5 };
    full(expectSamePixels(rig, 'a new view with the edit'));
    rig.edit({ removed: [5] });
    rig.backgroundImage = PHOTO;
    full(expectSamePixels(rig, 'a paper image with the edit'));
  });

  it('when a window has not painted for many edits (hidden)', () => {
    const rig = new Rig(pageStrokes());
    rig.paint();
    // What the first edits touched is no longer told, and is far from what the last ones touched.
    for (let k = 0; k < 8; k++) rig.edit({ recoloured: [{ color: k % 2 ? RED : GREEN, indices: [0] }] });
    for (let k = 0; k < 32; k++) rig.edit({ recoloured: [{ color: k % 2 ? RED : GREEN, indices: [41] }] });
    full(expectSamePixels(rig, '40 edits unseen'));
  });

  it('when a stroke not yet drawn lies under what the edit brings back (another layer)', () => {
    const layers: Layer[] = [
      { name: 'bas', visible: true },
      { name: 'haut', visible: true },
    ];
    const high = line(5000, 5000, { layer: 1, width: 500, color: RED });
    const rig = new Rig([high, ...pageStrokes().slice(30)], { note: { layers } });
    rig.paint();
    rig.edit({ removed: [0] });
    rig.write(line(5100, 5000, { layer: 0, width: 500 }));
    rig.edit({ inserted: [{ index: 0, stroke: { ...high, points: [...high.points] } }] });
    full(expectSamePixels(rig, 'the new stroke goes under the one that came back'));
  });

  it('without the outlines of the ink module', () => {
    const live = new LiveState();
    live.initial(snapshot(pageStrokes()));
    const surface = new RasterSurface();
    const renderer = new LiveRenderer(() => surface, null);
    const scene = () => buildScene(live, { mode: 'preview', viewport: VIEWPORT, pixelRatio: 1, aspect: 16 / 9, backgroundImage: null });
    renderer.paint(new FakeContext(), scene());
    live.receive({ events: [{ type: 'strokesEdited', pageIndex: 0, removed: [1], inserted: [], recoloured: [] }], sentAt: 1 });
    full(renderer.paint(new FakeContext(), scene()));
  });
});

/** A little deterministic random generator (mulberry32). */
function random(seed: number): () => number {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

describe('any sequence of edits', () => {
  const TOOLS: Tool[] = ['ballpoint', 'fountainPen', 'feltTip', 'pencil', 'brush', 'highlighter', 'chalk'];
  const COLORS: Rgba[] = [BLACK, RED, GREEN, YELLOW, { r: 255, g: 255, b: 255, a: 255 }, { r: 30, g: 60, b: 220, a: 120 }];

  it('gives the pixels of a full redraw after every frame', () => {
    let partial = 0;
    let fullRedraws = 0;
    for (let seed = 1; seed <= 24; seed++) {
      const r = random(seed * 7919);
      const pick = <T,>(list: readonly T[]): T => list[Math.floor(r() * list.length)];
      const layers: Layer[] = [
        { name: 'a', visible: true },
        { name: 'b', visible: r() > 0.3 },
        { name: 'c', visible: true },
      ];
      const stroke = (): Stroke =>
        line(1000 + r() * 24000, 1000 + r() * 17000, {
          dx: 500 + r() * 4000,
          dy: (r() - 0.5) * 4000,
          width: 100 + r() * 700,
          tool: pick(TOOLS),
          color: pick(COLORS),
          layer: pick([0, 0, 0, 1, 2, 7]),
        });
      const options: RigOptions = {
        mode: pick(['preview', 'presentation'] as const),
        pixelRatio: pick([1, 1.25, 1.5, 2]),
        backgroundImage: r() > 0.5 ? PHOTO : null,
        note: { layers, background: { paper: pick(['white', 'cream', 'kraft', 'slate'] as const), template: 'blank', texture: false } },
        view: r() > 0.5 ? { zoom: 1 + r() * 2, x: 8000 + r() * 12000, y: 6000 + r() * 8000, board: r() > 0.7 } : {},
      };
      const rig = new Rig(Array.from({ length: 30 + Math.floor(r() * 30) }, stroke), options);
      rig.paint();
      /** Strokes put aside by a delete, for an undo: places as they were. */
      const undo: PlacedStroke[][] = [];
      for (let step = 0; step < 25; step++) {
        const frameEdits = 1 + Math.floor(r() * 2);
        for (let k = 0; k < frameEdits; k++) {
          const list = rig.live.strokes;
          const choice = r();
          if (choice < 0.2) {
            rig.write(stroke());
          } else if (list.length > 0 && choice < 0.55) {
            // A small cluster of neighbours in the list, as a lasso takes them.
            const from = Math.floor(r() * list.length);
            const indices = Array.from({ length: Math.min(1 + Math.floor(r() * 3), list.length - from) }, (_, n) => from + n);
            rig.edit({ recoloured: [{ color: pick(COLORS), indices }] });
          } else if (list.length > 0 && choice < 0.8) {
            const from = Math.floor(r() * list.length);
            const removed = Array.from({ length: Math.min(1 + Math.floor(r() * 3), list.length - from) }, (_, n) => from + n);
            undo.push(removed.map((index) => ({ index, stroke: { ...list[index], points: [...list[index].points] } })));
            rig.edit({ removed });
          } else if (undo.length > 0) {
            rig.edit({ inserted: undo.pop() });
          }
        }
        const report = rig.paint();
        if (report.full) fullRedraws += 1;
        else if (report.repainted !== undefined) partial += 1;
        const diff = differences(rig.cache(), rig.fullRedraw());
        expect(diff, `seed ${seed}, step ${step}, ${JSON.stringify(report)}`).toEqual({ pixels: 0, first: null });
      }
    }
    // Both ways were really tried.
    expect(partial).toBeGreaterThan(100);
    expect(fullRedraws).toBeGreaterThan(10);
  }, 120_000);
});

describe('the selection over the page, in pixels', () => {
  function composed(rig: Rig, strokes: readonly number[]): { page: RasterContext; frame: RasterContext; dark: boolean } {
    const surfaces: RasterSurface[] = [];
    const renderer = new LiveRenderer(() => {
      const surface = new RasterSurface();
      surfaces.push(surface);
      return surface;
    }, ink);
    const scene = (selection: readonly number[]) =>
      buildScene(rig.live, { mode: 'preview', viewport: VIEWPORT, pixelRatio: 2, aspect: 16 / 9, backgroundImage: null, selection });
    const frame = new RasterContext(VIEWPORT.width * 2, VIEWPORT.height * 2);
    renderer.paint(frame, scene(strokes));
    return { page: surfaces[0].ctx, frame, dark: scene(strokes).background.paper === 'slate' };
  }

  /** The middle of a stroke on the canvas, and a point just beyond its edge. */
  function spots(transform: { scale: number; x: number; y: number }, stroke: Stroke) {
    const p = stroke.points[6];
    const k = 2;
    return {
      centre: { x: (p.x * transform.scale + transform.x) * k, y: (p.y * transform.scale + transform.y) * k },
    };
  }

  const nearly = (c: number[], rgb: number[]) => rgb.every((v, i) => Math.abs(c[i] - v) < 0.02);

  it('leaves the ink in its new colour, ringed, on light paper and on dark paper', () => {
    for (const board of [false, true]) {
      const thick = line(8000, 8000, { dx: 8000, dy: 0, width: 300 });
      const rig = new Rig([thick, ...pageStrokes().slice(40)], { view: { board } });
      rig.edit({ recoloured: [{ color: RED, indices: [0] }] });
      const { frame, page, dark } = composed(rig, [0]);
      expect(dark).toBe(board);
      const scene = buildScene(rig.live, { mode: 'preview', viewport: VIEWPORT, pixelRatio: 2, aspect: 16 / 9, backgroundImage: null });
      const { centre } = spots(scene.transform, thick);
      // The ink shows through untouched: the same pixel with and without the selection, and it is red.
      expect([...frame.pixel(centre.x, centre.y)]).toEqual([...page.pixel(centre.x, centre.y)]);
      expect(nearly(frame.pixel(centre.x, centre.y), [216 / 255, 0, 0])).toBe(true);
      // Along the column through the stroke: the ring is there, in the highlight's blue, away from the ink.
      let ring = 0;
      const x = Math.round(centre.x);
      for (let y = Math.round(centre.y) - 30; y < Math.round(centre.y) + 30; y++) {
        const [r, , b] = frame.pixel(x, y);
        if (b > r + 0.15) ring += 1;
      }
      expect(ring, `the ring, board ${board}`).toBeGreaterThanOrEqual(4);
    }
  });

  it('keeps blue ink apart from the blue ring by a band of paper', () => {
    const thick = line(8000, 8000, { dx: 8000, dy: 0, width: 300, color: { r: 47, g: 111, b: 223, a: 255 } });
    const rig = new Rig([thick]);
    const { frame } = composed(rig, [0]);
    const scene = buildScene(rig.live, { mode: 'preview', viewport: VIEWPORT, pixelRatio: 2, aspect: 16 / 9, backgroundImage: null });
    const { centre } = spots(scene.transform, thick);
    const x = Math.round(centre.x);
    const column: number[] = [];
    for (let y = Math.round(centre.y) - 20; y < Math.round(centre.y) + 20; y++) column.push(frame.pixel(x, y)[0]);
    // Going away from the ink (red very low) on either side: the band of paper (near 1), then the ring (less red again).
    const ink = column.findIndex((v) => v < 0.25);
    expect(ink).toBeGreaterThan(0);
    for (const direction of [-1, 1]) {
      let i = ink;
      while (column[i] !== undefined && column[i] < 0.9) i += direction;
      expect(column[i], 'the band of paper').toBeGreaterThan(0.9);
      while (column[i] !== undefined && column[i] > 0.7) i += direction;
      expect(column[i], 'the ring').toBeLessThan(0.7);
    }
  });
});
