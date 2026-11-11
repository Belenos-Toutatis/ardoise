// @vitest-environment jsdom
/**
 * The paper block (milestone M16): the paper's colour as swatches, its template, its texture; a custom paper shows a
 * fifth swatch that is pressed and cannot be chosen.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import { paperRgb } from '../../lib/live-render';
import type { Background } from '../../lib/note';
import PaperBlock from './PaperBlock.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => setLocale('fr'));
afterEach(() => {
  cleanup?.();
  cleanup = null;
});

const WHITE: Background = { paper: 'white', template: 'lined', texture: false };

function render(background: Background, disabled = false) {
  const target = document.createElement('div');
  document.body.append(target);
  const onbackground = vi.fn();
  const component = mount(PaperBlock, { target, props: { background, disabled, onbackground } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const swatch = (id: string) => target.querySelector<HTMLButtonElement>(`button[data-paper="${id}"]`)!;
  return { target, onbackground, swatch };
}

describe('paper block', () => {
  it('shows the four papers as swatches in their own colour, the current one pressed', () => {
    const { target, swatch } = render({ ...WHITE, paper: 'cream' });
    const ids = [...target.querySelectorAll<HTMLElement>('button[data-paper]')].map((b) => b.dataset.paper);
    expect(ids).toEqual(['white', 'cream', 'kraft', 'slate']);
    expect(swatch('cream').getAttribute('aria-pressed')).toBe('true');
    expect(swatch('white').getAttribute('aria-pressed')).toBe('false');
    expect(swatch('slate').textContent).toContain('Ardoise noire');
    const [r, g, b] = paperRgb('kraft');
    expect(swatch('kraft').getAttribute('style')).toContain(`rgb(${r} ${g} ${b})`);
    expect(target.querySelector('select[data-field="paper"]')).toBeNull();
  });

  it('chooses a paper, keeping the template and the texture', async () => {
    const { swatch, onbackground } = render({ ...WHITE, texture: true });
    swatch('kraft').click();
    await tick();
    expect(onbackground).toHaveBeenCalledWith({ paper: 'kraft', template: 'lined', texture: true });
  });

  it('chooses a template and switches the texture', async () => {
    const { target, onbackground } = render(WHITE);
    const template = target.querySelector<HTMLSelectElement>('select[data-field="template"]')!;
    expect(template.value).toBe('lined');
    template.value = 'seyes';
    template.dispatchEvent(new Event('change', { bubbles: true }));
    expect(onbackground).toHaveBeenLastCalledWith({ paper: 'white', template: 'seyes', texture: false });
    const texture = target.querySelector<HTMLInputElement>('input[data-field="texture"]')!;
    expect(texture.type).toBe('checkbox');
    expect(texture.getAttribute('role')).toBe('switch');
    texture.click();
    expect(onbackground).toHaveBeenLastCalledWith({ paper: 'white', template: 'lined', texture: true });
  });

  it('shows a custom paper as a fifth swatch, pressed and disabled', () => {
    const custom = { r: 200, g: 100, b: 50, a: 255 };
    const { target, swatch } = render({ ...WHITE, paper: { custom } });
    expect(target.querySelectorAll('button[data-paper]')).toHaveLength(5);
    expect(swatch('custom').getAttribute('aria-pressed')).toBe('true');
    expect(swatch('custom').disabled).toBe(true);
    expect(swatch('custom').textContent).toContain('Personnalisé');
    expect(swatch('custom').getAttribute('style')).toContain('rgb(200 100 50)');
    expect(swatch('white').disabled).toBe(false);
    expect(swatch('white').getAttribute('aria-pressed')).toBe('false');
  });

  it('locks every control when disabled', () => {
    const { target } = render(WHITE, true);
    const controls = [...target.querySelectorAll<HTMLElement>('button, select, input')] as HTMLButtonElement[];
    expect(controls).toHaveLength(6);
    expect(controls.every((c) => c.disabled)).toBe(true);
  });

  it('speaks English too', () => {
    setLocale('en');
    const { swatch } = render(WHITE);
    expect(swatch('slate').textContent).toContain('Black slate');
  });
});
