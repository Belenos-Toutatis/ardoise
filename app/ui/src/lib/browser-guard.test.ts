// @vitest-environment jsdom
/**
 * What Ardoise takes from the web view: the keys of a browser (reload, print, find, view source, downloads…) and the
 * right-click menu of a web page, except in a text field (`lib/browser-guard.ts`).
 */
import { afterEach, describe, expect, it } from 'vitest';
import { installBrowserGuard, isBrowserKey, isTextField } from './browser-guard';
import { keyPress } from './shortcuts';

const press = (key: string, mods: { ctrl?: boolean; shift?: boolean; alt?: boolean } = {}, devtools = true) =>
  isBrowserKey(keyPress(key, undefined, mods), devtools);

describe('the keys of a browser', () => {
  it('names reload, print, find, caret browsing, source, downloads, history, open and save', () => {
    for (const key of ['F5', 'F3', 'F7']) expect(press(key), key).toBe(true);
    expect(press('F3', { shift: true })).toBe(true);
    expect(press('F5', { ctrl: true })).toBe(true);
    for (const letter of ['r', 'p', 'f', 'g', 'u', 'j', 'h', 'o', 's']) expect(press(letter, { ctrl: true }), `Ctrl + ${letter}`).toBe(true);
    for (const letter of ['r', 'p', 'g']) expect(press(letter, { ctrl: true, shift: true }), `Ctrl + Shift + ${letter}`).toBe(true);
    expect(press('Delete', { ctrl: true, shift: true })).toBe(true);
  });

  it('keeps the developer tools to a development build', () => {
    expect(press('F12')).toBe(true);
    for (const letter of ['i', 'j', 'c']) expect(press(letter, { ctrl: true, shift: true })).toBe(true);
    expect(press('F12', {}, false)).toBe(false);
    for (const letter of ['i', 'j', 'c']) expect(press(letter, { ctrl: true, shift: true }, false)).toBe(false);
  });

  it('never takes the editing keys, the keys the app answers or the zoom keys', () => {
    for (const letter of ['a', 'c', 'v', 'x', 'z', 'y', 'w', 'q', 'n', 'b']) expect(press(letter, { ctrl: true }), `Ctrl + ${letter}`).toBe(false);
    expect(press('z', { ctrl: true, shift: true })).toBe(false);
    expect(press('F4', { ctrl: true })).toBe(false);
    for (const digit of ['0', '2', '+', '-', '=']) expect(press(digit, { ctrl: true }), `Ctrl + ${digit}`).toBe(false);
    for (const key of ['Escape', 'Enter', 'Tab', ' ', 'Delete', 'Backspace', 'F1', 'F2', 'F4', 'F6', 'F10', 'F11', 'ArrowLeft', 'p', 'r', 'f', 's']) {
      expect(press(key), key).toBe(false);
    }
    // Alt + arrows are the editor's pages; Alt + F4 is Windows'.
    expect(press('ArrowLeft', { alt: true })).toBe(false);
    expect(press('F4', { alt: true })).toBe(false);
    expect(press('p', { ctrl: true, alt: true })).toBe(false);
  });
});

describe('a text field', () => {
  it('is an input of a text kind, a text area, or an editable element; not a button, a checkbox or the page', () => {
    const input = (type: string) => Object.assign(document.createElement('input'), { type });
    for (const type of ['text', 'search', 'number', 'password', 'email', 'url', 'date']) expect(isTextField(input(type)), type).toBe(true);
    for (const type of ['button', 'checkbox', 'radio', 'range', 'color', 'file', 'submit']) expect(isTextField(input(type)), type).toBe(false);
    expect(isTextField(document.createElement('textarea'))).toBe(true);
    const editable = document.createElement('div');
    editable.contentEditable = 'true';
    // jsdom does not compute `isContentEditable`.
    Object.defineProperty(editable, 'isContentEditable', { value: true });
    expect(isTextField(editable)).toBe(true);
    expect(isTextField(document.createElement('div'))).toBe(false);
    expect(isTextField(document.createElement('select'))).toBe(false);
    expect(isTextField(null)).toBe(false);
    expect(isTextField(window)).toBe(false);
  });
});

describe('the guard on a window', () => {
  let remove: (() => void) | null = null;

  afterEach(() => {
    remove?.();
    remove = null;
    document.body.replaceChildren();
  });

  function keydown(target: EventTarget, key: string, init: KeyboardEventInit = {}) {
    const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
    target.dispatchEvent(event);
    return event;
  }

  function rightClick(target: EventTarget) {
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 });
    target.dispatchEvent(event);
    return event;
  }

  it('prevents the keys of a browser, wherever the focus is, and leaves the others alone', () => {
    remove = installBrowserGuard(window);
    const field = document.body.appendChild(document.createElement('input'));
    for (const target of [document.body, field]) {
      expect(keydown(target, 'F5').defaultPrevented).toBe(true);
      expect(keydown(target, 'p', { ctrlKey: true }).defaultPrevented).toBe(true);
      expect(keydown(target, 'P', { ctrlKey: true, shiftKey: true }).defaultPrevented).toBe(true);
      expect(keydown(target, 'f', { ctrlKey: true }).defaultPrevented).toBe(true);
      expect(keydown(target, 'r', { ctrlKey: true }).defaultPrevented).toBe(true);
      for (const key of ['a', 'c', 'v', 'x', 'z', 'y']) expect(keydown(target, key, { ctrlKey: true }).defaultPrevented, `Ctrl + ${key}`).toBe(false);
      expect(keydown(target, 'w', { ctrlKey: true }).defaultPrevented).toBe(false);
      expect(keydown(target, 'Escape').defaultPrevented).toBe(false);
    }
  });

  it('still lets the page handle the key it prevents', () => {
    remove = installBrowserGuard(window);
    let seen = false;
    const listen = () => (seen = true);
    window.addEventListener('keydown', listen);
    keydown(document.body, 'p', { ctrlKey: true });
    window.removeEventListener('keydown', listen);
    expect(seen).toBe(true);
  });

  it('keeps the keys of a browser when asked to, in a development build', () => {
    remove = installBrowserGuard(window, { keys: false });
    expect(keydown(document.body, 'F5').defaultPrevented).toBe(false);
    expect(keydown(document.body, 'p', { ctrlKey: true }).defaultPrevented).toBe(false);
    // The right click is never the web page's.
    expect(rightClick(document.body).defaultPrevented).toBe(true);
  });

  it('leaves the developer tools alone without `devtools`', () => {
    remove = installBrowserGuard(window, { devtools: false });
    expect(keydown(document.body, 'F12').defaultPrevented).toBe(false);
    expect(keydown(document.body, 'F5').defaultPrevented).toBe(true);
  });

  it('shows no menu on a right click, except in a text field and a text area', () => {
    remove = installBrowserGuard(window);
    const image = document.body.appendChild(document.createElement('img'));
    const button = document.body.appendChild(document.createElement('button'));
    const box = document.body.appendChild(document.createElement('input'));
    box.type = 'checkbox';
    const text = document.body.appendChild(document.createElement('input'));
    const area = document.body.appendChild(document.createElement('textarea'));
    for (const target of [document.body, image, button, box]) expect(rightClick(target).defaultPrevented).toBe(true);
    for (const target of [text, area]) expect(rightClick(target).defaultPrevented).toBe(false);
  });

  it('stops guarding when removed', () => {
    const stop = installBrowserGuard(window);
    stop();
    expect(keydown(document.body, 'F5').defaultPrevented).toBe(false);
    expect(rightClick(document.body).defaultPrevented).toBe(false);
  });
});
