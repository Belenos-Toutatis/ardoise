// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createRawSnippet, flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import { SidePanelState, type SideBadges, type SideTabId } from '../../lib/side-panel.svelte';
import SidePanel from './SidePanel.svelte';
import { openSideTab, rememberSide, sideOf, tabPanelOf } from './side-test-helpers';

/** The contract of the panel's DOM (docs/design/side-panel.md, §2): the behaviour of the rail is package A's tests. */
let cleanup: (() => void) | null = null;

beforeEach(() => {
  localStorage.clear();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  localStorage.clear();
});

const snippet = (html: string) => createRawSnippet(() => ({ render: () => html }));

/** The live window's four tabs, with a block in each. */
function render(options: { badges?: SideBadges; above?: boolean; tabs?: SideTabId[] } = {}) {
  const tabs = options.tabs ?? ['layers', 'sound', 'projection', 'help'];
  const target = document.createElement('div');
  document.body.append(target);
  const panel = new SidePanelState('live');
  const props: Record<string, unknown> = { where: 'live', panel, badges: options.badges ?? {} };
  for (const tab of tabs) props[tab] = snippet(`<p data-test="${tab}">block ${tab}</p>`);
  if (options.above) props.above = snippet('<p data-test="above">above</p>');
  const component = mount(SidePanel, { target, props: props as never });
  flushSync();
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const qa = (selector: string) => [...target.querySelectorAll<HTMLElement>(selector)];
  return { target, panel, q, qa, tab: (id: SideTabId) => q<HTMLButtonElement>(`[role="tab"][data-tab="${id}"]`)! };
}

