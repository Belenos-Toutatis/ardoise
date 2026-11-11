// @vitest-environment jsdom
// The « Vidéoprojecteur » block (M16, docs/design/side-panel.md, §8): the screen to use, project / stop, the refresh and
// what is projected now. It owns no state: the window gives it and does the acts, so the editor can use it from M17.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import type { MonitorInfo } from '../../lib/live';
import ProjectorBlock from './ProjectorBlock.svelte';

const screen = (n: number, primary = false): MonitorInfo => ({
  key: `\\\\.\\DISPLAY${n}@${n * 1920},0`,
  name: `\\\\.\\DISPLAY${n}`,
  x: n * 1920,
  y: 0,
  width: 1920,
  height: 1080,
  scale: 1,
  primary,
  chosen: false,
});
const ONE = screen(1, true);
const TWO = screen(2);

let cleanup: (() => void) | null = null;

beforeEach(() => setLocale('fr'));
afterEach(() => {
  cleanup?.();
  cleanup = null;
});

interface Init {
  monitors?: MonitorInfo[];
  monitorKey?: string;
  loaded?: boolean;
  presentation?: MonitorInfo | null;
  busy?: boolean;
  error?: string | null;
}

function render(init: Init = {}) {
  const target = document.createElement('div');
  document.body.append(target);
  const calls = { project: vi.fn(), stop: vi.fn(), refresh: vi.fn() };
  const state = { key: init.monitorKey ?? '' };
  const component = mount(ProjectorBlock, {
    target,
    props: {
      monitors: init.monitors ?? [ONE, TWO],
      get monitorKey() {
        return state.key;
      },
      set monitorKey(value: string) {
        state.key = value;
      },
      loaded: init.loaded ?? true,
      presentation: init.presentation ?? null,
      busy: init.busy ?? false,
      error: init.error ?? null,
      onproject: calls.project,
      onstop: calls.stop,
      onrefresh: calls.refresh,
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <E extends Element = HTMLElement>(selector: string) => target.querySelector<E>(selector);
  return { target, q, calls, state, select: () => q<HTMLSelectElement>('select') };
}

describe('the projector block', () => {
  it('is the « projector » section with its title', () => {
    const view = render();
    expect(view.q('section.pane-block[data-section="projector"]')).not.toBeNull();
    expect(view.q('h3.pane-title')!.textContent).toBe('Vidéoprojecteur');
    expect(view.q('section')!.getAttribute('aria-labelledby')).toBe(view.q('h3')!.id);
  });

  it('lists the screens by name and size, binds the choice, and offers « Projeter »', async () => {
    const view = render({ monitorKey: TWO.key });
    expect([...view.select()!.options].map((o) => o.textContent?.trim())).toEqual([
      'Écran 1 (principal) — 1920 × 1080',
      'Écran 2 — 1920 × 1080',
    ]);
    // Svelte selects the option once the list is in place.
    await vi.waitFor(() => expect(view.select()!.value).toBe(TWO.key));
    view.select()!.value = ONE.key;
    view.select()!.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    expect(view.state.key).toBe(ONE.key);
    view.q<HTMLButtonElement>('button[data-action="project"]')!.click();
    expect(view.calls.project).toHaveBeenCalledTimes(1);
    expect(view.q('button[data-action="stop-projecting"]')).toBeNull();
    expect(view.q('[data-presentation]')!.getAttribute('data-presentation')).toBe('');
    expect(view.q('[data-presentation]')!.textContent?.trim()).toBe("Rien n'est projeté.");
  });

  it('waits while the projection opens', () => {
    expect(render({ busy: true }).q<HTMLButtonElement>('button[data-action="project"]')!.disabled).toBe(true);
  });

  it('says what is projected and offers « Arrêter la projection » instead', () => {
    const view = render({ presentation: TWO });
    expect(view.q('button[data-action="project"]')).toBeNull();
    expect(view.q('[data-presentation]')!.getAttribute('data-presentation')).toBe(TWO.key);
    expect(view.q('[data-presentation]')!.textContent?.trim()).toBe('Projeté sur Écran 2.');
    view.q<HTMLButtonElement>('button[data-action="stop-projecting"]')!.click();
    expect(view.calls.stop).toHaveBeenCalledTimes(1);
  });

  it('refreshes the list from its button', () => {
    const view = render();
    view.q<HTMLButtonElement>('button[aria-label="Actualiser la liste des écrans"]')!.click();
    expect(view.calls.refresh).toHaveBeenCalledTimes(1);
  });

  it('says when there is a single screen or none, and nothing before the list was asked for', () => {
    expect(render({ monitors: [ONE] }).target.textContent).toContain('Un seul écran est détecté');
    cleanup?.();
    const none = render({ monitors: [] });
    expect(none.target.textContent).toContain("Aucun écran n'est détecté.");
    expect(none.select()).toBeNull();
    expect(none.q<HTMLButtonElement>('button[data-action="project"]')!.disabled).toBe(true);
    cleanup?.();
    const unknown = render({ monitors: [], loaded: false });
    expect(unknown.target.textContent).not.toContain("Aucun écran n'est détecté.");
    expect(unknown.select()!.disabled).toBe(true);
  });

  it('shows a failure as an alert', () => {
    const view = render({ error: 'Écran introuvable' });
    expect(view.q('[role="alert"]')!.textContent).toBe('Écran introuvable');
    cleanup?.();
    expect(render().q('[role="alert"]')).toBeNull();
  });

  it('speaks English', () => {
    setLocale('en');
    const view = render({ presentation: ONE });
    expect(view.q('h3')!.textContent).toBe('Projector');
    expect(view.q('[data-presentation]')!.textContent?.trim()).toBe('Projected on Screen 1 (main).');
    expect(view.q('button[data-action="stop-projecting"]')!.textContent?.trim()).toBe('Stop projecting');
  });
});
