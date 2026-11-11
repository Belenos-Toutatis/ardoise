// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The editor's page buttons inside the main window, with the real editor: they walk through the
 * notes as the library shows them now (its sort and its search), and the library gives its focus
 * and its scroll back to the note the editor ended on.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../lib/mock-data';
import { resetMockNarration } from '../lib/mock-narration';
import { resetMockShare } from '../lib/mock-share';
import { resetMockTablet } from '../lib/mock-tablet';
import { share } from '../lib/share-state.svelte';
import { tablet } from '../lib/tablet-state.svelte';
import Home from './Home.svelte';

vi.mock('../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

vi.mock('./VideoExportDialog.svelte', async () => ({
  default: (await import('./editor/VideoExportDialogDouble.svelte')).default,
}));

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

const PHYSICS = 'Cours de physique';
const SHOPPING = 'Liste de courses';
const PARENTS = 'Réunion parents';

let cleanup: (() => void) | null = null;
const scrolled = vi.fn();

beforeEach(() => {
  resetMockLibrary();
  resetMockTablet();
  resetMockNarration();
  resetMockShare();
  tablet.reset();
  share.reset();
  setLocale('fr');
  scrolled.mockClear();
  // jsdom has no scrollIntoView: a spy tells which element was brought into view.
  Element.prototype.scrollIntoView = function (this: Element) {
    scrolled(this);
  };
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
  resetMockTablet();
  Reflect.deleteProperty(Element.prototype, 'scrollIntoView');
  vi.restoreAllMocks();
});

const pathOf = (title: string) => [...mockLibrary().notes.keys()].find((p) => p.includes(title))!;

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Home, { target, props: { app, onchange: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const cards = () => [...target.querySelectorAll<HTMLElement>('.note-card')];
  const cardOf = (title: string) => cards().find((c) => c.dataset.path === pathOf(title))!;
  const shell = () => target.querySelector<HTMLElement>('.shell')!;
  const next = () => target.querySelector<HTMLButtonElement>('button[data-action="page-next"]');
  const previous = () => target.querySelector<HTMLButtonElement>('button[data-action="page-previous"]');
  const position = () => target.querySelector('[data-slot="page-position"]')?.textContent?.trim() ?? null;
  const shown = async (title: string) => {
    await vi.waitFor(() => expect(target.querySelector<HTMLInputElement>('.title-field input')?.value).toBe(title));
    await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
    await settle();
  };
  const open = async (title: string) => {
    cardOf(title).querySelector<HTMLButtonElement>('.card-open')!.click();
    await shown(title);
  };
  const back = async () => {
    [...target.querySelectorAll('button')].find((b) => b.textContent?.includes('Bibliothèque'))!.click();
    await settle();
  };
  const search = async (text: string) => {
    const field = target.querySelector<HTMLInputElement>('input.search')!;
    field.value = text;
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  };
  const sortBy = async (order: string) => {
    const select = target.querySelector<HTMLSelectElement>('select.sort')!;
    select.value = order;
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
  };
  return { target, cards, cardOf, shell, next, previous, position, shown, open, back, search, sortBy };
}

describe('the editor\'s page buttons in the main window', () => {
  it('walk through the notes in the order the library shows them (newest first by default)', async () => {
    const view = await render();
    expect(view.cards().map((c) => c.dataset.path)).toEqual([PARENTS, SHOPPING, PHYSICS].map(pathOf));
    await view.open(PARENTS);
    expect(view.position()).toBe('1 / 3');
    expect(view.previous()?.disabled).toBe(true);
    view.next()!.click();
    await view.shown(SHOPPING);
    expect(view.position()).toBe('2 / 3');
    view.next()!.click();
    await view.shown(PHYSICS);
    expect(view.position()).toBe('3 / 3');
    expect(view.next()?.disabled).toBe(true);
    view.previous()!.click();
    await view.shown(SHOPPING);
  });

  it('follow the sort and the search of the library', async () => {
    const view = await render();
    await view.sortBy('title-asc');
    expect(view.cards().map((c) => c.dataset.path)).toEqual([PHYSICS, SHOPPING, PARENTS].map(pathOf));
    await view.open(PHYSICS);
    view.next()!.click();
    await view.shown(SHOPPING);
    view.next()!.click();
    await view.shown(PARENTS);
    await view.back();

    // A search that keeps two notes: the third one is not a page of this list.
    await view.sortBy('date-desc');
    await view.search('c');
    expect(view.cards().map((c) => c.dataset.path)).toEqual([SHOPPING, PHYSICS].map(pathOf));
    await view.open(SHOPPING);
    expect(view.position()).toBe('1 / 2');
    view.next()!.click();
    await view.shown(PHYSICS);
    expect(view.position()).toBe('2 / 2');
    expect(view.next()?.disabled).toBe(true);
  });

  it('are hidden when the search leaves only the open note', async () => {
    const view = await render();
    await view.search('physique');
    expect(view.cards()).toHaveLength(1);
    await view.open(PHYSICS);
    expect(view.next()).toBeNull();
    expect(view.previous()).toBeNull();
    expect(view.position()).toBeNull();
  });

  it('give the library\'s focus and scroll to the note the editor ended on', async () => {
    const view = await render();
    const scroller = view.target.querySelector<HTMLElement>('.library-scroll')!;
    scroller.scrollTop = 120;
    await view.open(PARENTS);
    view.next()!.click();
    await view.shown(SHOPPING);
    view.next()!.click();
    await view.shown(PHYSICS);
    (document.activeElement as HTMLElement | null)?.blur();
    scrolled.mockClear();

    await view.back();
    expect(view.shell().classList.contains('shell-hidden')).toBe(false);
    const card = view.cardOf(PHYSICS).querySelector('.card-open');
    expect(document.activeElement).toBe(card);
    expect(scrolled).toHaveBeenCalledWith(card);
    expect(scroller.scrollTop).toBe(120);
  });

  it('give the focus back to the note when it is renamed on the way', async () => {
    const view = await render();
    await view.open(PARENTS);
    view.next()!.click();
    await view.shown(SHOPPING);
    const field = view.target.querySelector<HTMLInputElement>('.title-field input')!;
    field.focus();
    field.value = 'Courses de la semaine';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.blur();
    await vi.waitFor(() => {
      expect([...mockLibrary().notes.values()].map((n) => n.note.title)).toContain('Courses de la semaine');
    });
    await settle();
    // Still a page of the list, whichever place the new name gets in it.
    expect(view.position()).not.toBeNull();

    await view.back();
    const renamed = [...mockLibrary().notes.keys()].find((p) => p.includes('Courses de la semaine'))!;
    const card = view.cards().find((c) => c.dataset.path === renamed)!;
    expect(document.activeElement).toBe(card.querySelector('.card-open'));
  });
});
