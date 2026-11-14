// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The question « L'équation en cours n'est pas posée. » (docs/design/equations.md §4): while the equation panel holds code
 * that is not on the page (a new equation with some code, or an equation of the page that was changed), everything that
 * would close the panel first asks (another tool by a click or a key, undo, the replay, the split, another note, the back
 * button, Ctrl + W, the window, an export): « Poser » (only for a valid code: it is put down, then what was asked goes on),
 * « Abandonner » (dropped, what was asked goes on) or « Continuer l'écriture » (nothing changes; Escape gives this answer).
 * An empty or unchanged field asks nothing.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { bridgeScreens } from '../../lib/app-bridge.svelte';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import { share } from '../../lib/share-state.svelte';
import type { EquationObject, PageObject } from '../../lib/note';
import type { WindowClose } from '../../lib/editor/window-close';
import EditorPagesHarness from './EditorPagesHarness.svelte';

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

const PHYSICS = 'Cours de physique';
const SHOPPING = 'Liste de courses';
const PARENTS = 'Réunion parents';
let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockNarration();
  resetMockShare();
  share.reset();
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
  share.reset();
  resetMockShare();
  vi.restoreAllMocks();
  setLocale('fr');
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

/** A stand-in for the desktop window: `request()` is the user closing it (Alt + F4, Ctrl + Q, the close button). */
function fakeWindow() {
  let guard: (() => boolean) | null = null;
  const destroy = vi.fn(async () => {});
  const closer: WindowClose = {
    subscribe: async (g) => {
      guard = g;
      return () => {};
    },
    destroy,
  };
  return { closer, request: () => guard?.() ?? false, destroy };
}

async function render(open = SHOPPING) {
  const path = demoPath(open);
  const target = document.createElement('div');
  document.body.append(target);
  const [onclose, onsaved, onpage] = [vi.fn(), vi.fn(), vi.fn()];
  const win = fakeWindow();
  const component = mount(EditorPagesHarness, {
    target,
    props: { initial: path, pages: [PHYSICS, SHOPPING, PARENTS].map(demoPath), app, onclose, onsaved, onpage, windowClose: win.closer },
  });
  const harness = component as unknown as { held(): string; setPath(next: string): void };
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
    on.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
    return !event.defaultPrevented;
  };
  const field = () => q<HTMLTextAreaElement>('[data-slot="equation-source"]');
  const panel = () => q('[data-slot="equation-panel"]');
  const previewPath = () => q('[data-slot="equation-preview"] path')?.getAttribute('d') ?? null;
  const write = async (latex: string) => {
    const f = field()!;
    f.value = latex;
    f.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
  };
  const drawn = () => vi.waitFor(() => expect(previewPath()).toMatch(/^M/), { timeout: 10_000 });
  const tool = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === label)!;
  const mode = () => q('.toolbar .tool[aria-pressed="true"]')?.getAttribute('title');
  const dirty = () => q('section.editor')!.getAttribute('data-dirty');
  const equations = () => [...target.querySelectorAll('g.ink path[data-kind="equation"]')];
  const button = (text: string) => [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text);
  const undoButton = () => [...target.querySelectorAll<HTMLButtonElement>('.editor-top button')].find((b) => b.title.startsWith('Annuler'))!;
  /** The equation question, as it shows; null when it is not asked. */
  const ask = () => {
    const dialog = document.querySelector('[role="alertdialog"]');
    if (!dialog || !dialog.querySelector('[data-slot="equation-ask-message"]')) return null;
    return {
      title: dialog.querySelector('h2')?.textContent,
      message: dialog.querySelector('[data-slot="equation-ask-message"]')?.textContent?.trim(),
      invalid: dialog.querySelector('[data-slot="equation-ask-invalid"]')?.textContent?.trim() ?? null,
      buttons: [...dialog.querySelectorAll('button')].map((b) => b.textContent?.trim()),
      placeDisabled: dialog.querySelector<HTMLButtonElement>('[data-action="equation-ask-place"]')?.disabled ?? null,
    };
  };
  const asked = () => vi.waitFor(() => expect(ask()).not.toBeNull(), { timeout: 10_000 });
  const answer = async (name: 'place' | 'drop' | 'keep') => {
    document.querySelector<HTMLButtonElement>(`[data-action="equation-ask-${name}"]`)!.click();
    await tick();
    await tick();
  };
  const leaveQuestion = () => {
    const dialog = document.querySelector('[role="alertdialog"]');
    return dialog && dialog.querySelector('h2')?.textContent === 'Modifications non enregistrées' ? [...dialog.querySelectorAll('button')].map((b) => b.textContent?.trim()) : null;
  };
  /** The note as saved (Ctrl + S). */
  const saved = async () => {
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(dirty()).toBe('no'));
    return (mockLibrary().notes.get(demoPath(open))!.note.objects ?? []) as PageObject[];
  };
  /** The equation tool, a tap on the page, `latex` typed and drawn. */
  const startNew = async (latex: string, at: [number, number] = [6000, 9000], toolLabel = 'Équation') => {
    tool(toolLabel).click();
    await tick();
    await tap(...at);
    await write(latex);
    await drawn();
  };
  /** Puts `latex` down with the button, the tool still the equation tool. */
  const place = async (latex: string, at: [number, number] = [6000, 9000]) => {
    await startNew(latex, at);
    q<HTMLButtonElement>('[data-action="equation-place"]')!.click();
    await tick();
  };
  return {
    target,
    harness,
    win,
    onclose,
    onpage,
    q,
    tap,
    key,
    field,
    panel,
    write,
    drawn,
    tool,
    mode,
    dirty,
    equations,
    button,
    undoButton,
    ask,
    asked,
    answer,
    leaveQuestion,
    saved,
    startNew,
    place,
    previewPath,
  };
}

