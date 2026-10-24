/** Colour helpers and the custom colours remembered on this computer (browser storage). */
import { INKSPACE_COLORS, type Rgba } from '../note';

/** Translation keys (`editor.colors.*`) of the Inkspace palette, in order. */
export const INKSPACE_COLOR_NAMES = ['black', 'blue', 'green', 'purple', 'red', 'orange', 'grey'] as const;

export const CUSTOM_COLORS_KEY = 'ardoise.editor.customColors';
export const MAX_CUSTOM_COLORS = 12;

export function toHex(c: Rgba): string {
  return '#' + [c.r, c.g, c.b].map((v) => Math.round(v).toString(16).padStart(2, '0')).join('');
}

/** `#rrggbb` → opaque colour, or null if malformed. */
export function fromHex(hex: string): Rgba | null {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
  if (!m) return null;
  return { r: parseInt(m[1], 16), g: parseInt(m[2], 16), b: parseInt(m[3], 16), a: 255 };
}

export function sameColor(a: Rgba, b: Rgba): boolean {
  return a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
}

export function isInkspaceColor(c: Rgba): boolean {
  return INKSPACE_COLORS.some((p) => sameColor(p, c));
}

/** Remembered custom colours, most recent first; empty when storage is unavailable. */
export function loadCustomColors(storage: Pick<Storage, 'getItem'> | null = safeStorage()): Rgba[] {
  try {
    const raw = storage?.getItem(CUSTOM_COLORS_KEY);
    if (!raw) return [];
    const list: unknown = JSON.parse(raw);
    if (!Array.isArray(list)) return [];
    return list
      .map((v) => (typeof v === 'string' ? fromHex(v) : null))
      .filter((c): c is Rgba => c !== null)
      .slice(0, MAX_CUSTOM_COLORS);
  } catch {
    return [];
  }
}

/** Adds `color` in front of the remembered list (no duplicates); returns the new list. */
export function rememberCustomColor(
  color: Rgba,
  current: readonly Rgba[],
  storage: Pick<Storage, 'setItem'> | null = safeStorage(),
): Rgba[] {
  const opaque = { ...color, a: 255 };
  if (isInkspaceColor(opaque)) return [...current];
  const list = [opaque, ...current.filter((c) => !sameColor(c, opaque))].slice(0, MAX_CUSTOM_COLORS);
  try {
    storage?.setItem(CUSTOM_COLORS_KEY, JSON.stringify(list.map(toHex)));
  } catch {
    // Private mode or storage full: the colour is still usable for this session.
  }
  return list;
}

function safeStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}
