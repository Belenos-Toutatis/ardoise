// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M17, package B: the live control window's Page and Fond tabs (docs/design/paper-projection-narration.md §1.6):
// the paper of the page on screen (`live_page_paper`) and its background image (`live_page_image`), disabled while the
// session does not run, the swatches waiting under the blackboard, the refusals told in words, and the class's remote
// (Page Up and Page Down) turning the page whoever of the two tabs has the focus.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, resetMockLive } from '../../lib/mock-live';
import { mockNarration, resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import { resetMockTablet } from '../../lib/mock-tablet';
import { share } from '../../lib/share-state.svelte';
import { tablet } from '../../lib/tablet-state.svelte';
import { openSideTab } from '../side/side-test-helpers';
import LiveView from './LiveView.svelte';

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

type E2eWindow = { __ardoiseE2eOpenPath?: string };

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLive();
  resetMockTablet();
  resetMockNarration();
  resetMockLibrary();
  resetMockShare();
  share.reset();
  tablet.reset();
  setLocale('fr');
  localStorage.clear();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
  mockNarration.receivingAfterMs = 0;
  mockNarration.silentAfterMs = 0;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  delete (window as E2eWindow).__ardoiseE2eOpenPath;
  share.reset();
  resetMockShare();
  resetMockLive();
  vi.restoreAllMocks();
});

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(LiveView, { target, props: { app, resume: null, onclose: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <E extends Element = HTMLElement>(selector: string) => target.querySelector<E>(selector);
  const canvas = () => q<HTMLCanvasElement>('canvas.live-canvas')!;
  const paperBlock = () => q('[data-section="live-paper"]')!;
  const fondBlock = () => q('[data-section="background-image"]')!;
  const swatch = (id: string) => paperBlock().querySelector<HTMLButtonElement>(`button[data-paper="${id}"]`)!;
  const template = () => paperBlock().querySelector<HTMLSelectElement>('select[data-field="template"]')!;
  const texture = () => paperBlock().querySelector<HTMLInputElement>('input[data-field="texture"]')!;
  const board = () => q<HTMLInputElement>('input[data-field="board"]')!;
  const notice = () => q('.banner.error[role="alert"]');
  const button = (label: string) => {
    const found = [...fondBlock().querySelectorAll('button')].find((b) => b.textContent?.replace(/[⟲⟳]/g, '').trim() === label || b.title === label);
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  return { target, q, canvas, paperBlock, fondBlock, swatch, template, texture, board, notice, button };
}

type View = ReturnType<typeof render>;

async function live(view: View) {
  await vi.waitFor(() => expect(view.q('.live-view')!.dataset.state).toBe('live'));
}

/** Sets a control's value as the user would: the mouse presses first, then the control changes. */
function choose(control: HTMLSelectElement | HTMLInputElement, value: string, byMouse = true) {
  control.focus();
  if (byMouse) control.dispatchEvent(new Event('pointerdown', { bubbles: true }));
  control.value = value;
  control.dispatchEvent(new Event('change', { bubbles: true }));
}

function press(target: EventTarget, key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

describe('the live window Page tab', () => {
  it('shows the paper of the page, with its sentences, in the Page tab', async () => {
    const view = render();
    await live(view);
    const block = view.paperBlock();
    expect(block.closest('[data-tab-panel]')?.getAttribute('data-tab-panel')).toBe('page');
    expect(block.querySelector('h3')!.textContent).toBe('Papier');
    expect(block.textContent).toContain('Pour la page en cours, au vidéoprojecteur et sur le partage aussi. Une nouvelle page garde ce papier.');
    expect(block.textContent).toContain('Pour revenir en arrière, rechoisissez l\'ancien réglage');
    expect([...block.querySelectorAll('button[data-paper]')].map((b) => b.getAttribute('data-paper'))).toEqual(['white', 'cream', 'kraft', 'slate']);
    expect(view.swatch('white').getAttribute('aria-pressed')).toBe('true');
    expect(view.template().value).toBe('blank');
    expect(view.texture().checked).toBe(false);
    expect(view.q('[data-slot="paper-board"]')).toBeNull();
    expect(view.q('[data-slot="paper-slate"]')).toBeNull();
  });

  it('waits while the session does not run', async () => {
    const view = render();
    // Before the session answers: nothing to change yet.
    const controls = [...view.paperBlock().querySelectorAll<HTMLButtonElement>('button, select, input')];
    expect(controls.length).toBeGreaterThan(5);
    expect(controls.every((c) => c.disabled)).toBe(true);
    await live(view);
    expect(view.swatch('cream').disabled).toBe(false);
    expect(view.template().disabled).toBe(false);
    expect(view.texture().disabled).toBe(false);
  });

  it('changes the colour, the template and the texture of the page on screen', async () => {
    const paper = vi.spyOn(api, 'livePagePaper');
    const view = render();
    await live(view);
    view.swatch('cream').click();
    await vi.waitFor(() => expect(view.canvas().dataset.paper).toBe('cream'));
    expect(paper).toHaveBeenLastCalledWith(0, { paper: 'cream', template: 'blank', texture: false });
    expect(view.swatch('cream').getAttribute('aria-pressed')).toBe('true');
    choose(view.template(), 'seyes');
    await vi.waitFor(() => expect(view.canvas().dataset.template).toBe('seyes'));
    expect(paper).toHaveBeenLastCalledWith(0, { paper: 'cream', template: 'seyes', texture: false });
    view.texture().click();
    await vi.waitFor(() => expect(paper).toHaveBeenLastCalledWith(0, { paper: 'cream', template: 'seyes', texture: true }));
    expect(view.q('.live-view')!.querySelector('.banner.error')).toBeNull();
  });

  it('keeps every choice made before the session answers: each one builds on the one before', async () => {
    const answer = api.livePagePaper;
    // The session answers a little later (its snapshot comes after the next click).
    const paper = vi.spyOn(api, 'livePagePaper').mockImplementation(async (pageIndex, background) => {
      await new Promise((resolve) => setTimeout(resolve, 30));
      return answer(pageIndex, background);
    });
    const view = render();
    await live(view);
    // Three choices in a row, without waiting for the snapshots (a fast hand, or a loaded PC).
    view.swatch('cream').click();
    choose(view.template(), 'seyes');
    view.texture().click();
    expect(paper.mock.calls.map((call) => call[1])).toEqual([
      { paper: 'cream', template: 'blank', texture: false },
      { paper: 'cream', template: 'seyes', texture: false },
      { paper: 'cream', template: 'seyes', texture: true },
    ]);
    await vi.waitFor(() => expect(view.canvas().dataset.template).toBe('seyes'));
    expect(view.canvas().dataset.paper).toBe('cream');
    expect(view.swatch('cream').getAttribute('aria-pressed')).toBe('true');
    expect(view.texture().checked).toBe(true);
  });

  it('reads the page index when the click is made: the next page gets its own, and the new page keeps the paper', async () => {
    const paper = vi.spyOn(api, 'livePagePaper');
    const view = render();
    await live(view);
    view.swatch('kraft').click();
    await vi.waitFor(() => expect(view.canvas().dataset.paper).toBe('kraft'));
    press(window, 'PageDown');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    // A new page keeps the paper (O4).
    expect(view.canvas().dataset.paper).toBe('kraft');
    view.swatch('cream').click();
    await vi.waitFor(() => expect(paper).toHaveBeenLastCalledWith(1, { paper: 'cream', template: 'blank', texture: false }));
    // The first page kept its own.
    press(window, 'PageUp');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
    expect(view.canvas().dataset.paper).toBe('kraft');
  });

  it('keeps the swatches for later under the blackboard, and says so; the template and the texture still apply', async () => {
    const paper = vi.spyOn(api, 'livePagePaper');
    const view = render();
    await live(view);
    view.swatch('cream').click();
    await vi.waitFor(() => expect(view.canvas().dataset.paper).toBe('cream'));
    view.board().click();
    await vi.waitFor(() => expect(view.canvas().dataset.paper).toBe('slate'));
    for (const id of ['white', 'cream', 'kraft', 'slate']) expect(view.swatch(id).disabled, id).toBe(true);
    // The own paper is still the one shown as chosen.
    expect(view.swatch('cream').getAttribute('aria-pressed')).toBe('true');
    expect(view.q('[data-slot="paper-board"]')!.textContent).toContain('Tableau noir : la couleur du papier reviendra quand il sera éteint (B).');
    expect(view.q('[data-slot="paper-slate"]')).toBeNull();
    expect(view.template().disabled).toBe(false);
    choose(view.template(), 'lined');
    await vi.waitFor(() => expect(view.canvas().dataset.template).toBe('lined'));
    expect(paper).toHaveBeenLastCalledWith(0, { paper: 'cream', template: 'lined', texture: false });
    // Still slate under the board; board off: the paper comes back.
    expect(view.canvas().dataset.paper).toBe('slate');
    view.board().click();
    await vi.waitFor(() => expect(view.canvas().dataset.paper).toBe('cream'));
    expect(view.q('[data-slot="paper-board"]')).toBeNull();
    expect(view.swatch('kraft').disabled).toBe(false);
  });

  it('suggests the blackboard when slate is chosen as a paper without it', async () => {
    const view = render();
    await live(view);
    view.swatch('slate').click();
    await vi.waitFor(() => expect(view.canvas().dataset.paper).toBe('slate'));
    expect(view.q('[data-slot="paper-slate"]')!.textContent).toContain('Pour écrire à la craie, allumez plutôt le tableau noir (B).');
    view.swatch('white').click();
    await vi.waitFor(() => expect(view.q('[data-slot="paper-slate"]')).toBeNull());
  });

  it('says that the page changed meanwhile, and what else was refused, in words', async () => {
    const view = render();
    await live(view);
    const paper = vi.spyOn(api, 'livePagePaper').mockRejectedValueOnce({ code: 'liveEditStale', message: 'stale' });
    view.swatch('cream').click();
    await vi.waitFor(() => expect(view.notice()?.textContent).toContain('La page a changé entre-temps : refaites le choix.'));
    paper.mockRejectedValueOnce({ code: 'liveNotRunning', message: 'stopped' });
    view.swatch('kraft').click();
    await vi.waitFor(() =>
      expect(view.notice()?.textContent).toContain("Le papier de la page n'a pas pu être changé. Le mode direct n'est pas lancé."),
    );
  });

  it('speaks English', async () => {
    setLocale('en');
    const view = render();
    await live(view);
    expect(view.paperBlock().querySelector('h3')!.textContent).toBe('Paper');
    expect(view.paperBlock().textContent).toContain('For the current page, on the projector and on the share too.');
    view.board().click();
    await vi.waitFor(() => expect(view.q('[data-slot="paper-board"]')!.textContent).toContain('Blackboard on:'));
  });
});

describe('the class\'s remote in the Page and Fond tabs', () => {
  it('turns the page with Page Down and Page Up with the template select focused, and does not change the paper', async () => {
    const paper = vi.spyOn(api, 'livePagePaper');
    const view = render();
    await live(view);
    await openSideTab(view.target, 'page');
    const select = view.template();
    select.focus();
    expect(document.activeElement).toBe(select);
    const down = press(select, 'PageDown');
    expect(down.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    const up = press(select, 'PageUp');
    expect(up.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
    expect(paper).not.toHaveBeenCalled();
    expect(select.value).toBe('blank');
  });

  it('turns it from a swatch and from the texture switch too', async () => {
    const view = render();
    await live(view);
    await openSideTab(view.target, 'page');
    for (const control of [view.swatch('cream'), view.texture()]) {
      control.focus();
      press(control, 'PageDown');
      press(control, 'PageUp');
    }
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
  });

  it('keeps the other keys of the select to the select', async () => {
    const view = render();
    await live(view);
    await openSideTab(view.target, 'page');
    const select = view.template();
    select.focus();
    for (const key of ['ArrowDown', 'b', 'l', 'w', ' ']) expect(press(select, key).defaultPrevented, key).toBe(false);
    expect(view.canvas().dataset.page).toBe('1/1');
    expect(view.board().checked).toBe(false);
  });

  it('turns the page from the Fond tab\'s select and slider', async () => {
    window.localStorage.clear();
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Photos\\tableau.jpg';
    const view = render();
    await live(view);
    await openSideTab(view.target, 'background');
    view.button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() => expect(view.canvas().dataset.image).toBe('tableau.jpg'));
    const fit = () => view.fondBlock().querySelector<HTMLSelectElement>('select')!;
    const opacity = () => view.fondBlock().querySelector<HTMLInputElement>('input[type="range"]')!;
    for (const control of [fit, opacity]) {
      // The controls of page 1 go with the page (it has no image on page 2): each time they are found again.
      control().focus();
      expect(press(control(), 'PageDown').defaultPrevented).toBe(true);
      await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
      press(window, 'PageUp');
      await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
      await vi.waitFor(() => expect(view.canvas().dataset.image).toBe('tableau.jpg'));
    }
    // The image did not change under the remote's keys.
    expect(opacity().value).toBe('100');
    expect(fit().value).toBe('contain');
  });

  it('gives the focus back after a choice made with the mouse, and keeps it after one made with the keyboard', async () => {
    const view = render();
    await live(view);
    await openSideTab(view.target, 'page');
    const select = view.template();
    choose(select, 'grid5mm', false);
    await vi.waitFor(() => expect(view.canvas().dataset.template).toBe('grid5mm'));
    expect(document.activeElement).toBe(select);
    press(select, 'ArrowDown');
    choose(select, 'dotted', true);
    await vi.waitFor(() => expect(view.canvas().dataset.template).toBe('dotted'));
    expect(document.activeElement).not.toBe(select);
    // A press of the mouse on a key that follows: the keyboard's turn again.
    select.focus();
    press(select, 'ArrowUp');
    choose(select, 'lined', false);
    expect(document.activeElement).toBe(select);
  });
});

describe('the live window Fond tab', () => {
  it('shows the image block in the Fond tab with its sentence, waiting while the session does not run', async () => {
    const view = render();
    const block = view.fondBlock();
    expect(block.closest('[data-tab-panel]')?.getAttribute('data-tab-panel')).toBe('background');
    expect(block.querySelector('h3')!.textContent).toBe('Image ou PDF de fond');
    expect(block.querySelector('.pane-help')!.textContent).toBe(
      "Sous l'écriture de la page en cours : une fiche ou un document à compléter devant la classe. La page suivante n'a pas d'image.",
    );
    expect(view.button('Choisir un PDF ou une image…').disabled).toBe(true);
    await live(view);
    expect(view.button('Choisir un PDF ou une image…').disabled).toBe(false);
  });

  it('puts a page of a PDF under the page, with no offer to turn the page, then places and removes it', async () => {
    const stage = vi.spyOn(api, 'stageBackgroundImage');
    const set = vi.spyOn(api, 'livePageImage');
    const view = render();
    await live(view);
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Documents\\fiche.pdf';
    view.button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() => expect(view.fondBlock().querySelector('[data-slot="background-pdf"]')).toBeTruthy());
    choose(view.fondBlock().querySelector('select')!, '2');
    view.button('Mettre en fond').click();
    await vi.waitFor(() => expect(view.canvas().dataset.image).toBe('fiche.pdf'));
    // A portrait PDF on the tablet's landscape page: no question, the page stays as the tablet holds it.
    expect(view.fondBlock().querySelector('[data-slot="background-orientation-offer"]')).toBeNull();
    expect(stage).toHaveBeenCalledWith('C:\\Documents\\fiche.pdf', 2);
    const [pageIndex, op] = set.mock.calls[0];
    expect(pageIndex).toBe(0);
    expect(op).toMatchObject({ kind: 'set', key: expect.stringMatching(/^mock-/), image: { name: 'fiche.pdf', pdfPage: 2, quarterTurns: 0, opacity: 1 } });
    expect(view.fondBlock().querySelector('[data-slot="background-image-name"]')!.textContent).toContain('fiche.pdf, page 2');
    expect(view.canvas().dataset.page).toBe('1/1');

    // The fit, a quarter turn of the image alone, the opacity: `place`, with the page's own pixels.
    choose(view.fondBlock().querySelector('select')!, 'cover');
    await vi.waitFor(() => expect(set).toHaveBeenCalledTimes(2));
    const cover = set.mock.calls[1][1] as { kind: string; image: { rect: { width: number } } };
    expect(cover.kind).toBe('place');
    expect(cover.image.rect.width).toBeCloseTo(29700, 0);
    view.button('Image à droite').click();
    await vi.waitFor(() => expect(set).toHaveBeenCalledTimes(3));
    expect(set.mock.calls[2][1]).toMatchObject({ kind: 'place', image: { quarterTurns: 1 } });
    choose(view.fondBlock().querySelector<HTMLInputElement>('input[type="range"]')!, '50');
    await vi.waitFor(() => expect(set).toHaveBeenCalledTimes(4));
    expect(set.mock.calls[3][1]).toMatchObject({ kind: 'place', image: { opacity: 0.5, quarterTurns: 1 } });
    expect(view.canvas().dataset.image).toBe('fiche.pdf');

    view.button("Retirer l'image").click();
    await vi.waitFor(() => expect(view.canvas().dataset.image).toBe(''));
    expect(set).toHaveBeenLastCalledWith(0, { kind: 'remove' });
    expect(view.fondBlock().querySelector('[data-slot="background-image-name"]')).toBeNull();
  });

  it('keeps the image on its page: the next page has none, the first page finds it again', async () => {
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Photos\\tableau.jpg';
    const view = render();
    await live(view);
    view.button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() => expect(view.canvas().dataset.image).toBe('tableau.jpg'));
    press(window, 'PageDown');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    expect(view.canvas().dataset.image).toBe('');
    expect(view.button('Choisir un PDF ou une image…').disabled).toBe(false);
    press(window, 'PageUp');
    await vi.waitFor(() => expect(view.canvas().dataset.image).toBe('tableau.jpg'));
  });

  it('refuses an image for a page that changed during the import, and says so', async () => {
    let finish: (value: { key: string; name: string; width: number; height: number }) => void = () => {};
    vi.spyOn(api, 'stageBackgroundImage').mockImplementation(() => new Promise((resolve) => (finish = resolve)));
    const view = render();
    await live(view);
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Photos\\tableau.jpg';
    // The click that chooses the file is made on page 1; the import takes a while, the pages turn meanwhile.
    view.button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() => expect(view.fondBlock().querySelector('[role="status"]')).toBeTruthy());
    press(window, 'PageDown');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    finish({ key: 'mock-42', name: 'tableau.jpg', width: 1600, height: 1200 });
    await vi.waitFor(() => expect(view.notice()?.textContent).toContain('La page a changé entre-temps : refaites le choix.'));
    expect(view.canvas().dataset.image).toBe('');
    expect(view.fondBlock().querySelector('[role="status"]')).toBeNull();
  });

  it('says why an image could not be put, with the sentence of the refusal', async () => {
    vi.spyOn(api, 'livePageImage').mockRejectedValueOnce({ code: 'backgroundMissing', message: 'mock-1' });
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Photos\\tableau.jpg';
    const view = render();
    await live(view);
    view.button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() =>
      expect(view.notice()?.textContent).toContain("L'image de fond n'a pas pu être changée. l'image de fond n'est plus en mémoire"),
    );
    expect(view.canvas().dataset.image).toBe('');
  });

  it('speaks English', async () => {
    setLocale('en');
    const view = render();
    await live(view);
    expect(view.fondBlock().querySelector('.pane-help')!.textContent).toContain('Under the writing of the current page');
    await tick();
  });
});
