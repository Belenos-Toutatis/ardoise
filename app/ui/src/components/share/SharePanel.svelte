<script lang="ts">
  /**
   * The share panel (milestone M11; docs/design/lan-share.md, "Interface"): a dialog over the
   * library, the editor and live mode. Opening it starts the share (`share.openPanel()`); it
   * gives what the other computer needs — the address, a QR code, the code — and what the
   * teacher needs — how many screens are connected, what they show, pause, stop, what to do
   * about the firewall. Esc and « Fermer » close the panel only: the share keeps running (an
   * indicator stays on screen); « Arrêter le partage » ends it.
   */
  import { onDestroy } from 'svelte';
  import { genericError } from '../../lib/api';
  import { t, tn } from '../../lib/i18n.svelte';
  import { share } from '../../lib/share-state.svelte';
  import Modal from '../Modal.svelte';

  /** How long « Adresse copiée » stays, in milliseconds. */
  const COPIED_MS = 2000;

  const status = $derived(share.status);
  const qr = $derived(status.qr);
  const mainAddress = $derived(status.addresses[0] ?? '');
  const otherAddresses = $derived(status.addresses.slice(1));

  let copied = $state(false);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  onDestroy(() => clearTimeout(copiedTimer));

  /** Puts the first address on the clipboard; where that is refused, the address stays selectable. */
  async function copy() {
    const address = status.addresses[0];
    if (!address) return;
    try {
      await navigator.clipboard.writeText(address);
    } catch {
      return;
    }
    copied = true;
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copied = false), COPIED_MS);
  }
</script>

<Modal title={t('share.panel.title')} onclose={() => share.closePanel()}>
  <div class="share-panel" aria-busy={share.starting}>
    {#if status.active}
      <section class="block" aria-labelledby="share-open">
        <p id="share-open">{t('share.open')}</p>
        <div class="address-row">
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <code class="share-address main" data-share-address={mainAddress} tabindex="0">{mainAddress}</code>
          <button type="button" class="button" data-action="share-copy" onclick={copy}>{t('share.copy')}</button>
        </div>
        <p class="muted small copied" role="status">{copied ? t('share.copied') : ''}</p>
        {#if otherAddresses.length > 0}
          <p class="muted small">{t('share.otherAddresses')}</p>
          <ul class="others">
            {#each otherAddresses as address (address)}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <li><code class="share-address" data-share-address={address} tabindex="0">{address}</code></li>
            {/each}
          </ul>
        {/if}
      </section>

      <section class="block code-block" aria-labelledby="share-code-intro">
        <div class="code-text">
          <p id="share-code-intro">{t('share.codeIntro')}</p>
          <p class="share-code-line">
            <span class="visually-hidden">{t('share.codeLabel')}</span>
            <strong class="share-code" data-share-code={status.code ?? ''}>{status.code}</strong>
          </p>
        </div>
        {#if qr}
          <figure class="qr-figure">
            <svg
              class="share-qr"
              viewBox="-4 -4 {qr.size + 8} {qr.size + 8}"
              role="img"
              aria-label={t('share.qr')}
              shape-rendering="crispEdges"
            >
              <rect x="-4" y="-4" width={qr.size + 8} height={qr.size + 8} fill="#ffffff" />
              <path d={qr.path} fill="#000000" />
            </svg>
            <figcaption class="muted small">{t('share.qrHint')}</figcaption>
          </figure>
        {/if}
      </section>

      <section class="block status-block" aria-label={t('share.shown.label')}>
        <p class="share-viewers" data-share-viewers={status.viewers} role="status">
          {status.viewers === 0 ? t('share.viewersNone') : tn('share.viewers', status.viewers)}
        </p>
        <p class="small" data-share-shown={status.shown}>
          <span class="muted">{t('share.shown.label')} :</span>
          {t(`share.shown.${status.shown}`)}
        </p>
        {#if status.paused}
          <p class="paused" role="status" data-share-paused>{t('share.paused')}</p>
        {/if}
      </section>

      <div class="actions">
        <button
          type="button"
          class="button"
          data-action="share-pause"
          aria-pressed={status.paused}
          class:active={status.paused}
          disabled={share.busy}
          onclick={() => share.setPaused(!status.paused)}
        >
          {status.paused ? t('share.resume') : t('share.pause')}
        </button>
        <button type="button" class="button" data-action="share-stop" disabled={share.busy} onclick={() => share.stop()}>
          {t('share.stop')}
        </button>
      </div>

      <section class="help muted small" aria-label={t('share.helpTitle')}>
        <p>{t('share.helpFirewall')}</p>
        <p>{t('share.helpEnd')}</p>
      </section>
    {:else if share.starting}
      <p class="starting" role="status">
        <span class="spinner" aria-hidden="true"></span>
        {t('share.panel.starting')}
      </p>
    {:else}
      <p class="muted">{t('share.buttonHint')}</p>
      <div class="actions">
        <button type="button" class="button primary" data-action="share-start" onclick={() => share.start()}>
          {share.error ? t('share.panel.retry') : t('share.button')}
        </button>
      </div>
    {/if}

    {#if share.error}
      <p class="error" role="alert" data-share-error>{genericError(share.error)}</p>
    {/if}

    <div class="actions close">
      <button type="button" class="button ghost" data-autofocus data-action="share-close" onclick={() => share.closePanel()}>
        {t('share.panel.close')}
      </button>
    </div>
  </div>
</Modal>

<style>
  .share-panel {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .block p {
    margin: 0.25rem 0;
  }

  .address-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .share-address {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    overflow-wrap: anywhere;
    user-select: all;
    border-radius: 4px;
  }

  .share-address.main {
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--border);
    background: var(--surface);
    font-size: 1.35rem;
    font-weight: 600;
  }

  .share-address:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .others {
    margin: 0;
    padding-left: 1.25rem;
    font-size: 0.9rem;
  }

  .copied {
    min-height: 1.2em;
  }

  .code-block {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  .share-code-line {
    margin: 0.25rem 0 0;
  }

  .share-code {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 2.4rem;
    letter-spacing: 0.18em;
    line-height: 1.2;
  }

  .qr-figure {
    margin: 0;
    text-align: center;
  }

  .share-qr {
    display: block;
    width: 9rem;
    height: 9rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: #ffffff;
  }

  .status-block {
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .share-viewers {
    font-weight: 600;
  }

  .paused {
    color: var(--accent);
    font-weight: 600;
  }

  .actions {
    margin: 0;
  }

  .actions.close {
    justify-content: flex-end;
  }

  .help p {
    margin: 0.25rem 0;
  }

  .starting {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }

  .spinner {
    width: 1.1rem;
    height: 1.1rem;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: share-spin 0.9s linear infinite;
  }

  @keyframes share-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 3s;
    }
  }
</style>
