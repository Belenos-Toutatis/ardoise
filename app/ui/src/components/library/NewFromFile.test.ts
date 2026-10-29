// @vitest-environment jsdom
import { mount, tick, unmount, type Component } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { LibraryState } from '../../lib/library-state.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import type { NoteSummary } from '../../lib/note';
import AssembleDialog from './AssembleDialog.svelte';
import NewFromFileDialog from './NewFromFileDialog.svelte';

type E2eWindow = { __ardoiseE2eOpenPath?: string };
let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  delete (window as E2eWindow).__ardoiseE2eOpenPath;
  vi.restoreAllMocks();
});

function place<P extends Record<string, any>>(component: Component<P>, props: P) {
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(component, { target, props });
  cleanup = () => {
    unmount(mounted);
    target.remove();
  };
  const button = (label: string) => {
    const found = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  return { target, button };
}

describe('new notes from a PDF or an image', () => {
  it('makes one note per chosen page of a PDF', async () => {
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Documents\\fiche.pdf';
    const oncreated = vi.fn();
    const create = vi.spyOn(api, 'createNotesFromFile');
    const { button } = place(NewFromFileDialog, { binder: 'Classe', onclose: vi.fn(), oncreated });
    await vi.waitFor(() => expect(document.querySelector('[data-slot="new-from-file-name"]')?.textContent).toBe('fiche.pdf'));
    expect(document.body.textContent).toContain('3 page(s), la première de 210 × 297 mm.');
    expect(button('Créer les 3 notes').disabled).toBe(false);

    // Some pages only: an invalid list blocks the button.
    const some = document.querySelector<HTMLInputElement>('input[value="some"]')!;
    some.click();
    await tick();
    const list = document.querySelector<HTMLInputElement>('input[aria-label="Pages à importer"]')!;
    list.value = '4';
    list.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    expect(document.body.textContent).toContain('Écrivez des numéros de 1 à 3');
    expect(button('Créer la note').disabled).toBe(true);
    list.value = '2-3';
    list.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    expect(document.body.textContent).toContain('Notes à créer : 2');
    button('Créer les 2 notes').click();
    await vi.waitFor(() => expect(oncreated).toHaveBeenCalledTimes(1));
    expect(create).toHaveBeenCalledWith('Classe', 'C:\\Documents\\fiche.pdf', [2, 3], expect.any(Function));
    const made = oncreated.mock.lastCall?.[0] as NoteSummary[];
    expect(made.map((n) => n.title)).toEqual(['fiche - page 2', 'fiche - page 3']);
    const pages = made.map((n) => mockLibrary().notes.get(n.path)?.note.backgroundImage?.pdfPage);
    expect(pages).toEqual([2, 3]);
  });

  it('says how many notes are made while a PDF of many pages is imported', async () => {
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Documents\\fiche.pdf';
    let finish!: () => void;
    const gate = new Promise<void>((resolve) => (finish = resolve));
    const create = api.createNotesFromFile;
    vi.spyOn(api, 'createNotesFromFile').mockImplementation(async (binder, path, pages, onProgress) => {
      onProgress?.({ done: 0, total: 3 });
      onProgress?.({ done: 1, total: 3 });
      await gate;
      return create(binder, path, pages);
    });
    const oncreated = vi.fn();
    const { button } = place(NewFromFileDialog, { binder: '', onclose: vi.fn(), oncreated });
    await vi.waitFor(() => expect(button('Créer les 3 notes').disabled).toBe(false));
    button('Créer les 3 notes').click();
    await vi.waitFor(() =>
      expect(document.querySelector('[role="status"]')?.textContent).toContain('Création des notes : 1 sur 3…'),
    );
    finish();
    await vi.waitFor(() => expect(oncreated).toHaveBeenCalledTimes(1));
  });

  it('can be cancelled while the notes are made: nothing is kept, no error', async () => {
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\Documents\fiche.pdf';
    const cancelling = vi.spyOn(api, 'cancelCreateNotes');
    let refuse!: (error: unknown) => void;
    vi.spyOn(api, 'createNotesFromFile').mockImplementation(
      (_binder, _path, _pages, onProgress) =>
        new Promise((_resolve, reject) => {
          onProgress?.({ done: 1, total: 3 });
          refuse = reject;
        }),
    );
    const oncreated = vi.fn();
    const { button } = place(NewFromFileDialog, { binder: '', onclose: vi.fn(), oncreated });
    await vi.waitFor(() => expect(button('Créer les 3 notes').disabled).toBe(false));
    button('Créer les 3 notes').click();
    await vi.waitFor(() => expect(document.querySelector('progress')?.getAttribute('value')).toBe('1'));
    // « Annuler » is available while the notes are made.
    const cancel = document.querySelector<HTMLButtonElement>('button[data-action="cancel-create"]')!;
    expect(cancel.disabled).toBe(false);
    cancel.click();
    await tick();
    expect(cancelling).toHaveBeenCalledTimes(1);
    expect(cancel.textContent?.trim()).toBe('Annulation…');
    // Rust answers with the code « cancelled » once the page being rendered is done.
    refuse({ code: 'cancelled', message: 'the import was cancelled' });
    await vi.waitFor(() => expect(document.querySelector('[data-slot="new-from-file-cancelled"]')).toBeTruthy());
    expect(oncreated).not.toHaveBeenCalled();
    expect(document.querySelector('[role="alert"]')).toBeNull();
    expect(button('Créer les 3 notes').disabled).toBe(false);
  });

  it('makes one note from an image, and closes when no file is chosen', async () => {
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Photos\\tableau.jpg';
    const oncreated = vi.fn();
    const { button } = place(NewFromFileDialog, { binder: '', onclose: vi.fn(), oncreated });
    await vi.waitFor(() => expect(document.body.textContent).toContain('Une note sera créée avec cette image en fond.'));
    button('Créer la note').click();
    await vi.waitFor(() => expect(oncreated).toHaveBeenCalledTimes(1));
    expect((oncreated.mock.lastCall?.[0] as NoteSummary[]).map((n) => n.title)).toEqual(['tableau']);
    cleanup?.();

    // Cancelled native dialog (the browser stand-in's prompt answers null).
    delete (window as E2eWindow).__ardoiseE2eOpenPath;
    vi.spyOn(window, 'prompt').mockReturnValue(null);
    const onclose = vi.fn();
    place(NewFromFileDialog, { binder: '', onclose, oncreated: vi.fn() });
    await vi.waitFor(() => expect(onclose).toHaveBeenCalledTimes(1));
  });

  it('shows why a file cannot be used', async () => {
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Documents\\notes.docx';
    place(NewFromFileDialog, { binder: '', onclose: vi.fn(), oncreated: vi.fn() });
    await vi.waitFor(() => expect(document.querySelector('[role="alert"]')).toBeTruthy());
    expect(document.body.textContent).toContain("ce fichier n'est ni un PDF ni une image PNG ou JPEG");
  });
});

describe('assembling notes with background images', () => {
  it('says that only the first note keeps its image', async () => {
    const library = mockLibrary();
    const entries = [...library.notes.entries()].sort((a, b) => a[1].note.created - b[1].note.created);
    const [, second] = entries;
    second[1].note.backgroundImage = { name: 'fiche.png', rect: { x: 0, y: 0, width: 100, height: 100 } };
    const lib = new LibraryState();
    const listing = await api.libraryList();
    place(AssembleDialog, { lib, notes: listing.notes, onclose: vi.fn() });
    await vi.waitFor(() =>
      expect(document.querySelector('[data-slot="assemble-dropped-images"]')?.textContent).toContain(
        "celle d'une autre note ne sera pas reprise.",
      ),
    );
    cleanup?.();
    second[1].note.backgroundImage = undefined;
    place(AssembleDialog, { lib, notes: listing.notes, onclose: vi.fn() });
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(document.querySelector('[data-slot="assemble-dropped-images"]')).toBeNull();
  });
});
