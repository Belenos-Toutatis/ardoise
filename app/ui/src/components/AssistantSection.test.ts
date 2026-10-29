// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setLocale, t } from '../lib/i18n.svelte';
import AssistantSection from './AssistantSection.svelte';

let cleanup: (() => void) | null = null;

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.unstubAllGlobals();
});

async function settle() {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(AssistantSection, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  return target;
}

const EXE = 'C:\\Outils\\Ardoise\\ardoise.exe';
const snippet = (root: HTMLElement, name: string) => root.querySelector(`[data-snippet="${name}"]`)?.textContent ?? '';

describe('Settings > Assistant (MCP)', () => {
  it('shows what to paste, built with the running executable', async () => {
    const root = await render('fr');
    expect(root.querySelector('h3')?.textContent).toBe('Assistant (MCP)');
    expect(snippet(root, 'claude-code')).toBe(`claude mcp add --scope user ardoise -- "${EXE}" --mcp`);
    expect(JSON.parse(snippet(root, 'claude-desktop'))).toEqual({
      mcpServers: { ardoise: { command: EXE, args: ['--mcp'] } },
    });
    expect(root.querySelector('[data-slot="assistant-privacy"]')?.textContent).toBe(t('assistant.privacy'));
    expect(root.textContent).toContain('docs/guide/assistant.fr.md');
  });

  it('read only adds --read-only to both', async () => {
    const root = await render('en');
    root.querySelector<HTMLInputElement>('[data-setting="assistant-read-only"]')!.click();
    await settle();
    expect(snippet(root, 'claude-code')).toMatch(/--mcp --read-only$/);
    expect(JSON.parse(snippet(root, 'claude-desktop')).mcpServers.ardoise.args).toEqual(['--mcp', '--read-only']);
    expect(root.textContent).toContain('docs/guide/assistant.en.md');
  });

  it('copies each text, and says when the clipboard refuses', async () => {
    const writeText = vi.fn(async (_text: string) => {});
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    const root = await render('fr');
    root.querySelector<HTMLButtonElement>('[data-action="copy-claude-desktop"]')!.click();
    await settle();
    expect(writeText).toHaveBeenCalledWith(snippet(root, 'claude-desktop'));
    expect(root.querySelector('[data-action="copy-claude-desktop"]')?.textContent?.trim()).toBe(t('assistant.copied'));

    writeText.mockRejectedValueOnce(new Error('denied'));
    root.querySelector<HTMLButtonElement>('[data-action="copy-claude-code"]')!.click();
    await settle();
    expect(root.querySelector('[role="alert"]')?.textContent).toBe(t('assistant.copyFailed'));
  });
});
