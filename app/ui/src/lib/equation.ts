/**
 * The equation tool's conversion (SPEC §4.4 « Équation », `docs/design/equations.md`): LaTeX source → MathJax's SVG
 * output (`lib/mathjax.ts`, loaded the first time the tool converts, so that the editor's start does not wait for it) →
 * our own drawing, built here as path data (`lib/equation-path.ts`). Nothing of MathJax's output reaches the note or the
 * page's DOM: this module walks MathJax's tree (a light DOM of its own, never the document's), keeps only geometry
 * (glyph outlines, rules, lines, frames, with their transforms and the clipping of stretched delimiters), and writes it
 * out itself. Whatever it does not understand (a character MathJax has no glyph for, a construct drawn otherwise) is an
 * error, and the equation cannot be placed: nothing is drawn approximately in its stead.
 */
import {
  EQUATION_MAX_COORDINATE,
  EQUATION_MAX_PATH_BYTES,
  EQUATION_MAX_SOURCE_CHARS,
  equationBox,
  readEquationPath,
  sourceLength,
  stepsBounds,
  writeEquationPath,
} from './equation-path';
import type { PathStep } from './text';

/** An element of MathJax's SVG output, as `lib/mathjax.ts` hands it over (copied out of MathJax's light DOM). */
export interface SvgNode {
  /** Lower-case tag name (`svg`, `g`, `path`…), or `#text`. */
  tag: string;
  attrs: Readonly<Record<string, string>>;
  children: readonly SvgNode[];
  /** The text of a `#text` node or of a `<text>` element. */
  text: string;
}

/** The drawing of an equation, ready for the note: path data normalised to its box, and that box (equation units). */
export interface EquationDrawing {
  path: string;
  width: number;
  height: number;
}

/** Why an equation cannot be placed. `message` is MathJax's own (English) for a LaTeX error. */
export type EquationProblem =
  | { kind: 'empty' }
  | { kind: 'tooLong'; max: number }
  | { kind: 'latex'; message: string }
  | { kind: 'nothingDrawn' }
  | { kind: 'missingGlyph'; chars: string }
  | { kind: 'unsupported'; what: string }
  | { kind: 'tooLarge' }
  | { kind: 'unavailable'; message: string };

export type EquationResult = { ok: true; drawing: EquationDrawing } | { ok: false; problem: EquationProblem };

/** Something the walk cannot draw: the conversion stops with this problem. */
class Refusal extends Error {
  constructor(readonly problem: EquationProblem) {
    super(problem.kind);
  }
}

// --- Affine maps --------------------------------------------------------------------------------

/** `x' = a·x + c·y + e`, `y' = b·x + d·y + f`, as SVG's `matrix(a b c d e f)`. */
type Matrix = readonly [number, number, number, number, number, number];
const IDENTITY: Matrix = [1, 0, 0, 1, 0, 0];

/** `m` after `n` (`n` applied first). */
function compose(m: Matrix, n: Matrix): Matrix {
  return [
    m[0] * n[0] + m[2] * n[1],
    m[1] * n[0] + m[3] * n[1],
    m[0] * n[2] + m[2] * n[3],
    m[1] * n[2] + m[3] * n[3],
    m[0] * n[4] + m[2] * n[5] + m[4],
    m[1] * n[4] + m[3] * n[5] + m[5],
  ];
}

function apply(m: Matrix, x: number, y: number): [number, number] {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
}

function mapSteps(m: Matrix, steps: readonly PathStep[]): PathStep[] {
  return steps.map((s): PathStep => {
    switch (s.op) {
      case 'M':
      case 'L': {
        const [x, y] = apply(m, s.x, s.y);
        return { op: s.op, x, y };
      }
      case 'Q': {
        const [cx, cy] = apply(m, s.cx, s.cy);
        const [x, y] = apply(m, s.x, s.y);
        return { op: 'Q', cx, cy, x, y };
      }
      case 'C': {
        const [c1x, c1y] = apply(m, s.c1x, s.c1y);
        const [c2x, c2y] = apply(m, s.c2x, s.c2y);
        const [x, y] = apply(m, s.x, s.y);
        return { op: 'C', c1x, c1y, c2x, c2y, x, y };
      }
      default:
        return s;
    }
  });
}

