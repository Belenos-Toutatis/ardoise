/**
 * Ctrl + P, which starts Ardoise's own printing (the open note in the editor, the selected notes or the shown binder in
 * the library: `docs/guide`, « Imprimer »), and Ctrl + Shift + P, which WebView2 would answer with the system's print
 * dialog: the same here. Never the web view's print (`lib/browser-guard.ts` and `webview_keys.rs` keep it off).
 */
import type { KeyInput } from './live-keys';

/** Whether the key press asks to print: Ctrl (or Meta) + P, with or without Shift; Alt is AltGr, which types characters. */
export function isPrintKey(e: KeyInput): boolean {
  return !!(e.ctrlKey || e.metaKey) && !e.altKey && e.key.length === 1 && e.key.toLowerCase() === 'p';
}
