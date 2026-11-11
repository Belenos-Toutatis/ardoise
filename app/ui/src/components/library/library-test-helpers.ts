/** Mounting the main window over the demo library, for the component tests of the library (M10). */
import { mount, tick, unmount } from 'svelte';
import { expect, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import Home from '../Home.svelte';

export const ROOT = 'C:\\demo\\Ardoise';

export const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: ROOT,
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

export type Ui = Awaited<ReturnType<typeof renderHome>>;

/** Mounts `Home` and waits for `expected` cards. Call `ui.destroy()` when done. */
export async function renderHome(locale: 'fr' | 'en' = 'fr', expected = 3, props: Partial<AppState> = {}) {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const onchange = vi.fn();
  const component = mount(Home, { target, props: { app: { ...app, ...props }, onchange } });
  const destroy = () => {
    unmount(component);
    target.remove();
  };
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const all = <T extends Element = HTMLElement>(selector: string) => [...target.querySelectorAll<T>(selector)];
  const titles = () => all('.note-card .card-title').map((el) => el.textContent?.trim());
  const card = (title: string) => {
    const found = all('.note-card').find((el) => el.querySelector('.card-title')?.textContent?.trim() === title);
    if (!found) throw new Error(`no card "${title}" among ${titles().join(', ')}`);
    return found;
  };
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      await Promise.resolve();
      await tick();
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
  };
  const click = async (el: Element | null, init: MouseEventInit = {}) => {
    if (!el) throw new Error('nothing to click');
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...init }));
    await settle();
  };
  const button = (text: string, within: ParentNode = target) => {
    const found = [...within.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}" among ${[...within.querySelectorAll('button')].map((b) => b.textContent?.trim()).join(' | ')}`);
    return found;
  };
  const byLabel = (label: string, within: ParentNode = target) => {
    const found = [...within.querySelectorAll<HTMLElement>('[aria-label]')].find((el) => el.getAttribute('aria-label') === label);
    if (!found) throw new Error(`no element labelled "${label}"`);
    return found;
  };
  const type = async (input: HTMLInputElement | null, value: string) => {
    if (!input) throw new Error('no input');
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  };
  const key = async (el: Element | null, name: string, init: KeyboardEventInit = {}) => {
    if (!el) throw new Error('no element for the key');
    el.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init }));
    await settle();
  };
  /** Ctrl+click on the cards of `titles`: they become the selection. */
  const select = async (...names: string[]) => {
    for (const name of names) await click(card(name).querySelector('.card-open'), { ctrlKey: true });
  };
  const text = () => target.textContent ?? '';
  await vi.waitFor(() => expect(q('.library-section .loading')).toBeNull());
  await vi.waitFor(() => expect(all('.note-card').length).toBe(expected));
  return { target, onchange, destroy, q, all, titles, card, settle, click, button, byLabel, type, key, select, text };
}
