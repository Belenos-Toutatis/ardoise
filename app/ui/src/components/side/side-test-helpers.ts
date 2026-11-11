/**
 * Helpers of the vitest tests of the windows that hold the side panel (editor, live control window): they write the
 * remembered view before a mount, open a tab through the rail, and read the panel's state and a block's tab from the
 * DOM contract (docs/design/side-panel.md, §2 and §10). Under jsdom a hidden tab panel is still in the DOM, so a test
 * can reach its content with `querySelector` whatever tab is shown; `openSideTab` is for the tests that assert what
 * the user sees.
 */
import { tick } from 'svelte';
import { SIDE_TABS, saveSideView, type SideTabId, type SideView, type SideWhere } from '../../lib/side-panel.svelte';

/** Writes the view a window finds in the storage when it mounts (call it before the mount). */
export function rememberSide(where: SideWhere, view: SideView): void {
  saveSideView(where, view);
}

function panelOf(root: ParentNode): HTMLElement | null {
  if (root instanceof HTMLElement && root.matches('[data-side-panel]')) return root;
  return root.querySelector<HTMLElement>('[data-side-panel]');
}

/** `{ tab, collapsed }` from the data attributes of the side panel inside `root`. */
export function sideOf(root: ParentNode): SideView {
  const panel = panelOf(root);
  if (!panel) throw new Error('no side panel in the root');
  return { tab: panel.dataset.tab as SideTabId, collapsed: panel.dataset.collapsed === 'true' };
}

/** Clicks the rail's tab for `tab`, unless that tab is shown and open (a click would collapse it); then `tick()`. */
export async function openSideTab(root: ParentNode, tab: SideTabId): Promise<void> {
  const button = root.querySelector<HTMLButtonElement>(`[role="tab"][data-tab="${tab}"]`);
  if (!button) throw new Error(`no ${tab} tab in the rail`);
  const state = sideOf(root);
  if (!(state.tab === tab && !state.collapsed)) button.click();
  await tick();
}

/** The tab (`data-tab-panel`) whose panel holds `el`, or null when `el` is in none. */
export function tabPanelOf(el: Element): SideTabId | null {
  const id = el.closest('[data-tab-panel]')?.getAttribute('data-tab-panel') ?? null;
  return SIDE_TABS.find((tab) => tab === id) ?? null;
}
