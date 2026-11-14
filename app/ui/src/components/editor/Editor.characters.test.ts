// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The « Caractères » tab of the editor's side panel (the user's choice of 2026-11-15: the Greek palette in the side
 * panel, never over the page): « αβγ » of the Text tool shows it and gives the tab before back, the tab can be chosen on
 * the rail with any tool, its keys insert at the caret of the frame being typed (selection replaced, focus kept, one
 * undo step with the typing, the keyboard too), without a frame they are disabled with a sentence and the « Outil
 * Texte » button, and Escape ends the typing directly (no step of its own); FR and EN.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { TextObject } from '../../lib/note';
import { sideStorageKey } from '../../lib/side-panel.svelte';
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
  const key = async (k: string, init: KeyboardEventInit = {}, on: EventTarget = window) => {
    const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
    on.dispatchEvent(event);
    await tick();
    return !event.defaultPrevented;
  };
  const field = () => q<HTMLTextAreaElement>('[data-slot="text-field"]');
  const input = async (value: string) => {
    const f = field()!;
    f.value = value;
    f.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText' }));
    await tick();
  };
  const tool = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === label)!;
  const mode = () => q('.toolbar .tool[aria-pressed="true"]')?.getAttribute('title');
  const dirty = () => q('section.editor')!.getAttribute('data-dirty');
  const undoButton = () => [...target.querySelectorAll<HTMLButtonElement>('.editor-top button')].find((b) => b.title.startsWith('Annuler') || b.title.startsWith('Undo'))!;
  const saved = async () => {
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(dirty()).toBe('no'));
    return (mockLibrary().notes.get(demoPath(title))!.note.objects ?? []) as TextObject[];
  };
  const panel = () => q<HTMLElement>('.editor-side .side-panel')!;
  const side = () => ({ tab: panel().dataset.tab, collapsed: panel().dataset.collapsed });
  const railTab = (id: string) => q<HTMLButtonElement>(`.editor-side .side-rail [role="tab"][data-tab="${id}"]`)!;
  const greek = () => q<HTMLButtonElement>('[data-action="text-greek"]')!;
  const characters = () => q<HTMLElement>('.editor-side [data-tab-panel="characters"]')!;
  const letter = (char: string) => q<HTMLButtonElement>(`[data-slot="greek-palette"] button[data-char="${char}"]`)!;
  /** A press of the pen or the mouse on a key: its pointerdown keeps the focus (default prevented), then the click. */
  const press = async (button: HTMLButtonElement) => {
    const down = new MouseEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 });
    button.dispatchEvent(down);
    button.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));
    await tick();
    return down.defaultPrevented;
  };
  return { target, q, tap, key, field, input, tool, mode, undoButton, saved, panel, side, railTab, greek, characters, letter, press };
}

