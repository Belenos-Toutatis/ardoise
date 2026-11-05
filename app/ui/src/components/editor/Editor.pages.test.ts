// @vitest-environment jsdom
/// <reference types="node" />
/**
 * « Page précédente » / « Page suivante » in the editor's top bar: one note is one page, and the
 * buttons (and Alt + ← / →) go to the note before or after this one in the list the library shows.
 * Leaving follows the rules of leaving the editor: unsaved changes ask first, a note that cannot
 * be saved stays, a narration being recorded or a video being written blocks the move.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { mockShareShown, resetMockShare } from '../../lib/mock-share';
import type { Stroke } from '../../lib/note';
import type { ShareContent } from '../../lib/share';
import { share } from '../../lib/share-state.svelte';
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
};

const PHYSICS = 'Cours de physique';
const SHOPPING = 'Liste de courses';
const PARENTS = 'Réunion parents';
const KEEP = "Enregistrer et ouvrir l'autre note";
const LET_GO = 'Ouvrir sans enregistrer';

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
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

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

/** The editor held like the library screen holds it, on the note titled `open` among `order` (titles; null: no list). */
async function render(open: string, order: string[] | null = [PHYSICS, SHOPPING, PARENTS]) {
  const path = demoPath(open);
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const onpage = vi.fn();
  const component = mount(EditorPagesHarness, {
    target,
    props: { initial: path, pages: order ? order.map(demoPath) : (undefined as never), app, onclose, onsaved, onpage },
  });
  const harness = component as unknown as { held(): string; setPages(list: string[]): void };
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const buttons = () => [...document.querySelectorAll('button')];
  const named = (text: string) => buttons().find((b) => b.textContent?.trim() === text);
  const previous = () => target.querySelector<HTMLButtonElement>('button[data-action="page-previous"]');
  const next = () => target.querySelector<HTMLButtonElement>('button[data-action="page-next"]');
  const position = () => target.querySelector('[data-slot="page-position"]')?.textContent?.trim() ?? null;
  const titleField = () => target.querySelector<HTMLInputElement>('.title-field input')!;
  const dialog = () => document.querySelector('[role="alertdialog"]');
  const status = () => target.querySelector('.editor-top .status')?.textContent?.trim();
  const editorPath = () => target.querySelector('.editor')?.getAttribute('data-path');
  const dirty = () => target.querySelector('.editor')?.getAttribute('data-dirty');
  const key = async (name: string, init: KeyboardEventInit = {}, on: EventTarget = window) => {
    on.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init }));
    await tick();
  };
  /** Applies the current colour to the whole note: a change to save. */
  const modify = async () => {
    named('Couleur')!.click();
    await tick();
    expect(dirty()).toBe('yes');
  };
  /** The note titled `title` is the one shown, loaded. */
  const shown = async (title: string) => {
    await vi.waitFor(() => expect(titleField().value).toBe(title));
    await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
    expect(editorPath()).toBe(demoPath(title));
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  await tick();
  return {
    path,
    target,
    onclose,
    onsaved,
    onpage,
    named,
    previous,
    next,
    position,
    titleField,
    dialog,
    status,
    editorPath,
    dirty,
    key,
    modify,
    shown,
    held: () => harness.held(),
    setPages: (list: string[]) => harness.setPages(list),
  };
}

type View = Awaited<ReturnType<typeof render>>;

/** Types a title in the field without leaving it. */
function typeTitle(view: View, text: string) {
  const field = view.titleField();
  field.focus();
  field.value = text;
  field.dispatchEvent(new Event('input', { bubbles: true }));
  return field;
}

