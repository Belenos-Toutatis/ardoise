// @vitest-environment jsdom
/**
 * The shared context menu: at the point asked (kept inside the window), the keyboard (arrows, Home, End, Enter, Escape
 * with the focus given back), what closes it, disabled entries, one at a time.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MenuItem } from '../lib/context-menu';
import ContextMenu from './ContextMenu.svelte';

let mounted: { destroy: () => void }[] = [];

afterEach(() => {
  for (const one of mounted) one.destroy();
  mounted = [];
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

function items(picked: string[] = []): MenuItem[] {
  return [
    { id: 'copy', label: 'Copier', onselect: () => picked.push('copy') },
    { id: 'cut', label: 'Couper', onselect: () => picked.push('cut') },
    { id: 'off', label: 'Coller', disabled: true, hint: 'Rien à coller', onselect: () => picked.push('off') },
    { id: 'delete', label: 'Supprimer', danger: true, separatorBefore: true, onselect: () => picked.push('delete') },
  ];
}

async function open(options: { at?: { x: number; y: number }; list?: MenuItem[]; returnTo?: HTMLElement | null } = {}) {
  // The menu's own size is measured: jsdom has no layout, so the box is faked (a 160 x 120 menu).
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const size = this.classList.contains('context-menu') ? { width: 160, height: 120 } : { width: 0, height: 0 };
    return { left: 0, top: 0, right: size.width, bottom: size.height, x: 0, y: 0, ...size, toJSON: () => ({}) };
  });
  const closed = vi.fn();
  let gone = false;
  const target = document.createElement('div');
  document.body.append(target);
  // The owner forgets the menu when it closes, which unmounts it.
  const component = mount(ContextMenu, {
    target,
    props: {
      label: 'Menu de test',
      items: options.list ?? items(),
      at: options.at ?? { x: 100, y: 80 },
      onclose: () => {
        closed();
        handle.destroy();
      },
      returnTo: options.returnTo ?? null,
    },
  });
  const handle = {
    destroy: () => {
      if (gone) return;
      gone = true;
      unmount(component);
      target.remove();
    },
  };
  mounted.push(handle);
  await tick();
  return { closed, menu: () => document.querySelector<HTMLElement>('[role="menu"]'), entries: () => [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')] };
}

const key = (el: Element, name: string, init: KeyboardEventInit = {}) =>
  el.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init }));

describe('the context menu', () => {
  it('is a labelled menu at the end of the document, at the point, with the focus on its first entry', async () => {
    const view = await open();
    const menu = view.menu()!;
    expect(menu.parentElement).toBe(document.body);
    expect(menu.getAttribute('aria-label')).toBe('Menu de test');
    expect(menu.style.left).toBe('100px');
    expect(menu.style.top).toBe('80px');
    expect(view.entries().map((e) => e.textContent?.trim())).toEqual(['Copier', 'Couper', 'Coller', 'Supprimer']);
    expect(document.activeElement).toBe(view.entries()[0]);
    expect(document.querySelectorAll('[role="separator"]').length).toBe(1);
  });

  it('stays inside the window, however near the edge the point is', async () => {
    Object.assign(window, { innerWidth: 1000, innerHeight: 600 });
    const view = await open({ at: { x: 990, y: 595 } });
    expect(view.menu()!.style.left).toBe('836px');
    expect(view.menu()!.style.top).toBe('476px');
  });

  it('moves between the entries with the arrows, Home and End, going round', async () => {
    const view = await open();
    const [first, second, third, last] = view.entries();
    const menu = view.menu()!;
    key(first, 'ArrowDown');
    expect(document.activeElement).toBe(second);
    key(second, 'ArrowDown');
    expect(document.activeElement).toBe(third);
    key(last, 'End');
    expect(document.activeElement).toBe(last);
    key(last, 'ArrowDown');
    expect(document.activeElement).toBe(first);
    key(first, 'ArrowUp');
    expect(document.activeElement).toBe(last);
    key(last, 'Home');
    expect(document.activeElement).toBe(first);
    expect(menu.isConnected).toBe(true);
  });

  it('chooses an entry on Enter (a button\'s click) and closes, giving the focus back before it acts', async () => {
    const opener = document.body.appendChild(document.createElement('button'));
    opener.focus();
    const picked: string[] = [];
    let focusWhenChosen: Element | null = null;
    const list = items(picked);
    list[1].onselect = () => {
      picked.push('cut');
      focusWhenChosen = document.activeElement;
    };
    const view = await open({ list });
    view.entries()[1].click();
    await tick();
    expect(picked).toEqual(['cut']);
    expect(view.closed).toHaveBeenCalledTimes(1);
    expect(view.menu()).toBeNull();
    expect(focusWhenChosen).toBe(opener);
  });

  it('shows a disabled entry, with its hint, and does nothing when it is chosen', async () => {
    const picked: string[] = [];
    const view = await open({ list: items(picked) });
    const off = view.entries()[2];
    expect(off.getAttribute('aria-disabled')).toBe('true');
    expect(off.title).toBe('Rien à coller');
    off.click();
    await tick();
    expect(picked).toEqual([]);
    expect(view.menu()).toBeTruthy();
    expect(view.closed).not.toHaveBeenCalled();
  });

  it('starts on the first entry that is available', async () => {
    const list = items();
    list[0].disabled = true;
    const view = await open({ list });
    expect(document.activeElement).toBe(view.entries()[1]);
  });

  it('closes on Escape and gives the focus back to what had it, or to `returnTo`', async () => {
    const opener = document.body.appendChild(document.createElement('button'));
    opener.focus();
    const view = await open();
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    view.entries()[0].dispatchEvent(escape);
    await tick();
    expect(escape.defaultPrevented).toBe(true);
    expect(view.menu()).toBeNull();
    expect(view.closed).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(opener);

    const other = document.body.appendChild(document.createElement('button'));
    const again = await open({ returnTo: other });
    key(again.entries()[0], 'Escape');
    await tick();
    expect(document.activeElement).toBe(other);
  });

  it('keeps Escape from reaching the screen under it', async () => {
    const reached = vi.fn();
    window.addEventListener('keydown', reached);
    const view = await open();
    key(view.entries()[0], 'Escape');
    window.removeEventListener('keydown', reached);
    expect(reached).not.toHaveBeenCalled();
  });

  it('closes on Tab', async () => {
    const view = await open();
    key(view.entries()[0], 'Tab');
    await tick();
    expect(view.menu()).toBeNull();
  });

  it('closes on a click or a right click elsewhere, a scroll, a resize and the window losing the focus', async () => {
    const outsides: [string, () => void][] = [
      ['a click elsewhere', () => document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }))],
      ['a right click elsewhere', () => document.body.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, button: 2 }))],
      ['a scroll', () => document.body.dispatchEvent(new Event('scroll'))],
      ['a wheel', () => document.body.dispatchEvent(new Event('wheel', { bubbles: true }))],
      ['a resize', () => window.dispatchEvent(new Event('resize'))],
      ['the window losing the focus', () => window.dispatchEvent(new Event('blur'))],
    ];
    for (const [name, act] of outsides) {
      const view = await open();
      act();
      await tick();
      expect(view.menu(), name).toBeNull();
      expect(view.closed, name).toHaveBeenCalledTimes(1);
    }
  });

  it('does not close on a press inside it', async () => {
    const view = await open();
    view.entries()[0].dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await tick();
    expect(view.menu()).toBeTruthy();
  });

  it('is one at a time: a second menu closes the first', async () => {
    const first = await open();
    const second = await open();
    await tick();
    expect(first.closed).toHaveBeenCalledTimes(1);
    expect(second.closed).not.toHaveBeenCalled();
    expect(document.querySelectorAll('[role="menu"]').length).toBe(1);
  });
});
