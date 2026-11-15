// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The selection tool's two ways in the editor (docs/questions.md Q22): a click on its button from another tool gives the
 * rectangle, a click while it is the tool turns to the lasso, and so on; the V key does the same. The button says which way
 * is on (its label, its tooltip, its icon), and the tool's help line too. What each way selects is `Canvas.select-way.test.ts`.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import { share } from '../../lib/share-state.svelte';
import { selectPrefs } from '../../lib/select-prefs.svelte';
import { openSideTab } from '../side/side-test-helpers';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

// The real stroke geometry, from the module's bytes (jsdom cannot fetch it).
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
  setLocale('fr');
  selectPrefs.reset();
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
  selectPrefs.reset();
  share.reset();
  resetMockShare();
});

async function render() {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(TITLE))!;
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
  const q = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const tools = () => [...target.querySelectorAll<HTMLButtonElement>('.toolbar [role="group"] .tool')];
  /** The selection tool's button, whatever it says now: the first of the group. */
  const button = () => tools()[0];
  const pressed = () => q('.toolbar .tool[aria-pressed="true"]')?.title;
  const click = async (b: HTMLButtonElement) => {
    b.click();
    await tick();
  };
  const tool = (title: string) => tools().find((b) => b.title === title)!;
  const key = async (k: string, init: KeyboardEventInit = {}, on: EventTarget = window) => {
    const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
    on.dispatchEvent(event);
    on.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
    return !event.defaultPrevented;
  };
  const help = () => q('.select-help')?.textContent ?? null;
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = svg.getAttribute('data-view')!.split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  const pointer = (type: string, x: number, y: number) => {
    const at = client(x, y);
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    for (const [name, value] of Object.entries({ pointerId: 1, pressure: 0.5, pointerType: 'mouse', timeStamp: performance.now() })) {
      Object.defineProperty(event, name, { value, configurable: true });
    }
    svg.dispatchEvent(event);
  };
  const dirty = () => q('section.editor')!.getAttribute('data-dirty');
  return { target, svg, q, tools, button, pressed, click, tool, key, help, pointer, dirty };
}

const RECTANGLE = 'Sélection : rectangle';
const LASSO = 'Sélection : lasso';

describe('the button of the selection tool', () => {
  it('gives the rectangle from another tool, the lasso at the next click, the rectangle at the next, and so on', async () => {
    const v = await render();
    // The pen is the tool at first: the button offers what a click gives, the rectangle.
    expect(v.pressed()).toBe('Stylo');
    expect(v.button().title).toBe(RECTANGLE);
    expect(v.button().getAttribute('aria-pressed')).toBe('false');

    await v.click(v.button());
    expect(v.pressed()).toBe(RECTANGLE);
    expect(v.button().title).toBe(RECTANGLE);
    // The name is the accessible name; the visible label stays short so that the button keeps its height.
    expect(v.button().getAttribute('aria-label')).toBe(RECTANGLE);
    expect(v.button().querySelector('.tool-label')?.textContent?.trim()).toBe('Sélection');
    expect(v.button().dataset.way).toBe('rectangle');
    expect(v.help()).toMatch(/^Rectangle : /);

    await v.click(v.button());
    // Still the tool in use, now with the lasso.
    expect(v.pressed()).toBe(LASSO);
    expect(v.button().getAttribute('aria-pressed')).toBe('true');
    expect(v.button().title).toBe(LASSO);
    expect(v.button().getAttribute('aria-label')).toBe(LASSO);
    expect(v.button().querySelector('.tool-label')?.textContent?.trim()).toBe('Sélection');
    expect(v.button().dataset.way).toBe('lasso');
    expect(v.help()).toMatch(/^Lasso : /);

    await v.click(v.button());
    expect(v.pressed()).toBe(RECTANGLE);
    expect(v.help()).toMatch(/^Rectangle : /);

    await v.click(v.button());
    expect(v.pressed()).toBe(LASSO);
  });

  it('draws a different icon for each way, and the help line says that a new click changes it', async () => {
    const v = await render();
    await v.click(v.button());
    const icon = () => v.button().querySelector('svg')!;
    expect(icon().getAttribute('aria-hidden')).toBe('true');
    expect(icon().querySelector('rect')).not.toBeNull();
    expect(v.help()).toContain('Un nouveau clic sur le bouton (ou V) : lasso');
    await v.click(v.button());
    expect(icon().querySelector('rect')).toBeNull();
    expect(icon().querySelectorAll('path')).toHaveLength(2);
    expect(v.help()).toContain('Un nouveau clic sur le bouton (ou V) : rectangle');
  });

  it('gives the way used last when the tool is chosen after another one, and the button offers it meanwhile', async () => {
    const v = await render();
    await v.click(v.button());
    await v.click(v.button());
    expect(v.pressed()).toBe(LASSO);
    await v.click(v.tool('Gomme'));
    // The lasso is remembered while another tool is in use: the button shows what a click will give.
    expect(v.button().title).toBe(LASSO);
    expect(v.button().dataset.way).toBe('lasso');
    expect(v.button().getAttribute('aria-pressed')).toBe('false');
    await v.click(v.button());
    expect(v.pressed()).toBe(LASSO);
    expect(v.help()).toMatch(/^Lasso : /);
    // A click while it is the tool still turns it, and the rectangle is then the one remembered.
    await v.click(v.button());
    await v.click(v.tool('Stylo'));
    expect(v.button().title).toBe(RECTANGLE);
    await v.click(v.button());
    expect(v.pressed()).toBe(RECTANGLE);
  });

  it('gives the way used last after the manual crop too', async () => {
    const v = await render();
    await v.click(v.button());
    await v.click(v.button());
    await openSideTab(v.target, 'page');
    v.q<HTMLButtonElement>('[data-action="manual-crop"]')!.click();
    await tick();
    expect(v.pressed()).toBeUndefined();
    await v.key('Escape');
    expect(v.pressed()).toBe(LASSO);
  });

  it('keeps the way across notes, and from one opening of the editor to the next (a preference of the computer)', async () => {
    const first = await render();
    await first.click(first.button());
    await first.click(first.button());
    expect(first.pressed()).toBe(LASSO);
    cleanup?.();
    // The same note again, a new editor: the pen is the tool, and the lasso is what the button offers.
    const second = await render();
    expect(second.pressed()).toBe('Stylo');
    expect(second.button().title).toBe(LASSO);
    await second.key('v');
    expect(second.pressed()).toBe(LASSO);
    expect(second.help()).toMatch(/^Lasso : /);
    expect(localStorage.getItem('ardoise.select.way')).toBe('lasso');
  });

  it('is named in English too', async () => {
    setLocale('en');
    const v = await render();
    expect(v.button().title).toBe('Select: rectangle');
    await v.click(v.button());
    expect(v.help()).toMatch(/^Rectangle: /);
    await v.click(v.button());
    expect(v.button().title).toBe('Select: lasso');
    expect(v.button().getAttribute('aria-label')).toBe('Select: lasso');
    expect(v.button().querySelector('.tool-label')?.textContent?.trim()).toBe('Select');
    expect(v.help()).toMatch(/^Lasso: /);
    expect(v.help()).toContain('Click the button again (or V): rectangle');
  });

  it('keeps the selection and the note as they are when it turns', async () => {
    const v = await render();
    await v.click(v.button());
    await v.key('a', { ctrlKey: true });
    expect(v.target.textContent).toContain('Traits sélectionnés : 1');
    await v.click(v.button());
    expect(v.pressed()).toBe(LASSO);
    expect(v.target.textContent).toContain('Traits sélectionnés : 1');
    expect(v.dirty()).toBe('no');
    // Turning is no edit: nothing to undo.
    expect(v.q<HTMLButtonElement>('button[title^="Annuler"]')?.disabled).toBe(true);
  });

  it('is the tool to draw a rectangle or a lasso on the page, by the way it is on', async () => {
    const v = await render();
    await v.click(v.button());
    v.pointer('pointerdown', 1000, 1000);
    v.pointer('pointermove', 9000, 9000);
    await tick();
    expect(v.svg.querySelector('rect.marquee')).not.toBeNull();
    expect(v.svg.querySelector('polyline.lasso')).toBeNull();
    v.pointer('pointerup', 9000, 9000);
    await tick();
    await v.click(v.button());
    v.pointer('pointerdown', 1000, 1000);
    v.pointer('pointermove', 9000, 1000);
    v.pointer('pointermove', 9000, 9000);
    await tick();
    expect(v.svg.querySelector('polyline.lasso')).not.toBeNull();
    expect(v.svg.querySelector('rect.marquee')).toBeNull();
    v.pointer('pointerup', 9000, 9000);
  });
});

