/**
 * Drawing of live mode on a 2D canvas (docs/design/tablet-live-classroom.md, "Interface",
 * `LiveCanvas`), kept free of the DOM so that it runs on a fake context in the tests.
 *
 * - The page background (paper colour, then the image `render_background` made of the paper,
 *   template and texture; slate paper while the blackboard is on) and the finished strokes are
 *   drawn once on an offscreen surface, the cache. New finished strokes are added to it; it is
 *   redrawn only when the strokes are replaced (page change), when the view, the size or the
 *   background changes.
 * - An edit (a delete, a colour, an undo: `LiveState.edits`) repaints only the area of the strokes it
 *   touched: the device pixels around them are cleared and drawn again by the same code as a full
 *   redraw, restricted to the strokes crossing them, so that they end up the same (`planRepaint`).
 * - Each frame copies the cache, then draws the strokes in progress (its outline recomputed by
 *   `ink.ts`, the geometry of the exports), the laser dot and, on the control window's preview,
 *   the projected area. M14: two pens may write at once, the tablet's (`current`) and the computer's
 *   (`computerCurrent`); they are drawn in their layers' order (the lower first; on one layer, the
 *   tablet's first), each only while its layer shows. The control window draws its own computer
 *   strokes instead of Rust's copy (`SceneOptions.localComputer`), and the ring of the pen's eraser end.
 *
 * - The control window's selection (M12, `Scene.selection`, `Scene.lasso`) is drawn apart from that
 *   cache: the highlight of the chosen strokes (their outlines, found in the same cache of outlines,
 *   and the box around them) has a surface of its own, redrawn only when the selection, the view,
 *   the size or the strokes change, so that choosing strokes never redraws the page, and a stroke
 *   the tablet ends meanwhile is appended as usual. The highlight goes around the ink and leaves a hole
 *   where the ink is: the page below shows there, so that the strokes keep their real colour. The lasso
 *   being drawn is a few lines per frame. The presentation and the share's viewers never get a selection.
 *
 * - A selection being moved or resized (`Scene.lift`, control window only): the chosen strokes are taken out of
 *   the cache (the partial repaint of the area they leave, drawn without them: `exclusionFlips`), rendered once with
 *   their highlight on a third surface (the lift), and each frame copies that surface scaled and moved to where the
 *   drag has them (a bitmap: nothing per frame walks the strokes or rebuilds an outline), with the frame and the
 *   four handles drawn crisp over it. A drag that ends without an edit puts the strokes back on the cache the same
 *   way. The handles (`Scene.handles`) are drawn on the selection's frame the rest of the time too.
 *
 * Strokes are filled with the non-zero rule. Looks follow `ardoise-render/src/look.rs` where a
 * flat fill can: felt-tip core at 0.9, chalk at 0.95, highlighter multiplied on light paper and
 * screened on dark paper; the pencil and chalk grains are not drawn on screen (as in the editor).
 */
import { type Rect, type Vec } from './editor/geometry';
import { backgroundWidth } from './editor/view';
import type { Ink, Tool } from './ink';
import type { LiveViewState } from './live';
import type { StrokeEdit } from './live-edits';
import {
  fitTransform,
  intersect,
  pageToScreen,
  presentationTransform,
  projectedArea,
  type Size,
  type Transform,
} from './live-view';
import type { Background, Layer, PageSize, Paper, Rgba, Stroke } from './note';
import { HANDLE_SIZE_PX, corners, selectionFrame, transformRect, type PageTransform } from './selection-grab';

/** The part of `CanvasRenderingContext2D` the drawing uses. */
export interface Ctx2D {
  fillStyle: string | CanvasGradient | CanvasPattern;
  strokeStyle: string | CanvasGradient | CanvasPattern;
  lineWidth: number;
  globalAlpha: number;
  globalCompositeOperation: GlobalCompositeOperation;
  setTransform(a: number, b: number, c: number, d: number, e: number, f: number): void;
  clearRect(x: number, y: number, width: number, height: number): void;
  fillRect(x: number, y: number, width: number, height: number): void;
  strokeRect(x: number, y: number, width: number, height: number): void;
  drawImage(image: CanvasImageSource, dx: number, dy: number, dw?: number, dh?: number): void;
  save(): void;
  restore(): void;
  beginPath(): void;
  rect(x: number, y: number, width: number, height: number): void;
  clip(): void;
  moveTo(x: number, y: number): void;
  lineTo(x: number, y: number): void;
  closePath(): void;
  arc(x: number, y: number, radius: number, startAngle: number, endAngle: number): void;
  fill(fillRule?: CanvasFillRule): void;
  stroke(): void;
  /** Dashes of the lines (absent on a stand-in that does not draw them). */
  setLineDash?(segments: number[]): void;
}

/** An offscreen drawing surface (a canvas element in the app). */
export interface Surface {
  /** What `drawImage` copies. */
  readonly image: CanvasImageSource;
  readonly ctx: Ctx2D;
  resize(width: number, height: number): void;
}

/** Margin around the page on the control window's preview, CSS pixels. */
export const PREVIEW_MARGIN = 12;
/** Colour around the page on the projector. */
export const PRESENTATION_SURROUND = '#000000';
const PROJECTED_BORDER = '#2f6fdf';
const OUTSIDE_DIM = 'rgba(0, 0, 0, 0.28)';
const LASER = '#ff1f1f';
const LASER_GLOW = 'rgba(255, 31, 31, 0.28)';
const PEN_RING = 'rgba(47, 111, 223, 0.9)';
/** The ring of the pen's eraser end on the preview: its radius in CSS pixels is the eraser's (`MARK_ERASER_PX`). */
export const ERASER_RING_PX = 18;
const ERASER_RING = 'rgba(47, 111, 223, 0.85)';
const ERASER_RING_DARK = 'rgba(143, 184, 255, 0.9)';
const NO_STROKES: readonly Stroke[] = [];

