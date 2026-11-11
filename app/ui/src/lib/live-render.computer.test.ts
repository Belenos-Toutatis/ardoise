/// <reference types="node" />
// Milestone M14: two pens write at once in live mode, the tablet's and the computer's. Both strokes in progress are
// drawn over the cache in their layers' order, each only while its layer shows; the control window draws its own
// computer stroke instead of Rust's copy; the eraser end's ring is the preview's alone; the cache is left as it was.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from './ink';
import { FakeContext, FakeSurface } from './live-fake-canvas';
import { ERASER_RING_PX, LiveRenderer, buildScene, type LiveSource, type Scene, type SceneOptions } from './live-render';
import type { Layer, Rgba, Stroke } from './note';

let ink: Ink;
beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
});

const A4 = { width: 29700, height: 21600 };
const RED: Rgba = { r: 216, g: 0, b: 0, a: 255 };
const BLUE: Rgba = { r: 0, g: 0, b: 255, a: 255 };
const GREEN: Rgba = { r: 0, g: 160, b: 0, a: 255 };
const TWO_LAYERS: Layer[] = [
  { name: 'Calque 1', visible: true },
  { name: 'Correction', visible: true },
];

function stroke(x: number, color: Rgba, layer: number): Stroke {
  return {
    tool: 'ballpoint',
    color,
    width: 60,
    layer,
    points: Array.from({ length: 12 }, (_, i) => ({ x: x + 60 * i, y: 6000, pressure: 0.6, t: 5 * i })),
  };
}

function source(extra: Partial<LiveSource> = {}): LiveSource {
  return {
    pageSize: A4,
    background: { paper: 'white', template: 'blank', texture: false },
    layers: TWO_LAYERS,
    strokes: [],
    generation: 1,
    current: null,
    computerCurrent: null,
    hover: null,
    view: { zoom: 1, x: A4.width / 2, y: A4.height / 2, board: false, laser: false },
    ...extra,
  };
}

function scene(live: LiveSource, mode: 'preview' | 'presentation' = 'presentation', extra: Partial<SceneOptions> = {}): Scene {
  return buildScene(live, {
    mode,
    viewport: mode === 'preview' ? { width: 800, height: 560 } : { width: 1280, height: 720 },
    pixelRatio: 1,
    aspect: 16 / 10,
    backgroundImage: null,
    ...extra,
  });
}

function setup() {
  const surfaces: FakeSurface[] = [];
  const renderer = new LiveRenderer(() => {
    const s = new FakeSurface();
    surfaces.push(s);
    return s;
  }, ink);
  return { renderer, target: new FakeContext(), cache: () => surfaces[0] };
}

/** The colours filled on the frame itself (not on the cache), in order: the strokes in progress. */
function framed(target: FakeContext): string[] {
  return target.ops('fill').map((call) => call.fillStyle ?? '');
}

const css = (c: Rgba) => `rgb(${c.r}, ${c.g}, ${c.b})`;

describe('two strokes in progress', () => {
  it('draws both, the lower layer first, whichever pen it is', () => {
    const { renderer, target } = setup();
    // The tablet on « Correction » (layer 1), the computer on « Calque 1 » (layer 0).
    const report = renderer.paint(target, scene(source({ current: stroke(2000, RED, 1), computerCurrent: stroke(9000, BLUE, 0) })));
    expect(framed(target)).toEqual([css(BLUE), css(RED)]);
    expect(report.currentPoints).toBe(12);
    expect(report.computerPoints).toBe(12);
  });

  it("on one layer, draws the tablet's first", () => {
    const { renderer, target } = setup();
    renderer.paint(target, scene(source({ current: stroke(2000, RED, 1), computerCurrent: stroke(9000, BLUE, 1) })));
    expect(framed(target)).toEqual([css(RED), css(BLUE)]);
    target.reset();
    renderer.paint(target, scene(source({ current: stroke(2000, RED, 0), computerCurrent: stroke(9000, BLUE, 1) })));
    expect(framed(target)).toEqual([css(RED), css(BLUE)]);
  });

  it('does not draw a stroke in progress on a hidden layer, the other one still', () => {
    const { renderer, target } = setup();
    const layers: Layer[] = [TWO_LAYERS[0], { ...TWO_LAYERS[1], visible: false }];
    const report = renderer.paint(target, scene(source({ layers, current: stroke(2000, RED, 0), computerCurrent: stroke(9000, BLUE, 1) })));
    expect(framed(target)).toEqual([css(RED)]);
    expect(report.computerPoints).toBe(0);
    target.reset();
    // The tablet on a hidden layer writes nothing on the screen either (Q11: the presentation does not show it).
    renderer.paint(target, scene(source({ layers, current: stroke(2000, RED, 1), computerCurrent: stroke(9000, BLUE, 0) })));
    expect(framed(target)).toEqual([css(BLUE)]);
  });

  it("draws the control window's own strokes instead of Rust's copy", () => {
    const { renderer, target } = setup();
    const live = source({ computerCurrent: stroke(9000, BLUE, 1) });
    const report = renderer.paint(target, scene(live, 'preview', { localComputer: [stroke(9000, GREEN, 1), stroke(15000, GREEN, 1)] }));
    expect(framed(target)).toEqual([css(GREEN), css(GREEN)]);
    expect(report.computerPoints).toBe(24);
    target.reset();
    // An empty list (a cancelled gesture waiting for the session) hides Rust's copy.
    renderer.paint(target, scene(live, 'preview', { localComputer: [] }));
    expect(framed(target)).toEqual([]);
    target.reset();
    // Not given: Rust's copy, as in the other windows.
    renderer.paint(target, scene(live, 'preview'));
    expect(framed(target)).toEqual([css(BLUE)]);
  });

  it('leaves the cache alone while they grow, and appends the computer stroke once it is finished', () => {
    const { renderer, target, cache } = setup();
    const strokes = [stroke(2000, RED, 0)];
    const computer = stroke(9000, BLUE, 1);
    const live = source({ strokes, computerCurrent: computer });
    expect(renderer.paint(target, scene(live)).full).toBe(true);
    const fillsOnCache = () => cache().ctx.count('fill');
    const before = fillsOnCache();
    computer.points.push({ x: 9800, y: 6100, pressure: 0.6, t: 80 });
    const growing = renderer.paint(target, scene(live));
    expect(growing).toMatchObject({ full: false, appended: 0, computerPoints: 13 });
    expect(fillsOnCache()).toBe(before);
    // It ends: at the tail of the list, on its layer above the tablet's ink, the cache gets it alone.
    strokes.push(computer);
    const ended = renderer.paint(target, scene(source({ strokes, computerCurrent: null })));
    expect(ended).toEqual({ full: false, appended: 1, currentPoints: 0, computerPoints: 0 });
    expect(fillsOnCache()).toBe(before + 1);
  });

  it("rings the eraser end on the preview only, at the eraser's size", () => {
    const { renderer, target } = setup();
    renderer.paint(target, scene(source(), 'preview', { eraser: { x: 10000, y: 8000 } }));
    expect(target.ops('arc').map((call) => call.args[2])).toEqual([ERASER_RING_PX]);
    const projector = new FakeContext();
    renderer.paint(projector, scene(source(), 'presentation', { eraser: { x: 10000, y: 8000 } }));
    expect(projector.count('arc')).toBe(0);
  });
});
