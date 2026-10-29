/**
 * Strokes on the system clipboard (milestone M8): copying and pasting between notes, windows
 * and sessions. The clipboard holds plain text, a JSON document carrying a marker; anything
 * else on the clipboard (text, an image, a JSON of another program) is not ours and is ignored.
 * Colour, tool, width and samples travel; the layer does not (pasted strokes go to the active
 * layer of the destination).
 */
import { TOOLS, type InkPoint } from '../ink';
import type { Stroke } from '../note';
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

interface ClipboardDocument {
  ardoise: typeof CLIPBOARD_MARKER;
  version: number;
  strokes: { tool: string; color: Stroke['color']; width: number; points: InkPoint[] }[];
}

/** The clipboard text for `strokes`. */
export function encodeStrokes(strokes: readonly Stroke[]): string {
  const document: ClipboardDocument = {
    ardoise: CLIPBOARD_MARKER,
    version: CLIPBOARD_VERSION,
    strokes: strokes.map(({ tool, color, width, points }) => ({ tool, color, width, points })),
  };
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
  if (text.length > MAX_TEXT || !text.trimStart().startsWith('{') || !text.includes(CLIPBOARD_MARKER)) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof parsed !== 'object' || parsed === null) return null;
  const { ardoise, version, strokes } = parsed as Partial<ClipboardDocument>;
  if (ardoise !== CLIPBOARD_MARKER || version !== CLIPBOARD_VERSION) return null;
  if (!Array.isArray(strokes) || strokes.length === 0 || strokes.length > MAX_STROKES) return null;
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
  return last - first <= MAX_T_SPAN ? result : null;
}
