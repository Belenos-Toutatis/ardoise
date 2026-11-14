// @vitest-environment jsdom
/// <reference types="node" />
// Ctrl + P and the right click in the editor (user feedback 2026-11-14): Ctrl + P opens Ardoise's « Imprimer… » for the open note
// (never the web view's print); a mouse's right click on the page opens a menu of the commands the selection block and the keys
// already have; a pen's barrel button, a finger's long press and the keyboard open none; the layers' rows have theirs.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: 'C:\\demo\\Ardoise',
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  setLocale('fr');
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.querySelectorAll('[role="menu"]').forEach((menu) => menu.remove());
  vi.restoreAllMocks();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function render(title: string) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn(), onsaved: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const note = () => mockLibrary().notes.get(path)!.note;
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = (svg.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  const key = async (k: string, init: KeyboardEventInit = {}, on: EventTarget = window) => {
    const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
    on.dispatchEvent(event);
    on.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
    return event;
  };
  /** A `contextmenu` event over the page point (x, y), as the device gives it: Chromium makes it a PointerEvent with a `pointerType`. */
  const rightClick = async (x: number, y: number, device: 'mouse' | 'pen' | 'touch' | 'none' = 'mouse', from: EventTarget = svg) => {
    const at = client(x, y);
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: device === 'none' ? 0 : 2 });
    if (device !== 'none') Object.defineProperty(event, 'pointerType', { value: device });
    from.dispatchEvent(event);
    await tick();
    await tick();
    return event;
  };
  const menu = () => document.querySelector<HTMLElement>('[role="menu"]');
  const labels = () => [...document.querySelectorAll('[role="menuitem"]')].map((e) => e.textContent?.trim());
  const entry = (label: string) => {
    const found = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((e) => e.textContent?.trim() === label);
    if (!found) throw new Error(`no entry "${label}" among ${labels().join(' | ')}`);
    return found;
  };
  const choose = async (label: string) => {
    entry(label).click();
    await tick();
    await tick();
  };
  const paths = () => target.querySelectorAll('g.ink path:not(.live)').length;
  const selection = () => target.querySelector('[data-section="selection"]')?.textContent ?? '';
  return { path, target, svg, note, key, rightClick, menu, labels, entry, choose, paths, selection };
}