describe('the V key', () => {
  it('does what the button does: the rectangle from another tool, then the lasso, then the rectangle', async () => {
    const v = await render();
    await v.key('v');
    expect(v.pressed()).toBe(RECTANGLE);
    await v.key('v');
    expect(v.pressed()).toBe(LASSO);
    await v.key('v');
    expect(v.pressed()).toBe(RECTANGLE);
    // The variant with Shift (a capital V) is the same key.
    await v.key('V', { shiftKey: true });
    expect(v.pressed()).toBe(LASSO);
    // Another tool by its key, and back: the way used last, the lasso; the next press turns it.
    await v.key('p');
    expect(v.pressed()).toBe('Stylo');
    await v.key('v');
    expect(v.pressed()).toBe(LASSO);
    await v.key('v');
    expect(v.pressed()).toBe(RECTANGLE);
  });

  it('turns once per press, not while the key is held down', async () => {
    const v = await render();
    await v.key('v');
    expect(v.pressed()).toBe(RECTANGLE);
    await v.key('v', { repeat: true });
    await v.key('v', { repeat: true });
    expect(v.pressed()).toBe(RECTANGLE);
    await v.key('v');
    expect(v.pressed()).toBe(LASSO);
  });

  it('is a letter like another in a field, and does nothing with Ctrl', async () => {
    const v = await render();
    const title = v.q<HTMLInputElement>('.title-field input')!;
    await v.key('v', {}, title);
    expect(v.pressed()).toBe('Stylo');
    await v.key('v', { ctrlKey: true });
    expect(v.pressed()).toBe('Stylo');
  });

  it('does nothing in a replay, where no tool can be chosen', async () => {
    const v = await render();
    await v.key('r');
    expect(v.target.querySelector('.replay-bar')).not.toBeNull();
    await v.key('v');
    expect(v.target.querySelector('.replay-bar')).not.toBeNull();
    v.pointer('pointerdown', 1000, 1000);
    v.pointer('pointermove', 9000, 9000);
    await tick();
    expect(v.svg.querySelector('rect.marquee')).toBeNull();
    expect(v.svg.querySelector('polyline.lasso')).toBeNull();
    v.pointer('pointerup', 9000, 9000);
    await v.key('Escape');
    // Back in the editor, the tool is the one before the replay.
    expect(v.pressed()).toBe('Stylo');
  });
});
