<script lang="ts">
  /**
   * The tabbed side panel of the editor and of the live control window (milestone M16; contract:
   * docs/design/side-panel.md, §2): the rail of icon tabs on the right edge and, left of it, the pane that shows the open
   * tab. A window gives a tab's content as a snippet of the same name, declared as a direct child of `<SidePanel>`: a
   * tab exists in a window if and only if its snippet is given. The window owns the `SidePanelState` (created with
   * `new SidePanelState(where)`) and calls `panel.open(…)`; the component calls `panel.toggle` on a tab and
   * `panel.collapse` on « › », and emits no event.
   *
   * Package 0's skeleton: the final props and the DOM contract (attributes, hooks, `hidden`), no style and no arrow
   * keys. Every tab panel stays mounted (`hidden` when not shown), so that what a block holds survives a tab switch.
   */
  import type { Snippet } from 'svelte';
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

  /** What assistive technology reads after a tab's name: « Calques, 2 calques, un calque utilisé est masqué ». */
  function spoken(id: SideTabId, badge: SideBadge | undefined): string {
    const parts = [t(`side.tabs.${id}`)];
    if (badge?.count !== undefined && id === 'layers') parts.push(tn('side.badge.layers', badge.count));
    if (badge?.dot) parts.push(t(`side.dot.${badge.dot}`));
    return parts.join(', ');
  }
</script>

<div class="side-panel" data-side-panel={where} data-tab={shown} data-collapsed={panel.collapsed}>
  <div class="side-rail" role="tablist" aria-orientation="vertical" aria-label={t('side.tablist')}>
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
        onclick={() => panel.collapse()}
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
