/**
 * The computer's ink in live mode (docs/design/tablet-live-classroom.md, "M14", "The computer's ink"): the
 * tool, colour and width the control window's pen and mouse write with, remembered on this PC (the
 * browser's storage of the WebView2 profile), apart from the tablet's style. Not in `ardoise.toml`: that
 * file is shared by the PCs through Nextcloud, while the pen is this PC's; and Rust needs the style only per
 * stroke, which `begin` carries. The blackboard turns the tablet's ink to chalk, never the computer's.
 */
import { fromHex, toHex } from './editor/colors';
import { TOOLS, type Tool } from './ink';
import type { LiveStyle } from './live';
import { INKSPACE_COLORS } from './note';

export const COMPUTER_STYLE_KEY = 'ardoise.live.computerStyle';
/** The live window's width slider. */
export const MIN_LIVE_WIDTH = 10;
export const MAX_LIVE_WIDTH = 1000;

/**
 * Felt tip, Inkspace red, 1.2 mm: it reads well on the projector next to a ballpoint (our choice; Q11 says
 * red only).
 */
export const DEFAULT_COMPUTER_STYLE: LiveStyle = { tool: 'feltTip', color: { ...INKSPACE_COLORS[4] }, width: 120 };

function defaults(): LiveStyle {
  return { ...DEFAULT_COMPUTER_STYLE, color: { ...DEFAULT_COMPUTER_STYLE.color } };
}

function safeStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/**
 * The style remembered, field by field: a tool of `TOOLS`, an opaque `#rrggbb` colour, a width within the
 * slider's range; a field that is missing or makes no sense takes the default's. A blocked storage gives the
 * default.
 */
export function loadComputerStyle(storage: Pick<Storage, 'getItem'> | null = safeStorage()): LiveStyle {
  const style = defaults();
  let stored: unknown;
  try {
    stored = JSON.parse(storage?.getItem(COMPUTER_STYLE_KEY) ?? 'null');
  } catch {
    return style;
  }
  if (!stored || typeof stored !== 'object') return style;
  const { tool, color, width } = stored as { tool?: unknown; color?: unknown; width?: unknown };
  if (typeof tool === 'string' && (TOOLS as string[]).includes(tool)) style.tool = tool as Tool;
  const rgba = typeof color === 'string' ? fromHex(color) : null;
  if (rgba) style.color = rgba;
  if (typeof width === 'number' && Number.isFinite(width) && width >= MIN_LIVE_WIDTH && width <= MAX_LIVE_WIDTH) {
    style.width = Math.round(width);
  }
  return style;
}

/** Remembers `style` (its colour as `#rrggbb`: the computer's ink is opaque); a blocked storage is ignored. */
export function saveComputerStyle(style: LiveStyle, storage: Pick<Storage, 'setItem'> | null = safeStorage()): void {
  try {
    storage?.setItem(COMPUTER_STYLE_KEY, JSON.stringify({ tool: style.tool, color: toHex(style.color), width: style.width }));
  } catch {
    // Private mode or storage full: the style still holds for this session.
  }
}
