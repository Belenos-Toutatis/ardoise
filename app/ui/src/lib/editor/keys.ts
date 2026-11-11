/**
 * What a key press asks of the editor (milestone M16; docs/design/side-panel.md, §6, "Editor: `lib/editor/keys.ts`"):
 * the editor's key mapping, pure, the way `lib/live-keys.ts` is live mode's, so that the shortcut table
 * (`lib/editor/shortcuts.ts`) can be swept against it. The gates (dialogs, text fields, gestures under way) stay in
 * `Editor.svelte`, in their order.
 *
 * Package 0 commits the types and a stub: `editorKeyAction` returns null for every key until package B2 writes the
 * mapping (the design's §6 lists it branch by branch). Nothing calls it before the editor wires it.
 */
import type { EditorKeyContext } from '../shortcuts';
import type { KeyInput } from '../live-keys';

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

/** What a key press asks of the editor; no gate (dialogs, fields, gestures): the editor applies them. */
export function editorKeyAction(_e: KeyInput, _context: EditorKeyContext): EditorKeyAction | null {
  return null;
}
