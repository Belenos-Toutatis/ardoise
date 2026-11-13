// @vitest-environment jsdom
/// <reference types="node" />
/**
 * Escape in the editor, by steps (SPEC §4.4, docs/questions.md Q20; `lib/editor/escape.ts`): the gesture under way, then
 * the panel or tool mode that is open, then the selection, then the note is saved and the editor goes back to the
 * library, through the back button's own path (nothing during a recording, the failing save keeps the editor open).
 * Dialogs and a field typed in keep their own Escape. The order alone is `lib/editor/escape.test.ts`.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { openPdfPreviews } from '../../lib/mock-background';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockNarration, resetMockNarration, setMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import { share } from '../../lib/share-state.svelte';
import type { Stroke } from '../../lib/note';
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

/** A stroke of 21 samples spread over [t0, t1] ms. */
function slowStroke(y: number, t0: number, t1: number): Stroke {
  return {
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer: 0,
    points: Array.from({ length: 21 }, (_, i) => ({ x: 2000 + i * 500, y, pressure: 0.5, t: Math.round(t0 + ((t1 - t0) * i) / 20) })),
  };
}

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockNarration();
  resetMockShare();
  share.reset();
  mockNarration.receivingAfterMs = 0;
  setLocale('fr');
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
  const entry = mockLibrary().notes.get(demoPath(TITLE))!;
  entry.note = { ...entry.note, strokes: [slowStroke(3000, 0, 1000), slowStroke(6000, 3000, 4000), slowStroke(9000, 4200, 5000)] };
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.useRealTimers();
  vi.restoreAllMocks();
  share.reset();
  resetMockShare();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function render() {
  const path = demoPath(TITLE);
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose, onsaved } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const q = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const action = (name: string) => q<HTMLButtonElement>(`[data-action="${name}"]`);
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
  /** A complete drag from (x0, y0) to (x1, y1). */
  const drag = async (x0: number, y0: number, x1: number, y1: number) => {
    pointer('pointerdown', x0, y0);
    for (let i = 1; i <= 5; i++) pointer('pointermove', x0 + ((x1 - x0) * i) / 5, y0 + ((y1 - y0) * i) / 5);
    pointer('pointerup', x1, y1);
    await tick();
  };
  /** A key on the window; returns whether the page kept it (false: something took it with `preventDefault`). */
  const key = async (k: string, init: KeyboardEventInit = {}, on: EventTarget = window) => {
    const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init });
    on.dispatchEvent(event);
    on.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
    return !event.defaultPrevented;
  };
  const escape = (init: KeyboardEventInit = {}, on: EventTarget = window) => key('Escape', init, on);
  const strokes = () => target.querySelectorAll('g.ink path:not(.live)').length;
  const live = () => target.querySelectorAll('g.ink path.live').length;
  const selected = () => target.querySelector('[data-section="selection"]') !== null;
  const dirty = () => q('section.editor')!.getAttribute('data-dirty');
  const notice = () => q('.editor-notice')?.textContent?.trim() ?? null;
  const mode = () => q('.toolbar .tool[aria-pressed="true"]')?.title;
  const tool = (label: string) => [...target.querySelectorAll<HTMLButtonElement>('.toolbar .tool')].find((b) => b.title === label)!;
  /** A change to the note: a stroke drawn with the pen. */
  const change = async () => {
    await drag(5000, 12_000, 9000, 12_000);
    expect(dirty()).toBe('yes');
  };
  const selectAll = () => key('a', { ctrlKey: true });
  return { path, target, svg, q, action, pointer, drag, key, escape, strokes, live, selected, dirty, notice, mode, tool, change, selectAll, onclose, onsaved };
}

