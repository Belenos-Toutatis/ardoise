/// <reference types="node" />
// SPEC §4.2 « Retoucher pendant le direct »: the selection being moved or resized, in the control window's drawing. The
// chosen strokes are taken off the cache (the area they leave is drawn again without them, the pixels those of a full
// redraw of the page without them), rendered once on a surface of their own and copied, moved and scaled, at each
// frame, with the frame and the four handles over them; a drag that ends puts them back, an edit that comes replaces
// them, in one frame.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from './ink';
import type { LiveEvent, LiveSnapshot, LiveStrokesEdited, PlacedStroke } from './live';
import { FakeContext } from './live-fake-canvas';
import { RasterContext, RasterSurface } from './live-raster-canvas';
import { HANDLE_SIZE_PX } from './selection-grab';
import { LiveRenderer, SELECTION_COLORS, buildScene, type Lift, type PaintReport } from './live-render';
import { DEFAULT_VIEW, LiveState } from './live-state.svelte';
import { fitTransform } from './live-view';
import type { Background, Layer, Note, Rgba, Stroke } from './note';

let ink: Ink;
beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
});

const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 255 };
const WHITE_PAPER: Background = { paper: 'white', template: 'blank', texture: false };
const LAYERS: Layer[] = [
  { name: 'Calque 1', visible: true },
  { name: 'Calque 2', visible: false },
];
const PAGE = { width: 29700, height: 21600 };
const VIEWPORT = { width: 360, height: 250 };
const TRANSFORM = fitTransform(PAGE, VIEWPORT, 12);

function line(x: number, y: number, extra: Partial<Stroke> = {}): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 200,
    layer: 0,
    points: Array.from({ length: 14 }, (_, i) => ({ x: x + (2400 * i) / 13, y: y + (900 * i) / 13 + 90 * Math.sin(i), pressure: 0.6, t: 5 * i })),
    ...extra,
  };
}

const pageStrokes = () => {
  const strokes: Stroke[] = [];
  for (let row = 0; row < 6; row++) for (let column = 0; column < 7; column++) strokes.push(line(1500 + column * 3800, 1500 + row * 3200));
  return strokes;
};

function snapshot(strokes: Stroke[]): LiveSnapshot {
  const page: Note = {
    id: 'p1',
    title: '',
    created: 0,
    modified: 0,
    page: PAGE,
    background: WHITE_PAPER,
    tags: [],
    layers: LAYERS,
    strokes,
    timing: 'recorded',
    origin: { kind: 'live' },
  };
  return {
    state: 'live',
    page,
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
  };
}

/** A control window: the session's state, a renderer with a cache in pixels, the frame's target, and what the selection is. */
class Rig {
  readonly live = new LiveState();
  readonly target = new FakeContext();
  readonly surfaces: RasterSurface[] = [];
  readonly renderer: LiveRenderer;
  selection: readonly number[] = [];
  lift: Lift | null = null;
  handles = true;
  contours = 0;

  constructor(strokes: Stroke[]) {
    this.live.initial(snapshot(strokes));
    const counted: Ink = {
      ...ink,
      contours: (stroke) => {
        this.contours += 1;
        return ink.contours(stroke);
      },
    };
    this.renderer = new LiveRenderer(() => {
      const surface = new RasterSurface();
      this.surfaces.push(surface);
      return surface;
    }, counted);
  }

  scene() {
    return buildScene(this.live, {
      mode: 'preview',
      viewport: VIEWPORT,
      pixelRatio: 1,
      aspect: 16 / 9,
      backgroundImage: null,
      selection: this.selection,
      lift: this.lift,
      handles: this.handles,
    });
  }

  paint(): PaintReport {
    this.target.reset();
    return this.renderer.paint(this.target, this.scene());
  }

  receive(...events: LiveEvent[]): void {
    this.live.receive({ events, sentAt: 1 });
  }

  /** The session's event for a transform: the chosen strokes replaced, at their places, by moved ones. */
  transformed(indices: number[], dx: number, dy: number): void {
    const inserted: PlacedStroke[] = indices.map((index) => {
      const stroke = this.live.strokes[index];
      return { index, stroke: { ...stroke, points: stroke.points.map((p) => ({ ...p, x: p.x + dx, y: p.y + dy })) } };
    });
    const event: LiveStrokesEdited = { type: 'strokesEdited', pageIndex: 0, revision: 2, removed: indices, inserted, recoloured: [] };
    this.receive(event);
  }

