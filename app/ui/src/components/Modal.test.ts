// @vitest-environment jsdom
// The modal dialog's focus on opening.
import { createRawSnippet, flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import Modal from './Modal.svelte';

let cleanup: (() => void) | null = null;

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
});

describe('Modal', () => {
  it('gives the focus to the button asked for without scrolling the dialog to it', () => {
    const focus = vi.spyOn(HTMLElement.prototype, 'focus');
    const target = document.createElement('div');
    document.body.append(target);
    const children = createRawSnippet(() => ({
      render: () => '<div><p>Content</p><button data-autofocus>Export</button></div>',
    }));
    const component = mount(Modal, { target, props: { title: 'Dialog', onclose: () => undefined, children } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    flushSync();
    const button = target.querySelector<HTMLElement>('[data-autofocus]')!;
    expect(document.activeElement).toBe(button);
    // A tall dialog must open on its title, not scrolled down to the button.
    const options = focus.mock.calls.filter((_, i) => focus.mock.contexts[i] === button).map(([init]) => init);
    expect(options).toEqual([{ preventScroll: true }]);
  });
});
