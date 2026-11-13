// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The text tool in the editor (SPEC §4.4 « Texte tapé au clavier », docs/design/page-objects.md §5): a tap puts a frame
 * where one types, the typing is one undoable step, Escape ends it with the text kept, an empty frame goes, the keys
 * typed never reach the editor's shortcuts, a tap on a frame (or a double click with the selection tool) types in it,
 * the family, size, bold and colour change the frame, and frames are selected, moved and deleted with the strokes.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { TextObject } from '../../lib/note';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

// The real geometry and text layout, from the module's bytes (jsdom cannot fetch it).
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
  const pointer = (type: string, x: number, y: number, extra: Record<string, unknown> = {}) => {
    const at = client(x, y);
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    for (const [key, value] of Object.entries({ pointerId: 1, pressure: 0.5, pointerType: 'mouse', timeStamp: performance.now(), ...extra })) {
      Object.defineProperty(event, key, { value, configurable: true });
    }
    svg.dispatchEvent(event);
  };
  /** A tap at (x, y), page units. */
  const tap = async (x: number, y: number) => {
    pointer('pointerdown', x, y);
    pointer('pointerup', x, y);
    await tick();
    await tick();
  };
  const drag = async (x0: number, y0: number, x1: number, y1: number) => {
    pointer('pointerdown', x0, y0);
    for (let i = 1; i <= 5; i++) pointer('pointermove', x0 + ((x1 - x0) * i) / 5, y0 + ((y1 - y0) * i) / 5);
    pointer('pointerup', x1, y1);
    await tick();
  };
  const key = async (k: string, init: KeyboardEventInit = {}, on: EventTarget = window) => {
    const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
    on.dispatchEvent(event);
    on.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
    return !event.defaultPrevented;
  };
  const field = () => q<HTMLTextAreaElement>('[data-slot="text-field"]');
  /** Types `text` in the frame's field (the whole of it). */
  const type = async (text: string) => {
    const f = field()!;
    f.value = text;
    f.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
  };
  const frames = () => target.querySelectorAll('g.ink path[data-object]');
  const strokes = () => target.querySelectorAll('g.ink path[data-stroke]').length;
  const tool = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === label)!;
  const mode = () => q('.toolbar .tool[aria-pressed="true"]')?.getAttribute('title');
  const dirty = () => q('section.editor')!.getAttribute('data-dirty');
  const count = () => q('[data-section="selection"] .pane-help')?.textContent?.trim() ?? null;
  const undoButton = () => [...target.querySelectorAll<HTMLButtonElement>('.editor-top button')].find((b) => b.title.startsWith('Annuler'))!;
  /** Saves (Ctrl + S) and gives the note's objects as written. */
  const saved = async () => {
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(dirty()).toBe('no'));
    return (mockLibrary().notes.get(demoPath(title))!.note.objects ?? []) as TextObject[];
  };
  return { target, svg, q, pointer, tap, drag, key, field, type, frames, strokes, tool, mode, dirty, count, undoButton, saved };
}

