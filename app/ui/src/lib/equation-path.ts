/**
 * The stored drawing of an equation (page objects of kind `equation`, format 1.5, `docs/design/equations.md`): SVG
 * path data of absolute `M`, `L`, `Q`, `C` and `Z` commands, in the equation's units (1/1000 em, y down, origin at the
 * top-left corner of its box), filled with the non-zero rule. Rust reads the same data with the same rules
 * (`ardoise_text::Path::from_svg`, the limits of `ardoise_core::objects`): what this module accepts, every export draws.
 *
 * Pure and free of MathJax: the page, the live renderer and the share's viewers read stored drawings with it; only the
 * editor's equation tool makes new ones (`lib/equation.ts`).
 */
import type { PathStep } from './text';

/** Longest LaTeX source, in characters (code points): `EQUATION_MAX_SOURCE_CHARS` in Rust. */
export const EQUATION_MAX_SOURCE_CHARS = 4000;
/** Largest path data, bytes (ASCII: characters): `EQUATION_MAX_PATH_BYTES` in Rust. */
export const EQUATION_MAX_PATH_BYTES = 256 * 1024;
/** Largest coordinate, in the equation's units: `EQUATION_MAX_COORDINATE` in Rust. */
export const EQUATION_MAX_COORDINATE = 1e6;
/** Units of the drawing per em of its type: the placement's scale is the em on the page divided by this. */
export const EQUATION_UNITS_PER_EM = 1000;

/** How many characters (code points) `text` has, as Rust counts them. */
export function sourceLength(text: string): number {
  let count = 0;
  for (const _ of text) count++;
  return count;
}

const isSpace = (c: string) => c === ' ' || c === '\t' || c === '\n' || c === '\r';
const isDigit = (c: string) => c >= '0' && c <= '9';

/**
 * The steps of stored path data, or null when Rust would not read it (`Path::from_svg`): at most
 * `EQUATION_MAX_PATH_BYTES`, the first command an `M`, nothing but an `M` after a `Z`, numbers without exponent within
 * ±`EQUATION_MAX_COORDINATE`; a command may be followed by more groups of numbers, as SVG allows.
 */
export function readEquationPath(d: string): PathStep[] | null {
  if (d.length > EQUATION_MAX_PATH_BYTES) return null;
  // ASCII only (a byte per character, as Rust reads it).
  if (/[^\x20-\x7e\t\n\r]/.test(d)) return null;
  let at = 0;
  const skip = (commas: boolean) => {
    while (at < d.length && (isSpace(d[at]) || (commas && d[at] === ','))) at++;
  };
  const numberAhead = () => at < d.length && (d[at] === '-' || d[at] === '+' || d[at] === '.' || isDigit(d[at]));
  const number = (): number | null => {
    skip(true);
    const start = at;
    if (d[at] === '-' || d[at] === '+') at++;
    let digits = 0;
    while (at < d.length && isDigit(d[at])) {
      at++;
      digits++;
    }
    if (d[at] === '.') {
      at++;
      while (at < d.length && isDigit(d[at])) {
        at++;
        digits++;
      }
    }
    if (digits === 0) return null;
    const value = Number(d.slice(start, at));
    return Number.isFinite(value) && Math.abs(value) <= EQUATION_MAX_COORDINATE ? value : null;
  };
  const steps: PathStep[] = [];
  let command: string | null = null;
  for (;;) {
    skip(false);
    if (at >= d.length) break;
    const c = d[at];
    let current: string;
    if (/[A-Za-z]/.test(c)) {
      at++;
      if ((command === null || command === 'Z') && c !== 'M') return null;
      current = c;
    } else if (command !== null && command !== 'Z' && numberAhead()) {
      current = command === 'M' ? 'L' : command;
    } else {
      return null;
    }
    const n = (): number => {
      const value = number();
      if (value === null) throw new SyntaxError('number');
      return value;
    };
    try {
      if (current === 'M' || current === 'L') steps.push({ op: current, x: n(), y: n() });
      else if (current === 'Q') steps.push({ op: 'Q', cx: n(), cy: n(), x: n(), y: n() });
      else if (current === 'C') steps.push({ op: 'C', c1x: n(), c1y: n(), c2x: n(), c2y: n(), x: n(), y: n() });
      else if (current === 'Z') steps.push({ op: 'Z' });
      else return null;
    } catch {
      return null;
    }
    command = current;
    skip(true);
  }
  return steps;
}

/** The box of every point of `steps`, control points included (Rust's `Path::bounds`); null when there is none. */
export function stepsBounds(steps: readonly PathStep[]): { minX: number; minY: number; maxX: number; maxY: number } | null {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  const add = (x: number, y: number) => {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  };
  for (const s of steps) {
    if (s.op === 'M' || s.op === 'L') add(s.x, s.y);
    else if (s.op === 'Q') {
      add(s.cx, s.cy);
      add(s.x, s.y);
    } else if (s.op === 'C') {
      add(s.c1x, s.c1y);
      add(s.c2x, s.c2y);
      add(s.x, s.y);
    }
  }
  return minX === Infinity ? null : { minX, minY, maxX, maxY };
}

/** The box an equation's drawing gives it (`PageObject::measure` in Rust): from its origin to its farthest point. */
export function equationBox(steps: readonly PathStep[]): { width: number; height: number } {
  const b = stepsBounds(steps);
  return b ? { width: Math.max(0, b.maxX), height: Math.max(0, b.maxY) } : { width: 0, height: 0 };
}

/** `value` rounded to a tenth, written without exponent nor trailing zeros (`12`, `-3.5`, never `-0`). */
export function tenth(value: number): string {
  const rounded = Math.round(value * 10) / 10;
  return Object.is(rounded, -0) || rounded === 0 ? '0' : String(rounded);
}

/** The steps as stored path data: absolute commands, numbers to a tenth. */
export function writeEquationPath(steps: readonly PathStep[]): string {
  const out: string[] = [];
  for (const s of steps) {
    switch (s.op) {
      case 'M':
      case 'L':
        out.push(`${s.op}${tenth(s.x)} ${tenth(s.y)}`);
        break;
      case 'Q':
        out.push(`Q${tenth(s.cx)} ${tenth(s.cy)} ${tenth(s.x)} ${tenth(s.y)}`);
        break;
      case 'C':
        out.push(`C${tenth(s.c1x)} ${tenth(s.c1y)} ${tenth(s.c2x)} ${tenth(s.c2y)} ${tenth(s.x)} ${tenth(s.y)}`);
        break;
      case 'Z':
        out.push('Z');
        break;
    }
  }
  return out.join('');
}