describe('the page buttons', () => {
  it.each([
    ['fr', 'Page précédente', 'Page suivante', 'Page précédente (Alt+Gauche)', 'Page suivante (Alt+Droite)', 'Page 2 sur 3'],
    ['en', 'Previous page', 'Next page', 'Previous page (Alt+Left)', 'Next page (Alt+Right)', 'Page 2 of 3'],
  ] as const)('are in the top bar with their labels in %s, and the position', async (locale, back, forth, backHint, forthHint, where) => {
    setLocale(locale);
    const view = await render(SHOPPING);
    expect(view.previous()?.getAttribute('aria-label')).toBe(back);
    expect(view.next()?.getAttribute('aria-label')).toBe(forth);
    expect(view.previous()?.title).toBe(backHint);
    expect(view.next()?.title).toBe(forthHint);
    expect(view.position()).toBe('2 / 3');
    expect(view.target.querySelector('[data-slot="page-position"]')?.getAttribute('title')).toBe(where);
    // In the top bar, beside the title, not among the tools.
    expect(view.previous()?.closest('.editor-top')).not.toBeNull();
    expect(view.previous()?.closest('.toolbar')).toBeNull();
    expect(view.previous()?.disabled).toBe(false);
    expect(view.next()?.disabled).toBe(false);
  });

  it('stop at the ends of the list: disabled, no wrap around', async () => {
    const first = await render(PHYSICS);
    expect(first.position()).toBe('1 / 3');
    expect(first.previous()?.disabled).toBe(true);
    expect(first.next()?.disabled).toBe(false);
    first.previous()!.click();
    await first.key('ArrowLeft', { altKey: true });
    expect(first.onpage).not.toHaveBeenCalled();
    expect(first.editorPath()).toBe(first.path);
    cleanup?.();

    const last = await render(PARENTS);
    expect(last.position()).toBe('3 / 3');
    expect(last.next()?.disabled).toBe(true);
    expect(last.previous()?.disabled).toBe(false);
    last.next()!.click();
    await last.key('ArrowRight', { altKey: true });
    expect(last.onpage).not.toHaveBeenCalled();
    expect(last.editorPath()).toBe(last.path);
  });

  it.each([
    ['no list at all', null, SHOPPING],
    ['a note that is not in the list (opened from a draft banner, or by the assistant)', [PHYSICS, PARENTS], SHOPPING],
    ['a list with only this note', [SHOPPING], SHOPPING],
    ['an empty list (nothing matches the search)', [], SHOPPING],
  ] as const)('are hidden for %s', async (_what, order, open) => {
    const view = await render(open, order as string[] | null);
    expect(view.previous()).toBeNull();
    expect(view.next()).toBeNull();
    expect(view.position()).toBeNull();
    // The keys do nothing either.
    await view.key('ArrowRight', { altKey: true });
    await view.key('ArrowLeft', { altKey: true });
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(view.path);
  });
});

describe('going to another page', () => {
  it('opens the next note at once when nothing is unsaved, in the same view', async () => {
    const view = await render(PHYSICS);
    const section = view.target.querySelector('.editor');
    view.next()!.click();
    await view.shown(SHOPPING);
    expect(view.onpage).toHaveBeenCalledExactlyOnceWith(demoPath(SHOPPING));
    // The screen holding the editor follows (its `path` is the editor's again).
    expect(view.held()).toBe(demoPath(SHOPPING));
    // The same editor, and it did not go back to the library.
    expect(view.target.querySelector('.editor')).toBe(section);
    expect(view.onclose).not.toHaveBeenCalled();
    expect(view.dialog()).toBeNull();
    expect(view.position()).toBe('2 / 3');

    view.next()!.click();
    await view.shown(PARENTS);
    expect(view.position()).toBe('3 / 3');
    view.previous()!.click();
    await view.shown(SHOPPING);
    view.previous()!.click();
    await view.shown(PHYSICS);
    expect(view.position()).toBe('1 / 3');
    expect(view.held()).toBe(demoPath(PHYSICS));
  });

  it('follows the list as it is at the moment of the move: its order, its search, its binder', async () => {
    const view = await render(SHOPPING, [PARENTS, SHOPPING, PHYSICS]);
    expect(view.position()).toBe('2 / 3');
    view.next()!.click();
    await view.shown(PHYSICS);

    // A search narrowed the list meanwhile: the move reads it as it is.
    view.setPages([PHYSICS, PARENTS].map(demoPath));
    await tick();
    expect(view.position()).toBe('1 / 2');
    expect(view.previous()?.disabled).toBe(true);
    view.next()!.click();
    await view.shown(PARENTS);

    view.setPages([PARENTS, SHOPPING, PHYSICS].map(demoPath));
    await tick();
    view.next()!.click();
    await view.shown(SHOPPING);
    expect(view.held()).toBe(demoPath(SHOPPING));
  });

  it('keeps its position when the note is renamed (the list holds the old path until it is read again)', async () => {
    const view = await render(SHOPPING);
    typeTitle(view, 'Courses de la semaine').blur();
    await vi.waitFor(() => expect(view.held()).not.toBe(view.path));
    const renamed = view.held();
    expect(renamed).toContain('Courses de la semaine');
    // Not in the (stale) list under its new name: still page 2 of 3, and the buttons work.
    expect(view.position()).toBe('2 / 3');
    expect(view.next()).not.toBeNull();

    // The list is read again: the note, now first by title, is found under its new path.
    view.setPages([renamed, demoPath(PHYSICS), demoPath(PARENTS)]);
    await tick();
    expect(view.position()).toBe('1 / 3');
    expect(view.previous()?.disabled).toBe(true);
    view.next()!.click();
    await view.shown(PHYSICS);
    expect(view.held()).toBe(demoPath(PHYSICS));
  });

  it('goes on to the next page while the previous one is still loading', async () => {
    const view = await render(PHYSICS);
    view.next()!.click();
    await vi.waitFor(() => expect(view.editorPath()).toBe(demoPath(SHOPPING)));
    view.next()!.click();
    await view.shown(PARENTS);
    expect(view.held()).toBe(demoPath(PARENTS));
    expect(view.position()).toBe('3 / 3');
  });

  it('is blocked while a video is being exported, and says why', async () => {
    const view = await render(PHYSICS);
    view.target.querySelector<HTMLButtonElement>('button[data-action="video-export"]')!.click();
    await tick();
    const video = (name: string) => [...document.querySelectorAll('button')].find((b) => b.textContent === name)!;
    video('run-video').click();
    await tick();
    view.next()!.click();
    await tick();
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(view.path);
    expect(view.target.querySelector('.editor-notice')?.textContent).toContain('export vidéo est en cours');
    await view.key('ArrowRight', { altKey: true });
    expect(view.onpage).not.toHaveBeenCalled();

    // Once it is over, the move works.
    video('end-video').click();
    await tick();
    video('close-video').click();
    await tick();
    view.next()!.click();
    await view.shown(SHOPPING);
  });

  it('is blocked while a narration is being recorded, and says why', async () => {
    const entry = mockLibrary().notes.get(demoPath(PHYSICS))!;
    entry.note = { ...entry.note, strokes: [slowStroke(3000, 0, 1000), slowStroke(6000, 3000, 4000), slowStroke(9000, 4200, 5000)] };
    const view = await render(PHYSICS);
    vi.useFakeTimers();
    const action = (name: string) => view.target.querySelector<HTMLButtonElement>(`[data-action="${name}"]`)!;
    await view.key('r');
    await tick();
    action('narration-open').click();
    await tick();
    await vi.waitFor(() => expect(action('narration-start')).toBeTruthy());
    action('narration-start').click();
    await vi.waitFor(() => expect(view.target.querySelector('.replay-bar')?.getAttribute('data-playing')).toBe('yes'));
    await vi.advanceTimersByTimeAsync(300);

    view.next()!.click();
    await tick();
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(view.path);
    expect(view.target.querySelector('.editor-notice')?.textContent).toContain('Arrêtez ou annulez');
    await view.key('ArrowRight', { altKey: true });
    expect(view.onpage).not.toHaveBeenCalled();
  });
});

