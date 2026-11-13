<script lang="ts" module>
  export type TabletTabId = 'use' | 'settings' | 'tablets';

  export interface TabletTab {
    id: TabletTabId;
    /** The name, read by assistive technology and shown as the tooltip. */
    label: string;
    /** What a badge says in words (« 3 pages en attente »), read after the name. */
    spoken?: string;
    /** The number on the badge; none when 0 or absent. */
    count?: number;
  }
</script>

<script lang="ts">
  /**
   * The rail of icon tabs of the tablet panel (Q17), drawn as the editor's side panel rail (components/side/SidePanel.svelte,
   * docs/design/side-panel.md §2) and behaving like it: WAI-ARIA tabs with manual activation (the arrow keys, Home and
   * End move the focus along the rail, Enter or a click chooses). That rail is tied to the editor's own tabs, so the
   * look is repeated here, with its own class names (`tp-*`). Each tab controls the parent's element
   * `tablet-tabpanel-<id>`.
   */
  let {
    tabs,
    selected,
    label,
    onselect,
  }: {
    tabs: TabletTab[];
    selected: TabletTabId;
    label: string;
    onselect: (id: TabletTabId) => void;
  } = $props();

  let rail: HTMLElement | undefined = $state();

  /**
   * Roving focus: ArrowDown / ArrowUp move to the next / previous tab (wrapping), Home / End to the first / last. Keys with
   * a modifier are left alone.
   */
  function railKey(event: KeyboardEvent) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    const buttons = [...(rail?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])];
    const at = buttons.indexOf(event.currentTarget as HTMLElement);
    if (at < 0) return;
    event.preventDefault();
    const last = buttons.length - 1;
    const to = event.key === 'Home' ? 0 : event.key === 'End' ? last : (at + (event.key === 'ArrowDown' ? 1 : last)) % buttons.length;
    buttons[to].focus();
  }
</script>

<div bind:this={rail} class="tp-rail" role="tablist" aria-orientation="vertical" aria-label={label}>
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      role="tab"
      class="tp-tab"
      id="tablet-tab-{tab.id}"
      data-tab={tab.id}
      data-count={tab.count ? tab.count : undefined}
      aria-selected={tab.id === selected}
      aria-controls="tablet-tabpanel-{tab.id}"
      tabindex={tab.id === selected ? 0 : -1}
      title={tab.label}
      onclick={() => onselect(tab.id)}
      onkeydown={railKey}
    >
      <svg
        class="tp-icon"
        viewBox="0 0 20 20"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        {#if tab.id === 'use'}
          <path d="M4 16l1-4 8-8 3 3-8 8zM11.5 5.5l3 3" />
        {:else if tab.id === 'settings'}
          <path d="M3 6h8M15 6h2M3 14h2M9 14h8" />
          <circle cx="13" cy="6" r="2" />
          <circle cx="7" cy="14" r="2" />
        {:else}
          <rect x="3" y="3" width="9" height="12" rx="1.5" />
          <path d="M14.5 6H16a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1v-.5" />
        {/if}
      </svg>
      <span class="visually-hidden">{tab.spoken ? `${tab.label}, ${tab.spoken}` : tab.label}</span>
      {#if tab.count}
        <span class="tp-count" aria-hidden="true">{tab.count}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .tp-rail {
    display: flex;
    flex: none;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    width: 46px;
    padding: 6px 0;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }

  .tp-tab {
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

  .tp-tab:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .tp-tab[aria-selected='true'] {
    background: var(--accent-soft);
    color: var(--accent);
  }

  /* The bar on the left of the chosen tab, at the rail's edge. */
  .tp-tab[aria-selected='true']::before {
    content: '';
    position: absolute;
    top: 8px;
    bottom: 8px;
    left: -4px;
    width: 3px;
    border-radius: 2px;
    background: var(--accent);
  }

  .tp-tab:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .tp-icon {
    width: 1.3rem;
    height: 1.3rem;
  }

  /* The badge stays on the icon whatever tab is open: that is its point. */
  .tp-count {
    position: absolute;
    top: 1px;
    right: 1px;
    min-width: 15px;
    padding: 0 3px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 0.65rem;
    font-variant-numeric: tabular-nums;
    font-weight: 700;
    line-height: 15px;
    text-align: center;
  }
</style>
