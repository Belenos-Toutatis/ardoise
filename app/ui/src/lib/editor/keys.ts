/**
 * What a key press asks of the editor (milestone M16; docs/design/side-panel.md, §6, "Editor: `lib/editor/keys.ts`"):
 * the editor's key mapping, pure, the way `lib/live-keys.ts` is live mode's, so that the shortcut table
 * (`lib/editor/shortcuts.ts`) can be swept against it. The gates (dialogs, text fields, gestures under way) stay in
 * `Editor.svelte`, in their order.
 *
 * The mapping is today's `Editor.svelte` `onkeydown` without its gates, branch by branch (design §6): the editor
 * calls it first, applies its gates (no note, dialogs, text fields, a gesture under way) in their order, then acts on
 * the result.
 */
import { closeKey } from '../close-keys';
import type { KeyInput } from '../live-keys';
import type { EditorKeyContext } from '../shortcuts';

export type EditorKeyKind =
  | 'close'
  | 'save'
  | 'previousPage'
  | 'nextPage'
  | 'leaveReplay'
  | 'playPause'
  | 'copy'
  | 'cut'
  | 'paste'
  | 'undo'
  | 'redo'
  | 'selectAll'
  | 'fit'
  | 'fitWidth'
  | 'delete'
  | 'deselect'
  | 'panHold'
  | 'select'
  | 'pen'
  | 'eraser'
  | 'pan'
  | 'replay'
  | 'straighten';

export interface EditorKeyAction {
  kind: EditorKeyKind;
}

/** Ctrl + 2 (the page as wide as the editor), whatever the layout. */
export function isFitWidthKey(e: KeyInput): boolean {
  return e.key === '2' || e.code === 'Digit2' || e.code === 'Numpad2';
}

/** Ctrl + 0 (the whole page), whatever the layout: 0 is typed with Shift on a French keyboard. */
export function isFitKey(e: KeyInput): boolean {
  return e.key === '0' || e.code === 'Digit0' || e.code === 'Numpad0';
}

/**
 * What a key press asks of the editor; no gate (dialogs, fields, gestures): the editor applies them. The order of the
 * branches is the handler's: close, save, Alt + arrows, the replay's keys, the clipboard, the other Ctrl keys, the
 * plain keys. Ctrl + S and the Ctrl keys do not test Alt (as the handler never did).
 */
export function editorKeyAction(e: KeyInput, context: EditorKeyContext): EditorKeyAction | null {
  if (closeKey(e) === 'close') return { kind: 'close' };
  const mod = !!(e.ctrlKey || e.metaKey);
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (mod && key === 's') return { kind: 'save' };
  // Alt + ← / →: the previous / next page, in a replay too; not with other modifiers.
  if (e.altKey && !mod && !e.shiftKey && (key === 'ArrowLeft' || key === 'ArrowRight')) {
    return { kind: key === 'ArrowLeft' ? 'previousPage' : 'nextPage' };
  }
  if (context.replaying) {
    // R and Escape leave it, Space plays or pauses; nothing edits the note.
    if (mod) return isFitKey(e) ? { kind: 'fit' } : isFitWidthKey(e) ? { kind: 'fitWidth' } : null;
    if (e.altKey) return null;
    if (key === 'r' || key === 'Escape') return { kind: 'leaveReplay' };
    return key === ' ' ? { kind: 'playPause' } : null;
  }
  // Copy, cut and paste: the browser's clipboard events do the work.
  if (mod && !e.shiftKey && !e.altKey && (key === 'c' || key === 'x' || key === 'v')) {
    return { kind: key === 'c' ? 'copy' : key === 'x' ? 'cut' : 'paste' };
  }
  if (mod) {
    // Ctrl + 0 and Ctrl + 2 are typed with Shift on some layouts (French): the key code says it too.
    if (isFitKey(e)) return { kind: 'fit' };
    if (isFitWidthKey(e)) return { kind: 'fitWidth' };
    if (key === 'z') return { kind: e.shiftKey ? 'redo' : 'undo' };
    if (key === 'y') return { kind: 'redo' };
    return key === 'a' ? { kind: 'selectAll' } : null;
  }
  if (e.altKey) return null;
  switch (key) {
    case 'Delete':
    case 'Backspace':
      return { kind: 'delete' };
    case 'Escape':
      return { kind: 'deselect' };
    case ' ':
      return { kind: 'panHold' };
    case 'v':
      return { kind: 'select' };
    case 'p':
      return { kind: 'pen' };
    case 'e':
      return { kind: 'eraser' };
    case 'h':
      return { kind: 'pan' };
    case 'r':
      return { kind: 'replay' };
    case 'f':
      return { kind: 'straighten' };
    default:
      return null;
  }
}