describe('going to another page with unsaved changes', () => {
  it('asks first, and « Enregistrer et ouvrir l\'autre note » saves, then opens the next note', async () => {
    const save = vi.spyOn(api, 'saveNote');
    const view = await render(SHOPPING);
    await view.modify();
    view.next()!.click();
    await tick();
    expect(view.dialog()).toBeTruthy();
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.named(KEEP)).toBeTruthy();
    expect(view.named(LET_GO)).toBeTruthy();
    expect(view.named('Rester')).toBeTruthy();

    view.named(KEEP)!.click();
    await view.shown(PARENTS);
    expect(save).toHaveBeenCalledOnce();
    expect(save.mock.calls[0][0]).toBe(demoPath(SHOPPING));
    const stored = mockLibrary().notes.get(demoPath(SHOPPING))!.note;
    expect(stored.strokes.every((s) => s.color.r === 0 && s.color.g === 0 && s.color.b === 0)).toBe(true);
    expect(view.onpage).toHaveBeenCalledExactlyOnceWith(demoPath(PARENTS));
    expect(view.held()).toBe(demoPath(PARENTS));
    expect(view.dialog()).toBeNull();
    expect(view.onclose).not.toHaveBeenCalled();
  });

  it('« Ouvrir sans enregistrer » lets the changes go, and discards their draft', async () => {
    const discard = vi.spyOn(api, 'draftDiscard');
    const save = vi.spyOn(api, 'saveNote');
    const view = await render(SHOPPING);
    const before = JSON.stringify(mockLibrary().notes.get(view.path)!.note);
    await view.modify();
    view.previous()!.click();
    await tick();
    view.named(LET_GO)!.click();
    await view.shown(PHYSICS);
    expect(save).not.toHaveBeenCalled();
    expect(discard).toHaveBeenCalled();
    expect(JSON.stringify(mockLibrary().notes.get(view.path)!.note)).toBe(before);
    expect(view.held()).toBe(demoPath(PHYSICS));
  });

  it("« Rester » (and Escape) keep the note, its changes and the screen's path as they were", async () => {
    const view = await render(SHOPPING);
    await view.modify();
    view.next()!.click();
    await tick();
    view.named('Rester')!.click();
    await tick();
    expect(view.dialog()).toBeNull();
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(view.path);
    expect(view.held()).toBe(view.path);
    expect(view.titleField().value).toBe(SHOPPING);
    expect(view.dirty()).toBe('yes');
    expect(view.position()).toBe('2 / 3');

    // The key asks again, and Escape answers « Rester ».
    await view.key('ArrowLeft', { altKey: true });
    expect(view.dialog()).toBeTruthy();
    await view.key('Escape');
    expect(view.dialog()).toBeNull();
    expect(view.editorPath()).toBe(view.path);
    // Leaving for the library still asks the usual question.
    [...view.target.querySelectorAll('button')].find((b) => b.textContent?.includes('Bibliothèque'))!.click();
    await tick();
    expect(view.named('Enregistrer et revenir')).toBeTruthy();
    expect(view.named(KEEP)).toBeUndefined();
  });

  it('keeps the note here when it cannot be saved, with the error in view, and goes on once it can', async () => {
    const save = vi.spyOn(api, 'saveNote').mockRejectedValue({ code: 'libraryAccess', message: 'disk full' });
    const view = await render(SHOPPING);
    await view.modify();
    view.next()!.click();
    await tick();
    view.named(KEEP)!.click();
    await vi.waitFor(() => expect(view.target.querySelector('.editor-banner[role="alert"]')).toBeTruthy());
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(view.path);
    expect(view.held()).toBe(view.path);
    expect(view.titleField().value).toBe(SHOPPING);
    expect(view.dirty()).toBe('yes');
    expect(view.dialog()).toBeNull();

    // Once the disk is back, the same button goes on (the screen's path was never moved).
    save.mockRestore();
    view.next()!.click();
    await tick();
    view.named(KEEP)!.click();
    await view.shown(PARENTS);
    expect(view.held()).toBe(demoPath(PARENTS));
  });

  it('asks about a title typed and not committed, like leaving does', async () => {
    const view = await render(SHOPPING);
    const field = typeTitle(view, 'Autre titre');
    // The key typed in the field is the field's: nothing happens, the title is still uncommitted.
    await view.key('ArrowRight', { altKey: true }, field);
    expect(view.dialog()).toBeNull();
    view.next()!.click();
    await tick();
    expect(view.dialog()).toBeTruthy();
    expect(view.onpage).not.toHaveBeenCalled();
    view.named(KEEP)!.click();
    await view.shown(PARENTS);
    // Renamed first: the note is in the library under its new name.
    const titles = [...mockLibrary().notes.values()].map((n) => n.note.title);
    expect(titles).toContain('Autre titre');
    expect(titles).not.toContain(SHOPPING);
  });

  it("waits for the rename that the title field's blur starts, then goes", async () => {
    const view = await render(SHOPPING);
    // A real click takes the focus first: the title is committed (renamed) before the button acts.
    typeTitle(view, 'Courses').blur();
    view.next()!.click();
    await view.shown(PARENTS);
    expect(view.dialog()).toBeNull();
    expect([...mockLibrary().notes.values()].map((n) => n.note.title)).toContain('Courses');
    expect(view.held()).toBe(demoPath(PARENTS));
  });
});