const NUMBER = /[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g;

function numbers(text: string): number[] {
  return (text.match(NUMBER) ?? []).map(Number);
}

/** An SVG transform list (`translate`, `scale`, `rotate`, `matrix`, `skewX`, `skewY`), or a refusal for anything else. */
export function parseTransform(text: string): Matrix {
  let m: Matrix = IDENTITY;
  const rest = text.replace(/\s*([a-zA-Z]+)\s*\(([^)]*)\)\s*,?/g, (_, name: string, args: string) => {
    const v = numbers(args);
    let t: Matrix | null = null;
    switch (name) {
      case 'translate':
        if (v.length === 1 || v.length === 2) t = [1, 0, 0, 1, v[0], v[1] ?? 0];
        break;
      case 'scale':
        if (v.length === 1 || v.length === 2) t = [v[0], 0, 0, v[1] ?? v[0], 0, 0];
        break;
      case 'rotate':
        if (v.length === 1 || v.length === 3) {
          const a = (v[0] * Math.PI) / 180;
          const [cos, sin] = [Math.cos(a), Math.sin(a)];
          const [cx, cy] = [v[1] ?? 0, v[2] ?? 0];
          t = [cos, sin, -sin, cos, cx - cos * cx + sin * cy, cy - sin * cx - cos * cy];
        }
        break;
      case 'matrix':
        if (v.length === 6) t = [v[0], v[1], v[2], v[3], v[4], v[5]];
        break;
      case 'skewX':
        if (v.length === 1) t = [1, 0, Math.tan((v[0] * Math.PI) / 180), 1, 0, 0];
        break;
      case 'skewY':
        if (v.length === 1) t = [1, Math.tan((v[0] * Math.PI) / 180), 0, 1, 0, 0];
        break;
    }
    if (!t || !t.every(Number.isFinite)) throw new Refusal({ kind: 'unsupported', what: `transform ${name}` });
    m = compose(m, t);
    return '';
  });
  if (rest.trim() !== '') throw new Refusal({ kind: 'unsupported', what: 'transform' });
  return m;
}

// --- SVG path data (MathJax's: any command but arcs) ---------------------------------------------

/** MathJax's path data as absolute steps; arcs (which it does not use for math) are refused. */
export function parseSvgPath(d: string): PathStep[] {
  const steps: PathStep[] = [];
  const tokens = d.match(/[a-zA-Z]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g) ?? [];
  let i = 0;
  let x = 0;
  let y = 0;
  let startX = 0;
  let startY = 0;
  // The last control point, for S and T.
  let lastCubic: [number, number] | null = null;
  let lastQuad: [number, number] | null = null;
  let command = '';
  const isNumber = (token: string | undefined) => token !== undefined && !/^[a-zA-Z]$/.test(token);
  const n = (): number => {
    const token = tokens[i++];
    const value = Number(token);
    if (!isNumber(token) || !Number.isFinite(value)) throw new Refusal({ kind: 'unsupported', what: 'path data' });
    return value;
  };
  while (i < tokens.length) {
    if (!isNumber(tokens[i])) command = tokens[i++];
    else if (command === '' || command === 'Z' || command === 'z') throw new Refusal({ kind: 'unsupported', what: 'path data' });
    // A drawing command right after a close starts a new contour where the closed one began.
    if (steps[steps.length - 1]?.op === 'Z' && command.toUpperCase() !== 'M' && command.toUpperCase() !== 'Z') {
      steps.push({ op: 'M', x: startX, y: startY });
    }
    const relative = command === command.toLowerCase();
    const ox = relative ? x : 0;
    const oy = relative ? y : 0;
    let cubic: [number, number] | null = null;
    let quad: [number, number] | null = null;
    switch (command.toUpperCase()) {
      case 'M':
        x = ox + n();
        y = oy + n();
        startX = x;
        startY = y;
        steps.push({ op: 'M', x, y });
        // Pairs after the first are lines.
        command = relative ? 'l' : 'L';
        break;
      case 'L':
        x = ox + n();
        y = oy + n();
        steps.push({ op: 'L', x, y });
        break;
      case 'H':
        x = ox + n();
        steps.push({ op: 'L', x, y });
        break;
      case 'V':
        y = oy + n();
        steps.push({ op: 'L', x, y });
        break;
      case 'C': {
        const c1x = ox + n();
        const c1y = oy + n();
        const c2x = ox + n();
        const c2y = oy + n();
        x = ox + n();
        y = oy + n();
        steps.push({ op: 'C', c1x, c1y, c2x, c2y, x, y });
        cubic = [c2x, c2y];
        break;
      }
      case 'S': {
        const [c1x, c1y] = lastCubic ? [2 * x - lastCubic[0], 2 * y - lastCubic[1]] : [x, y];
        const c2x = ox + n();
        const c2y = oy + n();
        x = ox + n();
        y = oy + n();
        steps.push({ op: 'C', c1x, c1y, c2x, c2y, x, y });
        cubic = [c2x, c2y];
        break;
      }
      case 'Q': {
        const cx = ox + n();
        const cy = oy + n();
        x = ox + n();
        y = oy + n();
        steps.push({ op: 'Q', cx, cy, x, y });
        quad = [cx, cy];
        break;
      }
      case 'T': {
        const reflected: [number, number] = lastQuad ? [2 * x - lastQuad[0], 2 * y - lastQuad[1]] : [x, y];
        x = ox + n();
        y = oy + n();
        steps.push({ op: 'Q', cx: reflected[0], cy: reflected[1], x, y });
        quad = reflected;
        break;
      }
      case 'Z':
        steps.push({ op: 'Z' });
        x = startX;
        y = startY;
        break;
      default:
        throw new Refusal({ kind: 'unsupported', what: `path command ${command}` });
    }
    lastCubic = cubic;
    lastQuad = quad;
  }
  return steps;
}

