<script lang="ts">
  /**
   * The tablets Ardoise knows (the Bamboo Slate and the Xiaomi blackboard, either or both), each
   * with its state and its own « Forget » with the confirmation the Slate always had, and the way
   * to add the one that is missing (the Slate's registration wizard, the blackboard's search). Shared by the tablet panel and the settings.
   * Nothing here assumes a Slate: the list is `status.known`. The panel's « Tablets » tab adds what the connected tablet
   * reported (`readings`), the settings the address and the date (`detailed`).
   */
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { formatDate } from '../../lib/library-logic';
  import type { TabletKind } from '../../lib/tablet';
  import { formatBytes, sizeInMillimetres, tablet, tabletName } from '../../lib/tablet-state.svelte';
  import AddBlackboard from './AddBlackboard.svelte';
  import Battery from './Battery.svelte';
  import RegisterWizard from './RegisterWizard.svelte';

  let {
    detailed = false,
    emptyText = false,
    readings = false,
    onerror = () => {},
  }: {
    /** The address, the date and the size of each tablet (the settings). */
    detailed?: boolean;
    /** Says that no tablet is known, instead of showing nothing (the settings). */
    emptyText?: boolean;
    /** What the tablet reported at its last connection: battery, memory or firmware, pages, model, size, date (the panel). */
    readings?: boolean;
    /** Told the translated failure of a forget (or null when it starts over). */
    onerror?: (message: string | null) => void;
  } = $props();

  let confirming = $state<TabletKind | null>(null);
  /** The flow that is open to add a tablet. */
  let adding = $state<TabletKind | null>(null);

  const status = $derived(tablet.status);
  const known = $derived(tablet.known);
  const connection = $derived(status?.connection ?? 'unavailable');
  /** What the last connected tablet said about itself (it is about one tablet only: see `device.kind`). */
  const device = $derived(status?.device ?? null);
  const addBlocked = $derived(
    connection === 'registering' ||
      connection === 'adding' ||
      connection === 'syncing' ||
      connection === 'live' ||
      (connection === 'unavailable' && status?.error?.code === 'noLibrary'),
  );

  // A tablet that is gone has no question left.
  $effect(() => {
    if (confirming && !known.some((entry) => entry.kind === confirming)) confirming = null;
  });

  async function forget(kind: TabletKind) {
    confirming = null;
    onerror(null);
    if (!(await tablet.forgetKind(kind))) onerror(tablet.actionError);
  }
</script>