/** The selection's colours on light paper and on dark paper (the editor's accent, then a lighter blue). */
export const SELECTION_COLORS = {
  light: { line: '#2f6fdf', halo: '#4f87e6', box: 'rgba(47, 111, 223, 0.08)' },
  dark: { line: '#8fb8ff', halo: '#8fb8ff', box: 'rgba(143, 184, 255, 0.12)' },
} as const;
/**
 * The ring around the chosen strokes: the same strokes, wider than the ink by these many CSS pixels (half
 * on each side). The halo's colour (opaque: rings that overlap do not add up), then a band of paper colour
 * between it and the ink, which keeps blue ink apart from the blue halo.
 */
const SELECTION_HALO_PX = 7;
const SELECTION_GAP_PX = 3;
/** The hole left for the ink goes half of this past its outline: antialiased edges keep their real colour too. */
const SELECTION_HOLE_PX = 1;
const NO_SELECTION: readonly number[] = [];
const NO_STROKE_SET: ReadonlySet<Stroke> = new Set();

/**
 * Device pixels added around the strokes an edit touched, and around the strokes tried against that area:
 * antialiasing may touch the pixel next to the one the outline ends in.
 */
const REPAINT_MARGIN_PX = 2;
/** Beyond these shares of the canvas or of the strokes, an edit redraws everything: little would be saved. */
const REPAINT_MAX_AREA = 0.6;
const REPAINT_MAX_STROKES = 0.6;
/** So few strokes cost nothing, whatever their share of the page. */
const REPAINT_FEW_STROKES = 16;

/** Paper colours of `ardoise-render/src/background.rs`. */
const PAPERS: Record<Exclude<Paper, { custom: Rgba }>, [number, number, number]> = {
  white: [255, 255, 255],
  cream: [250, 244, 226],
  kraft: [206, 174, 128],
  slate: [42, 50, 48],
};

/** Opaque colour of `paper` (a custom colour with alpha over white), as in the renderer. */
export function paperRgb(paper: Paper): [number, number, number] {
  if (typeof paper === 'string') return PAPERS[paper] ?? PAPERS.white;
  const c = paper.custom;
  const over = (v: number) => Math.round((v * c.a + 255 * (255 - c.a)) / 255);
  return [over(c.r), over(c.g), over(c.b)];
}

/** Dark paper (slate, dark custom colours): light ink and a screened highlighter. */
export function isDarkPaper(paper: Paper): boolean {
  const [r, g, b] = paperRgb(paper);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b < 110;
}

/** The background drawn: the page's own, or slate paper while the blackboard is on. */
export function effectiveBackground(background: Background, board: boolean): Background {
  return board && background.paper !== 'slate' ? { ...background, paper: 'slate' } : background;
}

/** Opacity of a tool's flat fill (look.rs), before the colour's alpha. */
function toolOpacity(tool: Tool): number {
  switch (tool) {
    case 'feltTip':
      return 0.9;
    case 'chalk':
      return 0.95;
    default:
      return 1;
  }
}

/** Finished strokes in drawing order: visible layers bottom to top, strokes without a layer last. */
export function drawingOrder(strokes: readonly Stroke[], layers: readonly Layer[]): Stroke[] {
  const buckets: Stroke[][] = layers.map(() => []);
  const orphans: Stroke[] = [];
  for (const s of strokes) (buckets[s.layer] ?? orphans).push(s);
  const out: Stroke[] = [];
  layers.forEach((layer, i) => {
    if (layer.visible) out.push(...buckets[i]);
  });
  return out.concat(orphans);
}

/** A selection lifted off the page by a drag: which strokes, and the map the drag has made so far. */
export interface Lift extends PageTransform {
  indices: readonly number[];
}

/** Everything one frame shows. */
export interface Scene {
  page: PageSize;
  background: Background;
  /** The rendered paper, drawn over the paper colour once loaded. */
  backgroundImage: CanvasImageSource | null;
  layers: readonly Layer[];
  /** Finished strokes; `generation` changes when they are replaced or edited rather than appended to. */
  strokes: readonly Stroke[];
  generation: number;
  /** What the last edits touched: a change of `generation` found here repaints only that area. */
  edits: readonly StrokeEdit[];
  /** The tablet's stroke in progress. */
  current: Stroke | null;
  /** The computer's strokes in progress: Rust's copy, or the control window's own strokes. */
  computer: readonly Stroke[];
  /** Temporary marks written over the presentation (not part of the note), and the one in progress. */
  marks: readonly Stroke[];
  markCurrent: Stroke | null;
  /** The control window's selection: indices into `strokes` (a new array when it changes), and the lasso being drawn (page units). */
  selection: readonly number[];
  lasso: readonly Vec[] | null;
  /**
   * The control window's selection being moved or resized: `indices` (those of `selection`) are drawn lifted, where
   * `scale` and (`x`, `y`) of `PageTransform` put them, and left off the page where they were; null: not lifted.
   */
  lift: Lift | null;
  /** The selection's frame carries its four handles. */
  handles: boolean;
  /** The control window: where the pen's eraser end is (page units), drawn as a ring of its size; never projected. */
  eraser: Vec | null;
  hover: Vec | null;
  laser: boolean;
  /** The pen's position as a ring when the laser is off (control window). */
  penRing: boolean;
  transform: Transform;
  /** CSS pixels. */
  viewport: Size;
  pixelRatio: number;
  /** Around the page; null leaves it transparent. */
  surround: string | null;
  /** The projected area on the preview (page units), or null. */
  projected: Rect | null;
  /** Darken the page outside the projected area. */
  dimOutside: boolean;
}

/** What `LiveCanvas` gets from the live state. */
export interface LiveSource {
  pageSize: PageSize;
  background: Background;
  layers: readonly Layer[];
  strokes: readonly Stroke[];
  generation: number;
  edits?: readonly StrokeEdit[];
  current: Stroke | null;
  /** The computer's stroke in progress (M14); absent: none. */
  computerCurrent?: Stroke | null;
  hover: Vec | null;
  view: LiveViewState;
}

