/**
 * The check that keeps a window's shortcut table and its key mapping from disagreeing (docs/design/side-panel.md, §6,
 * "The sweep"): forward, every chord of the table must do what the table says in every context it is listed for;
 * backward, every key press the mapping answers (over a fixed set of keys and modifiers) must be in the table, in the
 * contexts where it acts. Written in English: the sentences are for developers, in a failing test.
 */
import type { KeyInput } from './live-keys';
import { chordEvents, chordMatches, keyPress, usKeyCode, type Shortcut, type ShortcutKey } from './shortcuts';

/** The keys the backward sweep presses: letters, digits, space, the signs of the zoom, arrows, paging, edit and function keys. */
export const SWEEP_KEYS: readonly string[] = [
  ...'abcdefghijklmnopqrstuvwxyz',
  ...'0123456789',
  ' ',
  '+',
  '-',
  '=',
  '−',
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'ArrowDown',
  'PageUp',
  'PageDown',
  'Home',
  'End',
  'Delete',
  'Backspace',
  'Escape',
  'Enter',
  'Tab',
  ...Array.from({ length: 12 }, (_, i) => `F${i + 1}`),
];

/** Ctrl + Alt and AltGr (Ctrl + Alt on Windows) are not swept: AltGr types characters. */
export const SWEEP_MODIFIERS: readonly { ctrl?: true; shift?: true; alt?: true }[] = [
  {},
  { shift: true },
  { ctrl: true },
  { ctrl: true, shift: true },
  { alt: true },
  { alt: true, shift: true },
];

/** A key press as a developer reads it: `Ctrl + Shift + Z`, `Space`. */
function describe(e: KeyInput): string {
  const key = e.key === ' ' ? 'Space' : e.key;
  return [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.code && e.code !== usKeyCode(e.key) ? `${key} (${e.code})` : key]
    .filter(Boolean)
    .join(' + ');
}

function inContext<C extends object>(key: ShortcutKey<C>, context: C): boolean {
  const values = context as Record<string, unknown>;
  return Object.entries(key.in ?? {}).every(([field, value]) => values[field] === value);
}

/** Every disagreement between `table` and `does` over `contexts`, as sentences; [] when they agree. */
export function sweepShortcuts<C extends object>(
  table: readonly Shortcut<C>[],
  contexts: readonly C[],
  does: (e: KeyInput, context: C) => string | null,
): string[] {
  const problems: string[] = [];

  // Forward: what the table lists is what the window does.
  for (const entry of table) {
    for (const key of entry.keys) {
      for (const context of contexts) {
        if (!inContext(key, context)) continue;
        for (const event of chordEvents(key.chord)) {
          const done = does(event, context);
          if (done !== key.does) {
            problems.push(
              `"${entry.id}": ${describe(event)} gives ${done === null ? 'nothing' : `"${done}"`} in ${JSON.stringify(context)}, the table says "${key.does}"`,
            );
          }
        }
      }
    }
  }

  // Backward: what the window does is in the table.
  for (const context of contexts) {
    for (const name of SWEEP_KEYS) {
      for (const mods of SWEEP_MODIFIERS) {
        const event = keyPress(name, undefined, mods);
        const done = does(event, context);
        if (done === null) continue;
        const listed = table.some((entry) =>
          entry.keys.some((key) => key.does === done && inContext(key, context) && chordMatches(key.chord, event)),
        );
        if (!listed) {
          problems.push(`${describe(event)} gives "${done}" in ${JSON.stringify(context)}, which no entry of the table lists`);
        }
      }
    }
  }
  return problems;
}
