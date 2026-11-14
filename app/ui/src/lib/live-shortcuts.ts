/**
 * Live mode's keyboard shortcuts as data (milestone M16; docs/design/side-panel.md, §6, "The tables"): what the help
 * tab of the control window shows, and what `sweepShortcuts` confronts with the key mapping (`liveKeyAction` and
 * `liveEditKeyAction`, `lib/live-keys.ts`) in both directions. `does` is the word of the action: the touch-up keys
 * (`select`, `pen`, `deselect`, `deleteSelection`, `undo`, `redo`), the `command` of a `command` action (`nextPage`,
 * `previousPage`, `clearPage`), `zoomIn` / `zoomOut` for a `zoom`, `quit` for Ctrl + Q and `guide` for F1 (which
 * `App.svelte` answers), and otherwise the action's `kind` (`board`, `laser`, `wholePage`, `fitWidth`, `clearMarks`, `pan`, `leave`).
 * The live window's `does` for the sweep: the touch-up keys first, as `LiveView.onkeydown` does (while a session
 * runs), then `closeKey(e) === 'quit'`, then `isGuideKey(e)`, then `liveKeyAction`.
 *
 * Entries in the order of the help; a key with `shown: false` is a variant the mapping takes too (Shift + a letter,
 * `=` for +, the keypad…). The `presentation` group holds the keys of the projected window, which do nothing in the
 * control window (`clearMarks`), so that the help does not promise what its window does not do.
 *
 * `LiveView` gives this table to `ShortcutsHelp` (its Aide tab). `LiveView.shortcuts.test.ts` runs a probe per entry:
 * the shown chord must act in the mounted window.
 */
import type { Chord, LiveKeyContext, Shortcut, ShortcutKey } from './shortcuts';

/** The contexts the sweep runs the table in: a selection exists only while a session runs (`LiveView.svelte`). */
export const LIVE_KEY_CONTEXTS: readonly LiveKeyContext[] = [
  { running: false, selection: false },
  { running: true, selection: false },
  { running: true, selection: true },
];

type Key = ShortcutKey<LiveKeyContext>;
const running: Partial<LiveKeyContext> = { running: true };
const selecting: Partial<LiveKeyContext> = { running: true, selection: true };
const noSelection: Partial<LiveKeyContext> = { selection: false };

const label = (id: string) => `live.keys.list.${id}.action`;
/** A key the help shows; every context unless `context` says. */
const key = (chord: Chord, does: string, context?: Partial<LiveKeyContext>): Key => ({ chord, does, in: context });
/** A variant the mapping takes too, which the help leaves out. */
const variant = (chord: Chord, does: string, context?: Partial<LiveKeyContext>): Key => ({ chord, does, in: context, shown: false });

export const LIVE_SHORTCUTS: readonly Shortcut<LiveKeyContext>[] = [
  {
    id: 'next',
    label: label('next'),
    keys: [
      key({ key: 'ArrowRight' }, 'nextPage'),
      key({ key: 'PageDown' }, 'nextPage'),
      key({ key: ' ' }, 'nextPage'),
      variant({ key: 'PageDown', shift: true }, 'nextPage'),
      variant({ key: 'Spacebar' }, 'nextPage'),
    ],
  },
  {
    id: 'previous',
    label: label('previous'),
    keys: [
      key({ key: 'ArrowLeft' }, 'previousPage'),
      key({ key: 'PageUp' }, 'previousPage'),
      variant({ key: 'PageUp', shift: true }, 'previousPage'),
    ],
  },
  {
    id: 'clear',
    label: label('clear'),
    keys: [key({ key: 'Delete', ctrl: true }, 'clearPage'), variant({ key: 'Backspace', ctrl: true }, 'clearPage')],
  },
  { id: 'board', label: label('board'), keys: [key({ key: 'b' }, 'board'), variant({ key: 'b', shift: true }, 'board')] },
  { id: 'laser', label: label('laser'), keys: [key({ key: 'l' }, 'laser'), variant({ key: 'l', shift: true }, 'laser')] },
  {
    id: 'select',
    label: label('select'),
    keys: [key({ key: 'v' }, 'select', running), variant({ key: 'v', shift: true }, 'select', running)],
  },
  { id: 'pen', label: label('pen'), keys: [key({ key: 'p' }, 'pen', running), variant({ key: 'p', shift: true }, 'pen', running)] },
  {
    id: 'delete',
    label: label('delete'),
    keys: [key({ key: 'Delete' }, 'deleteSelection', selecting), key({ key: 'Backspace' }, 'deleteSelection', selecting)],
  },
  {
    id: 'undo',
    label: label('undo'),
    keys: [
      key({ key: 'z', ctrl: true }, 'undo', running),
      key({ key: 'y', ctrl: true }, 'redo', running),
      variant({ key: 'z', ctrl: true, shift: true }, 'redo', running),
    ],
  },
  {
    id: 'zoom',
    label: label('zoom'),
    keys: [
      key({ key: '+', shift: 'any' }, 'zoomIn'),
      key({ key: '-', shift: 'any' }, 'zoomOut'),
      variant({ key: '=', shift: 'any' }, 'zoomIn'),
      variant({ key: '−', shift: 'any' }, 'zoomOut'),
    ],
  },
  {
    id: 'whole',
    label: label('whole'),
    keys: [
      key({ key: '0', shift: 'any' }, 'wholePage'),
      // On a French keyboard 0 is Shift + à, and the key code says it too.
      variant({ key: 'à', code: 'Digit0', shift: 'any' }, 'wholePage'),
      variant({ key: '0', code: 'Numpad0', shift: 'any' }, 'wholePage'),
    ],
  },
  { id: 'width', label: label('width'), keys: [key({ key: 'w' }, 'fitWidth'), variant({ key: 'w', shift: true }, 'fitWidth')] },
  {
    id: 'pan',
    label: label('pan'),
    keysLabel: 'shortcuts.keys.shiftArrows',
    keys: [
      key({ key: 'ArrowLeft', shift: true }, 'pan'),
      key({ key: 'ArrowRight', shift: true }, 'pan'),
      key({ key: 'ArrowUp', shift: true }, 'pan'),
      key({ key: 'ArrowDown', shift: true }, 'pan'),
    ],
  },
  {
    id: 'leave',
    label: label('leave'),
    // Both show « Échap »: the help merges them into one.
    keys: [key({ key: 'Escape' }, 'deselect', selecting), key({ key: 'Escape' }, 'leave', noSelection)],
  },
  { id: 'close', label: label('close'), keys: [key({ key: 'w', ctrl: true }, 'leave'), key({ key: 'F4', ctrl: true }, 'leave')] },
  { id: 'guide', label: label('guide'), keys: [key({ key: 'F1' }, 'guide')] },
  { id: 'quit', label: label('quit'), keys: [key({ key: 'q', ctrl: true }, 'quit')] },
  {
    id: 'marks',
    label: label('marks'),
    group: 'presentation',
    keys: [key({ key: 'Delete' }, 'clearMarks', noSelection), key({ key: 'Backspace' }, 'clearMarks', noSelection)],
  },
];