describe('step 1: the action in progress', () => {
  it('drops the stroke being drawn, and the editor stays', async () => {
    const { pointer, escape, strokes, live, onclose, dirty } = await render();
    expect(strokes()).toBe(3);
    pointer('pointerdown', 5000, 12_000);
    pointer('pointermove', 7000, 12_000);
    pointer('pointermove', 9000, 12_000);
    // The line being drawn is shown at the next frame.
    await vi.waitFor(() => expect(live()).toBe(1));
    expect(await escape()).toBe(false);
    expect(live()).toBe(0);
    // Let go: nothing is added.
    pointer('pointerup', 9000, 12_000);
    await tick();
    expect(strokes()).toBe(3);
    expect(dirty()).toBe('no');
    expect(onclose).not.toHaveBeenCalled();
  });

  it('drops a lasso being drawn without selecting anything', async () => {
    const { pointer, escape, tool, selected, onclose } = await render();
    tool('Sélection').click();
    await tick();
    const lasso = () => document.querySelectorAll('svg.canvas polyline.lasso').length;
    pointer('pointerdown', 1000, 1000);
    pointer('pointermove', 29_000, 1000);
    pointer('pointermove', 29_000, 20_000);
    await tick();
    expect(lasso()).toBe(1);
    await escape();
    expect(lasso()).toBe(0);
    pointer('pointerup', 1000, 20_000);
    await tick();
    expect(selected()).toBe(false);
    expect(onclose).not.toHaveBeenCalled();
  });

  it('answers « Rester » to the unsaved-changes question', async () => {
    const { change, q, escape, onclose, dirty } = await render();
    await change();
    q<HTMLButtonElement>('.editor-top button.ghost')!.click();
    await tick();
    expect(document.querySelector('[role="alertdialog"]')).not.toBeNull();
    await escape();
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
    expect(onclose).not.toHaveBeenCalled();
    expect(dirty()).toBe('yes');
  });

  it('drops the question about deleting the narration', async () => {
    setMockNarration(demoPath(TITLE), { durationMs: 2000, base: 'note', speed: 1, maxPauseMs: null, startMs: 5800 });
    const { target, action, escape, onclose } = await render();
    await openSideTab(target, 'sound');
    action('narration-delete')!.click();
    await tick();
    expect(action('narration-delete-yes')).toBeTruthy();
    await escape();
    expect(action('narration-delete-yes')).toBeNull();
    expect(action('narration-delete')).toBeTruthy();
    expect(onclose).not.toHaveBeenCalled();
  });
});

describe('step 1 again: a question about the background', () => {
  it('drops the choice of the page of a PDF, nothing is put under the writing, and the editor stays', async () => {
    const stage = vi.spyOn(api, 'stageBackgroundImage');
    const { target, action, escape, onclose, dirty } = await render();
    (window as unknown as { __ardoiseE2eOpenPath?: string }).__ardoiseE2eOpenPath = 'C:\\Documents\\fiche.pdf';
    try {
      await openSideTab(target, 'background');
      action('background-choose')!.click();
      await vi.waitFor(() => expect(target.querySelector('[data-slot="background-pdf"]')).toBeTruthy());
      await escape();
      expect(target.querySelector('[data-slot="background-pdf"]')).toBeNull();
      await vi.waitFor(() => expect(openPdfPreviews()).toBe(0));
      expect(stage).not.toHaveBeenCalled();
      expect(onclose).not.toHaveBeenCalled();
      expect(dirty()).toBe('no');
    } finally {
      delete (window as unknown as { __ardoiseE2eOpenPath?: string }).__ardoiseE2eOpenPath;
    }
  });
});

