// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { openPdfPreviews } from '../../lib/mock-background';
import { KEPT } from '../../lib/pdf-thumbnails.svelte';
import PdfPagePickerHarness from './PdfPagePickerHarness.svelte';

const A4 = { width: 595, height: 842 };
const pagesOf = (count: number) => Array.from({ length: count }, () => ({ ...A4 }));

/** An IntersectionObserver the test drives: `reveal` says which pages are in view. */
class FakeObserver {
  static all: FakeObserver[] = [];
  private cells = new Set<Element>();
  private callback: IntersectionObserverCallback;
  readonly options?: IntersectionObserverInit;
  constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
    this.callback = callback;
    this.options = options;
    FakeObserver.all.push(this);
  }
  observe(element: Element) {
    this.cells.add(element);
  }
  unobserve(element: Element) {
    this.cells.delete(element);
  }
  disconnect() {
    this.cells.clear();
    FakeObserver.all = FakeObserver.all.filter((o) => o !== this);
  }
  /** Reports every observed cell: in view when its page is in `pages`. */
  reveal(pages: number[]) {
    const entries = [...this.cells].map(
      (target) =>
        ({ target, isIntersecting: pages.includes(Number((target as HTMLElement).dataset.page)) }) as IntersectionObserverEntry,
    );
    this.callback(entries, this as unknown as IntersectionObserver);
  }
}
const reveal = (pages: number[]) => FakeObserver.all.forEach((o) => o.reveal(pages));

let cleanup: (() => void) | null = null;

beforeEach(() => {
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.unstubAllGlobals();
  FakeObserver.all = [];
  vi.restoreAllMocks();
});

type Harness = { setFile: (path: string, pages: { width: number; height: number }[]) => void };

function open(pages = pagesOf(3), path = 'C:\\Documents\\fiche.pdf') {
  const target = document.createElement('div');
  document.body.append(target);
  const harness = mount(PdfPagePickerHarness, { target, props: { path, pages } }) as unknown as Harness;
  cleanup = () => {
    unmount(harness as never);
    target.remove();
  };
  const radios = () => [...target.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
  const radio = (page: number) => {
    const found = radios().find((r) => r.dataset.page === String(page));
    if (!found) throw new Error(`no page ${page}`);
    return found;
  };
  const chosen = () => target.querySelector('[data-slot="selected"]')!.textContent;
  const group = () => target.querySelector<HTMLElement>('[role="radiogroup"]')!;
  const images = () => [...target.querySelectorAll('img')];
  const key = (page: number, name: string) => {
    radio(page).focus();
    group().dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true }));
  };
  return { target, harness, radios, radio, chosen, group, images, key };
}