describe('the keys Alt + Left and Alt + Right', () => {
  it('go to the previous and the next page', async () => {
    const view = await render(SHOPPING);
    await view.key('ArrowRight', { altKey: true });
    await view.shown(PARENTS);
    await view.key('ArrowLeft', { altKey: true });
    await view.shown(SHOPPING);
    await view.key('ArrowLeft', { altKey: true });
    await view.shown(PHYSICS);
    expect(view.onpage).toHaveBeenCalledTimes(3);
  });

  it('do not act while typing in a field, the title included', async () => {
    const view = await render(SHOPPING);
    const field = view.titleField();
    field.focus();
    await view.key('ArrowRight', { altKey: true }, field);
    await view.key('ArrowLeft', { altKey: true }, field);
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(view.path);
    // Another text field of the editor (a layer name, the split title…) is as well protected.
    const other = document.createElement('input');
    other.type = 'text';
    view.target.append(other);
    other.focus();
    await view.key('ArrowRight', { altKey: true }, other);
    expect(view.onpage).not.toHaveBeenCalled();
  });

  it('need Alt alone: the plain arrows, Ctrl + Alt, Shift + Alt and Meta + Alt do not change page', async () => {
    const view = await render(SHOPPING);
    await view.key('ArrowRight');
    await view.key('ArrowRight', { altKey: true, ctrlKey: true });
    await view.key('ArrowRight', { altKey: true, shiftKey: true });
    await view.key('ArrowRight', { altKey: true, metaKey: true });
    expect(view.onpage).not.toHaveBeenCalled();
  });

  it('wait while a dialog is open', async () => {
    const view = await render(SHOPPING);
    view.target.querySelector<HTMLButtonElement>('button[data-action="video-export"]')!.click();
    await tick();
    await view.key('ArrowRight', { altKey: true });
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(view.path);
  });

  it("are cancelled, so that the browser's own « back » does not run", async () => {
    const view = await render(SHOPPING);
    const event = new KeyboardEvent('keydown', { key: 'ArrowRight', altKey: true, bubbles: true, cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    await view.shown(PARENTS);
  });
});

describe('a note replaces the previous one', () => {
  it('leaves nothing of it on screen: selection, undo history, zoom, replay', async () => {
    const view = await render(SHOPPING);
    const zoom = () => view.target.querySelector('.zoom-level')?.textContent;
    const undo = () => view.target.querySelector<HTMLButtonElement>('button[title="Annuler (Ctrl+Z)"]')!;
    const redo = () => view.target.querySelector<HTMLButtonElement>('button[title="Rétablir (Ctrl+Y)"]')!;
    const fresh = zoom();
    // A change (so an undo step), a selection, a zoom, a replay left on the first note.
    await view.modify();
    expect(undo().disabled).toBe(false);
    await view.key('a', { ctrlKey: true });
    expect(view.target.querySelector('[aria-labelledby="editor-selection-heading"]')).toBeTruthy();
    view.target.querySelectorAll<HTMLButtonElement>('.zoom button')[1].click();
    await tick();
    expect(zoom()).not.toBe(fresh);
    await view.key('r');
    expect(view.target.querySelector('.replay-bar')).toBeTruthy();

    view.next()!.click();
    await tick();
    view.named(LET_GO)!.click();
    await view.shown(PARENTS);
    expect(view.target.querySelector('[aria-labelledby="editor-selection-heading"]')).toBeNull();
    expect(undo().disabled).toBe(true);
    expect(redo().disabled).toBe(true);
    expect(view.target.querySelector('.replay-bar')).toBeNull();
    expect(view.dirty()).toBe('no');
    expect(view.status()).toBe('Enregistrée');
    expect(zoom()).toBe(fresh);
    expect(view.target.querySelector('.editor-banner')).toBeNull();
    // The undo key finds nothing: the first note's history is gone.
    await view.key('z', { ctrlKey: true });
    expect(view.dirty()).toBe('no');

    // The first note, reopened, is as it was saved: the changes did not follow.
    view.previous()!.click();
    await view.shown(SHOPPING);
    expect(view.dirty()).toBe('no');
    expect(undo().disabled).toBe(true);
    const stored = mockLibrary().notes.get(demoPath(SHOPPING))!.note;
    expect(stored.strokes.every((s) => s.color.r === 0)).toBe(false);
  });

  it('shows no error of the previous note once the next one is there', async () => {
    const view = await render(SHOPPING);
    // A rename that fails leaves an error on screen.
    vi.spyOn(api, 'retitleNote').mockRejectedValueOnce({ code: 'libraryAccess', message: 'locked' });
    typeTitle(view, 'Impossible').blur();
    await vi.waitFor(() => expect(view.target.querySelector('.editor-banner[role="alert"]')).toBeTruthy());
    expect(view.dialog()).toBeNull();

    view.next()!.click();
    await view.shown(PARENTS);
    expect(view.target.querySelector('.editor-banner')).toBeNull();
    expect(view.titleField().value).toBe(PARENTS);
  });
});

describe('the share on the network', () => {
  const notes = (): Extract<ShareContent, { kind: 'note' }>[] =>
    mockShareShown.filter((c): c is Extract<ShareContent, { kind: 'note' }> => c.kind === 'note');

  it('follows the page: the note opened by the buttons is what the share shows', async () => {
    share.follow();
    expect(await share.start()).toBe(true);
    mockShareShown.length = 0;
    const view = await render(PHYSICS);
    await vi.waitFor(() => expect(notes().at(-1)?.note.title).toBe(PHYSICS));
    view.next()!.click();
    await view.shown(SHOPPING);
    await vi.waitFor(() => expect(notes().at(-1)?.note.title).toBe(SHOPPING));
    const stored = mockLibrary().notes.get(demoPath(SHOPPING))!.note;
    expect(notes().at(-1)?.note.strokes).toHaveLength(stored.strokes.length);
    expect(share.status.shown).toBe('note');
  });
});
