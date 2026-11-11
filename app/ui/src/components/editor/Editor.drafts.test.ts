// @vitest-environment jsdom
/// <reference types="node" />
/**
 * Drafts in the editor (M10): the unsaved note is kept when the editor goes away without saving
 * it, offered again when the note is opened, over the note or as a new note when the file
 * changed since, and removed once saved or discarded.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockDrafts, resetMockDrafts, startNewMockSession } from '../../lib/mock-drafts';
import { freshStamp, mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import Editor from './Editor.svelte';

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

let cleanups: (() => void)[] = [];

beforeEach(() => {
  resetMockLibrary();
  resetMockDrafts();
  setLocale('fr');
});

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function render(path: string) {
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const component = mount(Editor, { target, props: { path, app, onclose, onsaved } });
  let mounted = true;
  const close = () => {
    if (!mounted) return;
    mounted = false;
    unmount(component);
    target.remove();
  };
  cleanups.push(close);
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  /** Visible name of a button, without its decorative (aria-hidden) icon. */
  const name = (b: HTMLButtonElement) => {
    const copy = b.cloneNode(true) as HTMLElement;
    copy.querySelectorAll('[aria-hidden="true"]').forEach((icon) => icon.remove());
    return copy.textContent?.trim();
  };
  const button = (label: string) => {
    const found = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
      (b) => b.getAttribute('aria-label') === label || b.title === label || name(b) === label,
    );
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  const action = (name: string) => document.querySelector<HTMLButtonElement>(`[data-action="${name}"]`);
  const dialog = () => document.querySelector<HTMLElement>('[data-slot="draft-recovery"]');
  const status = () => target.querySelector('.status')?.textContent?.trim();
  /** Recolours the whole note red: one unsaved change. */
  const edit = async () => {
    button('Rouge').click();
    await tick();
    button('Couleur').click();
    await tick();
  };
  return { target, button, action, dialog, status, edit, close, onclose, onsaved };
}

/** Edits the note at `path` and leaves without saving, as a crash would: the draft stays. */
async function leaveADraft(path: string) {
  const editor = await render(path);
  await editor.edit();
  expect(mockDrafts.unsavedReason).toMatch(/non enregistrées/);
  editor.close();
  await vi.waitFor(() => expect(mockDrafts.writes).toBe(1));
  await vi.waitFor(() => expect(mockDrafts.unsavedReason).toBeNull());
  // The app starts again later.
  startNewMockSession();
}

const red = 'rgba(216, 0, 0, 1.000)';
const fills = (target: HTMLElement) =>
  [...target.querySelectorAll<SVGPathElement>('g.ink path:not(.live)')].map((p) => p.getAttribute('fill'));

