/**
 * A small software canvas for the tests (jsdom has no canvas): the part of `CanvasRenderingContext2D` the live
 * drawing uses, with real pixels, so that two ways of drawing can be compared pixel by pixel. The pixels are floats
 * (premultiplied red, green, blue, alpha, 0 to 1), filled shapes are antialiased by coverage (exact for rectangles, 4
 * x 4 samples per pixel for paths), and the composite operations follow the Compositing and Blending specification.
 * A clip must be one rectangle of whole pixels, as the renderer's is meant to be: anything else throws.
 */
import type { Ctx2D, Surface } from './live-render';

/** A picture for `drawImage` (the page's background image): its colour at `u, v` (0 to 1), straight RGBA, 0 to 1. */
export class RasterPicture {
  constructor(readonly sample: (u: number, v: number) => [number, number, number, number]) {}
}

interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

interface State {
  fillStyle: string | CanvasGradient | CanvasPattern;
  strokeStyle: string | CanvasGradient | CanvasPattern;
  lineWidth: number;
  globalAlpha: number;
  globalCompositeOperation: GlobalCompositeOperation;
  matrix: [number, number, number];
  clipBox: Box | null;
}

type Rgb = [number, number, number];

function parseColor(style: unknown): [number, number, number, number] {
  const text = String(style);
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(text);
  if (hex) {
    const h = hex[1].length === 3 ? [...hex[1]].map((c) => c + c).join('') : hex[1];
    return [parseInt(h.slice(0, 2), 16) / 255, parseInt(h.slice(2, 4), 16) / 255, parseInt(h.slice(4, 6), 16) / 255, 1];
  }
  const rgb = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(text);
  if (!rgb) throw new Error(`a colour the test canvas cannot read: ${text}`);
  return [+rgb[1] / 255, +rgb[2] / 255, +rgb[3] / 255, rgb[4] === undefined ? 1 : +rgb[4]];
}

export class RasterContext implements Ctx2D {
  fillStyle: string | CanvasGradient | CanvasPattern = '#000000';
  strokeStyle: string | CanvasGradient | CanvasPattern = '#000000';
  lineWidth = 1;
  globalAlpha = 1;
  globalCompositeOperation: GlobalCompositeOperation = 'source-over';
  width = 0;
  height = 0;
  pixels = new Float64Array(0);
  private matrix: [number, number, number] = [1, 0, 0];
  private clipBox: Box | null = null;
  private stack: State[] = [];
  /** The subpaths of the current path, device pixels. */
  private polygons: number[][] = [];
  private current: number[] | null = null;
  /** The boxes given to `rect` since `beginPath`, to check what `clip` gets. */
  private rects: Box[] = [];
  private pathOps = 0;

  constructor(width = 0, height = 0) {
    this.resize(width, height);
  }

  resize(width: number, height: number): void {
    this.width = width;
    this.height = height;
    this.pixels = new Float64Array(width * height * 4);
    this.matrix = [1, 0, 0];
    this.clipBox = null;
    this.stack = [];
  }

  /** The colour of a pixel, as a canvas shows it over nothing: `[r, g, b, a]`, premultiplied, 0 to 1. */
  pixel(x: number, y: number): [number, number, number, number] {
    const i = (Math.floor(y) * this.width + Math.floor(x)) * 4;
    return [this.pixels[i], this.pixels[i + 1], this.pixels[i + 2], this.pixels[i + 3]];
  }

  save(): void {
    this.stack.push({
      fillStyle: this.fillStyle,
      strokeStyle: this.strokeStyle,
      lineWidth: this.lineWidth,
      globalAlpha: this.globalAlpha,
      globalCompositeOperation: this.globalCompositeOperation,
      matrix: this.matrix,
      clipBox: this.clipBox,
    });
  }

  restore(): void {
    const state = this.stack.pop();
    if (!state) return;
    this.fillStyle = state.fillStyle;
    this.strokeStyle = state.strokeStyle;
    this.lineWidth = state.lineWidth;
    this.globalAlpha = state.globalAlpha;
    this.globalCompositeOperation = state.globalCompositeOperation;
    this.matrix = state.matrix;
    this.clipBox = state.clipBox;
  }

  setTransform(a: number, b: number, c: number, d: number, e: number, f: number): void {
    if (b !== 0 || c !== 0 || a !== d) throw new Error('the test canvas knows uniform scales only');
    // Kept apart: x and y offsets differ.
    this.matrix = [a, e, f];
  }

  private dx(x: number): number {
    return x * this.matrix[0] + this.matrix[1];
  }

  private dy(y: number): number {
    return y * this.matrix[0] + this.matrix[2];
  }

  beginPath(): void {
    this.polygons = [];
    this.current = null;
    this.rects = [];
    this.pathOps = 0;
  }