describe('the text tool', () => {
  it('puts a frame where the page is tapped; what is typed is drawn from the shipped glyphs', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    expect(v.mode()).toBe('Texte');
    expect(v.q('[data-slot="text-options"]')).not.toBeNull();
    await v.tap(6000, 9000);
    const f = v.field()!;
    expect(f).not.toBeNull();
    expect(document.activeElement).toBe(f);
    expect(f.getAttribute('aria-label')).toBe('Texte du cadre');
    await v.type('Bonjour, élève !\nDeuxième ligne');
    expect(v.frames()).toHaveLength(1);
    const d = v.frames()[0].getAttribute('d')!;
    expect(d.startsWith('M')).toBe(true);
    expect(d).toMatch(/[QC]/);
    // The tap is the middle of the first line, at its left end.
    const [, , , , x, y] = v.frames()[0].getAttribute('transform')!.match(/-?[\d.]+/g)!.map(Number);
    expect(x).toBeCloseTo(6000, 0);
    expect(y).toBeCloseTo(9000 - (600 * 1.25) / 2, 0);
  });

  it('ends the typing with Escape, the text kept and the frame selected, as one step that undo removes', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('B');
    await v.type('Bo');
    await v.type('Bonjour');
    expect(v.dirty()).toBe('yes');
    // Escape is typed in the field: the editor takes it first.
    expect(await v.key('Escape', {}, v.field()!)).toBe(false);
    expect(v.field()).toBeNull();
    expect(v.frames()).toHaveLength(1);
    expect(v.count()).toBe('Traits sélectionnés : 0 — textes : 1');
    // The next Escape deselects, as for strokes.
    await v.key('Escape');
    expect(v.count()).toBeNull();
    // One step for the whole typing.
    v.undoButton().click();
    await tick();
    expect(v.frames()).toHaveLength(0);
    expect(v.undoButton().disabled).toBe(true);
    const objects = await (async () => {
      await v.key('y', { ctrlKey: true });
      return v.saved();
    })();
    expect(objects).toHaveLength(1);
    expect(objects[0]).toMatchObject({ kind: 'text', text: 'Bonjour', font: 'latinModern', bold: false, size: 600, layer: 0, scale: 1, quarterTurns: 0 });
    expect(objects[0].width).toBeGreaterThan(0);
    expect(objects[0].id).not.toBe('');
  });

  it('drops a frame left empty, without a step', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('   ');
    await v.key('Escape', {}, v.field()!);
    expect(v.frames()).toHaveLength(0);
    expect(v.dirty()).toBe('no');
    expect(v.undoButton().disabled).toBe(true);
  });

  it('keeps every key typed in the frame away from the editor shortcuts', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('pe');
    const f = v.field()!;
    for (const [k, init] of [
      ['p', {}],
      ['e', {}],
      ['v', {}],
      ['h', {}],
      ['r', {}],
      ['f', {}],
      ['t', {}],
      [' ', {}],
      ['Delete', {}],
      ['Backspace', {}],
      ['a', { ctrlKey: true }],
      ['z', { ctrlKey: true }],
      ['0', { ctrlKey: true }],
    ] as [string, KeyboardEventInit][]) {
      expect(await v.key(k, init, f), k).toBe(true);
    }
    expect(v.mode()).toBe('Texte');
    expect(v.field()).not.toBeNull();
    expect(v.strokes()).toBe(1);
    expect(v.q('[data-section="selection"]')).toBeNull();
    expect(v.q('.replay-bar, [data-slot="replay-bar"]')).toBeNull();
  });

  it('types in a frame tapped with the text tool, or double clicked with the selection tool', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('Bonjour');
    await v.key('Escape', {}, v.field()!);
    // A tap on the frame: its own text, not a new frame.
    await v.tap(6100, 9000);
    expect(v.field()!.value).toBe('Bonjour');
    await v.type('Bonjour à tous');
    await v.key('Escape', {}, v.field()!);
    expect(v.frames()).toHaveLength(1);
    v.tool('Sélection').click();
    await tick();
    const at = { x: 6100, y: 9000 };
    v.pointer('dblclick', at.x, at.y);
    await tick();
    await tick();
    expect(v.mode()).toBe('Texte');
    expect(v.field()!.value).toBe('Bonjour à tous');
    await v.key('Escape', {}, v.field()!);
    const objects = await v.saved();
    expect(objects.map((o) => o.text)).toEqual(['Bonjour à tous']);
  });

  it('gives the family, the size, bold and the colour to the frame being typed and to selected frames', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('Titre');
    v.q<HTMLButtonElement>('[data-action="text-font-andika"]')!.click();
    v.q<HTMLButtonElement>('[data-action="text-size-large"]')!.click();
    v.q<HTMLButtonElement>('[data-action="text-bold"]')!.click();
    await tick();
    // The field stays: the options are applied to it.
    expect(v.field()).not.toBeNull();
    expect(v.field()!.getAttribute('style')).toContain('Ardoise Andika');
    [...v.target.querySelectorAll<HTMLButtonElement>('.swatch')].find((b) => b.title === 'Rouge')!.click();
    await tick();
    await v.key('Escape', {}, v.field()!);
    let objects = await v.saved();
    expect(objects[0]).toMatchObject({ font: 'andika', size: 900, bold: true, color: { r: 0xd8, g: 0, b: 0, a: 255 } });
    // Selected frames take a change too, one step.
    await v.key('a', { ctrlKey: true });
    v.q<HTMLButtonElement>('[data-action="text-size-small"]')!.click();
    await tick();
    objects = await v.saved();
    expect(objects[0]).toMatchObject({ size: 400, scale: 1 });
  });

  it('selects, moves and deletes frames with the strokes', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('Bonjour');
    await v.key('Escape', {}, v.field()!);
    v.tool('Sélection').click();
    await tick();
    await v.key('a', { ctrlKey: true });
    expect(v.count()).toBe('Traits sélectionnés : 1 — textes : 1');
    const before = (await v.saved())[0];
    // Move the selection by its frame.
    await v.drag(6100, 9000, 7100, 10_000);
    const after = (await v.saved())[0];
    expect(after.x).toBeCloseTo(before.x + 1000, 0);
    expect(after.y).toBeCloseTo(before.y + 1000, 0);
    await v.key('a', { ctrlKey: true });
    await v.key('Delete');
    expect(v.frames()).toHaveLength(0);
    expect(v.strokes()).toBe(0);
  });

  it('refuses a new frame on a hidden layer, and says so', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    // Hide the chosen layer from the Calques tab.
    const eye = v.target.querySelector<HTMLButtonElement>('[data-action="layer-visible"]')!;
    eye.click();
    await tick();
    await v.tap(6000, 9000);
    expect(v.field()).toBeNull();
    expect(v.q('.editor-notice')?.textContent).toBeTruthy();
  });

  it('speaks English too', async () => {
    setLocale('en');
    const v = await render();
    expect(v.tool('Text')).toBeTruthy();
    v.tool('Text').click();
    await tick();
    const options = v.q('[data-slot="text-options"]')!;
    expect(options.getAttribute('aria-label')).toBe('Text options');
    expect(options.textContent).toContain('Latin Modern');
    expect(options.textContent).toContain('Andika');
    expect(options.textContent).toContain('Small');
    expect(options.textContent).toContain('Bold');
    await v.tap(6000, 9000);
    expect(v.field()!.getAttribute('aria-label')).toBe('Text of the frame');
  });
});
