<script lang="ts">
  /**
   * The « Use » tab of the tablet panel (Q17): the connected tablet (its picture, name, state, battery and pages in
   * memory), « Live mode » and « Sync » one under the other, then what needs saying about the state: what is searched
   * for, what waits, the sync's progress, its last result or failure, the pages that wait on the tablet, and for the
   * Xiaomi the confirmation that its pages are deleted. With no tablet known, the invitation to add one. Follows the
   * window's shared status (`tablet-state.svelte.ts`).
   */
  import { errorMessage } from '../../lib/api';
  import { currentLocale, t, tn } from '../../lib/i18n.svelte';
  import { formatDate } from '../../lib/library-logic';
  import type { TabletKind } from '../../lib/tablet';
  import {
    blackboardNeedsConfirmation,
    blackboardWaitsForRequest,
    pagesOnTablet,
    tablet,
    tabletName,
  } from '../../lib/tablet-state.svelte';
  import AddBlackboard from './AddBlackboard.svelte';
  import Battery from './Battery.svelte';
  import BoardDrawing from './BoardDrawing.svelte';
  import OrientationIcon from './OrientationIcon.svelte';
  import RegisterWizard from './RegisterWizard.svelte';

  let { onlive }: { onlive: () => void } = $props();

  /** The flow open to add the first tablet. */
  let wizardOpen = $state<TabletKind | null>(null);
  /** An explicit sync of the blackboard waits for the user's word: it deletes the pages. */
  let confirmingSync = $state(false);

  const status = $derived(tablet.status);
  const connection = $derived(status?.connection ?? 'unavailable');
  /** The Slate, when one is registered (null when only the blackboard is known). */
  const registered = $derived(status?.registered ?? null);
  const device = $derived(status?.device ?? null);
  const hasTablet = $derived(tablet.known.length > 0);
  /** What the tablet read last is the blackboard's. */
  const boardDevice = $derived(device?.kind === 'blackboard');
  /**
   * The Xiaomi is the connected tablet and « Delete after import » is off: its pages are imported only on request, after
   * the user accepted that they are deleted from the tablet. Nothing is said while the Slate or no tablet is connected.
   */
  const warnBoard = $derived(blackboardNeedsConfirmation(status));
  /** Only the Xiaomi is known, it is not connected and its pages are imported on request: nothing to sync yet. */
  const boardOnRequest = $derived(blackboardWaitsForRequest(status));
  /** Pages the blackboard keeps until the user imports them. */
  const boardWaiting = $derived(warnBoard && connection === 'connected' ? (device?.storedPages ?? 0) : 0);
  /** Pages on the connected tablet: on the sync button and on the tab's badge. */
  const waiting = $derived(pagesOnTablet(status));
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
  /** The tablet the card is about: the connected one, else the only one known (none to name when two wait to connect). */
  const cardKind = $derived<TabletKind | null>(tablet.active ?? (tablet.known.length === 1 ? tablet.known[0].kind : null));
  /** What that tablet reported at its last connection. */
  const read = $derived(cardKind !== null && device?.kind === cardKind ? device : null);
  const cardName = $derived(
    cardKind === 'slate'
      ? (read?.name ?? registered?.name ?? '')
      : cardKind === 'blackboard'
        ? t('tablet.kind.blackboard')
        : tablet.known.map(tabletName).join(' · '),
  );
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
  /** Registering or searching for a blackboard is blocked by a library that is missing. */
  const noLibrary = $derived(connection === 'unavailable' && status?.error?.code === 'noLibrary');

  // The question has no meaning once the sync cannot start, or once the Xiaomi is no longer the connected tablet (the
  // « Sync and delete » that followed would send no flag and wait for nothing).
  $effect(() => {
    if (!canSync || !warnBoard) confirmingSync = false;
  });

  // The worker asked for the confirmation itself (the Xiaomi connected between the click and the request).
  $effect(() => {
    if (tablet.confirmDeleteAsked && warnBoard && canSync) confirmingSync = true;
  });

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