export interface SceneOptions {
  /** `preview`: the whole page with the projected area (control window); `presentation`: the view. */
  mode: 'preview' | 'presentation';
  viewport: Size;
  pixelRatio: number;
  /** Shape (width / height) of the projected area, for the preview. */
  aspect: number;
  backgroundImage: CanvasImageSource | null;
  /** Presentation: temporary marks (page units) and the one being written. */
  marks?: readonly Stroke[];
  markCurrent?: Stroke | null;
  /** Preview: the selection (indices into the finished strokes) and the lasso being drawn. */
  selection?: readonly number[];
  lasso?: readonly Vec[] | null;
  /** Preview: the selection is being moved or resized (`Scene.lift`), and whether its frame has handles. */
  lift?: Lift | null;
  handles?: boolean;
  /**
   * Control window: its own computer strokes in progress (drawn from its pointer events, no round trip),
   * drawn instead of `LiveSource.computerCurrent` when given; an empty list draws none of them.
   */
  localComputer?: readonly Stroke[];
  /** Preview: where the pen's eraser end is (page units), or null. */
  eraser?: Vec | null;
}

export function sceneTransform(live: Pick<LiveSource, 'pageSize' | 'view'>, options: Pick<SceneOptions, 'mode' | 'viewport'>): Transform {
  return options.mode === 'presentation'
    ? presentationTransform(live.pageSize, options.viewport, live.view)
    : fitTransform(live.pageSize, options.viewport, PREVIEW_MARGIN);
}

export function buildScene(live: LiveSource, options: SceneOptions): Scene {
  const preview = options.mode === 'preview';
  const page = live.pageSize;
  const sheet = { x: 0, y: 0, width: page.width, height: page.height };
  return {
    page,
    background: effectiveBackground(live.background, live.view.board),
    backgroundImage: options.backgroundImage,
    layers: live.layers,
    strokes: live.strokes,
    generation: live.generation,
    edits: live.edits ?? [],
    current: live.current,
    computer: options.localComputer ?? (live.computerCurrent ? [live.computerCurrent] : NO_STROKES),
    marks: options.marks ?? [],
    markCurrent: options.markCurrent ?? null,
    selection: preview ? (options.selection ?? NO_SELECTION) : NO_SELECTION,
    lasso: preview ? (options.lasso ?? null) : null,
    lift: preview ? (options.lift ?? null) : null,
    handles: preview ? (options.handles ?? false) : false,
    eraser: preview ? (options.eraser ?? null) : null,
    hover: live.hover,
    laser: live.view.laser,
    penRing: preview,
    transform: sceneTransform(live, options),
    viewport: options.viewport,
    pixelRatio: options.pixelRatio,
    surround: preview ? null : PRESENTATION_SURROUND,
    projected: preview ? intersect(projectedArea(page, live.view, options.aspect), sheet) : null,
    dimOutside: preview && live.view.zoom > 1.001,
  };
}

/** Pixel width at which to ask `render_background` for this scene's paper. */
export function backgroundPixels(scene: Pick<Scene, 'page' | 'transform' | 'pixelRatio'>): number {
  return backgroundWidth(scene.page, scene.transform.scale, scene.pixelRatio);
}

/** What one paint did (tests, diagnostics). */
export interface PaintReport {
  /** The cache was redrawn from scratch. */
  full: boolean;
  /** Set when an edit repainted an area of the cache: the strokes drawn in it. */
  repainted?: number;
  /** Finished strokes added to the cache. */
  appended: number;
  /** Samples of the tablet's stroke in progress drawn. */
  currentPoints: number;
  /** Samples of the computer's strokes in progress drawn. */
  computerPoints: number;
  /** Set when the selection's highlight was drawn again (it is copied from its own surface otherwise). */
  selectionDrawn?: true;
  /** Set when the lifted selection was rendered again on its surface (it is copied, scaled, otherwise). */
  liftDrawn?: true;
}

function addContours(ctx: Ctx2D, contours: readonly Float32Array[]): void {
  for (const c of contours) {
    if (c.length < 2) continue;
    ctx.moveTo(c[0], c[1]);
    for (let i = 2; i < c.length; i += 2) ctx.lineTo(c[i], c[i + 1]);
    ctx.closePath();
  }
}

function fillContours(ctx: Ctx2D, contours: readonly Float32Array[]): void {
  ctx.beginPath();
  addContours(ctx, contours);
  ctx.fill('nonzero');
}

/** A finished stroke's geometry, kept per stroke. */
interface Outline {
  contours: Float32Array[];
  /** The box of the contours, page units; null when there are none. */
  bounds: Rect | null;
  /** The cache redraw (`LiveRenderer.cacheRedraw`) that has this stroke on it. */
  onCache: number;
  /** Its selection's highlight, for the page scale it was made at (a selection often comes back: a colour, then Esc). */
  rings?: Rings;
}

/** The outlines of the highlight of a stroke, each the stroke made wider. */
interface Rings {
  scale: number;
  halo: Float32Array[];
  gap: Float32Array[];
  hole: Float32Array[];
}

function contoursBounds(contours: readonly Float32Array[]): Rect | null {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const c of contours) {
    for (let i = 0; i + 1 < c.length; i += 2) {
      if (c[i] < x0) x0 = c[i];
      if (c[i] > x1) x1 = c[i];
      if (c[i + 1] < y0) y0 = c[i + 1];
      if (c[i + 1] > y1) y1 = c[i + 1];
    }
  }
  return x1 < x0 ? null : { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
}

/** A box of device pixels, whole numbers. */
interface PixelBox {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** What an edit needs done to the cache. */
interface Repaint {
  /** Null when no pixel of the canvas changes. */
  area: PixelBox | null;
  /** The strokes crossing `area`, in drawing order. */
  crossing: Stroke[];
  /** The strokes finished since the last paint and not touched by the edit: they go on top of everything. */
  added: Stroke[];
}

export class LiveRenderer {
  private ink: Ink | null;
  private readonly createSurface: () => Surface | null;
  private cache: Surface | null = null;
  private cacheKey = '';
  private cacheSize = '';
  private cacheImage: CanvasImageSource | null = null;
  private cacheStrokes: readonly Stroke[] | null = null;
  private cacheGeneration = 0;
  /** Counts the full redraws of the cache, which say whether a stroke is on it (`Outline.onCache`). */
  private cacheRedraw = 0;
  /** Strokes of `cacheStrokes` already on the cache, and the highest layer rank among them. */
  private drawn = 0;
  private topRank = -1;
  /** Outlines of finished strokes (their points never change, and the colour is not part of them). */
  private outlines = new WeakMap<Stroke, Outline>();
  /** The selection's highlight: its own surface, and what it was drawn for. */
  private selectionLayer: Surface | null = null;
  private selectionKey = '';
  private selectionSize = '';
  private selectionFor: readonly number[] | null = null;
  /** The strokes the cache is drawn without (a lifted selection), and the ones the frame being painted wants left off. */
  private cacheExcluded: ReadonlySet<Stroke> = NO_STROKE_SET;
  private excluded: ReadonlySet<Stroke> = NO_STROKE_SET;
  private liftSet: { indices: readonly number[]; strokes: readonly Stroke[]; generation: number; set: ReadonlySet<Stroke> } | null = null;
  /** The lifted selection's surface (the chosen strokes and their highlight), and what it was drawn for. */
  private liftLayer: Surface | null = null;
  private liftKey = '';
  private liftFor: readonly number[] | null = null;
  /** The device pixels the lift's surface covers, which `drawImage` maps back with the drag's transform. */
  private liftBox: PixelBox | null = null;
  /** The selection's frame (page units, as it is drawn now), cached for the selection, strokes and layers it was found for. */
  private frame: { selection: readonly number[]; strokes: readonly Stroke[]; generation: number; layers: readonly Layer[]; box: Rect | null } | null = null;