describe('SidePanel: the DOM contract', () => {
  it('has a tab and a panel for each snippet given, in the rail order', () => {
    const { q, qa } = render();
    expect(qa('[role="tab"]').map((tab) => tab.dataset.tab)).toEqual(['layers', 'sound', 'projection', 'help']);
    expect(qa('[role="tabpanel"]').map((panel) => panel.dataset.tabPanel)).toEqual(['layers', 'sound', 'projection', 'help']);
    const root = q('.side-panel')!;
    expect(root.dataset.sidePanel).toBe('live');
    expect(root.dataset.tab).toBe('layers');
    expect(root.dataset.collapsed).toBe('false');
    const rail = q('.side-rail')!;
    expect(rail.getAttribute('role')).toBe('tablist');
    expect(rail.getAttribute('aria-orientation')).toBe('vertical');
    expect(rail.getAttribute('aria-label')).toBe('Onglets du panneau');
  });

  it('gives each tab its ids, aria attributes and title, and only the shown one in the tab order', () => {
    const { tab } = render();
    const layers = tab('layers');
    expect(layers.id).toBe('side-tab-live-layers');
    expect(layers.getAttribute('aria-controls')).toBe('side-tabpanel-live-layers');
    expect(layers.getAttribute('aria-selected')).toBe('true');
    expect(layers.getAttribute('aria-expanded')).toBe('true');
    expect(layers.tabIndex).toBe(0);
    expect(layers.title).toBe('Calques (cliquer pour replier)');
    const sound = tab('sound');
    expect(sound.getAttribute('aria-selected')).toBe('false');
    expect(sound.getAttribute('aria-expanded')).toBe('false');
    expect(sound.tabIndex).toBe(-1);
    expect(sound.title).toBe('Son');
    expect(layers.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
  });

  it('keeps every tab panel mounted, hidden but the shown one, each labelled by its tab', () => {
    const { qa, q } = render();
    expect(qa('[role="tabpanel"]').map((panel) => panel.hidden)).toEqual([false, true, true, true]);
    const sound = q('[data-tab-panel="sound"]')!;
    expect(sound.id).toBe('side-tabpanel-live-sound');
    expect(sound.getAttribute('aria-labelledby')).toBe('side-tab-live-sound');
    expect(sound.querySelector('[data-test="sound"]')).not.toBeNull();
    expect(tabPanelOf(sound.querySelector('p')!)).toBe('sound');
    expect(tabPanelOf(q('.side-rail')!)).toBeNull();
  });

  it('shows the pane title and the collapse button', () => {
    const { q } = render();
    expect(q('[data-slot="side-title"]')!.textContent).toBe('Calques');
    expect(q('[data-slot="side-title"]')!.tagName).toBe('H2');
    const fold = q('[data-action="side-collapse"]')!;
    expect(fold.getAttribute('aria-label')).toBe('Replier le panneau');
    expect(q('.side-pane')!.hidden).toBe(false);
  });

  it('opens a tab on a click, collapses on a click on the tab shown, and « › » collapses', async () => {
    const { q, tab, panel, target } = render();
    tab('sound').click();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'sound', collapsed: false });
    expect(q('[data-slot="side-title"]')!.textContent).toBe('Son');
    expect(tab('sound').getAttribute('aria-expanded')).toBe('true');
    expect(tab('layers').getAttribute('aria-selected')).toBe('false');
    tab('sound').click();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'sound', collapsed: true });
    expect(q('.side-pane')!.hidden).toBe(true);
    expect(tab('sound').getAttribute('aria-expanded')).toBe('false');
    expect(tab('sound').getAttribute('aria-selected')).toBe('true');
    expect(tab('sound').title).toBe('Son');
    // Collapsed: a click opens it again; « › » collapses.
    tab('help').click();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'help', collapsed: false });
    q<HTMLButtonElement>('[data-action="side-collapse"]')!.click();
    await tick();
    expect(panel.collapsed).toBe(true);
    expect(sideOf(target).collapsed).toBe(true);
    expect(JSON.parse(localStorage.getItem('ardoise.sidePanel.live')!)).toEqual({ tab: 'help', collapsed: true });
  });

  it('follows the window that opens it, and the tests helpers read and drive it', async () => {
    const { target, panel } = render();
    panel.open('projection');
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'projection', collapsed: false });
    await openSideTab(target, 'help');
    expect(sideOf(target)).toEqual({ tab: 'help', collapsed: false });
    // Already shown and open: the helper does not collapse it.
    await openSideTab(target, 'help');
    expect(sideOf(target)).toEqual({ tab: 'help', collapsed: false });
    panel.collapse();
    await tick();
    await openSideTab(target, 'help');
    expect(sideOf(target)).toEqual({ tab: 'help', collapsed: false });
    await expect(openSideTab(target, 'page')).rejects.toThrow('no page tab');
  });

  it('starts from the view remembered, and shows the default for a tab the window lacks', () => {
    rememberSide('live', { tab: 'page', collapsed: true });
    const { target, panel } = render();
    expect(sideOf(target)).toEqual({ tab: 'layers', collapsed: true });
    expect(panel.tab).toBe('page');
    expect(target.querySelector('.side-pane')!.hasAttribute('hidden')).toBe(true);
  });

  it('puts the badges in data-count and data-dot, with the hidden text and the visible marks', () => {
    const { tab, q } = render({ badges: { layers: { count: 2, dot: 'warn' }, sound: { dot: 'rec' }, projection: { dot: 'live' } } });
    const layers = tab('layers');
    expect(layers.dataset.count).toBe('2');
    expect(layers.dataset.dot).toBe('warn');
    expect(layers.querySelector('.visually-hidden')!.textContent).toBe('Calques, 2 calques, un calque utilisé est masqué');
    expect(layers.querySelector('.side-tab-count')!.textContent).toBe('2');
    expect(layers.querySelector('.side-tab-dot')!.getAttribute('data-kind')).toBe('warn');
    expect(tab('sound').dataset.dot).toBe('rec');
    expect(tab('sound').dataset.count).toBeUndefined();
    expect(tab('sound').querySelector('.visually-hidden')!.textContent).toBe('Son, enregistrement en cours');
    expect(tab('sound').querySelector('.side-tab-count')).toBeNull();
    expect(tab('projection').querySelector('.visually-hidden')!.textContent).toBe('Projection, projection ou partage en cours');
    // No badge: no attribute, no mark, the name alone.
    expect(tab('help').hasAttribute('data-count')).toBe(false);
    expect(tab('help').hasAttribute('data-dot')).toBe(false);
    expect(tab('help').querySelector('.visually-hidden')!.textContent).toBe('Aide');
    expect(q('.side-tab-dot[data-kind="live"]')).not.toBeNull();
  });

  it('puts the block above the open tab only while it is given', async () => {
    const withAbove = render({ above: true });
    const above = withAbove.q('[data-slot="side-above"]')!;
    expect(above.classList.contains('side-above')).toBe(true);
    expect(above.textContent).toBe('above');
    // Between the pane's head and the body.
    expect(above.previousElementSibling!.classList.contains('side-pane-head')).toBe(true);
    expect(above.nextElementSibling!.classList.contains('side-pane-body')).toBe(true);
    cleanup?.();
    expect(render().q('[data-slot="side-above"]')).toBeNull();
  });

  it('lists the six tabs when every snippet is given', () => {
    const { qa } = render({ tabs: ['layers', 'page', 'background', 'sound', 'projection', 'help'] });
    expect(qa('[role="tab"]').map((tab) => tab.dataset.tab)).toEqual(['layers', 'page', 'background', 'sound', 'projection', 'help']);
  });
});

