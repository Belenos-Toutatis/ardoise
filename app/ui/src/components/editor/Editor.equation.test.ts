// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The equation tool in the editor (SPEC §4.4 « Équation », docs/design/equations.md), with the real MathJax: a tap opens
 * the panel there, the preview is drawn from our own path data as the code is typed, an invalid code shows MathJax's
 * error and cannot be placed, « Poser » puts the equation down as one undoable step (selected), Escape drops it, the keys
 * typed never reach the editor's shortcuts, a double click (or a tap of the tool) on an equation reopens its source,
 * and the size and the colour act on the selected equations.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { readEquationPath } from '../../lib/equation-path';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { EquationObject, PageObject } from '../../lib/note';
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
  vi.restoreAllMocks();
  setLocale('fr');
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function render(title = TITLE) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = (svg.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
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
  const tap = async (x: number, y: number) => {
    pointer('pointerdown', x, y);
    pointer('pointerup', x, y);
    await tick();
    await tick();
  };
  const dblclick = async (x: number, y: number) => {
    const at = client(x, y);
    svg.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y }));
    await tick();
  };
  const key = async (k: string, init: KeyboardEventInit = {}, on: EventTarget = window) => {
    const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
    on.dispatchEvent(event);
    on.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
    return !event.defaultPrevented;
  };
  const panel = () => q('[data-slot="equation-panel"]');
  const field = () => q<HTMLTextAreaElement>('[data-slot="equation-source"]');
  const status = () => q('[data-slot="equation-status"]')?.textContent?.trim() ?? '';
  const previewPath = () => q('[data-slot="equation-preview"] path')?.getAttribute('d') ?? null;
  const placeButton = () => q<HTMLButtonElement>('[data-action="equation-place"]')!;
  /** Types `latex` in the panel's field (the whole of it). */
  const write = async (latex: string) => {
    const f = field()!;
    f.value = latex;
    f.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
  };
  /** Waits for the preview of what is typed (the conversion waits for the typing to pause). */
  const drawn = () => vi.waitFor(() => expect(previewPath()).toMatch(/^M/), { timeout: 10_000 });
  const equations = () => [...target.querySelectorAll('g.ink path[data-kind="equation"]')];
  const tool = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === label)!;
  const mode = () => q('.toolbar .tool[aria-pressed="true"]')?.getAttribute('title');
  const dirty = () => q('section.editor')!.getAttribute('data-dirty');
  const count = () => q('[data-section="selection"] .pane-help')?.textContent?.trim() ?? null;
  const undoButton = () => [...target.querySelectorAll<HTMLButtonElement>('.editor-top button')].find((b) => b.title.startsWith('Annuler'))!;
  const saved = async () => {
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(dirty()).toBe('no'));
    return (mockLibrary().notes.get(demoPath(title))!.note.objects ?? []) as PageObject[];
  };
  return { target, q, tap, dblclick, key, panel, field, status, previewPath, placeButton, write, drawn, equations, tool, mode, dirty, count, undoButton, saved };
}

/** Puts `latex` down at (x, y) with the equation tool, and waits until it is on the page. */
async function placeAt(v: Awaited<ReturnType<typeof render>>, x: number, y: number, latex: string) {
  v.tool('Équation').click();
  await tick();
  await v.tap(x, y);
  await v.write(latex);
  await v.drawn();
  v.placeButton().click();
  await tick();
}

