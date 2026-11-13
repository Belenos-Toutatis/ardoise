// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M12: « Reprendre en mode direct » in the editor's toolbar. Leaving for live mode follows
// the rules of leaving the editor: unsaved changes are saved first (or let go on purpose), a note
// that cannot be saved stays here.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

// The real stroke geometry, instantiated from the module's bytes (jsdom cannot fetch it).
vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  // Tests run from `app/` (the Vite root).
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

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockTablet();
  resetMockNarration();
  tablet.reset();
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

async function render(title = 'Liste de courses', withResume = true) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const onresume = vi.fn();
  const component = mount(Editor, {
    target,
    props: { path, app, onclose, onsaved, ...(withResume ? { onresume } : {}) },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  // The tablet's status comes in: the button depends on it.
  await vi.waitFor(() => expect(tablet.status).not.toBeNull());
  await tick();
  const resume = () => target.querySelector<HTMLButtonElement>('button[data-action="resume-live"]');
  const buttons = () => [...document.querySelectorAll('button')];
  const named = (text: string) => buttons().find((b) => b.textContent?.trim() === text);
  /** Applies the current colour to the whole note: a change to save. */
  const modify = async () => {
    named('Couleur')!.click();
    await tick();
    expect(target.querySelector('.editor')?.getAttribute('data-dirty')).toBe('yes');
  };
  const dialog = () => document.querySelector('[role="alertdialog"]');
  return { path, target, onclose, onsaved, onresume, resume, named, modify, dialog };
}

describe('« Reprendre en mode direct » in the editor', () => {
  it.each([
    ['fr', 'Reprendre en mode direct', 'Reprend le mode direct sur cette feuille'],
    ['en', 'Resume in live mode', 'Resumes live mode on this sheet'],
  ] as const)('is in the toolbar in %s', async (locale, label, hint) => {
    setLocale(locale);
    const view = await render();
    expect(view.resume()?.textContent).toContain(label);
    expect(view.resume()?.title).toContain(hint);
    expect(view.resume()?.disabled).toBe(false);
  });

  it('is not there when the screen does not take it (no onresume)', async () => {
    const view = await render('Liste de courses', false);
    expect(view.resume()).toBeNull();
  });

  it('hands a note without changes straight to live mode, as the Back button would close', async () => {
    const view = await render();
    view.resume()!.click();
    await vi.waitFor(() => expect(view.onresume).toHaveBeenCalledExactlyOnceWith(view.path));
    expect(view.onclose).not.toHaveBeenCalled();
    expect(view.dialog()).toBeNull();
  });

  it('asks first when the note has unsaved changes: save, then resume', async () => {
    const save = vi.spyOn(api, 'saveNote');
    const view = await render();
    await view.modify();
    view.resume()!.click();
    await tick();
    expect(view.dialog()).toBeTruthy();
    expect(view.onresume).not.toHaveBeenCalled();
    // The dialog says what the answer does.
    expect(view.named('Enregistrer et reprendre le direct')).toBeTruthy();
    expect(view.named('Reprendre sans enregistrer')).toBeTruthy();
    expect(view.named('Rester')).toBeTruthy();

    view.named('Enregistrer et reprendre le direct')!.click();
    await vi.waitFor(() => expect(view.onresume).toHaveBeenCalledExactlyOnceWith(view.path));
    expect(save).toHaveBeenCalledOnce();
    // What was saved is what live mode will read from the library.
    const stored = mockLibrary().notes.get(view.path)!.note;
    expect(stored.strokes.every((s) => s.color.r === 0 && s.color.g === 0 && s.color.b === 0)).toBe(true);
    expect(view.onclose).not.toHaveBeenCalled();
  });

  it('keeps the note here when it cannot be saved: nothing is lost, the error stays in view', async () => {
    vi.spyOn(api, 'saveNote').mockRejectedValue({ code: 'libraryAccess', message: 'disk full' });
    const view = await render();
    await view.modify();
    view.resume()!.click();
    await tick();
    view.named('Enregistrer et reprendre le direct')!.click();
    await vi.waitFor(() => expect(view.target.querySelector('.editor-banner[role="alert"]')).toBeTruthy());
    expect(view.onresume).not.toHaveBeenCalled();
    expect(view.target.querySelector('.editor')?.getAttribute('data-dirty')).toBe('yes');
    expect(view.dialog()).toBeNull();
  });

  it('lets the changes go when the user says so, and discards their draft', async () => {
    const discard = vi.spyOn(api, 'draftDiscard');
    const save = vi.spyOn(api, 'saveNote');
    const view = await render();
    const before = JSON.stringify(mockLibrary().notes.get(view.path)!.note);
    await view.modify();
    view.resume()!.click();
    await tick();
    view.named('Reprendre sans enregistrer')!.click();
    await vi.waitFor(() => expect(view.onresume).toHaveBeenCalledExactlyOnceWith(view.path));
    expect(save).not.toHaveBeenCalled();
    expect(discard).toHaveBeenCalled();
    expect(JSON.stringify(mockLibrary().notes.get(view.path)!.note)).toBe(before);
  });

  it('stays in the editor when the user stays, and the next Back button asks the usual question', async () => {
    const view = await render();
    await view.modify();
    view.resume()!.click();
    await tick();
    view.named('Rester')!.click();
    await tick();
    expect(view.dialog()).toBeNull();
    expect(view.onresume).not.toHaveBeenCalled();
    expect(view.target.querySelector('.editor')?.getAttribute('data-dirty')).toBe('yes');

    [...view.target.querySelectorAll('button')].find((b) => b.textContent?.includes('Bibliothèque'))!.click();
    await tick();
    expect(view.dialog()).toBeTruthy();
    expect(view.named('Enregistrer et revenir')).toBeTruthy();
    expect(view.named('Enregistrer et reprendre le direct')).toBeUndefined();
  });

  it('asks about a title typed and not committed, like leaving does', async () => {
    const view = await render();
    const field = view.target.querySelector<HTMLInputElement>('.title-field input')!;
    field.focus();
    field.value = 'Autre titre';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    view.resume()!.click();
    await tick();
    expect(view.dialog()).toBeTruthy();
    expect(view.onresume).not.toHaveBeenCalled();
    view.named('Enregistrer et reprendre le direct')!.click();
    await vi.waitFor(() => expect(view.onresume).toHaveBeenCalledOnce());
    // The note was renamed first: live mode gets the file under its new name.
    const [given] = view.onresume.mock.calls[0] as [string];
    expect(given).not.toBe(view.path);
    expect(mockLibrary().notes.get(given)?.note.title).toBe('Autre titre');
  });

  it('is disabled, with the reason, when no tablet is registered or it is busy', async () => {
    const view = await render();
    tablet.status = { ...tablet.status!, registered: null, known: [], connection: 'notRegistered' };
    await tick();
    expect(view.resume()?.disabled).toBe(true);
    expect(view.resume()?.title).toBe('Reprendre en mode direct demande une tablette enregistrée : enregistrez-la depuis le panneau de la tablette.');
    view.resume()!.click();
    await tick();
    expect(view.onresume).not.toHaveBeenCalled();

    tablet.status = { ...tablet.status!, registered: { name: 'Slate', width: 29700, height: 21600 } as never, connection: 'syncing' };
    await tick();
    expect(view.resume()?.disabled).toBe(true);
    expect(view.resume()?.title).toBe('La tablette est occupée (synchronisation ou enregistrement en cours).');
  });

  it('does not leave a note that is being exported as a video', async () => {
    const view = await render();
    view.target.querySelector<HTMLButtonElement>('button[data-action="video-export"]')!.click();
    await tick();
    const run = [...document.querySelectorAll('button')].find((b) => b.textContent === 'run-video')!;
    run.click();
    await tick();
    expect(view.resume()?.disabled).toBe(true);
    expect(view.onresume).not.toHaveBeenCalled();
  });
});
