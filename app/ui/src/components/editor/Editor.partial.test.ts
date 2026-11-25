// @vitest-environment jsdom
/// <reference types="node" />
/**
 * A note made from only part of its tablet page (Q27): the editor says so under the top bar, until the whole page comes
 * back with « Revenir à l'original » and is saved. What it says goes with the undo history, and into the save.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockDrafts, startNewMockSession } from '../../lib/mock-drafts';
import { freshStamp, mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { Stroke } from '../../lib/note';
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

const PHYSICS = 'Cours de physique';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockDrafts();
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

const entryOf = (path: string) => mockLibrary().notes.get(path)!;

/**
 * The demo's tablet note as the sync leaves a page read in part: its first stroke only. `readable`: this version reads
 * the whole page.
 */
function readInPart(readable: boolean): { path: string; whole: Stroke[] } {
  const path = demoPath(PHYSICS);
  const entry = entryOf(path);
  const original = entry.original;
  if (!original || original.source === 'none') throw new Error('a tablet note has an original');
  const whole = structuredClone(original.strokes);
  const part = whole.slice(0, 1);
  entry.partialPage = true;
  entry.note = { ...entry.note, strokes: structuredClone(part) };
  entry.original = readable ? original : { ...original, strokes: part, partial: true };
  return { path, whole };
}

async function render(path: string) {
  const target = document.createElement('div');
  document.body.append(target);
  const onsaved = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn(), onsaved } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const banner = () => target.querySelector<HTMLElement>('[data-slot="partial-page"]');
  const named = (text: string) => [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text)!;
  const key = async (k: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
    window.dispatchEvent(new KeyboardEvent('keyup', { key: k, bubbles: true, ...init }));
    await tick();
  };
  const dirty = () => target.querySelector('.editor')?.getAttribute('data-dirty');
  const save = async () => {
    onsaved.mockClear();
    await key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(onsaved).toHaveBeenCalled());
  };
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const pointer = (type: string, x: number, y: number) => {
    const [scale, vx, vy] = (svg.getAttribute('data-view') ?? '1 0 0').split(' ').map(Number);
    const options = {
      bubbles: true,
      cancelable: true,
      clientX: x * scale + vx,
      clientY: y * scale + vy,
      button: 0,
      buttons: 1,
      pointerId: 1,
      pressure: 0.5,
    };
    const Ctor = (globalThis as { PointerEvent?: typeof MouseEvent }).PointerEvent ?? MouseEvent;
    const event = new Ctor(type, options);
    for (const name of ['pointerId', 'pressure'] as const) {
      if (!(name in event)) Object.defineProperty(event, name, { value: options[name] });
    }
    svg.dispatchEvent(event);
  };
  /** A stroke written: one undoable step. */
  const edit = async () => {
    pointer('pointerdown', 3000, 15000);
    for (let i = 1; i <= 10; i++) pointer('pointermove', 3000 + i * 500, 15000);
    pointer('pointerup', 8000, 15000);
    await tick();
    expect(dirty()).toBe('yes');
  };
  const goBack = async () => {
    const section = target.querySelector<HTMLElement>('[data-section="original"]')!;
    await vi.waitFor(() =>
      expect(section.querySelector<HTMLButtonElement>('[data-action="original"]')!.disabled).toBe(false),
    );
    section.querySelector<HTMLButtonElement>('[data-action="original"]')!.click();
    await tick();
    section.querySelector<HTMLButtonElement>('[data-action="original-confirm"]')!.click();
    await tick();
  };
  /** The button whose text or name starts with `text`. */
  const notice = () => target.querySelector('.editor-notice')?.textContent?.trim();
  const originalHelp = () => target.querySelector('[data-slot="original-help"]')?.textContent?.trim();
  /** Splits the note at 150 ms. */
  const splitAt150 = async () => {
    titled('Découper').click();
    await tick();
    const slider = target.querySelector<HTMLInputElement>('.split-panel input[type="range"]')!;
    slider.value = '150';
    slider.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    [...target.querySelectorAll<HTMLButtonElement>('.split-panel button')].find((b) => b.textContent?.trim() === 'Découper')!.click();
    await vi.waitFor(() => expect(target.querySelector('.split-panel')).toBeNull());
  };
  const titled = (text: string) =>
    [...target.querySelectorAll('button')].find(
      (b) => b.textContent?.trim().endsWith(text) || b.getAttribute('aria-label')?.startsWith(text) || b.title.startsWith(text),
    )!;
  return { target, banner, named, titled, key, save, edit, goBack, splitAt150, notice, originalHelp, dirty, onsaved };
}

