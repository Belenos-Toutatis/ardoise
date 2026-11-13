<script lang="ts">
  /**
   * Side panel of the tablet (SPEC §4.1, Q17): a narrow panel with three icon tabs on a rail, as the editor's side panel
   * has. « Use » (`TabletUse`): the connected tablet, live mode, sync, and what needs saying about the state.
   * « Settings » (`TabletSettings`): those of the tablet chosen in a list, the connected one to begin with. « Tablets »
   * (`KnownTablets`): the known tablets with what they reported, each with its own « Forget », and the way to add the
   * one that is missing. A badge on the first tab says how many pages wait on the tablet. The failure of an action is
   * above every tab. Every tab stays mounted (hidden when not shown), so that what one holds survives a switch.
   * Follows the window's shared status (`tablet-state.svelte.ts`).
   */
  import { onMount } from 'svelte';
  import { t, tn } from '../../lib/i18n.svelte';
  import { pagesOnTablet, tablet } from '../../lib/tablet-state.svelte';
  import KnownTablets from './KnownTablets.svelte';
  import TabletRail, { type TabletTab, type TabletTabId } from './TabletRail.svelte';
  import TabletSettings from './TabletSettings.svelte';
  import TabletUse from './TabletUse.svelte';

  let { onclose, onlive, ontest }: { onclose: () => void; onlive: () => void; ontest: () => void } = $props();

  onMount(() => tablet.start());

  const TAB_IDS: TabletTabId[] = ['use', 'settings', 'tablets'];

  let tab = $state<TabletTabId>('use');

  const status = $derived(tablet.status);
  const connection = $derived(status?.connection ?? 'unavailable');
  /** Pages that wait on the connected tablet. */
  const waiting = $derived(pagesOnTablet(status));
  /** The guided test is the Slate's: the blackboard has none. */
  const canTest = $derived(!tablet.board || tablet.slate !== null);
  const tabs = $derived<TabletTab[]>(
    TAB_IDS.map((id) => ({
      id,
      label: t(`tablet.tabs.${id}`),
      count: id === 'use' ? waiting : undefined,
      spoken: id === 'use' && waiting > 0 ? tn('tablet.tabs.waiting', waiting) : undefined,
    })),
  );
</script>

{#snippet testButton()}
  {#if canTest}
    <div class="actions tp-foot">
      <button type="button" class="button" data-action="test" onclick={ontest}>{t('settings.tabletTest')}</button>
    </div>
  {/if}
{/snippet}

<aside class="tablet-panel" aria-label={t('tablet.title')} data-connection={connection} data-tab={tab}>
  {#if status !== null}
    <TabletRail {tabs} selected={tab} label={t('tablet.tablist')} onselect={(id) => (tab = id)} />
  {/if}
  <div class="tp-pane">
    <header class="tp-head">
      <h2 class="tp-title" data-slot="tablet-title">{status === null ? t('tablet.title') : t(`tablet.tabs.${tab}`)}</h2>
      <button type="button" class="button ghost" onclick={onclose}>{t('common.close')}</button>
    </header>

    {#if tablet.error}
      <div class="banner error tp-error" role="alert" data-slot="tablet-error">
        <span>{tablet.error}</span>
        {#if tablet.actionError}
          <button type="button" class="button ghost" onclick={() => tablet.dismissError()}>{t('library.dismiss')}</button>
        {/if}
      </div>
    {/if}

    <div class="tp-body">
      {#if status === null}
        <div class="tp-page">
          <p class="muted" data-slot="loading">{tablet.unavailable ? t('tablet.unavailableStatus') : t('tablet.loading')}</p>
          {@render testButton()}
        </div>
      {:else}
        <div class="tp-page" role="tabpanel" id="tablet-tabpanel-use" aria-labelledby="tablet-tab-use" data-tab-panel="use" hidden={tab !== 'use'}>
          <TabletUse {onlive} />
        </div>
        <div
          class="tp-page"
          role="tabpanel"
          id="tablet-tabpanel-settings"
          aria-labelledby="tablet-tab-settings"
          data-tab-panel="settings"
          hidden={tab !== 'settings'}
        >
          <TabletSettings compact />
        </div>
        <div
          class="tp-page"
          role="tabpanel"
          id="tablet-tabpanel-tablets"
          aria-labelledby="tablet-tab-tablets"
          data-tab-panel="tablets"
          hidden={tab !== 'tablets'}
        >
          <KnownTablets readings />
          {@render testButton()}
        </div>
      {/if}
    </div>
  </div>
</aside>

<style>
  /* Where the global `.panel` put it (over the right edge of the library), but narrow, without its padding and its scroll:
     the rail and the pane carry their own. The rail is first in the DOM (Tab reaches the tabs, then the open tab's
     content) and drawn on the right. */
  .tablet-panel {
    position: absolute;
    inset: 0 0 0 auto;
    display: flex;
    flex-direction: row-reverse;
    width: min(310px, 100%);
    overflow: hidden;
    border-left: 1px solid var(--border);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .tp-pane {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .tp-head {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.55rem 0.5rem 0.45rem 1rem;
    border-bottom: 1px solid var(--border);
  }

  .tp-title {
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font-size: 0.95rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tp-error {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    margin: 0.75rem 1rem 0;
    padding: 0.5rem 0.75rem;
    font-size: 0.875rem;
  }

  .tp-body {
    flex: 1 1 0;
    min-height: 0;
    overflow: auto;
  }

  /* app.css has no `[hidden]` rule: say it here, whatever display a tab's content gives itself. */
  .tp-page[hidden] {
    display: none;
  }

  .tp-page {
    padding: 0.8rem 1rem 0.9rem;
  }

  .tp-foot {
    margin-top: 0.9rem;
  }
</style>
