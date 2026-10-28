/**
 * Pen and touch input, free of the DOM (docs/design/interface.md, "M8: pen and touch"): what a
 * pointer is (mouse, pen tip, pen eraser end, finger), the pressure to record, palm rejection,
 * and the finger gestures (pan, pinch, double tap). Components feed it Pointer Events' fields
 * and apply the actions it returns to their own view.
 */
import type { Vec } from './editor/geometry';
import type { View } from './editor/view';
import { clampScale } from './editor/view';

export type PointerKind = 'mouse' | 'pen' | 'eraser' | 'touch';

/** The fields of a `PointerEvent` the classification reads. */
export interface PointerFields {
  pointerType?: string;
  /** Button that changed (-1 when none); 5 is the eraser. */
  button?: number;
  /** Buttons held: 1 tip or left, 2 barrel or right, 4 middle, 32 eraser. */
  buttons?: number;
}

export const BUTTON_PRIMARY = 1;
export const BUTTON_BARREL = 2;
export const BUTTON_ERASER = 32;

/**
 * What the pointer is. The back of a pen is a `pen` pointer with the eraser button (32) held, or
 * the eraser button as the one that changed (5); an unknown type counts as a mouse.
 */
export function pointerKind(e: PointerFields): PointerKind {
  switch (e.pointerType) {
    case 'touch':
      return 'touch';
    case 'pen':
      return ((e.buttons ?? 0) & BUTTON_ERASER) !== 0 || e.button === 5 ? 'eraser' : 'pen';
    default:
      return 'mouse';
  }
}

/** The pen's barrel button is held (a pen tip or a hovering pen, not the eraser end). */
export function barrelHeld(e: PointerFields): boolean {
  return e.pointerType === 'pen' && ((e.buttons ?? 0) & BUTTON_BARREL) !== 0 && ((e.buttons ?? 0) & BUTTON_ERASER) === 0;
}

/** Pressure of a sample that stands for "no information": a mouse, a finger, a pen reporting 0. */
export const DEFAULT_PRESSURE = 0.6;

/**
 * The pressure recorded for a sample. A pen's pressure is kept (so the stroke width follows
 * it); a finger has no useful pressure and a mouse reports a constant one, so both are kept as
 * they always were, with a floor for the drivers that report 0.
 */
export function inkPressure(kind: PointerKind, pressure: number | undefined): number {
  if (typeof pressure !== 'number' || !(pressure > 0)) return DEFAULT_PRESSURE;
  if (kind === 'touch') return DEFAULT_PRESSURE;
  return Math.min(1, pressure);
}

/**
 * Palm rejection: while a pen is in contact or has been seen hovering a moment ago, fingers are
 * ignored. Not every driver reports the pen leaving, so a hover also expires on its own.
 */
export class PenPresence {
  /** How long a hovering pen keeps fingers out after its last event, ms. */
  static readonly HOVER_MS = 1200;
  private down = new Set<number>();
  private lastSeen = Number.NEGATIVE_INFINITY;

  /** Any event of a pen pointer (`down` for a contact, `hover` for a move without contact). */
  seen(id: number, now: number, state: 'down' | 'hover'): void {
    this.lastSeen = now;
    if (state === 'down') this.down.add(id);
    else this.down.delete(id);
  }

  /** The pen lifted (`pointerup`/`pointercancel`): its hover time starts now. */
  lifted(id: number, now: number): void {
    this.down.delete(id);
    this.lastSeen = now;
  }

  /** The pen left the screen's range (`pointerleave`): it no longer blocks. */
  gone(id: number): void {
    this.down.delete(id);
    this.lastSeen = Number.NEGATIVE_INFINITY;
  }

  blocksTouch(now: number): boolean {
    return this.down.size > 0 || now - this.lastSeen < PenPresence.HOVER_MS;
  }
}

// ---------------------------------------------------------------------------------------------
// Finger gestures

/** A finger moves farther than this (CSS pixels) before it counts as a pan rather than a tap. */
export const PAN_SLOP = 8;
/** A touch shorter than this (ms) and within the slop is a tap. */
export const TAP_MS = 350;
/** Two taps closer than this (ms, pixels) are a double tap. */
export const DOUBLE_TAP_MS = 450;
export const DOUBLE_TAP_PX = 40;
/** Fingers closer than this (pixels) give no usable pinch. */
const MIN_PINCH_DISTANCE = 8;

export type TouchAction =
  /** The finger moved by this much: the content follows. */
  | { kind: 'pan'; dx: number; dy: number }
  /** Two fingers: the point under `from` goes to `to`, scaled by `factor`. */
  | { kind: 'pinch'; from: Vec; to: Vec; factor: number }
  | { kind: 'doubleTap'; at: Vec };

export interface TouchOptions {
  /**
   * `navigate`: one finger pans and double taps fit. `draw`: one finger belongs to the tool (it
   * draws, selects…): only the two-finger pinch is recognised.
   */
  mode: 'navigate' | 'draw';
}

