// @vitest-environment jsdom
/// <reference types="node" />
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { freshStamp, mockLibrary, notifyLibraryChanged, resetMockLibrary } from '../../lib/mock-data';
import { mockClipboard } from '../../lib/mock-export';
import { bigNote } from '../../lib/editor/test-fixtures';
import type { WindowClose } from '../../lib/editor/window-close';
import Editor from './Editor.svelte';

// The real stroke geometry, instantiated from the module's bytes (jsdom cannot fetch it).
vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  // Tests run from `app/` (the Vite root).
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
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

/** Lets pending promises (mock commands, the write queue) run. */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A stand-in for the desktop window: `request()` is the user clicking its close button. */
function fakeWindow() {
  let guard: (() => boolean) | null = null;
  const unlisten = vi.fn();
  const destroy = vi.fn(async () => {});
  const closer: WindowClose = {
    subscribe: async (g) => {
      guard = g;
      return unlisten;
    },
    destroy,
  };
  return { closer, request: () => guard?.() ?? false, destroy, unlisten };
}

async function render(title: string, extra: { windowClose?: WindowClose } = {}) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose, onsaved, ...extra } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;

  const paths = () => [...target.querySelectorAll<SVGPathElement>('g.ink path:not(.live)')];
  const fills = () => paths().map((p) => p.getAttribute('fill'));
  /** Visible name of a button, without its decorative (aria-hidden) icon. */
  const name = (b: HTMLButtonElement) => {
    const copy = b.cloneNode(true) as HTMLElement;
    copy.querySelectorAll('[aria-hidden="true"]').forEach((icon) => icon.remove());
    return copy.textContent?.trim();
  };
  const button = (label: string) => {
    const found = [...target.querySelectorAll('button')].find(
      (b) => b.getAttribute('aria-label') === label || b.title === label || name(b) === label,
    );
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  const input = (label: string) => {
    const found = target.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);
    if (!found) throw new Error(`no input "${label}"`);
    return found;
  };
  /** Page coordinates → client coordinates (the canvas sits at the top-left corner in jsdom). */
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = (svg.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  const pointer = (type: string, x: number, y: number, init: { button?: number; shiftKey?: boolean } = {}) => {
    const at = client(x, y);
    const options = {
      bubbles: true,
      cancelable: true,
      clientX: at.x,
      clientY: at.y,
      button: init.button ?? 0,
      buttons: 1,
      shiftKey: init.shiftKey ?? false,
      pointerId: 1,
      pressure: 0.5,
    };
    const Ctor = (globalThis as { PointerEvent?: typeof MouseEvent }).PointerEvent ?? MouseEvent;
    const event = new Ctor(type, options);
    for (const key of ['pointerId', 'pressure'] as const) {
      if (!(key in event)) Object.defineProperty(event, key, { value: options[key] });
    }
    svg.dispatchEvent(event);
  };
  /** A drag through page points. */
  const drag = async (points: [number, number][], init: { shiftKey?: boolean } = {}) => {
    const [first, ...rest] = points;
    pointer('pointerdown', first[0], first[1], init);
    for (const [x, y] of rest) pointer('pointermove', x, y, init);
    const last = points[points.length - 1];
    pointer('pointerup', last[0], last[1], init);
    await tick();
  };
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const status = () => target.querySelector('.status')?.textContent?.trim();
  const titleInput = () => target.querySelector<HTMLInputElement>('.title-field input')!;
  /** Types a new title in the title field (not committed yet). */
  const typeTitle = (text: string) => {
    const field = titleInput();
    field.focus();
    field.value = text;
    field.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const blurTitle = () => titleInput().dispatchEvent(new FocusEvent('blur'));
  return {
    path,
    target,
    svg,
    paths,
    fills,
    button,
    input,
    pointer,
    drag,
    key,
    status,
    typeTitle,
    blurTitle,
    titleInput,
    onclose,
    onsaved,
  };
}

const line = (x0: number, x1: number, y: number, steps = 20): [number, number][] =>
  Array.from({ length: steps + 1 }, (_, i) => [x0 + ((x1 - x0) * i) / steps, y]);

describe('note editor', () => {
  it('opens a demo note and draws its strokes over the background, in French', async () => {
    const { target, svg, paths, button, status } = await render('Liste de courses');
    expect(paths()).toHaveLength(1);
    expect(paths()[0].getAttribute('d')).toMatch(/^M.*Z$/);
    expect(paths()[0].getAttribute('fill-rule')).toBe('nonzero');
    expect(svg.getAttribute('data-page-width')).toBe('29700');
    await vi.waitFor(() =>
      expect(target.querySelector('image')?.getAttribute('href')).toMatch(/^data:image\/svg\+xml/),
    );
    expect(target.querySelector<HTMLInputElement>('.title-field input')?.value).toBe('Liste de courses');
    expect(status()).toBe('Enregistrée');
    for (const label of ['Bibliothèque', 'Enregistrer', 'Annuler (Ctrl+Z)', 'Rétablir (Ctrl+Y)', 'Stylo', 'Gomme', 'Rouge']) {
      expect(button(label)).toBeTruthy();
    }
    expect(target.textContent).toContain('Calques');
    expect(target.textContent).toContain('Épaisseur');
    expect(target.textContent).toContain('Stylo à bille');
  });

  it('shows the editor in English', async () => {
    setLocale('en');
    const { target, button } = await render('Liste de courses');
    for (const label of ['Library', 'Save', 'Undo (Ctrl+Z)', 'Pen', 'Eraser', 'Select', 'Black', 'Orange']) {
      expect(button(label)).toBeTruthy();
    }
    expect(target.textContent).toContain('Layers');
    expect(target.textContent).toContain('Ballpoint pen');
    expect(target.textContent).toContain('Crop around the content');
  });

  it('recolours the whole note, then undoes and redoes it', async () => {
    const { fills, button, key, status } = await render('Cours de physique');
    const original = fills();
    expect(original).toHaveLength(2);
    button('Rouge').click();
    await tick();
    // Picking a colour with nothing selected only changes the pen.
    expect(fills()).toEqual(original);
    button('Couleur').click();
    await tick();
    expect(fills()).toEqual(['rgba(216, 0, 0, 1.000)', 'rgba(216, 0, 0, 1.000)']);
    expect(status()).toBe('Modifications non enregistrées');
    button('Annuler (Ctrl+Z)').click();
    await tick();
    expect(fills()).toEqual(original);
    expect(status()).toBe('Enregistrée');
    await key('y', { ctrlKey: true });
    expect(fills()).toEqual(['rgba(216, 0, 0, 1.000)', 'rgba(216, 0, 0, 1.000)']);
    await key('z', { ctrlKey: true });
    expect(fills()).toEqual(original);
    await key('z', { ctrlKey: true, shiftKey: true });
    expect(fills()[0]).toBe('rgba(216, 0, 0, 1.000)');
  });

  it('draws strokes with the pen, erases one, and saves', async () => {
    const { path, paths, drag, button, status, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    await drag(line(5000, 15000, 18000));
    expect(paths()).toHaveLength(3);
    expect(status()).toBe('Modifications non enregistrées');

    button('Gomme').click();
    await tick();
    await drag(line(4000, 16000, 18000));
    expect(paths()).toHaveLength(2);

    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledWith(path));
    expect(status()).toBe('Enregistrée');
    const stored = mockLibrary().notes.get(path)!.note;
    expect(stored.strokes).toHaveLength(2);
    const drawn = stored.strokes[1];
    expect(drawn.tool).toBe('ballpoint');
    expect(drawn.layer).toBe(0);
    expect(drawn.points.length).toBeGreaterThan(10);
    expect(drawn.points[0].x).toBeCloseTo(5000, -1);
    expect(drawn.points[0].y).toBeCloseTo(15000, -1);
    // Drawn after the existing strokes on the replay timeline.
    expect(drawn.points[0].t).toBeGreaterThan(Math.max(...stored.strokes[0].points.map((p) => p.t)));
  });

  it('erases part of a stroke, cutting it in two', async () => {
    const { paths, drag, button, target } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    button('Gomme').click();
    await tick();
    const partial = [...target.querySelectorAll<HTMLInputElement>('input[type="radio"]')].find((r) =>
      r.parentElement?.textContent?.includes('Partielle'),
    )!;
    partial.click();
    await tick();
    await drag([
      [10000, 14000],
      [10000, 16000],
    ]);
    expect(paths()).toHaveLength(3);
  });

  it('shows a clear banner when the note changed on disk meanwhile', async () => {
    const { path, target, drag, key, status, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeTruthy());
    const banner = target.querySelector('[role="alert"]')!.textContent ?? '';
    expect(banner).toContain("Vos modifications n'ont pas été enregistrées");
    expect(banner).toContain('la note a été modifiée entre-temps');
    expect(onsaved).not.toHaveBeenCalled();
    expect(status()).toBe('Modifications non enregistrées');
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(1);
  });

  it('hides and shows layers, and draws on a new layer', async () => {
    const { paths, input, button, drag } = await render('Cours de physique');
    expect(paths()).toHaveLength(2);
    input('Afficher « Calque 1 »').click();
    await tick();
    expect(paths()).toHaveLength(0);
    input('Afficher « Calque 1 »').click();
    await tick();
    expect(paths()).toHaveLength(2);

    button('+ Ajouter un calque').click();
    await tick();
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(3);
    input('Afficher « Calque 2 »').click();
    await tick();
    expect(paths()).toHaveLength(2);
    input('Afficher « Calque 1 »').click();
    await tick();
    expect(paths()).toHaveLength(0);
  });

  it('rotates the page a quarter turn', async () => {
    const { svg, button } = await render('Liste de courses');
    button("Tourner la page d'un quart de tour vers la droite").click();
    await tick();
    expect(svg.getAttribute('data-page-width')).toBe('21600');
    expect(svg.getAttribute('data-page-height')).toBe('29700');
    button("Tourner la page d'un quart de tour vers la gauche").click();
    await tick();
    expect(svg.getAttribute('data-page-width')).toBe('29700');
  });

  it('crops automatically around the content', async () => {
    const { svg, button } = await render('Liste de courses');
    button('Recadrer autour du contenu').click();
    await tick();
    expect(Number(svg.getAttribute('data-page-width'))).toBeLessThan(29700);
    expect(Number(svg.getAttribute('data-page-height'))).toBeLessThan(21600);
  });

  it('crops by hand to a dragged rectangle', async () => {
    const { svg, button, drag } = await render('Liste de courses');
    button('Recadrer à la main').click();
    await tick();
    await drag([
      [1000, 1000],
      [11000, 9000],
    ]);
    expect(svg.getAttribute('data-page-width')).toBe('10000');
    expect(svg.getAttribute('data-page-height')).toBe('8000');
  });

  it('selects with the lasso, deletes, undoes, and duplicates with Ctrl+C / Ctrl+V', async () => {
    const { target, paths, button, drag, key } = await render('Liste de courses');
    button('Sélection').click();
    await tick();
    await drag([
      [2000, 3000],
      [13000, 3000],
      [13000, 5000],
      [2000, 5000],
      [2000, 3000],
    ]);
    expect(target.querySelector('rect.selection')).toBeTruthy();
    expect(target.textContent).toContain('Traits sélectionnés : 1');
    await key('Delete');
    expect(paths()).toHaveLength(0);
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(1);
    await key('a', { ctrlKey: true });
    await key('c', { ctrlKey: true });
    await key('v', { ctrlKey: true });
    expect(paths()).toHaveLength(2);
    await key('Escape');
    expect(target.querySelector('rect.selection')).toBeNull();
  });

  it('moves the selection by dragging it, in one undo step', async () => {
    const { path, target, button, drag, key } = await render('Liste de courses');
    button('Sélection').click();
    await tick();
    await key('a', { ctrlKey: true });
    const before = mockLibrary().notes.get(path)!.note.strokes[0].points[0];
    await drag([
      [7500, 4000],
      [8000, 5000],
      [9500, 6000],
    ]);
    expect(target.querySelector('path[transform]')).toBeNull();
    button('Enregistrer').click();
    await vi.waitFor(() => expect(mockLibrary().notes.get(path)!.note.strokes[0].points[0].x).not.toBe(before.x));
    const after = mockLibrary().notes.get(path)!.note.strokes[0].points[0];
    expect(after.x - before.x).toBeCloseTo(2000, -1);
    expect(after.y - before.y).toBeCloseTo(2000, -1);
    await key('z', { ctrlKey: true });
    expect(target.querySelector('.status')?.textContent?.trim()).toBe('Modifications non enregistrées');
  });

  it('remembers custom colours', async () => {
    const { target, button } = await render('Liste de courses');
    const picker = target.querySelector<HTMLInputElement>('input[type="color"]')!;
    picker.value = '#123456';
    picker.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    expect(button('Couleur personnalisée #123456')).toBeTruthy();
    expect(localStorage.getItem('ardoise.editor.customColors')).toBe('["#123456"]');
  });

  it('renames the note from its title', async () => {
    const { path, target, onsaved } = await render('Liste de courses');
    const title = target.querySelector<HTMLInputElement>('.title-field input')!;
    title.value = 'Courses du samedi';
    title.dispatchEvent(new Event('input', { bubbles: true }));
    title.dispatchEvent(new FocusEvent('blur'));
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const renamed = onsaved.mock.calls[0][0] as string;
    expect(renamed).not.toBe(path);
    expect(renamed).toContain('Courses du samedi');
    expect(mockLibrary().notes.get(renamed)?.note.title).toBe('Courses du samedi');
  });

  it('asks before leaving with unsaved changes', async () => {
    const { target, button, drag, onclose } = await render('Liste de courses');
    button('Bibliothèque').click();
    expect(onclose).toHaveBeenCalledTimes(1);

    await drag(line(5000, 15000, 15000));
    button('Bibliothèque').click();
    await tick();
    expect(target.querySelector('[role="alertdialog"]')).toBeTruthy();
    button('Rester').click();
    await tick();
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(onclose).toHaveBeenCalledTimes(1);

    button('Bibliothèque').click();
    await tick();
    button('Revenir sans enregistrer').click();
    expect(onclose).toHaveBeenCalledTimes(2);
  });

  it('saves and leaves from the confirmation', async () => {
    const { path, button, drag, onclose, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    button('Bibliothèque').click();
    await tick();
    button('Enregistrer et revenir').click();
    await vi.waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(onsaved).toHaveBeenCalledWith(path);
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(2);
  });

  it('stays responsive with a big note (300 strokes, 20 000 samples)', async () => {
    const library = mockLibrary();
    const big = { ...bigNote(), title: 'Grosse note', layers: [{ name: 'Calque 1', visible: true }] };
    library.notes.set(`${library.root}\\2026-10-24 Grosse note.ardoise`, { note: big, stamp: freshStamp(), binder: '' });
    let start = performance.now();
    const { paths, button, drag } = await render('Grosse note');
    const opened = performance.now() - start;
    expect(paths()).toHaveLength(300);

    // A recolour redraws every stroke (new stroke objects); a selection change redraws none.
    button('Rouge').click();
    start = performance.now();
    button('Couleur').click();
    await tick();
    const recolour = performance.now() - start;

    button('Gomme').click();
    await tick();
    start = performance.now();
    await drag(line(1000, 29000, 5000, 60));
    const erase = performance.now() - start;
    expect(paths().length).toBeLessThan(300);

    button('Stylo').click();
    await tick();
    start = performance.now();
    await drag(line(1000, 29000, 20000, 200));
    const pen = performance.now() - start;
    // Bounds catch accidental quadratic work (jsdom is far slower than WebView2).
    expect(opened).toBeLessThan(8000);
    expect(recolour).toBeLessThan(3000);
    expect(erase).toBeLessThan(3000);
    expect(pen).toBeLessThan(3000);
  });

  it('saves after a partial erase: the cut strokes have whole-millisecond times', async () => {
    const { path, target, drag, button, onsaved } = await render('Liste de courses');
    button('Gomme').click();
    await tick();
    const partial = [...target.querySelectorAll<HTMLInputElement>('input[type="radio"]')].find((r) =>
      r.parentElement?.textContent?.includes('Partielle'),
    )!;
    partial.click();
    await tick();
    await drag([
      [7500, 3000],
      [7500, 5000],
    ]);
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledWith(path));
    expect(target.querySelector('[role="alert"]')).toBeNull();
    const stored = mockLibrary().notes.get(path)!.note;
    expect(stored.strokes.length).toBeGreaterThan(1);
    const times = stored.strokes.flatMap((s) => s.points.map((p) => p.t));
    expect(times.every((t) => Number.isInteger(t) && t >= 0)).toBe(true);
  });

  it('holds back closing the window with unsaved changes, and asks', async () => {
    const win = fakeWindow();
    const { path, target, button, drag, onclose } = await render('Liste de courses', { windowClose: win.closer });
    // Nothing to save: the window closes.
    expect(win.request()).toBe(false);

    await drag(line(5000, 15000, 15000));
    expect(win.request()).toBe(true);
    await tick();
    expect(target.querySelector('[role="alertdialog"]')).toBeTruthy();
    button('Rester').click();
    await tick();
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(win.destroy).not.toHaveBeenCalled();

    expect(win.request()).toBe(true);
    await tick();
    button('Fermer sans enregistrer').click();
    await vi.waitFor(() => expect(win.destroy).toHaveBeenCalledTimes(1));
    expect(onclose).not.toHaveBeenCalled();
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(1);
  });

  it('saves, then closes the window, from the close confirmation', async () => {
    const win = fakeWindow();
    const { path, button, drag, onsaved } = await render('Liste de courses', { windowClose: win.closer });
    await drag(line(5000, 15000, 15000));
    expect(win.request()).toBe(true);
    await tick();
    button('Enregistrer et fermer').click();
    await vi.waitFor(() => expect(win.destroy).toHaveBeenCalledTimes(1));
    expect(onsaved).toHaveBeenCalledWith(path);
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(2);
    // The close request listener goes away with the editor.
    expect(win.unlisten).not.toHaveBeenCalled();
    cleanup?.();
    cleanup = null;
    expect(win.unlisten).toHaveBeenCalledTimes(1);
  });

  it('asks the browser to confirm leaving the page with unsaved changes', async () => {
    const { drag } = await render('Liste de courses');
    const unload = () => {
      const event = new Event('beforeunload', { cancelable: true });
      window.dispatchEvent(event);
      return event.defaultPrevented;
    };
    expect(unload()).toBe(false);
    await drag(line(5000, 15000, 15000));
    expect(unload()).toBe(true);
    cleanup?.();
    cleanup = null;
    expect(unload()).toBe(false);
  });

  it('saves the edits as a copy in the same binder after a conflict', async () => {
    const { path, target, drag, key, button, status, onsaved } = await render('Cours de physique');
    await drag(line(5000, 15000, 15000));
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeTruthy());
    button('Enregistrer comme copie').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const copyPath = onsaved.mock.calls[0][0] as string;
    expect(copyPath).not.toBe(path);
    const copy = mockLibrary().notes.get(copyPath)!;
    expect(copy.binder).toBe('Classe/3e B');
    expect(copy.note.title).toBe('Cours de physique (copie)');
    expect(copy.note.strokes).toHaveLength(3);
    expect(copy.note.id).not.toBe(mockLibrary().notes.get(path)!.note.id);
    // The original is untouched; the editor now edits the copy.
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(2);
    expect(target.querySelector('[role="alert"]')).toBeNull();
    expect(status()).toBe('Enregistrée');
    expect(target.querySelector('.editor')?.getAttribute('data-path')).toBe(copyPath);
    expect(target.querySelector<HTMLInputElement>('.title-field input')?.value).toBe('Cours de physique (copie)');
    await drag(line(5000, 15000, 18000));
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledTimes(2));
    expect(onsaved).toHaveBeenLastCalledWith(copyPath);
    expect(mockLibrary().notes.get(copyPath)!.note.strokes).toHaveLength(4);
  });

  it('offers to save as a copy when the note was deleted meanwhile', async () => {
    const { path, target, drag, button, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    mockLibrary().notes.delete(path);
    button('Enregistrer').click();
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')?.textContent).toContain("n'existe plus"));
    button('Enregistrer comme copie').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const copy = mockLibrary().notes.get(onsaved.mock.calls[0][0] as string)!;
    expect(copy.binder).toBe('');
    expect(copy.note.strokes).toHaveLength(2);
  });

  it('notices when the open note changes on disk, and reloads it', async () => {
    const { path, target, paths, button, drag, onsaved } = await render('Liste de courses');
    // Its own saves do not count.
    await drag(line(5000, 15000, 15000));
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    await flush();
    await flush();
    expect(target.querySelector('[data-disk]')).toBeNull();

    const stored = mockLibrary().notes.get(path)!;
    stored.note = { ...stored.note, strokes: [] };
    stored.stamp = freshStamp();
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="changed"]')).toBeTruthy());
    expect(target.querySelector('[data-disk]')?.textContent).toContain('Rechargez-la');
    button('Recharger').click();
    await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
    await vi.waitFor(() => expect(paths()).toHaveLength(0));
    expect(target.querySelector('[data-disk]')).toBeNull();
  });

  it('warns that saving would conflict when the note changes on disk with unsaved edits', async () => {
    const { path, target, button, drag, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="changed"]')).toBeTruthy());
    expect(target.querySelector('[data-disk]')?.textContent).toContain('serait refusé');
    button('Enregistrer comme copie').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const copyPath = onsaved.mock.calls[0][0] as string;
    expect(mockLibrary().notes.get(copyPath)!.note.strokes).toHaveLength(2);
    expect(target.querySelector('[data-disk]')).toBeNull();
  });

  it('notices when the open note disappears from the library', async () => {
    const { path, target } = await render('Liste de courses');
    mockLibrary().notes.delete(path);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(target.querySelector('[data-disk="missing"]')).toBeTruthy());
    expect(target.querySelector('[data-disk]')?.textContent).toContain('Enregistrer comme copie');
  });

  it('saves after a pending rename, with the new path and stamp', async () => {
    const { path, target, drag, button, typeTitle, blurTitle, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    typeTitle('Courses du samedi');
    blurTitle();
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledTimes(2));
    const renamed = onsaved.mock.calls[0][0] as string;
    expect(renamed).not.toBe(path);
    expect(onsaved.mock.calls[1][0]).toBe(renamed);
    expect(target.querySelector('[role="alert"]')).toBeNull();
    const stored = mockLibrary().notes.get(renamed)!.note;
    expect(stored.title).toBe('Courses du samedi');
    expect(stored.strokes).toHaveLength(2);
  });

  it('commits a typed title before saving with Ctrl+S from the title field', async () => {
    const { drag, typeTitle, titleInput, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    typeTitle('Courses du dimanche');
    titleInput().dispatchEvent(new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalledTimes(2));
    const stored = mockLibrary().notes.get(onsaved.mock.calls[1][0] as string)!.note;
    expect(stored.title).toBe('Courses du dimanche');
    expect(stored.strokes).toHaveLength(2);
  });

  it('calls no one back once closed: a rename finishing after Back', async () => {
    const { button, typeTitle, blurTitle, onclose, onsaved } = await render('Liste de courses');
    typeTitle('Courses du samedi');
    blurTitle();
    button('Bibliothèque').click();
    expect(onclose).toHaveBeenCalledTimes(1);
    await flush();
    await flush();
    // The rename itself went through.
    expect([...mockLibrary().notes.keys()].some((p) => p.includes('Courses du samedi'))).toBe(true);
    expect(onsaved).not.toHaveBeenCalled();
  });

  it('calls no one back once destroyed: a save finishing after unmount', async () => {
    const { path, drag, button, onsaved } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    button('Enregistrer').click();
    cleanup?.();
    cleanup = null;
    await flush();
    await flush();
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(2);
    expect(onsaved).not.toHaveBeenCalled();
  });

  it('keeps crops from deleting strokes on hidden layers', async () => {
    const { path, button, input, drag, onsaved } = await render('Liste de courses');
    button('+ Ajouter un calque').click();
    await tick();
    await drag(line(20000, 25000, 18000));
    input('Afficher « Calque 2 »').click();
    await tick();
    button('Recadrer autour du contenu').click();
    await tick();
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const stored = mockLibrary().notes.get(path)!.note;
    expect(stored.page.width).toBeLessThan(29700);
    expect(stored.strokes.map((s) => s.layer)).toEqual([0, 1]);
  });

  it('never draws on a hidden layer', async () => {
    const { target, paths, input, button, drag } = await render('Cours de physique');
    button('+ Ajouter un calque').click();
    await tick();
    // Hiding the active layer moves drawing to the visible one below.
    input('Afficher « Calque 2 »').click();
    await tick();
    expect(input('Dessiner sur « Calque 1 »').checked).toBe(true);
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(3);
    // Nothing visible: the pen draws nothing and says why.
    input('Afficher « Calque 1 »').click();
    await tick();
    await drag(line(5000, 15000, 18000));
    expect(paths()).toHaveLength(0);
    expect(target.textContent).toContain('Le calque actif est masqué');
    input('Afficher « Calque 1 »').click();
    await tick();
    expect(paths()).toHaveLength(3);
  });

  it('lets Space toggle a checkbox, and pans elsewhere', async () => {
    const { svg, input } = await render('Cours de physique');
    const box = input('Afficher « Calque 1 »');
    box.focus();
    const onBox = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    box.dispatchEvent(onBox);
    await tick();
    expect(onBox.defaultPrevented).toBe(false);
    expect(svg.classList.contains('space')).toBe(false);
    box.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', bubbles: true }));
    const elsewhere = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    window.dispatchEvent(elsewhere);
    await tick();
    expect(elsewhere.defaultPrevented).toBe(true);
    expect(svg.classList.contains('space')).toBe(true);
    window.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', bubbles: true }));
  });

  it('recovers when the canvas loses the pointer capture mid-stroke', async () => {
    const { path, target, pointer, drag, button, onsaved } = await render('Liste de courses');
    pointer('pointerdown', 5000, 15000);
    pointer('pointermove', 8000, 15000);
    pointer('lostpointercapture', 8000, 15000);
    await tick();
    expect(target.querySelector('path.live')).toBeNull();
    await drag(line(5000, 15000, 18000));
    button('Enregistrer').click();
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
    const strokes = mockLibrary().notes.get(path)!.note.strokes;
    expect(strokes).toHaveLength(2);
    expect(strokes[1].points[0].y).toBeCloseTo(18000, -1);
  });

  it('ignores undo while erasing, so that the undo history stays consistent', async () => {
    const { paths, pointer, drag, key, button } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    await drag(line(5000, 15000, 18000));
    expect(paths()).toHaveLength(3);
    button('Gomme').click();
    await tick();
    pointer('pointerdown', 4000, 15000);
    pointer('pointermove', 10000, 15000);
    await key('z', { ctrlKey: true });
    pointer('pointermove', 16000, 15000);
    pointer('pointerup', 16000, 15000);
    await tick();
    expect(paths()).toHaveLength(2);
    // Undo restores the erased stroke, then removes the drawn strokes in reverse order.
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(3);
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(2);
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(1);
  });

  it('reports a note that cannot be opened', async () => {
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(Editor, { target, props: { path: 'C:\\demo\\Ardoise\\nope.ardoise', app, onclose: () => {} } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeTruthy());
    expect(target.textContent).toContain("Impossible d'ouvrir cette note");
  });
});

