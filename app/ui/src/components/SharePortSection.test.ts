// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { api } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { resetMockShare } from '../lib/mock-share';
import { parseSharePort } from '../lib/share';
import SharePortSection from './SharePortSection.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockShare();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockShare();
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(SharePortSection, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const field = () => target.querySelector<HTMLInputElement>('input[data-setting="share-port"]')!;
  const save = () => target.querySelector<HTMLButtonElement>('button[data-action="share-port-save"]')!;
  const slot = (name: string) => target.querySelector<HTMLElement>(`[data-slot="${name}"]`);
  async function type(text: string) {
    const input = field();
    input.value = text;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  }
  async function submit() {
    // The form is submitted, not just the button clicked: an empty or invalid field must reach the check.
    target.querySelector('form')!.requestSubmit();
    await settle();
  }
  return { target, field, save, slot, type, submit };
}

describe('the port of the share, as a number', () => {
  it('accepts whole numbers from 1024 to 65535 only', () => {
    expect(parseSharePort('8765')).toBe(8765);
    expect(parseSharePort(' 1024 ')).toBe(1024);
    expect(parseSharePort('65535')).toBe(65535);
    for (const wrong of ['', '   ', '1023', '80', '65536', '99999', '123456', '-8765', '87.65', '8e3', 'huit', '+8765']) {
      expect(parseSharePort(wrong), wrong).toBeNull();
    }
  });
});

describe('settings: the port of the share', () => {
  it('shows the port this PC uses, with its help', async () => {
    const ui = await render();
    expect(ui.target.querySelector('h3')?.textContent).toBe('Partage sur le réseau');
    expect(ui.field().value).toBe('8765');
    expect(ui.field().type).toBe('number');
    expect(ui.target.textContent).toContain('Port du partage');
    expect(ui.target.textContent).toContain('Le pare-feu autorise le programme, pas le port');
    expect(ui.target.textContent).toContain('partage suivant');
    expect(ui.save().disabled).toBe(true);
    expect(ui.slot('own')).toBeNull();
  });

  it('saves a valid port through the api, and says so', async () => {
    const ui = await render();
    await ui.type('9000');
    expect(ui.save().disabled).toBe(false);
    await ui.submit();
    expect((await api.sharePort()).port).toBe(9000);
    expect(ui.field().value).toBe('9000');
    expect(ui.slot('saved')?.textContent).toBe('Port enregistré : 9000.');
    expect(ui.slot('invalid')).toBeNull();
    expect(ui.save().disabled).toBe(true);
    // Kept for this PC: the shared port is told to be unchanged.
    expect(ui.slot('own')?.textContent).toContain('gardent le port 8765');
  });

  it('refuses an invalid port in words, and saves nothing', async () => {
    const ui = await render();
    for (const wrong of ['80', '70000', '', '87.65', '-1']) {
      await ui.type(wrong);
      await ui.submit();
      const message = ui.slot('invalid');
      expect(message?.textContent, wrong).toBe('Le port doit être un nombre entier de 1024 à 65535.');
      expect(message?.getAttribute('role')).toBe('alert');
      expect(ui.field().getAttribute('aria-invalid')).toBe('true');
      expect(ui.slot('saved')).toBeNull();
      expect((await api.sharePort()).port).toBe(8765);
    }
    // Typing again takes the message away; a valid port then goes through.
    await ui.type('9001');
    expect(ui.slot('invalid')).toBeNull();
    await ui.submit();
    expect((await api.sharePort()).port).toBe(9001);
  });

  it('shows the port this PC already kept, and speaks English', async () => {
    await api.setSharePort(8766);
    const ui = await render('en');
    expect(ui.target.querySelector('h3')?.textContent).toBe('Sharing on the network');
    expect(ui.field().value).toBe('8766');
    expect(ui.target.textContent).toContain('Share port');
    expect(ui.slot('own')?.textContent).toContain('This port is kept for this PC');
    await ui.type('99999');
    await ui.submit();
    expect(ui.slot('invalid')?.textContent).toBe('The port must be a whole number from 1024 to 65535.');
  });

  it('has the api refuse a port out of range in words too', async () => {
    await expect(api.setSharePort(80)).rejects.toMatchObject({ code: 'sharePortInvalid' });
    await expect(api.setSharePort(65536)).rejects.toMatchObject({ code: 'sharePortInvalid' });
    expect((await api.setSharePort(1024)).port).toBe(1024);
  });
});