  constructor(createSurface: () => Surface | null, ink: Ink | null = null) {
    this.createSurface = createSurface;
    this.ink = ink;
  }

  setInk(ink: Ink | null): void {
    if (ink === this.ink) return;
    this.ink = ink;
    this.outlines = new WeakMap();
    this.invalidate();
  }

  /** The next paint redraws the cache. */
  invalidate(): void {
    this.cacheKey = '';
    this.selectionKey = '';
    this.liftKey = '';
  }

  private outline(stroke: Stroke): Outline | null {
    if (!this.ink) return null;
    let outline = this.outlines.get(stroke);
    if (!outline) {
      const contours = this.ink.contours(stroke);
      outline = { contours, bounds: contoursBounds(contours), onCache: 0 };
      this.outlines.set(stroke, outline);
    }
    return outline;
  }

  private contours(stroke: Stroke): Float32Array[] {
    return this.outline(stroke)?.contours ?? [];
  }

  /** Draws a finished stroke on the cache. */
  private drawOnCache(ctx: Ctx2D, stroke: Stroke, dark: boolean): void {
    const outline = this.outline(stroke);
    if (!outline) return;
    outline.onCache = this.cacheRedraw;
    this.drawStroke(ctx, stroke, outline.contours, dark);
  }

  private drawStroke(ctx: Ctx2D, stroke: Stroke, contours: readonly Float32Array[], dark: boolean): void {
    if (contours.length === 0) return;
    const { r, g, b, a } = stroke.color;
    ctx.globalAlpha = Math.min(1, Math.max(0, (a / 255) * toolOpacity(stroke.tool)));
    ctx.globalCompositeOperation = stroke.tool === 'highlighter' ? (dark ? 'screen' : 'multiply') : 'source-over';
    ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;
    fillContours(ctx, contours);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
  }

  private pageTransform(ctx: Ctx2D, scene: Scene): void {
    const k = scene.pixelRatio;
    const t = scene.transform;
    ctx.setTransform(t.scale * k, 0, 0, t.scale * k, t.x * k, t.y * k);
  }

  private rank(stroke: Stroke, layers: readonly Layer[]): number {
    return stroke.layer < layers.length ? stroke.layer : layers.length;
  }

  private visible(stroke: Stroke, layers: readonly Layer[]): boolean {
    return layers[stroke.layer]?.visible !== false;
  }

  /** What is under the strokes: the surround, the paper, its picture. Ends with the page transform set. */
  private drawBackground(ctx: Ctx2D, scene: Scene, width: number, height: number): void {
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, width, height);
    if (scene.surround) {
      ctx.fillStyle = scene.surround;
      ctx.fillRect(0, 0, width, height);
    }
    this.pageTransform(ctx, scene);
    const [r, g, b] = paperRgb(scene.background.paper);
    ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;
    ctx.fillRect(0, 0, scene.page.width, scene.page.height);
    if (scene.backgroundImage) ctx.drawImage(scene.backgroundImage, 0, 0, scene.page.width, scene.page.height);
  }

  private noteDrawn(scene: Scene): void {
    this.drawn = scene.strokes.length;
    this.topRank = scene.strokes.reduce((top, s) => Math.max(top, this.rank(s, scene.layers)), -1);
    this.cacheGeneration = scene.generation;
  }

  /** Background and every finished stroke, from scratch. */
  private redraw(ctx: Ctx2D, scene: Scene, width: number, height: number): void {
    this.cacheRedraw += 1;
    this.drawBackground(ctx, scene, width, height);
    const dark = isDarkPaper(scene.background.paper);
    for (const stroke of drawingOrder(scene.strokes, scene.layers)) {
      if (!this.excluded.has(stroke)) this.drawOnCache(ctx, stroke, dark);
    }
    this.cacheExcluded = this.excluded;
    this.noteDrawn(scene);
  }

  /** The edits since the last paint; null when one of those changes is not an edit. */
  private editsSince(scene: Scene): StrokeEdit[] | null {
    const since = this.cacheGeneration;
    const missed = scene.generation - since;
    if (missed <= 0) return null;
    const edits = scene.edits.filter((edit) => edit.generation > since && edit.generation <= scene.generation);
    return edits.length === missed ? edits : null;
  }

