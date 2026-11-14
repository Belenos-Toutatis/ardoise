/**
 * Text frames (SPEC §4.4 « Texte tapé au clavier », `docs/design/page-objects.md`) laid out and drawn by
 * `ardoise-text` compiled to WebAssembly, in the module of the ink (`lib/ink.ts`, `crates/ardoise-ink-wasm/src/text.rs`):
 * the screen draws exactly the glyphs of the exports, from the fonts shipped with the app, never from the PC's.
 *
 * The rules of the layout (`crates/ardoise-text/src/layout.rs`): a line break starts a new line, and a frame with a wrap
 * width (`wrap`, format 1.7) wraps between words, its white space hanging at the end of the line; glyphs follow each other
 * by their advance widths, without kerning nor ligatures; a character missing from the family comes from Latin Modern
 * Math, then from the other family (Andika never for Greek); one no face has is a white square (`missing`); lines are
 * 1.25 × size apart. The editor's text field (`TextField.svelte`) is styled the same way (`textFieldStyle`), so that its
 * caret falls between the glyphs drawn, and shows the lines as they wrap (`lib/text-field.ts`).
 */
import type { FontFamily } from './note';

/**
 * What a frame's text looks like: one style for the whole frame. `size`: the font's em, in the frame's units; `wrap`: the
 * width its lines wrap at, in the same units (absent, null or not positive: only line breaks start a line).
 */
export interface TextStyle {
  font: FontFamily;
  bold: boolean;
  size: number;
  wrap?: number | null;
}

export interface TextGlyph {
  /** Pen position from the left edge of the frame. */
  x: number;
  advance: number;
  /** Offset of its character in the text, in UTF-16 code units (a text field's positions). */
  index: number;
  /** No face has the character: a white square stands for it (`MISSING_CHAR`). */
  missing: boolean;
}

export interface TextLine {
  top: number;
  baseline: number;
  /** Advance width of the line. */
  width: number;
  /** UTF-16 offsets of its first character and of its end (its line break excluded; wrapped: where the next line starts). */
  start: number;
  end: number;
  /** The line ends where the text wrapped (the next line goes on with the same paragraph), not at a line break. */
  wrapped: boolean;
  glyphs: TextGlyph[];
}

/** A text laid out, in the frame's units, origin at its top-left corner, y down. */
export interface TextLayout {
  width: number;
  height: number;
  lineHeight: number;
  /** The family's ascent and descent, both positive, at the text's size. */
  ascent: number;
  descent: number;
  lines: TextLine[];
}

export interface TextEngine {
  layout(text: string, style: TextStyle): TextLayout;
  /** SVG path data of the glyphs, in the frame's units (origin top-left, y down), to fill with the non-zero rule. */
  path(text: string, style: TextStyle): string;
  /** The bytes of a font file shipped with the app (`FONT_FACES`' `code`), to style the text field with it. */
  fontBytes(face: number): Uint8Array;
}

/** Distance between two lines, in units of the size. */
export const LINE_HEIGHT = 1.25;
/** A tab goes to the next multiple of this many spaces. */
export const TAB_SPACES = 4;

/** The three sizes of the text tool, in device units (the font's em): 4, 6 and 9 mm (`TEXT_SIZES` in Rust). */
export const TEXT_SIZES = { small: 400, medium: 600, large: 900 } as const;
export type TextSize = keyof typeof TEXT_SIZES;
export const TEXT_SIZE_NAMES: readonly TextSize[] = ['small', 'medium', 'large'];

/** The code of a family in the WebAssembly interface (`FontFamily::code`). */
export const FONT_CODE: Readonly<Record<FontFamily, number>> = { latinModern: 0, andika: 1 };

/** The CSS family names the fonts are registered under (`registerTextFonts`). */
export const CSS_FAMILY: Readonly<Record<FontFamily, string>> = {
  latinModern: 'Ardoise Latin Modern',
  andika: 'Ardoise Andika',
};

/** The CSS family of Latin Modern Math, the face both families take Greek and the mathematical symbols from. */
export const CSS_MATH_FAMILY = 'Ardoise Latin Modern Math';