  get cache(): RasterContext {
    return this.surfaces[0].ctx;
  }

  /** The cache as a full redraw of this very scene makes it. */
  fullRedraw(): RasterContext {
    const surfaces: RasterSurface[] = [];
    new LiveRenderer(() => {
      const surface = new RasterSurface();
      surfaces.push(surface);
      return surface;
    }, ink).paint(new FakeContext(), this.scene());
    return surfaces[0].ctx;
  }
}

/** The cache of a page that holds `strokes` and nothing else. */
function pageOf(strokes: Stroke[]): RasterContext {
  const rig = new Rig(strokes);
  rig.paint();
  return rig.cache;
}

function differences(a: RasterContext, b: RasterContext): number {
  expect([a.width, a.height]).toEqual([b.width, b.height]);
  let pixels = 0;
  for (let i = 0; i < a.pixels.length; i += 4) {
    if (a.pixels[i] !== b.pixels[i] || a.pixels[i + 1] !== b.pixels[i + 1] || a.pixels[i + 2] !== b.pixels[i + 2] || a.pixels[i + 3] !== b.pixels[i + 3]) pixels += 1;
  }
  return pixels;
}

const MOVE = (indices: readonly number[], x: number, y: number): Lift => ({ indices, scale: 1, x, y });

describe('a selection lifted off the page', () => {
  it('leaves the page without the chosen strokes: the area is repainted, and the pixels are those of a full redraw', () => {
    const strokes = pageStrokes();
    const rig = new Rig(strokes);
    rig.selection = [8, 9];
    rig.paint();
    const withStrokes = rig.fullRedraw();
    expect(differences(rig.cache, pageOf(strokes))).toBe(0);

    rig.lift = MOVE(rig.selection, 3000, 2000);
    const report = rig.paint();
    expect(report.full).toBe(false);
    expect(report.repainted).toBeTypeOf('number');
    expect(report.liftDrawn).toBe(true);
    // The cache is the page without them: what a full redraw of the scene makes, and what a page of the others makes.
    expect(differences(rig.cache, rig.fullRedraw())).toBe(0);
    expect(differences(rig.cache, pageOf(strokes.filter((_, i) => i !== 8 && i !== 9)))).toBe(0);
    expect(differences(rig.cache, withStrokes)).toBeGreaterThan(40);
  });

  it('is drawn once on a surface of its own and copied each frame, moved to where the drag has it', () => {
    const rig = new Rig(pageStrokes());
    rig.selection = [8];
    rig.paint();
    rig.lift = MOVE(rig.selection, 3000, 2000);
    expect(rig.paint().liftDrawn).toBe(true);
    const lifted = rig.target.ops('drawImage');
    // The page's cache, then the lift's surface (the selection's own surface is not drawn: it would show the old place).
    expect(lifted).toHaveLength(2);
    const layer = lifted[1];
    const surface = rig.surfaces.find((s) => (s.image as unknown) === layer.args[0]);
    expect(surface).toBeDefined();
    expect(surface).not.toBe(rig.surfaces[0]);
    // 1 device pixel per CSS pixel: the move is the drag's in screen pixels, the size the surface's.
    const [, dx, dy, dw, dh] = layer.args as number[];
    const unit = TRANSFORM.scale;
    const box = surface!.ctx;
    expect(dw).toBe(box.width);
    expect(dh).toBe(box.height);
    // Where the surface starts on the page without the move: its own corner.
    const still = (() => {
      const other = new Rig(pageStrokes());
      other.selection = [8];
      other.lift = MOVE([8], 0, 0);
      other.paint();
      other.paint();
      return other.target.ops('drawImage')[1].args as number[];
    })();
    expect(dx - still[1]).toBeCloseTo(3000 * unit, 6);
    expect(dy - still[2]).toBeCloseTo(2000 * unit, 6);

    // Another frame of the drag: the same surface, not drawn again, the cache left alone.
    rig.lift = MOVE(rig.selection, 3600, 2600);
    const next = rig.paint();
    expect(next.liftDrawn).toBeUndefined();
    expect(next.full).toBe(false);
    expect(next.repainted).toBeUndefined();
    const again = rig.target.ops('drawImage')[1].args as number[];
    expect(again[1] - dx).toBeCloseTo(600 * unit, 6);
    expect(again[2] - dy).toBeCloseTo(600 * unit, 6);
  });

  it('a resize scales the surface around the anchor: the corner that was pressed follows, the opposite one stays', () => {
    const rig = new Rig(pageStrokes());
    rig.selection = [8, 9];
    rig.paint();
    const frame = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity };
    for (const i of rig.selection) {
      for (const p of rig.live.strokes[i].points) {
        frame.x0 = Math.min(frame.x0, p.x - 100);
        frame.y0 = Math.min(frame.y0, p.y - 100);
        frame.x1 = Math.max(frame.x1, p.x + 100);
        frame.y1 = Math.max(frame.y1, p.y + 100);
      }
    }
    // Half the size around the bottom right corner: x' = x * 0.5 + anchor * 0.5.
    const s = 0.5;
    rig.lift = { indices: rig.selection, scale: s, x: frame.x1 * (1 - s), y: frame.y1 * (1 - s) };
    rig.paint();
    const frames = rig.target.ops('strokeRect');
    // The frame over the strokes, in screen pixels, at the size the drag makes (the box of the ink * 0.5), then the 4 handles.
    const [fx, fy, fw, fh] = frames[0].args as number[];
    const unit = TRANSFORM.scale;
    expect(fw).toBeCloseTo((frame.x1 - frame.x0) * s * unit, 1);
    expect(fh).toBeCloseTo((frame.y1 - frame.y0) * s * unit, 1);
    expect(fx + fw).toBeCloseTo(TRANSFORM.x + frame.x1 * unit, 1);
    expect(fy + fh).toBeCloseTo(TRANSFORM.y + frame.y1 * unit, 1);
    const [, dx, dy, dw, dh] = rig.target.ops('drawImage')[1].args as number[];
    const surface = rig.surfaces[rig.surfaces.length - 1].ctx;
    expect(dw).toBeCloseTo(surface.width * s, 6);
    expect(dh).toBeCloseTo(surface.height * s, 6);
    // The surface's far corner is within a few pixels of the anchor (its ring reaches a little past the frame).
    expect(dx + dw).toBeGreaterThan(fx + fw - 1);
    expect(dy + dh).toBeGreaterThan(fy + fh - 1);
    expect(dx + dw).toBeLessThan(fx + fw + 6);
    expect(dy + dh).toBeLessThan(fy + fh + 6);
  });

  it('keeps the frame and the handles crisp: drawn over the strokes in screen pixels, not scaled with them', () => {
    const rig = new Rig(pageStrokes());
    rig.selection = [8];
    rig.lift = MOVE([8], 1000, 0);
    rig.paint();
    const handles = rig.target.ops('fillRect').filter((c) => (c.args as number[])[2] === HANDLE_SIZE_PX);
    expect(handles).toHaveLength(4);
    for (const handle of handles) expect((handle.args as number[])[3]).toBe(HANDLE_SIZE_PX);
    // The dashed box of the lifted frame, the squares' outlines (4), and the projected area's border.
    expect(rig.target.ops('strokeRect')).toHaveLength(6);
    expect(rig.target.ops('strokeRect')[0].strokeStyle).toBe(SELECTION_COLORS.light.line);
  });

  it('puts the strokes back on the cache when the drag ends without an edit, and draws the highlight again', () => {
    const strokes = pageStrokes();
    const rig = new Rig(strokes);
    rig.selection = [8];
    rig.paint();
    rig.lift = MOVE([8], 3000, 2000);
    rig.paint();
    rig.lift = null;
    const report = rig.paint();
    expect(report.full).toBe(false);
    expect(report.repainted).toBeTypeOf('number');
    expect(report.selectionDrawn).toBe(true);
    expect(differences(rig.cache, pageOf(strokes))).toBe(0);
    expect(differences(rig.cache, rig.fullRedraw())).toBe(0);
  });

  it('the edit that comes while the preview is kept replaces the strokes in one frame: pixels of a full redraw', () => {
    const strokes = pageStrokes();
    const rig = new Rig(strokes);
    rig.selection = [8, 9];
    rig.paint();
    rig.lift = MOVE(rig.selection, 3000, 2000);
    rig.paint();
    rig.paint();
    // The event is applied, and the window drops the preview in that very frame.
    rig.transformed([8, 9], 3000, 2000);
    rig.lift = null;
    const report = rig.paint();
    expect(report.full).toBe(false);
    expect(report.repainted).toBeTypeOf('number');
    expect(differences(rig.cache, rig.fullRedraw())).toBe(0);
    // The page is the others, plus the two where they went.
    const moved = rig.live.strokes;
    expect(moved[8].points[0].x).toBeCloseTo(strokes[8].points[0].x + 3000, 6);
    expect(differences(rig.cache, pageOf(moved.map((s) => s)))).toBe(0);
    // The selection (same indices) shows its highlight at the new place.
    expect(report.selectionDrawn).toBe(true);
  });

  it('does no work per frame that walks the strokes: no outline is computed again while the drag goes on', () => {
    const rig = new Rig(pageStrokes());
    rig.selection = [8, 9, 10];
    rig.paint();
    rig.lift = MOVE(rig.selection, 100, 100);
    rig.paint();
    const before = rig.contours;
    for (let i = 1; i <= 30; i++) {
      rig.lift = { indices: rig.selection, scale: 1 + i / 40, x: 100 * i, y: -50 * i };
      const report = rig.paint();
      expect(report.liftDrawn).toBeUndefined();
      expect(report.repainted).toBeUndefined();
      expect(report.full).toBe(false);
    }
    expect(rig.contours).toBe(before);
  });

  it('leaves a stroke of a hidden layer where it is, and a lift of nothing visible draws nothing', () => {
    const hidden = line(2000, 12000, { layer: 1 });
    const rig = new Rig([...pageStrokes(), hidden]);
    const at = rig.live.strokes.length - 1;
    rig.selection = [at];
    rig.paint();
    rig.lift = MOVE([at], 500, 500);
    const report = rig.paint();
    expect(report.liftDrawn).toBeUndefined();
    expect(report.full).toBe(false);
    expect(differences(rig.cache, pageOf([...pageStrokes(), hidden]))).toBe(0);
  });

  it('another page, or a size that changed, redraws the lifted strokes where they are', () => {
    const strokes = pageStrokes();
    const rig = new Rig(strokes);
    rig.selection = [8];
    rig.paint();
    rig.lift = MOVE([8], 1000, 1000);
    rig.paint();
    rig.renderer.invalidate();
    const report = rig.paint();
    expect(report.full).toBe(true);
    expect(report.liftDrawn).toBe(true);
    expect(differences(rig.cache, pageOf(strokes.filter((_, i) => i !== 8)))).toBe(0);
  });

  it('a stroke the tablet ends meanwhile joins the page, over the rest, with the others lifted', () => {
    const rig = new Rig(pageStrokes());
    rig.selection = [8];
    rig.paint();
    rig.lift = MOVE([8], 1000, 1000);
    rig.paint();
    const points = line(9000, 9000).points.map((p) => ({ x: p.x, y: p.y, p: p.pressure, t: p.t, recv: 1 }));
    rig.receive({ type: 'strokeStart', pageIndex: 0, stroke: { ...line(9000, 9000), points } }, { type: 'strokeEnd', pageIndex: 0 });
    const report = rig.paint();
    expect(report.appended).toBe(1);
    expect(report.full).toBe(false);
    const expected = pageStrokes().filter((_, i) => i !== 8);
    expected.push(line(9000, 9000));
    expect(differences(rig.cache, pageOf(expected))).toBe(0);
  });

  it('the handles are drawn on the frame without a drag, only when asked, never without a selection', () => {
    const rig = new Rig(pageStrokes());
    rig.selection = [8];
    rig.paint();
    const squares = () => rig.target.ops('fillRect').filter((c) => (c.args as number[])[2] === HANDLE_SIZE_PX);
    expect(squares()).toHaveLength(4);
    rig.handles = false;
    rig.paint();
    expect(squares()).toHaveLength(0);
    rig.handles = true;
    rig.selection = [];
    rig.paint();
    expect(squares()).toHaveLength(0);
  });

  it('the presentation never gets a lift or handles', () => {
    const live = new LiveState();
    live.initial(snapshot(pageStrokes()));
    const scene = buildScene(live, {
      mode: 'presentation',
      viewport: VIEWPORT,
      pixelRatio: 1,
      aspect: 16 / 9,
      backgroundImage: null,
      selection: [8],
      lift: MOVE([8], 1000, 1000),
      handles: true,
    });
    expect(scene.lift).toBeNull();
    expect(scene.handles).toBe(false);
    expect(scene.selection).toEqual([]);
  });
});