interface Finger {
  start: Vec;
  last: Vec;
  t0: number;
  /** Moved beyond the slop. */
  moved: boolean;
}

/**
 * Tracks the fingers on a surface. The first two fingers count; a third is ignored. One finger
 * pans (after the slop), two pinch (zoom and pan together), a quick still touch is a tap and
 * two taps in a row are a double tap. Positions are in the surface's CSS pixels.
 */
export class TouchGestures {
  private fingers = new Map<number, Finger>();
  /** A gesture with two fingers took place since the first one went down: it is not a tap. */
  private multi = false;
  private lastTap: { at: Vec; t: number } | null = null;

  /** Number of fingers currently tracked. */
  get count(): number {
    return this.fingers.size;
  }

  has(id: number): boolean {
    return this.fingers.has(id);
  }

  /** True when a second finger just joined (the caller drops what the first one was doing). */
  down(id: number, p: Vec, t: number): boolean {
    if (this.fingers.has(id) || this.fingers.size >= 2) return false;
    if (this.fingers.size === 0) this.multi = false;
    this.fingers.set(id, { start: p, last: p, t0: t, moved: false });
    if (this.fingers.size === 2) {
      this.multi = true;
      this.lastTap = null;
      return true;
    }
    return false;
  }

  move(id: number, p: Vec, options: TouchOptions): TouchAction | null {
    const finger = this.fingers.get(id);
    if (!finger) return null;
    if (this.fingers.size === 2) {
      const [a, b] = [...this.fingers.values()];
      const before: [Vec, Vec] = [a.last, b.last];
      finger.last = p;
      const after: [Vec, Vec] = [a.last, b.last];
      return pinchAction(before, after);
    }
    const dx = p.x - finger.last.x;
    const dy = p.y - finger.last.y;
    if (!finger.moved) {
      if (Math.hypot(p.x - finger.start.x, p.y - finger.start.y) < PAN_SLOP) return null;
      finger.moved = true;
      // The slop is eaten: the content starts moving from here, without a jump.
      finger.last = p;
      return null;
    }
    finger.last = p;
    if (options.mode === 'draw') return null;
    return dx === 0 && dy === 0 ? null : { kind: 'pan', dx, dy };
  }

  up(id: number, t: number, options: TouchOptions): TouchAction | null {
    const finger = this.fingers.get(id);
    if (!finger) return null;
    this.fingers.delete(id);
    if (this.fingers.size > 0) {
      // Back to one finger: it continues from where it is, with no slop and no jump.
      for (const rest of this.fingers.values()) rest.moved = true;
      return null;
    }
    if (this.multi || finger.moved || t - finger.t0 > TAP_MS || options.mode === 'draw') return null;
    const tap = { at: finger.last, t };
    const previous = this.lastTap;
    if (previous && t - previous.t <= DOUBLE_TAP_MS && Math.hypot(tap.at.x - previous.at.x, tap.at.y - previous.at.y) <= DOUBLE_TAP_PX) {
      this.lastTap = null;
      return { kind: 'doubleTap', at: tap.at };
    }
    this.lastTap = tap;
    return null;
  }

  /** A finger was taken away by the system (`pointercancel`). */
  cancel(id: number): void {
    if (this.fingers.delete(id)) this.lastTap = null;
  }

  /** Forgets every finger (the pen arrived, the window lost the focus…). */
  reset(): void {
    this.fingers.clear();
    this.multi = false;
    this.lastTap = null;
  }
}

/** The pinch between two pairs of finger positions, or null when the fingers are too close. */
export function pinchAction(before: readonly [Vec, Vec], after: readonly [Vec, Vec]): TouchAction | null {
  const d0 = Math.hypot(before[1].x - before[0].x, before[1].y - before[0].y);
  const d1 = Math.hypot(after[1].x - after[0].x, after[1].y - after[0].y);
  const from = { x: (before[0].x + before[1].x) / 2, y: (before[0].y + before[1].y) / 2 };
  const to = { x: (after[0].x + after[1].x) / 2, y: (after[0].y + after[1].y) / 2 };
  if (d0 < MIN_PINCH_DISTANCE || d1 < MIN_PINCH_DISTANCE) {
    return from.x === to.x && from.y === to.y ? null : { kind: 'pan', dx: to.x - from.x, dy: to.y - from.y };
  }
  const factor = d1 / d0;
  if (factor === 1 && from.x === to.x && from.y === to.y) return null;
  return { kind: 'pinch', from, to, factor };
}

/**
 * The editor's view after a pinch: the page point that was under `from` is under `to`, with the
 * zoom multiplied by `factor` (clamped; the point then follows the real factor).
 */
export function pinchView(view: View, from: Vec, to: Vec, factor: number): View {
  const scale = clampScale(view.scale * factor);
  const anchor = { x: (from.x - view.x) / view.scale, y: (from.y - view.y) / view.scale };
  return { scale, x: to.x - anchor.x * scale, y: to.y - anchor.y * scale };
}
