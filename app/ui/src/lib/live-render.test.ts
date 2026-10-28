/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from './ink';
import type { LiveViewState } from './live';
import { FakeContext, FakeSurface } from './live-fake-canvas';
import {
  LiveRenderer,
  PREVIEW_MARGIN,
  buildScene,
  drawingOrder,
  effectiveBackground,
  isDarkPaper,
  paperRgb,
  type LiveSource,
  type Scene,
} from './live-render';
import { fitTransform, projectedArea } from './live-view';
import type { Background, Layer, Stroke } from './note';

let ink: Ink;
beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
});

const A4 = { width: 29700, height: 21600 };
const WHITE: Background = { paper: 'white', template: 'blank', texture: false };
const ONE_LAYER: Layer[] = [{ name: 'Calque 1', visible: true }];

function stroke(x: number, extra: Partial<Stroke> = {}): Stroke {
  return {
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer: 0,
    points: Array.from({ length: 20 }, (_, i) => ({ x: x + 50 * i, y: 3000 + 40 * Math.sin(i), pressure: 0.6, t: 5 * i })),
    ...extra,
  };
}

function source(extra: Partial<LiveSource> = {}): LiveSource {
  return {
    pageSize: A4,
    background: WHITE,
    layers: ONE_LAYER,
    strokes: [],
    generation: 1,
    current: null,
    hover: null,
    view: { zoom: 1, x: A4.width / 2, y: A4.height / 2, board: false, laser: false },
    ...extra,
  };
}

