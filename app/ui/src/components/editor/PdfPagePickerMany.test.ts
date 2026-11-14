// @vitest-environment jsdom
// The page picker with `multiple` (« Nouvelle note depuis un PDF »): checkboxes, Shift + click for a run, the keyboard,
// and the same lazy thumbnails as the single choice.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { openPdfPreviews } from '../../lib/mock-background';
import PdfPagePickerManyHarness from './PdfPagePickerManyHarness.svelte';

const A4 = { width: 595, height: 842 };
const pagesOf = (count: number) => Array.from({ length: count }, () => ({ ...A4 }));

/** An IntersectionObserver the test drives: `reveal` says which pages are in view. */
class FakeObserver {
  static all: FakeObserver[] = [];
  private cells = new Set<Element>();
  constructor(private callback: IntersectionObserverCallback) {
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
  }
  reveal(pages: number[]) {
    const entries = [...this.cells].map(
      (target) => ({ target, isIntersecting: pages.includes(Number((target as HTMLElement).dataset.page)) }) as IntersectionObserverEntry,
    );
    this.callback(entries, this as unknown as IntersectionObserver);
  }
}

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

function open(count = 6, start: number[] = [], disabled = false) {
  const target = document.createElement('div');
  document.body.append(target);
  const harness = mount(PdfPagePickerManyHarness, { target, props: { path: 'C:\fiche.pdf', pages: pagesOf(count), start, disabled } });
  cleanup = () => {
    unmount(harness);
    target.remove();
  };
  const boxes = () => [...target.querySelectorAll<HTMLButtonElement>('[role="checkbox"]')];
  const box = (page: number) => boxes().find((b) => b.dataset.page === String(page))!;
  const picked = () => target.querySelector('[data-slot="picked"]')!.textContent;
  const told = () => target.querySelector('[data-slot="told"]')!.textContent;
  const click = async (page: number, shiftKey = false) => {
    box(page).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, shiftKey }));
    await tick();
  };
  const key = async (page: number, name: string, init: KeyboardEventInit = {}) => {
    box(page).focus();
    box(page).dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init }));
    await tick();
  };
  return { target, boxes, box, picked, told, click, key };
}

describe('the page picker with several pages', () => {
  it('is a group of labelled checkboxes, each page checked or not, with one tab stop', async () => {
    const view = open(3, [2]);
    expect(view.target.querySelector('[role="group"]')?.getAttribute('aria-label')).toBe('Pages de « fiche.pdf »');
    expect(view.boxes().map((b) => b.getAttribute('aria-label'))).toEqual(['Page 1', 'Page 2', 'Page 3']);
    expect(view.boxes().map((b) => b.getAttribute('aria-checked'))).toEqual(['false', 'true', 'false']);
    // A chosen page is ticked as well as framed (not by colour alone).
    expect(view.box(2).querySelector('.pdf-number')?.textContent).toBe('✓ 2');
    expect(view.box(1).querySelector('.pdf-number')?.textContent).toBe('1');
    expect(view.boxes().filter((b) => b.tabIndex === 0)).toHaveLength(1);
    // No number field for a long PDF here: the dialog has its own.
    expect(view.target.querySelector('[data-slot="pdf-page-number"]')).toBeNull();
    // The thumbnails are drawn all the same.
    await vi.waitFor(() => expect(view.target.querySelectorAll('img').length).toBe(3));
  });

  it('toggles a page by click and tells the parent the pages chosen, in order', async () => {
    const view = open(5);
    await view.click(4);
    await view.click(2);
    expect(view.picked()).toBe('2,4');
    expect(view.told()).toBe('4 | 2,4');
    await view.click(4);
    expect(view.picked()).toBe('2');
    expect(view.box(4).getAttribute('aria-checked')).toBe('false');
  });

  it('chooses a run with Shift + click, from the last page clicked, in the state of that page', async () => {
    const view = open(8);
    await view.click(2);
    await view.click(5, true);
    expect(view.picked()).toBe('2,3,4,5');
    // Backwards too, and added to what is there.
    await view.click(8);
    await view.click(7, true);
    expect(view.picked()).toBe('2,3,4,5,7,8');
    // The last page clicked is unchecked: the run is unchecked.
    await view.click(3);
    expect(view.box(3).getAttribute('aria-checked')).toBe('false');
    await view.click(5, true);
    expect(view.picked()).toBe('2,7,8');
    // Shift + click without any page clicked before is a plain click.
  });

  it('is usable with the keyboard: the arrows move the focus, Space chooses, Shift + arrow extends, Ctrl + A all', async () => {
    const view = open(7);
    await view.key(1, 'ArrowRight');
    await vi.waitFor(() => expect(document.activeElement).toBe(view.box(2)));
    // Moving the focus chooses nothing.
    expect(view.picked()).toBe('');
    await view.key(2, 'ArrowDown');
    await vi.waitFor(() => expect(document.activeElement).toBe(view.box(4)));
    // Space activates the focused button: a click (the browser's own; here the click it makes).
    await view.click(4);
    expect(view.picked()).toBe('4');
    await view.key(4, 'ArrowRight', { shiftKey: true });
    await vi.waitFor(() => expect(view.picked()).toBe('4,5'));
    await view.key(5, 'ArrowRight', { shiftKey: true });
    await vi.waitFor(() => expect(view.picked()).toBe('4,5,6'));
    await view.key(6, 'End');
    await vi.waitFor(() => expect(document.activeElement).toBe(view.box(7)));
    await view.key(7, 'Home');
    await vi.waitFor(() => expect(document.activeElement).toBe(view.box(1)));
    await view.key(1, 'a', { ctrlKey: true });
    expect(view.picked()).toBe('1,2,3,4,5,6,7');
    // One tab stop: the page that has the focus.
    expect(view.boxes().filter((b) => b.tabIndex === 0).map((b) => b.dataset.page)).toEqual(['1']);
  });

  it('draws only the pages in view, and frees the PDF when it goes away', async () => {
    const before = openPdfPreviews();
    const view = open(3);
    await vi.waitFor(() => expect(openPdfPreviews()).toBe(before + 1));
    cleanup?.();
    cleanup = null;
    await vi.waitFor(() => expect(openPdfPreviews()).toBe(before));
    expect(view.boxes()).toHaveLength(0);
  });

  it('draws only the pages in view for a PDF of hundreds of pages', async () => {
    vi.stubGlobal('IntersectionObserver', FakeObserver);
    const render = vi.spyOn(api, 'renderPdfThumbnail');
    const view = open(400);
    await tick();
    expect(view.boxes()).toHaveLength(400);
    // Nothing is in view yet: nothing is drawn.
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(render).not.toHaveBeenCalled();
    FakeObserver.all.forEach((o) => o.reveal([1, 2, 3]));
    await vi.waitFor(() => expect(view.target.querySelectorAll('img').length).toBeGreaterThan(0));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(render.mock.calls.map((call) => call[1]).sort()).toEqual([1, 2, 3]);
  });

  it('changes nothing when disabled', async () => {
    const view = open(3, [], true);
    expect(view.boxes().every((b) => b.disabled)).toBe(true);
  });
});
