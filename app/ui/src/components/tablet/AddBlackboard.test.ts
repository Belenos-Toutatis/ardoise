// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockTablet, mockTabletPatch, resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import AddBlackboard from './AddBlackboard.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  tablet.reset();
  // The Slate is out of range: nothing else connects while the dialog is open.
  mockTablet.inRange = false;
});

function dispose() {
  cleanup?.();
  cleanup = null;
}

afterEach(() => {
  dispose();
  vi.restoreAllMocks();
  resetMockTablet();
  vi.useRealTimers();
});

async function settle() {
  for (let i = 0; i < 3; i++) {
    await vi.advanceTimersByTimeAsync(0);
    await tick();
  }
}

const beat = async (beats = 1) => {
  await vi.advanceTimersByTimeAsync(Math.round(beats * mockTablet.stepMs));
  await settle();
};

async function render(locale: 'fr' | 'en' = 'fr', { forgetSlate = false } = {}) {
  setLocale(locale);
  if (forgetSlate) await api.forgetTablet();
  tablet.start();
  await settle();
  const onclose = vi.fn();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(AddBlackboard, { target, props: { onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const step = () => q('.add-blackboard [data-step]')?.getAttribute('data-step');
  const click = async (selector: string) => {
    const el = q<HTMLButtonElement>(selector);
    if (!el) throw new Error(`nothing matches ${selector}`);
    el.click();
    await settle();
  };
  return { target, q, step, click, onclose };
}

describe('adding the Xiaomi tablet', () => {
  it('opens on what to check, and searches nothing before the user goes on', async () => {
    const find = vi.spyOn(api, 'tabletFindBlackboards');
    const { target, q, step } = await render();
    expect(step()).toBe('intro');
    const text = target.textContent!;
    expect(text).toContain('Allumez la tablette Xiaomi');
    expect(text).toContain('Fermez Mi Home');
    expect(text).toContain("qu'à une application à la fois");
    expect(text).toContain('reste liée à Mi Home');
    // No registration: nothing about holding a button, and no drawing of the Slate.
    expect(text).not.toContain('Maintenez');
    expect(q('.tablet-drawing')).toBeNull();
    expect(document.activeElement).toBe(q('[data-action="search"]'));
    await beat(5);
    expect(find).not.toHaveBeenCalled();
    expect(tablet.status?.connection).not.toBe('adding');
  });

  it('searches, shows the tablet heard, adds it and says it is linked', async () => {
    const { target, q, step, click, onclose } = await render();
    await click('[data-action="search"]');
    expect(step()).toBe('searching');
    expect(tablet.status?.connection).toBe('adding');
    await beat(1);
    expect(step()).toBe('found');
    const items = [...target.querySelectorAll('[data-slot="found"] li')];
    expect(items).toHaveLength(1);
    expect(items[0].textContent).toContain('xiaomi.dev.ble');
    expect(items[0].textContent).toContain('00:00:5E:00:53:01');
    // One tablet: its signal says nothing useful.
    expect(q('[data-slot="signal"]')).toBeNull();
    expect(target.textContent).toContain('Une tablette Xiaomi a été trouvée');

    await click('[data-action="add-blackboard"]');
    expect(step()).toBe('added');
    expect(tablet.status?.known.map((k) => k.kind)).toEqual(['slate', 'blackboard']);
    expect(target.textContent).toContain('Tablette Xiaomi ajoutée');
    expect(target.textContent).toContain('chacune est effacée de la tablette');
    expect(onclose).not.toHaveBeenCalled();
    await click('[data-action="finish"]');
    expect(onclose).toHaveBeenCalledOnce();
  });

  it('shows the signal and asks to choose when several are heard', async () => {
    mockTablet.boardsHeard = 2;
    const { target, q, step, click } = await render();
    await click('[data-action="search"]');
    await beat(2);
    expect(step()).toBe('found');
    expect(target.textContent).toContain('2 tablettes Xiaomi ont été trouvées');
    const signals = [...target.querySelectorAll('[data-slot="signal"]')].map((el) => el.textContent?.trim());
    expect(signals).toEqual(['Signal : -52 dBm', 'Signal : -66 dBm']);
    const second = q<HTMLButtonElement>('[data-action="add-blackboard"][data-address="00:00:5E:00:53:02"]')!;
    second.click();
    await settle();
    expect(step()).toBe('added');
    expect(tablet.board?.address).toBe('00:00:5E:00:53:02');
  });

  it('says what to check when nothing is heard, and tries again', async () => {
    mockTablet.boardsHeard = 0;
    const { q, step, click, onclose } = await render();
    await click('[data-action="search"]');
    await beat(61);
    expect(step()).toBe('error');
    const text = q('[data-step="error"]')!.textContent!;
    expect(q('[data-step="error"]')!.getAttribute('role')).toBe('alert');
    expect(text).toContain("La tablette Xiaomi n'a pas été ajoutée");
    expect(text).toContain('Aucune tablette Xiaomi trouvée');
    expect(text).toContain('allumez-la');
    expect(text).toContain('fermez Mi Home');

    mockTablet.boardsHeard = 1;
    await click('[data-action="retry"]');
    expect(step()).toBe('searching');
    await beat(1);
    expect(step()).toBe('found');
    // And closing from an error needs no cancel.
    expect(onclose).not.toHaveBeenCalled();
  });

  it('leaves « searching » for an error that repeats the one shown before the search', async () => {
    const error = { code: 'blackboardNotFound', message: '' };
    mockTabletPatch({ error });
    // The request is accepted, but the search ends at once with the same error (no status ever said « adding »).
    vi.spyOn(api, 'tabletFindBlackboards').mockResolvedValue(undefined);
    const { q, step, click } = await render();
    await click('[data-action="search"]');
    expect(step()).toBe('searching');
    mockTabletPatch({ error: { ...error } });
    await settle();
    expect(step()).toBe('error');
    expect(q('[data-step="error"]')!.textContent).toContain('Aucune tablette Xiaomi trouvée');
  });

  it('can be cancelled at every step, and stops the search', async () => {
    const cancel = vi.spyOn(api, 'tabletCancel');
    const first = await render();
    await first.click('[data-action="cancel-add"]');
    expect(first.onclose).toHaveBeenCalledOnce();
    expect(cancel).not.toHaveBeenCalled();
    dispose();
    tablet.reset();
    resetMockTablet();
    mockTablet.inRange = false;

    for (const wait of [0, 1]) {
      const { click, onclose } = await render();
      await click('[data-action="search"]');
      await beat(wait);
      cancel.mockClear();
      await click('[data-action="cancel-add"]');
      expect(cancel).toHaveBeenCalledOnce();
      expect(onclose).toHaveBeenCalledOnce();
      expect(tablet.status?.connection).toBe('searching');
      expect(tablet.status?.boardSearch).toBeNull();
      expect(tablet.status?.error).toBeNull();
      expect(tablet.board).toBeNull();
      dispose();
      tablet.reset();
      resetMockTablet();
      mockTablet.inRange = false;
    }
  });

  it('is cancelled by Escape like by its button', async () => {
    const cancel = vi.spyOn(api, 'tabletCancel');
    const { q, click, onclose } = await render();
    await click('[data-action="search"]');
    q('.modal')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await settle();
    expect(cancel).toHaveBeenCalledOnce();
    expect(onclose).toHaveBeenCalledOnce();
  });

  it('shows the error of a refused request (a sync under way)', async () => {
    vi.spyOn(api, 'tabletFindBlackboards').mockRejectedValue({ code: 'tabletBusy', message: '' });
    const { q, step, click } = await render();
    await click('[data-action="search"]');
    expect(step()).toBe('error');
    expect(q('[data-step="error"]')!.textContent).toContain('La tablette est occupée');
  });

  it('works when the Slate is not registered either (the blackboard alone)', async () => {
    const { step, click } = await render('fr', { forgetSlate: true });
    expect(tablet.status?.connection).toBe('notRegistered');
    await click('[data-action="search"]');
    await beat(1);
    await click('[data-action="add-blackboard"]');
    expect(step()).toBe('added');
    expect(tablet.status?.registered).toBeNull();
    expect(tablet.known.map((k) => k.kind)).toEqual(['blackboard']);
  });

  it('follows a search that is already under way instead of restarting it', async () => {
    await api.tabletFindBlackboards();
    tablet.start();
    await beat(1);
    const find = vi.spyOn(api, 'tabletFindBlackboards');
    const { step } = await render();
    expect(step()).toBe('found');
    expect(find).not.toHaveBeenCalled();
  });

  it('speaks English', async () => {
    const { target, q, click } = await render('en');
    expect(target.textContent).toContain('Switch on the Xiaomi tablet');
    expect(target.textContent).toContain('Close Mi Home');
    await click('[data-action="search"]');
    await beat(1);
    expect(target.textContent).toContain('A Xiaomi tablet was found');
    await click('[data-action="add-blackboard"]');
    expect(q('[data-step="added"]')!.textContent).toContain('Xiaomi tablet added');
  });
});
