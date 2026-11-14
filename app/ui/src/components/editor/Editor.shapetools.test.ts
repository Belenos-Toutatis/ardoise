// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The shape tools in the editor (docs/design/shape-tools.md): the three tools of the toolbar and the three toggles that
 * appear with them (buttons, `aria-pressed`, FR and EN names), kept while the editor is open, the snapping toggle
 * enabled only on a page whose template has lines and following the template chosen on the Page tab, and the shapes
 * drawn through the whole editor (undo, saved as ordinary strokes). The geometry and the canvas are
 * `lib/editor/shape-tools.test.ts` and `Canvas.shapes.test.ts`.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { shapePrefs } from '../../lib/editor/shape-prefs.svelte';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import type { Template } from '../../lib/note';
import { share } from '../../lib/share-state.svelte';
import { openSideTab } from '../side/side-test-helpers';
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

const TITLE = 'Liste de courses';
let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockNarration();
  resetMockShare();
  share.reset();
  shapePrefs.reset();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
  share.reset();
  resetMockShare();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function render(template: Template = 'blank') {
  const path = demoPath(TITLE);
  const entry = mockLibrary().notes.get(path)!;
  entry.note = { ...entry.note, background: { ...entry.note.background, template } };
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose, onsaved: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const q = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const tool = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === label)!;
  const toggle = (name: string) => q<HTMLButtonElement>(`[data-slot="shape-options"] [data-action="shape-${name}"]`);
  const pressed = (name: string) => toggle(name)!.getAttribute('aria-pressed');
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = svg.getAttribute('data-view')!.split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  const pointer = (type: string, x: number, y: number) => {
    const at = client(x, y);
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    for (const [key, value] of Object.entries({ pointerId: 1, pressure: 0.5, pointerType: 'mouse', timeStamp: performance.now() })) {
      Object.defineProperty(event, key, { value, configurable: true });
    }
    svg.dispatchEvent(event);
  };
  const drag = async (x0: number, y0: number, x1: number, y1: number) => {
    pointer('pointerdown', x0, y0);
    for (let i = 1; i <= 4; i++) pointer('pointermove', x0 + ((x1 - x0) * i) / 4, y0 + ((y1 - y0) * i) / 4);
    pointer('pointerup', x1, y1);
    await tick();
  };
  const strokes = () => target.querySelectorAll('g.ink path:not(.live)').length;
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    await tick();
  };
  return { path, target, q, tool, toggle, pressed, pointer, drag, strokes, key, onclose };
}

describe('the three tools', () => {
  it('are in the toolbar next to the pen, with a drawn icon and a name, in French and in English', async () => {
    const fr = await render();
    const labels = [...fr.target.querySelectorAll<HTMLButtonElement>('.toolbar [aria-label="Outil"] .tool')].map((b) => b.title);
    expect(labels).toEqual(['Sélection', 'Stylo', 'Ligne', 'Rectangle', 'Ellipse', 'Texte', 'Équation', 'Gomme', 'Déplacer la page']);
    for (const name of ['Ligne', 'Rectangle', 'Ellipse']) {
      const button = fr.tool(name);
      expect(button.querySelector('svg')).not.toBeNull();
      expect(button.querySelector('svg')!.getAttribute('aria-hidden')).toBe('true');
      expect(button.getAttribute('aria-pressed')).toBe('false');
      expect(button.textContent?.trim()).toBe(name);
    }
    cleanup?.();
    setLocale('en');
    const en = await render();
    expect([...en.target.querySelectorAll<HTMLButtonElement>('.toolbar [aria-label="Tool"] .tool')].map((b) => b.title)).toEqual([
      'Select',
      'Pen',
      'Line',
      'Rectangle',
      'Ellipse',
      'Text',
      'Equation',
      'Eraser',
      'Move the page',
    ]);
  });

  it('choosing one presses it, and the ink, colour and width controls stay for it', async () => {
    const { tool, target } = await render();
    tool('Ellipse').click();
    await tick();
    expect(tool('Ellipse').getAttribute('aria-pressed')).toBe('true');
    expect(tool('Stylo').getAttribute('aria-pressed')).toBe('false');
    expect(target.querySelector('.toolbar select')).not.toBeNull();
    expect(target.querySelector('.toolbar input[type="range"]')).not.toBeNull();
    expect(target.querySelector('svg.canvas')!.getAttribute('class')).toContain('mode-ellipse');
  });
});

