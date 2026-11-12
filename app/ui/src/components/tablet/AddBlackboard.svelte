<script lang="ts">
  /**
   * Adding the Xiaomi blackboard (SPEC §4.1, docs/design/blackboard.md §5.6). No « hold the
   * button » and no registration: the tablet only has to be switched on, and Mi Home (the only
   * other application that talks to it) closed. A first screen says so; then the search the
   * service runs (`connection: 'adding'`, `boardSearch.found` grows) shows what it hears, one
   * tablet being the usual case (the signal is shown when there are several); the user adds one,
   * or cancels at any moment. Nothing heard ends with `blackboardNotFound` and what to check.
   * The step shown is in `[data-step]`.
   */
  import { tick, untrack } from 'svelte';
  import { errorMessage } from '../../lib/api';
  import { t } from '../../lib/i18n.svelte';
  import type { TabletStatus } from '../../lib/tablet';
  import { tablet } from '../../lib/tablet-state.svelte';
  import Modal from '../Modal.svelte';

  let { onclose }: { onclose: () => void } = $props();

  type Phase = 'intro' | 'running' | 'added' | 'error';

  // A search already under way (started before the dialog was opened) is followed, not restarted.
  const underWay = untrack(() => tablet.status?.connection === 'adding');
  let phase = $state<Phase>(underWay ? 'running' : 'intro');
  /** Why the search or the addition did not succeed, translated. */
  let failure = $state<string | null>(null);
  let root = $state<HTMLElement | null>(null);

  // What the status looked like when the search started, to tell new news from old.
  let sawSearch = underWay;
  let startError: TabletStatus['error'] = null;
  let leaving = false;

  const status = $derived(tablet.status);
  const found = $derived(status?.boardSearch?.found ?? []);
  const step = $derived(phase === 'running' ? (found.length > 0 ? 'found' : 'searching') : phase);

  function sameError(a: TabletStatus['error'], b: TabletStatus['error']): boolean {
    return a?.code === b?.code && a?.message === b?.message;
  }

  // Follows the service while the search is under way.
  $effect(() => {
    if (phase !== 'running' || !status || leaving) return;
    if (status.connection === 'adding') {
      sawSearch = true;
    } else if (status.error && (sawSearch || !sameError(status.error, startError))) {
      // The search ended with nothing heard (or Bluetooth is off…).
      failure = errorMessage(status.error);
      phase = 'error';
    } else if (sawSearch) {
      // Ended without an error: cancelled from elsewhere.
      onclose();
    }
  });

  // Each new screen puts the keyboard focus on its main button.
  $effect(() => {
    void phase;
    void step;
    void tick().then(() => root?.querySelector<HTMLElement>('[data-autofocus]')?.focus());
  });

  async function search() {
    sawSearch = false;
    startError = status?.error ?? null;
    failure = null;
    phase = 'running';
    if (!(await tablet.findBlackboards())) {
      failure = tablet.actionError;
      phase = 'error';
    }
  }

  async function add(address: string) {
    // The search ends by itself once the tablet is added: that is not news for the effect above.
    leaving = true;
    const done = await tablet.addBlackboard(address);
    if (done) {
      phase = 'added';
    } else {
      leaving = false;
      failure = tablet.actionError;
      phase = 'error';
    }
  }

  async function leave() {
    leaving = true;
    if (phase === 'running') await tablet.cancel();
    onclose();
  }
</script>

<Modal title={t('tablet.board.title')} onclose={leave}>
  <div bind:this={root} class="add-blackboard" data-slot="add-blackboard">
    {#if phase === 'intro'}
      <div class="step" data-step="intro">
        <h3>{t('tablet.board.introTitle')}</h3>
        <ul class="checks">
          <li>{t('tablet.board.intro')}</li>
          <li>{t('tablet.board.introMiHome')}</li>
        </ul>
        <p class="muted">{t('tablet.board.introLinked')}</p>
      </div>
      <div class="actions">
        <button type="button" class="button primary" data-action="search" data-autofocus onclick={search}>
          {t('tablet.board.search')}
        </button>
        <button type="button" class="button" data-action="cancel-add" onclick={leave}>{t('tablet.board.cancel')}</button>
      </div>
    {:else if phase === 'running'}
      <div class="step" data-step={step}>
        <div aria-live="polite">
          {#if found.length === 0}
            <h3><span class="pulse" aria-hidden="true"></span>{t('tablet.board.searching.title')}</h3>
            <p>{t('tablet.board.searching.text')}</p>
            <p class="muted">{t('tablet.board.introMiHome')}</p>
          {:else}
            <h3>{found.length === 1 ? t('tablet.board.found.one') : t('tablet.board.found.many', { count: found.length })}</h3>
          {/if}
        </div>
        {#if found.length > 0}
          <ul class="found" data-slot="found">
            {#each found as entry (entry.address)}
              <li data-address={entry.address}>
                <span class="who">
                  <strong>{entry.name ?? t('tablet.board.found.unnamed')}</strong>
                  <code class="small">{entry.address}</code>
                  {#if found.length > 1 && entry.rssi !== null}
                    <span class="muted small" data-slot="signal">{t('tablet.board.found.signal', { rssi: entry.rssi })}</span>
                  {/if}
                </span>
                <button
                  type="button"
                  class="button primary"
                  data-action="add-blackboard"
                  data-address={entry.address}
                  data-autofocus={entry === found[0] ? '' : undefined}
                  aria-label={t('tablet.board.found.addLabel', { address: entry.address })}
                  onclick={() => add(entry.address)}
                >
                  {t('tablet.board.found.add')}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
      <div class="actions">
        <button type="button" class="button" data-action="cancel-add" data-autofocus={found.length === 0 ? '' : undefined} onclick={leave}>
          {t('tablet.board.cancel')}
        </button>
      </div>
    {:else if phase === 'added'}
      <div class="step" data-step="added">
        <div aria-live="polite">
          <h3>{t('tablet.board.added.title')}</h3>
          <p>{t('tablet.board.added.text')}</p>
        </div>
      </div>
      <div class="actions">
        <button type="button" class="button primary" data-action="finish" data-autofocus onclick={onclose}>
          {t('tablet.board.finish')}
        </button>
      </div>
    {:else}
      <div class="step" data-step="error" role="alert">
        <h3>{t('tablet.board.error.title')}</h3>
        {#if failure}
          <p class="error">{failure}</p>
        {/if}
      </div>
      <div class="actions">
        <button type="button" class="button primary" data-action="retry" data-autofocus onclick={search}>
          {t('tablet.board.retry')}
        </button>
        <button type="button" class="button" data-action="cancel-add" onclick={leave}>{t('common.close')}</button>
      </div>
    {/if}
  </div>
</Modal>

<style>
  .step h3 {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0.5rem 0 0.25rem;
  }

  .step p {
    margin: 0.4rem 0;
  }

  .checks {
    margin: 0.4rem 0 0.8rem;
    padding-left: 1.2rem;
  }

  .found {
    margin: 0.6rem 0 0;
    padding: 0;
    list-style: none;
  }

  .found li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.6rem 0;
    border-top: 1px solid var(--border);
  }

  .who {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    min-width: 0;
  }

  .pulse {
    flex: none;
    width: 0.65rem;
    height: 0.65rem;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1.4s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pulse {
      animation: none;
    }
  }
</style>