const QUESTION_FR = {
  title: 'Équation non posée',
  message: "L'équation en cours n'est pas posée.",
  invalid: null,
  buttons: ['Poser', 'Abandonner', "Continuer l'écriture"],
  placeDisabled: false,
};

describe('another tool', () => {
  it('asks with the toolbar: « Poser » puts the equation down and goes on to the tool', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.tool('Stylo').click();
    await v.asked();
    expect(v.ask()).toEqual(QUESTION_FR);
    // Nothing happened yet: the tool, the panel and the code are as they were.
    expect(v.mode()).toBe('Équation');
    expect(v.field()!.value).toBe('x^2');
    expect(v.equations()).toHaveLength(0);
    expect(v.dirty()).toBe('no');
    await v.answer('place');
    expect(v.ask()).toBeNull();
    expect(v.panel()).toBeNull();
    expect(v.equations()).toHaveLength(1);
    expect(v.mode()).toBe('Stylo');
    expect(v.dirty()).toBe('yes');
    const equation = (await v.saved()).find((o) => o.kind === 'equation') as EquationObject;
    expect(equation.latex).toBe('x^2');
  });

  it('asks with a key pressed outside the field, and « Abandonner » drops the code and goes on', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.field()!.blur();
    await v.key('p');
    await v.asked();
    expect(v.mode()).toBe('Équation');
    await v.answer('drop');
    expect(v.panel()).toBeNull();
    expect(v.equations()).toHaveLength(0);
    expect(v.mode()).toBe('Stylo');
    expect(v.dirty()).toBe('no');
  });

  it('« Continuer l’écriture » changes nothing, the focus back in the field; so does Escape', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.tool('Gomme').click();
    await v.asked();
    await v.answer('keep');
    expect(v.ask()).toBeNull();
    expect(v.mode()).toBe('Équation');
    expect(v.field()!.value).toBe('x^2');
    expect(document.activeElement).toBe(v.field());
    // Asked again, Escape is the same answer: it does not put the code down (the question is not the panel's).
    v.tool('Gomme').click();
    await v.asked();
    await v.key('Escape');
    expect(v.ask()).toBeNull();
    expect(v.panel()).not.toBeNull();
    expect(v.mode()).toBe('Équation');
    expect(v.equations()).toHaveLength(0);
    // Ctrl + W on the question is « Continuer » too.
    v.tool('Gomme').click();
    await v.asked();
    await v.key('w', { ctrlKey: true });
    expect(v.ask()).toBeNull();
    expect(v.panel()).not.toBeNull();
    expect(v.onclose).not.toHaveBeenCalled();
  });

  it('drops nothing behind the question: a second click while it is being prepared is taken by it', async () => {
    const v = await render();
    await v.startNew('x^2');
    // Typed a moment ago: the drawing is not made yet, so the question waits for it.
    await v.write('x^3');
    v.tool('Stylo').click();
    v.tool('Gomme').click();
    await v.asked();
    expect(v.panel()).not.toBeNull();
    expect(v.field()!.value).toBe('x^3');
    expect(v.mode()).toBe('Équation');
    await v.answer('drop');
    // Only the first request went on.
    expect(v.mode()).toBe('Stylo');
  });

  it('cannot put down a code that is incorrect: « Poser » is disabled and the question says why', async () => {
    const v = await render();
    await v.startNew('x^2');
    await v.write('\\frac{1');
    v.tool('Stylo').click();
    await v.asked();
    expect(v.ask()).toEqual({
      ...QUESTION_FR,
      invalid: 'Le code est vide ou incorrect : rien ne peut être posé.',
      placeDisabled: true,
    });
    await v.answer('keep');
    expect(v.mode()).toBe('Équation');
    expect(v.field()!.value).toBe('\\frac{1');
    v.tool('Stylo').click();
    await v.asked();
    await v.answer('drop');
    expect(v.mode()).toBe('Stylo');
    expect(v.equations()).toHaveLength(0);
  });

  it('asks nothing when the field is empty, or when the equation reopened is as it was', async () => {
    const v = await render();
    v.tool('Équation').click();
    await tick();
    await v.tap(6000, 9000);
    expect(v.panel()).not.toBeNull();
    v.tool('Stylo').click();
    await tick();
    expect(v.ask()).toBeNull();
    expect(v.mode()).toBe('Stylo');
    expect(v.panel()).toBeNull();
    // A code made of spaces is no code.
    v.tool('Équation').click();
    await tick();
    await v.tap(6000, 9000);
    await v.write('   ');
    v.tool('Stylo').click();
    await tick();
    expect(v.ask()).toBeNull();
    // An equation of the page, reopened and left as it was.
    await v.place('E = mc^2');
    const [box] = ((await v.saved()).filter((o) => o.kind === 'equation') as EquationObject[]).map((o) => ({ x: o.x, y: o.y + (o.height * o.scale) / 2 }));
    await v.tap(box.x + 20, box.y);
    expect(v.field()!.value).toBe('E = mc^2');
    v.tool('Stylo').click();
    await tick();
    expect(v.ask()).toBeNull();
    expect(v.mode()).toBe('Stylo');
    expect(v.equations()).toHaveLength(1);
  });

  it('asks about the changes made to an equation of the page: « Mettre à jour » updates it, then goes on', async () => {
    const v = await render();
    await v.place('E = mc^2');
    const equation = ((await v.saved()).find((o) => o.kind === 'equation') as EquationObject);
    await v.tap(equation.x + 20, equation.y + (equation.height * equation.scale) / 2);
    await v.drawn();
    await v.write('E = hf');
    v.tool('Stylo').click();
    await v.asked();
    expect(v.ask()).toEqual({
      ...QUESTION_FR,
      message: "Les changements de l'équation ne sont pas posés.",
      buttons: ['Mettre à jour', 'Abandonner', "Continuer l'écriture"],
    });
    await v.answer('place');
    expect(v.mode()).toBe('Stylo');
    const updated = ((await v.saved()).find((o) => o.kind === 'equation') as EquationObject);
    expect([updated.id, updated.latex]).toEqual([equation.id, 'E = hf']);
  });

  it('asks about an equation of the page whose field was emptied, and « Abandonner » leaves it as it was', async () => {
    const v = await render();
    await v.place('E = mc^2');
    const equation = ((await v.saved()).find((o) => o.kind === 'equation') as EquationObject);
    await v.tap(equation.x + 20, equation.y + (equation.height * equation.scale) / 2);
    await v.write('');
    v.tool('Stylo').click();
    await v.asked();
    expect(v.ask()?.placeDisabled).toBe(true);
    await v.answer('drop');
    expect(v.mode()).toBe('Stylo');
    expect(v.equations()).toHaveLength(1);
    expect(v.dirty()).toBe('no');
  });

  it('asks when another equation, or a new one, would take the place of a panel with changes', async () => {
    const v = await render();
    await v.place('E = mc^2');
    const equation = ((await v.saved()).find((o) => o.kind === 'equation') as EquationObject);
    await v.tap(equation.x + 20, equation.y + (equation.height * equation.scale) / 2);
    await v.write('E = hf');
    // A tap elsewhere would start a new equation there: asked first.
    await v.tap(6000, 16000);
    await v.asked();
    expect(v.field()!.value).toBe('E = hf');
    await v.answer('keep');
    expect(v.field()!.value).toBe('E = hf');
    await v.tap(6000, 16000);
    await v.asked();
    await v.answer('drop');
    // The new panel is there, empty, at the tap.
    expect(v.panel()).not.toBeNull();
    expect(v.field()!.value).toBe('');
    expect(v.dirty()).toBe('no');
  });

  it('is in English', async () => {
    setLocale('en');
    const v = await render();
    await v.startNew('x^2', [6000, 9000], 'Equation');
    v.tool('Pen').click();
    await v.asked();
    expect(v.ask()).toEqual({
      title: 'Equation not placed',
      message: 'The equation being written has not been placed.',
      invalid: null,
      buttons: ['Place', 'Drop it', 'Keep writing'],
      placeDisabled: false,
    });
    await v.answer('keep');
  });
});

