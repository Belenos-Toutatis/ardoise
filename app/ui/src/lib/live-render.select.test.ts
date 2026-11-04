/// <reference types="node" />
// Milestone M12: the control window's selection in the live drawing: a highlight apart from the cache
// of the strokes, right at every zoom and orientation, visible on dark paper, absent from the projector.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from './ink';
import { FakeContext, FakeSurface } from './live-fake-canvas';
import { LiveRenderer, SELECTION_COLORS, buildScene, type LiveSource, type Scene } from './live-render';
import { fitTransform } from './live-view';
import type { Background, Layer, Stroke } from './note';

let ink: Ink;
beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
});

const A4 = { width: 29700, height: 21600 };
const WHITE: Background = { paper: 'white', template: 'blank', texture: false };
const SLATE: Background = { paper: 'slate', template: 'blank', texture: false };
const LAYERS: Layer[] = [
  { name: 'Calque 1', visible: true },
  { name: 'Calque 2', visible: false },
];

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

function source(strokes: Stroke[], extra: Partial<LiveSource> = {}): LiveSource {
  return {
    pageSize: A4,
    background: WHITE,
    layers: LAYERS,
    strokes,
    generation: 1,
    current: null,
    hover: null,
    view: { zoom: 1, x: A4.width / 2, y: A4.height / 2, board: false, laser: false },
    ...extra,
  };
}

function preview(live: LiveSource, selection: readonly number[], extra: { lasso?: { x: number; y: number }[] } = {}): Scene {
  return buildScene(live, {
    mode: 'preview',
    viewport: { width: 800, height: 560 },
    pixelRatio: 1,
    aspect: 16 / 9,
    backgroundImage: null,
    selection,
    lasso: extra.lasso ?? null,
  });
}

function setup() {
  const surfaces: FakeSurface[] = [];
  const renderer = new LiveRenderer(() => {
    const s = new FakeSurface();
    surfaces.push(s);
    return s;
  }, ink);
  // The page cache is made first, the selection's surface second.
  return { renderer, target: new FakeContext(), page: () => surfaces[0], layer: () => surfaces[1], surfaces };
}

