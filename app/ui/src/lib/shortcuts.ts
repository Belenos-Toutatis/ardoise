/**
 * The keyboard shortcuts as data (milestone M16; docs/design/side-panel.md, §6 "The help tab and the keys"): one
 * declarative table per window (`lib/editor/shortcuts.ts`, `lib/live-shortcuts.ts`), which the help tab renders and
 * which `sweepShortcuts` (`lib/shortcuts-sweep.ts`) confronts, in both directions, with what the window's key mapping
 * really does. Not a dispatcher: the mappings keep their own code.
 */
import { t } from './i18n.svelte';
import type { KeyInput } from './live-keys';

/** A key press as the tables write it. `shift: 'any'` accepts it with and without Shift. */
export interface Chord {
  key: string;
  code?: string;
  ctrl?: boolean;
  shift?: boolean | 'any';
  alt?: boolean;
}

export interface ShortcutKey<C> {
  chord: Chord;
  /** What the window's mapping gives for it (the `does` words of the tables). */
  does: string;
  /** The contexts where it does that (each field given must match); absent: every context. */
  in?: Partial<C>;
  /** Shown in the help (default true); false: a variant the mapping takes too (Shift + V, `=` for +…). */
  shown?: boolean;
}

export interface Shortcut<C> {
  id: string;
  /** i18n key of what it does. */
  label: string;
  keys: readonly ShortcutKey<C>[];
  /** i18n key shown instead of the chords (« Maj + flèches »). */
  keysLabel?: string;
  /** Help group; absent: the window's main list. */
  group?: 'replay' | 'presentation';
}

export interface EditorKeyContext {
  replaying: boolean;
}
export interface LiveKeyContext {
  running: boolean;
  selection: boolean;
}

/** A single letter is compared without case; every other key name as it is. */
function folded(key: string): string {
  return key.length === 1 ? key.toLowerCase() : key;
}

/** Whether `e` is the chord (single letters compared without case; `code` compared when given; Ctrl = Ctrl or Meta). */
export function chordMatches(chord: Chord, e: KeyInput): boolean {
  if (folded(chord.key) !== folded(e.key)) return false;
  if (chord.code !== undefined && chord.code !== e.code) return false;
  if (!!chord.ctrl !== !!(e.ctrlKey || e.metaKey)) return false;
  if (!!chord.alt !== !!e.altKey) return false;
  return chord.shift === 'any' || !!chord.shift === !!e.shiftKey;
}

/** The key code of `key` on a US keyboard (`KeyV`, `Digit0`, `Space`, `Equal`, `Minus`, `ArrowLeft`…), when it has one. */
export function usKeyCode(key: string): string | undefined {
  if (/^[a-z]$/i.test(key)) return `Key${key.toUpperCase()}`;
  if (/^[0-9]$/.test(key)) return `Digit${key}`;
  if (key === ' ' || key === 'Spacebar') return 'Space';
  if (key === '=' || key === '+') return 'Equal';
  if (key === '-' || key === '−') return 'Minus';
  // The named keys (arrows, Page Up, Delete, F4…) have the key's own name as their code.
  return key.length > 1 ? key : undefined;
}

/** A key press with the given modifiers: a letter is upper-case with Shift; `code` defaults to the US one. */
export function keyPress(key: string, code: string | undefined, mods: { ctrl?: boolean; shift?: boolean; alt?: boolean }): KeyInput {
  const letter = /^[a-z]$/i.test(key);
  return {
    key: letter ? (mods.shift ? key.toUpperCase() : key.toLowerCase()) : key,
    code: code ?? usKeyCode(key),
    ctrlKey: !!mods.ctrl,
    shiftKey: !!mods.shift,
    altKey: !!mods.alt,
    repeat: false,
    target: null,
  };
}

/** The concrete key presses of a chord (two with `shift: 'any'`), with the US key code. */
export function chordEvents(chord: Chord): KeyInput[] {
  const shifts = chord.shift === 'any' ? [false, true] : [chord.shift === true];
  return shifts.map((shift) => keyPress(chord.key, chord.code, { ctrl: chord.ctrl, shift, alt: chord.alt }));
}

const KEY_NAMES: Record<string, string> = {
  ArrowLeft: 'shortcuts.keys.arrowLeft',
  ArrowRight: 'shortcuts.keys.arrowRight',
  ArrowUp: 'shortcuts.keys.arrowUp',
  ArrowDown: 'shortcuts.keys.arrowDown',
  PageUp: 'shortcuts.keys.pageUp',
  PageDown: 'shortcuts.keys.pageDown',
  ' ': 'shortcuts.keys.space',
  Spacebar: 'shortcuts.keys.space',
  Delete: 'shortcuts.keys.delete',
  Backspace: 'shortcuts.keys.backspace',
  Escape: 'shortcuts.keys.escape',
};

/** The name of one key as the help shows it: a letter upper-case, `-` as « − », the named keys translated. */
function keyName(key: string): string {
  const named = KEY_NAMES[key];
  if (named) return t(named);
  if (key === '-') return '−';
  return key.length === 1 ? key.toUpperCase() : key;
}

/** The parts of a chord as the help shows them, translated: ['Ctrl', 'Z'], ['Maj', '→']… (`shift: 'any'` shows no Shift). */
export function chordParts(chord: Chord): string[] {
  const parts: string[] = [];
  if (chord.ctrl) parts.push(t('shortcuts.keys.ctrl'));
  if (chord.alt) parts.push(t('shortcuts.keys.alt'));
  if (chord.shift === true) parts.push(t('shortcuts.keys.shift'));
  parts.push(keyName(chord.key));
  return parts;
}