describe('note editor: split, copy image and export', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    mockClipboard.available = true;
  });

  const panelOf = (target: HTMLElement) => target.querySelector<HTMLElement>('.split-panel');
  const slider = (target: HTMLElement) => target.querySelector<HTMLInputElement>('.split-panel input[type="range"]')!;
  const titleField = (target: HTMLElement) => target.querySelector<HTMLInputElement>('.split-panel input[type="text"]')!;
  const moved = (target: HTMLElement) => target.querySelectorAll('g.ink path.split-moved');
  const kept = (target: HTMLElement) => target.querySelectorAll('g.ink path.split-kept');
  const notice = (target: HTMLElement) => target.querySelector('.editor-notice');
  const panelButton = (target: HTMLElement, label: string) => {
    const found = [...target.querySelectorAll<HTMLButtonElement>('.split-panel button')].find((b) => b.textContent?.trim() === label);
    if (!found) throw new Error(`no panel button "${label}"`);
    return found;
  };
  const setAt = async (target: HTMLElement, value: number) => {
    const input = slider(target);
    input.value = String(value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
  };
  const chooseZone = async (target: HTMLElement) => {
    [...target.querySelectorAll<HTMLLabelElement>('.split-panel label.choice')]
      .find((l) => /Selon une zone|By zone/.test(l.textContent ?? ''))!
      .querySelector('input')!
      .click();
    await tick();
  };
  /** The mock library's notes other than the three demo ones. */
  const created = () => [...mockLibrary().notes.values()].filter((s) => !s.note.id.startsWith('b1f0c6c2'));

  it('splits at an instant: live highlight, new note, kept part shown saved with a fresh history', async () => {
    const splitSpy = vi.spyOn(api, 'splitNote');
    const { path, target, paths, button, drag, status, onsaved } = await render('Cours de physique');
    // An unsaved stroke written after the two demo ones (their samples end at 295 ms).
    await drag(line(5000, 15000, 15000));
    expect(status()).toBe('Modifications non enregistrées');
    expect(panelOf(target)).toBeNull();

    button('Découper').click();
    await tick();
    expect(panelOf(target)).toBeTruthy();
    expect(slider(target).min).toBe('0');
    expect(Number(slider(target).max)).toBeGreaterThanOrEqual(795);
    expect(panelOf(target)!.textContent).toContain('enregistrées avec la découpe');

    // At 150 ms the two demo strokes are cut in two and the drawn stroke leaves entirely.
    await setAt(target, 150);
    expect(target.querySelector('.split-time')?.textContent).toContain('0:00.1');
    expect(moved(target)).toHaveLength(3);
    expect(kept(target)).toHaveLength(2);
    expect(target.querySelector('.split-counts')?.textContent).toBe('Traits qui restent ici : 2 · traits qui partent : 3');
    expect(titleField(target).value).toBe('Cours de physique (suite)');

    // Moving the slider moves the highlight: past the demo strokes only the drawn one leaves.
    await setAt(target, 400);
    expect(moved(target)).toHaveLength(1);
    expect(kept(target)).toHaveLength(2);
    await setAt(target, 150);

    panelButton(target, 'Découper').click();
    await vi.waitFor(() => expect(panelOf(target)).toBeNull());
    expect(splitSpy).toHaveBeenCalledTimes(1);
    expect(splitSpy).toHaveBeenCalledWith(
      path,
      expect.objectContaining({ title: 'Cours de physique' }),
      expect.anything(),
      { kind: 'time', at: 150 },
      'Cours de physique (suite)',
    );
    // The editor shows what is kept, saved, with nothing left to undo.
    expect(paths()).toHaveLength(2);
    expect(moved(target)).toHaveLength(0);
    expect(status()).toBe('Enregistrée');
    expect(button('Annuler (Ctrl+Z)').disabled).toBe(true);
    expect(notice(target)?.textContent).toBe('Nouvelle note créée : Cours de physique (suite)');
    expect(onsaved).toHaveBeenCalledWith(path);

    const stored = mockLibrary().notes.get(path)!.note;
    expect(stored.strokes.map((s) => s.points.length)).toEqual([30, 30]);
    const [other] = created();
    expect(other.note.title).toBe('Cours de physique (suite)');
    expect(other.note.strokes).toHaveLength(3);
    expect(other.binder).toBe('Classe/3e B');
  });

  it('proposes English titles in English', async () => {
    setLocale('en');
    const { target, button } = await render('Cours de physique');
    button('Split').click();
    await tick();
    expect(titleField(target).value).toBe('Cours de physique (continued)');
    await chooseZone(target);
    expect(titleField(target).value).toBe('Cours de physique (excerpt)');
    expect(panelOf(target)!.textContent).toContain('Trace the zone to cut out');
  });

  it('keeps a title the user typed when the mode changes', async () => {
    const { target, button } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    const field = titleField(target);
    field.value = 'Ma suite';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await chooseZone(target);
    expect(titleField(target).value).toBe('Ma suite');
  });

  it('splits by zone with the lasso rule', async () => {
    const splitSpy = vi.spyOn(api, 'splitNote');
    const { path, target, paths, button, drag } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    await chooseZone(target);
    expect(slider(target)).toBeNull();
    expect(titleField(target).value).toBe('Cours de physique (extrait)');
    // Nothing traced yet: nothing highlighted, the confirm button waits.
    expect(moved(target)).toHaveLength(0);
    expect(kept(target)).toHaveLength(0);
    expect(panelButton(target, 'Découper').disabled).toBe(true);

    // The zone encloses the first demo stroke (a wave around y = 3000) only.
    await drag([
      [1500, 2000],
      [23000, 2000],
      [23000, 4500],
      [1500, 4500],
      [1500, 2000],
    ]);
    expect(target.querySelector('polygon.split-zone')).toBeTruthy();
    expect(moved(target)).toHaveLength(1);
    expect(moved(target)[0].getAttribute('data-stroke')).toBe('0');
    expect(kept(target)).toHaveLength(1);

    panelButton(target, 'Découper').click();
    await vi.waitFor(() => expect(panelOf(target)).toBeNull());
    const spec = splitSpy.mock.calls[0][3];
    expect(spec.kind).toBe('zone');
    expect(spec.kind === 'zone' && spec.polygon).toHaveLength(5);
    expect(splitSpy.mock.calls[0][4]).toBe('Cours de physique (extrait)');
    expect(paths()).toHaveLength(1);
    expect(mockLibrary().notes.get(path)!.note.strokes.map((s) => s.tool)).toEqual(['fountainPen']);
    expect(created()[0].note.strokes.map((s) => s.tool)).toEqual(['ballpoint']);
  });

  it('keeps the page read-only while the panel is open', async () => {
    const { target, paths, button, drag } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    const before = paths().length;
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(before);
    expect(target.querySelector('g.ink path.live')).toBeNull();
    button('Annuler (Ctrl+Z)').click();
    await tick();
    expect(button('Enregistrer').disabled).toBe(true);
  });

  it('refuses a cut that leaves one side empty and explains why', async () => {
    const splitSpy = vi.spyOn(api, 'splitNote');
    const { target, button } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    await setAt(target, 0);
    expect(panelButton(target, 'Découper').disabled).toBe(true);
    expect(panelOf(target)!.textContent).toContain('Rien à découper ainsi');
    await setAt(target, 100);
    expect(panelButton(target, 'Découper').disabled).toBe(false);
    expect(panelOf(target)!.textContent).not.toContain('Rien à découper ainsi');
    expect(splitSpy).not.toHaveBeenCalled();
  });

  it('cancels with Escape or the cancel button, leaving everything untouched', async () => {
    const splitSpy = vi.spyOn(api, 'splitNote');
    const { target, paths, button, key, status } = await render('Cours de physique');
    const before = mockLibrary().notes.size;
    button('Découper').click();
    await tick();
    await setAt(target, 150);
    expect(moved(target).length).toBeGreaterThan(0);
    await key('Escape');
    expect(panelOf(target)).toBeNull();
    expect(moved(target)).toHaveLength(0);
    expect(kept(target)).toHaveLength(0);

    button('Découper').click();
    await tick();
    panelButton(target, 'Annuler').click();
    await tick();
    expect(panelOf(target)).toBeNull();
    expect(paths()).toHaveLength(2);
    expect(status()).toBe('Enregistrée');
    expect(splitSpy).not.toHaveBeenCalled();
    expect(mockLibrary().notes.size).toBe(before);
  });

  it('closes the panel from the toolbar button too', async () => {
    const { target, button } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    expect(button('Découper').getAttribute('aria-pressed')).toBe('true');
    button('Découper').click();
    await tick();
    expect(panelOf(target)).toBeNull();
  });

  it('ignores the editing keys while the panel is open', async () => {
    const { target, paths, button, drag, key } = await render('Cours de physique');
    await drag(line(5000, 15000, 15000));
    expect(paths()).toHaveLength(3);
    button('Découper').click();
    await tick();
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(3);
    expect(panelOf(target)).toBeTruthy();
  });

  it('keeps the note and says why when the file changed meanwhile', async () => {
    const { path, target, paths, button, status } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    await setAt(target, 150);
    // Someone else saved the note: its stamp is no longer the one the editor knows.
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    const before = mockLibrary().notes.size;
    panelButton(target, 'Découper').click();
    await vi.waitFor(() => expect(panelOf(target)!.querySelector('[role="alert"]')).toBeTruthy());
    expect(panelOf(target)!.querySelector('[role="alert"]')!.textContent).toContain(
      'Impossible de découper la note : la note a été modifiée entre-temps',
    );
    expect(panelButton(target, 'Découper').disabled).toBe(false);
    // Still showing the preview of the same note, which was not touched.
    expect(moved(target)).toHaveLength(2);
    expect(paths()).toHaveLength(4);
    expect(status()).toBe('Enregistrée');
    expect(mockLibrary().notes.size).toBe(before);
    expect(mockLibrary().notes.get(path)!.note.strokes.map((s) => s.points.length)).toEqual([60, 60]);
  });

  it('shows the refusal of the desktop app when nothing can be split', async () => {
    vi.spyOn(api, 'splitNote').mockRejectedValueOnce({ code: 'nothingToSplit', message: 'x' });
    const { target, button } = await render('Cours de physique');
    button('Découper').click();
    await tick();
    panelButton(target, 'Découper').click();
    await vi.waitFor(() => expect(panelOf(target)!.querySelector('[role="alert"]')).toBeTruthy());
    expect(panelOf(target)!.querySelector('[role="alert"]')!.textContent).toContain('cette découpe ne laisserait rien');
  });

  it('says so when the note is empty', async () => {
    const { target, button, key } = await render('Cours de physique');
    button('Sélection').click();
    await tick();
    await key('a', { ctrlKey: true });
    await key('Delete');
    button('Découper').click();
    await tick();
    expect(panelOf(target)!.textContent).toContain('Cette note est vide');
    expect(slider(target)).toBeNull();
  });

  it('copies the note as an image at 150 dpi', async () => {
    const copySpy = vi.spyOn(api, 'copyNoteImage');
    const { path, target, button } = await render('Liste de courses');
    button("Copier l'image").click();
    await vi.waitFor(() => expect(notice(target)?.textContent).toContain('Image copiée'));
    expect(copySpy).toHaveBeenCalledWith({ path, note: expect.objectContaining({ title: 'Liste de courses' }) }, 150);
    expect(notice(target)?.getAttribute('role')).toBe('status');
  });

  it('says in plain words when the clipboard is unavailable', async () => {
    mockClipboard.available = false;
    const { target, button } = await render('Liste de courses');
    button("Copier l'image").click();
    await vi.waitFor(() => expect(notice(target)).toBeTruthy());
    expect(notice(target)?.getAttribute('role')).toBe('alert');
    expect(notice(target)?.textContent).toContain("Impossible de copier l'image : le presse-papiers n'est pas disponible");
  });

  it('exports the note as it is in the editor, unsaved changes included', async () => {
    const exportSpy = vi.spyOn(api, 'exportNotes');
    const { path, target, drag, button, key, paths } = await render('Liste de courses');
    await drag(line(5000, 15000, 15000));
    button('Exporter…').click();
    await tick();
    const dialog = target.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog).toBeTruthy();
    expect(dialog.textContent).toContain('Note : Liste de courses');

    // The editing keys wait while the dialog is open.
    await key('a', { ctrlKey: true });
    await key('Delete');
    expect(paths()).toHaveLength(2);

    [...dialog.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Exporter…')!.click();
    await vi.waitFor(() => expect(exportSpy).toHaveBeenCalled());
    const [sources, format, options, destination] = exportSpy.mock.calls[0];
    expect(sources[0].path).toBe(path);
    expect(sources[0].note?.strokes).toHaveLength(2);
    expect(format).toBe('png');
    expect(options).toEqual({ dpi: 150, background: true, quality: 90, pdfPage: 'actual' });
    expect(destination).toMatch(/Liste de courses\.png$/);
    await vi.waitFor(() => expect(dialog.textContent).toContain('Fichiers créés : 1'));

    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await tick();
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(paths()).toHaveLength(2);
  });
});
