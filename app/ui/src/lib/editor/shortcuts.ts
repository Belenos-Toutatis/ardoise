/**
 * The editor's keyboard shortcuts as data (milestone M16; docs/design/side-panel.md, §6, "The tables"): what the help
 * tab shows, and what `sweepShortcuts` confronts with `editorKeyAction` (`lib/editor/keys.ts`) in both directions.
 * `does` is the kind of action the mapping gives (`EditorKeyKind`), or `quit` for Ctrl + Q, which `App.svelte`
 * answers. Entries in the order of the help; a key with `shown: false` is a variant the mapping takes too (Shift +
 * a letter, a layout's own key for 0 and 2…) and stays out of the help.
 *
 * The sweep that checks the table against the mapping is `shortcuts.test.ts`; the probes that check each shown
 * shortcut acts in the mounted editor are `Editor.shortcuts.test.ts`. The editor's `does` for the sweep:
 * `closeKey(e) === 'quit' ? 'quit' : editorKeyAction(e, context)?.kind ?? null`.
 */
import type { Chord, EditorKeyContext, Shortcut, ShortcutKey } from '../shortcuts';

/** The contexts the sweep runs the table in. */
export const EDITOR_KEY_CONTEXTS: readonly EditorKeyContext[] = [{ replaying: false }, { replaying: true }];

type Key = ShortcutKey<EditorKeyContext>;
const editing: Partial<EditorKeyContext> = { replaying: false };
const replaying: Partial<EditorKeyContext> = { replaying: true };

const label = (id: string) => `editor.keys.list.${id}`;
/** A key the help shows. */
const key = (chord: Chord, does: string, context?: Partial<EditorKeyContext>): Key => ({ chord, does, in: context });
/** A variant the mapping takes too, which the help leaves out. */
const variant = (chord: Chord, does: string, context?: Partial<EditorKeyContext>): Key => ({ chord, does, in: context, shown: false });

/** A tool key outside a replay: the letter, and Shift + the letter as a variant. */
function tool(id: string, letter: string, does: string): Shortcut<EditorKeyContext> {
  return {
    id,
    label: label(id),
    keys: [key({ key: letter }, does, editing), variant({ key: letter, shift: true }, does, editing)],
  };
}

export const EDITOR_SHORTCUTS: readonly Shortcut<EditorKeyContext>[] = [
  tool('select', 'v', 'select'),
  tool('pen', 'p', 'pen'),
  tool('eraser', 'e', 'eraser'),
  tool('pan', 'h', 'pan'),
  {
    id: 'panHold',
    label: label('panHold'),
    keys: [key({ key: ' ' }, 'panHold', editing), variant({ key: ' ', shift: true }, 'panHold', editing)],
  },
  tool('straighten', 'f', 'straighten'),
  tool('replay', 'r', 'replay'),
  { id: 'undo', label: label('undo'), keys: [key({ key: 'z', ctrl: true }, 'undo', editing)] },
  {
    id: 'redo',
    label: label('redo'),
    keys: [
      key({ key: 'y', ctrl: true }, 'redo', editing),
      key({ key: 'z', ctrl: true, shift: true }, 'redo', editing),
      variant({ key: 'y', ctrl: true, shift: true }, 'redo', editing),
    ],
  },
  {
    id: 'selectAll',
    label: label('selectAll'),
    keys: [key({ key: 'a', ctrl: true }, 'selectAll', editing), variant({ key: 'a', ctrl: true, shift: true }, 'selectAll', editing)],
  },
  {
    id: 'clipboard',
    label: label('clipboard'),
    keys: [
      key({ key: 'c', ctrl: true }, 'copy', editing),
      key({ key: 'x', ctrl: true }, 'cut', editing),
      key({ key: 'v', ctrl: true }, 'paste', editing),
    ],
  },
  {
    id: 'delete',
    label: label('delete'),
    keys: [
      key({ key: 'Delete' }, 'delete', editing),
      key({ key: 'Backspace' }, 'delete', editing),
      variant({ key: 'Delete', shift: true }, 'delete', editing),
      variant({ key: 'Backspace', shift: true }, 'delete', editing),
    ],
  },
  {
    id: 'deselect',
    label: label('deselect'),
    keys: [key({ key: 'Escape' }, 'deselect', editing), variant({ key: 'Escape', shift: true }, 'deselect', editing)],
  },
  {
    id: 'fit',
    label: label('fit'),
    keys: [
      key({ key: '0', ctrl: true }, 'fit'),
      variant({ key: '0', ctrl: true, shift: true }, 'fit'),
      // On a French keyboard 0 is Shift + à, and the key code says it too.
      variant({ key: 'à', code: 'Digit0', ctrl: true, shift: 'any' }, 'fit'),
      variant({ key: '0', code: 'Numpad0', ctrl: true, shift: 'any' }, 'fit'),
    ],
  },
  {
    id: 'fitWidth',
    label: label('fitWidth'),
    keys: [
      key({ key: '2', ctrl: true }, 'fitWidth'),
      variant({ key: '2', ctrl: true, shift: true }, 'fitWidth'),
      variant({ key: 'é', code: 'Digit2', ctrl: true, shift: 'any' }, 'fitWidth'),
      variant({ key: '2', code: 'Numpad2', ctrl: true, shift: 'any' }, 'fitWidth'),
    ],
  },
  {
    id: 'pages',
    label: label('pages'),
    keys: [key({ key: 'ArrowLeft', alt: true }, 'previousPage'), key({ key: 'ArrowRight', alt: true }, 'nextPage')],
  },
  {
    id: 'save',
    label: label('save'),
    keys: [key({ key: 's', ctrl: true }, 'save'), variant({ key: 's', ctrl: true, shift: true }, 'save')],
  },
  { id: 'close', label: label('close'), keys: [key({ key: 'w', ctrl: true }, 'close'), key({ key: 'F4', ctrl: true }, 'close')] },
  { id: 'quit', label: label('quit'), keys: [key({ key: 'q', ctrl: true }, 'quit')] },
  {
    id: 'leaveReplay',
    label: label('leaveReplay'),
    group: 'replay',
    keys: [
      key({ key: 'r' }, 'leaveReplay', replaying),
      key({ key: 'Escape' }, 'leaveReplay', replaying),
      variant({ key: 'r', shift: true }, 'leaveReplay', replaying),
      variant({ key: 'Escape', shift: true }, 'leaveReplay', replaying),
    ],
  },
  {
    id: 'playPause',
    label: label('playPause'),
    group: 'replay',
    keys: [key({ key: ' ' }, 'playPause', replaying), variant({ key: ' ', shift: true }, 'playPause', replaying)],
  },
];
