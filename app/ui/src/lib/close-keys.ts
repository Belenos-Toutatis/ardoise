/**
 * The keys of Windows that close things: Ctrl + W and Ctrl + F4 close what is open, Ctrl + Q quits
 * the app. Each screen decides what is open (`docs/design/interface.md`, "Closing and quitting"). They
 * work from a text field too, where they mean nothing else.
 */
import type { KeyInput } from './live-keys';

export type CloseKey = 'close' | 'quit';

/** What a key press asks to close, or null. A held key acts once. */
export function closeKey(e: KeyInput): CloseKey | null {
  if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || e.repeat) return null;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (key === 'w' || key === 'F4') return 'close';
  return key === 'q' ? 'quit' : null;
}