/** A key press on `el`, as the browser dispatches it (bubbling, cancelable); returns the event. */
function press(el: Element, key: string, mods: { shiftKey?: boolean; ctrlKey?: boolean; altKey?: boolean } = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...mods });
  el.dispatchEvent(event);
  return event;
}

describe('SidePanel: the keyboard on the rail', () => {
  const ALL: SideTabId[] = ['layers', 'page', 'background', 'sound', 'projection', 'help'];

  it('moves the focus with ArrowDown and ArrowUp, wrapping, without activating anything', () => {
    const { tab, target, panel } = render({ tabs: ALL });
    tab('layers').focus();
    expect(press(document.activeElement!, 'ArrowDown').defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(tab('page'));
    press(document.activeElement!, 'ArrowDown');
    expect(document.activeElement).toBe(tab('background'));
    press(document.activeElement!, 'ArrowUp');
    press(document.activeElement!, 'ArrowUp');
    expect(document.activeElement).toBe(tab('layers'));
    // Wraps at both ends.
    press(tab('layers'), 'ArrowUp');
    expect(document.activeElement).toBe(tab('help'));
    press(tab('help'), 'ArrowDown');
    expect(document.activeElement).toBe(tab('layers'));
    // Manual activation: nothing was opened, the roving tabindex stays on the tab shown.
    expect(sideOf(target)).toEqual({ tab: 'layers', collapsed: false });
    expect(panel.tab).toBe('layers');
    expect(tab('layers').tabIndex).toBe(0);
    expect(tab('help').tabIndex).toBe(-1);
    expect(localStorage.getItem('ardoise.sidePanel.live')).toBeNull();
  });

  it('goes to the first and last tab with Home and End', () => {
    const { tab } = render({ tabs: ALL });
    tab('sound').focus();
    expect(press(tab('sound'), 'End').defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(tab('help'));
    expect(press(tab('help'), 'Home').defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(tab('layers'));
  });

  it('does not reopen a collapsed pane by moving the focus', async () => {
    const { tab, target, panel } = render({ tabs: ALL });
    panel.collapse();
    await tick();
    tab('layers').focus();
    press(tab('layers'), 'ArrowDown');
    await tick();
    expect(document.activeElement).toBe(tab('page'));
    expect(sideOf(target)).toEqual({ tab: 'layers', collapsed: true });
    // Enter is the native button's click: it opens the tab the focus is on.
    tab('page').click();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'page', collapsed: false });
  });

  it('works with the tabs a window has, and with a single tab', () => {
    const live = render();
    live.tab('help').focus();
    press(live.tab('help'), 'ArrowDown');
    expect(document.activeElement).toBe(live.tab('layers'));
    press(live.tab('layers'), 'ArrowDown');
    expect(document.activeElement).toBe(live.tab('sound'));
    cleanup?.();
    const one = render({ tabs: ['layers'] });
    one.tab('layers').focus();
    press(one.tab('layers'), 'ArrowDown');
    press(one.tab('layers'), 'ArrowUp');
    expect(document.activeElement).toBe(one.tab('layers'));
  });

  it('leaves to the window every key it does not use, and the arrows with a modifier', () => {
    const { tab } = render({ tabs: ALL });
    tab('layers').focus();
    // Space is the next page in live mode, ArrowLeft / Right turn pages, Shift + arrows pan, Alt + arrows go through notes.
    expect(press(tab('layers'), ' ').defaultPrevented).toBe(false);
    expect(press(tab('layers'), 'ArrowLeft').defaultPrevented).toBe(false);
    expect(press(tab('layers'), 'ArrowRight').defaultPrevented).toBe(false);
    expect(press(tab('layers'), 'Enter').defaultPrevented).toBe(false);
    expect(press(tab('layers'), 'ArrowDown', { shiftKey: true }).defaultPrevented).toBe(false);
    expect(press(tab('layers'), 'ArrowUp', { ctrlKey: true }).defaultPrevented).toBe(false);
    expect(press(tab('layers'), 'End', { altKey: true }).defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(tab('layers'));
  });

  it('lets the event reach the window, its default already cancelled', () => {
    const { tab } = render({ tabs: ALL });
    const seen: boolean[] = [];
    const onKey = (e: KeyboardEvent) => seen.push(e.defaultPrevented);
    window.addEventListener('keydown', onKey);
    try {
      tab('layers').focus();
      press(tab('layers'), 'ArrowDown');
      // The live window returns on a defaultPrevented key: the rail's keys never run as a page action.
      expect(seen).toEqual([true]);
    } finally {
      window.removeEventListener('keydown', onKey);
    }
  });
});

describe('SidePanel: the focus when the pane folds', () => {
  it('passes the focus from the collapse button to the tab shown', async () => {
    const { q, tab, target } = render();
    tab('sound').click();
    await tick();
    const fold = q<HTMLButtonElement>('[data-action="side-collapse"]')!;
    fold.focus();
    expect(document.activeElement).toBe(fold);
    fold.click();
    // The component refocuses after its own tick: let that continuation run.
    await tick();
    await tick();
    expect(sideOf(target)).toEqual({ tab: 'sound', collapsed: true });
    expect(document.activeElement).toBe(tab('sound'));
  });

  it('puts it on the default tab when the tab remembered is one the window lacks', async () => {
    rememberSide('live', { tab: 'page', collapsed: false });
    const { q, tab } = render();
    q<HTMLButtonElement>('[data-action="side-collapse"]')!.click();
    await tick();
    await tick();
    expect(document.activeElement).toBe(tab('layers'));
  });

  it('moves no focus when a window opens the pane (a selection, a replay)', async () => {
    const { q, panel, tab } = render();
    const outside = document.createElement('button');
    document.body.append(outside);
    try {
      panel.collapse();
      await tick();
      outside.focus();
      panel.open();
      await tick();
      expect(document.activeElement).toBe(outside);
      panel.open('sound');
      await tick();
      expect(document.activeElement).toBe(outside);
      expect(tab('sound').getAttribute('aria-selected')).toBe('true');
      expect(q('.side-pane')!.hidden).toBe(false);
    } finally {
      outside.remove();
    }
  });
});

describe('SidePanel: the stylesheet', () => {
  // jsdom applies no component stylesheet (M15's `.panel` strip went unseen for that reason): read the rules from the
  // source for the two that only a real window would show.
  const source = readFileSync(resolve(process.cwd(), 'ui/src/components/side/SidePanel.svelte'), 'utf8');
  const style = source.slice(source.indexOf('<style>'));
  const rules = [...style.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
    selector: m[1].replace(/\/\*[\s\S]*?\*\//g, '').trim(),
    body: m[2],
  }));

  it('gives no display to the pane except while it is not hidden (an author display beats the hidden attribute)', () => {
    const paneRules = rules.filter(({ selector }) => /(^|[\s,])\.side-pane(?![-\w])/.test(selector) && !selector.startsWith(':where'));
    expect(paneRules.length).toBeGreaterThan(0);
    for (const { selector, body } of paneRules) {
      if (/(^|;|\s)display\s*:/.test(body)) expect(selector, 'a display on the pane').toContain(':not([hidden])');
    }
  });

  it('keeps every block look inside the pane, so that nothing leaks to the rest of the app', () => {
    const blockRules = rules.filter(({ selector }) => selector.includes('.pane-') || selector.includes('kbd') || selector.includes('dl.keys'));
    expect(blockRules.length).toBeGreaterThan(10);
    for (const { selector } of blockRules) expect(selector, 'a block rule outside the pane').toMatch(/\.side-(pane|above)/);
  });
});
