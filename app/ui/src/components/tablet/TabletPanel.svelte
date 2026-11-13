<script lang="ts">
  /**
   * Side panel of the tablet (SPEC §4.1): the known tablets (the Bamboo Slate and the Xiaomi
   * blackboard, each with its own « Forget »), connection and information, sync now (with progress
   * and cancel; for the blackboard with the warning that its pages are deleted), live mode, the
   * options, the Slate's own settings (name, clock) and the way to add a tablet when none is
   * known. Follows the window's shared status (`tablet-state.svelte.ts`).
   */
  import { onMount, untrack } from 'svelte';
  import { errorMessage } from '../../lib/api';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { formatDate } from '../../lib/library-logic';
  import {
    formatBytes,
    sizeInMillimetres,
    blackboardNeedsConfirmation,
    blackboardWaitsForRequest,
    tablet,
    validTabletName,
  } from '../../lib/tablet-state.svelte';
  import AddBlackboard from './AddBlackboard.svelte';
  import Battery from './Battery.svelte';
  import KnownTablets from './KnownTablets.svelte';
  import RegisterWizard from './RegisterWizard.svelte';
  import TabletOptions from './TabletOptions.svelte';

  let { onclose, onlive, ontest }: { onclose: () => void; onlive: () => void; ontest: () => void } = $props();

  onMount(() => tablet.start());

  /** The flow open to add the first tablet. */
  let wizardOpen = $state<'slate' | 'blackboard' | null>(null);
  /** An explicit sync of the blackboard waits for the user's word: it deletes the pages. */
  let confirmingSync = $state(false);
  /** The name as being edited. */
  let draft = $state(untrack(() => tablet.status?.registered?.name ?? ''));
  /** The name was submitted and refused for its form. */
  let nameRefused = $state(false);

  const status = $derived(tablet.status);
  const connection = $derived(status?.connection ?? 'unavailable');
  /** The Slate, when one is registered (null when only the blackboard is known). */
  const registered = $derived(status?.registered ?? null);
  const registeredName = $derived(registered?.name ?? '');
  const device = $derived(status?.device ?? null);
  /** What the tablet read last is the blackboard's. */
  const boardDevice = $derived(device?.kind === 'blackboard');
  const hasTablet = $derived(tablet.known.length > 0);
  /** The tablet whose size is shown until one is read. */
  const firstKnown = $derived(tablet.known[0] ?? null);
  /**
   * The Xiaomi is the connected tablet and « Delete after import » is off: its pages are imported only on request, after
   * the user accepted that they are deleted from the tablet. Nothing is said while the Slate or no tablet is connected.
   */
  const warnBoard = $derived(blackboardNeedsConfirmation(status));
  /** Only the Xiaomi is known, it is not connected and its pages are imported on request: nothing to sync yet. */
  const boardOnRequest = $derived(blackboardWaitsForRequest(status));
  /** Pages the blackboard keeps until the user imports them. */
  const boardWaiting = $derived(warnBoard && connection === 'connected' ? (device?.storedPages ?? 0) : 0);
  const sync = $derived(status?.sync ?? null);
  const lastSync = $derived(status?.lastSync ?? null);
  /**
   * Pages left on the tablet after a sync that deleted nothing: the tablet hands over only its
   * oldest page, the next one is reachable once that one is deleted.
   */
  const pagesWaiting = $derived(
    lastSync &&
    lastSync.deleted === 0 &&
    status?.options.deleteAfterImport === false &&
    connection !== 'syncing' &&
    !boardDevice
      ? (device?.storedPages ?? 0)
      : 0,
  );
  const name = $derived(device?.name ?? registered?.name ?? '');
  const canSync = $derived(connection === 'connected' || connection === 'searching' || connection === 'connecting');
  const canLive = $derived(connection !== 'syncing' && connection !== 'registering' && connection !== 'adding');
  /** What to do to connect the tablet that is waited for. */
  const searchingHint = $derived(
    tablet.slate && tablet.board
      ? 'tablet.hint.searchingBoth'
      : tablet.board
        ? 'tablet.hint.searchingBoard'
        : 'tablet.hint.searching',
  );
  const hint = $derived(
    status?.pending === 'sync'
      ? 'tablet.hint.pendingSync'
      : status?.pending === 'live'
        ? 'tablet.hint.pendingLive'
        : connection === 'searching'
          ? searchingHint
          : connection === 'adding'
            ? 'tablet.hint.adding'
            : null,
  );
  const drift = $derived(
    tablet.drift === null
      ? null
      : new Intl.NumberFormat(currentLocale(), { maximumFractionDigits: 1 }).format(Math.abs(tablet.drift)),
  );

  // A new name (this PC's rename, or another tablet) replaces what is in the field.
  $effect(() => {
    draft = registeredName;
    nameRefused = false;
  });

  // The question has no meaning once the sync cannot start, or once the Xiaomi is no longer the connected tablet (the
  // « Sync and delete » that followed would send no flag and wait for nothing).
  $effect(() => {
    if (!canSync || !warnBoard) confirmingSync = false;
  });

  // The worker asked for the confirmation itself (the Xiaomi connected between the click and the request).
  $effect(() => {
    if (tablet.confirmDeleteAsked && warnBoard && canSync) confirmingSync = true;
  });

  async function rename(event: SubmitEvent) {
    event.preventDefault();
    nameRefused = !validTabletName(draft);
    if (!nameRefused) await tablet.rename(draft);
  }

  /** The blackboard always deletes the pages it hands over: say so first when the option is off. */
  async function syncNow() {
    if (warnBoard) {
      confirmingSync = true;
      return;
    }
    // The Xiaomi connected meanwhile: the service asks for the confirmation itself.
    if (!(await tablet.sync()) && tablet.actionCode === 'blackboardConfirmDelete') {
      tablet.dismissError();
      confirmingSync = true;
    }
  }

  async function syncConfirmed() {
    confirmingSync = false;
    // The flag is for the Xiaomi only, and only after this confirmation.
    await tablet.sync(blackboardNeedsConfirmation(tablet.status));
  }

  function savedText(count: number): string {
    if (count === 0) return t('tablet.sync.savedNone');
    return count === 1 ? t('tablet.sync.savedOne') : t('tablet.sync.savedMany', { count });
  }

  function deletedText(count: number): string {
    return count === 1 ? t('tablet.sync.deletedOne') : t('tablet.sync.deletedMany', { count });
  }