describe('the warning of a page read in part', () => {
  it('is not there for an ordinary note, whose save says nothing of its page', async () => {
    const save = vi.spyOn(api, 'saveNote');
    const view = await render(demoPath(PHYSICS));
    expect(view.banner()).toBeNull();
    await view.edit();
    await view.save();
    expect(save.mock.calls[0]).toHaveLength(4);
    expect(entryOf(demoPath(PHYSICS)).partialPage).toBeUndefined();
  });

  it.each([
    ['fr', 'Page lue en partie', "« Revenir à l'original » (onglet « Page »)", 'toutes les données de la tablette', 'Fermer'],
    ['en', 'Page read in part', '"Back to the original" (the "Page" tab)', "all the tablet's data", 'Dismiss'],
  ] as const)('says what is safe and what to do in %s, until it is closed', async (locale, title, action, safe, close) => {
    setLocale(locale);
    const { path } = readInPart(false);
    const view = await render(path);
    expect(view.banner()?.getAttribute('role')).toBe('status');
    const text = view.banner()!.textContent!;
    expect(text).toContain(title);
    expect(text).toContain(safe);
    expect(text).toContain(action);
    expect(text).toContain('_ardoise\\diagnostics');
    // The banner names the button and the tab as they are.
    const button = view.target.querySelector<HTMLElement>('[data-section="original"] [data-action="original"]')!;
    expect(action).toContain(button.textContent!.replace('↺', '').trim());
    [...view.banner()!.querySelectorAll('button')].find((b) => b.textContent?.trim() === close)!.click();
    await tick();
    expect(view.banner()).toBeNull();
    // Closed for this time: it is there the next time the note is opened.
    cleanup?.();
    const again = await render(path);
    expect(again.banner()).toBeTruthy();
  });

  it('stays after an edit is saved, and after going back to an original still read in part', async () => {
    const { path } = readInPart(false);
    const save = vi.spyOn(api, 'saveNote');
    const view = await render(path);
    await view.edit();
    await view.save();
    expect(view.banner()).toBeTruthy();
    expect(entryOf(path).partialPage).toBe(true);
    expect(save.mock.calls[0]!.slice(3)).toEqual([undefined, undefined, true]);
    // It is said of the page, never in the note itself.
    expect(Object.keys(save.mock.calls[0]![1])).not.toContain('partialPage');

    await view.goBack();
    expect(view.banner()).toBeTruthy();
    await view.save();
    expect(save.mock.calls[1]!.slice(3)).toEqual([undefined, undefined, true]);
    expect(entryOf(path).partialPage).toBe(true);
  });

  it('goes with the whole page once this version reads it, comes back with undo, and follows the saves', async () => {
    const { path, whole } = readInPart(true);
    expect(whole).toHaveLength(2);
    const view = await render(path);
    await vi.waitFor(() => expect(view.banner()?.textContent).toContain('Cette version d\'Ardoise lit la page en entier'));
    expect(view.banner()!.textContent).toContain("« Revenir à l'original » (onglet « Page »)");
    // Looking is not going back.
    await view.edit();
    await view.save();
    expect(entryOf(path).partialPage).toBe(true);

    await view.goBack();
    expect(view.banner()).toBeNull();
    await view.key('z', { ctrlKey: true });
    expect(view.banner()).toBeTruthy();
    await view.key('y', { ctrlKey: true });
    expect(view.banner()).toBeNull();
    await view.save();
    expect(entryOf(path).partialPage).toBe(false);
    expect(entryOf(path).note.strokes).toEqual(whole);

    // Edited afterwards: the note is the whole page's.
    await view.edit();
    await view.save();
    expect(entryOf(path).partialPage).toBe(false);
    // Undone down to the part, and saved: a part again.
    await view.key('z', { ctrlKey: true });
    await view.key('z', { ctrlKey: true });
    expect(view.banner()).toBeTruthy();
    await view.save();
    expect(entryOf(path).partialPage).toBe(true);
  });

  it('is not there for the copy that a conflict makes: it has no tablet page', async () => {
    const { path } = readInPart(false);
    const view = await render(path);
    expect(view.banner()).toBeTruthy();
    await view.edit();
    entryOf(path).stamp = freshStamp();
    await view.key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(view.named('Enregistrer comme copie')).toBeTruthy());
    const save = vi.spyOn(api, 'saveNote');
    view.named('Enregistrer comme copie').click();
    await vi.waitFor(() => expect(view.onsaved).toHaveBeenCalled());
    await tick();
    expect(view.banner()).toBeNull();
    const copy = view.onsaved.mock.calls[0]![0] as string;
    expect(copy).not.toBe(path);
    expect(entryOf(copy).partialPage).toBeUndefined();
    expect(entryOf(path).partialPage).toBe(true);
    expect(save.mock.calls[0]).toHaveLength(4);
    // Nor do the saves that follow say anything.
    await view.edit();
    await view.save();
    expect(save.mock.calls[1]).toHaveLength(4);
  });

  it('goes back to the whole page before a split, which leaves the file as it is marked', async () => {
    const { path } = readInPart(true);
    const save = vi.spyOn(api, 'saveNote');
    const split = vi.spyOn(api, 'splitNote');
    const view = await render(path);
    await view.goBack();
    expect(view.banner()).toBeNull();
    await view.splitAt150();
    expect(split).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0]!.slice(3)).toEqual([undefined, undefined, false]);
    expect(entryOf(path).partialPage).toBe(false);
    expect(view.banner()).toBeNull();
  });

  it('is put back before a split when going back to the whole page was saved, then undone', async () => {
    const { path } = readInPart(true);
    const view = await render(path);
    await view.goBack();
    await view.save();
    expect(entryOf(path).partialPage).toBe(false);
    await view.key('z', { ctrlKey: true });
    expect(view.banner()).toBeTruthy();
    const save = vi.spyOn(api, 'saveNote');
    await view.splitAt150();
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0]!.slice(3)).toEqual([undefined, undefined, true]);
    expect(entryOf(path).partialPage).toBe(true);
    expect(view.banner()).toBeTruthy();
    // Split again: the file says what the note holds, nothing to save first.
    await view.edit();
    await view.save();
    save.mockClear();
    await view.splitAt150();
    expect(save).not.toHaveBeenCalled();
    expect(entryOf(path).partialPage).toBe(true);
  });

  it('is in the draft of the whole page not saved: restored, the page is whole, and saved so', async () => {
    const { path, whole } = readInPart(true);
    const view = await render(path);
    await view.goBack();
    expect(view.banner()).toBeNull();
    const draft = await leftAsDraft(path);
    expect(draft.partialPage).toBe(false);
    expect(entryOf(path).partialPage).toBe(true);

    const again = await render(path);
    expect(again.banner()).toBeTruthy();
    await restore();
    expect(again.banner()).toBeNull();
    await again.save();
    expect(entryOf(path).partialPage).toBe(false);
    expect(entryOf(path).note.strokes).toEqual(whole);
  });

  it('is in the draft of a return to the whole page that was saved, then undone: restored, the note is a part again', async () => {
    const { path } = readInPart(true);
    const view = await render(path);
    await view.goBack();
    await view.save();
    await view.key('z', { ctrlKey: true });
    expect(view.banner()).toBeTruthy();
    expect((await leftAsDraft(path)).partialPage).toBe(true);
    expect(entryOf(path).partialPage).toBe(false);

    const again = await render(path);
    expect(again.banner()).toBeNull();
    await restore();
    expect(again.banner()).toBeTruthy();
    await again.save();
    expect(entryOf(path).partialPage).toBe(true);
  });

  it('follows the opened note when the draft says nothing (written before, or a note never read in part)', async () => {
    const { path } = readInPart(true);
    const entry = entryOf(path);
    const other = { ...structuredClone(entry.note), strokes: [] };
    await api.draftWrite({ path, noteId: entry.note.id, note: other, baseStamp: entry.stamp, imageKey: null });
    startNewMockSession();
    const view = await render(path);
    await restore();
    expect(view.banner()).toBeTruthy();
    await view.save();
    expect(entryOf(path).partialPage).toBe(true);

    // An ordinary note: nothing is said of its page, in the draft either.
    cleanup?.();
    resetMockLibrary();
    resetMockDrafts();
    const plain = demoPath(PHYSICS);
    const editor = await render(plain);
    await editor.edit();
    expect('partialPage' in (await leftAsDraft(plain))).toBe(false);
  });
});

