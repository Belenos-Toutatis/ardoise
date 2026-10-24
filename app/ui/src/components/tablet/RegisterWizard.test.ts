// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockTablet, mockTabletPatch, resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import RegisterWizard from './RegisterWizard.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  tablet.reset();
  mockTablet.inRange = true;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
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

/** Mounts the wizard over a tablet that is not registered. */
async function render(locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  await api.forgetTablet();
  tablet.start();
  await settle();
  const onclose = vi.fn();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(RegisterWizard, { target, props: { onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const step = () => q('.register-wizard [data-step]')?.getAttribute('data-step');
  const click = async (selector: string) => {
    const el = q<HTMLButtonElement>(selector);
    if (!el) throw new Error(`nothing matches ${selector}`);
    el.click();
    await settle();
  };
  return { target, q, step, click, onclose };
}

describe('registration wizard', () => {
  it('opens on the warning, and registers nothing before the user continues', async () => {
    const register = vi.spyOn(api, 'tabletRegister');
    const { target, q, step } = await render();
    expect(step()).toBe('warning');
    expect(target.textContent).toContain("ne fonctionnera plus avec l'application Wacom du téléphone ni avec Inkspace");
    expect(target.textContent).toContain('Coupez le Bluetooth de votre téléphone');
    expect(q('[data-action="continue"]')).not.toBeNull();
    await beat(10);
    expect(register).not.toHaveBeenCalled();
    expect(tablet.status?.connection).toBe('notRegistered');
  });

  it('has an accessible dialog with the focus on Continue', async () => {
    const { q } = await render();
    const dialog = q('[role="dialog"]')!;
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-labelledby')).toBeTruthy();
    expect(document.activeElement).toBe(q('[data-action="continue"]'));
  });

  it('follows the steps the service reports, with its animation, up to the result', async () => {
    const { q, step, click, onclose } = await render();
    const mode = () => q('.tablet-drawing')?.getAttribute('data-mode');

    await click('[data-action="continue"]');
    expect(step()).toBe('holdButton');
    expect(mode()).toBe('hold');
    expect(q('.register-wizard')!.textContent).toContain('Maintenez le bouton enfoncé environ 6 secondes');
    expect(q('.register-wizard')!.textContent).toContain('clignote en bleu');

    await beat(1);
    expect(step()).toBe('searching');
    expect(mode()).toBe('wait');
    await beat(1);
    expect(step()).toBe('connecting');
    expect(mode()).toBe('wait');
    await beat(1);
    expect(step()).toBe('pressButton');
    expect(mode()).toBe('press');
    expect(q('.register-wizard')!.textContent).toContain('Appuyez brièvement sur le bouton');

    await beat(1);
    expect(step()).toBe('done');
    // The service goes on to the next state without taking the result away.
    await beat(2);
    expect(tablet.status?.registerStep).toBeNull();
    expect(step()).toBe('done');
    const text = q('.register-wizard')!.textContent!;
    expect(text).toContain('Tablette enregistrée');
    expect(text).toContain('« Bamboo Slate » est prête');
    expect(text).toContain('87 %');
    expect(text).toContain('Bamboo Slate A4');
    expect(text).toContain('297 × 216 mm');

    expect(onclose).not.toHaveBeenCalled();
    await click('[data-action="finish"]');
    expect(onclose).toHaveBeenCalledOnce();
  });

  it('can be cancelled at every step, and tells the service when something is under way', async () => {
    const cancel = vi.spyOn(api, 'tabletCancel');

    // On the warning nothing runs: it only closes.
    const first = await render();
    await first.click('[data-action="cancel-register"]');
    expect(first.onclose).toHaveBeenCalledOnce();
    expect(cancel).not.toHaveBeenCalled();
    cleanup?.();
    tablet.reset();
    resetMockTablet();

    // Then at each step that follows.
    for (const wait of [0, 1, 1, 1]) {
      const { click, onclose } = await render();
      await click('[data-action="continue"]');
      await beat(wait);
      cancel.mockClear();
      await click('[data-action="cancel-register"]');
      expect(cancel).toHaveBeenCalledOnce();
      expect(onclose).toHaveBeenCalledOnce();
      expect(tablet.status?.connection).toBe('notRegistered');
      expect(tablet.status?.registerStep).toBeNull();
      cleanup?.();
      cleanup = null;
      tablet.reset();
      resetMockTablet();
      mockTablet.inRange = true;
    }
  });

  it('is cancelled by Escape like by its button', async () => {
    const cancel = vi.spyOn(api, 'tabletCancel');
    const { q, click, onclose } = await render();
    await click('[data-action="continue"]');
    q('.modal')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await settle();
    expect(cancel).toHaveBeenCalledOnce();
    expect(onclose).toHaveBeenCalledOnce();
  });

  it('shows what went wrong and a way forward, then tries again', async () => {
    mockTablet.failRegistration = 'tabletTimeout';
    const { q, step, click, onclose } = await render();
    await click('[data-action="continue"]');
    await beat(2);
    expect(step()).toBe('error');
    const error = q('[data-step="error"]')!;
    expect(error.getAttribute('role')).toBe('alert');
    expect(error.textContent).toContain("L'enregistrement n'a pas abouti");
    expect(error.textContent).toContain('La tablette ne répond pas');
    expect(error.textContent).toContain('maintenez le bouton jusqu');

    mockTablet.failRegistration = null;
    await click('[data-action="retry"]');
    expect(step()).toBe('holdButton');
    await beat(4);
    expect(step()).toBe('done');

    // And closing from an error needs no cancel.
    expect(onclose).not.toHaveBeenCalled();
  });

  it('shows the error of a refused request', async () => {
    vi.spyOn(api, 'tabletRegister').mockRejectedValue({ code: 'bluetoothOff', message: '' });
    const { q, step, click } = await render();
    await click('[data-action="continue"]');
    expect(step()).toBe('error');
    expect(q('[data-step="error"]')!.textContent).toContain('Le Bluetooth est désactivé');
  });

  it('closes from an error without cancelling', async () => {
    vi.spyOn(api, 'tabletRegister').mockRejectedValue({ code: 'tablet', message: 'x' });
    const cancel = vi.spyOn(api, 'tabletCancel');
    const { click, onclose } = await render();
    await click('[data-action="continue"]');
    await click('[data-action="cancel-register"]');
    expect(onclose).toHaveBeenCalledOnce();
    expect(cancel).not.toHaveBeenCalled();
  });

  it('presses on with the simulated button', async () => {
    mockTablet.autoPress = false;
    const { step, click } = await render();
    await click('[data-action="continue"]');
    await beat(10);
    expect(step()).toBe('pressButton');
    await api.simPressButton();
    await settle();
    expect(step()).toBe('done');
  });

  it('closes when the registration is cancelled from elsewhere', async () => {
    const { step, click, onclose } = await render();
    await click('[data-action="continue"]');
    await beat(1);
    expect(step()).toBe('searching');
    await api.tabletCancel();
    await settle();
    expect(onclose).toHaveBeenCalledOnce();
  });

  it('follows a registration that is already under way instead of restarting it', async () => {
    await api.forgetTablet();
    await api.tabletRegister();
    tablet.start();
    await beat(1);
    const register = vi.spyOn(api, 'tabletRegister');
    setLocale('fr');
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(RegisterWizard, { target, props: { onclose: vi.fn() } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await settle();
    expect(target.querySelector('[data-step]')?.getAttribute('data-step')).toBe('searching');
    expect(register).not.toHaveBeenCalled();
  });

  it('speaks English', async () => {
    const { target, q, click } = await render('en');
    expect(target.textContent).toContain('will no longer work with the Wacom phone app or with Inkspace');
    await click('[data-action="continue"]');
    expect(q('.register-wizard')!.textContent).toContain('Hold the button down for about 6 seconds');
    await beat(3);
    expect(q('.register-wizard')!.textContent).toContain('Press the button briefly');
    mockTabletPatch({ registerStep: 'done' });
    await settle();
    expect(q('.register-wizard')!.textContent).toContain('Tablet registered');
  });

  it('labels the drawing for screen readers', async () => {
    const { q, click } = await render();
    await click('[data-action="continue"]');
    expect(q('.tablet-drawing')!.getAttribute('role')).toBe('img');
    expect(q('.tablet-drawing')!.getAttribute('aria-label')).toBe('Schéma de la tablette et de son bouton');
  });
});
