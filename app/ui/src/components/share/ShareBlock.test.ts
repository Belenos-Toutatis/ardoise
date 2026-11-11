// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import { INACTIVE_SHARE } from '../../lib/share';
import { share } from '../../lib/share-state.svelte';
import ShareBlock from './ShareBlock.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  share.reset();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  share.reset();
});

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(ShareBlock, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const status = () => target.querySelector<HTMLElement>('[data-slot="share-status"]')!;
  return { target, status };
}

describe('ShareBlock', () => {
  it('is a block with its title, a status line and the unchanged share button', () => {
    const { target, status } = render();
    const block = target.querySelector('section.pane-block[data-section="share"]')!;
    expect(block.querySelector('h3.pane-title')!.textContent).toBe('Partage sur le réseau');
    expect(block.getAttribute('aria-labelledby')).toBe(block.querySelector('h3')!.id);
    expect(status().classList.contains('pane-help')).toBe(true);
    expect(status().textContent!.trim()).toBe('Un autre ordinateur du réseau affiche la page dans un navigateur.');
    expect(block.querySelectorAll('button[data-action="share"]')).toHaveLength(1);
    expect(block.querySelector('.share-indicator')).toBeNull();
  });

  it('says how many screens are connected while a share runs, and that it is paused', () => {
    const { target, status } = render();
    share.status = { ...INACTIVE_SHARE, active: true, viewers: 1 };
    flushSync();
    expect(status().textContent!.trim()).toBe('Partagé : 1 écran connecté.');
    expect(target.querySelector('.share-indicator')!.getAttribute('data-viewers')).toBe('1');
    share.status = { ...INACTIVE_SHARE, active: true, viewers: 3, paused: true };
    flushSync();
    expect(status().textContent!.replace(/\s+/g, ' ').trim()).toBe("Partagé : 3 écrans connectés. En pause : l'image partagée est figée");
    share.status = INACTIVE_SHARE;
    flushSync();
    expect(status().textContent!.trim()).toBe('Un autre ordinateur du réseau affiche la page dans un navigateur.');
  });

  it('speaks English too', () => {
    setLocale('en');
    const { status } = render();
    expect(status().textContent!.trim()).toBe('Another computer on the network shows the page in a browser.');
    share.status = { ...INACTIVE_SHARE, active: true, viewers: 2 };
    flushSync();
    expect(status().textContent!.trim()).toBe('Shared: 2 screens connected.');
  });
});

describe('ShareBlock: the block of the Projection tab', () => {
  it('says « paused » only while a share runs, and keeps the status line a help text', () => {
    const { status } = render();
    share.status = { ...INACTIVE_SHARE, active: false, paused: true };
    flushSync();
    expect(status().textContent!.trim()).toBe('Un autre ordinateur du réseau affiche la page dans un navigateur.');
    expect(status().classList.contains('pane-help')).toBe(true);
  });

  it('puts the title, the status and the button in that order, the title labelling the block', () => {
    const { target } = render();
    const block = target.querySelector('section.pane-block')!;
    expect([...block.children].map((el) => el.tagName.toLowerCase() + (el.getAttribute('data-slot') ? `[${el.getAttribute('data-slot')}]` : ''))).toEqual([
      'h3',
      'p[share-status]',
      'button',
    ]);
    expect(block.querySelector('h3')!.id).toBe('side-share-title');
    expect(block.getAttribute('aria-labelledby')).toBe('side-share-title');
    expect(block.querySelector('button')!.classList.contains('share-button')).toBe(true);
  });

  it('keeps the button the toolbars had: its name and hint, and the indicator while a share runs', () => {
    const { target } = render();
    const button = target.querySelector<HTMLButtonElement>('button[data-action="share"]')!;
    expect(button.textContent).toContain('Partager sur le réseau');
    expect(button.getAttribute('aria-haspopup')).toBe('dialog');
    expect(button.classList.contains('active')).toBe(false);
    share.status = { ...INACTIVE_SHARE, active: true, viewers: 2 };
    flushSync();
    expect(button.classList.contains('active')).toBe(true);
    expect(button.getAttribute('aria-label')).not.toBeNull();
  });
});