  moveTo(x: number, y: number): void {
    this.pathOps += 1;
    this.current = [this.dx(x), this.dy(y)];
    this.polygons.push(this.current);
  }

  lineTo(x: number, y: number): void {
    this.pathOps += 1;
    if (!this.current) this.moveTo(x, y);
    else this.current.push(this.dx(x), this.dy(y));
  }

  closePath(): void {
    this.current = null;
  }

  arc(): void {
    throw new Error('the test canvas draws no arcs');
  }

  rect(x: number, y: number, width: number, height: number): void {
    this.pathOps += 1;
    const box = { x0: this.dx(x), y0: this.dy(y), x1: this.dx(x + width), y1: this.dy(y + height) };
    this.rects.push(box);
    this.polygons.push([box.x0, box.y0, box.x1, box.y0, box.x1, box.y1, box.x0, box.y1]);
    this.current = null;
  }

  clip(): void {
    const box = this.rects[0];
    if (this.rects.length !== 1 || this.pathOps !== 1 || !box) throw new Error('a clip of the test canvas is one rectangle');
    if (![box.x0, box.y0, box.x1, box.y1].every(Number.isInteger)) {
      throw new Error(`a clip off the pixel grid leaves a seam: ${JSON.stringify(box)}`);
    }
    const c = this.clipBox;
    this.clipBox = c
      ? { x0: Math.max(c.x0, box.x0), y0: Math.max(c.y0, box.y0), x1: Math.min(c.x1, box.x1), y1: Math.min(c.y1, box.y1) }
      : box;
  }

  private bounds(): Box {
    const c = this.clipBox;
    return c
      ? { x0: Math.max(0, c.x0), y0: Math.max(0, c.y0), x1: Math.min(this.width, c.x1), y1: Math.min(this.height, c.y1) }
      : { x0: 0, y0: 0, x1: this.width, y1: this.height };
  }

  private blend(i: number, source: Rgb, alpha: number): void {
    const p = this.pixels;
    if (alpha <= 0) return;
    const sr = source[0] * alpha;
    const sg = source[1] * alpha;
    const sb = source[2] * alpha;
    const da = p[i + 3];
    switch (this.globalCompositeOperation) {
      case 'source-over':
        p[i] = sr + p[i] * (1 - alpha);
        p[i + 1] = sg + p[i + 1] * (1 - alpha);
        p[i + 2] = sb + p[i + 2] * (1 - alpha);
        p[i + 3] = alpha + da * (1 - alpha);
        break;
      case 'multiply':
        p[i] = sr * (1 - da) + p[i] * (1 - alpha) + sr * p[i];
        p[i + 1] = sg * (1 - da) + p[i + 1] * (1 - alpha) + sg * p[i + 1];
        p[i + 2] = sb * (1 - da) + p[i + 2] * (1 - alpha) + sb * p[i + 2];
        p[i + 3] = alpha + da - alpha * da;
        break;
      case 'screen':
        p[i] = sr + p[i] - sr * p[i];
        p[i + 1] = sg + p[i + 1] - sg * p[i + 1];
        p[i + 2] = sb + p[i + 2] - sb * p[i + 2];
        p[i + 3] = alpha + da - alpha * da;
        break;
      case 'destination-out':
        p[i] *= 1 - alpha;
        p[i + 1] *= 1 - alpha;
        p[i + 2] *= 1 - alpha;
        p[i + 3] *= 1 - alpha;
        break;
      default:
        throw new Error(`a composite the test canvas does not know: ${this.globalCompositeOperation}`);
    }
  }

  /** Blends a rectangle in device pixels, with exact coverage. */
  private paintBox(x0: number, y0: number, x1: number, y1: number, colorAt: (x: number, y: number) => [number, number, number, number]): void {
    const b = this.bounds();
    const from = { x: Math.max(b.x0, Math.floor(x0)), y: Math.max(b.y0, Math.floor(y0)) };
    const to = { x: Math.min(b.x1, Math.ceil(x1)), y: Math.min(b.y1, Math.ceil(y1)) };
    for (let y = from.y; y < to.y; y++) {
      const coverY = Math.min(y + 1, y1) - Math.max(y, y0);
      for (let x = from.x; x < to.x; x++) {
        const cover = coverY * (Math.min(x + 1, x1) - Math.max(x, x0));
        const c = colorAt(x, y);
        this.blend((y * this.width + x) * 4, [c[0], c[1], c[2]], cover * c[3] * this.globalAlpha);
      }
    }
  }