  /**
   * What the edits since the last paint ask of the cache, or null if only a full redraw is right or
   * worth it. Nothing is drawn here. Whole device pixels: a pixel of the area is replaced entirely, so
   * it owes nothing to what was there, and no stroke outside the list can reach it (it would have to
   * come within `REPAINT_MARGIN_PX`). Strokes finished meanwhile stay out of the area and are appended
   * after it, which is right only if they come last in the drawing order: that is checked here.
   */
  private planRepaint(scene: Scene, width: number, height: number, flips: readonly Stroke[]): Repaint | null {
    if (!this.ink) return null;
    let edits: StrokeEdit[] = [];
    if (scene.generation !== this.cacheGeneration) {
      const found = this.editsSince(scene);
      if (!found) return null;
      edits = found;
    }
    const scale = scene.transform.scale * scene.pixelRatio;
    const originX = scene.transform.x * scene.pixelRatio;
    const originY = scene.transform.y * scene.pixelRatio;
    let x0 = Infinity;
    let y0 = Infinity;
    let x1 = -Infinity;
    let y1 = -Infinity;
    // The strokes an edit touched, and those a lift takes off the cache or puts back on it (`flips`).
    const touched = (stroke: Stroke) => {
      const outline = this.outline(stroke);
      if (!outline) return;
      // The area covers it: it needs no drawing of its own, even if it was finished just now.
      outline.onCache = this.cacheRedraw;
      const b = outline.bounds;
      if (!b || !this.visible(stroke, scene.layers)) return;
      x0 = Math.min(x0, b.x);
      y0 = Math.min(y0, b.y);
      x1 = Math.max(x1, b.x + b.width);
      y1 = Math.max(y1, b.y + b.height);
    };
    for (const edit of edits) edit.touched.forEach(touched);
    flips.forEach(touched);
    let area: PixelBox | null = null;
    if (x1 >= x0) {
      const box = {
        x0: Math.max(0, Math.floor(x0 * scale + originX) - REPAINT_MARGIN_PX),
        y0: Math.max(0, Math.floor(y0 * scale + originY) - REPAINT_MARGIN_PX),
        x1: Math.min(width, Math.ceil(x1 * scale + originX) + REPAINT_MARGIN_PX),
        y1: Math.min(height, Math.ceil(y1 * scale + originY) + REPAINT_MARGIN_PX),
      };
      if (box.x1 > box.x0 && box.y1 > box.y0) area = box;
    }
    if (area && (area.x1 - area.x0) * (area.y1 - area.y0) > REPAINT_MAX_AREA * width * height) return null;

    const crossing: Stroke[] = [];
    const added: Stroke[] = [];
    const order = drawingOrder(scene.strokes, scene.layers);
    for (const stroke of order) {
      // A lifted stroke is on no part of the cache, and in none of the lists: it is on the lift's surface.
      if (this.excluded.has(stroke)) continue;
      const outline = this.outline(stroke);
      if (!outline) continue;
      if (outline.onCache !== this.cacheRedraw) {
        added.push(stroke);
        continue;
      }
      // Under a stroke that is not on the cache yet: drawing that one last would put it on top of this one.
      if (added.length > 0) return null;
      const b = outline.bounds;
      if (
        area &&
        b &&
        b.x * scale + originX < area.x1 + REPAINT_MARGIN_PX &&
        (b.x + b.width) * scale + originX > area.x0 - REPAINT_MARGIN_PX &&
        b.y * scale + originY < area.y1 + REPAINT_MARGIN_PX &&
        (b.y + b.height) * scale + originY > area.y0 - REPAINT_MARGIN_PX
      ) {
        crossing.push(stroke);
      }
    }
    if (crossing.length > REPAINT_MAX_STROKES * order.length && crossing.length > REPAINT_FEW_STROKES) return null;
    return { area, crossing, added };
  }

  /** The area of the cache, drawn by the same code as a full redraw, with what crosses it. */
  private repaintArea(ctx: Ctx2D, scene: Scene, width: number, height: number, area: PixelBox, crossing: readonly Stroke[]): void {
    const dark = isDarkPaper(scene.background.paper);
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.beginPath();
    ctx.rect(area.x0, area.y0, area.x1 - area.x0, area.y1 - area.y0);
    ctx.clip();
    this.drawBackground(ctx, scene, width, height);
    for (const stroke of crossing) this.drawOnCache(ctx, stroke, dark);
    ctx.restore();
  }

  /** Paints `scene` on `target`. */
  paint(target: Ctx2D, scene: Scene): PaintReport {
    const width = Math.max(1, Math.round(scene.viewport.width * scene.pixelRatio));
    const height = Math.max(1, Math.round(scene.viewport.height * scene.pixelRatio));
    const t = scene.transform;
    const key = [
      width,
      height,
      scene.pixelRatio,
      t.scale,
      t.x,
      t.y,
      scene.page.width,
      scene.page.height,
      JSON.stringify(scene.background),
      JSON.stringify(scene.layers),
      scene.surround,
    ].join('|');
    const report: PaintReport = { full: false, appended: 0, currentPoints: 0, computerPoints: 0 };
    const dark = isDarkPaper(scene.background.paper);

    if (!this.cache) this.cache = this.createSurface();
    const cache = this.cache;
    const surface = cache?.ctx ?? target;
    this.excluded = this.lifted(scene);
    const stale = !cache || key !== this.cacheKey || scene.backgroundImage !== this.cacheImage || scene.strokes !== this.cacheStrokes;
    const edited = !stale && scene.generation !== this.cacheGeneration;
    const flips = stale ? NO_STROKES : this.exclusionFlips();
    const repaint = edited || flips.length > 0 ? this.planRepaint(scene, width, height, flips) : null;
    if (repaint) {
      if (repaint.area) this.repaintArea(surface, scene, width, height, repaint.area, repaint.crossing);
      if (repaint.added.length > 0) this.pageTransform(surface, scene);
      for (const s of repaint.added) this.drawOnCache(surface, s, dark);
      this.cacheExcluded = this.excluded;
      this.noteDrawn(scene);
      report.repainted = repaint.area ? repaint.crossing.length : 0;
      report.appended = repaint.added.length;
    } else if (stale || edited || flips.length > 0 || scene.strokes.length < this.drawn) {
      if (cache && this.cacheSize !== `${width}x${height}`) {
        cache.resize(width, height);
        this.cacheSize = `${width}x${height}`;
      }
      this.redraw(surface, scene, width, height);
      this.cacheKey = key;
      this.cacheImage = scene.backgroundImage;
      this.cacheStrokes = scene.strokes;
      report.full = true;
    } else if (scene.strokes.length > this.drawn) {
      const added = scene.strokes.slice(this.drawn);
      if (added.some((s) => this.rank(s, scene.layers) < this.topRank)) {
        // A stroke under strokes already drawn (an older layer): start again.
        this.redraw(surface, scene, width, height);
        report.full = true;
      } else {
        this.pageTransform(surface, scene);
        for (const s of added) {
          if (this.visible(s, scene.layers) && !this.excluded.has(s)) this.drawOnCache(surface, s, dark);
          this.topRank = Math.max(this.topRank, this.rank(s, scene.layers));
        }
        this.drawn = scene.strokes.length;
        report.appended = added.length;
      }
    }

    if (cache) {
      target.setTransform(1, 0, 0, 1, 0, 0);
      target.clearRect(0, 0, width, height);
      target.drawImage(cache.image, 0, 0);
    }
    if (scene.lift && this.excluded.size > 0) this.paintLift(target, scene, `${key}|${scene.generation}`, report);
    else this.paintSelection(target, scene, `${key}|${scene.generation}`, report);
    this.paintInProgress(target, scene, dark, report);
    if (scene.marks.length > 0 || scene.markCurrent) {
      this.pageTransform(target, scene);
      for (const mark of scene.marks) this.drawStroke(target, mark, this.contours(mark), dark);
      // The mark being written grows: its outline is recomputed each frame.
      if (scene.markCurrent && scene.markCurrent.points.length > 0 && this.ink) {
        this.drawStroke(target, scene.markCurrent, this.ink.contours(scene.markCurrent), dark);
      }
    }
    this.overlay(target, scene);
    return report;
  }