function scene(live: LiveSource, mode: 'preview' | 'presentation' = 'presentation', pixelRatio = 1): Scene {
  return buildScene(live, {
    mode,
    viewport: mode === 'preview' ? { width: 800, height: 560 } : { width: 1280, height: 720 },
    pixelRatio,
    aspect: 16 / 10,
    backgroundImage: null,
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

describe('live drawing', () => {
  it('draws the paper and the finished strokes once, then only what is new', () => {
    const { renderer, target, cache } = setup();
    const strokes = [stroke(1000), stroke(5000)];
    const live = source({ strokes });
    const first = renderer.paint(target, scene(live));
    expect(first.full).toBe(true);
    const offscreen = cache().ctx;
    // Paper colour, then one fill per stroke, with the non-zero rule.
    expect(offscreen.ops('fillRect').map((c) => c.fillStyle)).toEqual(['#000000', 'rgb(255, 255, 255)']);
    expect(offscreen.ops('fill')).toHaveLength(2);
    expect(offscreen.ops('fill').every((c) => c.args[0] === 'nonzero')).toBe(true);
    expect(cache().width).toBe(1280);
    // The frame copies the cache.
    expect(target.ops('drawImage')[0].args[0]).toBe(cache().image);

    offscreen.reset();
    strokes.push(stroke(9000));
    const second = renderer.paint(target, scene(live));
    expect(second).toEqual({ full: false, appended: 1, currentPoints: 0 });
    expect(offscreen.ops('fill')).toHaveLength(1);
    expect(offscreen.ops('fillRect')).toHaveLength(0);

    offscreen.reset();
    expect(renderer.paint(target, scene(live)).appended).toBe(0);
    expect(offscreen.calls).toHaveLength(0);
  });

  it('redraws the stroke in progress at every frame, over the cache', () => {
    const { renderer, target, cache } = setup();
    const current = stroke(2000, { tool: 'feltTip', color: { r: 216, g: 0, b: 0, a: 255 } });
    const live = source({ strokes: [stroke(1000)], current });
    renderer.paint(target, scene(live));
    target.reset();
    cache().ctx.reset();
    current.points.push({ x: 3100, y: 3100, pressure: 0.8, t: 100 });
    const report = renderer.paint(target, scene(live));
    expect(report.currentPoints).toBe(21);
    expect(cache().ctx.calls).toHaveLength(0);
    const fills = target.ops('fill');
    expect(fills).toHaveLength(1);
    expect(fills[0].fillStyle).toBe('rgb(216, 0, 0)');
    // The felt tip's core is slightly translucent (look.rs).
    expect(fills[0].alpha).toBeCloseTo(0.9, 6);
    expect(target.pathPoints).toBeGreaterThan(20);
  });

  it('starts again when the strokes are replaced (page change, clear)', () => {
    const { renderer, target } = setup();
    const live = source({ strokes: [stroke(1000), stroke(2000)] });
    renderer.paint(target, scene(live));
    const cleared = { ...live, strokes: [], generation: 2 };
    expect(renderer.paint(target, scene(cleared)).full).toBe(true);
    // Same array, fewer strokes: also from scratch.
    const strokes = [stroke(1), stroke(2)];
    renderer.paint(target, scene({ ...live, strokes, generation: 3 }));
    strokes.pop();
    expect(renderer.paint(target, scene({ ...live, strokes, generation: 3 })).full).toBe(true);
  });

  it('draws a stroke under strokes of a higher layer from scratch', () => {
    const { renderer, target } = setup();
    const layers: Layer[] = [
      { name: 'Calque 1', visible: true },
      { name: 'Calque 2', visible: true },
    ];
    const strokes = [stroke(1000, { layer: 1 })];
    const live = source({ strokes, layers });
    renderer.paint(target, scene(live));
    strokes.push(stroke(2000, { layer: 1 }));
    expect(renderer.paint(target, scene(live)).full).toBe(false);
    strokes.push(stroke(3000, { layer: 0 }));
    expect(renderer.paint(target, scene(live)).full).toBe(true);
  });

  it('orders strokes by layer, hides hidden layers, draws strokes without a layer on top', () => {
    const a = stroke(1, { layer: 1 });
    const b = stroke(2, { layer: 0 });
    const c = stroke(3, { layer: 7 });
    const d = stroke(4, { layer: 2 });
    const layers: Layer[] = [
      { name: '1', visible: true },
      { name: '2', visible: true },
      { name: '3', visible: false },
    ];
    expect(drawingOrder([a, b, c, d], layers)).toEqual([b, a, c]);
  });

  it('follows the view on the projector: a new view redraws the cache at the new scale', () => {
    const { renderer, target, cache } = setup();
    const live = source({ strokes: [stroke(1000)] });
    renderer.paint(target, scene(live));
    const before = cache().ctx.ops('setTransform').at(-1)!.args[0] as number;
    // Zoom 2 on 16:9 shows 19 200 × 10 800 units: (12 000, 8 000) needs no clamping.
    const zoomed: LiveViewState = { ...live.view, zoom: 2, x: 12000, y: 8000 };
    expect(renderer.paint(target, scene({ ...live, view: zoomed })).full).toBe(true);
    const after = cache().ctx.ops('setTransform').at(-1)!.args[0] as number;
    expect(after / before).toBeCloseTo(2, 6);
    // The view's centre is the screen's centre.
    const [scale, , , , x, y] = cache().ctx.transform;
    expect(12000 * scale + x).toBeCloseTo(640, 4);
    expect(8000 * scale + y).toBeCloseTo(360, 4);
  });

  it('draws for high-density screens at device pixels', () => {
    const { renderer, target, cache } = setup();
    const live = source({ strokes: [stroke(1000)] });
    renderer.paint(target, scene(live, 'presentation', 2));
    expect(cache().width).toBe(2560);
    expect(cache().height).toBe(1440);
    const one = scene(live, 'presentation', 1).transform.scale;
    expect(cache().ctx.transform[0]).toBeCloseTo(one * 2, 9);
  });

  it('shows the laser dot only with the laser on, the pen ring on the preview otherwise', () => {
    const { renderer, target } = setup();
    const hover = { x: 10000, y: 8000 };
    renderer.paint(target, scene(source({ hover })));
    expect(target.ops('arc')).toHaveLength(0);
    target.reset();
    const laser = source({ hover, view: { ...source().view, laser: true } });
    renderer.paint(target, scene(laser));
    const fills = target.ops('fill').map((c) => c.fillStyle);
    expect(fills).toContain('#ff1f1f');
    expect(target.ops('arc')).toHaveLength(2);
    // Where the pen is, on screen.
    const s = scene(laser);
    const [x, y] = target.ops('arc')[1].args as number[];
    expect(x).toBeCloseTo(hover.x * s.transform.scale + s.transform.x, 6);
    expect(y).toBeCloseTo(hover.y * s.transform.scale + s.transform.y, 6);
    target.reset();
    renderer.paint(target, scene(source({ hover }), 'preview'));
    expect(target.ops('stroke')).toHaveLength(1);
    expect(target.ops('fill')).toHaveLength(0);
  });

  it('draws the projected area on the preview, the page outside it darkened when zoomed', () => {
    const { renderer, target } = setup();
    renderer.paint(target, scene(source(), 'preview'));
    // Zoom 1 on a 16:10 screen: the area is the whole page (the bands are not on the page).
    const t = fitTransform(A4, { width: 800, height: 560 }, PREVIEW_MARGIN);
    const [x, y, w, h] = target.ops('strokeRect')[0].args as number[];
    expect(x).toBeCloseTo(t.x, 6);
    expect(y).toBeCloseTo(t.y, 6);
    expect(w).toBeCloseTo(A4.width * t.scale, 6);
    expect(h).toBeCloseTo(A4.height * t.scale, 6);
    expect(target.ops('fillRect')).toHaveLength(0);

    target.reset();
    const view: LiveViewState = { zoom: 3, x: 9000, y: 7000, board: false, laser: false };
    renderer.paint(target, scene(source({ view }), 'preview'));
    const area = projectedArea(A4, view, 16 / 10);
    const [ax, , aw] = target.ops('strokeRect')[0].args as number[];
    expect(ax).toBeCloseTo(area.x * t.scale + t.x, 6);
    expect(aw).toBeCloseTo(area.width * t.scale, 6);
    expect(target.ops('fillRect')).toHaveLength(4);
  });

  it('puts slate paper under the blackboard, and screens the highlighter on dark paper', () => {
    expect(effectiveBackground(WHITE, true).paper).toBe('slate');
    expect(effectiveBackground(WHITE, false)).toBe(WHITE);
    expect(isDarkPaper('slate')).toBe(true);
    expect(isDarkPaper('kraft')).toBe(false);
    expect(paperRgb({ custom: { r: 0, g: 0, b: 0, a: 0 } })).toEqual([255, 255, 255]);
    expect(isDarkPaper({ custom: { r: 10, g: 10, b: 60, a: 255 } })).toBe(true);

    const { renderer, target, cache } = setup();
    const marker = stroke(1000, { tool: 'highlighter', width: 500, color: { r: 255, g: 230, b: 0, a: 128 } });
    renderer.paint(target, scene(source({ strokes: [marker] })));
    expect(cache().ctx.ops('fill')[0].composite).toBe('multiply');
    expect(cache().ctx.ops('fill')[0].alpha).toBeCloseTo(128 / 255, 6);
    cache().ctx.reset();
    const board = source({ strokes: [marker], view: { ...source().view, board: true } });
    expect(renderer.paint(target, scene(board)).full).toBe(true);
    expect(cache().ctx.ops('fillRect')[1].fillStyle).toBe('rgb(42, 50, 48)');
    expect(cache().ctx.ops('fill')[0].composite).toBe('screen');
  });

  it('draws the paper image over the paper colour once it is loaded', () => {
    const { renderer, target, cache } = setup();
    const live = source();
    renderer.paint(target, scene(live));
    const image = { fake: 'paper' } as unknown as CanvasImageSource;
    const withImage = { ...scene(live), backgroundImage: image };
    expect(renderer.paint(target, withImage).full).toBe(true);
    const drawn = cache().ctx.ops('drawImage').at(-1)!;
    expect(drawn.args).toEqual([image, 0, 0, A4.width, A4.height]);
  });

  it('draws straight on the canvas when no offscreen surface exists, and nothing without the geometry', () => {
    const target = new FakeContext();
    const direct = new LiveRenderer(() => null, ink);
    const live = source({ strokes: [stroke(1000)] });
    expect(direct.paint(target, scene(live)).full).toBe(true);
    expect(direct.paint(target, scene(live)).full).toBe(true);
    expect(target.ops('fill').length).toBe(2);

    const blind = new LiveRenderer(() => new FakeSurface(), null);
    const empty = new FakeContext();
    expect(blind.paint(empty, scene({ ...live, current: stroke(2000) })).currentPoints).toBe(0);
    expect(empty.ops('fill')).toHaveLength(0);
    blind.setInk(ink);
    expect(blind.paint(empty, scene(live)).full).toBe(true);
  });
});

describe('marks written over the presentation (M8)', () => {
  const mark = (extra: Partial<Stroke> = {}): Stroke => stroke(2000, { tool: 'feltTip', color: { r: 216, g: 0, b: 0, a: 255 }, ...extra });

  it('draws the marks over the page and the stroke in progress, not into the cache', () => {
    const { renderer, target, cache } = setup();
    const live = source({ strokes: [stroke(1000)] });
    const marks = [mark(), mark({ color: { r: 0, g: 0, b: 255, a: 255 } })];
    renderer.paint(target, scene(live));
    target.reset();
    cache().ctx.reset();
    const sceneWithMarks = { ...scene(live), marks, markCurrent: mark() };
    const report = renderer.paint(target, sceneWithMarks);
    expect(report.full).toBe(false);
    expect(cache().ctx.calls).toHaveLength(0);
    // Two finished marks and the one in progress, filled non-zero, in page units.
    const fills = target.ops('fill');
    expect(fills).toHaveLength(3);
    expect(fills.every((c) => c.args[0] === 'nonzero')).toBe(true);
    expect(fills[0].fillStyle).toBe('rgb(216, 0, 0)');
    expect(fills[1].fillStyle).toBe('rgb(0, 0, 255)');
  });

  it('draws nothing more once the marks are gone', () => {
    const { renderer, target } = setup();
    const live = source();
    renderer.paint(target, { ...scene(live), marks: [mark()] });
    target.reset();
    renderer.paint(target, scene(live));
    expect(target.ops('fill')).toHaveLength(0);
  });
});
