/**
 * Strokes on the system clipboard (milestone M8): copying and pasting between notes, windows
 * and sessions. The clipboard holds plain text, a JSON document carrying a marker; anything
 * else on the clipboard (text, an image, a JSON of another program) is not ours and is ignored.
 * Colour, tool, width and samples travel; the layer does not (pasted strokes go to the active
 * layer of the destination). Text frames travel with the strokes (`objects`, an optional list of the same
 * version: an older reader ignores it): their placement, text, family, bold, size and colour; their id and layer do not
 * (a paste gives new ones). Equations travel the same way: their placement, LaTeX source, drawing and colour, the
 * drawing checked like a note's (`isEquationObject`) before anything is pasted.
 */
import { equationBox, readEquationPath } from '../equation-path';
import { TOOLS, type InkPoint } from '../ink';
import { FONT_FAMILIES, isEquationObject, type EquationObject, type PageObject, type Stroke, type TextObject } from '../note';
import { MAX_PAGE_SIDE } from './ops';

/** The marker that makes a text ours. */
export const CLIPBOARD_MARKER = 'ardoise-strokes';
/** The clipboard format's version. */
export const CLIPBOARD_VERSION = 1;

/** Largest clipboard text read, characters (a note is far smaller; this only bounds hostile input). */
const MAX_TEXT = 64 << 20;
const MAX_STROKES = 100_000;
const MAX_POINTS = 5_000_000;
/**
 * The ranges Rust's `Point` and `Stroke` can hold, with room to spare: a note that cannot be
 * saved must never come in through a paste. Samples: `t` is a `u32` of milliseconds (the paste
 * re-times them, so only the span matters: 24 h), `x` and `y` are `f32` page coordinates within
 * the page limit, the width is a diameter in device units (10 µm; 1 m at most). Pressure is the
 * one value that is clamped to 0..1 instead of refused: a hair outside is harmless.
 */
const MAX_T = 0xffff_ffff;
const MAX_T_SPAN = 24 * 3_600_000;
const MAX_WIDTH = 100_000;
/** At most this many text frames, of at most this many characters each, and sizes and scales within these. */
const MAX_OBJECTS = 10_000;
const MAX_TEXT_LENGTH = 100_000;
const MAX_TEXT_SIZE = 100_000;
const MAX_OBJECT_SCALE = 1_000;

type ClipboardText = Pick<TextObject, 'kind' | 't' | 'x' | 'y' | 'scale' | 'quarterTurns' | 'text' | 'font' | 'bold' | 'size' | 'color'>;
type ClipboardEquation = Pick<EquationObject, 'kind' | 't' | 'x' | 'y' | 'scale' | 'quarterTurns' | 'latex' | 'path' | 'color'>;
type ClipboardObject = ClipboardText | ClipboardEquation;

interface ClipboardDocument {
  ardoise: typeof CLIPBOARD_MARKER;
  version: number;
  strokes: { tool: string; color: Stroke['color']; width: number; points: InkPoint[] }[];
  objects?: ClipboardObject[];
}

/** The clipboard text for `strokes`. */
export function encodeStrokes(strokes: readonly Stroke[]): string {
  return encodeClipboard(strokes, []);
}

/** The clipboard text for `strokes` and the text frames and equations among `objects` (other kinds stay behind). */
export function encodeClipboard(strokes: readonly Stroke[], objects: readonly PageObject[]): string {
  const document: ClipboardDocument = {
    ardoise: CLIPBOARD_MARKER,
    version: CLIPBOARD_VERSION,
    strokes: strokes.map(({ tool, color, width, points }) => ({ tool, color, width, points })),
  };
  const carried: ClipboardObject[] = [];
  for (const o of objects) {
    if (o.kind === 'text') {
      const { kind, t, x, y, scale, quarterTurns, text, font, bold, size, color } = o as TextObject;
      carried.push({ kind, t, x, y, scale, quarterTurns, text, font, bold, size, color });
    } else if (isEquationObject(o)) {
      const { kind, t, x, y, scale, quarterTurns, latex, path, color } = o;
      carried.push({ kind, t, x, y, scale, quarterTurns, latex, path, color });
    }
  }
  if (carried.length > 0) document.objects = carried;
  return JSON.stringify(document);
}

const finite = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const within = (v: unknown, limit: number): v is number => finite(v) && Math.abs(v) <= limit;

function color(value: unknown): Stroke['color'] | null {
  if (typeof value !== 'object' || value === null) return null;
  const { r, g, b, a } = value as Record<string, unknown>;
  const channel = (c: unknown): c is number => finite(c) && c >= 0 && c <= 255;
  return channel(r) && channel(g) && channel(b) && channel(a) ? { r, g, b, a } : null;
}

function point(value: unknown): InkPoint | null {
  if (typeof value !== 'object' || value === null) return null;
  const { x, y, pressure, t } = value as Record<string, unknown>;
  if (!within(x, MAX_PAGE_SIDE) || !within(y, MAX_PAGE_SIDE) || !finite(pressure)) return null;
  if (!finite(t) || !Number.isInteger(t) || t < 0 || t > MAX_T) return null;
  return { x, y, pressure: Math.min(1, Math.max(0, pressure)), t };
}

