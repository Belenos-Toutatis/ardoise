<script lang="ts">
  /**
   * The tabbed side panel of the editor and of the live control window (milestone M16; contract:
   * docs/design/side-panel.md, §2): the rail of icon tabs on the right edge and, left of it, the pane that shows the open
   * tab. A window gives a tab's content as a snippet of the same name, declared as a direct child of `<SidePanel>`: a
   * tab exists in a window if and only if its snippet is given. The window owns the `SidePanelState` (created with
   * `new SidePanelState(where)`) and calls `panel.open(…)`; the component calls `panel.toggle` on a tab and
   * `panel.collapse` on « › », and emits no event.
   *
   * Every tab panel stays mounted (`hidden` when not shown), so that what a block holds survives a tab switch.
   * The rail follows the WAI-ARIA tabs pattern with manual activation: the arrow keys, Home and End move the focus
   * between the tabs, Enter (or a click) activates one; in the live window Space is the page turn, never the rail's.
   * The look of the shell is scoped here; the look of the blocks a window puts in the pane (`pane-*`) is defined once at
   * the end, for everything inside `.side-pane`.
   */
  import { tick, type Snippet } from 'svelte';
  import { t, tn } from '../../lib/i18n.svelte';
  import {
    SIDE_TABS,
    type SideBadge,
    type SideBadges,
    type SidePanelState,
    type SideTabId,
    type SideWhere,
  } from '../../lib/side-panel.svelte';
  import SideIcon from './SideIcon.svelte';

  let {
    where,
    panel,
    badges = {},
    above,
    layers,
    page,
    background,
    sound,
    projection,
    help,
  }: {
    where: SideWhere;
    panel: SidePanelState;
    badges?: SideBadges;
    /** Shown above the open tab, whatever the tab; given only while there is something to show (§5). */
    above?: Snippet;
    layers?: Snippet;
    page?: Snippet;
    background?: Snippet;
    sound?: Snippet;
    projection?: Snippet;
    help?: Snippet;
  } = $props();

  const contents = $derived<Record<SideTabId, Snippet | undefined>>({ layers, page, background, sound, projection, help });
  /** The tabs this window gives content for, in the rail's order. */
  const available = $derived(SIDE_TABS.filter((id) => contents[id] !== undefined));
  const shown = $derived(panel.shown(available));
  let rail: HTMLElement | undefined = $state();

  /** What assistive technology reads after a tab's name: « Calques, 2 calques, un calque utilisé est masqué ». */
  function spoken(id: SideTabId, badge: SideBadge | undefined): string {
    const parts = [t(`side.tabs.${id}`)];
    if (badge?.count !== undefined && id === 'layers') parts.push(tn('side.badge.layers', badge.count));
    if (badge?.dot) parts.push(t(`side.dot.${badge.dot}`));
    return parts.join(', ');
  }

  /**
   * Roving focus on the rail: ArrowDown / ArrowUp move to the next / previous tab (wrapping), Home / End to the first /
   * last. They only move the focus (manual activation: moving must not reopen a collapsed pane). Keys with a modifier
   * are left alone (Shift + arrows pan the live preview); for the four keys the default is cancelled, and the event
   * still bubbles to the window.
   */
  function railKey(event: KeyboardEvent) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    const tabs = [...(rail?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])];
    const at = tabs.indexOf(event.currentTarget as HTMLElement);
    if (at < 0) return;
    event.preventDefault();
    const last = tabs.length - 1;
    const to = event.key === 'Home' ? 0 : event.key === 'End' ? last : (at + (event.key === 'ArrowDown' ? 1 : last)) % tabs.length;
    tabs[to].focus();
  }

  /** « › » goes with the pane it is in: the focus passes to the tab shown, so that it is not lost with the pane. */
  async function fold() {
    panel.collapse();
    await tick();
    rail?.querySelector<HTMLElement>(`[role="tab"][data-tab="${shown}"]`)?.focus();
  }
</script>

