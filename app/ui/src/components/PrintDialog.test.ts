// @vitest-environment jsdom
// Printing (milestone M8): the dialog over the editor's note or the library's selection.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../lib/mock-data';
import { mockPrint } from '../lib/mock-export';
import type { ExportSource } from '../lib/note';
import PrintDialog from './PrintDialog.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  mockPrint.last = null;
  mockPrint.cancel = false;
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

function pathOf(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function settle() {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(sources: ExportSource[], locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(PrintDialog, { target, props: { sources, subject: 'Sujet', onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const button = (text: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  const background = () => target.querySelector<HTMLInputElement>('input[data-option="background"]')!;
  const fit = (value: string) => target.querySelector<HTMLInputElement>(`input[name="print-fit"][value="${value}"]`)!;
  return { target, onclose, button, background, fit };
}

describe('print dialog', () => {
  it('prints the notes with the default choices and says how many pages were sent', async () => {
    const spy = vi.spyOn(api, 'printNotes');
    const sources = [{ path: pathOf('Réunion parents') }, { path: pathOf('Liste de courses') }];
    const ui = await render(sources);
    expect(ui.target.textContent).toContain("Windows ouvre sa fenêtre d'impression");
    expect(ui.background().checked).toBe(true);
    expect(ui.fit('actual').checked).toBe(true);
    ui.button('Imprimer…').click();
    await settle();
    expect(spy).toHaveBeenCalledWith(sources, { background: true, fit: 'actual', perSheet: 1 });
    expect(mockPrint.last?.sources).toEqual(sources);
    expect(ui.target.querySelector('[role="status"]')?.textContent).toBe("Envoyé à l'imprimante : 2 pages.");
    expect(ui.button('Imprimer de nouveau…')).toBeTruthy();
    expect(ui.button('Fermer')).toBeTruthy();
  });

  it('passes the chosen options and remembers them for the next time', async () => {
    const spy = vi.spyOn(api, 'printNotes');
    const ui = await render([{ path: pathOf('Liste de courses') }], 'en');
    ui.background().click();
    ui.fit('fit').click();
    ui.target.querySelector<HTMLInputElement>('input[name="print-per-sheet"][value="4"]')!.click();
    await settle();
    ui.button('Print…').click();
    await settle();
    expect(spy).toHaveBeenCalledWith(expect.any(Array), { background: false, fit: 'fit', perSheet: 4 });
    cleanup?.();

    const again = await render([{ path: pathOf('Liste de courses') }], 'en');
    expect(again.background().checked).toBe(false);
    expect(again.fit('fit').checked).toBe(true);
    expect(again.target.querySelector<HTMLInputElement>('input[name="print-per-sheet"][value="4"]')!.checked).toBe(true);
  });

  it('offers 1, 2 or 4 notes per sheet, says what the dialog counts, and counts the sheets printed', async () => {
    const ui = await render([{ path: pathOf('Liste de courses') }, { path: pathOf('Réunion parents') }, { path: pathOf('Cours de physique') }], 'en');
    const radios = [...ui.target.querySelectorAll<HTMLInputElement>('input[name="print-per-sheet"]')];
    expect(radios.map((r) => r.value)).toEqual(['1', '2', '4']);
    expect(radios.map((r) => r.closest('label')?.textContent?.trim())).toEqual(['1 (one note per sheet)', '2 per sheet', '4 per sheet']);
    expect(radios[0].checked).toBe(true);
    expect(ui.target.textContent).not.toContain('the pages are the sheets');
    radios[1].click();
    await settle();
    expect(ui.target.textContent).toContain('the pages are the sheets');
    ui.button('Print…').click();
    await settle();
    // Three notes, two to a sheet: two sheets.
    expect(ui.target.querySelector('[role="status"]')?.textContent).toBe('Sent to the printer: 2 pages.');
  });

  it('prints the note of the editor with its unsaved changes', async () => {
    const path = pathOf('Réunion parents');
    const note = { ...mockLibrary().notes.get(path)!.note, title: 'Brouillon' };
    const spy = vi.spyOn(api, 'printNotes');
    const ui = await render([{ path, note }]);
    ui.button('Imprimer…').click();
    await settle();
    expect(spy.mock.calls[0][0][0].note?.title).toBe('Brouillon');
  });

  it('says so when the user closes the Windows dialog without printing', async () => {
    mockPrint.cancel = true;
    const ui = await render([{ path: pathOf('Liste de courses') }]);
    ui.button('Imprimer…').click();
    await settle();
    expect(ui.target.textContent).toContain('Impression annulée.');
    // Nothing was printed: the button still says "Imprimer…" and the dialog can be closed.
    expect(ui.button('Imprimer…')).toBeTruthy();
    expect(ui.button('Annuler')).toBeTruthy();
  });

  it('explains a failure in the interface language', async () => {
    vi.spyOn(api, 'printNotes').mockRejectedValue({ code: 'printUnavailable', message: 'not Windows' });
    const ui = await render([{ path: pathOf('Liste de courses') }], 'en');
    ui.button('Print…').click();
    await settle();
    expect(ui.target.querySelector('[role="alert"]')?.textContent).toContain('Printing failed: printing is only possible on Windows');
  });
});