<div class="known-tablets" data-slot="known-tablets">
  {#each known as entry (entry.kind)}
    {@const read = readings && device !== null && device.kind === entry.kind ? device : null}
    <div class="known" data-slot="known-tablet" data-kind={entry.kind} data-connected={tablet.active === entry.kind}>
      <p class="known-head">
        <span class="known-dot" aria-hidden="true"></span>
        <strong>{tabletName(entry)}</strong>
        <span class="muted small" data-slot="known-state">
          {tablet.active === entry.kind ? t(`tablet.state.${connection}`) : t('tablet.known.notConnected')}
        </span>
      </p>
      {#if detailed}
        <dl class="facts">
          {#if entry.kind === 'blackboard'}
            <dt>{t('tablet.info.model')}</dt>
            <dd>{entry.model}</dd>
          {/if}
          <dt>{t('settings.tabletAddress')}</dt>
          <dd><code>{entry.address}</code></dd>
          <dt>{entry.kind === 'blackboard' ? t('settings.tabletAdded') : t('settings.tabletRegistered')}</dt>
          <dd>{formatDate(Date.parse(entry.added), currentLocale())}</dd>
          <dt>{t('settings.tabletSize')}</dt>
          <dd>{sizeInMillimetres(entry.width, entry.height)}</dd>
        </dl>
      {:else if readings}
        <dl class="facts compact">
          <dt>{t('tablet.info.model')}</dt>
          <dd>{entry.model}</dd>
          {#if read}
            <dt>{t('tablet.info.battery')}</dt>
            <dd><Battery percent={read.batteryPercent} charging={read.kind === 'blackboard' ? false : read.charging} /></dd>
            {#if read.kind === 'blackboard' && read.memory}
              <dt>{t('tablet.info.memory')}</dt>
              <dd data-slot="known-memory">
                {t('tablet.info.memoryValue', {
                  free: formatBytes(read.memory.free, currentLocale()),
                  total: formatBytes(read.memory.total, currentLocale()),
                })}
              </dd>
            {/if}
            {#if read.kind === 'slate'}
              <dt>{t('tablet.info.firmware')}</dt>
              <dd data-slot="known-firmware">{read.firmware}</dd>
            {/if}
            <dt>{t('tablet.info.pages')}</dt>
            <dd data-slot="known-pages">{read.storedPages}</dd>
          {/if}
          <dt>{t('tablet.info.size')}</dt>
          <dd>{sizeInMillimetres(entry.width, entry.height)}</dd>
          {#if read}
            <dt>{t('tablet.info.readAt')}</dt>
            <dd data-slot="known-read-at">{formatDate(read.readAt, currentLocale())}</dd>
          {/if}
        </dl>
        {#if read && read.kind === 'blackboard'}
          <p class="muted small" data-slot="board-clock">{t('tablet.info.clockAuto')}</p>
        {/if}
      {:else}
        <p class="muted small">{entry.model}</p>
      {/if}
      {#if confirming === entry.kind}
        <div class="confirm" role="alertdialog" aria-label={t('settings.tabletForget')}>
          <p>
            {entry.kind === 'blackboard'
              ? t('settings.boardForgetConfirm')
              : t('settings.tabletForgetConfirm', { name: entry.name })}
          </p>
          <div class="actions">
            <button type="button" class="button danger" data-action="forget-yes" data-kind={entry.kind} onclick={() => forget(entry.kind)}>
              {t('settings.tabletForgetYes')}
            </button>
            <button type="button" class="button" data-action="forget-no" onclick={() => (confirming = null)}>
              {t('settings.cancel')}
            </button>
          </div>
        </div>
      {:else}
        <div class="actions">
          <button
            type="button"
            class="button danger"
            data-action="forget"
            data-kind={entry.kind}
            aria-label={t('tablet.known.forgetLabel', { name: tabletName(entry) })}
            onclick={() => (confirming = entry.kind)}
          >
            {t('settings.tabletForget')}
          </button>
        </div>
      {/if}
    </div>
  {:else}
    {#if emptyText && status}
      <p class="muted">{t('settings.tabletNone')}</p>
    {/if}
  {/each}

  {#if readings && known.length > 0 && !device}
    <p class="muted small" data-slot="known-not-read">{t('tablet.info.notRead')}</p>
  {/if}

  {#if !tablet.slate || !tablet.board}
    <!-- The tablet that is missing: one button each, the Slate's flow being the one it always was. -->
    <div class="add" data-slot="add-tablet">
      <p class="add-title">{t('tablet.known.add')}</p>
      <div class="actions">
        {#if !tablet.slate}
          <button type="button" class="button" data-action="register" disabled={addBlocked} onclick={() => (adding = 'slate')}>
            {t('tablet.register.open')}
          </button>
        {/if}
        {#if !tablet.board}
          <button type="button" class="button" data-action="add-blackboard" disabled={addBlocked} onclick={() => (adding = 'blackboard')}>
            {t('tablet.board.open')}
          </button>
        {/if}
      </div>
    </div>
  {/if}
</div>

{#if adding === 'slate'}
  <RegisterWizard onclose={() => (adding = null)} />
{:else if adding === 'blackboard'}
  <AddBlackboard onclose={() => (adding = null)} />
{/if}

<style>
  .known {
    margin: 0 0 0.9rem;
  }

  .known-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.5rem;
    margin: 0;
  }

  /* A dot as the panel's state line has: green for the connected tablet, grey for the others. */
  .known-dot {
    flex: none;
    align-self: center;
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
    background: var(--muted);
  }

  [data-connected='true'] .known-dot {
    background: #2fa866;
  }

  .known p.muted {
    margin: 0.1rem 0 0.3rem;
  }

  .compact {
    gap: 0.15rem 0.75rem;
    margin: 0.3rem 0 0.4rem;
    font-size: 0.8125rem;
  }

  .add {
    margin-top: 0.5rem;
  }

  .add-title {
    margin: 0 0 0.4rem;
    font-size: 0.875rem;
    font-weight: 600;
  }
</style>
