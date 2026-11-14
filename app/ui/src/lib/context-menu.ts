/**
 * The pieces of the context menus shared by every screen (`components/ContextMenu.svelte`): what a menu entry is, where
 * a right click was (or was not: the keyboard's Menu key has no position), and how a menu is kept inside the window.
 */

export interface MenuItem {
  id: string;
  label: string;
  danger?: boolean;
  /** Not available now: shown, but nothing happens; `hint` says why. */
  disabled?: boolean;
  hint?: string;
  /** A rule is drawn above the entry (to group the entries of a longer menu). */
  separatorBefore?: boolean;
  onselect: () => void;
}

export interface Point {
  x: number;
  y: number;
}

/** What gave the `contextmenu` event: a mouse's right click, a pen's barrel button, a finger's long press, or the keyboard. */
export type MenuOrigin = 'mouse' | 'pen' | 'touch' | 'keyboard';

/**
 * Where a `contextmenu` event comes from. Chromium makes it a `PointerEvent`; for the Menu key and Shift + F10 it has no
 * button (`button` is -1 in WebView2, checked on the real window, with the last pointer's type and position: not to be taken
 * for a mouse), otherwise `pointerType` says which device. Without a pointer type, a right click has button 2.
 */
export function menuOrigin(event: MouseEvent): MenuOrigin {
  if (event.button === -1) return 'keyboard';
  const type = (event as Partial<PointerEvent>).pointerType;
  if (type === 'mouse' || type === 'pen' || type === 'touch') return type;
  return event.button === 2 ? 'mouse' : 'keyboard';
}

/** The pointer's position in the window for a menu opened by a right click; undefined when the keyboard opened it. */
export function pointOf(event: MouseEvent): Point | undefined {
  return menuOrigin(event) === 'keyboard' ? undefined : { x: event.clientX, y: event.clientY };
}

/**
 * Where a menu opened by `event` stands: at the pointer for a right click; for the keyboard's Menu key, under `anchor` (the
 * element that has the focus and that the menu is about), at its left edge.
 */
export function menuPoint(event: MouseEvent, anchor: Element): Point {
  const box = anchor.getBoundingClientRect();
  return pointOf(event) ?? { x: box.left, y: box.bottom };
}

/** Where a menu of `size` goes so that it opens at `at` and stays inside `bounds` (the window), `margin` px from its edges. */
export function clampToWindow(
  at: Point,
  size: { width: number; height: number },
  bounds: { width: number; height: number },
  margin = 4,
): Point {
  const fit = (start: number, length: number, room: number) => Math.max(margin, Math.min(start, room - length - margin));
  return { x: fit(at.x, size.width, bounds.width), y: fit(at.y, size.height, bounds.height) };
}