describe('the page picker', () => {
  it('shows a labelled thumbnail per page, the first one chosen, one tab stop', async () => {
    const view = open(pagesOf(3));
    expect(view.group().getAttribute('aria-label')).toBe('Pages de « fiche.pdf »');
    expect(view.radios().map((r) => r.getAttribute('aria-label'))).toEqual(['Page 1', 'Page 2', 'Page 3']);
    expect(view.radios().map((r) => r.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    // The number is under each thumbnail; the picture and the number are hidden from a screen reader (the label says it).
    expect(view.radios().map((r) => r.querySelector('.pdf-number')?.textContent)).toEqual(['1', '2', '3']);
    expect(view.radios().every((r) => r.querySelector('.pdf-frame')?.getAttribute('aria-hidden') === 'true')).toBe(true);
    expect(view.radios().every((r) => r.querySelector('.pdf-number')?.getAttribute('aria-hidden') === 'true')).toBe(true);
    // A single tab stop: the chosen page.
    expect(view.radios().map((r) => r.tabIndex)).toEqual([0, -1, -1]);
    await vi.waitFor(() => expect(view.images().length).toBe(3));
    expect(view.images().every((img) => img.getAttribute('alt') === '')).toBe(true);
    // A short PDF has no number field.
    expect(view.target.querySelector('[data-slot="pdf-page-number"]')).toBeNull();
  });

  it('chooses a page by click', async () => {
    const view = open();
    view.radio(3).click();
    await tick();
    expect(view.chosen()).toBe('3');
    expect(view.radio(3).getAttribute('aria-checked')).toBe('true');
    expect(view.radio(1).getAttribute('aria-checked')).toBe('false');
    expect(view.radios().map((r) => r.tabIndex)).toEqual([-1, -1, 0]);
    expect(view.radio(3).classList.contains('pdf-picked')).toBe(true);
  });

  it('moves the choice with the arrows (a page, a row of two), Home and End, and keeps the focus on it', async () => {
    const view = open(pagesOf(7));
    view.key(1, 'ArrowRight');
    await vi.waitFor(() => expect(view.chosen()).toBe('2'));
    await vi.waitFor(() => expect(document.activeElement).toBe(view.radio(2)));
    view.key(2, 'ArrowDown');
    await vi.waitFor(() => expect(view.chosen()).toBe('4'));
    await vi.waitFor(() => expect(document.activeElement).toBe(view.radio(4)));
    view.key(4, 'ArrowLeft');
    await vi.waitFor(() => expect(view.chosen()).toBe('3'));
    view.key(3, 'ArrowUp');
    await vi.waitFor(() => expect(view.chosen()).toBe('1'));
    // Never beyond the first or the last page.
    view.key(1, 'ArrowUp');
    view.key(1, 'ArrowLeft');
    await tick();
    expect(view.chosen()).toBe('1');
    view.key(1, 'End');
    await vi.waitFor(() => expect(view.chosen()).toBe('7'));
    view.key(7, 'ArrowDown');
    view.key(7, 'ArrowRight');
    await tick();
    expect(view.chosen()).toBe('7');
    view.key(7, 'Home');
    await vi.waitFor(() => expect(view.chosen()).toBe('1'));
    // Other keys, and shortcuts with a modifier, are left alone.
    const other = new KeyboardEvent('keydown', { key: 'a', bubbles: true, cancelable: true });
    view.group().dispatchEvent(other);
    expect(other.defaultPrevented).toBe(false);
    const modified = new KeyboardEvent('keydown', { key: 'ArrowRight', ctrlKey: true, bubbles: true, cancelable: true });
    view.group().dispatchEvent(modified);
    expect(modified.defaultPrevented).toBe(false);
    expect(view.chosen()).toBe('1');
  });

  it('has real buttons, so that Enter and Space choose the page that has the focus', () => {
    const view = open();
    expect(view.radios().every((r) => r.tagName === 'BUTTON' && r.type === 'button')).toBe(true);
  });

  it('offers a number field for a long PDF, in step with the grid', async () => {
    const view = open(pagesOf(30));
    const field = view.target.querySelector<HTMLInputElement>('[data-slot="pdf-page-number"]')!;
    expect(field.value).toBe('1');
    expect(field.max).toBe('30');
    expect(view.target.textContent).toContain('sur 30');
    field.value = '12';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    expect(view.chosen()).toBe('12');
    expect(view.radio(12).getAttribute('aria-checked')).toBe('true');
    // A page that does not exist is ignored; leaving the field shows the choice again.
    field.value = '99';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    expect(view.chosen()).toBe('12');
    expect(field.value).toBe('12');
    // And the grid's choice shows in the field.
    view.radio(20).click();
    await vi.waitFor(() => expect(field.value).toBe('20'));
  });

  it('draws only the pages in view, two at a time, and keeps them', async () => {
    vi.stubGlobal('IntersectionObserver', FakeObserver);
    const resolvers = new Map<number, (url: string) => void>();
    let inFlight = 0;
    let mostInFlight = 0;
    const asked: number[] = [];
    vi.spyOn(api, 'renderPdfThumbnail').mockImplementation((_token, page) => {
      asked.push(page);
      inFlight += 1;
      mostInFlight = Math.max(mostInFlight, inFlight);
      return new Promise<string>((resolve) => {
        resolvers.set(page, (url) => {
          inFlight -= 1;
          resolve(url);
        });
      });
    });
    const view = open(pagesOf(200));
    // 200 cells at once, nothing asked while none is in view.
    expect(view.radios().length).toBe(200);
    await tick();
    expect(asked).toEqual([]);
    expect(FakeObserver.all[0]?.options?.root).toBe(view.group());

    reveal([1, 2, 3, 4, 5, 6]);
    // Two at a time, the lowest page first.
    await vi.waitFor(() => expect(asked).toEqual([1, 2]));
    expect(mostInFlight).toBe(2);
    resolvers.get(1)!('data:image/png;base64,AA==');
    await vi.waitFor(() => expect(asked).toEqual([1, 2, 3]));
    // Pages 4 and 5 scroll away before their turn: they are not drawn.
    reveal([3, 6]);
    resolvers.get(2)!('data:image/png;base64,AA==');
    await vi.waitFor(() => expect(asked).toEqual([1, 2, 3, 6]));
    resolvers.get(3)!('data:image/png;base64,AA==');
    resolvers.get(6)!('data:image/png;base64,AA==');
    await vi.waitFor(() => expect(view.images().length).toBe(4));
    expect(mostInFlight).toBe(2);
    // Back in view: the pictures already drawn are not asked for again, the two missing ones are.
    reveal([1, 2, 3, 4, 5, 6]);
    await vi.waitFor(() => expect(asked).toEqual([1, 2, 3, 6, 4, 5]));
    resolvers.get(4)!('data:image/png;base64,AA==');
    resolvers.get(5)!('data:image/png;base64,AA==');
    await vi.waitFor(() => expect(view.images().length).toBe(6));
    expect(asked.length).toBe(6);
    // Page 200 was never in view: never drawn.
    expect(asked).not.toContain(200);
  });

  it('does not keep every picture of a PDF scrolled through: the oldest out of view are forgotten', async () => {
    vi.stubGlobal('IntersectionObserver', FakeObserver);
    const render = vi.spyOn(api, 'renderPdfThumbnail');
    // The stand-in gives a PDF of 240 pages to a name ending in `-240p.pdf`.
    const view = open(pagesOf(240), 'C:/Documents/long-240p.pdf');
    await tick();
    // Scrolling through every page, six in view at a time.
    for (let first = 1; first <= 235; first += 6) {
      reveal([first, first + 1, first + 2, first + 3, first + 4, first + 5]);
      await vi.waitFor(() => expect(view.radio(first + 5).querySelector('img')).toBeTruthy());
    }
    expect(render).toHaveBeenCalledTimes(240);
    expect(view.images().length).toBeLessThanOrEqual(KEPT);
    expect(view.images().length).toBeGreaterThan(KEPT - 10);
    // The pages in view keep theirs; the first pages, long gone, are placeholders again...
    expect(view.radio(240).querySelector('img')).toBeTruthy();
    expect(view.radio(1).querySelector('img')).toBeNull();
    // ...and are drawn again when they come back.
    reveal([1, 2]);
    await vi.waitFor(() => expect(view.radio(1).querySelector('img')).toBeTruthy());
    expect(render).toHaveBeenCalledTimes(242);
  }, 30_000);

  it('asks for a thumbnail sized for the screen', async () => {
    vi.stubGlobal('devicePixelRatio', 1.5);
    const render = vi.spyOn(api, 'renderPdfThumbnail');
    open(pagesOf(2));
    await vi.waitFor(() => expect(render).toHaveBeenCalledTimes(2));
    expect(render.mock.calls[0][1]).toBe(1);
    expect(render.mock.calls[0][2]).toBe(192);
  });

  it('frees the PDF when the picker goes away', async () => {
    const view = open(pagesOf(3));
    await vi.waitFor(() => expect(view.images().length).toBe(3));
    expect(openPdfPreviews()).toBe(1);
    const close = vi.spyOn(api, 'closePdfPreview');
    cleanup?.();
    cleanup = null;
    await vi.waitFor(() => expect(openPdfPreviews()).toBe(0));
    expect(close).toHaveBeenCalledTimes(1);
  });

  it('frees the first PDF when another file is chosen, and starts again for the new one', async () => {
    const opening = vi.spyOn(api, 'openPdfPreview');
    const view = open(pagesOf(3), 'C:\\Documents\\un.pdf');
    await vi.waitFor(() => expect(view.images().length).toBe(3));
    view.radio(3).click();
    await tick();
    view.harness.setFile('C:\\Documents\\deux-5p.pdf', pagesOf(5));
    await vi.waitFor(() => expect(view.radios().length).toBe(5));
    await vi.waitFor(() => expect(view.images().length).toBe(5));
    expect(opening).toHaveBeenCalledTimes(2);
    expect(openPdfPreviews()).toBe(1);
    expect(view.chosen()).toBe('1');
  });

  it('closes a preview that opened after the picker was gone, and draws nothing for it', async () => {
    let opened: (token: number) => void = () => {};
    vi.spyOn(api, 'openPdfPreview').mockReturnValue(new Promise<number>((resolve) => (opened = resolve)));
    const close = vi.spyOn(api, 'closePdfPreview').mockResolvedValue(undefined);
    const render = vi.spyOn(api, 'renderPdfThumbnail');
    open(pagesOf(2));
    await tick();
    cleanup?.();
    cleanup = null;
    opened(42);
    await vi.waitFor(() => expect(close).toHaveBeenCalledWith(42));
    expect(render).not.toHaveBeenCalled();
  });

  it('drops a thumbnail that arrives after the picker was closed', async () => {
    let arrive: (url: string) => void = () => {};
    vi.spyOn(api, 'renderPdfThumbnail').mockReturnValue(new Promise<string>((resolve) => (arrive = resolve)));
    const view = open(pagesOf(1));
    await vi.waitFor(() => expect(openPdfPreviews()).toBe(1));
    const target = view.target;
    cleanup?.();
    cleanup = null;
    arrive('data:image/png;base64,AA==');
    await tick();
    expect(target.querySelector('img')).toBeNull();
    await vi.waitFor(() => expect(openPdfPreviews()).toBe(0));
  });

  it('shows a neutral placeholder for a page that cannot be drawn, which can still be chosen', async () => {
    vi.spyOn(api, 'renderPdfThumbnail').mockImplementation(async (_token, page) => {
      if (page === 2) throw { code: 'backgroundDamaged', message: 'broken' };
      return 'data:image/png;base64,AA==';
    });
    const view = open(pagesOf(3));
    await vi.waitFor(() => expect(view.radio(2).querySelector('.pdf-missing')).toBeTruthy());
    expect(view.radio(2).querySelector('.pdf-missing')?.textContent).toBe('Aperçu indisponible');
    expect(view.radio(2).querySelector('img')).toBeNull();
    expect(view.radio(2).querySelector('.pdf-number')?.textContent).toBe('2');
    await vi.waitFor(() => expect(view.radio(1).querySelector('img')).toBeTruthy());
    view.radio(2).click();
    await tick();
    expect(view.chosen()).toBe('2');
  });

  it('shows the placeholder everywhere when the preview cannot be opened, the pages still chooseable', async () => {
    vi.spyOn(api, 'openPdfPreview').mockRejectedValue({ code: 'pdfEncrypted', message: 'x' });
    const render = vi.spyOn(api, 'renderPdfThumbnail');
    const view = open(pagesOf(3));
    await vi.waitFor(() => expect(view.target.querySelectorAll('.pdf-missing').length).toBe(3));
    expect(render).not.toHaveBeenCalled();
    view.radio(3).click();
    await tick();
    expect(view.chosen()).toBe('3');
  });

  it('is in English', () => {
    setLocale('en');
    const view = open(pagesOf(30), 'C:\\Documents\\worksheet.pdf');
    expect(view.group().getAttribute('aria-label')).toBe('Pages of “fiche.pdf”');
    expect(view.radio(3).getAttribute('aria-label')).toBe('Page 3');
    expect(view.target.textContent).toContain('of 30');
    expect(view.target.querySelector('label')?.textContent).toContain('Page');
  });

  it('is disabled while the page is being placed', () => {
    const target = document.createElement('div');
    document.body.append(target);
    const harness = mount(PdfPagePickerHarness, { target, props: { path: 'C:\\a.pdf', pages: pagesOf(3), disabled: true } });
    expect(target.querySelectorAll<HTMLButtonElement>('[role="radio"]:disabled').length).toBe(3);
    unmount(harness);
    target.remove();
  });
});
