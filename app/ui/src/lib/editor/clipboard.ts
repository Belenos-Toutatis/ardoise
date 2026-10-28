/**
 * Strokes on the system clipboard (milestone M8): copying and pasting between notes, windows
 * and sessions. The clipboard holds plain text, a JSON document carrying a marker; anything
 * else on the clipboard (text, an image, a JSON of another program) is not ours and is ignored.
 * Colour, tool, width and samples travel; the layer does not (pasted strokes go to the active
 * layer of the destination).
 */
import { TOOLS, type InkPoint } from '../ink';
import type { Stroke } from '../note';

/** The marker that makes a text ours. */
export const CLIPBOARD_MARKER = 'ardoise-strokes';
/** The clipboard format's version. */
export const CLIPBOARD_VERSION = 1;

/** Largest clipboard text read, characters (a note is far smaller; this only bounds hostile input). */
const MAX_TEXT = 64 << 20;
const MAX_STROKES = 100_000;
const MAX_POINTS = 5_000_000;

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

function color(value: unknown): Stroke['color'] | null {
  if (typeof value !== 'object' || value === null) return null;
  const { r, g, b, a } = value as Record<string, unknown>;
  const channel = (c: unknown): c is number => finite(c) && c >= 0 && c <= 255;
  return channel(r) && channel(g) && channel(b) && channel(a) ? { r, g, b, a } : null;
}

function point(value: unknown): InkPoint | null {
  if (typeof value !== 'object' || value === null) return null;
  const { x, y, pressure, t } = value as Record<string, unknown>;
  return finite(x) && finite(y) && finite(pressure) && finite(t) ? { x, y, pressure, t } : null;
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
  for (const raw of strokes) {
    if (typeof raw !== 'object' || raw === null) return null;
    const tool = TOOLS.find((t) => t === raw.tool);
    const rgba = color(raw.color);
    if (!tool || !rgba || !finite(raw.width) || raw.width <= 0 || !Array.isArray(raw.points) || raw.points.length === 0) {
      return null;
    }
    total += raw.points.length;
    if (total > MAX_POINTS) return null;
    const points: InkPoint[] = [];
    for (const p of raw.points) {
      const checked = point(p);
      if (!checked) return null;
      points.push(checked);
    }
    result.push({ tool, color: rgba, width: raw.width, layer: 0, points });
  }
  return result;
}
