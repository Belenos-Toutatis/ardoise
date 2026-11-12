import { describe, expect, it } from 'vitest';
import { resumeBlock } from './live-resume.svelte';
import type { TabletStatus } from './tablet';

function status(change: Partial<TabletStatus>): TabletStatus {
  return { connection: 'searching', registered: { name: 'Slate', width: 29700, height: 21600 }, known: [], ...change } as TabletStatus;
}

describe('resumeBlock', () => {
  it('needs a registered tablet, which is not busy', () => {
    expect(resumeBlock(null)).toBe('noTablet');
    expect(resumeBlock(status({ registered: null, connection: 'notRegistered' }))).toBe('noTablet');
    expect(resumeBlock(status({ connection: 'syncing' }))).toBe('busy');
    expect(resumeBlock(status({ connection: 'registering' }))).toBe('busy');
    expect(resumeBlock(status({ connection: 'adding' }))).toBe('busy');
    // The blackboard alone is a tablet too (no Slate registered).
    const board = { kind: 'blackboard', name: 'Mijia LCD blackboard' } as TabletStatus['known'][number];
    expect(resumeBlock(status({ registered: null, known: [board] }))).toBeNull();
    for (const connection of ['searching', 'connecting', 'connected', 'live'] as const) {
      expect(resumeBlock(status({ connection }))).toBeNull();
    }
  });
});