// --- Shapes ---------------------------------------------------------------------------------------

/** A rectangle as one contour; `reverse`: the other way round (a hole, for the non-zero rule). */
function rectangle(x: number, y: number, w: number, h: number, reverse = false): PathStep[] {
  const corners: [number, number][] = [
    [x, y],
    [x + w, y],
    [x + w, y + h],
    [x, y + h],
  ];
  if (reverse) corners.reverse();
  return [
    { op: 'M', x: corners[0][0], y: corners[0][1] },
    ...corners.slice(1).map(([px, py]): PathStep => ({ op: 'L', x: px, y: py })),
    { op: 'Z' },
  ];
}

/** Four cubic arcs (the usual approximation, within 0.03 % of the radius). */
function ellipse(cx: number, cy: number, rx: number, ry: number, reverse = false): PathStep[] {
  const k = 0.5522847498;
  const quarter = (sx: number, sy: number, ex: number, ey: number, c1: [number, number], c2: [number, number]): PathStep => ({
    op: 'C',
    c1x: c1[0],
    c1y: c1[1],
    c2x: c2[0],
    c2y: c2[1],
    x: ex,
    y: ey,
  });
  const right: [number, number] = [cx + rx, cy];
  const bottom: [number, number] = [cx, cy + ry];
  const left: [number, number] = [cx - rx, cy];
  const top: [number, number] = [cx, cy - ry];
  if (!reverse) {
    return [
      { op: 'M', x: right[0], y: right[1] },
      quarter(...right, ...bottom, [cx + rx, cy + k * ry], [cx + k * rx, cy + ry]),
      quarter(...bottom, ...left, [cx - k * rx, cy + ry], [cx - rx, cy + k * ry]),
      quarter(...left, ...top, [cx - rx, cy - k * ry], [cx - k * rx, cy - ry]),
      quarter(...top, ...right, [cx + k * rx, cy - ry], [cx + rx, cy - k * ry]),
      { op: 'Z' },
    ];
  }
  return [
    { op: 'M', x: right[0], y: right[1] },
    quarter(...right, ...top, [cx + rx, cy - k * ry], [cx + k * rx, cy - ry]),
    quarter(...top, ...left, [cx - k * rx, cy - ry], [cx - rx, cy - k * ry]),
    quarter(...left, ...bottom, [cx - rx, cy + k * ry], [cx - k * rx, cy + ry]),
    quarter(...bottom, ...right, [cx + k * rx, cy + ry], [cx + rx, cy + k * ry]),
    { op: 'Z' },
  ];
}

