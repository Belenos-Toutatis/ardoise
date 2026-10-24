<script lang="ts">
  /**
   * Side panel of the tablet (SPEC §4.1): connection and information, sync now (with progress and
   * cancel), live mode, the options, the tablet's own settings (name, clock, forget) and the
   * registration wizard when no tablet is registered. Follows the window's shared status
   * (`tablet-state.svelte.ts`).
   */
  import { onMount, untrack } from 'svelte';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { formatDate } from '../../lib/library-logic';
  import { sizeInMillimetres, tablet, validTabletName } from '../../lib/tablet-state.svelte';
  import Battery from './Battery.svelte';
  import RegisterWizard from './RegisterWizard.svelte';
  import TabletOptions from './TabletOptions.svelte';

  let { onclose, onlive, ontest }: { onclose: () => void; onlive: () => void; ontest: () => void } = $props();

  onMount(() => tablet.start());

  let wizardOpen = $state(false);
  let confirmingForget = $state(false);
  /** The name as being edited. */
  let draft = $state(untrack(() => tablet.status?.registered?.name ?? ''));
  /** The name was submitted and refused for its form. */
  let nameRefused = $state(false);

  const status = $derived(tablet.status);
  const connection = $derived(status?.connection ?? 'unavailable');
  const registered = $derived(status?.registered ?? null);
  const registeredName = $derived(registered?.name ?? '');
  const device = $derived(status?.device ?? null);
  const sync = $derived(status?.sync ?? null);
  const lastSync = $derived(status?.lastSync ?? null);
  const name = $derived(device?.name ?? registered?.name ?? '');
  const canSync = $derived(connection === 'connected' || connection === 'searching' || connection === 'connecting');
  const canLive = $derived(connection !== 'syncing' && connection !== 'registering');
  const hint = $derived(
    status?.pending === 'sync'
      ? 'tablet.hint.pendingSync'
      : status?.pending === 'live'
        ? 'tablet.hint.pendingLive'
        : connection === 'searching'
          ? 'tablet.hint.searching'
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

  $effect(() => {
    if (!registered) confirmingForget = false;
  });

  async function rename(event: SubmitEvent) {
    event.preventDefault();
    nameRefused = !validTabletName(draft);
    if (!nameRefused) await tablet.rename(draft);
  }

  async function forget() {
    confirmingForget = false;
    await tablet.forget();
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
  {:else if !registered}
    <section class="block" aria-labelledby="tablet-register">
      <h3 id="tablet-register">{t('tablet.register.none')}</h3>
      <p class="muted">{t('tablet.register.intro')}</p>
      <div class="actions">
        <button
          type="button"
          class="button primary"
          data-action="register"
          disabled={connection === 'unavailable' && status.error?.code === 'noLibrary'}
          onclick={() => (wizardOpen = true)}
        >
          {t('tablet.register.open')}
        </button>
      </div>
    </section>
  {:else}
    <section class="block" aria-labelledby="tablet-info">
      <h3 id="tablet-info">{t('tablet.info.title')}</h3>
      <p class="state" data-slot="state">
        <span class="dot" aria-hidden="true"></span>
        <strong>{t(`tablet.state.${connection}`)}</strong>
      </p>
      {#if hint}
        <p class="muted small" data-slot="hint">{t(hint)}</p>
      {/if}
      <dl class="facts">
        <dt>{t('tablet.info.name')}</dt>
        <dd data-slot="name">{name}</dd>
        {#if device}
          <dt>{t('tablet.info.battery')}</dt>
          <dd><Battery percent={device.batteryPercent} charging={device.charging} /></dd>
          <dt>{t('tablet.info.firmware')}</dt>
          <dd>{device.firmware}</dd>
          <dt>{t('tablet.info.pages')}</dt>
          <dd data-slot="stored-pages">{device.storedPages}</dd>
          <dt>{t('tablet.info.model')}</dt>
          <dd>{device.model}</dd>
          <dt>{t('tablet.info.size')}</dt>
          <dd>{sizeInMillimetres(device.width, device.height)}</dd>
          <dt>{t('tablet.info.readAt')}</dt>
          <dd>{formatDate(device.readAt, currentLocale())}</dd>
        {:else}
          <dt>{t('tablet.info.size')}</dt>
          <dd>{sizeInMillimetres(registered.width, registered.height)}</dd>
        {/if}
      </dl>
      {#if !device}
        <p class="muted small">{t('tablet.info.notRead')}</p>
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
      {:else}
        <div class="actions">
          <button type="button" class="button primary" data-action="sync" disabled={!canSync} onclick={() => tablet.sync()}>
            {t('tablet.sync.now')}
          </button>
        </div>
      {/if}
      <p class="muted small last-sync" data-slot="last-sync">
        {#if lastSync}
          {t('tablet.sync.last', { date: formatDate(lastSync.at, currentLocale()) })}
          {savedText(lastSync.saved.length)}
          {#if lastSync.deleted > 0}{deletedText(lastSync.deleted)}{/if}
        {:else}
          {t('tablet.sync.never')}
        {/if}
      </p>
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

      {#if confirmingForget}
        <div class="confirm" role="alertdialog" aria-label={t('settings.tabletForget')}>
          <p>{t('settings.tabletForgetConfirm', { name: registered.name })}</p>
          <div class="actions">
            <button type="button" class="button danger" data-action="forget-yes" onclick={forget}>
              {t('settings.tabletForgetYes')}
            </button>
            <button type="button" class="button" onclick={() => (confirmingForget = false)}>{t('settings.cancel')}</button>
          </div>
        </div>
      {:else}
        <div class="actions">
          <button type="button" class="button danger" data-action="forget" onclick={() => (confirmingForget = true)}>
            {t('settings.tabletForget')}
          </button>
        </div>
      {/if}
    </section>

    {#if status.simulated}
      <section class="block" aria-label={t('tablet.simulated')}>
        <!-- The simulated tablet has a button too: used by the wizard and by live mode. -->
        <button type="button" class="button ghost small" data-action="sim-press" onclick={() => tablet.pressButton()}>
          {t('tablet.simulatedButton')}
        </button>
      </section>
    {/if}
  {/if}

  <div class="actions foot">
    <button type="button" class="button" data-action="test" onclick={ontest}>{t('settings.tabletTest')}</button>
  </div>
</aside>

{#if wizardOpen}
  <RegisterWizard onclose={() => (wizardOpen = false)} />
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
