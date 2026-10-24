/**
 * Closing the window while a note has unsaved changes. The editor gives a guard that says
 * whether to hold the close back; in the desktop app it then asks the user with its own dialog
 * and closes the window itself (`destroy`), in a plain browser the browser asks
 * (`beforeunload`). Injected into the editor so that tests can stand in for the window.
 */
import { getCurrentWindow } from '@tauri-apps/api/window';
import { inDesktopApp } from '../api';

export interface WindowClose {
  /**
   * Calls `guard` whenever the user asks to close the window; when it returns true, the close
   * is held back. Resolves to the function that stops listening.
   */
  subscribe(guard: () => boolean): Promise<() => void>;
  /**
   * Closes the window for good, without asking again; null when the platform asks the user
   * itself (browser).
   */
  destroy: (() => Promise<void>) | null;
}

/** The desktop window (Tauri): `onCloseRequested`, then `destroy` once the user has chosen. */
export function desktopWindowClose(): WindowClose {
  return {
    subscribe: (guard) =>
      getCurrentWindow().onCloseRequested((event) => {
        if (guard()) event.preventDefault();
      }),
    destroy: () => getCurrentWindow().destroy(),
  };
}

/** A browser tab: `beforeunload` makes the browser ask before leaving. */
export function browserWindowClose(target: Window = window): WindowClose {
  return {
    subscribe: (guard) => {
      const listener = (event: BeforeUnloadEvent) => {
        if (!guard()) return;
        event.preventDefault();
        // Older engines need a return value to show their prompt.
        event.returnValue = '';
      };
      target.addEventListener('beforeunload', listener);
      return Promise.resolve(() => target.removeEventListener('beforeunload', listener));
    },
    destroy: null,
  };
}

/** The right one for where the interface runs (decided at each call, like `api`). */
export function defaultWindowClose(): WindowClose {
  return inDesktopApp() ? desktopWindowClose() : browserWindowClose();
}
