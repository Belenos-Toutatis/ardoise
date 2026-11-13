// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { setLocale } from '../lib/i18n.svelte';
import WidthControl from './WidthControl.svelte';

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
});

function render(value: number, extra: Record<string, unknown> = {}) {
  setLocale('fr');
  const target = document.createElement('div');
  document.body.append(target);
  const kept: number[] = [];
  const followed: number[] = [];
  const component = mount(WidthControl, {
    target,
    props: { value, label: 'Épaisseur', onchange: (w: number) => kept.push(w), oninput: (w: number) => followed.push(w), ...extra },
  });
  cleanup = () => {
    void unmount(component);
    target.remove();
  };
  const q = <T extends HTMLElement>(selector: string) => target.querySelector<T>(selector)!;
  return { kept, followed, q, typed: () => q<HTMLInputElement>('input.typed') };
}

async function type(field: HTMLInputElement, text: string) {
  field.value = text;
  field.dispatchEvent(new Event('change', { bubbles: true }));
  await tick();
}

describe('width control', () => {
  it('shows the width in millimetres and names its controls', () => {
    const ui = render(60);
    expect(ui.typed().value).toBe('0.60');
    expect(ui.typed().getAttribute('aria-label')).toBe('Épaisseur en millimètres');
    expect(ui.q('[data-step="down"]').getAttribute('aria-label')).toBe('Épaisseur : plus fin');
    expect(ui.q('[data-step="up"]').getAttribute('aria-label')).toBe('Épaisseur : plus épais');
    expect(ui.q('input[type="range"]').getAttribute('aria-label')).toBe('Épaisseur');
  });

  it('steps by 0.05 mm with the buttons, onto the steps when the value is between two', () => {
    const ui = render(60);
    ui.q('[data-step="up"]').click();
    ui.q('[data-step="down"]').click();
    expect(ui.kept).toEqual([65, 55]);
    cleanup?.();
    const odd = render(62);
    odd.q('[data-step="up"]').click();
    odd.q('[data-step="down"]').click();
    expect(odd.kept).toEqual([65, 60]);
  });

  it('stops at the ends', () => {
    const low = render(10);
    expect(low.q<HTMLButtonElement>('[data-step="down"]').disabled).toBe(true);
    cleanup?.();
    const high = render(1000);
    expect(high.q<HTMLButtonElement>('[data-step="up"]').disabled).toBe(true);
  });

  it('takes a typed value with a comma or a point, to the hundredth, within the limits', async () => {
    const ui = render(60);
    await type(ui.typed(), '0,37');
    await type(ui.typed(), '1.2');
    await type(ui.typed(), '50');
    await type(ui.typed(), '0');
    expect(ui.kept).toEqual([37, 120, 1000, 10]);
  });

  it('shows the width again when what is typed is not a number', async () => {
    const ui = render(60);
    await type(ui.typed(), 'abc');
    await type(ui.typed(), '');
    expect(ui.kept).toEqual([]);
    expect(ui.typed().value).toBe('0.60');
  });

  it('follows the slider while it moves and keeps the value when it is let go', () => {
    const ui = render(60);
    const slider = ui.q<HTMLInputElement>('input[type="range"]');
    slider.value = '300';
    slider.dispatchEvent(new Event('input', { bubbles: true }));
    slider.dispatchEvent(new Event('change', { bubbles: true }));
    expect(ui.followed).toEqual([300]);
    expect(ui.kept).toEqual([300]);
  });

  it('is disabled as a whole', () => {
    const ui = render(60, { disabled: true });
    expect(ui.typed().disabled).toBe(true);
    expect(ui.q<HTMLButtonElement>('[data-step="up"]').disabled).toBe(true);
    expect(ui.q<HTMLInputElement>('input[type="range"]').disabled).toBe(true);
  });
});