/** A stroked line of width `w`, as the rectangles of its dashes (`dashes`: on and off lengths; none: solid, butt caps). */
function strokedLine(x1: number, y1: number, x2: number, y2: number, w: number, dashes: readonly number[], round: boolean): PathStep[] {
  const length = Math.hypot(x2 - x1, y2 - y1);
  if (!(w > 0) || !(length > 0)) return [];
  const [ux, uy] = [(x2 - x1) / length, (y2 - y1) / length];
  const [nx, ny] = [(-uy * w) / 2, (ux * w) / 2];
  const piece = (from: number, to: number): PathStep[] => {
    const [ax, ay] = [x1 + ux * from, y1 + uy * from];
    const [bx, by] = [x1 + ux * to, y1 + uy * to];
    return [
      { op: 'M', x: ax + nx, y: ay + ny },
      { op: 'L', x: bx + nx, y: by + ny },
      { op: 'L', x: bx - nx, y: by - ny },
      { op: 'L', x: ax - nx, y: ay - ny },
      { op: 'Z' },
    ];
  };
  const on = dashes.filter((v) => Number.isFinite(v) && v >= 0);
  if (on.length === 0 || on.every((v) => v === 0)) return piece(0, length);
  // SVG repeats an odd list to make it even.
  const pattern = on.length % 2 === 1 ? [...on, ...on] : on;
  const out: PathStep[] = [];
  let at = 0;
  for (let k = 0; at < length && out.length < 20_000; k++) {
    const dash = pattern[k % pattern.length];
    if (k % 2 === 0) {
      if (dash > 0) out.push(...piece(at, Math.min(at + dash, length)));
      // A dash of length 0 with round caps is a dot.
      else if (round) out.push(...ellipse(x1 + ux * at, y1 + uy * at, w / 2, w / 2));
    }
    at += Math.max(dash, k % 2 === 1 ? 1 : 0);
  }
  return out;
}

// --- Clipping (the extenders of stretched delimiters live in a nested <svg> that clips them) ----

interface ClipRect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** The contours of `steps` (each from its `M`). */
function contours(steps: readonly PathStep[]): PathStep[][] {
  const out: PathStep[][] = [];
  for (const s of steps) {
    if (s.op === 'M' || out.length === 0) out.push([]);
    out[out.length - 1].push(s);
  }
  return out;
}

/** A contour as a polygon, its curves cut into short lines. */
function flatten(contour: readonly PathStep[]): [number, number][] {
  const points: [number, number][] = [];
  let [x, y] = [0, 0];
  const PIECES = 12;
  for (const s of contour) {
    if (s.op === 'M' || s.op === 'L') {
      [x, y] = [s.x, s.y];
      points.push([x, y]);
    } else if (s.op === 'Q') {
      for (let k = 1; k <= PIECES; k++) {
        const t = k / PIECES;
        const u = 1 - t;
        points.push([u * u * x + 2 * u * t * s.cx + t * t * s.x, u * u * y + 2 * u * t * s.cy + t * t * s.y]);
      }
      [x, y] = [s.x, s.y];
    } else if (s.op === 'C') {
      for (let k = 1; k <= PIECES; k++) {
        const t = k / PIECES;
        const u = 1 - t;
        points.push([
          u * u * u * x + 3 * u * u * t * s.c1x + 3 * u * t * t * s.c2x + t * t * t * s.x,
          u * u * u * y + 3 * u * u * t * s.c1y + 3 * u * t * t * s.c2y + t * t * t * s.y,
        ]);
      }
      [x, y] = [s.x, s.y];
    }
  }
  return points;
}

/** Sutherland–Hodgman against the rectangle (convex: exact for any polygon under the non-zero rule but slivers on its edges). */
function clipPolygon(points: readonly [number, number][], r: ClipRect): [number, number][] {
  const edges: ((p: [number, number]) => number)[] = [(p) => p[0] - r.x0, (p) => r.x1 - p[0], (p) => p[1] - r.y0, (p) => r.y1 - p[1]];
  let polygon = [...points];
  for (const inside of edges) {
    if (polygon.length === 0) break;
    const next: [number, number][] = [];
    for (let k = 0; k < polygon.length; k++) {
      const a = polygon[k];
      const b = polygon[(k + 1) % polygon.length];
      const [da, db] = [inside(a), inside(b)];
      if (da >= 0) next.push(a);
      if ((da >= 0) !== (db >= 0)) {
        const t = da / (da - db);
        next.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
      }
    }
    polygon = next;
  }
  return polygon;
}

