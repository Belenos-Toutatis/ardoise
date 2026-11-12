// @vitest-environment jsdom
// The recording panel's replace question (milestone M17, O11; docs/design/paper-projection-narration.md §3.4): with a
// narration to replace, « Démarrer » asks first, and only « Remplacer » starts; without one it starts at once (the live
// view passes no `replaces`).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockNarration } from '../../lib/mock-narration';
import { monitorSettled } from './MicrophonePicker.svelte';
import RecordPanel from './RecordPanel.svelte';
import { NarrationRecorder } from './recorder.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockNarration();
  setLocale('fr');
});

afterEach(async () => {
  cleanup?.();
  cleanup = null;
  await monitorSettled();
});

function render(props: { replaces?: boolean; replacedMs?: number | null } = {}) {
  const target = document.createElement('div');
  document.body.append(target);
  const onstart = vi.fn();
  const component = mount(RecordPanel, {
    target,
    props: {
      recorder: new NarrationRecorder(),
      device: null,
      ondevice: vi.fn(),
      onstart,
      onstop: vi.fn(),
      oncancel: vi.fn(),
      open: true,
      openLabel: 'Enregistrer',
      ...props,
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const action = (name: string) => q<HTMLButtonElement>(`[data-action="${name}"]`);
  return { target, onstart, q, action };
}

describe('the replace question', () => {
  it('starts at once when there is nothing to replace', async () => {
    const { action, q, onstart } = render();
    action('narration-start')!.click();
    await tick();
    expect(onstart).toHaveBeenCalledTimes(1);
    expect(q('[data-slot="replace-question"]')).toBeNull();
  });

  it('asks before replacing, with the narration’s length, and starts only on « Remplacer »', async () => {
    const { action, q, onstart } = render({ replaces: true, replacedMs: 83_000 });
    expect(q('[data-slot="replaces"]')?.textContent).toBe('Un nouvel enregistrement remplace la narration actuelle.');
    action('narration-start')!.click();
    await tick();
    expect(onstart).not.toHaveBeenCalled();
    const question = q('[data-slot="replace-question"]')!;
    expect(question.getAttribute('role')).toBe('alert');
    expect(question.textContent).toContain('Cette note a déjà une narration (1:23). La remplacer par ce nouvel enregistrement ?');
    // The start button gives way to the two answers; the focus is on the one that goes on.
    expect(action('narration-start')).toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(action('narration-replace-yes')));
    expect(action('narration-replace-yes')!.textContent?.trim()).toBe('Remplacer');
    expect(action('narration-replace-no')!.textContent?.trim()).toBe("Garder l'ancienne");

    action('narration-replace-yes')!.click();
    await tick();
    expect(onstart).toHaveBeenCalledTimes(1);
    expect(q('[data-slot="replace-question"]')).toBeNull();
  });

  it('« Garder l’ancienne » starts nothing and gives « Démarrer » back', async () => {
    const { action, q, onstart } = render({ replaces: true, replacedMs: 2000 });
    action('narration-start')!.click();
    await tick();
    action('narration-replace-no')!.click();
    await tick();
    expect(onstart).not.toHaveBeenCalled();
    expect(q('[data-slot="replace-question"]')).toBeNull();
    expect(action('narration-start')).toBeTruthy();
    // Asked again the next time.
    action('narration-start')!.click();
    await tick();
    expect(q('[data-slot="replace-question"]')).toBeTruthy();
  });

  it('speaks English', async () => {
    setLocale('en');
    const { action, q } = render({ replaces: true, replacedMs: 5000 });
    action('narration-start')!.click();
    await tick();
    expect(q('[data-slot="replace-question"]')?.textContent).toContain(
      'This note already has a narration (0:05). Replace it with this new recording?',
    );
    expect(action('narration-replace-yes')!.textContent?.trim()).toBe('Replace');
    expect(action('narration-replace-no')!.textContent?.trim()).toBe('Keep the old one');
  });
});