describe('the « Caractères » tab of the side panel', () => {
  it('is on the rail before Aide, with its icon and name, and live mode has none', async () => {
    const v = await render();
    const rail = [...v.target.querySelectorAll<HTMLElement>('.editor-side .side-rail [role="tab"]')].map((b) => b.dataset.tab);
    expect(rail).toEqual(['layers', 'page', 'background', 'sound', 'projection', 'characters', 'help']);
    expect(v.railTab('characters').textContent?.trim()).toBe('Caractères');
    expect(v.railTab('characters').querySelector('svg.side-icon path')).not.toBeNull();
    expect(v.characters().hidden).toBe(true);
  });

  it('« αβγ » shows it, unfolding the panel, and gives back the tab before, folding the panel again', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    expect(v.greek().getAttribute('aria-pressed')).toBe('false');
    expect(v.greek().getAttribute('aria-controls')).toBe('side-tabpanel-editor-characters');
    expect(v.greek().getAttribute('aria-label')).toBe('Alphabet grec');
    // The panel folded: « αβγ » unfolds it on Caractères, then folds it again on Calques.
    v.q<HTMLButtonElement>('[data-action="side-collapse"]')!.click();
    await tick();
    expect(v.side()).toEqual({ tab: 'layers', collapsed: 'true' });
    expect(await v.press(v.greek())).toBe(true);
    expect(v.side()).toEqual({ tab: 'characters', collapsed: 'false' });
    expect(v.greek().getAttribute('aria-pressed')).toBe('true');
    expect(v.characters().hidden).toBe(false);
    await v.press(v.greek());
    expect(v.side()).toEqual({ tab: 'layers', collapsed: 'true' });
    expect(v.greek().getAttribute('aria-pressed')).toBe('false');
    // Page open: « αβγ » goes to Caractères and back to Page, the panel staying open.
    v.railTab('page').click();
    await tick();
    expect(v.side()).toEqual({ tab: 'page', collapsed: 'false' });
    await v.press(v.greek());
    expect(v.side().tab).toBe('characters');
    await v.press(v.greek());
    expect(v.side()).toEqual({ tab: 'page', collapsed: 'false' });
    // The tab chosen is remembered like any other (the panel's rule).
    await v.press(v.greek());
    expect(JSON.parse(localStorage.getItem(sideStorageKey('editor')) ?? '{}')).toEqual({ tab: 'characters', collapsed: false });
  });

  it('can be chosen on the rail with any tool: its keys wait for a frame, and « Outil Texte » takes the tool', async () => {
    const v = await render();
    expect(v.mode()).not.toBe('Texte');
    v.railTab('characters').click();
    await tick();
    expect(v.side()).toEqual({ tab: 'characters', collapsed: 'false' });
    expect(v.q('[data-slot="greek-hint"]')!.textContent).toBe("Touchez d'abord la page avec l'outil Texte pour taper un texte");
    expect(v.letter('α').disabled).toBe(true);
    // The whole alphabet in both cases, final sigma, the variants, the symbols and the scripts, by groups.
    for (const c of 'αβγδεζηθικλμνξοπρσςτυφχψωΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩϵϑϕϖϱµ°±×·≈≠≤≥→⇌∞√′⁰²⁻₂') expect(v.letter(c), c).not.toBeNull();
    const groups = [...v.characters().querySelectorAll<HTMLElement>('section[data-group] h3')].map((h) => h.textContent?.trim());
    expect(groups).toEqual(['Minuscules', 'Majuscules', 'Variantes', 'Symboles', 'Exposants et indices']);
    expect(v.letter('α').getAttribute('aria-label')).toBe('alpha minuscule');
    expect(v.letter('Α').getAttribute('aria-label')).toBe('Alpha majuscule');
    expect(v.letter('ς').getAttribute('aria-label')).toBe('sigma final minuscule');
    expect(v.letter('²').getAttribute('aria-label')).toBe('exposant 2');
    expect(v.letter('₂').getAttribute('aria-label')).toBe('indice 2');
    // « Outil Texte »: the tool taken, the button gone, the sentence still there until a frame is typed.
    const textTool = v.q<HTMLButtonElement>('[data-action="characters-text-tool"]')!;
    expect(textTool.textContent?.trim()).toBe('Outil Texte');
    textTool.click();
    await tick();
    expect(v.mode()).toBe('Texte');
    expect(v.q('[data-action="characters-text-tool"]')).toBeNull();
    expect(v.q('[data-slot="greek-hint"]')).not.toBeNull();
    // A frame: the keys work, the sentence goes; the keys are drawn in the frame's family.
    await v.tap(6000, 9000);
    expect(v.q('[data-slot="greek-hint"]')).toBeNull();
    expect(v.letter('α').disabled).toBe(false);
    expect(v.letter('α').getAttribute('style')).toContain('Ardoise Latin Modern Math');
  });

  it('inserts at the caret, in place of the selection, keeping the focus in the frame, in the typing’s undo step', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.input('v = d/t');
    await v.press(v.greek());
    const f = v.field()!;
    f.setSelectionRange(0, 0);
    expect(await v.press(v.letter('Δ'))).toBe(true);
    expect(f.value).toBe('Δv = d/t');
    expect(document.activeElement).toBe(f);
    expect(f.selectionStart).toBe(1);
    // A selection is replaced.
    f.setSelectionRange(5, 6);
    await v.press(v.letter('Δ'));
    expect(f.value).toBe('Δv = Δ/t');
    await v.press(v.letter('τ'));
    expect(f.value).toBe('Δv = Δτ/t');
    // Escape ends the typing at once (no step of the tab's own); the tab stays shown.
    await v.key('Escape', {}, f);
    expect(v.field()).toBeNull();
    expect(v.side().tab).toBe('characters');
    const [frame] = await v.saved();
    expect(frame.text).toBe('Δv = Δτ/t');
    // One step for the typing and the letters.
    v.undoButton().click();
    await tick();
    expect(await v.saved()).toHaveLength(0);
  });

  it('inserts from the keyboard too, the arrows moving between the keys, the focus going back to the frame', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    v.railTab('characters').click();
    await tick();
    const alpha = v.letter('α');
    alpha.focus();
    // The typing goes on while the focus is on the keys.
    expect(v.field()).not.toBeNull();
    await v.key('ArrowRight', {}, alpha);
    expect(document.activeElement).toBe(v.letter('β'));
    await v.key('ArrowLeft', {}, document.activeElement!);
    expect(document.activeElement).toBe(alpha);
    await v.key('ArrowDown', {}, alpha);
    expect(document.activeElement).not.toBe(alpha);
    alpha.focus();
    // Enter on a key is a click without a pointer.
    alpha.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 0 }));
    await tick();
    expect(v.field()!.value).toBe('α');
    expect(document.activeElement).toBe(v.field());
  });

  it('keeps the typing when its tab is pressed on the rail, and the focus comes back to the frame', async () => {
    const v = await render();
    v.tool('Texte').click();
    await tick();
    await v.tap(6000, 9000);
    await v.input('λ = c/');
    const f = v.field()!;
    // The window has the focus (jsdom says it does not: a blur would then be the window's, which ends nothing).
    vi.spyOn(document, 'hasFocus').mockReturnValue(true);
    // A press on the tab takes the focus from the field (as a mouse does), then the click shows the tab.
    v.railTab('characters').focus();
    v.railTab('characters').click();
    await tick();
    expect(v.field()).toBe(f);
    await vi.waitFor(() => expect(document.activeElement).toBe(f));
    f.setSelectionRange(f.value.length, f.value.length);
    await v.press(v.letter('ν'));
    expect(f.value).toBe('λ = c/ν');
    // Another tab of the rail ends the typing, as leaving the field always did.
    v.railTab('layers').focus();
    await tick();
    expect(v.field()).toBeNull();
  });

  it('speaks English too', async () => {
    setLocale('en');
    const v = await render();
    expect(v.railTab('characters').textContent?.trim()).toBe('Characters');
    v.railTab('characters').click();
    await tick();
    expect(v.q('[data-slot="side-title"]')!.textContent?.trim()).toBe('Characters');
    expect(v.q('[data-slot="greek-hint"]')!.textContent).toBe('First touch the page with the Text tool to type a text');
    expect(v.q('[data-action="characters-text-tool"]')!.textContent?.trim()).toBe('Text tool');
    expect(v.letter('α').getAttribute('aria-label')).toBe('lowercase alpha');
    expect(v.letter('Ω').getAttribute('aria-label')).toBe('uppercase Omega');
    expect(v.letter('ϑ').getAttribute('aria-label')).toBe('theta symbol');
    expect(v.letter('⁻').getAttribute('aria-label')).toBe('superscript minus');
    v.tool('Text').click();
    await tick();
    expect(v.greek().getAttribute('aria-label')).toBe('Greek alphabet');
    expect(v.greek().getAttribute('aria-pressed')).toBe('true');
  });
});
