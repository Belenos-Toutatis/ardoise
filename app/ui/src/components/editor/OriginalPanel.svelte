<script lang="ts">
  /**
   * "Revenir à l'original" (SPEC §4.4): the ink as it came from the tablet, or as it was before
   * its first edit, after a confirmation that says what is lost and what stays. Disabled, with
   * the reason, while the note has no original. The confirmation takes the focus (on its Cancel
   * button, the harmless one) when it opens, and gives it back to the button when it closes.
   */
  import { tick } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import type { NoteOriginal } from '../../lib/original';

  let {
    original,
    error = null,
    disabled = false,
    onrevert,
  }: {
    /** `null` while it is being looked for. */
    original: NoteOriginal | null;
    /** Why the original could not be read. */
    error?: string | null;
    /** The split panel is open: the page cannot change meanwhile. */
    disabled?: boolean;
    onrevert: () => void;
  } = $props();

  let confirming = $state(false);
  let openButton = $state<HTMLButtonElement>();
  let cancelButton = $state<HTMLButtonElement>();

  async function ask() {
    confirming = true;
    await tick();
    cancelButton?.focus();
  }

  async function settle(revert: boolean) {
    confirming = false;
    if (revert) onrevert();
    await tick();
    openButton?.focus();
  }

  const available = $derived(original !== null && original.source !== 'none');
  const help = $derived(
    original === null
      ? t('editor.original.loading')
      : original.source === 'tablet'
        ? t('editor.original.helpTablet')
        : original.source === 'firstVersion'
          ? original.strokes.length === 0
            ? t('editor.original.helpBlank')
            : t('editor.original.helpFirst')
          : t('editor.original.none'),
  );
</script>

<section class="side-section" aria-labelledby="editor-original-heading" data-section="original">
  <h3 id="editor-original-heading">{t('editor.original.label')}</h3>
  <p class="small muted" data-slot="original-help">{help}</p>
  {#if error}
    <p class="small original-error" role="alert">{error}</p>
  {/if}
  {#if confirming && available && !disabled}
    <div class="original-confirm" role="alertdialog" aria-labelledby="editor-original-confirm" aria-describedby="editor-original-lost">
      <p class="small" id="editor-original-confirm"><strong>{t('editor.original.confirmTitle')}</strong></p>
      <p class="small" id="editor-original-lost">{t('editor.original.confirmLost')}</p>
      <p class="small">{t('editor.original.confirmKept')}</p>
      <p class="small muted">{t('editor.original.confirmUndo')}</p>
      <div class="buttons">
        <button
          type="button"
          class="button small-button danger"
          data-action="original-confirm"
          onclick={() => void settle(true)}
        >
          {t('editor.original.confirm')}
        </button>
        <button
          type="button"
          class="button small-button"
          data-action="original-cancel"
          bind:this={cancelButton}
          onclick={() => void settle(false)}
        >
          {t('editor.original.cancel')}
        </button>
      </div>
    </div>
  {:else}
    <div class="buttons">
      <button
        type="button"
        class="button small-button"
        data-action="original"
        disabled={disabled || !available}
        title={help}
        bind:this={openButton}
        onclick={() => void ask()}
      >
        <span aria-hidden="true">↺</span>
        {t('editor.original.button')}
      </button>
    </div>
  {/if}
</section>

<style>
  .original-confirm {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.6rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
  }

  .original-error {
    color: var(--danger);
  }
</style>
