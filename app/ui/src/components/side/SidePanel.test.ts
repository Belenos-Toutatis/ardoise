// @vitest-environment jsdom
import { createRawSnippet, mount, tick, unmount } from 'svelte';
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