describe('the three toggles', () => {
  it('appear with a shape tool only, as buttons that say whether they are on', async () => {
    const { tool, q, toggle, pressed } = await render('grid5mm');
    expect(q('[data-slot="shape-options"]')).toBeNull();
    for (const name of ['Ligne', 'Rectangle', 'Ellipse']) {
      tool(name).click();
      await tick();
      expect(q('[data-slot="shape-options"]')!.getAttribute('role')).toBe('group');
      expect(q('[data-slot="shape-options"]')!.getAttribute('aria-label')).toBe('Options des formes');
      expect(['regular', 'centre', 'snap'].map((n) => toggle(n)!.textContent?.trim())).toEqual(['Forme régulière', 'Depuis le centre', 'Aimanter au quadrillage']);
      expect(['regular', 'centre', 'snap'].map(pressed)).toEqual(['false', 'false', 'false']);
    }
    tool('Stylo').click();
    await tick();
    expect(q('[data-slot="shape-options"]')).toBeNull();
  });

  it('are named in English', async () => {
    setLocale('en');
    const { tool, toggle } = await render('grid5mm');
    tool('Line').click();
    await tick();
    expect(['regular', 'centre', 'snap'].map((n) => toggle(n)!.textContent?.trim())).toEqual(['Regular shape', 'From the centre', 'Snap to the grid']);
  });

  it('turn on and off by touch or click, and are kept from one tool to the next and from one note to the next', async () => {
    const first = await render('grid5mm');
    first.tool('Rectangle').click();
    await tick();
    first.toggle('regular')!.click();
    first.toggle('snap')!.click();
    await tick();
    expect(['regular', 'centre', 'snap'].map(first.pressed)).toEqual(['true', 'false', 'true']);
    first.tool('Ellipse').click();
    await tick();
    expect(['regular', 'centre', 'snap'].map(first.pressed)).toEqual(['true', 'false', 'true']);
    first.toggle('regular')!.click();
    first.toggle('centre')!.click();
    await tick();
    expect(['regular', 'centre', 'snap'].map(first.pressed)).toEqual(['false', 'true', 'true']);
    // The editor opens again (another note): the toggles are as they were left.
    cleanup?.();
    const next = await render('grid5mm');
    next.tool('Ligne').click();
    await tick();
    expect(['regular', 'centre', 'snap'].map(next.pressed)).toEqual(['false', 'true', 'true']);
  });

  it('« Aimanter au quadrillage » is disabled on a page with no lines, and says why', async () => {
    const { tool, toggle, q } = await render('blank');
    tool('Ligne').click();
    await tick();
    const snap = toggle('snap')!;
    expect(snap.disabled).toBe(true);
    expect(snap.getAttribute('aria-pressed')).toBe('false');
    expect(snap.title).toBe("Ce fond n'a pas de lignes sur lesquelles aimanter : choisissez un fond ligné, Seyès, quadrillé, millimétré ou pointillé (onglet Page).");
    expect(q('[data-slot="shape-snap-hint"]')!.textContent).toBe('Pas de quadrillage sur ce fond');
    // The two others work.
    expect(toggle('regular')!.disabled).toBe(false);
    expect(toggle('centre')!.disabled).toBe(false);
    // A toggle left on does not show as pressed while it cannot work, and nothing snaps.
    shapePrefs.snap = true;
    await tick();
    expect(snap.getAttribute('aria-pressed')).toBe('false');
  });

  it('« Aimanter au quadrillage » is enabled once the page has a template with lines, and follows the template chosen', async () => {
    const asked = vi.spyOn(api, 'templateSnapGrid');
    const { tool, toggle, target, q } = await render('lined');
    tool('Rectangle').click();
    await tick();
    await vi.waitFor(() => expect(toggle('snap')!.disabled).toBe(false));
    expect(asked).toHaveBeenCalledWith({ width: 29_700, height: 21_600 }, 'lined');
    expect(q('[data-slot="shape-snap-hint"]')).toBeNull();
    // Another paper on the Page tab: blank has none.
    await openSideTab(target, 'page');
    const select = q<HTMLSelectElement>('[data-field="template"]')!;
    select.value = 'blank';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await vi.waitFor(() => expect(toggle('snap')!.disabled).toBe(true));
    select.value = 'seyes';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await vi.waitFor(() => expect(toggle('snap')!.disabled).toBe(false));
  });

  it('are disabled while the page cannot be edited (the cutting panel is open)', async () => {
    const { tool, toggle, target } = await render('grid5mm');
    tool('Ligne').click();
    await tick();
    [...target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.trim().endsWith('Découper'))!.click();
    await tick();
    expect(['regular', 'centre'].map((n) => toggle(n)!.disabled)).toEqual([true, true]);
  });
});

