// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../lib/mock-data';
import type { ExportSource } from '../lib/note';
import ExportDialog from './ExportDialog.svelte';

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

function pathOf(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(titles: string[], locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  const sources: ExportSource[] = titles.map((title) => ({ path: pathOf(title) }));
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const mountIt = () => mount(ExportDialog, { target, props: { sources, subject: 'Sujet', onclose } });
  let component = mountIt();
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const dialog = () => target.querySelector<HTMLElement>('[role="dialog"]')!;
  const button = (text: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  const radio = (label: string) => {
    const found = [...target.querySelectorAll<HTMLLabelElement>('label.choice')].find((l) => l.textContent?.trim() === label);
    const input = found?.querySelector('input');
    if (!input) throw new Error(`no choice "${label}"`);
    return input;
  };
  const choose = async (label: string) => {
    radio(label).click();
    await settle();
  };
  const select = (el: HTMLSelectElement | HTMLInputElement, value: string) => {
    el.value = value;
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  };
  const resolutionSelect = () => target.querySelector<HTMLSelectElement>('.export-row select');
  const qualityRange = () => target.querySelector<HTMLInputElement>('input[type="range"]');
  const background = () => target.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  const remount = async () => {
    unmount(component);
    component = mountIt();
    await settle();
  };
  return { target, sources, onclose, dialog, button, radio, choose, select, resolutionSelect, qualityRange, background, remount };
}

const DEFAULTS = { dpi: 150, background: true, quality: 90, pdfPage: 'actual' };

describe('export dialog: options', () => {
  it('shows the options that suit each format', async () => {
    const ui = await render(['Liste de courses']);
    expect(ui.dialog().getAttribute('aria-modal')).toBe('true');
    expect(ui.dialog().textContent).toContain('Sujet');
    // PNG: resolution, background; no JPG quality, no PDF page.
    expect(ui.resolutionSelect()).toBeTruthy();
    expect(ui.qualityRange()).toBeNull();
    expect(ui.target.textContent).not.toContain('Page du PDF');
    expect(ui.target.textContent).toContain('fond est transparent');

    await ui.choose('JPG (image)');
    expect(ui.resolutionSelect()).toBeTruthy();
    expect(ui.qualityRange()).toBeTruthy();
    expect(ui.target.textContent).toContain('le JPG ne gère pas la transparence');

    await ui.choose('SVG (vectoriel)');
    expect(ui.resolutionSelect()).toBeNull();
    expect(ui.qualityRange()).toBeNull();
    expect(ui.target.textContent).toContain('Inkscape');

    await ui.choose('PDF');
    expect(ui.resolutionSelect()).toBeNull();
    expect(ui.target.textContent).toContain('Page du PDF');
    expect(ui.radio('Taille réelle de la page').checked).toBe(true);
    expect(ui.radio('A4 (note ajustée et centrée)').checked).toBe(false);
  });

  it('offers 96, 150 and 300 dpi', async () => {
    const ui = await render(['Liste de courses']);
    const values = [...ui.resolutionSelect()!.options].map((o) => o.value);
    expect(values).toEqual(['96', '150', '300']);
    expect(ui.resolutionSelect()!.value).toBe('150');
  });

  it('speaks English', async () => {
    const ui = await render(['Liste de courses'], 'en');
    expect(ui.dialog().querySelector('h2')?.textContent).toBe('Export');
    expect(ui.target.textContent).toContain('Include the background');
    expect(ui.button('Export…')).toBeTruthy();
    expect(ui.button('Cancel')).toBeTruthy();
  });
});

describe('export dialog: exporting', () => {
  it('exports one note to the chosen file and shows where it went', async () => {
    const spy = vi.spyOn(api, 'exportNotes');
    const ui = await render(['Liste de courses']);
    ui.button('Exporter…').click();
    await settle();
    const path = pathOf('Liste de courses');
    expect(spy).toHaveBeenCalledWith(
      [{ path }],
      'png',
      DEFAULTS,
      'C:\\Users\\demo\\Exports\\2026-10-22 Liste de courses.png',
    );
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('Fichiers créés : 1');
    expect(ui.target.querySelector('.export-result code')?.textContent).toBe(
      'C:\\Users\\demo\\Exports\\2026-10-22 Liste de courses.png',
    );
    expect(ui.button('Fermer')).toBeTruthy();
    expect(ui.button('Exporter de nouveau…')).toBeTruthy();
  });

  it('passes the chosen options, and remembers them for the next time', async () => {
    const spy = vi.spyOn(api, 'exportNotes');
    const ui = await render(['Liste de courses']);
    await ui.choose('JPG (image)');
    ui.select(ui.resolutionSelect()!, '300');
    ui.select(ui.qualityRange()!, '70');
    ui.background().click();
    ui.button('Exporter…').click();
    await settle();
    expect(spy).toHaveBeenCalledWith(expect.any(Array), 'jpg', { dpi: 300, background: false, quality: 70, pdfPage: 'actual' }, expect.stringMatching(/\.jpg$/));

    await ui.remount();
    expect(ui.radio('JPG (image)').checked).toBe(true);
    expect(ui.resolutionSelect()!.value).toBe('300');
    expect(ui.qualityRange()!.value).toBe('70');
    expect(ui.background().checked).toBe(false);
  });

  it('puts several notes in one PDF, one page each in the order given, in A4', async () => {
    const spy = vi.spyOn(api, 'exportNotes');
    const titles = ['Réunion parents', 'Liste de courses', 'Cours de physique'];
    const ui = await render(titles);
    await ui.choose('PDF');
    await ui.choose('A4 (note ajustée et centrée)');
    expect(ui.target.textContent).toContain('un seul PDF, une page par note');
    ui.button('Exporter…').click();
    await settle();
    expect(spy).toHaveBeenCalledWith(
      titles.map((title) => ({ path: pathOf(title) })),
      'pdf',
      { ...DEFAULTS, pdfPage: 'a4' },
      'C:\\Users\\demo\\Exports\\Notes.pdf',
    );
    expect(ui.target.querySelector('.export-result code')?.textContent).toBe('C:\\Users\\demo\\Exports\\Notes.pdf');
  });

  it('writes one image per note into a folder when several notes are exported', async () => {
    const spy = vi.spyOn(api, 'exportNotes');
    const ui = await render(['Réunion parents', 'Liste de courses']);
    expect(ui.target.textContent).toContain('dans le dossier que vous choisirez');
    ui.button('Exporter…').click();
    await settle();
    expect(spy).toHaveBeenCalledWith(expect.any(Array), 'png', DEFAULTS, 'C:\\Users\\demo\\Exports');
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('Fichiers créés : 2');
    expect(ui.target.querySelector('.export-result code')?.textContent).toBe('C:\\Users\\demo\\Exports');
  });

  it('exports the note it was given (unsaved changes included)', async () => {
    const spy = vi.spyOn(api, 'exportNotes');
    const path = pathOf('Liste de courses');
    const { note } = await api.openNote(path);
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(ExportDialog, {
      target,
      props: { sources: [{ path, note: { ...note, title: 'Brouillon' } }], subject: 'S', onclose: vi.fn() },
    });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await settle();
    [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Exporter…')!.click();
    await settle();
    expect(spy.mock.calls[0][0][0].note?.title).toBe('Brouillon');
  });

  it('disables the buttons while exporting, then shows the result', async () => {
    let finish: (paths: string[]) => void = () => {};
    vi.spyOn(api, 'exportNotes').mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
    const ui = await render(['Liste de courses']);
    ui.button('Exporter…').click();
    await settle();
    expect(ui.target.querySelector('[role="status"]')?.textContent).toBe('Export en cours…');
    expect(ui.button('Export en cours…').disabled).toBe(true);
    expect(ui.button('Annuler').disabled).toBe(true);
    expect(ui.target.querySelector('fieldset')?.disabled).toBe(true);
    // Escape does not close the dialog under the operation.
    ui.dialog().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(ui.onclose).not.toHaveBeenCalled();

    finish(['C:\\x\\a.png']);
    await settle();
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('Fichiers créés : 1');
    expect(ui.button('Fermer').disabled).toBe(false);
  });

  it('shows a translated error and lets the user try again', async () => {
    vi.spyOn(api, 'exportNotes').mockRejectedValueOnce({ code: 'exportFailed', message: 'disque plein' });
    const ui = await render(['Liste de courses']);
    ui.button('Exporter…').click();
    await settle();
    const alert = ui.target.querySelector('[role="alert"]')!;
    expect(alert.textContent).toContain("L'export a échoué : l'export n'a pas pu être écrit (disque plein)");
    expect(ui.button('Exporter…').disabled).toBe(false);
    expect(ui.target.querySelector('.export-result')).toBeNull();

    ui.button('Exporter…').click();
    await settle();
    expect(ui.target.querySelector('[role="alert"]')).toBeNull();
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('Fichiers créés : 1');
  });

  it('closes with Escape and with the cancel button when idle', async () => {
    const ui = await render(['Liste de courses']);
    ui.dialog().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(ui.onclose).toHaveBeenCalledTimes(1);
    ui.button('Annuler').click();
    expect(ui.onclose).toHaveBeenCalledTimes(2);
  });

  it('moves the focus into the dialog and keeps Tab inside', async () => {
    const outside = document.createElement('button');
    document.body.append(outside);
    outside.focus();
    const ui = await render(['Liste de courses']);
    expect(ui.dialog().contains(document.activeElement)).toBe(true);
    expect(document.activeElement?.textContent?.trim()).toBe('Exporter…');
    // From the last control, Tab wraps to the first one.
    const controls = [...ui.dialog().querySelectorAll<HTMLElement>('button, input, select')].filter((el) => !el.hasAttribute('disabled'));
    controls[controls.length - 1].focus();
    ui.dialog().dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(controls[0]);
    outside.remove();
  });
});