describe('step 2: the panel or the tool mode that is open', () => {
  it('closes the cutting panel', async () => {
    const { target, escape, onclose } = await render();
    [...target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.trim().endsWith('Découper'))!.click();
    await tick();
    expect(target.querySelector('.split-panel')).not.toBeNull();
    await escape();
    expect(target.querySelector('.split-panel')).toBeNull();
    expect(onclose).not.toHaveBeenCalled();
  });

  it('closes the replay', async () => {
    const { target, key, escape, onclose } = await render();
    await key('r');
    expect(target.querySelector('.replay-bar')).not.toBeNull();
    await escape();
    expect(target.querySelector('.replay-bar')).toBeNull();
    expect(onclose).not.toHaveBeenCalled();
  });

  it('closes the manual crop, back to the selection tool', async () => {
    const { target, action, escape, mode, onclose } = await render();
    await openSideTab(target, 'page');
    action('manual-crop')!.click();
    await tick();
    // No tool of the bar is the one in use while the crop is.
    expect(mode()).toBeUndefined();
    expect(action('manual-crop')!.getAttribute('aria-pressed')).toBe('true');
    await escape();
    expect(action('manual-crop')!.getAttribute('aria-pressed')).toBe('false');
    expect(mode()).toBe('Sélection');
    expect(onclose).not.toHaveBeenCalled();
  });

  it('closes the setting of « Redresser les formes » before the selection', async () => {
    const { q, action, tool, selectAll, escape, selected, onclose } = await render();
    // The setting belongs to the selection tool.
    tool('Sélection').click();
    await tick();
    await selectAll();
    action('straighten-settings')!.click();
    await tick();
    expect(q('[data-panel="straighten"]')).not.toBeNull();
    await escape();
    expect(q('[data-panel="straighten"]')).toBeNull();
    expect(selected()).toBe(true);
    expect(onclose).not.toHaveBeenCalled();
  });

  it('goes by the order: the replay, then the selection, then the library', async () => {
    const { target, selectAll, key, escape, selected, onclose } = await render();
    await selectAll();
    await key('r');
    expect(target.querySelector('.replay-bar')).not.toBeNull();
    await escape();
    expect(target.querySelector('.replay-bar')).toBeNull();
    expect(selected()).toBe(true);
    await escape();
    expect(selected()).toBe(false);
    expect(onclose).not.toHaveBeenCalled();
    await escape();
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('goes by the order: the setting, then the selection, then the library', async () => {
    const { q, action, tool, selectAll, escape, selected, onclose } = await render();
    tool('Sélection').click();
    await tick();
    await selectAll();
    action('straighten-settings')!.click();
    await tick();
    await escape();
    expect(q('[data-panel="straighten"]')).toBeNull();
    expect(selected()).toBe(true);
    await escape();
    expect(selected()).toBe(false);
    expect(onclose).not.toHaveBeenCalled();
    await escape();
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});

describe('step 3: the selection', () => {
  it('deselects, and the editor stays', async () => {
    const { selectAll, escape, selected, onclose } = await render();
    await selectAll();
    expect(selected()).toBe(true);
    await escape();
    expect(selected()).toBe(false);
    expect(onclose).not.toHaveBeenCalled();
  });

  it('does the same with Shift', async () => {
    const { selectAll, escape, selected } = await render();
    await selectAll();
    await escape({ shiftKey: true });
    expect(selected()).toBe(false);
  });
});

describe('step 4: the note is saved and the editor goes back to the library', () => {
  it('leaves at once when nothing is unsaved', async () => {
    const { escape, onclose } = await render();
    expect(await escape()).toBe(false);
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('saves the changes first, without the question, then leaves', async () => {
    const saveNote = vi.spyOn(api, 'saveNote');
    const { change, escape, onclose, onsaved, path } = await render();
    await change();
    await escape();
    await vi.waitFor(() => expect(onclose).toHaveBeenCalledTimes(1));
    expect(saveNote).toHaveBeenCalledTimes(1);
    expect(onsaved).toHaveBeenCalledWith(path);
    // The saved note holds the stroke drawn.
    expect(mockLibrary().notes.get(path)!.note.strokes).toHaveLength(4);
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('presses once, leaves once: a second press while it saves does nothing more', async () => {
    const { change, escape, onclose } = await render();
    await change();
    await Promise.all([escape(), escape()]);
    await vi.waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('a key held down is one press', async () => {
    const { selectAll, escape, selected, onclose } = await render();
    await selectAll();
    await escape();
    await escape({ repeat: true });
    await escape({ repeat: true });
    expect(selected()).toBe(false);
    expect(onclose).not.toHaveBeenCalled();
  });

  it('a save that fails keeps the editor open and says why', async () => {
    vi.spyOn(api, 'saveNote').mockRejectedValue({ code: 'io', message: 'Disque plein' });
    const { change, escape, onclose, q, dirty } = await render();
    await change();
    await escape();
    await vi.waitFor(() => expect(q('.editor-banner[role="alert"]')).not.toBeNull());
    expect(q('.editor-banner[role="alert"]')!.textContent).toContain("Impossible d'enregistrer la note");
    expect(onclose).not.toHaveBeenCalled();
    expect(dirty()).toBe('yes');
    // The way out is still there: a press after the failure tries again, and fails the same.
    await escape();
    await vi.waitFor(() => expect(vi.mocked(api.saveNote)).toHaveBeenCalledTimes(2));
    expect(onclose).not.toHaveBeenCalled();
  });

  it('a note changed on the disk meanwhile is not overwritten: the conflict is shown and the editor stays', async () => {
    vi.spyOn(api, 'saveNote').mockRejectedValue({ code: 'noteConflict', message: 'changed' });
    const { change, escape, onclose, q } = await render();
    await change();
    await escape();
    await vi.waitFor(() => expect(q('.editor-banner[role="alert"] strong')).not.toBeNull());
    expect(q('.editor-banner[role="alert"] strong')!.textContent).toBe("Vos modifications n'ont pas été enregistrées");
    expect(onclose).not.toHaveBeenCalled();
  });

  it('waits for a title typed and not yet renamed: it is saved with the note', async () => {
    const retitle = vi.spyOn(api, 'retitleNote');
    const { q, escape, key, onclose } = await render();
    const field = q<HTMLInputElement>('.title-field input')!;
    field.value = 'Courses de la semaine';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    // The key typed in the field is the field's own (it gives the old title back)…
    await key('Escape', {}, field);
    expect(onclose).not.toHaveBeenCalled();
    expect(field.value).toBe('Liste de courses');
    // … and a title typed then committed by leaving the field is saved before the editor goes.
    field.value = 'Courses de la semaine';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.dispatchEvent(new Event('blur'));
    await tick();
    await escape();
    await vi.waitFor(() => expect(onclose).toHaveBeenCalledTimes(1));
    expect(retitle).toHaveBeenCalled();
  });
});

describe('what Escape leaves alone', () => {
  it('does nothing at the last step while a narration is being recorded, and says to stop it first', async () => {
    const { target, action, escape, onclose, notice } = await render();
    vi.useFakeTimers();
    await openSideTab(target, 'sound');
    if (!action('narration-start')) {
      action('narration-open')!.click();
      await tick();
    }
    action('narration-start')!.click();
    await vi.waitFor(() => expect(target.querySelector('[data-tab-panel="sound"] .record')?.getAttribute('data-phase')).toBe('recording'));
    await vi.advanceTimersByTimeAsync(0);
    await escape();
    expect(onclose).not.toHaveBeenCalled();
    expect(notice()).toBe("Arrêtez ou annulez d'abord l'enregistrement de la narration.");
  });

  it('is the field’s own in a text field and in a select', async () => {
    const { q, selectAll, key, selected, onclose } = await render();
    await selectAll();
    const select = q<HTMLSelectElement>('.toolbar select')!;
    expect(await key('Escape', {}, select)).toBe(true);
    expect(selected()).toBe(true);
    const field = q<HTMLInputElement>('.title-field input')!;
    field.focus();
    await key('Escape', {}, field);
    expect(selected()).toBe(true);
    expect(onclose).not.toHaveBeenCalled();
  });

  it('is the dialog’s own: the export dialog closes with it and the editor stays', async () => {
    const { target, escape, selectAll, selected, onclose } = await render();
    await selectAll();
    [...target.querySelectorAll<HTMLButtonElement>('.toolbar button')].find((b) => b.textContent?.trim().endsWith('Exporter…'))!.click();
    await tick();
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    await escape();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    // Nothing of the editor's own: the selection is still there, the editor is still open.
    expect(selected()).toBe(true);
    expect(onclose).not.toHaveBeenCalled();
  });

  it('ignores Ctrl, Alt and Meta + Escape', async () => {
    const { selectAll, key, selected, onclose } = await render();
    await selectAll();
    for (const init of [{ ctrlKey: true }, { altKey: true }, { metaKey: true }]) await key('Escape', init);
    expect(selected()).toBe(true);
    expect(onclose).not.toHaveBeenCalled();
  });
});

describe('the back button', () => {
  it('says that Escape does it too, in both languages', async () => {
    const fr = await render();
    expect(fr.q('.editor-top button.ghost')!.getAttribute('title')).toBe("Revenir à la bibliothèque (Échap quand rien d'autre n'est ouvert ni sélectionné)");
    cleanup?.();
    setLocale('en');
    const en = await render();
    expect(en.q('.editor-top button.ghost')!.getAttribute('title')).toBe('Back to the library (Esc when nothing else is open or selected)');
  });
});
