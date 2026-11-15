// @vitest-environment jsdom
/// <reference types="node" />
/**
 * Text frames that wrap in the editor (format 1.7, docs/design/page-objects.md §4): a new frame wraps at the page's right
 * edge, the field shows the frame's lines (a line break of its own where the frame wraps) and turns what is typed back
 * into the frame's text, the side handles of a selected frame change its width, a frame made before 1.7 keeps its lines,
 * a character no font has is named under the frame (FR and EN). The « Caractères » tab: `Editor.characters.test.ts`.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { instantiateInk } from '../../lib/ink';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { TextObject } from '../../lib/note';
import { textStyle } from '../../lib/editor/objects';
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

const ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));

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
const LONG =
  'La lumière se propage en ligne droite dans un milieu transparent et homogène, et sa vitesse dans le vide vaut trois cent mille kilomètres par seconde.';
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
  const pointer = (type: string, x: number, y: number) => {
    const at = client(x, y);
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    for (const [key, value] of Object.entries({ pointerId: 1, pressure: 0.5, pointerType: 'mouse', timeStamp: performance.now() })) {
      Object.defineProperty(event, key, { value, configurable: true });
    }
    svg.dispatchEvent(event);
    return event;
  };
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
    await tick();
    return !event.defaultPrevented;
  };
  const field = () => q<HTMLTextAreaElement>('[data-slot="text-field"]');
  /** The field's value set as the browser would after a key, with the input's type. */
  const input = async (value: string, inputType = 'insertText') => {
    const f = field()!;
    f.value = value;
    f.dispatchEvent(new InputEvent('input', { bubbles: true, inputType }));
    await tick();
  };
  const tool = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === label)!;
  const dirty = () => q('section.editor')!.getAttribute('data-dirty');
  const undoButton = () => [...target.querySelectorAll<HTMLButtonElement>('.editor-top button')].find((b) => b.title.startsWith('Annuler') || b.title.startsWith('Undo'))!;
  const saved = async () => {
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(dirty()).toBe('no'));
    return (mockLibrary().notes.get(demoPath(title))!.note.objects ?? []) as TextObject[];
  };
  return { target, svg, q, pointer, tap, drag, key, field, input, tool, undoButton, saved };
}

