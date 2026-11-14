import { describe, expect, it } from 'vitest';
import { closeKey } from './close-keys';
import { isGuideKey } from './guide';
import { liveEditKeyAction, liveKeyAction, type KeyInput } from './live-keys';
import { LIVE_KEY_CONTEXTS, LIVE_SHORTCUTS } from './live-shortcuts';
import type { LiveKeyContext } from './shortcuts';
import { sweepShortcuts } from './shortcuts-sweep';

/**
 * What the control window does with a key, in the words of the table: the touch-up keys first, as `LiveView.onkeydown`
 * does while a session runs, then Ctrl + Q and F1 (which `App.svelte` answers), then `liveKeyAction`.
 */
function liveDoes(e: KeyInput, context: LiveKeyContext): string | null {
  if (context.running) {
    const edit = liveEditKeyAction(e, context.selection);
    if (edit) return edit.kind;
  }
  if (closeKey(e) === 'quit') return 'quit';
  if (isGuideKey(e)) return 'guide';
  const action = liveKeyAction(e);
  if (!action) return null;
  if (action.kind === 'command') return action.command;
  if (action.kind === 'zoom') return action.direction === 1 ? 'zoomIn' : 'zoomOut';
  return action.kind;
}

describe('the live shortcut table', () => {
  it('agrees with the key mapping, both ways', () => {
    expect(sweepShortcuts(LIVE_SHORTCUTS, LIVE_KEY_CONTEXTS, liveDoes)).toEqual([]);
  });

  it('is not a dead table: the sweep reads what the mapping does', () => {
    expect(liveDoes({ key: 'v' }, { running: true, selection: false })).toBe('select');
    expect(liveDoes({ key: 'v' }, { running: false, selection: false })).toBeNull();
    expect(liveDoes({ key: 'Delete' }, { running: true, selection: true })).toBe('deleteSelection');
    expect(liveDoes({ key: 'Delete' }, { running: true, selection: false })).toBe('clearMarks');
  });
});

describe('the live shortcut table, entry by entry', () => {
  const entry = (id: string) => LIVE_SHORTCUTS.find((e) => e.id === id)!;

  it('puts the keys of the projected window in a group of their own, for no selection only', () => {
    const apart = LIVE_SHORTCUTS.filter((e) => e.group === 'presentation');
    expect(apart.map((e) => e.id)).toEqual(['marks']);
    for (const key of apart[0].keys) expect(key.in).toEqual({ selection: false });
    // Nothing else is grouped: the help's main list is what the control window does.
    expect(LIVE_SHORTCUTS.filter((e) => e.group !== undefined && e.group !== 'presentation')).toEqual([]);
  });

  it('keeps the variants the mapping takes out of the help', () => {
    const hidden = (id: string) => entry(id).keys.filter((key) => key.shown === false).map((key) => key.chord.key + (key.chord.shift === true ? '+Shift' : ''));
    expect(hidden('next')).toEqual(['PageDown+Shift', 'Spacebar']);
    expect(hidden('zoom')).toEqual(['=', '−']);
    expect(hidden('board')).toEqual(['b+Shift']);
    expect(liveDoes({ key: '=' }, { running: false, selection: false })).toBe('zoomIn');
    expect(liveDoes({ key: 'Spacebar' }, { running: false, selection: false })).toBe('nextPage');
    expect(liveDoes({ key: 'B', shiftKey: true }, { running: false, selection: false })).toBe('board');
    expect(liveDoes({ key: '0', code: 'Numpad0' }, { running: false, selection: false })).toBe('wholePage');
  });

  it('says Esc once, whatever the selection', () => {
    const escape = entry('leave').keys.map((key) => [key.chord.key, key.does]);
    expect(escape).toEqual([
      ['Escape', 'deselect'],
      ['Escape', 'leave'],
    ]);
    expect(liveDoes({ key: 'Escape' }, { running: true, selection: true })).toBe('deselect');
    expect(liveDoes({ key: 'Escape' }, { running: true, selection: false })).toBe('leave');
  });

  it('has F1 open the guide in every context, and Ctrl + F1 do nothing', () => {
    for (const context of LIVE_KEY_CONTEXTS) {
      expect(liveDoes({ key: 'F1' }, context)).toBe('guide');
      expect(liveDoes({ key: 'F1', ctrlKey: true }, context)).toBeNull();
    }
  });

  it('has Space turn the page and nothing else, selected or not', () => {
    for (const context of LIVE_KEY_CONTEXTS) expect(liveDoes({ key: ' ', code: 'Space' }, context)).toBe('nextPage');
  });
});

