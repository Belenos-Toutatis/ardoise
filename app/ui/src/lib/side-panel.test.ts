import { describe, expect, it } from 'vitest';
import {
  SIDE_DEFAULTS,
  SIDE_TABS,
  SidePanelState,
  loadSideView,
  saveSideView,
  sideStorageKey,
  type SideTabId,
} from './side-panel.svelte';

/** A storage in memory, with the two methods the panel uses. */
function memory(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

const ALL = SIDE_TABS;
/** What the live window gives in M16: no Page, no Fond. */
const LIVE: readonly SideTabId[] = ['layers', 'sound', 'projection', 'help'];

describe('the remembered view', () => {
  it('has one key per window and the same defaults in both', () => {
    expect(sideStorageKey('editor')).toBe('ardoise.sidePanel.editor');
    expect(sideStorageKey('live')).toBe('ardoise.sidePanel.live');
    expect(SIDE_DEFAULTS.editor).toEqual({ tab: 'layers', collapsed: false });
    expect(SIDE_DEFAULTS.live).toEqual({ tab: 'layers', collapsed: false });
    expect(SIDE_TABS).toEqual(['layers', 'page', 'background', 'sound', 'projection', 'help']);
  });

  it('is read field by field', () => {
    const read = (value: string) => loadSideView('editor', memory({ [sideStorageKey('editor')]: value }));
    expect(read('{"tab":"page","collapsed":true}')).toEqual({ tab: 'page', collapsed: true });
    expect(read('{"tab":"sound"}')).toEqual({ tab: 'sound', collapsed: false });
    expect(read('{"collapsed":true}')).toEqual({ tab: 'layers', collapsed: true });
    expect(read('{"tab":"nope","collapsed":"yes"}')).toEqual(SIDE_DEFAULTS.editor);
    expect(read('{"tab":3,"collapsed":1}')).toEqual(SIDE_DEFAULTS.editor);
  });

  it('gives the defaults for garbage, nothing stored or a blocked storage', () => {
    const read = (value: string) => loadSideView('live', memory({ [sideStorageKey('live')]: value }));
    for (const garbage of ['', '{', 'null', '[]', '"page"', '42', 'true']) expect(read(garbage), garbage).toEqual(SIDE_DEFAULTS.live);
    expect(loadSideView('live', memory())).toEqual(SIDE_DEFAULTS.live);
    expect(loadSideView('live', null)).toEqual(SIDE_DEFAULTS.live);
    expect(loadSideView('live', { getItem: () => { throw new Error('blocked'); } })).toEqual(SIDE_DEFAULTS.live);
  });

  it('does not hand out the shared defaults', () => {
    const view = loadSideView('editor', null);
    view.tab = 'help';
    expect(SIDE_DEFAULTS.editor.tab).toBe('layers');
  });

  it('is written as { tab, collapsed } and a blocked storage is ignored', () => {
    const storage = memory();
    saveSideView('editor', { tab: 'help', collapsed: true }, storage);
    expect(storage.data.get('ardoise.sidePanel.editor')).toBe('{"tab":"help","collapsed":true}');
    expect(() =>
      saveSideView('editor', SIDE_DEFAULTS.editor, { setItem: () => { throw new Error('full'); } }),
    ).not.toThrow();
    expect(() => saveSideView('editor', SIDE_DEFAULTS.editor, null)).not.toThrow();
  });
});

describe('SidePanelState', () => {
  it('starts from what is remembered', () => {
    const state = new SidePanelState('editor', memory({ [sideStorageKey('editor')]: '{"tab":"background","collapsed":true}' }));
    expect(state.snapshot()).toEqual({ tab: 'background', collapsed: true });
    expect(new SidePanelState('live', memory()).snapshot()).toEqual(SIDE_DEFAULTS.live);
    expect(new SidePanelState('live', null).snapshot()).toEqual(SIDE_DEFAULTS.live);
  });

  it('shows the chosen tab, else the default, else the first', () => {
    const state = new SidePanelState('live', null);
    state.tab = 'projection';
    expect(state.shown(LIVE)).toBe('projection');
    state.tab = 'page';
    expect(state.shown(LIVE)).toBe('layers');
    expect(state.tab).toBe('page');
    expect(state.shown(['sound', 'help'])).toBe('sound');
    expect(state.shown([])).toBe('layers');
  });

  it('toggles: the tab shown and open collapses, any other tab opens', () => {
    const storage = memory();
    const state = new SidePanelState('editor', storage);
    state.toggle('layers', ALL);
    expect(state.snapshot()).toEqual({ tab: 'layers', collapsed: true });
    state.toggle('layers', ALL);
    expect(state.snapshot()).toEqual({ tab: 'layers', collapsed: false });
    state.toggle('sound', ALL);
    expect(state.snapshot()).toEqual({ tab: 'sound', collapsed: false });
    state.toggle('help', ALL);
    state.toggle('help', ALL);
    expect(state.snapshot()).toEqual({ tab: 'help', collapsed: true });
    // Collapsed: a click on another tab opens that one.
    state.toggle('page', ALL);
    expect(state.snapshot()).toEqual({ tab: 'page', collapsed: false });
    expect(storage.data.get('ardoise.sidePanel.editor')).toBe('{"tab":"page","collapsed":false}');
  });

  it('ignores a tab the window lacks', () => {
    const state = new SidePanelState('live', null);
    state.toggle('page', LIVE);
    expect(state.snapshot()).toEqual(SIDE_DEFAULTS.live);
  });

  it('keeps a remembered tab the window lacks until the user picks a tab', () => {
    const storage = memory({ [sideStorageKey('live')]: '{"tab":"page","collapsed":false}' });
    const state = new SidePanelState('live', storage);
    expect(state.shown(LIVE)).toBe('layers');
    // The default tab is the one shown: a click on it collapses, and the stored `page` is still there.
    state.toggle('layers', LIVE);
    expect(storage.data.get('ardoise.sidePanel.live')).toBe('{"tab":"page","collapsed":true}');
    state.toggle('sound', LIVE);
    expect(storage.data.get('ardoise.sidePanel.live')).toBe('{"tab":"sound","collapsed":false}');
  });

  it('opens, collapses, takes a snapshot and restores it, saving each time', () => {
    const storage = memory();
    const state = new SidePanelState('editor', storage);
    const key = sideStorageKey('editor');
    state.collapse();
    expect(storage.data.get(key)).toBe('{"tab":"layers","collapsed":true}');
    state.open();
    expect(state.snapshot()).toEqual({ tab: 'layers', collapsed: false });
    state.collapse();
    const kept = state.snapshot();
    state.open('sound');
    expect(storage.data.get(key)).toBe('{"tab":"sound","collapsed":false}');
    state.restore(kept);
    expect(state.snapshot()).toEqual({ tab: 'layers', collapsed: true });
    expect(storage.data.get(key)).toBe('{"tab":"layers","collapsed":true}');
  });

  it('shows a view without remembering it with persist: false, and a later change is remembered as usual', () => {
    const storage = memory();
    const state = new SidePanelState('editor', storage);
    const key = sideStorageKey('editor');
    state.collapse();
    const stored = storage.data.get(key);
    state.open('sound', { persist: false });
    expect(state.snapshot()).toEqual({ tab: 'sound', collapsed: false });
    expect(storage.data.get(key)).toBe(stored);
    // Without the option, `open` still saves; and a click on a tab saves.
    state.open('help', { persist: true });
    expect(storage.data.get(key)).toBe('{"tab":"help","collapsed":false}');
    state.collapse();
    state.open(undefined, { persist: false });
    expect(state.collapsed).toBe(false);
    expect(storage.data.get(key)).toBe('{"tab":"help","collapsed":true}');
    state.toggle('page', ALL);
    expect(storage.data.get(key)).toBe('{"tab":"page","collapsed":false}');
  });

  it('survives a blocked storage for the session', () => {
    const state = new SidePanelState('editor', {
      getItem: () => { throw new Error('blocked'); },
      setItem: () => { throw new Error('blocked'); },
    });
    state.toggle('help', ALL);
    expect(state.snapshot()).toEqual({ tab: 'help', collapsed: false });
  });
});
