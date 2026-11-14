import { describe, expect, it } from 'vitest';
import { closeKey } from '../close-keys';
import type { KeyInput } from '../live-keys';
import { chordEvents, keyPress, type EditorKeyContext, type Shortcut, type ShortcutKey } from '../shortcuts';
import { SWEEP_KEYS, SWEEP_MODIFIERS, sweepShortcuts } from '../shortcuts-sweep';
import { isGuideKey } from '../guide';
import { editorKeyAction } from './keys';
import { EDITOR_KEY_CONTEXTS, EDITOR_SHORTCUTS } from './shortcuts';

/** What the editor does with a key, in the words of the table: Ctrl + Q and F1 are `App.svelte`'s, the rest `editorKeyAction`'s. */
function editorDoes(e: KeyInput, context: EditorKeyContext): string | null {
  if (closeKey(e) === 'quit') return 'quit';
  return isGuideKey(e) ? 'guide' : (editorKeyAction(e, context)?.kind ?? null);
}

function inContext(key: { in?: Partial<EditorKeyContext> }, context: EditorKeyContext): boolean {
  return Object.entries(key.in ?? {}).every(([field, value]) => (context as unknown as Record<string, unknown>)[field] === value);
}

describe('the editor shortcut table', () => {
  it('agrees with the key mapping, both ways', () => {
    expect(sweepShortcuts(EDITOR_SHORTCUTS, EDITOR_KEY_CONTEXTS, editorDoes)).toEqual([]);
  });

  it('is swept in the two contexts of the editor', () => {
    expect(EDITOR_KEY_CONTEXTS).toEqual([{ replaying: false }, { replaying: true }]);
  });

  it('is not a dead sweep: the mapping answers differently in each context', () => {
    expect(editorDoes({ key: 'v' }, { replaying: false })).toBe('select');
    expect(editorDoes({ key: 'v' }, { replaying: true })).toBeNull();
    expect(editorDoes({ key: 'r' }, { replaying: false })).toBe('replay');
    expect(editorDoes({ key: 'r' }, { replaying: true })).toBe('leaveReplay');
    expect(editorDoes({ key: 'q', ctrlKey: true }, { replaying: false })).toBe('quit');
    expect(editorDoes({ key: 'F1' }, { replaying: false })).toBe('guide');
    expect(editorDoes({ key: 'F1' }, { replaying: true })).toBe('guide');
    expect(editorDoes({ key: 'F1', ctrlKey: true }, { replaying: false })).toBeNull();
  });

  it('lists every kind of action the mapping gives over the swept keys, so none escapes the sweep', () => {
    const swept = new Set<string>();
    for (const context of EDITOR_KEY_CONTEXTS) {
      for (const name of SWEEP_KEYS) {
        for (const mods of SWEEP_MODIFIERS) {
          const done = editorDoes(keyPress(name, undefined, mods), context);
          if (done) swept.add(done);
        }
      }
    }
    const listed = new Set(EDITOR_SHORTCUTS.flatMap((entry) => entry.keys.map((key) => key.does)));
    expect([...swept].sort()).toEqual([...listed].sort());
  });

  it('gives each chord one meaning in each context', () => {
    for (const context of EDITOR_KEY_CONTEXTS) {
      const seen = new Map<string, string>();
      for (const entry of EDITOR_SHORTCUTS) {
        for (const key of entry.keys) {
          if (!inContext(key, context)) continue;
          for (const event of chordEvents(key.chord)) {
            const id = JSON.stringify([event.key.toLowerCase(), event.code, !!event.ctrlKey, !!event.shiftKey, !!event.altKey]);
            const previous = seen.get(id);
            expect(previous === undefined || previous === key.does, `${entry.id}: ${id} is also "${previous}"`).toBe(true);
            seen.set(id, key.does);
          }
        }
      }
    }
  });

  it('shows the keys of a replay in the replay group only, and no other group key acts only in a replay', () => {
    for (const entry of EDITOR_SHORTCUTS) {
      const replays = entry.keys.map((key) => key.in?.replaying);
      if (entry.group === 'replay') expect(replays.every((value) => value === true), entry.id).toBe(true);
      else expect(replays.includes(true), entry.id).toBe(false);
    }
  });

  it('keeps every hidden variant out of the help and in the mapping, in every context it is listed for', () => {
    let hidden = 0;
    for (const entry of EDITOR_SHORTCUTS) {
      expect(entry.keys.some((key) => key.shown !== false), `${entry.id} shows nothing`).toBe(true);
      for (const key of entry.keys.filter((candidate) => candidate.shown === false)) {
        hidden++;
        const contexts = EDITOR_KEY_CONTEXTS.filter((context) => inContext(key, context));
        expect(contexts.length, `${entry.id} is in no context`).toBeGreaterThan(0);
        for (const context of contexts) {
          for (const event of chordEvents(key.chord)) expect(editorDoes(event, context), entry.id).toBe(key.does);
        }
      }
    }
    expect(hidden).toBeGreaterThan(20);
  });

  it('the layouts\' own keys for Ctrl + 0 and Ctrl + 2 act in every context', () => {
    for (const context of EDITOR_KEY_CONTEXTS) {
      expect(editorDoes({ key: 'à', code: 'Digit0', ctrlKey: true, shiftKey: true }, context)).toBe('fit');
      expect(editorDoes({ key: '0', code: 'Numpad0', ctrlKey: true }, context)).toBe('fit');
      expect(editorDoes({ key: 'é', code: 'Digit2', ctrlKey: true, shiftKey: true }, context)).toBe('fitWidth');
      expect(editorDoes({ key: '2', code: 'Numpad2', ctrlKey: true }, context)).toBe('fitWidth');
    }
  });
});

