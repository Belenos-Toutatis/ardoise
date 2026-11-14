/**
 * The user guide's types and key (opening it: `guide.svelte.ts`; Rust side: `guide.rs`). F1 opens the guide in every
 * screen of the main window (`App.svelte` answers it, like Ctrl + Q), also from a text field, where it means nothing
 * else.
 */
import type { KeyInput } from './live-keys';

/** The user guide, or the guide of the assistant (MCP). */
export type GuidePage = 'guide' | 'assistant';
/** Where the page came from: the `guide` folder next to the executable (offline), or GitHub. */
export type GuideOpened = 'local' | 'web';

/** Whether a key press asks for the guide: F1 alone (with Ctrl, Alt or Shift it is another chord); a held key acts once. */
export function isGuideKey(e: KeyInput): boolean {
  return e.key === 'F1' && !(e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) && !e.repeat;
}