function clip(steps: readonly PathStep[], r: ClipRect): PathStep[] {
  const out: PathStep[] = [];
  for (const contour of contours(steps)) {
    const b = stepsBounds(contour);
    if (!b || b.maxX <= r.x0 || b.minX >= r.x1 || b.maxY <= r.y0 || b.minY >= r.y1) continue;
    if (b.minX >= r.x0 && b.maxX <= r.x1 && b.minY >= r.y0 && b.maxY <= r.y1) {
      // Wholly inside: its curves are kept.
      out.push(...contour);
      if (contour[contour.length - 1]?.op !== 'Z') out.push({ op: 'Z' });
      continue;
    }
    const polygon = clipPolygon(flatten(contour), r);
    if (polygon.length < 3) continue;
    out.push({ op: 'M', x: polygon[0][0], y: polygon[0][1] });
    for (const [x, y] of polygon.slice(1)) out.push({ op: 'L', x, y });
    out.push({ op: 'Z' });
  }
  return out;
}

// --- The walk -----------------------------------------------------------------------------------

const SKIPPED = new Set(['#text', '#comment', 'title', 'desc', 'defs', 'style', 'metadata']);

function attribute(node: SvgNode, name: string): number | null {
  const raw = node.attrs[name];
  if (raw === undefined) return null;
  const value = Number.parseFloat(raw);
  return Number.isFinite(value) ? value : null;
}

/** A width given by an attribute or the element's own style (`stroke-width: 67`). */
function strokeWidth(node: SvgNode): number | null {
  const own = attribute(node, 'stroke-width') ?? attribute(node, 'stroke-thickness');
  if (own !== null) return own;
  const style = /stroke-width\s*:\s*([-+.\d]+)/.exec(node.attrs.style ?? '');
  return style ? Number.parseFloat(style[1]) : null;
}

function hasClass(node: SvgNode, name: string): boolean {
  return (node.attrs.class ?? '').split(/\s+/).includes(name);
}

/** Whether the element is painted without fill (`fill="none"`, or in its style). */
function unfilled(node: SvgNode): boolean {
  return node.attrs.fill === 'none' || /(?:^|;)\s*fill\s*:\s*none/.test(node.attrs.style ?? '');
}

/** The width MathJax gives table lines in its style sheet (`stroke-width: 70px`). */
const TABLE_LINE_WIDTH = 70;
/** The dash and the dot spacing of `mjx-dashed` and `mjx-dotted` table lines. */
const TABLE_DASH = 140;

