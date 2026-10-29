// @vitest-environment jsdom
// The export dialog also exports the recognised handwriting as text (milestone M7).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { drawingCheck } from '../lib/export-options';
import { setLocale } from '../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../lib/mock-data';
import { mockClipboard } from '../lib/mock-export';
import { mockDrawings, mockHandwriting, resetMockHandwriting } from '../lib/mock-handwriting';
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
      includeDrawings: true,
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
      includeDrawings: true,
    });
  });

  it('includes the drawings only when the notes have some, and lets the user leave them out', async () => {
    const drawingsOption = (ui: Awaited<ReturnType<typeof render>>) =>
      ui.target.querySelector<HTMLInputElement>('input[data-option="text-drawings"]')!;
    const hint = (ui: Awaited<ReturnType<typeof render>>) =>
      ui.target.querySelector('[data-hint="text-drawings"]')?.textContent?.trim();

    // The physics lesson has a drawing: on, and it can be turned off.
    const lesson = await render([{ path: pathOf('Cours de physique') }]);
    expect(lesson.section().textContent).toContain('Inclure les dessins (Markdown, Word)');
    expect(drawingsOption(lesson).checked).toBe(true);
    expect(drawingsOption(lesson).disabled).toBe(false);
    expect(hint(lesson)).toContain('ces mots sont soulignés');
    drawingsOption(lesson).click();
    await settle();
    (window as { __ardoiseE2eSavePath?: unknown }).__ardoiseE2eSavePath = 'C:\\Exports\\sans-dessins.md';
    lesson.action('text-md').click();
    await settle();
    expect(mockHandwriting.exportedStructured?.includeDrawings).toBe(false);
    cleanup?.();

    // The shopping list has none: off, and nothing to choose.
    const list = await render([{ path: pathOf('Liste de courses') }], 'en');
    expect(list.section().textContent).toContain('Include the drawings (Markdown, Word)');
    expect(drawingsOption(list).checked).toBe(false);
    expect(drawingsOption(list).disabled).toBe(true);
    expect(hint(list)).toBe('No drawing found: the text alone is exported.');
    (window as { __ardoiseE2eSavePath?: unknown }).__ardoiseE2eSavePath = 'C:\\Exports\\list.docx';
    list.action('text-docx').click();
    await settle();
    expect(mockHandwriting.exportedStructured?.includeDrawings).toBe(false);
    expect(mockHandwriting.drawingChecks).toBe(2);
    cleanup?.();

    // Several notes, one with a drawing: on.
    mockDrawings.set(mockLibrary().notes.get(pathOf('Liste de courses'))!.note.id, 2);
    const both = await render([{ path: pathOf('Liste de courses') }, { path: pathOf('Réunion parents') }]);
    expect(drawingsOption(both).checked).toBe(true);
  });

  it('waits for the drawings to be looked for before a Markdown or Word export', async () => {
    let answer: (count: number) => void = () => {};
    const { api } = await import('../lib/api');
    const check = vi.spyOn(api, 'structuredTextDrawings').mockReturnValue(
      new Promise<number>((resolve) => {
        answer = resolve;
      }),
    );
    const ui = await render([{ path: pathOf('Cours de physique') }]);
    const option = ui.target.querySelector<HTMLInputElement>('input[data-option="text-drawings"]')!;
    expect(ui.target.querySelector('[data-hint="text-drawings"]')?.textContent?.trim()).toBe('Recherche des dessins…');
    expect(option.disabled).toBe(true);
    expect(ui.action('text-md').disabled).toBe(true);
    expect(ui.action('text-docx').disabled).toBe(true);
    // The plain text needs no drawing.
    expect(ui.action('text-save').disabled).toBe(false);
    expect(ui.action('text-copy').disabled).toBe(false);
    answer(1);
    await settle();
    expect(ui.action('text-md').disabled).toBe(false);
    expect(option.disabled).toBe(false);
    expect(option.checked).toBe(true);
    check.mockRestore();
  });

  it('stops waiting for the drawings after a few seconds: the buttons work, the drawings stay in', async () => {
    const { api } = await import('../lib/api');
    const check = vi.spyOn(api, 'structuredTextDrawings').mockReturnValue(new Promise<number>(() => {}));
    drawingCheck.limitMs = 400;
    try {
      const ui = await render([{ path: pathOf('Cours de physique') }]);
      expect(ui.action('text-md').disabled).toBe(true);
      await new Promise((resolve) => setTimeout(resolve, 450));
      await settle();
      expect(ui.action('text-md').disabled).toBe(false);
      expect(ui.action('text-docx').disabled).toBe(false);
      expect(ui.target.querySelector<HTMLInputElement>('input[data-option="text-drawings"]')!.checked).toBe(true);
    } finally {
      drawingCheck.limitMs = 5000;
      check.mockRestore();
    }
  });

  it('keeps the drawings when they cannot be looked for', async () => {
    // Too many notes to analyse them all when the dialog opens: not looked for, on.
    const many = Array.from({ length: 6 }, () => ({ path: pathOf('Liste de courses') }));
    const ui = await render(many);
    const option = ui.target.querySelector<HTMLInputElement>('input[data-option="text-drawings"]')!;
    expect(mockHandwriting.drawingChecks).toBe(0);
    expect(option.checked).toBe(true);
    expect(option.disabled).toBe(false);
    cleanup?.();
    // The search fails (no recognizer): on, the export says why it fails.
    mockHandwriting.recognizers = [];
    const failed = await render([{ path: pathOf('Liste de courses') }]);
    const kept = failed.target.querySelector<HTMLInputElement>('input[data-option="text-drawings"]')!;
    expect(kept.checked).toBe(true);
    expect(kept.disabled).toBe(false);
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