describe('the equation tool', () => {
  it('opens its panel where the page is tapped, previews the LaTeX as paths and puts it down as one step', async () => {
    const v = await render();
    v.tool('Équation').click();
    await tick();
    expect(v.mode()).toBe('Équation');
    expect(v.q('[data-slot="equation-options"]')).not.toBeNull();
    await v.tap(6000, 9000);
    expect(v.panel()).not.toBeNull();
    expect(document.activeElement).toBe(v.field());
    expect(v.q('label[for="equation-source"]')?.textContent).toBe('Code LaTeX (mode mathématique)');
    expect(v.placeButton().textContent?.trim()).toBe('Poser');
    // The keys typed in the field are the field's: « p » does not take the pen.
    expect(await v.key('p', {}, v.field()!)).toBe(true);
    expect(v.mode()).toBe('Équation');
    await v.write('\\ce{2H2 + O2 -> 2H2O}');
    await v.drawn();
    const preview = v.previewPath()!;
    expect(readEquationPath(preview)).not.toBeNull();
    expect(v.q('[data-slot="equation-preview"] svg')?.getAttribute('aria-label')).toContain('\\ce{2H2 + O2 -> 2H2O}');
    // Nothing is on the page until it is put down.
    expect(v.equations()).toHaveLength(0);
    expect(v.dirty()).toBe('no');
    v.placeButton().click();
    await tick();
    expect(v.panel()).toBeNull();
    expect(v.equations()).toHaveLength(1);
    expect(v.equations()[0].getAttribute('d')).toBe(preview);
    expect(v.count()).toBe('Traits sélectionnés : 0 — textes, équations et signets : 1');
    const objects = await v.saved();
    const equation = objects.find((o) => o.kind === 'equation') as EquationObject;
    expect(equation.latex).toBe('\\ce{2H2 + O2 -> 2H2O}');
    expect(equation.path).toBe(preview);
    expect(equation.scale).toBeCloseTo(0.6, 6);
    // Its left end where the page was tapped, its middle at the tap's height.
    expect(equation.x).toBeCloseTo(6000, 3);
    expect(equation.y + (equation.height * equation.scale) / 2).toBeCloseTo(9000, 3);
    expect(equation.color).toEqual({ r: 0, g: 0, b: 0, a: 255 });
    // One step: undo takes it away.
    v.undoButton().click();
    await tick();
    expect(v.equations()).toHaveLength(0);
  });

  it('shows MathJax’s error for invalid code, which cannot be put down; Enter puts a valid one down', async () => {
    const v = await render();
    v.tool('Équation').click();
    await tick();
    await v.tap(6000, 9000);
    await v.write('\\frac{1');
    await vi.waitFor(() => expect(v.status()).toBe('Code LaTeX incorrect : Missing close brace'), { timeout: 10_000 });
    expect(v.previewPath()).toBeNull();
    expect(v.placeButton().disabled).toBe(true);
    expect(await v.key('Enter', {}, v.field()!)).toBe(false);
    expect(v.panel()).not.toBeNull();
    expect(v.equations()).toHaveLength(0);
    // Corrected, Enter puts it down; Shift + Enter would have been a new line.
    await v.write('\\frac{1}{2}');
    await v.drawn();
    expect(v.status()).toBe('');
    expect(await v.key('Enter', { shiftKey: true }, v.field()!)).toBe(true);
    expect(v.panel()).not.toBeNull();
    await v.key('Enter', {}, v.field()!);
    expect(v.panel()).toBeNull();
    expect(v.equations()).toHaveLength(1);
  });

  it('drops the equation being written with Escape, the page unchanged, then Escape goes on by steps', async () => {
    const v = await render();
    v.tool('Équation').click();
    await tick();
    await v.tap(6000, 9000);
    await v.write('x^2');
    await v.drawn();
    expect(await v.key('Escape', {}, v.field()!)).toBe(false);
    expect(v.panel()).toBeNull();
    expect(v.equations()).toHaveLength(0);
    expect(v.dirty()).toBe('no');
    // Another tool closes it too.
    await v.tap(6000, 9000);
    expect(v.panel()).not.toBeNull();
    v.tool('Stylo').click();
    await tick();
    expect(v.panel()).toBeNull();
  });

  it('reopens an equation’s source by a double click or a tap of the tool, and updates it in place as one step', async () => {
    const v = await render();
    await placeAt(v, 6000, 9000, 'v = \\dfrac{d}{t}');
    const before = (await v.saved()).find((o) => o.kind === 'equation') as EquationObject;
    const box = { x: before.x, y: before.y, w: before.width * before.scale, h: before.height * before.scale };
    v.tool('Sélection').click();
    await tick();
    await v.dblclick(box.x + box.w / 2, box.y + box.h / 2);
    expect(v.mode()).toBe('Équation');
    expect(v.field()!.value).toBe('v = \\dfrac{d}{t}');
    expect(v.placeButton().textContent?.trim()).toBe('Mettre à jour');
    await v.drawn();
    await v.write('v = \\dfrac{\\Delta x}{\\Delta t}');
    await v.drawn();
    v.placeButton().click();
    await tick();
    expect(v.equations()).toHaveLength(1);
    const after = (await v.saved()).find((o) => o.kind === 'equation') as EquationObject;
    expect(after.id).toBe(before.id);
    expect(after.latex).toBe('v = \\dfrac{\\Delta x}{\\Delta t}');
    expect(after.path).not.toBe(before.path);
    // Its top-left corner stays.
    expect(after.x).toBeCloseTo(before.x, 3);
    expect(after.y).toBeCloseTo(before.y, 3);
    // A tap of the tool on it reopens it too; undo brings the first source back.
    await v.tap(box.x + 20, box.y + box.h / 2);
    expect(v.field()!.value).toBe('v = \\dfrac{\\Delta x}{\\Delta t}');
    await v.key('Escape', {}, v.field()!);
    v.undoButton().click();
    await tick();
    const undone = (await v.saved()).find((o) => o.kind === 'equation') as EquationObject;
    expect(undone.latex).toBe('v = \\dfrac{d}{t}');
  });

  it('gives the selected equation the tool’s size and colour', async () => {
    const v = await render();
    await placeAt(v, 6000, 9000, 'E = mc^2');
    v.q<HTMLButtonElement>('[data-action="equation-size-large"]')!.click();
    await tick();
    v.q<HTMLButtonElement>('.swatch[aria-label="Rouge"]')!.click();
    await tick();
    const equation = (await v.saved()).find((o) => o.kind === 'equation') as EquationObject;
    expect(equation.scale).toBeCloseTo(0.9, 6);
    expect(equation.color).toEqual({ r: 0xd8, g: 0, b: 0, a: 255 });
    // The next one takes the size too.
    await v.tap(6000, 15000);
    await v.write('F = ma');
    await v.drawn();
    v.placeButton().click();
    await tick();
    const all = (await v.saved()).filter((o): o is EquationObject => o.kind === 'equation');
    expect(all).toHaveLength(2);
    expect(all[1].scale).toBeCloseTo(0.9, 6);
  });

  it('keeps the source typed when the page is tapped again, even on another equation, and puts it down there', async () => {
    const v = await render();
    await placeAt(v, 6000, 9000, 'E = mc^2');
    const first = (await v.saved()).find((o) => o.kind === 'equation') as EquationObject;
    await v.tap(6000, 15000);
    await v.write('F = ma');
    // A tap on the first equation moves where the new one goes; its source stays in the field.
    await v.tap(first.x + 50, first.y + (first.height * first.scale) / 2);
    expect(v.field()!.value).toBe('F = ma');
    await v.drawn();
    v.placeButton().click();
    await tick();
    const all = (await v.saved()).filter((o): o is EquationObject => o.kind === 'equation');
    expect(all.map((o) => o.latex)).toEqual(['E = mc^2', 'F = ma']);
    expect(all[1].x).toBeCloseTo(first.x + 50, 3);
  });

  it('puts an equation deleted while its panel is open back as it was, rewritten', async () => {
    const v = await render();
    await placeAt(v, 6000, 9000, 'x^2');
    v.q<HTMLButtonElement>('.swatch[aria-label="Rouge"]')!.click();
    await tick();
    const before = (await v.saved()).find((o) => o.kind === 'equation') as EquationObject;
    v.tool('Sélection').click();
    await tick();
    await v.dblclick(before.x + 10, before.y + (before.height * before.scale) / 2);
    expect(v.field()!.value).toBe('x^2');
    // The page has the key now: Delete removes the selected equation, the one being rewritten.
    v.field()!.blur();
    await v.key('Delete');
    expect(v.equations()).toHaveLength(0);
    await v.write('x^3');
    await v.drawn();
    v.placeButton().click();
    await tick();
    const after = (await v.saved()).find((o) => o.kind === 'equation') as EquationObject;
    expect(after.latex).toBe('x^3');
    expect([after.x, after.y, after.scale, after.color]).toEqual([before.x, before.y, before.scale, before.color]);
  });

  it('speaks English', async () => {
    setLocale('en');
    const v = await render();
    v.tool('Equation').click();
    await tick();
    await v.tap(6000, 9000);
    expect(v.q('label[for="equation-source"]')?.textContent).toBe('LaTeX code (math mode)');
    expect(v.placeButton().textContent?.trim()).toBe('Place');
    await v.write('\\sqrt{');
    await vi.waitFor(() => expect(v.status()).toMatch(/^Invalid LaTeX code: /), { timeout: 10_000 });
  });
});
