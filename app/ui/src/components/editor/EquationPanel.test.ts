// @vitest-environment jsdom
/**
 * The equation panel's own markup (audit R10-4, R10-7): its status line is not the editor's `.status` (app.css keeps
 * that one on a single line), and the preview knows when the paper under it is dark.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import EquationPanel from './EquationPanel.svelte';

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
});

async function render(extra: { darkPaper?: boolean } = {}) {
  setLocale('fr');
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(EquationPanel, {
    target,
    props: {
      color: { r: 0, g: 0, b: 0, a: 255 },
      paper: 'rgb(42, 50, 48)',
      left: 0,
      top: 0,
      width: 300,
      onplace: vi.fn(),
      oncancel: vi.fn(),
      ...extra,
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await tick();
  return target;
}

describe('the equation panel', () => {
  it('does not wear the editor’s single-line `.status` class, so an error wraps in the panel', async () => {
    const target = await render();
    const status = target.querySelector('[data-slot="equation-status"]')!;
    expect(status.classList.contains('status')).toBe(false);
  });

  it('marks its preview when the paper is dark, for the light waiting text', async () => {
    const light = await render();
    expect(light.querySelector('[data-slot="equation-preview"]')!.classList.contains('dark')).toBe(false);
    cleanup?.();
    const dark = await render({ darkPaper: true });
    expect(dark.querySelector('[data-slot="equation-preview"]')!.classList.contains('dark')).toBe(true);
  });
});
