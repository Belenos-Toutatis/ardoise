/**
 * Closing the window while something would be lost. A screen (the editor, the library) gives a guard
 * that says whether to hold the close back; in the desktop app it then asks the user with its own
 * dialog and closes the window itself (`destroy`), in a plain browser the browser asks
 * (`beforeunload`). Injected into the screens so that tests can stand in for the window.
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

/**
 * The guards of the desktop window, under one listener: a listener of `onCloseRequested` destroys the
 * window unless it prevents the close itself, so with one each, the first guard not to hold would close
 * the window under the guard that does.
 */
const guards = new Set<() => boolean>();
let listening: Promise<() => void> | null = null;

/** The first guard to hold asks the user; the others are not asked. */
function closeHeldBack(): boolean {
  return [...guards].some((guard) => guard());
}

/** The desktop window (Tauri): `onCloseRequested`, then `destroy` once the user has chosen. */
export function desktopWindowClose(): WindowClose {
  return {
    subscribe: (guard) => {
      guards.add(guard);
      listening ??= getCurrentWindow().onCloseRequested((event) => {
        if (closeHeldBack()) event.preventDefault();
      });
      return listening.then(() => () => {
        guards.delete(guard);
      });
    },
    destroy: () => getCurrentWindow().destroy(),
  };
}

/** Closes the desktop window as Alt + F4 does: the guards are asked first. */
export async function requestWindowClose(): Promise<void> {
  if (!inDesktopApp() || closeHeldBack()) return;
  await getCurrentWindow().destroy();
}

let quitQuestion: (() => void) | null = null;

/** While a screen is on display, Ctrl + Q asks its question first (live mode). Returns how to stop. */
export function askBeforeQuitting(ask: () => void): () => void {
  quitQuestion = ask;
  return () => {
    if (quitQuestion === ask) quitQuestion = null;
  };
}

/** Ctrl + Q. */
export function quit(): void {
  if (quitQuestion) quitQuestion();
  else void requestWindowClose();
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
