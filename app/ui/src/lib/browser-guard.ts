/**
 * What Ardoise takes from the web view it runs in: it is an application, not a web page. The window's keys of a
 * browser (reload, print, find bar, view source, downloads…) and its right-click menu (« Enregistrer l'image sous »,
 * « Outils supplémentaires »…) have no place in it.
 *
 * On Windows the shell turns WebView2's own accelerator keys off (`webview_keys.rs`); this guard does the same from the
 * page: `preventDefault` on those keys, in every window (`main.ts`: the app, the classroom's presentation, the editor's
 * projection), and on every right click except in a text field, where the system's cut, copy and paste menu is useful.
 * The guard only ever prevents a default: the page's own handlers still see the key (`Ctrl + P` is answered by the
 * screens, `Ctrl + S` by the editor). It never touches the editing keys (Ctrl + A, C, V, X, Z, Y), the keys the app
 * answers (Ctrl + W, Ctrl + F4, Ctrl + Q, Escape, Ctrl + 0, Ctrl + 2, Alt + arrows…) or the zoom keys.
 */
import type { KeyInput } from './live-keys';

/** Input types where a right click is the system's cut, copy and paste menu: the ones text is typed in. */
const NON_TEXT_INPUTS = new Set(['button', 'submit', 'reset', 'checkbox', 'radio', 'range', 'color', 'file', 'image']);

/** Whether `target` is a text field (an input, a text area, an editable element): its right click keeps the system's menu. */
export function isTextField(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target instanceof HTMLInputElement) return !NON_TEXT_INPUTS.has(target.type);
  if (target instanceof HTMLTextAreaElement) return true;
  return target instanceof HTMLElement && !!target.isContentEditable;
}

/** The keys of the developer tools (kept in a development build, where they are the developer's). */
function isDevtoolsKey(key: string, ctrl: boolean, shift: boolean, alt: boolean): boolean {
  if (alt) return false;
  if (key === 'F12') return !ctrl;
  return ctrl && shift && (key === 'i' || key === 'j' || key === 'c');
}

/**
 * Whether the key press is one that a browser answers with a feature of its own, which Ardoise neutralises: reload (F5,
 * Ctrl + R, Ctrl + Shift + R, Ctrl + F5), print (Ctrl + P, Ctrl + Shift + P), find (Ctrl + F, F3, Shift + F3, Ctrl + G,
 * Ctrl + Shift + G), caret browsing (F7), view source (Ctrl + U), downloads (Ctrl + J), history (Ctrl + H), open and save
 * a page (Ctrl + O, Ctrl + S), clear browsing data (Ctrl + Shift + Delete), and the developer tools (F12, Ctrl + Shift + I,
 * J, C: `devtools`). Ctrl + S is the editor's save, Ctrl + P Ardoise's print, Ctrl + F the library's search: the screens
 * answer them, the guard only keeps the browser's version from also happening.
 */
export function isBrowserKey(e: KeyInput, devtools = true): boolean {
  const ctrl = !!(e.ctrlKey || e.metaKey);
  const shift = !!e.shiftKey;
  const alt = !!e.altKey;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (devtools && isDevtoolsKey(key, ctrl, shift, alt)) return true;
  if (alt) return false;
  if (!ctrl) return !shift ? key === 'F5' || key === 'F3' || key === 'F7' : key === 'F3';
  if (key === 'F5') return true;
  if (shift) return ['r', 'p', 'g', 'Delete'].includes(key);
  return ['r', 'p', 'f', 'g', 'u', 'j', 'h', 'o', 's'].includes(key);
}

export interface GuardOptions {
  /** Neutralise the browser's keys (a production build; in development they are the developer's tools). */
  keys?: boolean;
  /** With `keys`, also the developer tools' keys (default: yes; a production build has no tools to open). */
  devtools?: boolean;
}

/**
 * Installs the guard on `target`: the keys, when asked, and the right click (always). Capture phase, so that no handler
 * can keep the guard from running; it only prevents defaults, which no handler of the app reads for these keys. Returns
 * what removes it.
 */
export function installBrowserGuard(target: Window = window, options: GuardOptions = {}): () => void {
  const { keys = true, devtools = true } = options;
  const onkeydown = (event: KeyboardEvent) => {
    if (isBrowserKey(event, devtools)) event.preventDefault();
  };
  // The text fields keep the system's menu; everywhere else a right click shows nothing unless the screen draws its own
  // (which prevents the default first or after: the order does not matter).
  const oncontextmenu = (event: MouseEvent) => {
    if (!isTextField(event.target)) event.preventDefault();
  };
  if (keys) target.addEventListener('keydown', onkeydown, true);
  target.addEventListener('contextmenu', oncontextmenu, true);
  return () => {
    target.removeEventListener('keydown', onkeydown, true);
    target.removeEventListener('contextmenu', oncontextmenu, true);
  };
}