describe('the sweep catches a table that lies about the editor', () => {
  const copy = () => structuredClone(EDITOR_SHORTCUTS) as Shortcut<EditorKeyContext>[];
  const sweep = (table: Shortcut<EditorKeyContext>[]) => sweepShortcuts(table, EDITOR_KEY_CONTEXTS, editorDoes);
  const entry = (table: Shortcut<EditorKeyContext>[], id: string) => table.find((candidate) => candidate.id === id)!;

  it('a shown key that does something else fails', () => {
    const table = copy();
    entry(table, 'pen').keys[0].does = 'eraser';
    expect(sweep(table).some((p) => p.includes('"pen"') && p.includes('gives "pen"') && p.includes('says "eraser"'))).toBe(true);
  });

  it('a key listed for a context where it does nothing fails', () => {
    const table = copy();
    delete entry(table, 'select').keys[0].in; // V does nothing in a replay
    expect(sweep(table).some((p) => p.includes('"select"') && p.includes('gives nothing') && p.includes('"replaying":true'))).toBe(true);
  });

  it('a replay key listed for the wrong context fails, both ways', () => {
    const table = copy();
    entry(table, 'leaveReplay').keys[0].in = { replaying: false };
    const problems = sweep(table);
    expect(problems.some((p) => p.includes('"leaveReplay"'))).toBe(true);
    expect(problems.some((p) => p.includes('no entry of the table lists'))).toBe(true);
  });

  it('a handled key missing from the table fails: a plain key, a Ctrl key, a replay key, Alt + arrow, Ctrl + Q', () => {
    for (const [id, index, sentence] of [
      ['straighten', 0, 'f gives "straighten" in {"replaying":false}'],
      ['redo', 0, 'Ctrl + y gives "redo"'],
      ['playPause', 0, 'Space gives "playPause" in {"replaying":true}'],
      ['pages', 1, 'Alt + ArrowRight gives "nextPage"'],
      ['quit', 0, 'Ctrl + q gives "quit"'],
      ['guide', 0, 'F1 gives "guide"'],
    ] as const) {
      const table = copy();
      (entry(table, id).keys as ShortcutKey<EditorKeyContext>[]).splice(index, 1);
      const problems = sweep(table);
      expect(problems.some((p) => p.includes(sentence) && p.includes('no entry of the table lists')), `${id}: ${problems.join(' | ')}`).toBe(true);
    }
  });

  it('a hidden variant missing from the table fails', () => {
    for (const id of ['select', 'delete', 'redo', 'selectAll', 'save', 'escape']) {
      const table = copy();
      const e = entry(table, id);
      const index = e.keys.findIndex((key) => key.shown === false);
      (e.keys as ShortcutKey<EditorKeyContext>[]).splice(index, 1);
      expect(sweep(table).length, id).toBeGreaterThan(0);
    }
  });

  it('an entry whose context is too narrow fails: Ctrl + 0 listed outside a replay only', () => {
    const table = copy();
    entry(table, 'fit').keys[0].in = { replaying: false };
    expect(sweep(table).some((p) => p.includes('Ctrl + 0') && p.includes('"replaying":true'))).toBe(true);
  });
});