/** What stands for a character no face has (`MISSING_CHAR` in Rust): a white square, from Latin Modern Math. */
export const MISSING_CHAR = '\u25A1';

/**
 * Andika is not used for the Greek and Coptic block (its Greek is partial: `FontFace::serves` in Rust): its files are
 * registered for every other character, so that the browser takes Greek from Latin Modern Math, as the layout does.
 */
export const ANDIKA_UNICODE_RANGE = 'U+0-36F, U+400-10FFFF';

/** The five font files (`FontFace` in Rust, in its order: `code`), as the text field registers them. */
export const FONT_FACES: readonly { code: number; family: string; weight: 400 | 700; unicodeRange?: string }[] = [
  { code: 0, family: CSS_FAMILY.latinModern, weight: 400 },
  { code: 1, family: CSS_FAMILY.latinModern, weight: 700 },
  { code: 2, family: CSS_FAMILY.andika, weight: 400, unicodeRange: ANDIKA_UNICODE_RANGE },
  { code: 3, family: CSS_FAMILY.andika, weight: 700, unicodeRange: ANDIKA_UNICODE_RANGE },
  // One weight: a bold field takes it as it is (`font-synthesis: none`), with the regular advances the layout uses.
  { code: 4, family: CSS_MATH_FAMILY, weight: 400 },
];

/** The wrap width a style lays out with: a positive number, or 0 for none (the module's convention). */
export function wrapOf(style: Pick<TextStyle, 'wrap'>): number {
  const wrap = style.wrap;
  return typeof wrap === 'number' && Number.isFinite(wrap) && wrap > 0 ? wrap : 0;
}

/** The exports of the module that `textEngine` uses. */
export interface TextExports {
  memory: WebAssembly.Memory;
  text_alloc(bytes: number): number;
  text_free(ptr: number, bytes: number): void;
  text_layout(ptr: number, len: number, family: number, bold: number, size: number, wrap: number): number;
  text_layout_result(): number;
  text_path(ptr: number, len: number, family: number, bold: number, size: number, wrap: number): number;
  text_path_result(): number;
  text_font(face: number): number;
  text_font_len(face: number): number;
}

/** Results kept per text and style (frames are drawn again at each change of the page). */
const CACHE_SIZE = 400;

/** The engine over the module's exports, with a cache of the latest layouts and paths. */
export function textEngine(wasm: TextExports): TextEngine {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  const layouts = new Map<string, TextLayout>();
  const paths = new Map<string, string>();

  function keyOf(text: string, style: TextStyle): string {
    return `${style.font}|${style.bold ? 1 : 0}|${style.size}|${wrapOf(style)}|${text}`;
  }

  function remember<T>(cache: Map<string, T>, key: string, value: T): T {
    if (cache.size >= CACHE_SIZE) {
      const oldest = cache.keys().next().value;
      if (oldest !== undefined) cache.delete(oldest);
    }
    cache.set(key, value);
    return value;
  }

  /** Runs `call` with the text written in the module's memory. */
  function withText<T>(text: string, call: (ptr: number, len: number) => T): T {
    const bytes = encoder.encode(text);
    const ptr = bytes.length > 0 ? wasm.text_alloc(bytes.length) : 0;
    try {
      if (bytes.length > 0) new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
      return call(ptr, bytes.length);
    } finally {
      if (bytes.length > 0) wasm.text_free(ptr, bytes.length);
    }
  }

  function layout(text: string, style: TextStyle): TextLayout {
    const key = keyOf(text, style);
    const known = layouts.get(key);
    if (known) return known;
    const flat = withText(text, (ptr, len) => {
      const count = wasm.text_layout(ptr, len, FONT_CODE[style.font] ?? 0, style.bold ? 1 : 0, style.size, wrapOf(style));
      return new Float32Array(wasm.memory.buffer, wasm.text_layout_result(), count).slice();
    });
    let at = 0;
    const next = () => flat[at++];
    const [width, height, lineHeight, ascent, descent] = [next(), next(), next(), next(), next()];
    const lines: TextLine[] = [];
    for (let l = next(); l > 0; l--) {
      const [top, baseline, lineWidth, start, end, wrapped] = [next(), next(), next(), next(), next(), next()];
      const glyphs: TextGlyph[] = [];
      for (let g = next(); g > 0; g--) glyphs.push({ x: next(), advance: next(), index: next(), missing: next() === 1 });
      lines.push({ top, baseline, width: lineWidth, start, end, wrapped: wrapped === 1, glyphs });
    }
    return remember(layouts, key, { width, height, lineHeight, ascent, descent, lines });
  }

  function path(text: string, style: TextStyle): string {
    const key = keyOf(text, style);
    const known = paths.get(key);
    if (known !== undefined) return known;
    const d = withText(text, (ptr, len) => {
      const count = wasm.text_path(ptr, len, FONT_CODE[style.font] ?? 0, style.bold ? 1 : 0, style.size, wrapOf(style));
      return decoder.decode(new Uint8Array(wasm.memory.buffer, wasm.text_path_result(), count));
    });
    return remember(paths, key, d);
  }

  function fontBytes(face: number): Uint8Array {
    const len = wasm.text_font_len(face);
    return new Uint8Array(wasm.memory.buffer, wasm.text_font(face), len).slice();
  }

  return { layout, path, fontBytes };
}

