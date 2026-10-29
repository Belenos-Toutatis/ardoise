<script lang="ts">
  /**
   * Offers the draft of the note just opened (M10, `lib/editor/drafts.ts`): Ardoise ended before
   * the note was saved. Over the note when the file is still the version the edits started from,
   * else only as a new note. "Later" goes back to the library and keeps the draft.
   */
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import type { Offer } from '../../lib/editor/drafts';
  import Modal from '../Modal.svelte';

  let {
    offer,
    busy = false,
    error = null,
    onrestore,
    oncopy,
    ondiscard,
    onlater,
  }: {
    offer: Offer;
    busy?: boolean;
    error?: string | null;
    onrestore: () => void;
    oncopy: () => void;
    ondiscard: () => void;
    onlater: () => void;
  } = $props();

  /** Throwing the changes away asks once more. */
  let confirming = $state(false);

  const when = $derived(
    new Intl.DateTimeFormat(currentLocale(), { dateStyle: 'long', timeStyle: 'short' }).format(new Date(offer.draft.savedAt)),
  );
</script>

<Modal title={t('drafts.recovery.title')} onclose={onlater} dismissible={!busy}>
  <div data-slot="draft-recovery" data-recovery={offer.recovery}>
    <p>
      {offer.recovery === 'restore'
        ? t('drafts.recovery.restoreText', { date: when })
        : t('drafts.recovery.copyText', { date: when })}
    </p>
    {#if error}
      <p class="banner error inline" role="alert">{t('drafts.recovery.failed', { message: error })}</p>
    {/if}
    {#if confirming}
      <p>{t('drafts.recovery.discardConfirm')}</p>
      <div class="actions">
        <button type="button" class="button danger" data-action="draft-discard-yes" disabled={busy} onclick={ondiscard}>
          {t('drafts.recovery.discardYes')}
        </button>
        <button type="button" class="button" data-autofocus disabled={busy} onclick={() => (confirming = false)}>
          {t('drafts.recovery.discardNo')}
        </button>
      </div>
    {:else}
      <div class="actions">
        {#if offer.recovery === 'restore'}
          <button type="button" class="button primary" data-action="draft-restore" data-autofocus disabled={busy} onclick={onrestore}>
            {t('drafts.recovery.restore')}
          </button>
        {/if}
        <button
          type="button"
          class="button"
          class:primary={offer.recovery === 'copy'}
          data-action="draft-copy"
          data-autofocus={offer.recovery === 'copy' ? '' : undefined}
          disabled={busy}
          onclick={oncopy}
        >
          {t('drafts.recovery.copy')}
        </button>
        <button type="button" class="button danger" data-action="draft-discard" disabled={busy} onclick={() => (confirming = true)}>
          {t('drafts.recovery.discard')}
        </button>
        <button type="button" class="button" data-action="draft-later" disabled={busy} onclick={onlater}>
          {t('drafts.recovery.later')}
        </button>
      </div>
    {/if}
  </div>
</Modal>
