// @vitest-environment jsdom
/// <reference types="node" />
// The editor's milestone M8 page features: portrait / landscape, paper size, and copying strokes
// to the system clipboard.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { encodeStrokes } from '../../lib/editor/clipboard';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { Stroke } from '../../lib/note';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

// The real stroke geometry, instantiated from the module's bytes (jsdom cannot fetch it).
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

async function render(title: string) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const onsaved = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn(), onsaved } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const page = () => [Number(svg.getAttribute('data-page-width')), Number(svg.getAttribute('data-page-height'))];
  const paths = () => [...target.querySelectorAll<SVGPathElement>('g.ink path:not(.live)')];
  const action = (name: string) => {
    const found = target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`);
    if (!found) throw new Error(`no button ${name}`);
    return found;
  };
  const field = (name: string) => target.querySelector<HTMLInputElement & HTMLSelectElement>(`[data-field="${name}"]`)!;
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const notice = () => target.querySelector('.editor-notice')?.textContent?.trim();
  const save = async () => {
    onsaved.mockClear();
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
  };
  const stored = () => mockLibrary().notes.get(path)!;
  return { path, target, page, paths, action, field, key, notice, save, stored };
}

/** Sets a control's value as the user would. */
async function set(control: HTMLInputElement | HTMLSelectElement, value: string, event = 'change') {
  control.value = value;
  control.dispatchEvent(new Event(event, { bubbles: true }));
  await tick();
}

/** A fake system clipboard and the clipboard events the browser raises for Ctrl+C / X / V. */
function systemClipboard(initial = '') {
  const box = { text: initial };
  const fire = async (type: 'copy' | 'cut' | 'paste') => {
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', {
      value: { setData: (_: string, value: string) => (box.text = value), getData: () => box.text },
    });
    document.body.dispatchEvent(event);
    await tick();
    return event.defaultPrevented;
  };
  return { box, fire };
}

const stroke = (x: number, y: number, layer = 0): Stroke => ({
  tool: 'feltTip',
  color: { r: 10, g: 20, b: 30, a: 255 },
  width: 77,
  layer,
  points: [0, 1, 2, 3].map((i) => ({ x: x + i * 100, y: y + i * 50, pressure: 0.7, t: i * 10 })),
});

describe('page orientation and paper size', () => {
  it('swaps portrait and landscape without turning the ink, and undoes it', async () => {
    const { page, action, key, save, stored, path } = await render('Liste de courses');
    const before = structuredClone(stored().note.strokes);
    expect(page()).toEqual([29700, 21600]);
    expect(action('orientation-landscape').getAttribute('aria-pressed')).toBe('true');
    action('orientation-portrait').click();
    await tick();
    expect(page()).toEqual([21600, 29700]);
    expect(action('orientation-portrait').getAttribute('aria-pressed')).toBe('true');
    await save();
    // The ink did not turn (and, fitting the portrait page, did not move either).
    expect(mockLibrary().notes.get(path)!.note.page).toEqual({ width: 21600, height: 29700 });
    expect(mockLibrary().notes.get(path)!.note.strokes).toEqual(before);
    await key('z', { ctrlKey: true });
    expect(page()).toEqual([29700, 21600]);
  });

  it('keeps ink that does not fit, outside the page, and says so', async () => {
    const path = demoPath('Liste de courses');
    const wide = { ...stroke(1000, 1000), points: [0, 1].map((i) => ({ x: 1000 + i * 26000, y: 1000, pressure: 0.5, t: i })) };
    mockLibrary().notes.get(path)!.note.strokes = [wide];
    const { page, action, notice } = await render('Liste de courses');
    action('orientation-portrait').click();
    await tick();
    expect(page()).toEqual([21600, 29700]);
    expect(notice()).toBe("Un trait dépasse de la feuille : il est conservé mais n'est pas affiché.");
  });

  it('keeps both turning and orientation, clearly labelled apart', async () => {
    const { target, page, action } = await render('Liste de courses');
    const text = target.textContent ?? '';
    expect(text).toContain('Format de la feuille');
    expect(text).toContain('Tourner et recadrer');
    expect(text).toContain("La feuille, l'écriture et l'image de fond tournent ensemble");
    action('rotate-cw').click();
    await tick();
    expect(page()).toEqual([21600, 29700]);
  });

  it('applies A4, A5, Letter and a typed size in the current orientation', async () => {
    const { page, action, field, save, stored, path } = await render('Liste de courses');
    const before = structuredClone(stored().note.strokes);
    await set(field('paper-size'), 'a4');
    expect(page()).toEqual([29700, 21000]);
    await set(field('paper-size'), 'a5');
    expect(page()).toEqual([21000, 14800]);
    await set(field('paper-size'), 'letter');
    expect(page()).toEqual([27940, 21590]);
    expect(field('paper-size').value).toBe('letter');

    await set(field('width-mm'), '250', 'input');
    await set(field('height-mm'), '180,5', 'input');
    action('apply-size').click();
    await tick();
    expect(page()).toEqual([25000, 18050]);
    expect(field('paper-size').value).toBe('custom');
    // Back to a size that holds the ink where it was: it never moved.
    await set(field('paper-size'), 'a4');
    await save();
    expect(mockLibrary().notes.get(path)!.note.strokes).toEqual(before);
  });

  it('refuses a size out of range', async () => {
    const { target, page, action, field } = await render('Liste de courses');
    await set(field('width-mm'), '5', 'input');
    expect(action('apply-size').disabled).toBe(true);
    expect(target.textContent).toContain('Entrez des dimensions entre 10 et 2000 mm.');
    expect(page()).toEqual([29700, 21600]);
  });

  it('scales the writing with the page when asked', async () => {
    const path = demoPath('Liste de courses');
    mockLibrary().notes.get(path)!.note.strokes = [stroke(0, 0)];
    const { field, save } = await render('Liste de courses');
    field('scale-ink').click();
    await tick();
    await set(field('paper-size'), 'a5');
    await save();
    const saved = mockLibrary().notes.get(path)!.note;
    // Landscape A5 is 21000 x 14800: 29700 x 21600 scaled by min(0.707, 0.685).
    const k = 14800 / 21600;
    expect(saved.page).toEqual({ width: 21000, height: 14800 });
    expect(saved.strokes[0].width).toBeCloseTo(77 * k, 6);
    expect(saved.strokes[0].points[1].x).toBeCloseTo(100 * k + (21000 - 29700 * k) / 2, 6);
  });
});

describe('copying strokes through the system clipboard', () => {
  it('copies the selection, cuts it, and pastes it offset, selected, in one undo step', async () => {
    const { paths, key, target } = await render('Liste de courses');
    const clipboard = systemClipboard();
    // Nothing selected: the copy is left to the browser.
    expect(await clipboard.fire('copy')).toBe(false);
    expect(clipboard.box.text).toBe('');
    await key('a', { ctrlKey: true });
    const count = paths().length;
    expect(await clipboard.fire('copy')).toBe(true);
    expect(clipboard.box.text).toContain('ardoise-strokes');
    // Pasting on the original: shifted, and the copies are the selection.
    expect(await clipboard.fire('paste')).toBe(true);
    expect(paths()).toHaveLength(count * 2);
    expect(target.textContent).toContain(`Traits sélectionnés : ${count}`);
    expect(await clipboard.fire('paste')).toBe(true);
    expect(paths()).toHaveLength(count * 3);
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(count * 2);
    // Cut: copied, then deleted, as one step.
    await key('a', { ctrlKey: true });
    expect(await clipboard.fire('cut')).toBe(true);
    expect(paths()).toHaveLength(0);
    expect(clipboard.box.text).toContain('ardoise-strokes');
    await key('z', { ctrlKey: true });
    expect(paths()).toHaveLength(count * 2);
  });

  it('pastes strokes copied elsewhere on the active layer, and ignores foreign clipboard content', async () => {
    const { paths, save, stored, path } = await render('Liste de courses');
    const before = paths().length;
    const foreign = systemClipboard('some text copied in a browser');
    expect(await foreign.fire('paste')).toBe(false);
    expect(paths()).toHaveLength(before);
    const fromOtherNote = systemClipboard(encodeStrokes([stroke(5000, 8000), stroke(5000, 9000)]));
    expect(await fromOtherNote.fire('paste')).toBe(true);
    expect(paths()).toHaveLength(before + 2);
    await save();
    const strokes = stored().note.strokes;
    const pasted = strokes[strokes.length - 2];
    expect(pasted).toMatchObject({ width: 77, color: { r: 10, g: 20, b: 30, a: 255 } });
    expect(pasted.points.map((p) => [p.x, p.y, p.pressure])).toEqual(
      [0, 1, 2, 3].map((i) => [5000 + i * 100, 8000 + i * 50, 0.7]),
    );
    expect(pasted.layer).toBe(0);
  });

  it('leaves the clipboard alone while typing', async () => {
    const { target, key } = await render('Liste de courses');
    await key('a', { ctrlKey: true });
    const field = target.querySelector<HTMLInputElement>('input[type="text"]')!;
    const clipboard = systemClipboard();
    const event = new Event('copy', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', { value: { setData: () => (clipboard.box.text = 'x'), getData: () => '' } });
    field.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(clipboard.box.text).toBe('');
  });

  it('copies, cuts and pastes from the selection panel buttons', async () => {
    const written: string[] = [];
    let held = '';
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: async (text: string) => {
          written.push(text);
          held = text;
        },
        readText: async () => held,
      },
    });
    const { paths, action, key } = await render('Liste de courses');
    const count = paths().length;
    await key('a', { ctrlKey: true });
    action('copy-selection').click();
    await vi.waitFor(() => expect(written).toHaveLength(1));
    action('paste-selection').click();
    await vi.waitFor(() => expect(paths()).toHaveLength(count * 2));
    action('cut-selection').click();
    await vi.waitFor(() => expect(paths()).toHaveLength(count));
    expect(written).toHaveLength(2);
  });

  it('falls back on the clipboard API when the browser raises no clipboard event for the keys', async () => {
    let held = '';
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: async (text: string) => (held = text), readText: async () => held },
    });
    const { paths, key } = await render('Liste de courses');
    const count = paths().length;
    await key('a', { ctrlKey: true });
    await key('c', { ctrlKey: true });
    await vi.waitFor(() => expect(held).toContain('ardoise-strokes'));
    await key('v', { ctrlKey: true });
    await vi.waitFor(() => expect(paths()).toHaveLength(count * 2));
    // A key that did get its event is not done twice.
    const clipboard = systemClipboard();
    await key('x', { ctrlKey: true });
    expect(await clipboard.fire('cut')).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(paths()).toHaveLength(count);
  });
});