describe('drafts in the editor', () => {
  it('offers the unsaved changes over the note, as one undoable step, and drops the draft once saved', async () => {
    const path = demoPath('Cours de physique');
    await leaveADraft(path);
    expect(await api.draftList()).toHaveLength(1);

    const editor = await render(path);
    await vi.waitFor(() => expect(editor.dialog()?.dataset.recovery).toBe('restore'));
    expect(editor.dialog()?.textContent).toContain('Ardoise s\'est fermé avant que cette note soit enregistrée');
    expect(fills(editor.target)).not.toContain(red);
    editor.action('draft-restore')!.click();
    await vi.waitFor(() => expect(editor.dialog()).toBeNull());
    expect(fills(editor.target).every((f) => f === red)).toBe(true);
    expect(editor.status()).toBe('Modifications non enregistrées');
    // Ctrl+Z gives the saved version back.
    editor.button('Annuler (Ctrl+Z)').click();
    await tick();
    expect(fills(editor.target)).not.toContain(red);
    editor.button('Rétablir (Ctrl+Y)').click();
    await tick();

    editor.button('Enregistrer').click();
    await vi.waitFor(() => expect(editor.onsaved).toHaveBeenCalledWith(path));
    await vi.waitFor(() => expect(mockDrafts.unsavedReason).toBeNull());
    await flush();
    expect(await api.draftFind(mockLibrary().notes.get(path)!.note.id)).toBeNull();
    expect(mockLibrary().notes.get(path)!.note.strokes.every((s) => s.color.r === 216)).toBe(true);
  });

  it('offers only a new note when the note changed since, and leaves the newer note alone', async () => {
    const path = demoPath('Cours de physique');
    await leaveADraft(path);
    // Another PC saved the note meanwhile (Nextcloud brought it here).
    const stored = mockLibrary().notes.get(path)!;
    stored.note = { ...stored.note, title: 'Cours de physique (autre PC)' };
    stored.stamp = freshStamp();
    const before = JSON.stringify(stored.note);
    const count = mockLibrary().notes.size;

    const editor = await render(path);
    await vi.waitFor(() => expect(editor.dialog()?.dataset.recovery).toBe('copy'));
    expect(editor.action('draft-restore')).toBeNull();
    editor.action('draft-copy')!.click();
    await vi.waitFor(() => expect(editor.dialog()).toBeNull());
    await vi.waitFor(() => expect(mockLibrary().notes.size).toBe(count + 1));
    const [copyPath, copy] = [...mockLibrary().notes].find(([p]) => p.includes('(récupérée)'))!;
    expect(copy.note.title).toBe('Cours de physique (récupérée)');
    expect(copy.note.strokes.every((s) => s.color.r === 216)).toBe(true);
    expect(copy.note.id).not.toBe(stored.note.id);
    expect(JSON.stringify(mockLibrary().notes.get(path)!.note)).toBe(before);
    // The editor shows the copy, saved, and the draft is gone.
    expect(editor.onsaved).toHaveBeenCalledWith(copyPath);
    await vi.waitFor(() => expect(editor.status()).toBe('Enregistrée'));
    expect(fills(editor.target).every((f) => f === red)).toBe(true);
    expect(editor.target.textContent).toContain('Les modifications récupérées sont dans la nouvelle note');
    expect(await api.draftList()).toEqual([]);
  });

  it('discards the draft after asking once more', async () => {
    const path = demoPath('Cours de physique');
    await leaveADraft(path);
    const editor = await render(path);
    await vi.waitFor(() => expect(editor.dialog()).toBeTruthy());
    editor.action('draft-discard')!.click();
    await tick();
    expect(editor.dialog()?.textContent).toContain('Abandonner définitivement ces modifications ?');
    editor.action('draft-discard-yes')!.click();
    await vi.waitFor(() => expect(editor.dialog()).toBeNull());
    await flush();
    expect(await api.draftList()).toEqual([]);
    expect(fills(editor.target)).not.toContain(red);
    expect(editor.status()).toBe('Enregistrée');
  });

  it('goes back to the library with "Later" and keeps the draft', async () => {
    const path = demoPath('Cours de physique');
    await leaveADraft(path);
    const editor = await render(path);
    await vi.waitFor(() => expect(editor.dialog()).toBeTruthy());
    editor.action('draft-later')!.click();
    await vi.waitFor(() => expect(editor.onclose).toHaveBeenCalled());
    editor.close();
    await flush();
    expect(await api.draftList()).toHaveLength(1);
  });

  it('removes the draft when the changes are discarded on leaving, and tells Windows nothing is unsaved', async () => {
    const path = demoPath('Cours de physique');
    const editor = await render(path);
    await editor.edit();
    expect(mockDrafts.unsavedReason).toBe(
      "Une note a des modifications non enregistrées. Revenez dans Ardoise pour l'enregistrer.",
    );
    editor.button('Bibliothèque').click();
    await tick();
    editor.button('Revenir sans enregistrer').click();
    await vi.waitFor(() => expect(editor.onclose).toHaveBeenCalled());
    editor.close();
    await flush();
    expect(mockDrafts.unsavedReason).toBeNull();
    startNewMockSession();
    expect(await api.draftList()).toEqual([]);
    expect(mockDrafts.writes).toBe(0);
  });

  it('opens a note without a draft as before', async () => {
    const editor = await render(demoPath('Liste de courses'));
    await flush();
    expect(editor.dialog()).toBeNull();
    expect(mockDrafts.unsavedReason).toBeNull();
  });
});