/**
 * The UTF-16 offset nearest to `(x, y)` (frame units): the line under `y` (the first or the last beyond them), the gap
 * between two glyphs nearest to `x` on it (`Layout::offset_at` in Rust).
 */
export function offsetAt(layout: TextLayout, x: number, y: number): number {
  const last = layout.lines.length - 1;
  if (last < 0) return 0;
  const row = layout.lineHeight > 0 ? Math.min(last, Math.max(0, Math.floor(y / layout.lineHeight))) : 0;
  const line = layout.lines[row];
  for (const glyph of line.glyphs) if (x < glyph.x + glyph.advance / 2) return glyph.index;
  return line.end;
}

/** Where the caret at `offset` stands (frame units): the line, its x, and the line's top. */
export function caretAt(layout: TextLayout, offset: number): { line: number; x: number; top: number } {
  for (let l = 0; l < layout.lines.length; l++) {
    const line = layout.lines[l];
    const isLast = l === layout.lines.length - 1;
    if (offset <= line.end || isLast) {
      let x = line.width;
      for (const glyph of line.glyphs) {
        if (glyph.index >= offset) {
          x = glyph.x;
          break;
        }
      }
      if (offset <= line.start) x = 0;
      return { line: l, x, top: line.top };
    }
  }
  return { line: 0, x: 0, top: 0 };
}

/** One step of a path: `M`/`L` x y, `Q` cx cy x y, `C` c1x c1y c2x c2y x y, `Z`. */
export type PathStep =
  | { op: 'M' | 'L'; x: number; y: number }
  | { op: 'Q'; cx: number; cy: number; x: number; y: number }
  | { op: 'C'; c1x: number; c1y: number; c2x: number; c2y: number; x: number; y: number }
  | { op: 'Z' };

/** The steps of SVG path data as `TextEngine.path` writes it (absolute `M`, `L`, `Q`, `C`, `Z`). */
export function parsePath(d: string): PathStep[] {
  const steps: PathStep[] = [];
  const tokens = d.match(/[MLQCZ]|-?\d*\.?\d+(?:e[-+]?\d+)?/gi) ?? [];
  let i = 0;
  const n = () => Number(tokens[i++]);
  while (i < tokens.length) {
    const op = tokens[i++].toUpperCase();
    if (op === 'M' || op === 'L') steps.push({ op, x: n(), y: n() });
    else if (op === 'Q') steps.push({ op, cx: n(), cy: n(), x: n(), y: n() });
    else if (op === 'C') steps.push({ op, c1x: n(), c1y: n(), c2x: n(), c2y: n(), x: n(), y: n() });
    else if (op === 'Z') steps.push({ op: 'Z' });
  }
  return steps;
}