describe('Ctrl + P in the editor', () => {
  it('opens Ardoise\'s print dialog for the note, and the browser\'s print stays off', async () => {
    const view = await render('Cours de physique');
    const event = await view.key('p', { ctrlKey: true });
    expect(event.defaultPrevented).toBe(true);
    const dialog = view.target.querySelector('[data-dialog="print"]')!;
    expect(dialog).toBeTruthy();
    expect(dialog.textContent).toContain('Cours de physique');
  });

  it('does the same with Ctrl + Shift + P, from a field being typed in, and does not stack a second dialog', async () => {
    const view = await render('Cours de physique');
    const title = view.target.querySelector<HTMLInputElement>('.title-field input')!;
    title.focus();
    const event = await view.key('P', { ctrlKey: true, shiftKey: true }, title);
    expect(event.defaultPrevented).toBe(true);
    expect(view.target.querySelectorAll('[data-dialog="print"]').length).toBe(1);
    const again = await view.key('p', { ctrlKey: true });
    expect(again.defaultPrevented).toBe(true);
    expect(view.target.querySelectorAll('[data-dialog="print"]').length).toBe(1);
  });

  it('asks once when held down, and never from a dialog already open', async () => {
    const view = await render('Cours de physique');
    await view.key('p', { ctrlKey: true, repeat: true });
    expect(view.target.querySelector('[data-dialog="print"]')).toBeNull();
    [...view.target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.includes('Exporter'))!.click();
    await tick();
    const dialogs = view.target.querySelectorAll('[role="dialog"]').length;
    expect(dialogs).toBe(1);
    const event = await view.key('p', { ctrlKey: true });
    expect(event.defaultPrevented).toBe(true);
    expect(view.target.querySelector('[data-dialog="print"]')).toBeNull();
    expect(view.target.querySelectorAll('[role="dialog"]').length).toBe(dialogs);
  });

  it('puts the text being typed into the note first', async () => {
    const view = await render('Cours de physique');
    await view.key('t');
    const [scale, vx, vy] = (view.svg.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
    const Ctor = (globalThis as { PointerEvent?: typeof MouseEvent }).PointerEvent ?? MouseEvent;
    const options = { bubbles: true, cancelable: true, clientX: 5000 * scale + vx, clientY: 15000 * scale + vy, button: 0, buttons: 1, pointerId: 1 };
    // A tap of the text tool puts a new frame there, typed in right away.
    for (const type of ['pointerdown', 'pointerup']) {
      const event = new Ctor(type, options);
      if (!('pointerId' in event)) Object.defineProperty(event, 'pointerId', { value: 1 });
      view.svg.dispatchEvent(event);
    }
    await tick();
    const field = await vi.waitFor(() => {
      const found = view.target.querySelector<HTMLTextAreaElement | HTMLInputElement>('.stage textarea, .stage input[type="text"]');
      expect(found).toBeTruthy();
      return found!;
    });
    field.value = 'Bonjour';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    await view.key('p', { ctrlKey: true }, field);
    expect(view.target.querySelector('[data-dialog="print"]')).toBeTruthy();
    // The frame was kept (the typing ended), not lost.
    expect(view.target.querySelectorAll('[data-kind="text"], .text-frame').length).toBeGreaterThan(0);
  });
});

describe('a right click on the page', () => {
  /** The first sample of the first stroke of the demo note: a point a stroke surely passes through. */
  const onFirstStroke = (view: Awaited<ReturnType<typeof render>>) => {
    const p = view.note().strokes[0].points[0];
    return { x: p.x, y: p.y };
  };

  it('over a stroke, with the selection tool, selects it and opens the menu of the selection at the pointer', async () => {
    const view = await render('Cours de physique');
    await view.key('v');
    const { x, y } = onFirstStroke(view);
    const event = await view.rightClick(x, y);
    expect(event.defaultPrevented).toBe(true);
    expect(view.selection()).toContain('Traits sélectionnés : 1');
    expect(view.labels()).toEqual(['Copier', 'Couper', 'Coller', 'Dupliquer', 'Redresser les formes', 'Supprimer']);
    const at = view.menu()!;
    expect(at.parentElement).toBe(document.body);
    expect(at.style.left).toBe(`${event.clientX}px`);
    expect(at.getAttribute('aria-label')).toBe('Menu de la page');
  });

  it('deletes, duplicates and copies what is selected, as one undoable step each', async () => {
    const view = await render('Cours de physique');
    await view.key('v');
    const { x, y } = onFirstStroke(view);
    await view.rightClick(x, y);
    const before = view.paths();
    await view.choose('Dupliquer');
    expect(view.paths()).toBe(before + 1);
    await view.rightClick(x, y);
    await view.choose('Supprimer');
    expect(view.paths()).toBeLessThan(before + 1);
    await view.key('z', { ctrlKey: true });
    expect(view.paths()).toBe(before + 1);

    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', { value: { writeText, readText: async () => '' }, configurable: true });
    await view.rightClick(x, y);
    await view.choose('Copier');
    await vi.waitFor(() => expect(writeText).toHaveBeenCalled());
    expect(String((writeText.mock.calls[0] as unknown[])[0])).toContain('ardoise-strokes');
  });

  it('keeps a selection when the pointer is on a part of it, and replaces it for a stroke that is not', async () => {
    const view = await render('Cours de physique');
    await view.key('a', { ctrlKey: true });
    expect(view.selection()).toContain('Traits sélectionnés : 2');
    const { x, y } = onFirstStroke(view);
    await view.rightClick(x, y);
    expect(view.selection()).toContain('Traits sélectionnés : 2');
    expect(view.labels()).toContain('Supprimer');
  });

  it('over nothing with nothing selected, offers paste, select all, undo and redo', async () => {
    const view = await render('Cours de physique');
    await view.key('v');
    const event = await view.rightClick(100, 100);
    expect(event.defaultPrevented).toBe(true);
    expect(view.labels()).toEqual(['Coller', 'Tout sélectionner', 'Annuler', 'Rétablir']);
    // Nothing was done yet: nothing to undo or redo.
    expect(view.entry('Annuler').getAttribute('aria-disabled')).toBe('true');
    expect(view.entry('Rétablir').getAttribute('aria-disabled')).toBe('true');
    await view.choose('Tout sélectionner');
    expect(view.selection()).toContain('Traits sélectionnés : 2');
    expect(view.menu()).toBeNull();
  });

  it('undoes and redoes from the menu', async () => {
    const view = await render('Cours de physique');
    await view.key('v');
    const { x, y } = onFirstStroke(view);
    await view.rightClick(x, y);
    await view.choose('Supprimer');
    const left = view.paths();
    await view.key('Escape');
    await view.rightClick(100, 100);
    expect(view.entry('Annuler').getAttribute('aria-disabled')).toBeNull();
    await view.choose('Annuler');
    expect(view.paths()).toBeGreaterThan(left);
    // The stroke that came back is selected: let it go, to get the menu of the bare page again.
    await view.key('Escape');
    await view.rightClick(100, 100);
    await view.choose('Rétablir');
    expect(view.paths()).toBe(left);
  });

  it('offers « Modifier » for a text frame, which starts typing in it', async () => {
    const view = await render('Réunion parents');
    await view.key('v');
    const event = await view.rightClick(2100, 9100);
    expect(event.defaultPrevented).toBe(true);
    expect(view.labels()).toEqual(['Copier', 'Couper', 'Coller', 'Dupliquer', 'Modifier', 'Supprimer']);
    await view.choose('Modifier');
    await vi.waitFor(() => expect(view.target.querySelector('.stage textarea, .stage input[type="text"]')).toBeTruthy());
  });

  it('opens no menu from a pen\'s barrel button, a finger\'s long press, or the keyboard, and shows no web menu either', async () => {
    const view = await render('Cours de physique');
    await view.key('v');
    const { x, y } = onFirstStroke(view);
    for (const device of ['pen', 'touch', 'none'] as const) {
      const event = await view.rightClick(x, y, device);
      expect(event.defaultPrevented, device).toBe(true);
      expect(view.menu(), device).toBeNull();
    }
    // What was under the pen was not selected either.
    expect(view.selection()).toBe('');
  });

  it('opens no menu in a replay', async () => {
    const view = await render('Cours de physique');
    await view.key('r');
    await vi.waitFor(() => expect(view.target.querySelector('.replay-bar')).toBeTruthy());
    const { x, y } = onFirstStroke(view);
    const event = await view.rightClick(x, y);
    expect(event.defaultPrevented).toBe(true);
    expect(view.menu()).toBeNull();
  });

  it('with another tool offers the existing selection\'s commands, and does not select by itself', async () => {
    const view = await render('Cours de physique');
    await view.key('v');
    await view.key('a', { ctrlKey: true });
    await view.key('p');
    const { x, y } = onFirstStroke(view);
    await view.rightClick(x, y);
    expect(view.labels()).toContain('Supprimer');
    expect(view.selection()).toContain('Traits sélectionnés : 2');
  });

  it('closes with Escape without leaving the editor or dropping the selection', async () => {
    const view = await render('Cours de physique');
    await view.key('v');
    const { x, y } = onFirstStroke(view);
    await view.rightClick(x, y);
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    document.activeElement!.dispatchEvent(escape);
    await tick();
    expect(view.menu()).toBeNull();
    expect(view.selection()).toContain('Traits sélectionnés : 1');
  });
});

describe('the menu of a layer', () => {
  async function layersTab(view: Awaited<ReturnType<typeof render>>) {
    const tab = [...view.target.querySelectorAll<HTMLElement>('[role="tab"]')].find((el) => /Calques/.test(el.textContent ?? el.getAttribute('aria-label') ?? ''));
    tab?.click();
    await tick();
  }

  it('is not the web page\'s menu: the row chooses its layer and opens its own, at the pointer', async () => {
    const view = await render('Cours de physique');
    await layersTab(view);
    const row = view.target.querySelector<HTMLElement>('li.layer[data-layer="0"]')!;
    expect(row).toBeTruthy();
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 700, clientY: 300 });
    Object.defineProperty(event, 'pointerType', { value: 'mouse' });
    row.dispatchEvent(event);
    await tick();
    await tick();
    expect(event.defaultPrevented).toBe(true);
    expect(view.menu()!.style.left).toBe('700px');
    expect(view.labels()).toEqual(['Renommer…', 'Masquer', 'Monter', 'Descendre', 'Fusionner avec le calque du dessous']);
    // The only layer: nothing to move or merge with.
    for (const label of ['Monter', 'Descendre', 'Fusionner avec le calque du dessous']) {
      expect(view.entry(label).getAttribute('aria-disabled'), label).toBe('true');
    }
    await view.choose('Masquer');
    expect(view.target.querySelector('li.layer[data-layer="0"]')!.getAttribute('data-hidden')).toBe('true');
  });
});