</script>

<aside class="panel tablet-panel" aria-label={t('tablet.title')} data-connection={connection}>
  <header class="panel-header">
    <h2>{t('tablet.title')}</h2>
    <button type="button" class="button ghost" onclick={onclose}>{t('common.close')}</button>
  </header>

  {#if status?.simulated}
    <p class="banner notice simulated" role="status" data-slot="simulated">{t('tablet.simulated')}</p>
  {/if}

  {#if tablet.error}
    <div class="banner error" role="alert" data-slot="tablet-error">
      <span>{tablet.error}</span>
      {#if tablet.actionError}
        <button type="button" class="button ghost" onclick={() => tablet.dismissError()}>{t('library.dismiss')}</button>
      {/if}
    </div>
  {/if}

  {#if status === null}
    <p class="muted" data-slot="loading">{tablet.unavailable ? t('tablet.unavailableStatus') : t('tablet.loading')}</p>
  {:else if !hasTablet}
    <section class="block" aria-labelledby="tablet-register">
      <h3 id="tablet-register">{t('tablet.register.none')}</h3>
      <p class="muted">{t('tablet.register.intro')}</p>
      <div class="actions">
        <button
          type="button"
          class="button primary"
          data-action="register"
          disabled={(connection === 'unavailable' && status.error?.code === 'noLibrary') || connection === 'adding'}
          onclick={() => (wizardOpen = 'slate')}
        >
          {t('tablet.register.open')}
        </button>
        <button
          type="button"
          class="button primary"
          data-action="add-blackboard"
          disabled={(connection === 'unavailable' && status.error?.code === 'noLibrary') || connection === 'adding'}
          onclick={() => (wizardOpen = 'blackboard')}
        >
          {t('tablet.board.open')}
        </button>
      </div>
      {#if connection === 'adding'}
        <p class="muted small" data-slot="hint">{t('tablet.hint.adding')}</p>
        <div class="actions">
          <button type="button" class="button" data-action="cancel-search" onclick={() => tablet.cancel()}>
            {t('tablet.board.cancel')}
          </button>
        </div>
      {/if}
    </section>
  {:else}
    <section class="block" aria-labelledby="tablet-known">
      <h3 id="tablet-known">{t('tablet.known.title')}</h3>
      <KnownTablets />
    </section>

    <section class="block" aria-labelledby="tablet-info">
      <h3 id="tablet-info">{t('tablet.info.title')}</h3>
      <p class="state" data-slot="state">
        <span class="dot" aria-hidden="true"></span>
        <strong>{t(`tablet.state.${connection}`)}</strong>
      </p>
      {#if hint}
        <p class="muted small" data-slot="hint">{t(hint)}</p>
      {/if}
      {#if connection === 'adding'}
        <div class="actions">
          <button type="button" class="button" data-action="cancel-search" onclick={() => tablet.cancel()}>
            {t('tablet.board.cancel')}
          </button>
        </div>
      {/if}
      <dl class="facts" data-kind={device?.kind ?? null}>
        {#if device}
          {#if !boardDevice}
            <dt>{t('tablet.info.name')}</dt>
            <dd data-slot="name">{name}</dd>
          {/if}
          <dt>{t('tablet.info.battery')}</dt>
          <dd><Battery percent={device.batteryPercent} charging={boardDevice ? false : device.charging} /></dd>
          {#if boardDevice && device.memory}
            <dt>{t('tablet.info.memory')}</dt>
            <dd data-slot="memory">
              {t('tablet.info.memoryValue', {
                free: formatBytes(device.memory.free, currentLocale()),
                total: formatBytes(device.memory.total, currentLocale()),
              })}
            </dd>
          {/if}
          {#if !boardDevice}
            <dt>{t('tablet.info.firmware')}</dt>
            <dd>{device.firmware}</dd>
          {/if}
          <dt>{t('tablet.info.pages')}</dt>
          <dd data-slot="stored-pages">{device.storedPages}</dd>
          <dt>{t('tablet.info.model')}</dt>
          <dd>{device.model}</dd>
          <dt>{t('tablet.info.size')}</dt>
          <dd>{sizeInMillimetres(device.width, device.height)}</dd>
          <dt>{t('tablet.info.readAt')}</dt>
          <dd>{formatDate(device.readAt, currentLocale())}</dd>
        {:else}
          {#if registered}
            <dt>{t('tablet.info.name')}</dt>
            <dd data-slot="name">{name}</dd>
          {/if}
          {#if firstKnown}
            <dt>{t('tablet.info.size')}</dt>
            <dd>{sizeInMillimetres(firstKnown.width, firstKnown.height)}</dd>
          {/if}
        {/if}
      </dl>
      {#if !device}
        <p class="muted small">{t('tablet.info.notRead')}</p>
      {:else if boardDevice}
        <p class="muted small" data-slot="board-clock">{t('tablet.info.clockAuto')}</p>
      {/if}
    </section>

    <section class="block" aria-labelledby="tablet-sync">
      <h3 id="tablet-sync">{t('tablet.sync.title')}</h3>
      {#if connection === 'syncing'}
        <div class="syncing" data-slot="sync-progress">
          <progress
            class="bar"
            max={Math.max(sync?.total ?? 1, 1)}
            value={sync?.current ?? 0}
            aria-label={t('tablet.sync.progressLabel')}
          ></progress>
          {#if sync}
            <p class="small" role="status">{t('tablet.sync.progress', { current: sync.current, total: sync.total })}</p>
          {/if}
        </div>
        <div class="actions">
          <button type="button" class="button" data-action="cancel-sync" onclick={() => tablet.cancel()}>
            {t('tablet.sync.cancel')}
          </button>
        </div>
      {:else if confirmingSync}
        <div class="confirm" role="alertdialog" aria-label={t('tablet.sync.boardTitle')} data-slot="sync-confirm">
          <p><strong>{t('tablet.sync.boardTitle')}</strong></p>
          <p>{t('tablet.sync.boardConfirm')}</p>
          <div class="actions">
            <button type="button" class="button primary" data-action="sync-confirm" data-autofocus onclick={syncConfirmed}>
              {t('tablet.sync.boardConfirmYes')}
            </button>
            <button type="button" class="button" data-action="sync-cancel" onclick={() => (confirmingSync = false)}>
              {t('settings.cancel')}
            </button>
          </div>
        </div>
      {:else}
        <div class="actions">
          <button type="button" class="button primary" data-action="sync" disabled={!canSync || boardOnRequest} onclick={syncNow}>
            {t('tablet.sync.now')}
          </button>
        </div>
        {#if boardOnRequest}
          <p class="muted small" data-slot="board-on-request">{t('tablet.sync.boardOnRequest')}</p>
        {/if}
      {/if}
      {#if lastSync?.error}
        <p class="small last-sync failed error" role="alert" data-slot="last-sync" data-failed="true">
          <span>
            {t('tablet.sync.failed', {
              date: formatDate(lastSync.at, currentLocale()),
              reason: errorMessage(lastSync.error),
            })}
          </span>
          {#if lastSync.saved.length > 0}<span>{savedText(lastSync.saved.length)}</span>{/if}
          {#if lastSync.deleted > 0}<span>{deletedText(lastSync.deleted)}</span>{/if}
          <span>{t('tablet.sync.failedKept')}</span>
        </p>
      {:else}
        <p class="muted small last-sync" data-slot="last-sync">
          {#if lastSync}
            {t('tablet.sync.last', { date: formatDate(lastSync.at, currentLocale()) })}
            {savedText(lastSync.saved.length)}
            {#if lastSync.deleted > 0}{deletedText(lastSync.deleted)}{/if}
          {:else}
            {t('tablet.sync.never')}
          {/if}
        </p>
      {/if}
      {#if boardWaiting > 0}
        <p class="banner notice" role="status" data-slot="board-waiting">
          {boardWaiting === 1 ? t('tablet.sync.boardWaitingOne') : t('tablet.sync.boardWaitingMany', { count: boardWaiting })}
        </p>
      {/if}
      {#if pagesWaiting > 1}
        <p class="banner notice" role="status" data-slot="pages-waiting">
          {t('tablet.sync.pagesWaiting', { count: pagesWaiting, option: t('tablet.options.deleteAfterImport') })}
        </p>
      {/if}
    </section>

    <section class="block" aria-labelledby="tablet-live">
      <h3 id="tablet-live">{t('tablet.live.open')}</h3>
      <p class="muted small">{t('tablet.live.hint')}</p>
      <div class="actions">
        <button type="button" class="button" data-action="live" disabled={!canLive} onclick={onlive}>
          {t('tablet.live.open')}
        </button>
      </div>
    </section>

    <section class="block" aria-labelledby="tablet-options">
      <h3 id="tablet-options">{t('tablet.options.title')}</h3>
      <TabletOptions />
    </section>

    {#if registered && !boardDevice}
      <!-- The Slate's own settings: the blackboard has no name to change, and its clock is set at each connection. -->
      <section class="block" aria-labelledby="tablet-manage">
        <h3 id="tablet-manage">{t('tablet.manage.title')}</h3>
        <form class="field" onsubmit={rename}>
          <label for="tablet-name"><span>{t('tablet.manage.name')}</span></label>
          <div class="inline">
            <input
              id="tablet-name"
              class="text-input"
              type="text"
              bind:value={draft}
              maxlength="40"
              aria-invalid={nameRefused}
              aria-describedby="tablet-name-hint"
              oninput={() => (nameRefused = false)}
            />
            <button
              type="submit"
              class="button"
              data-action="rename"
              disabled={tablet.working || draft.trim() === registered.name}
            >
              {t('tablet.manage.rename')}
            </button>
          </div>
          <span id="tablet-name-hint" class="muted small field-hint">{t('tablet.manage.nameHint')}</span>
          {#if nameRefused}
            <span class="error small" role="alert">{t('tablet.manage.nameInvalid')}</span>
          {:else if tablet.renamed}
            <span class="muted small" role="status">{t('tablet.manage.renamed')}</span>
          {/if}
        </form>

        <div class="field">
          <span class="muted small">{t('tablet.manage.timeHint')}</span>
          <button type="button" class="button" data-action="set-time" disabled={tablet.working} onclick={() => tablet.setTime()}>
            {t('tablet.manage.setTime')}
          </button>
          {#if drift !== null}
            <span class="muted small" role="status" data-slot="time-result">{t('tablet.manage.timeSet', { seconds: drift })}</span>
          {/if}
        </div>
      </section>
    {/if}

    {#if status.simulated}
      <section class="block" aria-label={t('tablet.simulated')}>
        <!-- The simulated tablet has a button too: used by the wizard and by live mode. -->
        {#if tablet.slate}
          <button type="button" class="button ghost small" data-action="sim-press" onclick={() => tablet.pressButton()}>
            {t('tablet.simulatedButton')}
          </button>
        {/if}
        {#if tablet.board}
          <!-- The simulated blackboard's own button stores the page on its screen. -->
          <button type="button" class="button ghost small" data-action="sim-store" onclick={() => tablet.storeBlackboardPage()}>
            {t('tablet.simulatedBoardButton')}
          </button>
        {/if}
      </section>
    {/if}
  {/if}

  {#if !tablet.board || tablet.slate}
    <!-- The guided test is the Slate's: the blackboard has none. -->
    <div class="actions foot">
      <button type="button" class="button" data-action="test" onclick={ontest}>{t('settings.tabletTest')}</button>
    </div>
  {/if}
</aside>

{#if wizardOpen === 'slate'}
  <RegisterWizard onclose={() => (wizardOpen = null)} />
{:else if wizardOpen === 'blackboard'}
  <AddBlackboard onclose={() => (wizardOpen = null)} />
{/if}

<style>
  .block {
    margin: 1.1rem 0;
    padding-top: 0.9rem;
    border-top: 1px solid var(--border);
  }

  .block h3 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    margin: 0.25rem 0;
  }

  .dot {
    width: 0.65rem;
    height: 0.65rem;
    border-radius: 50%;
    background: var(--muted);
  }

  [data-connection='connected'] .dot {
    background: #2fa866;
  }

  [data-connection='syncing'] .dot,
  [data-connection='live'] .dot {
    background: var(--accent);
  }

  [data-connection='searching'] .dot,
  [data-connection='connecting'] .dot,
  [data-connection='registering'] .dot {
    background: #d99a1b;
  }

  [data-connection='unavailable'] .dot {
    background: var(--danger);
  }

  .bar {
    width: 100%;
    accent-color: var(--accent);
  }

  .syncing p {
    margin: 0.3rem 0 0;
  }

  .last-sync {
    margin: 0.6rem 0 0;
  }

  .last-sync.failed span {
    display: block;
  }

  .inline {
    display: flex;
    gap: 0.5rem;
  }

  .inline input {
    flex: 1;
    min-width: 0;
  }

  .banner {
    margin: 0.75rem 0;
  }

  .foot {
    margin-top: 1.25rem;
  }
</style>