  /** The strokes `scene.lift` takes off the page (those of visible layers), the same set while nothing changes. */
  private lifted(scene: Scene): ReadonlySet<Stroke> {
    const lift = scene.lift;
    if (!lift) return NO_STROKE_SET;
    const known = this.liftSet;
    if (known && known.indices === lift.indices && known.strokes === scene.strokes && known.generation === scene.generation) return known.set;
    const set = new Set<Stroke>();
    for (const i of lift.indices) {
      const stroke = scene.strokes[i];
      if (stroke && this.visible(stroke, scene.layers)) set.add(stroke);
    }
    this.liftSet = { indices: lift.indices, strokes: scene.strokes, generation: scene.generation, set };
    return set;
  }

  /** The strokes whose place on the cache changes because the lift began, moved on to others, or ended. */
  private exclusionFlips(): readonly Stroke[] {
    if (this.excluded === this.cacheExcluded) return NO_STROKES;
    const flips: Stroke[] = [];
    for (const stroke of this.excluded) if (!this.cacheExcluded.has(stroke)) flips.push(stroke);
    for (const stroke of this.cacheExcluded) if (!this.excluded.has(stroke)) flips.push(stroke);
    return flips;
  }

  /** The frame of the selection in page units, found again only when the selection or the strokes change. */
  private selectionBox(scene: Scene): Rect | null {
    const known = this.frame;
    if (known && known.selection === scene.selection && known.strokes === scene.strokes && known.generation === scene.generation && known.layers === scene.layers) {
      return known.box;
    }
    const box = selectionFrame(scene.strokes, scene.layers, scene.selection);
    this.frame = { selection: scene.selection, strokes: scene.strokes, generation: scene.generation, layers: scene.layers, box };
    return box;
  }

  /**
   * The strokes in progress over the cache, their outlines recomputed each frame (they grow; the cache keeps
   * only finished strokes): the tablet's and the computer's in their layers' order, the lower first, the
   * tablet's first on one layer (stable sort), each only while its layer shows.
   */
  private paintInProgress(target: Ctx2D, scene: Scene, dark: boolean, report: PaintReport): void {
    const ink = this.ink;
    if (!ink) return;
    const drawn: { stroke: Stroke; computer: boolean }[] = [];
    const add = (stroke: Stroke | null, computer: boolean) => {
      if (stroke && stroke.points.length > 0 && this.visible(stroke, scene.layers)) drawn.push({ stroke, computer });
    };
    add(scene.current, false);
    for (const stroke of scene.computer) add(stroke, true);
    if (drawn.length === 0) return;
    drawn.sort((a, b) => this.rank(a.stroke, scene.layers) - this.rank(b.stroke, scene.layers));
    this.pageTransform(target, scene);
    for (const { stroke, computer } of drawn) {
      this.drawStroke(target, stroke, ink.contours(stroke), dark);
      if (computer) report.computerPoints += stroke.points.length;
      else report.currentPoints = stroke.points.length;
    }
  }

  /**
   * The selection's highlight over the cache. Its surface is redrawn only when the selection (a new
   * array each time), the strokes' generation, the layers, the view or the size change — not when the
   * tablet appends a stroke: the indices of the others do not move.
   */
  private paintSelection(target: Ctx2D, scene: Scene, key: string, report: PaintReport): void {
    if (scene.selection.length === 0) {
      this.selectionFor = null;
      return;
    }
    this.liftKey = '';
    const width = Math.max(1, Math.round(scene.viewport.width * scene.pixelRatio));
    const height = Math.max(1, Math.round(scene.viewport.height * scene.pixelRatio));
    const dark = isDarkPaper(scene.background.paper);
    if (!this.selectionLayer) this.selectionLayer = this.createSurface();
    const layer = this.selectionLayer;
    const wanted = `${key}|${dark}`;
    if (!layer) {
      // No surface (a stand-in): drawn on the frame itself, where no hole can be made: the ink goes back over the ring.
      this.pageTransform(target, scene);
      this.drawSelection(target, scene, dark, scene.selection, { hole: false, box: true });
      for (const i of scene.selection) {
        const stroke = scene.strokes[i];
        if (stroke && this.visible(stroke, scene.layers)) this.drawStroke(target, stroke, this.contours(stroke), dark);
      }
      report.selectionDrawn = true;
      return;
    }
    if (wanted !== this.selectionKey || scene.selection !== this.selectionFor) {
      if (this.selectionSize !== `${width}x${height}`) {
        layer.resize(width, height);
        this.selectionSize = `${width}x${height}`;
      }
      layer.ctx.setTransform(1, 0, 0, 1, 0, 0);
      layer.ctx.clearRect(0, 0, width, height);
      this.pageTransform(layer.ctx, scene);
      this.drawSelection(layer.ctx, scene, dark, scene.selection, { hole: true, box: true });
      this.selectionKey = wanted;
      this.selectionFor = scene.selection;
      report.selectionDrawn = true;
    }
    target.setTransform(1, 0, 0, 1, 0, 0);
    target.drawImage(layer.image, 0, 0);
  }

