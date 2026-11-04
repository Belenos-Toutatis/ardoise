/**
 * Drawing of live mode on a 2D canvas (docs/design/tablet-live-classroom.md, "Interface",
 * `LiveCanvas`), kept free of the DOM so that it runs on a fake context in the tests.
 *
 * - The page background (paper colour, then the image `render_background` made of the paper,
 *   template and texture; slate paper while the blackboard is on) and the finished strokes are
 *   drawn once on an offscreen surface, the cache. New finished strokes are added to it; it is
 *   redrawn only when the strokes are replaced (page change, clear), when the view, the size or
 *   the background changes.
 * - Each frame copies the cache, then draws the stroke in progress (its outline recomputed by
 *   `ink.ts`, the geometry of the exports), the laser dot and, on the control window's preview,
 *   the projected area.
 *
 * - The control window's selection (M12, `Scene.selection`, `Scene.lasso`) is drawn apart from that
 *   cache: the highlight of the chosen strokes (their outlines, found in the same cache of outlines,
 *   and the box around them) has a surface of its own, redrawn only when the selection, the view,
 *   the size or the strokes change, so that choosing strokes never redraws the page, and a stroke
 *   the tablet ends meanwhile is appended as usual. The lasso being drawn is a few lines per frame.
 *   The presentation and the share's viewers never get a selection.
 *
 * Strokes are filled with the non-zero rule. Looks follow `ardoise-render/src/look.rs` where a
 * flat fill can: felt-tip core at 0.9, chalk at 0.95, highlighter multiplied on light paper and
 * screened on dark paper; the pencil and chalk grains are not drawn on screen (as in the editor).
 */
import { strokeBounds, union, type Rect, type Vec } from './editor/geometry';
import { backgroundWidth } from './editor/view';
import type { Ink, Tool } from './ink';
import type { LiveViewState } from './live';
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
  beginPath(): void;
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

/** The selection's colours on light paper and on dark paper (the editor's accent, then a lighter blue). */
export const SELECTION_COLORS = {
  light: { line: '#2f6fdf', halo: 'rgba(47, 111, 223, 0.28)', box: 'rgba(47, 111, 223, 0.08)' },
  dark: { line: '#8fb8ff', halo: 'rgba(143, 184, 255, 0.32)', box: 'rgba(143, 184, 255, 0.12)' },
} as const;
/** Width of the selection's outline and of the glow around it, CSS pixels. */
const SELECTION_LINE_PX = 1.5;
const SELECTION_HALO_PX = 6;
const NO_SELECTION: readonly number[] = [];

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

/** Everything one frame shows. */
export interface Scene {
  page: PageSize;
  background: Background;
  /** The rendered paper, drawn over the paper colour once loaded. */
  backgroundImage: CanvasImageSource | null;
  layers: readonly Layer[];
  /** Finished strokes; `generation` changes when they are replaced rather than appended to. */
  strokes: readonly Stroke[];
  generation: number;
  current: Stroke | null;
  /** Temporary marks written over the presentation (not part of the note), and the one in progress. */
  marks: readonly Stroke[];
  markCurrent: Stroke | null;
  /** The control window's selection: indices into `strokes` (a new array when it changes), and the lasso being drawn (page units). */
  selection: readonly number[];
  lasso: readonly Vec[] | null;
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
  current: Stroke | null;
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
    current: live.current,
    marks: options.marks ?? [],
    markCurrent: options.markCurrent ?? null,
    selection: preview ? (options.selection ?? NO_SELECTION) : NO_SELECTION,
    lasso: preview ? (options.lasso ?? null) : null,
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
  /** Finished strokes added to the cache. */
  appended: number;
  /** Samples of the stroke in progress drawn. */
  currentPoints: number;
  /** Set when the selection's highlight was drawn again (it is copied from its own surface otherwise). */
  selectionDrawn?: true;
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

export class LiveRenderer {
  private ink: Ink | null;
  private readonly createSurface: () => Surface | null;
  private cache: Surface | null = null;
  private cacheKey = '';
  private cacheSize = '';
  private cacheImage: CanvasImageSource | null = null;
  private cacheStrokes: readonly Stroke[] | null = null;
  /** Strokes of `cacheStrokes` already on the cache, and the highest layer rank among them. */
  private drawn = 0;
  private topRank = -1;
  /** Outlines of finished strokes (they never change). */
  private outlines = new WeakMap<Stroke, Float32Array[]>();
  /** The selection's highlight: its own surface, and what it was drawn for. */
  private selectionLayer: Surface | null = null;
  private selectionKey = '';
  private selectionSize = '';
  private selectionFor: readonly number[] | null = null;

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
  }

