import { describe, expect, it } from 'vitest';
import { closeKey } from './close-keys';
import { liveEditKeyAction, liveKeyAction, type KeyInput } from './live-keys';
import { LIVE_KEY_CONTEXTS, LIVE_SHORTCUTS } from './live-shortcuts';
import type { LiveKeyContext } from './shortcuts';
import { sweepShortcuts } from './shortcuts-sweep';

/**
 * What the control window does with a key, in the words of the table: the touch-up keys first, as `LiveView.onkeydown`
 * does while a session runs, then Ctrl + Q (which `App.svelte` answers), then `liveKeyAction`.
 */
function liveDoes(e: KeyInput, context: LiveKeyContext): string | null {
  if (context.running) {
    const edit = liveEditKeyAction(e, context.selection);
    if (edit) return edit.kind;
  }
  if (closeKey(e) === 'quit') return 'quit';
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