describe('Escape during a shape', () => {
  it('drops the shape being drawn: nothing is added, nothing is left on the page, and the editor stays', async () => {
    const { target, tool, pointer, key, strokes, onclose } = await render();
    const before = strokes();
    tool('Rectangle').click();
    await tick();
    pointer('pointerdown', 5000, 12_000);
    pointer('pointermove', 9000, 15_000);
    const live = () => target.querySelectorAll('g.ink path.live').length;
    await vi.waitFor(() => expect(live()).toBe(1));
    await key('Escape');
    expect(live()).toBe(0);
    pointer('pointerup', 9000, 15_000);
    await tick();
    expect(strokes()).toBe(before);
    expect(onclose).not.toHaveBeenCalled();
  });
});

describe('a shape in the note', () => {
  it('is one undoable change like any stroke, and saves with the note', async () => {
    const { target, tool, drag, strokes, path, key } = await render();
    const before = strokes();
    tool('Rectangle').click();
    await tick();
    await drag(5000, 12_000, 12_000, 15_000);
    expect(strokes()).toBe(before + 1);
    expect(target.querySelector('section.editor')!.getAttribute('data-dirty')).toBe('yes');
    await key('z', { ctrlKey: true });
    expect(strokes()).toBe(before);
    await key('y', { ctrlKey: true });
    expect(strokes()).toBe(before + 1);
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(target.querySelector('section.editor')!.getAttribute('data-dirty')).toBe('no'));
    const saved = mockLibrary().notes.get(path)!.note.strokes;
    expect(saved).toHaveLength(before + 1);
    // Closed: it ends where it began.
    const shape = saved.at(-1)!;
    expect(shape.points.at(-1)).toMatchObject({ x: shape.points[0].x, y: shape.points[0].y });
  });

  it('on a grid with the magnet, the corners are on the grid', async () => {
    shapePrefs.snap = true;
    const { target, tool, drag, path, key } = await render('grid5mm');
    tool('Rectangle').click();
    await tick();
    await vi.waitFor(() => expect(target.querySelector<HTMLButtonElement>('[data-action="shape-snap"]')!.disabled).toBe(false));
    await drag(5130, 12_240, 12_380, 15_070);
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(target.querySelector('section.editor')!.getAttribute('data-dirty')).toBe('no'));
    const shape = mockLibrary().notes.get(path)!.note.strokes.at(-1)!;
    const xs = shape.points.map((p) => p.x);
    const ys = shape.points.map((p) => p.y);
    // A4 landscape squared paper: lines every 500 units from 100 (x) and 50 (y).
    expect([Math.min(...xs), Math.max(...xs)].map((v) => Math.round(v))).toEqual([5100, 12_600]);
    expect([Math.min(...ys), Math.max(...ys)].map((v) => Math.round(v))).toEqual([12_050, 15_050]);
  });
});
