<script lang="ts">
  /**
   * « Ce qui a changé »: the account that comes with a revision, one dated entry per retouching.
   * It is written by an assistant: every text is shown as plain text (line breaks kept), never
   * as markup.
   */
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import type { RevisionOfNote } from '../../lib/note';
  import Modal from '../Modal.svelte';

  let { revision, onclose }: { revision: RevisionOfNote; onclose: () => void } = $props();

  const format = $derived(new Intl.DateTimeFormat(currentLocale(), { dateStyle: 'long', timeStyle: 'short' }));
  /** An instant of a file we do not control: out of the range of dates, it is shown as nothing. */
  const valid = (ms: number) => Number.isFinite(ms) && Math.abs(ms) <= 8.64e15;
  const when = (ms: number) => (valid(ms) ? format.format(new Date(ms)) : '');
  /** The title the source has now when it is known. */
  const sourceTitle = $derived(revision.source?.title ?? revision.ofTitle);
</script>

<Modal title={t('editor.revision.account.title')} {onclose}>
  <div data-dialog="revision-account">
    <p class="muted small">{t('editor.revision.account.intro', { date: when(revision.created), title: sourceTitle })}</p>
    {#if revision.summary.length > 0}
      <ol class="revision-account" aria-label={t('editor.revision.account.list')} data-slot="revision-account">
        {#each revision.summary as entry, index (index)}
          <li>
            <time class="muted small" datetime={valid(entry.at) ? new Date(entry.at).toISOString() : undefined}>{when(entry.at)}</time>
            <p class="revision-text">{entry.text}</p>
          </li>
        {/each}
      </ol>
      <p class="muted small">{t('editor.revision.account.byAssistant')}</p>
    {:else}
      <p data-slot="revision-account-empty">{t('editor.revision.account.empty')}</p>
    {/if}
    <div class="actions">
      <button type="button" class="button primary" data-autofocus data-action="revision-close" onclick={onclose}>
        {t('common.close')}
      </button>
    </div>
  </div>
</Modal>
