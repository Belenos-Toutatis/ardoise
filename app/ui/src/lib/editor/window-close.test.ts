import { beforeEach, describe, expect, it, vi } from 'vitest';

/** The window's close request as Tauri delivers it: destroyed unless the handler prevents it. */
const tauri = vi.hoisted(() => {
  const state = {
    handler: null as ((event: { preventDefault(): void }) => void) | null,
    listeners: 0,
    destroyed: 0,
    /** Alt + F4: returns whether the window went away. */
    close() {
      let prevented = false;
      state.handler?.({
        preventDefault: () => {
          prevented = true;
        },
      });
      if (!prevented) state.destroyed += 1;
      return !prevented;
    },
  };
  return state;
});

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onCloseRequested: async (handler: (event: { preventDefault(): void }) => void) => {
      tauri.handler = handler;
      tauri.listeners += 1;
      return () => {};
    },
    destroy: async () => {
      tauri.destroyed += 1;
    },
  }),
}));

vi.mock('../api', () => ({ inDesktopApp: () => true }));

import { askBeforeQuitting, desktopWindowClose, quit, requestWindowClose } from './window-close';

describe('the guards of the desktop window', () => {
  beforeEach(() => {
    tauri.destroyed = 0;
  });

  it('one screen holding the close is enough, whatever the others say', async () => {
    const closer = desktopWindowClose();
    let held = true;
    const stops = await Promise.all([closer.subscribe(() => false), closer.subscribe(() => held)]);
    // One listener only: with one each, the first would destroy the window under the second.
    expect(tauri.listeners).toBe(1);
    expect(tauri.close()).toBe(false);
    held = false;
    expect(tauri.close()).toBe(true);
    stops.forEach((stop) => stop());
  });

  it('a guard gone says nothing any more', async () => {
    const stop = await desktopWindowClose().subscribe(() => true);
    expect(tauri.close()).toBe(false);
    stop();
    expect(tauri.close()).toBe(true);
  });

  it('the first guard to hold asks, and the others are not asked', async () => {
    const second = vi.fn(() => true);
    const closer = desktopWindowClose();
    const stops = await Promise.all([closer.subscribe(() => true), closer.subscribe(second)]);
    tauri.close();
    expect(second).not.toHaveBeenCalled();
    stops.forEach((stop) => stop());
  });

  it('Ctrl + Q goes through the guards', async () => {
    const stop = await desktopWindowClose().subscribe(() => true);
    await requestWindowClose();
    expect(tauri.destroyed).toBe(0);
    stop();
    await requestWindowClose();
    expect(tauri.destroyed).toBe(1);
  });

  it('a screen can ask its own question first, until it goes away', async () => {
    const ask = vi.fn();
    const stop = askBeforeQuitting(ask);
    quit();
    expect(ask).toHaveBeenCalledOnce();
    expect(tauri.destroyed).toBe(0);
    stop();
    quit();
    await vi.waitFor(() => expect(tauri.destroyed).toBe(1));
    expect(ask).toHaveBeenCalledOnce();
  });
});