describe('undo and redo', () => {
  it('ask, without « Poser » (it would take the equation away at once), then undo what was before', async () => {
    const v = await render();
    await v.place('E = mc^2');
    expect(v.equations()).toHaveLength(1);
    await v.startNew('x^2', [6000, 15000]);
    v.undoButton().click();
    await v.asked();
    expect(v.ask()?.buttons).toEqual(['Abandonner', "Continuer l'écriture"]);
    await v.answer('keep');
    expect(v.equations()).toHaveLength(1);
    expect(v.field()!.value).toBe('x^2');
    v.undoButton().click();
    await v.asked();
    await v.answer('drop');
    // The code is dropped and the undo is done: the first equation is gone.
    expect(v.panel()).toBeNull();
    expect(v.equations()).toHaveLength(0);
  });
});

const splitButton = (v: Awaited<ReturnType<typeof render>>) =>
  [...v.target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.getAttribute('title') === 'Détacher une partie de la note dans une nouvelle note')!;

describe('the replay and the split', () => {
  it('the replay asks first, and starts once answered', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.q<HTMLButtonElement>('[data-action="replay-toggle"]')!.click();
    await v.asked();
    expect(v.q<HTMLButtonElement>('[data-action="replay-toggle"]')!.getAttribute('aria-pressed')).not.toBe('true');
    await v.answer('place');
    expect(v.panel()).toBeNull();
    expect(v.q<HTMLButtonElement>('[data-action="replay-toggle"]')!.getAttribute('aria-pressed')).toBe('true');
  });

  it('the split panel asks first too', async () => {
    const v = await render();
    await v.startNew('x^2');
    splitButton(v).click();
    await v.asked();
    await v.answer('drop');
    expect(v.panel()).toBeNull();
    expect(v.equations()).toHaveLength(0);
    expect(splitButton(v).getAttribute('aria-pressed')).toBe('true');
  });
});

