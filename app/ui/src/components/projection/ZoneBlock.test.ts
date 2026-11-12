// @vitest-environment jsdom
// The « Zone projetée » block (M16, docs/design/side-panel.md, §8): the zoom as − / value / + (no slider), the two fits,
// what the window adds, and a hint. It owns no state: the window gives the zoom and does the acts.
import { createRawSnippet, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import ZoneBlock from './ZoneBlock.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => setLocale('fr'));
afterEach(() => {
  cleanup?.();
  cleanup = null;
});

interface Init {
  zoom?: number;
  min?: number;
  max?: number;
  hint?: string;
  children?: boolean;
  widthTitle?: string;
}

function render(init: Init = {}) {
  const target = document.createElement('div');
  document.body.append(target);
  const calls = { zoom: vi.fn(), whole: vi.fn(), width: vi.fn() };
  const component = mount(ZoneBlock, {
    target,
    props: {
      zoom: init.zoom ?? 1,
      min: init.min ?? 1,
      max: init.max ?? 4,
      onzoom: calls.zoom,
      onwhole: calls.whole,
      onwidth: calls.width,
      hint: init.hint,
      widthTitle: init.widthTitle,
      children: init.children ? createRawSnippet(() => ({ render: () => '<p data-slot="extra">extra</p>' })) : undefined,
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <E extends Element = HTMLElement>(selector: string) => target.querySelector<E>(selector);
  return { target, q, calls };
}

describe('the projected area block', () => {
  it('is the « zone » section with its title, the zoom and the two fits', () => {
    const view = render({ zoom: 1.25 });
    expect(view.q('section.pane-block[data-section="zone"]')).not.toBeNull();
    expect(view.q('h3.pane-title')!.textContent).toBe('Zone projetée');
    expect(view.q('section')!.getAttribute('aria-labelledby')).toBe(view.q('h3')!.id);
    expect(view.q('[data-zoom]')!.getAttribute('data-zoom')).toBe('1.25');
    expect(view.q('[data-zoom]')!.textContent?.trim()).toBe('× 1,3');
    expect(view.q('button[data-action="whole-page"]')!.textContent?.trim()).toBe('Page entière');
    expect(view.q('button[data-action="page-width"]')!.textContent?.trim()).toBe('Largeur');
    expect(view.q('button[data-action="page-width"]')!.title).toContain('(W)');
    expect(view.q('input[type="range"]')).toBeNull();
  });

  it('calls back for each button', () => {
    const view = render({ zoom: 2 });
    view.q<HTMLButtonElement>('button[aria-label="Agrandir"]')!.click();
    expect(view.calls.zoom).toHaveBeenLastCalledWith(1);
    view.q<HTMLButtonElement>('button[aria-label="Réduire"]')!.click();
    expect(view.calls.zoom).toHaveBeenLastCalledWith(-1);
    view.q<HTMLButtonElement>('button[data-action="whole-page"]')!.click();
    expect(view.calls.whole).toHaveBeenCalledTimes(1);
    view.q<HTMLButtonElement>('button[data-action="page-width"]')!.click();
    expect(view.calls.width).toHaveBeenCalledTimes(1);
  });

  it('disables − at the smallest zoom and + at the largest', () => {
    const small = render({ zoom: 1 });
    expect(small.q<HTMLButtonElement>('button[aria-label="Réduire"]')!.disabled).toBe(true);
    expect(small.q<HTMLButtonElement>('button[aria-label="Agrandir"]')!.disabled).toBe(false);
    cleanup?.();
    const large = render({ zoom: 4 });
    expect(large.q<HTMLButtonElement>('button[aria-label="Réduire"]')!.disabled).toBe(false);
    expect(large.q<HTMLButtonElement>('button[aria-label="Agrandir"]')!.disabled).toBe(true);
  });

  it("shows the window's rows after the fits and the hint last, and only what it is given", () => {
    const view = render({ children: true, hint: 'Faites glisser le rectangle bleu.' });
    const kinds = [...view.q('section')!.children].map((el) => (el.matches('[data-slot="extra"]') ? 'extra' : el.matches('p.pane-help') ? 'hint' : 'block'));
    expect(kinds.slice(-2)).toEqual(['extra', 'hint']);
    expect(view.q('p.pane-help')!.textContent).toBe('Faites glisser le rectangle bleu.');
    cleanup?.();
    const bare = render();
    expect(bare.q('p.pane-help')).toBeNull();
    expect(bare.q('[data-slot="extra"]')).toBeNull();
  });

  it('titles « Largeur » as the window says (M17, the editor), live’s title with its key by default', () => {
    const view = render({ widthTitle: 'Afficher la page sur toute la largeur du vidéoprojecteur' });
    expect(view.q('button[data-action="page-width"]')!.title).toBe('Afficher la page sur toute la largeur du vidéoprojecteur');
    cleanup?.();
    expect(render().q('button[data-action="page-width"]')!.title).toContain('(W)');
  });

  it('speaks English', () => {
    setLocale('en');
    const view = render({ zoom: 1.5 });
    expect(view.q('h3')!.textContent).toBe('Projected area');
    expect(view.q('[data-zoom]')!.textContent?.trim()).toBe('× 1.5');
    expect(view.q('button[aria-label="Zoom in"]')).not.toBeNull();
    expect(view.q('button[data-action="whole-page"]')!.textContent?.trim()).toBe('Whole page');
  });
});