  private contours(stroke: Stroke): Float32Array[] {
    if (!this.ink) return [];
    let c = this.outlines.get(stroke);
    if (!c) {
      c = this.ink.contours(stroke);
      this.outlines.set(stroke, c);
    }
    return c;
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

  /** Background and every finished stroke, from scratch. */
  private redraw(ctx: Ctx2D, scene: Scene, width: number, height: number): void {
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
    const dark = isDarkPaper(scene.background.paper);
    for (const stroke of drawingOrder(scene.strokes, scene.layers)) {
      this.drawStroke(ctx, stroke, this.contours(stroke), dark);
    }
    this.drawn = scene.strokes.length;
    this.topRank = scene.strokes.reduce((top, s) => Math.max(top, this.rank(s, scene.layers)), -1);
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
      scene.generation,
      JSON.stringify(scene.background),
      JSON.stringify(scene.layers),
      scene.surround,
    ].join('|');
    const report: PaintReport = { full: false, appended: 0, currentPoints: 0 };
    const dark = isDarkPaper(scene.background.paper);

    if (!this.cache) this.cache = this.createSurface();
    const cache = this.cache;
    const surface = cache?.ctx ?? target;
    const stale =
      !cache ||
      key !== this.cacheKey ||
      scene.backgroundImage !== this.cacheImage ||
      scene.strokes !== this.cacheStrokes ||
      scene.strokes.length < this.drawn;
    if (stale) {
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
          if (this.visible(s, scene.layers)) this.drawStroke(surface, s, this.contours(s), dark);
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
    this.paintSelection(target, scene, key, report);
    const current = scene.current;
    if (current && current.points.length > 0 && this.ink && this.visible(current, scene.layers)) {
      this.pageTransform(target, scene);
      // Recomputed each frame: the stroke grows (the cache keeps only finished strokes).
      this.drawStroke(target, current, this.ink.contours(current), dark);
      report.currentPoints = current.points.length;
    }
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
    const width = Math.max(1, Math.round(scene.viewport.width * scene.pixelRatio));
    const height = Math.max(1, Math.round(scene.viewport.height * scene.pixelRatio));
    const dark = isDarkPaper(scene.background.paper);
    if (!this.selectionLayer) this.selectionLayer = this.createSurface();
    const layer = this.selectionLayer;
    const wanted = `${key}|${dark}`;
    if (!layer) {
      // No surface (a stand-in): drawn on the frame itself.
      this.pageTransform(target, scene);
      this.drawSelection(target, scene, dark);
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
      this.drawSelection(layer.ctx, scene, dark);
      this.selectionKey = wanted;
      this.selectionFor = scene.selection;
      report.selectionDrawn = true;
    }
    target.setTransform(1, 0, 0, 1, 0, 0);
    target.drawImage(layer.image, 0, 0);
  }

  /** The chosen strokes (visible ones) outlined with a glow, and a dashed box around them, in page units. */
  private drawSelection(ctx: Ctx2D, scene: Scene, dark: boolean): void {
    const colors = dark ? SELECTION_COLORS.dark : SELECTION_COLORS.light;
    const scale = scene.transform.scale || 1;
    let box: Rect | null = null;
    ctx.beginPath();
    for (const i of scene.selection) {
      const stroke = scene.strokes[i];
      if (!stroke || !this.visible(stroke, scene.layers)) continue;
      addContours(ctx, this.contours(stroke));
      box = union(box, strokeBounds(stroke));
    }
    ctx.strokeStyle = colors.halo;
    ctx.lineWidth = SELECTION_HALO_PX / scale;
    ctx.stroke();
    ctx.strokeStyle = colors.line;
    ctx.lineWidth = SELECTION_LINE_PX / scale;
    ctx.stroke();
    if (!box) return;
    ctx.fillStyle = colors.box;
    ctx.fillRect(box.x, box.y, box.width, box.height);
    ctx.setLineDash?.([6 / scale, 4 / scale]);
    ctx.lineWidth = 1.5 / scale;
    ctx.strokeRect(box.x, box.y, box.width, box.height);
    ctx.setLineDash?.([]);
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
