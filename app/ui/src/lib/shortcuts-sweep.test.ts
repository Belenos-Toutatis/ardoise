import { describe, expect, it } from 'vitest';
import type { KeyInput } from './live-keys';
import type { Shortcut } from './shortcuts';
import { SWEEP_KEYS, SWEEP_MODIFIERS, sweepShortcuts } from './shortcuts-sweep';

interface Context {
  armed: boolean;
}
const CONTEXTS: readonly Context[] = [{ armed: false }, { armed: true }];

/** A tiny mapping: V → `tool`, Ctrl + Z → `undo` (only when armed), Shift + Space → `pan`. */
function does(e: KeyInput, context: Context): string | null {
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (e.ctrlKey && !e.altKey && !e.shiftKey && key === 'z') return context.armed ? 'undo' : null;
  if (e.ctrlKey || e.altKey) return null;
  if (key === 'v' && !e.shiftKey) return 'tool';
  if (key === ' ' && e.shiftKey) return 'pan';
  return null;
}

const tool: Shortcut<Context> = { id: 'tool', label: 'x', keys: [{ chord: { key: 'v' }, does: 'tool' }] };
const undo: Shortcut<Context> = { id: 'undo', label: 'x', keys: [{ chord: { key: 'z', ctrl: true }, does: 'undo', in: { armed: true } }] };
const pan: Shortcut<Context> = { id: 'pan', label: 'x', keys: [{ chord: { key: ' ', shift: true }, does: 'pan', shown: false }] };

describe('sweepShortcuts', () => {
  it('finds no disagreement when the table is right', () => {
    expect(sweepShortcuts([tool, undo, pan], CONTEXTS, does)).toEqual([]);
  });

  it('reports a chord that does something else, or nothing', () => {
    const wrong: Shortcut<Context> = { ...tool, keys: [{ chord: { key: 'v' }, does: 'pen' }] };
    expect(sweepShortcuts([wrong, undo, pan], CONTEXTS, does)).toContain(
      '"tool": v gives "tool" in {"armed":false}, the table says "pen"',
    );
    const nothing: Shortcut<Context> = { ...tool, keys: [{ chord: { key: 'v', shift: true }, does: 'tool' }] };
    const problems = sweepShortcuts([nothing, undo, pan], CONTEXTS, does);
    expect(problems).toContain('"tool": Shift + V gives nothing in {"armed":true}, the table says "tool"');
  });

  it('reports a chord the mapping handles that no entry lists', () => {
    const problems = sweepShortcuts([undo, pan], CONTEXTS, does);
    expect(problems).toContain('v gives "tool" in {"armed":false}, which no entry of the table lists');
    expect(problems.some((p) => p.startsWith('Ctrl + z'))).toBe(false);
  });

  it('reports a context an entry forgets', () => {
    const everywhere: Shortcut<Context> = { ...undo, keys: [{ chord: { key: 'z', ctrl: true }, does: 'undo', in: { armed: false } }] };
    const problems = sweepShortcuts([tool, everywhere, pan], CONTEXTS, does);
    // Listed for a context where the mapping does nothing, and missing in the one where it acts.
    expect(problems).toContain('"undo": Ctrl + z gives nothing in {"armed":false}, the table says "undo"');
    expect(problems).toContain('Ctrl + z gives "undo" in {"armed":true}, which no entry of the table lists');
  });

  it('presses letters upper-case with Shift and counts a key that is not shown as listed', () => {
    const seen: KeyInput[] = [];
    sweepShortcuts([], [{ armed: false }], (e) => (seen.push(e), null));
    expect(seen).toHaveLength(SWEEP_KEYS.length * SWEEP_MODIFIERS.length);
    expect(seen.find((e) => e.key === 'V' && e.shiftKey && e.code === 'KeyV')).toBeDefined();
    expect(seen.find((e) => e.key === ' ' && e.code === 'Space')).toBeDefined();
    expect(seen.every((e) => e.repeat === false && e.target === null)).toBe(true);
  });

  it('sweeps the keys of the design and the modifiers without Ctrl + Alt', () => {
    expect(SWEEP_KEYS).toEqual(
      expect.arrayContaining(['a', 'z', '0', '9', ' ', '+', '-', '=', '−', 'ArrowDown', 'PageUp', 'Home', 'Delete', 'Escape', 'Tab', 'F1', 'F12']),
    );
    expect(SWEEP_MODIFIERS).toHaveLength(6);
    expect(SWEEP_MODIFIERS.some((m) => 'ctrl' in m && 'alt' in m)).toBe(false);
  });
});