/** What `tracePath` needs of a 2D context (the curves are optional: a stand-in without them gets straight lines). */
export interface PathSink {
  moveTo(x: number, y: number): void;
  lineTo(x: number, y: number): void;
  closePath(): void;
  quadraticCurveTo?(cx: number, cy: number, x: number, y: number): void;
  bezierCurveTo?(c1x: number, c1y: number, c2x: number, c2y: number, x: number, y: number): void;
}

/** Adds `steps` to the current path of `ctx`. */
export function tracePath(ctx: PathSink, steps: readonly PathStep[]): void {
  for (const step of steps) {
    switch (step.op) {
      case 'M':
        ctx.moveTo(step.x, step.y);
        break;
      case 'L':
        ctx.lineTo(step.x, step.y);
        break;
      case 'Q':
        if (ctx.quadraticCurveTo) ctx.quadraticCurveTo(step.cx, step.cy, step.x, step.y);
        else ctx.lineTo(step.x, step.y);
        break;
      case 'C':
        if (ctx.bezierCurveTo) ctx.bezierCurveTo(step.c1x, step.c1y, step.c2x, step.c2y, step.x, step.y);
        else ctx.lineTo(step.x, step.y);
        break;
      case 'Z':
        ctx.closePath();
        break;
    }
  }
}

let registered: Promise<void> | null = null;

/**
 * Registers the five font files as web fonts (`FONT_FACES`), for the text field that edits a frame and the letters of the
 * Greek palette; once per window. Without the FontFace API (tests), nothing happens: the frames are drawn from the
 * module's glyphs anyway.
 */
export function registerTextFonts(engine: TextEngine): Promise<void> {
  if (registered) return registered;
  if (typeof FontFace === 'undefined' || typeof document === 'undefined' || !document.fonts) {
    registered = Promise.resolve();
    return registered;
  }
  const faces: FontFace[] = FONT_FACES.map((face) => {
    const bytes = engine.fontBytes(face.code);
    const descriptors: FontFaceDescriptors = { weight: String(face.weight), style: 'normal' };
    if (face.unicodeRange) descriptors.unicodeRange = face.unicodeRange;
    return new FontFace(face.family, bytes.buffer as ArrayBuffer, descriptors);
  });
  registered = Promise.all(
    faces.map((face) =>
      face.load().then(
        (loaded) => {
          document.fonts.add(loaded);
        },
        () => {
          // The field then shows another font; the frame itself is still drawn from the module's glyphs.
        },
      ),
    ),
  ).then(() => undefined);
  return registered;
}

/**
 * The CSS families of a frame of `font`, in the order the layout takes a character from them (`fallback_chain` in Rust):
 * its own, Latin Modern Math, the other family.
 */
export function cssFamilies(font: FontFamily): string {
  const other = font === 'latinModern' ? 'andika' : 'latinModern';
  return `"${CSS_FAMILY[font]}", "${CSS_MATH_FAMILY}", "${CSS_FAMILY[other]}"`;
}

/**
 * The CSS of a text field that lays text out like `layout` (no kerning, no ligature, the same faces in the same order,
 * line height and tab stops), at `px` pixels per unit of the frame. The field itself never wraps: where the frame wraps,
 * its value holds a line break (`lib/text-field.ts`).
 */
export function textFieldStyle(style: TextStyle, px: number): string {
  const size = style.size * px;
  return [
    `font-family: ${cssFamilies(style.font)}`,
    `font-weight: ${style.bold ? 700 : 400}`,
    `font-size: ${size}px`,
    `line-height: ${size * LINE_HEIGHT}px`,
    'font-kerning: none',
    'font-variant-ligatures: none',
    'font-feature-settings: "kern" 0, "liga" 0, "clig" 0, "calt" 0',
    'font-synthesis: none',
    'letter-spacing: 0',
    'word-spacing: 0',
    'white-space: pre',
    `tab-size: ${TAB_SPACES}`,
  ].join('; ');
}