<div class="side-panel" data-side-panel={where} data-tab={shown} data-collapsed={panel.collapsed}>
  <div bind:this={rail} class="side-rail" role="tablist" aria-orientation="vertical" aria-label={t('side.tablist')}>
    {#each available as id (id)}
      {@const badge = badges[id]}
      {@const open = id === shown && !panel.collapsed}
      <button
        type="button"
        role="tab"
        class="side-tab"
        id="side-tab-{where}-{id}"
        data-tab={id}
        aria-selected={id === shown}
        aria-expanded={open}
        aria-controls="side-tabpanel-{where}-{id}"
        tabindex={id === shown ? 0 : -1}
        title={open ? t('side.openTitle', { tab: t(`side.tabs.${id}`) }) : t(`side.tabs.${id}`)}
        data-count={badge?.count}
        data-dot={badge?.dot}
        onclick={() => panel.toggle(id, available)}
        onkeydown={railKey}
      >
        <SideIcon name={id} />
        <span class="visually-hidden">{spoken(id, badge)}</span>
        {#if badge?.count !== undefined}
          <span class="side-tab-count" aria-hidden="true">{badge.count}</span>
        {/if}
        {#if badge?.dot}
          <span class="side-tab-dot" data-kind={badge.dot} aria-hidden="true"></span>
        {/if}
      </button>
    {/each}
  </div>
  <div class="side-pane" hidden={panel.collapsed}>
    <div class="side-pane-head">
      <h2 class="side-pane-title" data-slot="side-title">{t(`side.tabs.${shown}`)}</h2>
      <button
        type="button"
        class="side-fold"
        data-action="side-collapse"
        aria-label={t('side.collapse')}
        title={t('side.collapse')}
        onclick={fold}
      >
        <SideIcon name="collapse" />
      </button>
    </div>
    {#if above}
      <div class="side-above" data-slot="side-above">{@render above()}</div>
    {/if}
    <div class="side-pane-body">
      {#each available as id (id)}
        {@const content = contents[id]}
        <div
          class="side-tabpanel"
          role="tabpanel"
          id="side-tabpanel-{where}-{id}"
          aria-labelledby="side-tab-{where}-{id}"
          data-tab-panel={id}
          hidden={id !== shown}
        >
          {@render content?.()}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  /* The shell. The rail is first in the DOM (Tab reaches the tabs, then the open tab's content) and drawn right of the
     pane. */
  .side-panel {
    display: flex;
    flex-direction: row-reverse;
    height: 100%;
    min-height: 0;
    background: var(--surface);
  }

  /* app.css has no `[hidden]` rule and an author `display` beats the browser's: every display below is for the not
     hidden state only (a collapsed pane must really go, and the stage take its width). */
  .side-pane:not([hidden]) {
    display: flex;
    flex: none;
    flex-direction: column;
    width: 290px;
    min-height: 0;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }

  .side-rail {
    display: flex;
    flex: none;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    width: 48px;
    padding: 6px 0;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }

  .side-tab {
    position: relative;
    display: grid;
    flex: none;
    place-items: center;
    width: 38px;
    height: 38px;
    padding: 0;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }

  .side-tab:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .side-tab[aria-selected='true'] {
    background: var(--accent-soft);
    color: var(--accent);
  }

  /* The bar on the left of the chosen tab, at the rail's edge. */
  .side-tab[aria-selected='true']::before {
    content: '';
    position: absolute;
    top: 8px;
    bottom: 8px;
    left: -5px;
    width: 3px;
    border-radius: 2px;
    background: var(--accent);
  }

  .side-tab:focus-visible,
  .side-fold:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .side-tab :global(.side-icon) {
    width: 1.3rem;
    height: 1.3rem;
  }

  /* The badges stay visible when the pane is collapsed: that is their point. */
  .side-tab-count {
    position: absolute;
    right: 2px;
    bottom: 2px;
    min-width: 15px;
    padding: 0 3px;
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 0.65rem;
    font-variant-numeric: tabular-nums;
    font-weight: 700;
    line-height: 15px;
    text-align: center;
  }

  .side-tab-dot {
    position: absolute;
    top: 5px;
    right: 5px;
    width: 9px;
    height: 9px;
    border: 2px solid var(--surface);
    border-radius: 50%;
  }

  .side-tab-dot[data-kind='warn'] {
    background: var(--warn);
  }

  .side-tab-dot[data-kind='rec'] {
    background: var(--danger);
  }

  .side-tab-dot[data-kind='live'] {
    background: var(--accent);
  }

  .side-pane-head {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.55rem 0.6rem 0.45rem 1rem;
    border-bottom: 1px solid var(--border);
  }

  .side-pane-title {
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font-size: 0.95rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .side-fold {
    display: grid;
    flex: none;
    place-items: center;
    width: 1.9rem;
    height: 1.9rem;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }

  .side-fold:hover {
    border-color: var(--border);
    color: var(--text);
  }

  .side-fold :global(.side-icon) {
    width: 1.05rem;
    height: 1.05rem;
  }

  /* What the window shows above the open tab (the split, the selection): lightly tinted, scrolls on its own when tall. */
  .side-above {
    flex: 0 1 auto;
    max-height: 60%;
    min-height: 0;
    overflow: auto;
    border-bottom: 1px solid var(--border);
    background: var(--accent-soft);
  }

  .side-above :global(.pane-block) {
    border-bottom: 0;
  }

  .side-above :global(.pane-block + .pane-block) {
    border-top: 1px solid var(--border);
  }

  .side-pane-body {
    flex: 1 1 0;
    min-height: 0;
    overflow: auto;
  }

  @media (max-width: 900px) {
    .side-pane:not([hidden]) {
      width: 240px;
    }
  }

  /* The blocks of a tab, for the windows to use (docs/design/side-panel.md, §2 "Look"): defined here once, inside
     `.side-pane` only, so that they cannot leak (M15: a global `.panel` squashed a panel to a strip). `:where` keeps
     the specificity of the pane at zero: a block's own scoped rule wins over these defaults. */
  :where(.side-pane) :global(.pane-block) {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border);
  }

  :where(.side-pane) :global(.pane-block:last-child) {
    border-bottom: 0;
  }

  /* The discreet title of a block (an `h3`). */
  :where(.side-pane) :global(.pane-title) {
    margin: 0;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    line-height: 1.3;
    text-transform: uppercase;
  }

  /* A label on the left, its control on the right. */
  :where(.side-pane) :global(.pane-row) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    font-size: 0.875rem;
  }

  :where(.side-pane) :global(.pane-row select) {
    min-width: 0;
    max-width: 10.5rem;
    padding: 0.25rem 0.4rem;
    border-radius: 6px;
    font-size: 0.875rem;
  }

  :where(.side-pane) :global(.pane-help) {
    margin: 0;
    color: var(--muted);
    font-size: 0.8rem;
  }

  :where(.side-pane) :global(.pane-actions) {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  /* A checkbox drawn as a switch: `<input type="checkbox" role="switch" class="pane-switch">`. */
  :where(.side-pane) :global(.pane-switch) {
    position: relative;
    flex: none;
    width: 2.2rem;
    height: 1.25rem;
    margin: 0;
    border-radius: 1rem;
    background: var(--border);
    cursor: pointer;
    appearance: none;
  }

  :where(.side-pane) :global(.pane-switch::after) {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: calc(1.25rem - 4px);
    height: calc(1.25rem - 4px);
    border-radius: 50%;
    background: #fff;
    transition: left 0.15s;
  }

  :where(.side-pane) :global(.pane-switch:checked) {
    background: var(--accent);
  }

  :where(.side-pane) :global(.pane-switch:checked::after) {
    left: calc(100% - 1.25rem + 2px);
  }

  :where(.side-pane) :global(.pane-switch:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  :where(.side-pane) :global(.pane-switch:disabled) {
    opacity: 0.5;
    cursor: default;
  }

  /* Round colour pastilles, their name under: `<button class="pane-swatch" style="--swatch: rgb(…)" aria-pressed>`. */
  :where(.side-pane) :global(.pane-swatches) {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  :where(.side-pane) :global(.pane-swatch) {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.15rem;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 0.72rem;
    cursor: pointer;
  }

  /* The pastille is the colour the renderer paints the paper with, ringed with the border colour (dark theme too). */
  :where(.side-pane) :global(.pane-swatch::before) {
    content: '';
    display: block;
    width: 1.9rem;
    height: 1.9rem;
    border: 1px solid var(--border);
    border-radius: 50%;
    background: var(--swatch, transparent);
  }

  :where(.side-pane) :global(.pane-swatch[aria-pressed='true']) {
    color: var(--text);
  }

  :where(.side-pane) :global(.pane-swatch[aria-pressed='true']::before) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  :where(.side-pane) :global(.pane-swatch:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  :where(.side-pane) :global(.pane-swatch:disabled) {
    cursor: default;
  }

  /* A slider and its value: `<input type="range" class="pane-range"> <span class="pane-value">80 %</span>`. */
  :where(.side-pane) :global(.pane-range),
  :where(.side-pane) :global(.pane-row input[type='range']) {
    flex: 1;
    min-width: 0;
    accent-color: var(--accent);
  }

  :where(.side-pane) :global(.pane-value) {
    min-width: 3rem;
    color: var(--muted);
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  /* The help's lists and keys. */
  :where(.side-pane) :global(dl.keys) {
    display: grid;
    grid-template-columns: fit-content(40%) 1fr;
    gap: 0.3rem 0.75rem;
    margin: 0;
    font-size: 0.82rem;
  }

  :where(.side-pane) :global(dl.keys dd) {
    margin: 0;
    color: var(--muted);
  }

  :where(.side-pane) :global(kbd) {
    padding: 0 0.3rem;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 4px;
    background: var(--surface);
    font: inherit;
    font-size: 0.78rem;
  }

  @media (prefers-reduced-motion: reduce) {
    :where(.side-pane) :global(.pane-switch::after) {
      transition: none;
    }
  }
</style>