  clearRect(x: number, y: number, width: number, height: number): void {
    const b = this.bounds();
    const x0 = Math.max(b.x0, Math.round(this.dx(x)));
    const y0 = Math.max(b.y0, Math.round(this.dy(y)));
    const x1 = Math.min(b.x1, Math.round(this.dx(x + width)));
    const y1 = Math.min(b.y1, Math.round(this.dy(y + height)));
    for (let py = y0; py < y1; py++) this.pixels.fill(0, (py * this.width + x0) * 4, (py * this.width + x1) * 4);
  }

  fillRect(x: number, y: number, width: number, height: number): void {
    const color = parseColor(this.fillStyle);
    this.paintBox(this.dx(x), this.dy(y), this.dx(x + width), this.dy(y + height), () => color);
  }

  strokeRect(): void {
    // The dashes of the selection's box: no test looks at them on this canvas.
  }

  drawImage(image: CanvasImageSource, dx: number, dy: number, dw?: number, dh?: number): void {
    if (image instanceof RasterContext) {
      const source = image;
      const x = this.dx(dx);
      const y = this.dy(dy);
      this.paintBox(x, y, x + source.width, y + source.height, (px, py) => {
        const [r, g, b, a] = source.pixel(px - x, py - y);
        return a > 0 ? [r / a, g / a, b / a, a] : [0, 0, 0, 0];
      });
      return;
    }
    const picture = image as unknown as RasterPicture;
    if (dw === undefined || dh === undefined) throw new Error('a picture is drawn with its size');
    const x0 = this.dx(dx);
    const y0 = this.dy(dy);
    const x1 = this.dx(dx + dw);
    const y1 = this.dy(dy + dh);
    this.paintBox(x0, y0, x1, y1, (px, py) => picture.sample((px + 0.5 - x0) / (x1 - x0), (py + 0.5 - y0) / (y1 - y0)));
  }

  /** Blends `polygons` with the non-zero rule, 4 x 4 samples per pixel. */
  private paintPolygons(polygons: readonly number[][], style: unknown): void {
    const [r, g, b, a] = parseColor(style);
    const clip = this.bounds();
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const polygon of polygons) {
      for (let k = 0; k + 1 < polygon.length; k += 2) {
        minX = Math.min(minX, polygon[k]);
        maxX = Math.max(maxX, polygon[k]);
        minY = Math.min(minY, polygon[k + 1]);
        maxY = Math.max(maxY, polygon[k + 1]);
      }
    }
    if (maxX < minX) return;
    const x0 = Math.max(clip.x0, Math.floor(minX));
    const x1 = Math.min(clip.x1, Math.ceil(maxX));
    const y0 = Math.max(clip.y0, Math.floor(minY));
    const y1 = Math.min(clip.y1, Math.ceil(maxY));
    if (x1 <= x0 || y1 <= y0) return;
    const SAMPLES = 4;
    const coverage = new Float64Array(x1 - x0);
    for (let y = y0; y < y1; y++) {
      coverage.fill(0);
      for (let sub = 0; sub < SAMPLES; sub++) {
        const sy = y + (sub + 0.5) / SAMPLES;
        const crossings: [number, number][] = [];
        for (const polygon of polygons) {
          const n = polygon.length;
          for (let k = 0; k + 1 < n; k += 2) {
            const ax = polygon[k];
            const ay = polygon[k + 1];
            const bx = polygon[(k + 2) % n];
            const by = polygon[(k + 3) % n];
            if (ay <= sy === by <= sy) continue;
            crossings.push([ax + ((sy - ay) / (by - ay)) * (bx - ax), by > ay ? 1 : -1]);
          }
        }
        crossings.sort((p, q) => p[0] - q[0]);
        let winding = 0;
        for (let k = 0; k + 1 < crossings.length; k++) {
          winding += crossings[k][1];
          if (winding === 0) continue;
          const from = crossings[k][0];
          const to = crossings[k + 1][0];
          const first = Math.max(0, Math.ceil((from - x0) * SAMPLES - 0.5));
          const last = Math.min(coverage.length * SAMPLES - 1, Math.ceil((to - x0) * SAMPLES - 0.5) - 1);
          for (let s = first; s <= last; s++) coverage[Math.floor(s / SAMPLES)] += 1 / (SAMPLES * SAMPLES);
        }
      }
      for (let x = x0; x < x1; x++) {
        this.blend((y * this.width + x) * 4, [r, g, b], coverage[x - x0] * a * this.globalAlpha);
      }
    }
  }

  fill(): void {
    this.paintPolygons(this.polygons, this.fillStyle);
  }

  stroke(): void {
    // Lines of the overlays: no test looks at them on this canvas.
  }
}

/** An offscreen surface with pixels. */
export class RasterSurface implements Surface {
  readonly ctx = new RasterContext();
  get image(): CanvasImageSource {
    return this.ctx as unknown as CanvasImageSource;
  }

  resize(width: number, height: number): void {
    this.ctx.resize(width, height);
  }
}
