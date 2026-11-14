// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The bookmark tool in the editor (SPEC §4.4 « Signets », docs/design/page-objects.md §7): a tap puts a bookmark where one
 * types its label, the typing is one undoable step, Enter and Escape end it with the label kept, an empty label removes the
 * bookmark, the keys typed never reach the editor's shortcuts, a tap on a bookmark (or a double click with the selection
 * tool) renames it, the selection moves and deletes it, undo and redo restore it, and a note opens at one of its bookmarks.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { BookmarkObject, PageObject } from '../../lib/note';
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

/** The bookmarks of the demo note, written in the stored note before the editor opens it. */
function withBookmarks(labels: [string, number, number][], layer = 0): string[] {
  const stored = mockLibrary().notes.get(demoPath(TITLE))!;
  const objects: PageObject[] = labels.map(([label, x, y], i) => ({
    kind: 'bookmark',
    id: `signet-${i}`,
    layer,
    t: 0,
    x,
    y,
    scale: 1,
    quarterTurns: 0,
    width: 3000,
    height: 925,
    label,
  }));
  stored.note.objects = objects;
  return objects.map((o) => o.id);
}

async function render(props: Record<string, unknown> = {}) {
  const path = demoPath(TITLE);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn(), ...props } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const view = () => (svg.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = view();
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
  const tap = async (x: number, y: number, extra: Record<string, unknown> = {}) => {
    pointer('pointerdown', x, y, extra);
    pointer('pointerup', x, y, extra);
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
  const field = () => q<HTMLInputElement>('[data-slot="bookmark-field"]');
  const type = async (text: string) => {
    const f = field()!;
    f.value = text;
    f.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
  };
  const marks = () => [...target.querySelectorAll<SVGGElement>('g.ink g.bookmark')];
  const labels = () => marks().map((g) => g.getAttribute('data-bookmark'));
  const tool = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === label)!;
  const mode = () => q('.toolbar .tool[aria-pressed="true"]')?.getAttribute('title');
  const dirty = () => q('section.editor')!.getAttribute('data-dirty');
  const count = () => q('[data-section="selection"] .pane-help')?.textContent?.trim() ?? null;
  const undoButton = () => [...target.querySelectorAll<HTMLButtonElement>('.editor-top button')].find((b) => b.title.startsWith('Annuler'))!;
  const saved = async () => {
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(dirty()).toBe('no'));
    return (mockLibrary().notes.get(demoPath(TITLE))!.note.objects ?? []) as BookmarkObject[];
  };
  return { target, svg, q, view, pointer, tap, drag, key, field, type, marks, labels, tool, mode, dirty, count, undoButton, saved };
}