  private rings(stroke: Stroke, scale: number): Rings | null {
    const outline = this.outline(stroke);
    if (!outline || !this.ink) return null;
    if (outline.rings?.scale === scale) return outline.rings;
    const { tool, width, points } = stroke;
    // A width is scaled by the pressure: this one gives the wanted pixels where the pen pressed hardest.
    const pressure = points.reduce((most, p) => Math.max(most, p.pressure), 0);
    const share = this.ink.widthAt(tool, width, pressure) / width || 1;
    const wider = (px: number) => this.ink?.contours({ tool, points, width: width + px / scale / share }) ?? [];
    outline.rings = { scale, halo: wider(SELECTION_HALO_PX), gap: wider(SELECTION_GAP_PX), hole: wider(SELECTION_HOLE_PX) };
    return outline.rings;
  }

  /**
   * The chosen strokes (visible ones) ringed, and a dashed box around them, in page units. Everything drawn
   * here is then taken out where the ink is: this surface goes over the page, and the page below it shows
   * the strokes as they are, with their colour, their tool's look and whatever lies over them.
   *
   * Fills, one path per stroke: measured on 100 strokes of 30 samples in a real WebView2, one path holding the
   * whole selection takes 70 to 90 ms of calls and two frames are late, and `stroke()` makes one frame very late
   * (some 90 ms in the graphics process) on strokes it has not seen yet.
   */
  private drawSelection(
    ctx: Ctx2D,
    scene: Scene,
    dark: boolean,
    indices: readonly number[],
    { hole, box: framed }: { hole: boolean; box: boolean },
  ): void {
    const colors = dark ? SELECTION_COLORS.dark : SELECTION_COLORS.light;
    const scale = scene.transform.scale || 1;
    const box = selectionFrame(scene.strokes, scene.layers, indices);
    const rings: Rings[] = [];
    for (const i of indices) {
      const stroke = scene.strokes[i];
      if (!stroke || !this.visible(stroke, scene.layers)) continue;
      const ring = this.rings(stroke, scale);
      if (ring) rings.push(ring);
    }
    if (!box) return;
    if (framed) {
      ctx.fillStyle = colors.box;
      ctx.fillRect(box.x, box.y, box.width, box.height);
    }
    ctx.fillStyle = colors.halo;
    for (const ring of rings) fillContours(ctx, ring.halo);
    const [r, g, b] = paperRgb(scene.background.paper);
    ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;
    for (const ring of rings) fillContours(ctx, ring.gap);
    if (hole) {
      ctx.globalCompositeOperation = 'destination-out';
      ctx.fillStyle = '#000';
      for (const ring of rings) fillContours(ctx, ring.hole);
      ctx.globalCompositeOperation = 'source-over';
    }
    if (!framed) return;
    ctx.strokeStyle = colors.line;
    ctx.setLineDash?.([6 / scale, 4 / scale]);
    ctx.lineWidth = 1.5 / scale;
    ctx.strokeRect(box.x, box.y, box.width, box.height);
    ctx.setLineDash?.([]);
  }

  /**
   * The selection lifted off the page by a drag: the chosen strokes in their colours inside their highlight, rendered
   * once on a surface that covers them (and goes unchanged while the drag goes on), then copied each frame to where
   * the drag has them: that is a bitmap scaled and moved, whatever the number of strokes. The page below was
   * repainted without them (`exclusionFlips`). The frame and the handles go over it in screen pixels (`overlay`).
   */
  private paintLift(target: Ctx2D, scene: Scene, key: string, report: PaintReport): void {
    const lift = scene.lift;
    const box = this.selectionBox(scene);
    if (!lift || !box) return;
    this.selectionFor = null;
    const dark = isDarkPaper(scene.background.paper);
    const k = scene.pixelRatio;
    const t = scene.transform;
    const unit = t.scale * k;
    const originX = t.x * k;
    const originY = t.y * k;
    const layer = this.liftLayer ?? (this.liftLayer = this.createSurface());
    if (!layer) {
      // No surface (a stand-in): the strokes themselves, where the drag has them (the page units go to device pixels).
      target.setTransform(unit * lift.scale, 0, 0, unit * lift.scale, unit * lift.x + originX, unit * lift.y + originY);
      this.drawSelection(target, scene, dark, lift.indices, { hole: false, box: false });
      this.drawLifted(target, scene, lift.indices, dark);
      report.liftDrawn = true;
      return;
    }
    const width = Math.max(1, Math.round(scene.viewport.width * k));
    const height = Math.max(1, Math.round(scene.viewport.height * k));
    // The surface covers the strokes and their ring, no more.
    const reach = Math.ceil((SELECTION_HALO_PX / 2 + 1) * k) + 1;
    const x0 = Math.max(0, Math.floor(box.x * unit + originX) - reach);
    const y0 = Math.max(0, Math.floor(box.y * unit + originY) - reach);
    const x1 = Math.min(width, Math.ceil((box.x + box.width) * unit + originX) + reach);
    const y1 = Math.min(height, Math.ceil((box.y + box.height) * unit + originY) + reach);
    if (x1 <= x0 || y1 <= y0) return;
    const wanted = `${key}|${dark}|${x0},${y0},${x1},${y1}|${lift.indices.length}`;
    if (wanted !== this.liftKey || this.liftFor !== lift.indices) {
      layer.resize(x1 - x0, y1 - y0);
      layer.ctx.setTransform(1, 0, 0, 1, 0, 0);
      layer.ctx.clearRect(0, 0, x1 - x0, y1 - y0);
      layer.ctx.setTransform(unit, 0, 0, unit, originX - x0, originY - y0);
      this.drawSelection(layer.ctx, scene, dark, lift.indices, { hole: false, box: false });
      this.drawLifted(layer.ctx, scene, lift.indices, dark);
      this.liftKey = wanted;
      this.liftFor = lift.indices;
      this.liftBox = { x0, y0, x1, y1 };
      report.liftDrawn = true;
    }
    const at = this.liftBox;
    if (!at) return;
    // A device point d goes to s * (d - origin) + origin + unit * (x, y): the surface's corner and size follow.
    const s = lift.scale;
    target.setTransform(1, 0, 0, 1, 0, 0);
    target.drawImage(
      layer.image,
      s * (at.x0 - originX) + originX + unit * lift.x,
      s * (at.y0 - originY) + originY + unit * lift.y,
      (at.x1 - at.x0) * s,
      (at.y1 - at.y0) * s,
    );
  }