<div class="tp-use" data-slot="tab-use" data-connection={connection}>
  {#if status?.simulated}
    <p class="banner notice simulated" role="status" data-slot="simulated">{t('tablet.simulated')}</p>
  {/if}

  {#if !hasTablet}
    <section class="tp-block" aria-labelledby="tablet-register">
      <h3 id="tablet-register">{t('tablet.register.none')}</h3>
      <p class="muted">{t('tablet.register.intro')}</p>
      <div class="actions">
        <button type="button" class="button primary" data-action="register" disabled={noLibrary || connection === 'adding'} onclick={() => (wizardOpen = 'slate')}>
          {t('tablet.register.open')}
        </button>
        <button
          type="button"
          class="button primary"
          data-action="add-blackboard"
          disabled={noLibrary || connection === 'adding'}
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
    <section class="tp-card" aria-labelledby="tablet-info" data-slot="tablet-card" data-kind={cardKind}>
      <h3 id="tablet-info" class="visually-hidden">{t('tablet.info.title')}</h3>
      <div class="tp-who">
        {#if cardKind === 'slate'}
          <OrientationIcon orientation="landscape" size={42} />
        {:else if cardKind === 'blackboard'}
          <BoardDrawing size={48} />
        {/if}
        <div class="tp-names">
          <div class="tp-name" data-slot={cardKind === 'slate' ? 'name' : 'card-name'}>{cardName}</div>
          <p class="tp-state" data-slot="state">
            <span class="tp-dot" aria-hidden="true"></span>
            <strong>{t(`tablet.state.${connection}`)}</strong>
          </p>
        </div>
      </div>
      {#if read}
        <div class="tp-chips">
          <span class="tp-chip"><Battery percent={read.batteryPercent} charging={read.kind === 'blackboard' ? false : read.charging} /></span>
          <span class="tp-chip" data-slot="stored-pages" data-count={read.storedPages}>{tn('tablet.use.pages', read.storedPages)}</span>
        </div>
      {/if}
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
    </section>

    <section class="tp-actions" aria-label={t('tablet.sync.title')}>
      <button type="button" class="button primary tp-big" data-action="live" disabled={!canLive} onclick={onlive} aria-describedby="tablet-live-hint">
        <span>{t('tablet.live.open')}</span>
      </button>
      <p id="tablet-live-hint" class="muted small tp-live-hint">{t('tablet.live.hint')}</p>

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
        <button type="button" class="button tp-big" data-action="sync" disabled={!canSync || boardOnRequest} onclick={syncNow}>
          <span>{t('tablet.sync.now')}</span>
          {#if waiting > 0}
            <small data-slot="sync-waiting">{tn('tablet.sync.waiting', waiting)}</small>
          {/if}
        </button>
        {#if boardOnRequest}
          <p class="muted small" data-slot="board-on-request">{t('tablet.sync.boardOnRequest')}</p>
        {/if}
      {/if}
    </section>

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

    {#if status?.simulated}
      <section class="tp-sim" aria-label={t('tablet.simulated')}>
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
</div>

{#if wizardOpen === 'slate'}
  <RegisterWizard onclose={() => (wizardOpen = null)} />
{:else if wizardOpen === 'blackboard'}
  <AddBlackboard onclose={() => (wizardOpen = null)} />
{/if}

<style>
  .tp-use {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .tp-use p {
    margin: 0;
  }

  .tp-block h3 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }

  .tp-block p + .actions,
  .tp-block .actions + p {
    margin-top: 0.6rem;
  }

  .tp-who {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .tp-names {
    min-width: 0;
  }

  .tp-name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .tp-state {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.875rem;
  }

  .tp-dot {
    flex: none;
    width: 0.65rem;
    height: 0.65rem;
    border-radius: 50%;
    background: var(--muted);
  }

  [data-connection='connected'] .tp-dot {
    background: #2fa866;
  }

  [data-connection='syncing'] .tp-dot,
  [data-connection='live'] .tp-dot {
    background: var(--accent);
  }

  [data-connection='searching'] .tp-dot,
  [data-connection='connecting'] .tp-dot,
  [data-connection='registering'] .tp-dot {
    background: #d99a1b;
  }

  [data-connection='unavailable'] .tp-dot {
    background: var(--danger);
  }

  .tp-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-top: 0.7rem;
  }

  .tp-chip {
    padding: 0.1rem 0.6rem;
    border-radius: 999px;
    background: var(--accent-soft);
    font-size: 0.8125rem;
  }

  .tp-card .actions {
    margin-top: 0.6rem;
  }

  .tp-card .muted {
    margin-top: 0.6rem;
  }

  .tp-actions {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  /* The two big buttons: the name on the left, what it will do or bring on the right. */
  .tp-big {
    justify-content: space-between;
    width: 100%;
    padding: 0.7rem 0.85rem;
    text-align: left;
  }

  .tp-big small {
    font-size: 0.8125rem;
    opacity: 0.85;
  }

  .tp-live-hint {
    margin: -0.15rem 0 0.25rem;
  }

  .syncing p {
    margin: 0.3rem 0 0;
  }

  .bar {
    width: 100%;
    accent-color: var(--accent);
  }

  .last-sync.failed span {
    display: block;
  }

  .tp-use .banner {
    margin: 0;
  }

  .tp-sim {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.25rem;
  }
</style>