describe('a frame that wraps', () => {
  it('wraps at the page edge: the field shows its lines, the text keeps only the line breaks typed', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.input(LONG);
    const f = v.field()!;
    const [frame] = await (async () => {
      await v.key('s', { ctrlKey: true });
      return (mockLibrary().notes.get(demoPath(TITLE))!.note.objects ?? []) as TextObject[];
    })();
    // 29 700 wide page, 1 cm of margin, put at x = 6000.
    expect(frame.wrapWidth).toBe(29700 - 1000 - 6000);
    const laid = ink.text.layout(LONG, textStyle(frame));
    expect(laid.lines.length).toBeGreaterThan(1);
    // The field's lines are the frame's lines: its line k is drawn on the frame's line k.
    expect(f.value.split('\n')).toEqual(laid.lines.map((l) => LONG.slice(l.start, l.end)));
    // The frame's text has no line break of the field's.
    expect(frame.text).toBe(LONG);
    expect(frame.height).toBe(laid.lines.length * 600 * 1.25);
    // The field is as wide as the wrap width (its outline shows where the lines end).
    expect(parseFloat(f.style.width)).toBeGreaterThan(0);
  });

  it('turns Backspace at the start of a wrapped line into the deletion of the space before it, and Enter into a line break', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.input(LONG);
    const value = v.field()!.value;
    const soft = value.indexOf('\n');
    expect(soft).toBeGreaterThan(0);
    const offset = soft; // No soft break before the first one: the field's position is the text's.
    expect(LONG[offset - 1]).toBe(' ');
    await v.input(value.slice(0, soft) + value.slice(soft + 1), 'deleteContentBackward');
    await v.key('Escape', {}, v.field()!);
    let [frame] = await v.saved();
    expect(frame.text).toBe(LONG.slice(0, offset - 1) + LONG.slice(offset));
    // Undo removes the whole typing at once.
    v.undoButton().click();
    await tick();
    expect(await v.saved()).toHaveLength(0);
    // Enter starts a new line of the text.
    await v.tap(6000, 9000);
    await v.input('Ligne un');
    await v.input('Ligne un\nLigne deux', 'insertLineBreak');
    await v.key('Escape', {}, v.field()!);
    [frame] = await v.saved();
    expect(frame.text).toBe('Ligne un\nLigne deux');
  });

  it('has side handles when it is selected alone, which change its width; one step', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.input(LONG);
    await v.key('Escape', {}, v.field()!);
    const before = (await v.saved())[0];
    // The text tool: no handles (the selection's handles are the selection tool's).
    expect(v.q('[data-slot="wrap-handle-right"]')).toBeNull();
    v.tool('Sélection : rectangle').click();
    await tick();
    const right = v.q<SVGRectElement>('[data-slot="wrap-handle-right"]')!;
    expect(right).not.toBeNull();
    expect(v.q('[data-slot="wrap-handle-left"]')).not.toBeNull();
    const hx = Number(right.getAttribute('x')) + Number(right.getAttribute('width')) / 2;
    const hy = Number(right.getAttribute('y')) + Number(right.getAttribute('height')) / 2;
    expect(hx).toBeCloseTo(before.x + before.width, 0);
    await v.drag(hx, hy, before.x + 8000, hy);
    const after = (await v.saved())[0];
    expect(after.wrapWidth).toBeCloseTo(8000, 0);
    expect(after.height).toBeGreaterThan(before.height);
    expect([after.x, after.y, after.scale]).toEqual([before.x, before.y, before.scale]);
    // The left handle moves the left edge, the right one stays.
    const left = v.q<SVGRectElement>('[data-slot="wrap-handle-left"]')!;
    const lx = Number(left.getAttribute('x')) + Number(left.getAttribute('width')) / 2;
    const ly = Number(left.getAttribute('y')) + Number(left.getAttribute('height')) / 2;
    await v.drag(lx, ly, lx + 1000, ly);
    const moved = (await v.saved())[0];
    expect(moved.x).toBeCloseTo(after.x + 1000, 0);
    expect(moved.wrapWidth).toBeCloseTo(7000, 0);
    // Each drag is one step.
    v.undoButton().click();
    await tick();
    expect((await v.saved())[0].wrapWidth).toBeCloseTo(8000, 0);
    // Two objects selected: no side handles (the corners scale them).
    await v.key('a', { ctrlKey: true });
    expect(v.q('[data-slot="wrap-handle-right"]')).toBeNull();
  });

  it('keeps the lines of a frame made before wrapping existed, until its width is set', async () => {
    const path = demoPath(TITLE);
    const stored = mockLibrary().notes.get(path)!;
    const old: TextObject = {
      kind: 'text',
      id: 'old',
      layer: 0,
      t: 0,
      x: 2000,
      y: 3000,
      scale: 1,
      quarterTurns: 0,
      width: 0,
      height: 0,
      text: LONG,
      font: 'latinModern',
      bold: false,
      size: 600,
      color: { r: 0, g: 0, b: 0, a: 255 },
    };
    stored.note = { ...stored.note, objects: [old] };
    const v = await render();
    v.tool('Texte').click();
    await tick();
    // Typing in it: one line, no wrap width given.
    await v.tap(2100, 3300);
    expect(v.field()!.value).toBe(LONG);
    await v.key('Escape', {}, v.field()!);
    const [frame] = await v.saved();
    expect(frame.wrapWidth).toBeUndefined();
    expect(ink.text.layout(frame.text, textStyle(frame)).lines).toHaveLength(1);
  });
});

describe('a character no font has', () => {
  it('is drawn as a square and named under the frame, in French and in English', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    expect(v.q('[data-slot="text-missing"]')).toBeNull();
    await v.input('Bravo \u{1F600}');
    const hint = v.q('[data-slot="text-missing"]')!;
    expect(hint.textContent).toContain("Ce caractère n'existe dans aucune police d'Ardoise");
    expect(hint.textContent).toContain('U+1F600');
    // The field shows the square the frame draws, the text keeps the character.
    expect(v.field()!.value).toBe('Bravo \u25A1');
    await v.key('Escape', {}, v.field()!);
    expect((await v.saved())[0].text).toBe('Bravo \u{1F600}');
    setLocale('en');
    await tick();
    await v.tap(6100, 9100);
    await v.input('Bravo \u{1F600}\u{4E2D}');
    expect(v.q('[data-slot="text-missing"]')!.textContent).toContain('No font of Ardoise has these characters');
  });
});