  /** The lifted strokes' ink, in index order, over their ring. */
  private drawLifted(ctx: Ctx2D, scene: Scene, indices: readonly number[], dark: boolean): void {
    for (const i of indices) {
      const stroke = scene.strokes[i];
      if (stroke && this.visible(stroke, scene.layers)) this.drawStroke(ctx, stroke, this.contours(stroke), dark);
    }
  }

  /** The four handles of the selection's frame `box` (page units): squares of `HANDLE_SIZE_PX` screen pixels. */
  private drawHandles(ctx: Ctx2D, scene: Scene, box: Rect): void {
    const dark = isDarkPaper(scene.background.paper);
    const colors = dark ? SELECTION_COLORS.dark : SELECTION_COLORS.light;
    ctx.fillStyle = dark ? '#1b2120' : '#ffffff';
    ctx.strokeStyle = colors.line;
    ctx.lineWidth = 1.5;
    for (const corner of corners(box)) {
      const at = pageToScreen(scene.transform, corner);
      ctx.fillRect(at.x - HANDLE_SIZE_PX / 2, at.y - HANDLE_SIZE_PX / 2, HANDLE_SIZE_PX, HANDLE_SIZE_PX);
      ctx.strokeRect(at.x - HANDLE_SIZE_PX / 2, at.y - HANDLE_SIZE_PX / 2, HANDLE_SIZE_PX, HANDLE_SIZE_PX);
    }
  }

  /** The frame of a lifted selection where the drag has it (the surface holds the strokes, not the frame), and the handles. */
  private drawFrame(ctx: Ctx2D, scene: Scene): void {
    const box = this.selectionBox(scene);
    if (!box) return;
    const lift = scene.lift;
    const lifted = lift !== null && this.excluded.size > 0;
    const shown = lifted ? transformRect(box, lift) : box;
    if (lifted) {
      const colors = isDarkPaper(scene.background.paper) ? SELECTION_COLORS.dark : SELECTION_COLORS.light;
      const a = pageToScreen(scene.transform, { x: shown.x, y: shown.y });
      const w = shown.width * scene.transform.scale;
      const h = shown.height * scene.transform.scale;
      ctx.fillStyle = colors.box;
      ctx.fillRect(a.x, a.y, w, h);
      ctx.strokeStyle = colors.line;
      ctx.lineWidth = 1.5;
      ctx.setLineDash?.([6, 4]);
      ctx.strokeRect(a.x, a.y, w, h);
      ctx.setLineDash?.([]);
    }
    if (scene.handles) this.drawHandles(ctx, scene, shown);
  }

  /** The lasso being drawn, in screen pixels. */
  private drawLasso(ctx: Ctx2D, scene: Scene, lasso: readonly Vec[]): void {
    const colors = isDarkPaper(scene.background.paper) ? SELECTION_COLORS.dark : SELECTION_COLORS.light;
    ctx.beginPath();
    lasso.forEach((p, i) => {
      const at = pageToScreen(scene.transform, p);
      if (i === 0) ctx.moveTo(at.x, at.y);
      else ctx.lineTo(at.x, at.y);
    });
    ctx.fillStyle = colors.box;
    ctx.fill('nonzero');
    ctx.strokeStyle = colors.line;
    ctx.lineWidth = 1.5;
    ctx.setLineDash?.([4, 3]);
    ctx.stroke();
    ctx.setLineDash?.([]);
  }

  /** Projected area, pen ring and laser dot, in screen pixels. */
  private overlay(ctx: Ctx2D, scene: Scene): void {
    const k = scene.pixelRatio;
    ctx.setTransform(k, 0, 0, k, 0, 0);
    if (scene.selection.length > 0 && (scene.handles || scene.lift)) this.drawFrame(ctx, scene);
    if (scene.lasso && scene.lasso.length > 1) this.drawLasso(ctx, scene, scene.lasso);
    if (scene.projected) {
      const t = scene.transform;
      const a = pageToScreen(t, { x: scene.projected.x, y: scene.projected.y });
      const aw = scene.projected.width * t.scale;
      const ah = scene.projected.height * t.scale;
      if (scene.dimOutside) {
        const p = pageToScreen(t, { x: 0, y: 0 });
        const pw = scene.page.width * t.scale;
        const ph = scene.page.height * t.scale;
        ctx.fillStyle = OUTSIDE_DIM;
        ctx.fillRect(p.x, p.y, pw, a.y - p.y);
        ctx.fillRect(p.x, a.y + ah, pw, p.y + ph - (a.y + ah));
        ctx.fillRect(p.x, a.y, a.x - p.x, ah);
        ctx.fillRect(a.x + aw, a.y, p.x + pw - (a.x + aw), ah);
      }
      ctx.strokeStyle = PROJECTED_BORDER;
      ctx.lineWidth = 2;
      ctx.strokeRect(a.x, a.y, aw, ah);
    }
    if (scene.eraser) {
      const at = pageToScreen(scene.transform, scene.eraser);
      ctx.strokeStyle = isDarkPaper(scene.background.paper) ? ERASER_RING_DARK : ERASER_RING;
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(at.x, at.y, ERASER_RING_PX, 0, Math.PI * 2);
      ctx.stroke();
    }
    if (!scene.hover) return;
    const at = pageToScreen(scene.transform, scene.hover);
    if (scene.laser) {
      const radius = Math.min(12, Math.max(4, scene.viewport.width / 160));
      ctx.fillStyle = LASER_GLOW;
      ctx.beginPath();
      ctx.arc(at.x, at.y, radius * 2.2, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = LASER;
      ctx.beginPath();
      ctx.arc(at.x, at.y, radius, 0, Math.PI * 2);
      ctx.fill();
    } else if (scene.penRing) {
      ctx.strokeStyle = PEN_RING;
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(at.x, at.y, 5, 0, Math.PI * 2);
      ctx.stroke();
    }
  }
}