describe('selection highlight', () => {
  it('is drawn on a surface of its own and copied over the page, without drawing the page again', () => {
    const { renderer, target, page, layer } = setup();
    const strokes = [stroke(1000), stroke(5000), stroke(9000)];
    const live = source(strokes);
    expect(renderer.paint(target, preview(live, []))).toEqual({ full: true, appended: 0, currentPoints: 0 });
    expect(layer()).toBeUndefined();

    page().ctx.reset();
    target.reset();
    const selection = [1];
    const report = renderer.paint(target, preview(live, selection));
    // The page cache was left alone: no fill, no redraw.
    expect(report.full).toBe(false);
    expect(report.selectionDrawn).toBe(true);
    expect(page().ctx.calls).toEqual([]);
    // The highlight: a glow and a line round the chosen stroke only, then the box.
    const selected = layer().ctx;
    expect(selected.ops('stroke').map((c) => c.strokeStyle)).toEqual([SELECTION_COLORS.light.halo, SELECTION_COLORS.light.line]);
    expect(selected.pathPoints).toBeGreaterThan(10);
    expect(selected.ops('fillRect')).toHaveLength(1);
    expect(selected.ops('strokeRect')).toHaveLength(1);
    // Over the page in the frame: first the cache, then the highlight's surface.
    expect(target.ops('drawImage').map((c) => c.args[0])).toEqual([page().image, layer().image]);

    // Another frame with the same selection copies it without drawing it again.
    selected.reset();
    target.reset();
    expect(renderer.paint(target, preview(live, selection)).selectionDrawn).toBeUndefined();
    expect(selected.calls).toEqual([]);
    expect(target.ops('drawImage')).toHaveLength(2);
  });

  it('puts the box where the chosen strokes are, in page units, at the page scale', () => {
    const { renderer, target, layer } = setup();
    const live = source([stroke(1000), stroke(5000)]);
    renderer.paint(target, preview(live, [0, 1]));
    const [x, y, width, height] = layer().ctx.ops('strokeRect')[0].args as number[];
    expect(x).toBeCloseTo(1000 - 30, 5);
    expect(x + width).toBeCloseTo(5000 + 50 * 19 + 30, 5);
    expect(y).toBeLessThan(3000);
    expect(y + height).toBeGreaterThan(3000);
    // The transform maps page units to the screen of the preview.
    const t = fitTransform(A4, { width: 800, height: 560 }, 12);
    expect(layer().ctx.transform).toEqual([t.scale, 0, 0, t.scale, t.x, t.y]);
  });

  it('is drawn again when the zoom, the size or the orientation changes the view, not when the tablet appends', () => {
    const { renderer, target, layer } = setup();
    const strokes = [stroke(1000), stroke(5000)];
    const live = source(strokes);
    const selection = [0];
    renderer.paint(target, preview(live, selection));
    const drawn = layer().ctx.count('stroke');

    // The tablet ends a stroke: it is appended to the page cache, the highlight is untouched.
    strokes.push(stroke(9000));
    const appended = renderer.paint(target, preview(live, selection));
    expect(appended.appended).toBe(1);
    expect(appended.selectionDrawn).toBeUndefined();
    expect(layer().ctx.count('stroke')).toBe(drawn);

    // The page turned a quarter: another page size, so another transform.
    const portrait = source(strokes, { pageSize: { width: A4.height, height: A4.width } });
    const turned = renderer.paint(target, preview(portrait, selection));
    expect(turned.full).toBe(true);
    expect(turned.selectionDrawn).toBe(true);
    const t = fitTransform(portrait.pageSize, { width: 800, height: 560 }, 12);
    expect(layer().ctx.transform).toEqual([t.scale, 0, 0, t.scale, t.x, t.y]);

    // A new selection (a new array) is drawn at once.
    expect(renderer.paint(target, preview(portrait, [0, 1])).selectionDrawn).toBe(true);
  });

  it('is drawn again after an edit (a new generation) and skips strokes that are gone or hidden', () => {
    const { renderer, target, layer } = setup();
    const strokes = [stroke(1000), stroke(5000, { layer: 1 })];
    const live = source(strokes);
    renderer.paint(target, preview(live, [0, 1, 7]));
    // The stroke of the hidden layer and the index that is not a stroke add nothing: one box only.
    const [, , width] = layer().ctx.ops('strokeRect')[0].args as number[];
    expect(width).toBeLessThan(2000);
    expect(layer().ctx.count('fillRect')).toBe(1);

    const edited = source(strokes, { generation: 2 });
    expect(renderer.paint(target, preview(edited, [0, 1, 7])).selectionDrawn).toBe(true);
  });

  it('shows on dark paper with its lighter colours', () => {
    const { renderer, target, layer } = setup();
    renderer.paint(target, preview(source([stroke(1000)], { background: SLATE }), [0]));
    expect(layer().ctx.ops('stroke').map((c) => c.strokeStyle)[0]).toBe(SELECTION_COLORS.dark.halo);
    expect(layer().ctx.ops('fillRect')[0].fillStyle).toBe(SELECTION_COLORS.dark.box);
    // The blackboard of a white sheet: the same.
    const board = setup();
    board.renderer.paint(
      board.target,
      preview(source([stroke(1000)], { view: { zoom: 1, x: 0, y: 0, board: true, laser: false } }), [0]),
    );
    expect(board.layer().ctx.ops('stroke').map((c) => c.strokeStyle)[0]).toBe(SELECTION_COLORS.dark.halo);
  });

  it('is drawn over the paper image too (a resumed sheet): after it, never under it', () => {
    const { renderer, target, page, layer } = setup();
    const image = { fake: 'photo' } as unknown as CanvasImageSource;
    const scene = { ...preview(source([stroke(1000)]), [0]), backgroundImage: image };
    renderer.paint(target, scene);
    expect(page().ctx.ops('drawImage').map((c) => c.args[0])).toEqual([image]);
    const frame = target.ops('drawImage').map((c) => c.args[0]);
    expect(frame.indexOf(layer().image)).toBeGreaterThan(frame.indexOf(page().image));
  });

  it('draws the lasso over the page in the frame, not in the cache', () => {
    const { renderer, target, page } = setup();
    const live = source([stroke(1000)]);
    renderer.paint(target, preview(live, []));
    page().ctx.reset();
    target.reset();
    const lasso = [
      { x: 1000, y: 2000 },
      { x: 4000, y: 2000 },
      { x: 4000, y: 5000 },
    ];
    renderer.paint(target, preview(live, [], { lasso }));
    expect(page().ctx.calls).toEqual([]);
    const strokes = target.ops('stroke').filter((c) => c.strokeStyle === SELECTION_COLORS.light.line);
    expect(strokes).toHaveLength(1);
    expect(target.ops('fill').some((c) => c.fillStyle === SELECTION_COLORS.light.box)).toBe(true);
  });

  it('never reaches the projector: a presentation scene has no selection and no lasso', () => {
    const scene = buildScene(source([stroke(1000)]), {
      mode: 'presentation',
      viewport: { width: 1280, height: 720 },
      pixelRatio: 1,
      aspect: 16 / 9,
      backgroundImage: null,
      selection: [0],
      lasso: [
        { x: 0, y: 0 },
        { x: 100, y: 100 },
      ],
    });
    expect(scene.selection).toEqual([]);
    expect(scene.lasso).toBeNull();
    const { renderer, target, layer } = setup();
    renderer.paint(target, scene);
    expect(layer()).toBeUndefined();
    expect(target.ops('stroke')).toEqual([]);
  });

  it('works without a second surface (a stand-in that makes only one)', () => {
    const only = new FakeSurface();
    let made = 0;
    const renderer = new LiveRenderer(() => (made++ === 0 ? only : null), ink);
    const target = new FakeContext();
    const live = source([stroke(1000)]);
    renderer.paint(target, preview(live, []));
    target.reset();
    renderer.paint(target, preview(live, [0]));
    expect(target.ops('stroke').length).toBe(2);
  });
});
