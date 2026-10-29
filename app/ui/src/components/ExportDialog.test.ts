// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../lib/mock-data';
import { mockExport } from '../lib/mock-export';
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
    // A format card holds its name and its strengths and limits: match the name only.
    const found = [...target.querySelectorAll<HTMLLabelElement>('label.choice')].find(
      (l) => (l.querySelector('.choice-name') ?? l).textContent?.trim() === label,
    );
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
  it('gives the strengths and limits of every format, and compares them', async () => {
    for (const locale of ['fr', 'en'] as const) {
      const ui = await render(['Liste de courses'], locale);
      const cards = [...ui.target.querySelectorAll<HTMLLabelElement>('label.format-card')];
      expect(cards).toHaveLength(5);
      for (const card of cards) {
        const radio = card.querySelector('input')!;
        // Named by the format alone, described by its strengths and limits.
        const name = document.getElementById(radio.getAttribute('aria-labelledby')!)!.textContent!.trim();
        expect(['PNG (image)', 'JPG (image)', 'WebP (image)', 'SVG (vectoriel)', 'SVG (vector)', 'PDF']).toContain(name);
        const described = radio
          .getAttribute('aria-describedby')!
          .split(' ')
          .map((id) => document.getElementById(id)!.textContent!.trim());
        expect(described).toHaveLength(2);
        expect(described[0]).toMatch(locale === 'fr' ? /^Atouts :/ : /^Strengths:/);
        expect(described[1]).toMatch(locale === 'fr' ? /^Limites :/ : /^Limits:/);
      }
      // The selected card is marked; choosing another one moves the mark.
      expect(ui.target.querySelector('.format-card.selected .choice-name')!.textContent).toBe('PNG (image)');
      await ui.choose('PDF');
      expect(ui.target.querySelector('.format-card.selected .choice-name')!.textContent).toBe('PDF');

      const compare = ui.target.querySelector('details.format-compare')!;
      expect(compare.querySelector('summary')!.textContent).toBe(
        locale === 'fr' ? 'Quel format choisir ?' : 'Which format should I choose?',
      );
      expect([...compare.querySelectorAll('thead th')].map((th) => th.textContent!.trim())).toEqual([
        locale === 'fr' ? 'Critère' : 'Criterion',
        'PNG',
        'JPG',
        'WEBP',
        'SVG',
        'PDF',
      ]);
      const rows = [...compare.querySelectorAll('tbody tr')];
      expect(rows).toHaveLength(6);
      for (const row of rows) expect(row.querySelectorAll('td')).toHaveLength(5);
      expect(rows.map((r) => r.querySelector('th')!.textContent)).toContain(locale === 'fr' ? 'Fond transparent possible' : 'Transparent background possible');
      cleanup?.();
      cleanup = null;
    }
  });

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

  it('does not offer a resolution whose image would exceed 16 384 px, and moves to one that fits', async () => {
    // 3 m wide: 17 717 px at 150 dpi (too much), 11 339 px at 96 dpi.
    mockLibrary().notes.get(pathOf('Liste de courses'))!.note.page = { width: 300_000, height: 21600 };
    const spy = vi.spyOn(api, 'exportNotes');
    const ui = await render(['Liste de courses', 'Cours de physique']);
    const options = [...ui.resolutionSelect()!.options];
    expect(options.map((o) => [o.value, o.disabled])).toEqual([
      ['96', false],
      ['150', true],
      ['300', true],
    ]);
    expect(options[1].textContent?.trim()).toBe('150 ppp (usuel) : image trop grande');
    expect(ui.target.querySelector('.export-hint')?.textContent).toContain('ne sont pas proposées');
    // The remembered 150 dpi gave way to the largest resolution that fits.
    expect(ui.resolutionSelect()!.value).toBe('96');
    ui.button('Exporter…').click();
    await settle();
    expect(spy.mock.calls[0][2].dpi).toBe(96);
  });

  it('blocks the raster export of a note given by the editor when no resolution fits, but not the PDF', async () => {
    const path = pathOf('Liste de courses');
    const { note } = await api.openNote(path);
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(ExportDialog, {
      target,
      props: { sources: [{ path, note: { ...note, page: { width: 600_000, height: 21600 } } }], subject: 'S', onclose: vi.fn() },
    });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await settle();
    // 6 m wide: 96 dpi still gives 22 677 px, nothing fits.
    const options = [...target.querySelectorAll<HTMLOptionElement>('.export-row select option')];
    expect(options.every((o) => o.disabled)).toBe(true);
    expect(target.querySelector('.export-hint')?.textContent).toContain('PDF ou en SVG');
    const run = [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Exporter…')!;
    expect(run.disabled).toBe(true);
    // Vector formats have no such limit.
    target.querySelector<HTMLInputElement>('input[name="export-format"][value="pdf"]')!.click();
    await settle();
    expect(run.disabled).toBe(false);
  });

  it('shows no hint and disables nothing for an ordinary page', async () => {
    const ui = await render(['Liste de courses']);
    expect([...ui.resolutionSelect()!.options].some((o) => o.disabled)).toBe(false);
    expect(ui.target.querySelector('.export-hint')).toBeNull();
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

  it('exports WebP like PNG: resolution, transparent background, one file per note in a folder', async () => {
    const spy = vi.spyOn(api, 'exportNotes');
    const ui = await render(['Réunion parents', 'Liste de courses']);
    await ui.choose('WebP (image)');
    expect(ui.resolutionSelect()).not.toBeNull();
    expect(ui.qualityRange()).toBeNull();
    expect(ui.target.textContent).toContain('Décoché : le fond est transparent');
    ui.background().click();
    ui.button('Exporter…').click();
    await settle();
    expect(spy).toHaveBeenCalledWith(
      expect.any(Array),
      'webp',
      { dpi: 150, background: false, quality: 90, pdfPage: 'actual' },
      'C:\\Users\\demo\\Exports',
    );
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('Fichiers créés : 2');
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

  it('disables the settings while exporting, then shows the result', async () => {
    let finish: (paths: string[]) => void = () => {};
    vi.spyOn(api, 'exportNotes').mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
    const ui = await render(['Liste de courses']);
    ui.button('Exporter…').click();
    await settle();
    expect(ui.target.querySelector('[role="status"]')?.textContent).toBe('Export en cours…');
    expect(ui.button('Export en cours…').disabled).toBe(true);
    expect(ui.button("Annuler l'export").disabled).toBe(false);
    expect([...ui.target.querySelectorAll('button')].some((b) => b.textContent?.trim() === 'Fermer')).toBe(false);
    expect(ui.target.querySelector('fieldset')?.disabled).toBe(true);

    finish(['C:\\x\\a.png']);
    await settle();
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('Fichiers créés : 1');
    expect(ui.button('Fermer').disabled).toBe(false);
  });

  it('cancels the export with the button: a neutral notice, no red banner, and the dialog stays', async () => {
    let fail: (reason: unknown) => void = () => {};
    vi.spyOn(api, 'exportNotes').mockImplementationOnce(() => new Promise((_, reject) => (fail = reject)));
    const cancelSpy = vi.spyOn(api, 'cancelExport').mockResolvedValue(undefined);
    const ui = await render(['Liste de courses']);
    ui.button('Exporter…').click();
    await settle();
    ui.button("Annuler l'export").click();
    await settle();
    expect(cancelSpy).toHaveBeenCalledTimes(1);
    // Waiting for the desktop app to stop: no second request, a visible state.
    expect(ui.target.querySelector('.export-status')?.textContent).toBe('Annulation…');
    expect(ui.button('Annulation…').disabled).toBe(true);

    fail({ code: 'cancelled', message: 'cancelled' });
    await settle();
    expect(ui.target.querySelector('[role="alert"]')).toBeNull();
    expect(ui.target.querySelector('.banner.error')).toBeNull();
    expect(ui.target.querySelector('.export-notice')?.textContent).toBe('Export annulé.');
    expect(ui.target.querySelector('.export-result')).toBeNull();
    expect(ui.onclose).not.toHaveBeenCalled();
    // Back to idle: export again, or close.
    expect(ui.button('Exporter…').disabled).toBe(false);
    expect(ui.button('Annuler').disabled).toBe(false);
  });

  it('cancels the export with Escape instead of closing the dialog', async () => {
    vi.spyOn(api, 'exportNotes').mockImplementationOnce(() => new Promise(() => {}));
    const cancelSpy = vi.spyOn(api, 'cancelExport').mockResolvedValue(undefined);
    const ui = await render(['Liste de courses']);
    ui.button('Exporter…').click();
    await settle();
    ui.dialog().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle();
    expect(cancelSpy).toHaveBeenCalledTimes(1);
    expect(ui.onclose).not.toHaveBeenCalled();
  });

  it('cancels a pending export of the browser stand-in: it rejects and the notice shows', async () => {
    mockExport.delayMs = 5000;
    try {
      const ui = await render(['Liste de courses'], 'en');
      ui.button('Export…').click();
      await settle();
      expect(ui.target.querySelector('.export-status')?.textContent).toBe('Exporting…');
      ui.button('Cancel the export').click();
      await settle();
      expect(ui.target.querySelector('.export-notice')?.textContent).toBe('Export cancelled.');
      expect(ui.target.querySelector('[role="alert"]')).toBeNull();
      expect(ui.button('Export…').disabled).toBe(false);
    } finally {
      mockExport.delayMs = 0;
    }
  });

  it('reports a failure to cancel as an error', async () => {
    vi.spyOn(api, 'exportNotes').mockImplementationOnce(() => new Promise(() => {}));
    vi.spyOn(api, 'cancelExport').mockRejectedValueOnce({ code: 'exportFailed', message: 'x' });
    const ui = await render(['Liste de courses']);
    ui.button('Exporter…').click();
    await settle();
    ui.button("Annuler l'export").click();
    await settle();
    expect(ui.target.querySelector('[role="alert"]')).toBeTruthy();
    expect(ui.button("Annuler l'export").disabled).toBe(false);
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