/** Walks `node` with the map `m` to the root's coordinates, adding what it paints to `out`. */
function walk(node: SvgNode, m: Matrix, out: PathStep[], missing: Set<string>): void {
  if (SKIPPED.has(node.tag)) return;
  if (node.attrs.display === 'none' || node.attrs.visibility === 'hidden') return;
  const own = node.attrs.transform ? compose(m, parseTransform(node.attrs.transform)) : m;
  switch (node.tag) {
    case 'g':
      for (const child of node.children) walk(child, own, out, missing);
      return;
    case 'svg':
      return nestedSvg(node, own, out, missing);
    case 'path': {
      if (unfilled(node)) throw new Refusal({ kind: 'unsupported', what: 'stroked path' });
      out.push(...mapSteps(own, parseSvgPath(node.attrs.d ?? '')));
      return;
    }
    case 'rect': {
      const [x, y] = [attribute(node, 'x') ?? 0, attribute(node, 'y') ?? 0];
      const [w, h] = [attribute(node, 'width') ?? 0, attribute(node, 'height') ?? 0];
      if (!(w > 0) || !(h > 0)) return;
      if (!unfilled(node)) {
        out.push(...mapSteps(own, rectangle(x, y, w, h)));
        return;
      }
      // A frame (`\boxed`): its stroke, centred on the edges, as a ring.
      const t = strokeWidth(node) ?? 0;
      if (!(t > 0)) return;
      out.push(...mapSteps(own, rectangle(x - t / 2, y - t / 2, w + t, h + t)));
      if (w > t && h > t) out.push(...mapSteps(own, rectangle(x + t / 2, y + t / 2, w - t, h - t, true)));
      return;
    }
    case 'line': {
      const t = strokeWidth(node) ?? TABLE_LINE_WIDTH;
      const listed = (node.attrs['stroke-dasharray'] ?? '').split(/[\s,]+/).filter(Boolean).map(Number);
      const dotted = hasClass(node, 'mjx-dotted');
      const dashes = listed.length > 0 ? listed : dotted ? [0, TABLE_DASH] : hasClass(node, 'mjx-dashed') ? [TABLE_DASH] : [];
      const [x1, y1, x2, y2] = ['x1', 'y1', 'x2', 'y2'].map((k) => attribute(node, k) ?? 0);
      out.push(...mapSteps(own, strokedLine(x1, y1, x2, y2, t, dashes, dotted || node.attrs['stroke-linecap'] === 'round')));
      return;
    }
    case 'ellipse':
    case 'circle': {
      const [cx, cy] = [attribute(node, 'cx') ?? 0, attribute(node, 'cy') ?? 0];
      const r = attribute(node, 'r');
      const [rx, ry] = node.tag === 'circle' ? [r ?? 0, r ?? 0] : [attribute(node, 'rx') ?? 0, attribute(node, 'ry') ?? 0];
      if (!(rx > 0) || !(ry > 0)) return;
      if (!unfilled(node)) {
        out.push(...mapSteps(own, ellipse(cx, cy, rx, ry)));
        return;
      }
      const t = strokeWidth(node) ?? 0;
      if (!(t > 0)) return;
      out.push(...mapSteps(own, ellipse(cx, cy, rx + t / 2, ry + t / 2)));
      if (rx > t / 2 && ry > t / 2) out.push(...mapSteps(own, ellipse(cx, cy, rx - t / 2, ry - t / 2, true)));
      return;
    }
    case 'text':
      // A character MathJax's fonts (or the part of them Ardoise carries) have no glyph for: it would draw it with a
      // system font, which no export has.
      for (const c of node.text || collectText(node)) if (c.trim() !== '') missing.add(c);
      return;
    default:
      throw new Refusal({ kind: 'unsupported', what: node.tag });
  }
}

function collectText(node: SvgNode): string {
  return node.tag === '#text' ? node.text : node.children.map(collectText).join('');
}

/** A nested `<svg>` (an extender of a stretched delimiter): its viewport, its view box, and the clipping to it. */
function nestedSvg(node: SvgNode, m: Matrix, out: PathStep[], missing: Set<string>): void {
  // A numbered equation (`\tag`) is laid out against the width of a page of text, which a drawing has not.
  if ('data-labels' in node.attrs || 'data-table' in node.attrs) throw new Refusal({ kind: 'unsupported', what: 'tag' });
  const [x, y] = [attribute(node, 'x') ?? 0, attribute(node, 'y') ?? 0];
  const width = attribute(node, 'width');
  const height = attribute(node, 'height');
  const box = numbers(node.attrs.viewBox ?? '');
  let inner: Matrix = [1, 0, 0, 1, x, y];
  if (box.length === 4 && width !== null && height !== null && box[2] > 0 && box[3] > 0) {
    // preserveAspectRatio's default, xMidYMid meet.
    const s = Math.min(width / box[2], height / box[3]);
    inner = [s, 0, 0, s, x + (width - box[2] * s) / 2 - box[0] * s, y + (height - box[3] * s) / 2 - box[1] * s];
  }
  const content: PathStep[] = [];
  for (const child of node.children) walk(child, compose(m, inner), content, missing);
  const clips = node.attrs.overflow !== 'visible' && width !== null && height !== null && width > 0 && height > 0;
  // The viewport in the root's coordinates, when it stays an upright rectangle there.
  if (clips && Math.abs(m[1]) < 1e-9 && Math.abs(m[2]) < 1e-9) {
    const [ax, ay] = apply(m, x, y);
    const [bx, by] = apply(m, x + width, y + height);
    out.push(...clip(content, { x0: Math.min(ax, bx), y0: Math.min(ay, by), x1: Math.max(ax, bx), y1: Math.max(ay, by) }));
  } else {
    out.push(...content);
  }
}

