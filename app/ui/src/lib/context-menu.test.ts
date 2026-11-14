// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { clampToWindow, menuOrigin, menuPoint, pointOf } from './context-menu';

/** A `contextmenu` event as Chromium makes it: a `PointerEvent` with the device's `pointerType` (jsdom has none: faked). */
function event(init: MouseEventInit, pointerType?: string): MouseEvent {
  const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, ...init });
  if (pointerType !== undefined) Object.defineProperty(e, 'pointerType', { value: pointerType });
  return e;
}

describe('where a context menu comes from', () => {
  it('tells a mouse, a pen and a finger by the pointer type', () => {
    expect(menuOrigin(event({ button: 2 }, 'mouse'))).toBe('mouse');
    expect(menuOrigin(event({ button: 2 }, 'pen'))).toBe('pen');
    expect(menuOrigin(event({ button: 0 }, 'touch'))).toBe('touch');
  });

  it('takes a contextmenu without a button for the keyboard, whatever pointer type it carries (WebView2 repeats the last pointer type)', () => {
    expect(menuOrigin(event({ button: -1, clientX: 593, clientY: 225 }, 'mouse'))).toBe('keyboard');
    expect(pointOf(event({ button: -1, clientX: 593, clientY: 225 }, 'mouse'))).toBeUndefined();
  });

  it('falls back on the button when there is no pointer type: the right click has button 2, the Menu key none', () => {
    expect(menuOrigin(event({ button: 2 }))).toBe('mouse');
    expect(menuOrigin(event({ button: 0 }))).toBe('keyboard');
    // A PointerEvent of the keyboard: an empty type.
    expect(menuOrigin(event({ button: 0 }, ''))).toBe('keyboard');
  });

  it('gives the pointer position of a right click, and nothing for the keyboard', () => {
    expect(pointOf(event({ button: 2, clientX: 120, clientY: 45 }, 'mouse'))).toEqual({ x: 120, y: 45 });
    expect(pointOf(event({ button: 0, clientX: 5, clientY: 6 }))).toBeUndefined();
  });

  it('puts the keyboard\'s menu under the element it is about', () => {
    const anchor = document.createElement('button');
    anchor.getBoundingClientRect = () => ({ left: 30, bottom: 70, top: 50, right: 90, width: 60, height: 20, x: 30, y: 50, toJSON: () => ({}) });
    expect(menuPoint(event({ button: 2, clientX: 5, clientY: 6 }, 'mouse'), anchor)).toEqual({ x: 5, y: 6 });
    expect(menuPoint(event({ button: 0 }), anchor)).toEqual({ x: 30, y: 70 });
  });
});

describe('keeping a menu inside the window', () => {
  const size = { width: 200, height: 150 };
  const bounds = { width: 1000, height: 600 };

  it('opens at the point when there is room', () => {
    expect(clampToWindow({ x: 100, y: 80 }, size, bounds)).toEqual({ x: 100, y: 80 });
  });

  it('moves left and up from the right and bottom edges, with a margin', () => {
    expect(clampToWindow({ x: 950, y: 590 }, size, bounds)).toEqual({ x: 796, y: 446 });
    expect(clampToWindow({ x: 950, y: 590 }, size, bounds, 10)).toEqual({ x: 790, y: 440 });
  });

  it('stays on screen when it is larger than the window', () => {
    expect(clampToWindow({ x: 300, y: 300 }, { width: 2000, height: 900 }, bounds)).toEqual({ x: 4, y: 4 });
  });

  it('never goes before the top and left edges', () => {
    expect(clampToWindow({ x: -20, y: -5 }, size, bounds)).toEqual({ x: 4, y: 4 });
  });
});
