import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import fr from '../locales/fr.json';
import { EDITOR_KEY_CONTEXTS, EDITOR_SHORTCUTS } from './editor/shortcuts';
import { lookup, type Messages } from './i18n-core';
import { LIVE_KEY_CONTEXTS, LIVE_SHORTCUTS } from './live-shortcuts';
import type { Shortcut } from './shortcuts';

/** The structure of the two tables (the sweeps, which compare them with the key mappings, are in each window's tests). */
const TABLES: [string, readonly Shortcut<object>[], readonly object[]][] = [
  ['editor', EDITOR_SHORTCUTS, EDITOR_KEY_CONTEXTS],
  ['live', LIVE_SHORTCUTS, LIVE_KEY_CONTEXTS],
];

describe('the shortcut tables', () => {
  for (const [name, table, contexts] of TABLES) {
    it(`${name}: every entry has a unique id, a label and the keys it shows in both languages`, () => {
      expect(new Set(table.map((entry) => entry.id)).size).toBe(table.length);
      for (const entry of table) {
        for (const catalog of [fr, en] as Messages[]) {
          expect(lookup(catalog, entry.label), `${name} ${entry.id}`).toBeTruthy();
          if (entry.keysLabel) expect(lookup(catalog, entry.keysLabel), `${name} ${entry.id} keysLabel`).toBeTruthy();
        }
        // Something to show: chords, or the text of `keysLabel`.
        expect(entry.keysLabel !== undefined || entry.keys.some((key) => key.shown !== false), `${name} ${entry.id} shows nothing`).toBe(true);
      }
    });

    it(`${name}: a key's context names fields of the contexts, with values they take`, () => {
      const fields = Object.keys(contexts[0]);
      for (const entry of table) {
        for (const key of entry.keys) {
          for (const [field, value] of Object.entries(key.in ?? {})) {
            expect(fields, `${name} ${entry.id}`).toContain(field);
            expect(
              contexts.some((context) => (context as Record<string, unknown>)[field] === value),
              `${name} ${entry.id}: no context with ${field} = ${String(value)}`,
            ).toBe(true);
          }
        }
      }
    });

    it(`${name}: the groups are the ones the help knows`, () => {
      for (const entry of table) expect([undefined, 'replay', 'presentation']).toContain(entry.group);
    });
  }

  it('give the help the groups of the design: replay for the editor, presentation for live', () => {
    expect(new Set(EDITOR_SHORTCUTS.map((entry) => entry.group))).toEqual(new Set([undefined, 'replay']));
    expect(new Set(LIVE_SHORTCUTS.map((entry) => entry.group))).toEqual(new Set([undefined, 'presentation']));
  });
});
