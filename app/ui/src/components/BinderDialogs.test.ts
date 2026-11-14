// @vitest-environment jsdom
// « Exporter le classeur en PDF » and « Imprimer le classeur » of a binder whose notes are all in its sub-binders (a binder
// « oral du dnb » with the notes in « oral du dnb/2026 »): the dialogs start with the sub-binders included and never offer
// to export nothing; the dates of the summary and of the bookmarks are one remembered choice.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../lib/i18n.svelte';
import { mockExportRequest, mockPrint } from '../lib/mock-export';
import { mockLibrary, resetMockLibrary } from '../lib/mock-data';
import { loadShowDates } from '../lib/binder-options';
import type { ExportSource } from '../lib/note';
import ExportDialog from './ExportDialog.svelte';
import PrintDialog from './PrintDialog.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  mockPrint.last = null;
  mockExportRequest.last = null;
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

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

/** A binder of `own` notes and `below` notes in its sub-binder; an untitled note's name follows `withDates`. */
function fakeBinder(own: number, below: number) {
  const paths = [...mockLibrary().notes.keys()];
  const source = (index: number, path: string[]): ExportSource => ({
    path: paths[index % paths.length],
    outline: { path, title: `note ${index}` },
  });
  return {
    name: 'oral du dnb',
    hasSub: below > 0 || own === 0,
    sourcesFor: vi.fn((includeSub: boolean, withDates: boolean) => [
      ...Array.from({ length: own }, (_, i) => source(i, [])),
      ...(includeSub ? Array.from({ length: below }, (_, i) => source(own + i, ['2026'])) : []),
    ].map((s) => ({ ...s, outline: { ...s.outline!, title: `${s.outline!.title}${withDates ? ' (dates)' : ''}` } })),
    ),
  };
}

async function mountDialog(kind: 'export' | 'print', binder: ReturnType<typeof fakeBinder>, locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const props = { sources: [] as ExportSource[], binder, subject: 'Sujet', onclose: vi.fn() };
  const component = mount(kind === 'export' ? ExportDialog : PrintDialog, { target, props });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const run = target.querySelector<HTMLButtonElement>(kind === 'export' ? '.actions .button.primary' : '[data-action="print-run"]')!;
  const include = () => target.querySelector<HTMLInputElement>('input[data-option="include-sub-binders"]');
  const text = () => target.textContent ?? '';
  const click = async (element: HTMLElement | null) => {
    element!.click();
    await settle();
  };
  return { target, run, include, text, click };
}

describe.each(['export', 'print'] as const)('the %s dialog of a binder', (kind) => {
  it('starts with the sub-binders included when the binder has no note of its own, and says why', async () => {
    const view = await mountDialog(kind, fakeBinder(0, 2));
    expect(view.include()!.checked).toBe(true);
    expect(view.text()).toContain("n'a pas de note à lui");
    expect(view.text()).toContain('qui sont donc inclus');
    expect(view.run.disabled).toBe(false);
    // Unchecked, there is nothing to take: the button waits and the sentence says what to tick.
    await view.click(view.include());
    expect(view.run.disabled).toBe(true);
    expect(view.text()).toContain('cochez « Inclure les sous-classeurs »');
    expect(view.target.querySelector('[role="alert"]')).toBeNull();
    // And back.
    await view.click(view.include());
    expect(view.run.disabled).toBe(false);
  });

  it('leaves the sub-binders out when the binder has notes of its own', async () => {
    const view = await mountDialog(kind, fakeBinder(2, 3));
    expect(view.include()!.checked).toBe(false);
    expect(view.target.querySelector('[data-slot="binder-only-sub"]')).toBeNull();
    expect(view.run.disabled).toBe(false);
  });

  it('is disabled, with a plain sentence and no error, for a binder that has no note at all', async () => {
    const view = await mountDialog(kind, fakeBinder(0, 0));
    expect(view.text()).toContain('Ce classeur ne contient aucune note.');
    expect(view.run.disabled).toBe(true);
    expect(view.text()).not.toContain('place');
    expect(view.target.querySelector('[role="alert"]')).toBeNull();
    view.run.click();
    await settle();
    expect(mockExportRequest.last).toBeNull();
    expect(mockPrint.last).toBeNull();
  });

  it('says it in English too', async () => {
    const view = await mountDialog(kind, fakeBinder(0, 0), 'en');
    expect(view.text()).toContain('This binder has no notes.');
    const other = await mountDialog(kind, fakeBinder(0, 1), 'en');
    expect(other.text()).toContain('no notes of its own');
  });
});

describe('the dates of a binder\'s PDF', () => {
  it('are off at first, asked of the binder (the bookmarks\' names) and remembered', async () => {
    const binder = fakeBinder(2, 0);
    const view = await mountDialog('export', binder);
    const box = view.target.querySelector<HTMLInputElement>('input[data-option="show-dates"]')!;
    expect(box.checked).toBe(false);
    expect(binder.sourcesFor).toHaveBeenLastCalledWith(false, false);
    await view.click(box);
    expect(binder.sourcesFor).toHaveBeenLastCalledWith(false, true);
    expect(loadShowDates()).toBe(true);
    view.run.click();
    await vi.waitFor(() => expect(mockExportRequest.last).not.toBeNull());
    expect(mockExportRequest.last!.outline!.map((o) => o.title)).toEqual(['note 0 (dates)', 'note 1 (dates)']);
    cleanup?.();
    // The next dialog starts as the user left it.
    const again = await mountDialog('export', fakeBinder(1, 0));
    expect(again.target.querySelector<HTMLInputElement>('input[data-option="show-dates"]')!.checked).toBe(true);
  });

  it('are not a choice of the print dialog (the page draws none)', async () => {
    const view = await mountDialog('print', fakeBinder(2, 0));
    expect(view.target.querySelector('input[data-option="show-dates"]')).toBeNull();
  });
});

describe('the summary page of a binder\'s PDF', () => {
  it('is off at first, asked for with its titles, and remembered', async () => {
    const view = await mountDialog('export', fakeBinder(2, 0));
    const box = view.target.querySelector<HTMLInputElement>('input[data-option="summary-page"]')!;
    expect(box.checked).toBe(false);
    expect(view.text()).toContain('Ajouter une page de sommaire au début');
    view.run.click();
    await vi.waitFor(() => expect(mockExportRequest.last).not.toBeNull());
    expect(mockExportRequest.last!.summary).toBeUndefined();

    mockExportRequest.last = null;
    await view.click(box);
    view.target.querySelector<HTMLButtonElement>('.actions .button.primary')!.click();
    await vi.waitFor(() => expect(mockExportRequest.last).not.toBeNull());
    // The title is the binder's name; the outline's entry is the translated « Sommaire ».
    expect(mockExportRequest.last!.summary).toEqual({ title: 'oral du dnb', outlineTitle: 'Sommaire' });
    cleanup?.();
    // The next dialog starts as left.
    const again = await mountDialog('export', fakeBinder(1, 0), 'en');
    expect(again.target.querySelector<HTMLInputElement>('input[data-option="summary-page"]')!.checked).toBe(true);
    expect(again.text()).toContain('Add a summary page at the start');
    again.target.querySelector<HTMLButtonElement>('.actions .button.primary')!.click();
    await vi.waitFor(() => expect(mockExportRequest.last!.summary).toEqual({ title: 'oral du dnb', outlineTitle: 'Summary' }));
  });

  it('is not offered by the print dialog (PDF only: print the PDF)', async () => {
    const view = await mountDialog('print', fakeBinder(2, 0));
    expect(view.target.querySelector('input[data-option="summary-page"]')).toBeNull();
  });
});
