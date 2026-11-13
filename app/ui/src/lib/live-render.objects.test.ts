/// <reference types="node" />
/**
 * Text frames on the live displays (`lib/live-render.ts`: the control window and the projector of live mode on a
 * resumed sheet, the editor's projection, the share's viewers): drawn with their layer after its strokes, never under a
 * stroke appended or repainted alone; and the editor's projection sends the page again when a frame changes
 * (`lib/projection-feed.ts`).
 */
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { measureText } from './editor/objects';
import { instantiateInk, type Ink } from './ink';
import { FakeContext, FakeSurface } from './live-fake-canvas';
import { LiveRenderer, buildScene, type LiveSource, type Scene } from './live-render';
import type { Background, Layer, Note, Stroke, TextObject } from './note';
import { projectionDiff, sentPageOf } from './projection-feed';

let ink: Ink;
beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
});

const A4 = { width: 29700, height: 21600 };
const WHITE: Background = { paper: 'white', template: 'blank', texture: false };
const TWO_LAYERS: Layer[] = [
  { name: 'Calque 1', visible: true },
  { name: 'Calque 2', visible: true },
];

function stroke(x: number, layer = 0): Stroke {
  return {
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer,
    points: Array.from({ length: 20 }, (_, i) => ({ x: x + 50 * i, y: 3000 + 40 * Math.sin(i), pressure: 0.6, t: 5 * i })),
  };
}

function frame(fields: Partial<TextObject> = {}): TextObject {
  return measureText(
    {
      kind: 'text',
      id: 'f',
      layer: 0,
      t: 0,
      x: 1000,
      y: 2800,
      scale: 1,
      quarterTurns: 0,
      width: 0,
      height: 0,
      text: 'Ordre du jour',
      font: 'andika',
      bold: false,
      size: 600,
      color: { r: 0x1a, g: 0x3a, b: 0x8a, a: 255 },
      ...fields,
    },
    ink.text,
  );
}

function source(extra: Partial<LiveSource> = {}): LiveSource {
  return {
    pageSize: A4,
    background: WHITE,
    layers: TWO_LAYERS,
    strokes: [],
    generation: 1,
    current: null,
    hover: null,
    view: { zoom: 1, x: A4.width / 2, y: A4.height / 2, board: false, laser: false },
    ...extra,
  };
}

function scene(live: LiveSource): Scene {
  return buildScene(live, { mode: 'presentation', viewport: { width: 1280, height: 720 }, pixelRatio: 1, aspect: 16 / 10, backgroundImage: null });
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

describe('text frames on the live displays', () => {
  it('are drawn on the cache with their layer, after its strokes, in their colour', () => {
    const { renderer, target, cache } = setup();
    const live = source({ strokes: [stroke(1000, 1), stroke(5000, 0)], objects: [frame({ layer: 0 }), frame({ id: 'g', layer: 1, color: { r: 200, g: 0, b: 0, a: 255 } })] });
    expect(renderer.paint(target, scene(live)).full).toBe(true);
    const fills = cache().ctx.ops('fill').map((c) => c.fillStyle);
    // Layer 0: its stroke, then its frame; layer 1: its stroke, then its frame.
    expect(fills).toEqual(['rgb(0, 0, 0)', 'rgb(26, 58, 138)', 'rgb(0, 0, 0)', 'rgb(200, 0, 0)']);
  });

  it('leaves out hidden layers and kinds this version does not draw', () => {
    const { renderer, target, cache } = setup();
    const layers = [TWO_LAYERS[0], { ...TWO_LAYERS[1], visible: false }];
    const other = { kind: 'equation', id: 'e', layer: 0, t: 0, x: 0, y: 0, scale: 1, quarterTurns: 0, width: 10, height: 10 };
    renderer.paint(target, scene(source({ layers, objects: [frame({ layer: 1 }), other] })));
    expect(cache().ctx.ops('fill')).toHaveLength(0);
  });

  it('draws the whole page again for a stroke appended under a frame, or a new list of frames', () => {
    const { renderer, target, cache } = setup();
    const strokes = [stroke(9000, 0)];
    const objects = [frame({ layer: 0 })];
    const live = source({ strokes, objects });
    renderer.paint(target, scene(live));
    // A stroke on a layer above every frame is appended.
    cache().ctx.reset();
    strokes.push(stroke(12_000, 1));
    expect(renderer.paint(target, scene(live))).toMatchObject({ full: false, appended: 1 });
    expect(cache().ctx.ops('fill')).toHaveLength(1);
    // On the frame's layer, it goes under the frame: the page is drawn again.
    strokes.push(stroke(15_000, 0));
    expect(renderer.paint(target, scene(live)).full).toBe(true);
    // Unchanged: nothing redrawn.
    expect(renderer.paint(target, scene(live))).toMatchObject({ full: false, appended: 0 });
    // Another list of frames (a snapshot): drawn again.
    expect(renderer.paint(target, scene({ ...live, objects: [frame({ layer: 1, text: 'Autre' })] })).full).toBe(true);
  });
});

describe("the editor's projection", () => {
  it('sends the page again when a frame changes, and nothing when they are the same', () => {
    const page: Note = {
      id: 'n',
      title: '',
      created: 0,
      modified: 0,
      page: A4,
      background: WHITE,
      tags: [],
      layers: TWO_LAYERS,
      strokes: [stroke(1000)],
      objects: [frame()],
      timing: 'recorded',
      origin: { kind: 'ardoise' },
    };
    const view = { zoom: 1, x: 0, y: 0 };
    const sent = sentPageOf(page, null);
    expect(projectionDiff(sent, page, null, view)).toBeNull();
    expect(projectionDiff(sent, { ...page, objects: [frame()] }, null, view)).toBeNull();
    const typed = { ...page, objects: [frame({ text: 'Ordre du jour !' })] };
    expect(projectionDiff(sent, typed, null, view)?.kind).toBe('page');
    const gone: Note = { ...page };
    delete gone.objects;
    expect(projectionDiff(sent, gone, null, view)?.kind).toBe('page');
  });
});
