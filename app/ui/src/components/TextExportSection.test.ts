// @vitest-environment jsdom
// The export dialog also exports the recognised handwriting as text (milestone M7).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../lib/mock-data';
import { mockClipboard } from '../lib/mock-export';
import { mockHandwriting, resetMockHandwriting } from '../lib/mock-handwriting';
import type { ExportSource } from '../lib/note';
import ExportDialog from './ExportDialog.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockHandwriting();
  mockClipboard.available = true;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  delete (window as { __ardoiseE2eSavePath?: unknown }).__ardoiseE2eSavePath;
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
  const component = mount(ExportDialog, { target, props: { sources, subject: 'Sujet', onclose: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const section = () => target.querySelector<HTMLElement>('.text-export')!;
  const action = (name: string) => target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`)!;
  return { target, section, action };
}

describe('export dialog: recognised text', () => {
  it('saves the text of the selected notes as a .txt file', async () => {
    const sources = [{ path: pathOf('Cours de physique') }, { path: pathOf('Liste de courses') }];
    const ui = await render(sources);
    expect(ui.section().querySelector('h3')?.textContent).toBe('Texte reconnu (écriture manuscrite)');
    expect(ui.action('text-save').textContent?.trim()).toBe('Enregistrer en .txt…');
    (window as { __ardoiseE2eSavePath?: unknown }).__ardoiseE2eSavePath = 'C:\\Exports\\texte.txt';
    ui.action('text-save').click();
    await settle();
    expect(mockHandwriting.exported?.destination).toBe('C:\\Exports\\texte.txt');
    expect(mockHandwriting.exported?.document).toContain('Cours de physique\r\n');
    expect(mockHandwriting.exported?.document).toContain('Liste de courses\r\n');
    expect(ui.section().querySelector('.export-result code')?.textContent).toBe('C:\\Exports\\texte.txt');
  });

  it('saves the text with its structure as Markdown or Word, with the words of the interface language', async () => {
    const sources = [{ path: pathOf('Cours de physique') }];
    const fr = await render(sources);
    expect(fr.action('text-md').textContent?.trim()).toBe('Enregistrer en Markdown…');
    expect(fr.action('text-docx').textContent?.trim()).toBe('Enregistrer en Word…');
    (window as { __ardoiseE2eSavePath?: unknown }).__ardoiseE2eSavePath = 'C:\\Exports\\physique.md';
    fr.action('text-md').click();
    await settle();
    expect(mockHandwriting.exportedStructured).toEqual({
      format: 'md',
      destination: 'C:\\Exports\\physique.md',
      labels: { date: 'Créée le :', tags: 'Étiquettes :', drawing: 'Dessin' },
    });
    expect(fr.section().querySelector('.export-result code')?.textContent).toBe('C:\\Exports\\physique.md');
    expect(fr.section().textContent).not.toContain("Windows n'a pas pu analyser");
    cleanup?.();

    const en = await render(sources, 'en');
    (window as { __ardoiseE2eSavePath?: unknown }).__ardoiseE2eSavePath = 'C:\\Exports\\physique.docx';
    en.action('text-docx').click();
    await settle();
    expect(mockHandwriting.exportedStructured).toEqual({
      format: 'docx',
      destination: 'C:\\Exports\\physique.docx',
      labels: { date: 'Created:', tags: 'Tags:', drawing: 'Drawing' },
    });
  });

  it('says why a structured export fails', async () => {
    mockHandwriting.recognizers = [];
    const ui = await render([{ path: pathOf('Cours de physique') }]);
    (window as { __ardoiseE2eSavePath?: unknown }).__ardoiseE2eSavePath = 'C:\\Exports\\x.docx';
    ui.action('text-docx').click();
    await settle();
    expect(ui.section().querySelector('[role="alert"]')?.textContent).toContain(
      "aucun module de reconnaissance d'écriture de Windows n'est installé",
    );
    expect(mockHandwriting.exportedStructured).toBeNull();
  });

  it('copies the text of the note in the editor, unsaved changes included', async () => {
    const path = pathOf('Réunion parents');
    const note = { ...mockLibrary().notes.get(path)!.note, title: 'Réunion (brouillon)' };
    const ui = await render([{ path, note }], 'en');
    expect(ui.action('text-copy').textContent?.trim()).toBe('Copy the text');
    ui.action('text-copy').click();
    await settle();
    expect(mockHandwriting.copied).toBe(
      'Réunion (brouillon)\r\n===================\r\n\r\nrendez-vous jeudi à dix-sept heures\r\n',
    );
    expect(ui.section().textContent).toContain('Text copied to the clipboard.');
    mockClipboard.available = false;
    ui.action('text-copy').click();
    await settle();
    expect(ui.section().querySelector('[role="alert"]')?.textContent).toContain('The text could not be exported');
  });

  it('previews the text, and says when a note has none', async () => {
    const created = await import('../lib/api').then(({ api }) => api.createNote('', 'Vide'));
    const ui = await render([{ path: pathOf('Cours de physique') }, { path: created.path }]);
    const details = ui.section().querySelector('details')!;
    details.open = true;
    details.dispatchEvent(new Event('toggle'));
    await settle();
    expect(ui.section().querySelector('pre')?.textContent).toBe(
      'la lumière se propage\nen ligne droite dans un milieu homogène',
    );
    expect(ui.section().textContent).toContain('Aucun texte reconnu dans cette note.');
  });

  it('says why when no recognizer is installed', async () => {
    mockHandwriting.recognizers = [];
    const ui = await render([{ path: pathOf('Cours de physique') }]);
    ui.action('text-copy').click();
    await settle();
    expect(ui.section().querySelector('[role="alert"]')?.textContent).toContain(
      "aucun module de reconnaissance d'écriture de Windows n'est installé",
    );
    expect(mockHandwriting.copied).toBeNull();
  });
});