/**
 * The strokes (on layer 0, to be placed by the paste) in a clipboard text, or null when the
 * text is not ours or is damaged: nothing foreign is ever pasted.
 */
export function decodeStrokes(text: string): Stroke[] | null {
  const content = decodeClipboard(text);
  return content && content.strokes.length > 0 ? content.strokes : null;
}

/** The placement of a clipboard object (its time, position, scale and turns), or null when it is damaged. */
function placement(o: Record<string, unknown>): Pick<PageObject, 't' | 'x' | 'y' | 'scale' | 'quarterTurns'> | null {
  if (!within(o.x, MAX_PAGE_SIDE) || !within(o.y, MAX_PAGE_SIDE) || !within(o.scale, MAX_OBJECT_SCALE) || o.scale <= 0) return null;
  const turns = o.quarterTurns ?? 0;
  if (!finite(turns) || !Number.isInteger(turns) || turns < 0 || turns > 3) return null;
  if (!finite(o.t) || !Number.isInteger(o.t) || o.t < 0 || o.t > MAX_T) return null;
  return { t: o.t, x: o.x, y: o.y, scale: o.scale, quarterTurns: turns };
}

/** A text frame of the clipboard, or null when it is damaged (its box is measured again by the paste's note). */
function textObject(o: Record<string, unknown>): TextObject | null {
  const rgba = color(o.color);
  const font = FONT_FAMILIES.find((f) => f === o.font);
  if (o.kind !== 'text' || !rgba || !font || typeof o.text !== 'string' || o.text.length > MAX_TEXT_LENGTH) return null;
  if (typeof o.bold !== 'boolean' || !within(o.size, MAX_TEXT_SIZE) || o.size <= 0) return null;
  const placed = placement(o);
  if (!placed) return null;
  return { kind: 'text', id: '', layer: 0, ...placed, width: 0, height: 0, text: o.text, font, bold: o.bold, size: o.size, color: rgba };
}

/** An equation of the clipboard, its box measured from its drawing, or null when it is damaged or its drawing unreadable. */
function equationObject(o: Record<string, unknown>): EquationObject | null {
  const rgba = color(o.color);
  const placed = placement(o);
  if (o.kind !== 'equation' || !rgba || !placed || typeof o.latex !== 'string' || typeof o.path !== 'string') return null;
  const steps = readEquationPath(o.path);
  if (!steps) return null;
  const equation: EquationObject = { kind: 'equation', id: '', layer: 0, ...placed, ...equationBox(steps), latex: o.latex, path: o.path, color: rgba };
  return isEquationObject(equation) ? equation : null;
}

/** A page object of the clipboard, or null when it is damaged or of a kind this version does not paste. */
function clipboardObject(value: unknown): TextObject | EquationObject | null {
  if (typeof value !== 'object' || value === null) return null;
  const o = value as Record<string, unknown>;
  return o.kind === 'equation' ? equationObject(o) : textObject(o);
}

/**
 * The strokes, text frames and equations (on layer 0, to be placed by the paste) in a clipboard text, or null when the
 * text is not ours, is damaged, or holds nothing: nothing foreign is ever pasted. The frames' boxes are 0 until measured.
 */
export function decodeClipboard(text: string): { strokes: Stroke[]; objects: (TextObject | EquationObject)[] } | null {
  if (text.length > MAX_TEXT || !text.trimStart().startsWith('{') || !text.includes(CLIPBOARD_MARKER)) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof parsed !== 'object' || parsed === null) return null;
  const { ardoise, version, strokes, objects } = parsed as Partial<ClipboardDocument>;
  if (ardoise !== CLIPBOARD_MARKER || version !== CLIPBOARD_VERSION) return null;
  if (!Array.isArray(strokes) || strokes.length > MAX_STROKES) return null;
  if (objects !== undefined && (!Array.isArray(objects) || objects.length > MAX_OBJECTS)) return null;
  const frames: (TextObject | EquationObject)[] = [];
  for (const raw of objects ?? []) {
    const frame = clipboardObject(raw);
    if (!frame) return null;
    frames.push(frame);
  }
  if (strokes.length === 0 && frames.length === 0) return null;
  const result: Stroke[] = [];
  let total = 0;
  let first = Infinity;
  let last = -Infinity;
  for (const raw of strokes) {
    if (typeof raw !== 'object' || raw === null) return null;
    const tool = TOOLS.find((t) => t === raw.tool);
    const rgba = color(raw.color);
    if (!tool || !rgba || !within(raw.width, MAX_WIDTH) || raw.width <= 0 || !Array.isArray(raw.points) || raw.points.length === 0) {
      return null;
    }
    total += raw.points.length;
    if (total > MAX_POINTS) return null;
    const points: InkPoint[] = [];
    for (const p of raw.points) {
      const checked = point(p);
      if (!checked) return null;
      first = Math.min(first, checked.t);
      last = Math.max(last, checked.t);
      points.push(checked);
    }
    result.push({ tool, color: rgba, width: raw.width, layer: 0, points });
  }
  for (const frame of frames) {
    first = Math.min(first, frame.t);
    last = Math.max(last, frame.t);
  }
  return last - first <= MAX_T_SPAN ? { strokes: result, objects: frames } : null;
}