/** The editor goes away without saving, as in a crash, and the app starts again: the draft it left. */
async function leftAsDraft(path: string) {
  cleanup?.();
  cleanup = null;
  const id = entryOf(path).note.id;
  await vi.waitFor(async () => expect(await api.draftFind(id)).toBeTruthy());
  startNewMockSession();
  const draft = await api.draftFind(id);
  if (!draft) throw new Error('no draft');
  return draft;
}

/** Takes the draft offered over the opened note. */
async function restore() {
  const button = () => document.querySelector<HTMLButtonElement>('[data-action="draft-restore"]');
  await vi.waitFor(() => expect(button()).toBeTruthy());
  button()!.click();
  await vi.waitFor(() => expect(document.querySelector('[data-slot="draft-recovery"]')).toBeNull());
}

describe('what « Revenir à l\'original » says of a page read in part', () => {
  it.each([
    ['fr', "Retrouvez ce qu'Ardoise sait lire de la page envoyée par la tablette", "Ce qu'Ardoise sait lire de la page de la tablette est rétabli"],
    ['en', 'Get back what Ardoise can read of the page the tablet sent', "What Ardoise can read of the tablet's page is restored"],
  ] as const)('promises what can be read, not the page as it came (%s)', async (locale, help, done) => {
    setLocale(locale);
    const { path } = readInPart(false);
    const view = await render(path);
    await vi.waitFor(() => expect(view.originalHelp()).toContain(help));
    await view.edit();
    await view.goBack();
    expect(view.notice()).toContain(done);
  });

  it.each([
    [true, 'Ardoise ne sait lire aucun trait de la page'],
    [false, "La page envoyée par la tablette n'a aucun trait"],
  ] as const)('says that the page will be empty (read in part: %s)', async (partial, help) => {
    const { path } = readInPart(false);
    const entry = entryOf(path);
    if (entry.original?.source !== 'tablet') throw new Error('a tablet note');
    entry.original = { ...entry.original, strokes: [], partial: partial || undefined };
    const view = await render(path);
    await vi.waitFor(() => expect(view.originalHelp()).toContain(help));
    await view.goBack();
    expect(view.notice()).toBe('Page vide rétablie : enregistrez pour la garder, ou Ctrl+Z pour annuler.');
  });

  it('says the same as before for a page read whole', async () => {
    const view = await render(demoPath(PHYSICS));
    await vi.waitFor(() => expect(view.originalHelp()).toContain("telle qu'elle est arrivée de la tablette"));
    await view.edit();
    await view.goBack();
    expect(view.notice()).toContain("Encre de l'original rétablie");
  });
});