describe('the bookmark tool', () => {
  it('is a tool of the toolbar, and the B key picks it', async () => {
    const v = await render();
    const button = v.tool('Signet');
    expect(button).toBeTruthy();
    expect(button.querySelector('.tool-label')?.textContent).toBe('Signet');
    button.click();
    await tick();
    expect(v.mode()).toBe('Signet');
    expect(v.q('[data-slot="bookmark-help"]')?.textContent).toContain('signet');
    v.tool('Sélection').click();
    await tick();
    await v.key('b');
    expect(v.mode()).toBe('Signet');
  });

  it('puts a bookmark where the page is tapped, with its label typed in a field, as one step that undo removes', async () => {
    const v = await render();
    v.tool('Signet').click();
    await tick();
    await v.tap(6000, 9000);
    const f = v.field()!;
    expect(f).not.toBeNull();
    expect(document.activeElement).toBe(f);
    expect(f.getAttribute('aria-label')).toBe('Texte du signet');
    expect(f.maxLength).toBe(60);
    await v.type('É');
    await v.type('Équation');
    await v.type('Équation du second degré');
    // The bookmark follows what is typed, drawn from the shipped glyphs (a flag, a chip, the label).
    expect(v.labels()).toEqual(['Équation du second degré']);
    const g = v.marks()[0];
    expect(g.querySelector('.bookmark-flag')?.getAttribute('d')).toMatch(/^M/);
    expect(g.querySelector('.bookmark-label')?.getAttribute('d')).toMatch(/[QC]/);
    expect(v.dirty()).toBe('yes');
    // Enter ends the typing: the label kept, the bookmark selected, one step.
    expect(await v.key('Enter', {}, f)).toBe(false);
    expect(v.field()).toBeNull();
    expect(v.labels()).toEqual(['Équation du second degré']);
    expect(v.count()).toBe('Traits sélectionnés : 0 — textes, équations et signets : 1');
    v.undoButton().click();
    await tick();
    expect(v.labels()).toEqual([]);
    expect(v.undoButton().disabled).toBe(true);
    // Redo brings it back.
    await v.key('y', { ctrlKey: true });
    expect(v.labels()).toEqual(['Équation du second degré']);
    const objects = await v.saved();
    expect(objects).toHaveLength(1);
    expect(objects[0]).toMatchObject({ kind: 'bookmark', label: 'Équation du second degré', layer: 0, scale: 1, quarterTurns: 0 });
    expect(objects[0].width).toBeGreaterThan(3000);
    expect(objects[0].height).toBeCloseTo(925, 0);
    expect(objects[0].x).toBeCloseTo(6000, 0);
    expect(objects[0].y).toBeCloseTo(9000 - 925 / 2, 0);
  });

  it('keeps the label as one short line: white space collapsed, cut at the limit', async () => {
    const v = await render();
    v.tool('Signet').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('  Chap.  3 \n\t les forces  ');
    await v.key('Enter', {}, v.field()!);
    expect(v.labels()).toEqual(['Chap. 3 les forces']);
    v.tool('Signet').click();
    await v.tap(6000, 15000);
    await v.type('x'.repeat(80));
    await v.key('Enter', {}, v.field()!);
    expect(v.labels()[1]).toHaveLength(60);
  });

  it('ends the typing with Escape, the label kept; the next Escape deselects', async () => {
    const v = await render();
    v.tool('Signet').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('Début');
    expect(await v.key('Escape', {}, v.field()!)).toBe(false);
    expect(v.field()).toBeNull();
    expect(v.labels()).toEqual(['Début']);
    expect(v.count()).toBe('Traits sélectionnés : 0 — textes, équations et signets : 1');
    await v.key('Escape');
    expect(v.count()).toBeNull();
  });

  it('removes a bookmark left without a label, with no step', async () => {
    const v = await render();
    v.tool('Signet').click();
    await tick();
    await v.tap(6000, 9000);
    await v.type('   ');
    await v.key('Escape', {}, v.field()!);
    expect(v.labels()).toEqual([]);
    expect(v.dirty()).toBe('no');
    expect(v.undoButton().disabled).toBe(true);
  });

  it('keeps every key typed in the field away from the editor shortcuts', async () => {
    const v = await render();
    v.tool('Signet').click();
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
      ['b', {}],
      ['t', {}],
      [' ', {}],
      ['Delete', {}],
      ['Backspace', {}],
      ['a', { ctrlKey: true }],
      ['z', { ctrlKey: true }],
    ] as [string, KeyboardEventInit][]) {
      expect(await v.key(k, init, f), k).toBe(true);
    }
    expect(v.mode()).toBe('Signet');
    expect(v.field()).not.toBeNull();
    expect(v.q('[data-slot="replay-bar"]')).toBeNull();
  });

  it('renames the bookmark tapped with the tool, as one step', async () => {
    withBookmarks([['Avant', 3000, 5000]]);
    const v = await render();
    expect(v.labels()).toEqual(['Avant']);
    v.tool('Signet').click();
    await tick();
    await v.tap(3200, 5400);
    expect(v.field()!.value).toBe('Avant');
    await v.type('Après');
    await v.key('Enter', {}, v.field()!);
    expect(v.labels()).toEqual(['Après']);
    expect(v.undoButton().disabled).toBe(false);
    v.undoButton().click();
    await tick();
    expect(v.labels()).toEqual(['Avant']);
    expect(v.undoButton().disabled).toBe(true);
  });

  it('renames with a double click of the selection tool, and removes by emptying the label', async () => {
    withBookmarks([['Avant', 3000, 5000]]);
    const v = await render();
    v.tool('Sélection').click();
    await tick();
    v.pointer('dblclick', 3200, 5400);
    await tick();
    await tick();
    expect(v.field()!.value).toBe('Avant');
    await v.type('');
    await v.key('Enter', {}, v.field()!);
    expect(v.labels()).toEqual([]);
    // One step: undo brings it back with its label.
    v.undoButton().click();
    await tick();
    expect(v.labels()).toEqual(['Avant']);
  });

  it('does not change anything when the label is left as it was', async () => {
    withBookmarks([['Pareil', 3000, 5000]]);
    const v = await render();
    v.tool('Signet').click();
    await tick();
    await v.tap(3200, 5400);
    await v.key('Escape', {}, v.field()!);
    expect(v.labels()).toEqual(['Pareil']);
    expect(v.dirty()).toBe('no');
    expect(v.undoButton().disabled).toBe(true);
  });

  it('moves with the selection, undoable, and is deleted with it', async () => {
    withBookmarks([['Ici', 3000, 5000]]);
    const v = await render();
    // A click selects it, a drag from inside its frame moves it.
    v.tool('Sélection').click();
    await tick();
    await v.tap(3200, 5400);
    expect(v.count()).toBe('Traits sélectionnés : 0 — textes, équations et signets : 1');
    await v.drag(3200, 5400, 4200, 6400);
    const stored = async () => (await v.saved())[0];
    const moved = await stored();
    expect(moved.x).toBeCloseTo(4000, 3);
    expect(moved.y).toBeCloseTo(6000, 3);
    v.undoButton().click();
    await tick();
    const back = await stored();
    expect([back.x, back.y]).toEqual([3000, 5000]);
    // Delete removes it; undo brings it back.
    await v.tap(3200, 5400);
    await v.key('Delete');
    expect(v.labels()).toEqual([]);
    v.undoButton().click();
    await tick();
    expect(v.labels()).toEqual(['Ici']);
  });

  it('is shown and selectable whatever its layer shows, and its layer is not the chosen one', async () => {
    // Layer 1 does not exist in the demo note: add it hidden, and put the bookmark on it.
    const stored = mockLibrary().notes.get(demoPath(TITLE))!.note;
    stored.layers = [...stored.layers, { name: 'Cachée', visible: false }];
    withBookmarks([['Sur un calque caché', 3000, 5000]], 1);
    const v = await render();
    expect(v.labels()).toEqual(['Sur un calque caché']);
    v.tool('Sélection').click();
    await tick();
    await v.tap(3200, 5400);
    expect(v.count()).toBe('Traits sélectionnés : 0 — textes, équations et signets : 1');
  });

  it('is copied and pasted, a cut keeping it on the clipboard', async () => {
    withBookmarks([['Copié', 3000, 5000]]);
    const written: string[] = [];
    const clipboard = {
      writeText: vi.fn(async (text: string) => {
        written.push(text);
      }),
      readText: vi.fn(async () => written[written.length - 1] ?? ''),
    };
    Object.defineProperty(navigator, 'clipboard', { value: clipboard, configurable: true });
    const v = await render();
    v.tool('Sélection').click();
    await tick();
    await v.tap(3200, 5400);
    await v.key('c', { ctrlKey: true });
    await vi.waitFor(() => expect(written).toHaveLength(1));
    const copied = JSON.parse(written[0]);
    expect(copied.objects).toHaveLength(1);
    expect(copied.objects[0]).toMatchObject({ kind: 'bookmark', label: 'Copié' });
    await v.key('v', { ctrlKey: true });
    await vi.waitFor(() => expect(v.labels()).toEqual(['Copié', 'Copié']));
  });
});