describe('leaving the note', () => {
  it('the back button asks; « Abandonner » goes back at once (nothing else is unsaved)', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.q<HTMLButtonElement>('.editor-top button.ghost')!.click();
    await v.asked();
    expect(v.onclose).not.toHaveBeenCalled();
    await v.answer('drop');
    expect(v.onclose).toHaveBeenCalledTimes(1);
  });

  it('the back button: « Poser » puts it down, and the usual question about unsaved changes follows', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.q<HTMLButtonElement>('.editor-top button.ghost')!.click();
    await v.asked();
    await v.answer('place');
    expect(v.equations()).toHaveLength(1);
    expect(v.leaveQuestion()).toEqual(['Enregistrer et revenir', 'Revenir sans enregistrer', 'Rester']);
    expect(v.onclose).not.toHaveBeenCalled();
    v.button('Enregistrer et revenir')!.click();
    await vi.waitFor(() => expect(v.onclose).toHaveBeenCalledTimes(1));
    const stored = (mockLibrary().notes.get(demoPath(SHOPPING))!.note.objects ?? []) as PageObject[];
    expect(stored.filter((o) => o.kind === 'equation')).toHaveLength(1);
  });

  it('Ctrl + W, from the field itself, asks the same way', async () => {
    const v = await render();
    await v.startNew('x^2');
    expect(await v.key('w', { ctrlKey: true }, v.field()!)).toBe(false);
    await v.asked();
    expect(v.onclose).not.toHaveBeenCalled();
    await v.answer('keep');
    expect(v.panel()).not.toBeNull();
    expect(v.onclose).not.toHaveBeenCalled();
  });

  it('Escape puts a valid code down as before; then the next ones ask about the unsaved note', async () => {
    const v = await render();
    await v.startNew('x^2');
    await v.key('Escape', {}, v.field()!);
    expect(v.ask()).toBeNull();
    expect(v.equations()).toHaveLength(1);
    // Deselect, then back: the question of the unsaved note (the equation is on the page).
    await v.key('Escape');
    await v.key('Escape');
    expect(v.leaveQuestion()).not.toBeNull();
    expect(v.onclose).not.toHaveBeenCalled();
  });

  it('the window closed holds back and asks; « Abandonner » then closes it, « Poser » asks about the unsaved note', async () => {
    const v = await render();
    await v.startNew('x^2');
    expect(v.win.request()).toBe(true);
    await v.asked();
    expect(v.win.destroy).not.toHaveBeenCalled();
    await v.answer('keep');
    expect(v.win.request()).toBe(true);
    await v.asked();
    await v.answer('drop');
    await vi.waitFor(() => expect(v.win.destroy).toHaveBeenCalledTimes(1));
    expect(v.equations()).toHaveLength(0);
  });

  it('the window closed, « Poser »: the note then asks whether to save before the window goes', async () => {
    const v = await render();
    await v.startNew('x^2');
    expect(v.win.request()).toBe(true);
    await v.asked();
    // A second close request while the question is open holds the window too, and asks nothing more.
    expect(v.win.request()).toBe(true);
    await v.answer('place');
    expect(v.leaveQuestion()).toEqual(['Enregistrer et fermer', 'Fermer sans enregistrer', 'Rester']);
    expect(v.win.destroy).not.toHaveBeenCalled();
  });

  it('a clean note with nothing to ask closes the window as before', async () => {
    const v = await render();
    v.tool('Équation').click();
    await tick();
    await v.tap(6000, 9000);
    expect(v.win.request()).toBe(false);
    expect(v.ask()).toBeNull();
  });

  it('another page asks first: « Abandonner » goes to it, « Continuer » stays', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.field()!.blur();
    await v.key('ArrowRight', { altKey: true });
    await v.asked();
    await v.answer('keep');
    expect(v.onpage).not.toHaveBeenCalled();
    expect(v.field()!.value).toBe('x^2');
    v.q<HTMLButtonElement>('[data-action="page-next"]')!.click();
    await v.asked();
    await v.answer('drop');
    await vi.waitFor(() => expect(v.onpage).toHaveBeenCalledWith(demoPath(PARENTS)));
    expect(v.equations()).toHaveLength(0);
  });

  it('another page: « Poser » puts it down, then the note asks about its unsaved changes', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.q<HTMLButtonElement>('[data-action="page-previous"]')!.click();
    await v.asked();
    await v.answer('place');
    expect(v.equations()).toHaveLength(1);
    expect(v.leaveQuestion()).toEqual(["Enregistrer et ouvrir l'autre note", 'Ouvrir sans enregistrer', 'Rester']);
    expect(v.onpage).not.toHaveBeenCalled();
  });

  it('a note given by the screen asks too, and the screen follows this note again when the writing goes on', async () => {
    const v = await render();
    await v.startNew('x^2');
    v.harness.setPath(demoPath(PARENTS));
    await v.asked();
    await v.answer('keep');
    // The screen holds this note again: `onsaved` tells it so.
    expect(v.harness.held()).toBe(demoPath(SHOPPING));
    expect(v.field()!.value).toBe('x^2');
    v.harness.setPath(demoPath(PARENTS));
    await v.asked();
    await v.answer('drop');
    await vi.waitFor(() => expect(v.q('section.editor')!.getAttribute('data-path')).toBe(demoPath(PARENTS)));
    expect(v.panel()).toBeNull();
  });
});