/**
 * The drawing of MathJax's output `svg` (the `<svg>` element it makes for one formula): every painted element in the
 * root's coordinates (MathJax's units, 1/1000 em, y down), moved so that its box starts at (0, 0), rounded to a tenth,
 * written as path data. A refusal says why it cannot be placed.
 */
export function drawingOfSvg(svg: SvgNode): EquationResult {
  try {
    if (svg.attrs.width === '100%') throw new Refusal({ kind: 'unsupported', what: 'tag' });
    const steps: PathStep[] = [];
    const missing = new Set<string>();
    for (const child of svg.children) walk(child, IDENTITY, steps, missing);
    if (missing.size > 0) return { ok: false, problem: { kind: 'missingGlyph', chars: [...missing].join(' ') } };
    const bounds = stepsBounds(steps);
    if (!bounds) return { ok: false, problem: { kind: 'nothingDrawn' } };
    const moved = mapSteps([1, 0, 0, 1, -bounds.minX, -bounds.minY], steps);
    // Each contour closed (the fill closes them anyway; Rust takes a new `M` only after a `Z` or another `M`).
    const closed: PathStep[] = [];
    for (const contour of contours(moved)) {
      const drawn = contour.filter((s) => s.op !== 'Z');
      // A contour is an `M` and what it draws (a lone `M` draws nothing).
      if (drawn[0]?.op !== 'M' || drawn.length < 2) continue;
      closed.push(...drawn, { op: 'Z' });
    }
    const path = writeEquationPath(closed);
    if (path.length > EQUATION_MAX_PATH_BYTES) return { ok: false, problem: { kind: 'tooLarge' } };
    // Read back as every export will: the box is measured on the rounded data.
    const read = readEquationPath(path);
    if (!read) return { ok: false, problem: { kind: 'tooLarge' } };
    const box = equationBox(read);
    if (box.width > EQUATION_MAX_COORDINATE || box.height > EQUATION_MAX_COORDINATE) return { ok: false, problem: { kind: 'tooLarge' } };
    return { ok: true, drawing: { path, ...box } };
  } catch (error) {
    if (error instanceof Refusal) return { ok: false, problem: error.problem };
    throw error;
  }
}

// --- LaTeX to drawing ----------------------------------------------------------------------------

/** What `lib/mathjax.ts` gives: the `<svg>` of a formula, or MathJax's error message. */
export type TexOutput = { ok: true; svg: SvgNode } | { ok: false; message: string };

/** Converts LaTeX with MathJax; replaced in tests. */
export type TexConverter = (latex: string) => Promise<TexOutput>;

let loading: Promise<TexConverter> | null = null;

/** MathJax, loaded once, the first time the equation tool converts something. */
export function loadMathJax(): Promise<TexConverter> {
  loading ??= import('./mathjax').then((module) => module.texToSvg).catch((error: unknown) => {
    loading = null;
    throw error;
  });
  return loading;
}

/** Conversions run one at a time (MathJax's document is shared); a newer one does not wait behind an older one twice. */
let queue: Promise<unknown> = Promise.resolve();

/** The drawing of `latex`, or why there is none. `convert`: MathJax by default. */
export async function convertLatex(latex: string, convert?: TexConverter): Promise<EquationResult> {
  if (latex.trim() === '') return { ok: false, problem: { kind: 'empty' } };
  if (sourceLength(latex) > EQUATION_MAX_SOURCE_CHARS) return { ok: false, problem: { kind: 'tooLong', max: EQUATION_MAX_SOURCE_CHARS } };
  const run = async (): Promise<EquationResult> => {
    let converter: TexConverter;
    try {
      converter = convert ?? (await loadMathJax());
    } catch (error) {
      return { ok: false, problem: { kind: 'unavailable', message: error instanceof Error ? error.message : String(error) } };
    }
    let output: TexOutput;
    try {
      output = await converter(latex);
    } catch (error) {
      return { ok: false, problem: { kind: 'latex', message: error instanceof Error ? error.message : String(error) } };
    }
    return output.ok ? drawingOfSvg(output.svg) : { ok: false, problem: { kind: 'latex', message: output.message } };
  };
  const result = queue.then(run, run);
  queue = result.catch(() => undefined);
  return result;
}