describe('opening a note at a bookmark', () => {
  it('selects the bookmark and shows it at the middle of the editor', async () => {
    const ids = withBookmarks([
      ['Premier', 3000, 2000],
      ['Cible', 26000, 19000],
    ]);
    const v = await render({ focus: { id: ids[1] } });
    await vi.waitFor(() => expect(v.count()).toBe('Traits sélectionnés : 0 — textes, équations et signets : 1'));
    const [scale, vx, vy] = v.view();
    // The bookmark's centre (26000 + w/2, 19000 + h/2) is at the middle of the stage, at real size or more.
    expect(scale).toBeGreaterThanOrEqual(96 / 25.4 / 100 - 1e-9);
    const object = (mockLibrary().notes.get(demoPath(TITLE))!.note.objects ?? [])[1];
    const screenX = (object.x + object.width / 2) * scale + vx;
    const screenY = (object.y + object.height / 2) * scale + vy;
    const stage = v.q('.stage')!;
    // jsdom has no layout: the canvas keeps its nominal 1000 × 700 stage.
    // (The stored box is not the measured one: within a few centimetres of the middle.)
    expect(screenX).toBeCloseTo(stage.clientWidth > 0 ? stage.clientWidth / 2 : 500, -2);
    expect(screenY).toBeCloseTo(stage.clientHeight > 0 ? stage.clientHeight / 2 : 350, -2);
  });

  it('says so when the bookmark is gone, and opens the note anyway', async () => {
    withBookmarks([['Autre', 3000, 2000]]);
    const v = await render({ focus: { id: 'disparu' } });
    await vi.waitFor(() => expect(v.q('.editor-notice')?.textContent).toContain("n'existe plus"));
    expect(v.labels()).toEqual(['Autre']);
  });
});