describe('the assistant', () => {
  it('cannot move the editor away from an equation being written, and can once it is answered for', async () => {
    const v = await render();
    expect(bridgeScreens.editor?.blocker()).toBeNull();
    v.tool('Équation').click();
    await tick();
    await v.tap(6000, 9000);
    // An empty field is nothing to lose.
    expect(bridgeScreens.editor?.blocker()).toBeNull();
    await v.write('x^2');
    expect(bridgeScreens.editor?.blocker()).toBe('an equation is being written in the editor: the teacher must put it down or drop it first');
    await v.drawn();
    v.q<HTMLButtonElement>('[data-action="equation-place"]')!.click();
    await tick();
    // Put down: the note has unsaved changes now, which is the usual reason.
    expect(bridgeScreens.editor?.blocker()).toContain('unsaved changes');
  });
});

describe('an export of the page', () => {
  it('asks first, so that the export has the equation: « Poser » then opens the dialog', async () => {
    const v = await render();
    await v.startNew('x^2');
    [...v.target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.trim().endsWith('Exporter…'))!.click();
    await v.asked();
    expect(document.querySelector('.modal[role="dialog"]')).toBeNull();
    await v.answer('place');
    expect(v.equations()).toHaveLength(1);
    expect(document.querySelector('.modal[role="dialog"]')).not.toBeNull();
  });
});
