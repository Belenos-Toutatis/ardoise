<script lang="ts">
  /**
   * The line under the top bar of a revision, a retouched copy of another note made by an
   * assistant (SPEC §4.9): whose copy it is, and three buttons. « Ouvrir l'original » and
   * « Comparer » need the source: without it (trashed, deleted, not synchronised here) they are
   * greyed and the reason is written beside them, not only in a tooltip.
   */
  import { t } from '../../lib/i18n.svelte';
  import type { RevisionOfNote } from '../../lib/note';

  let {
    revision,
    onopen,
    onchanges,
    oncompare,
  }: {
    revision: RevisionOfNote;
    onopen: () => void;
    onchanges: () => void;
    oncompare: () => void;
  } = $props();

  /** The source's title now; the one it had when the copy was made while it cannot be found. */
  const sourceTitle = $derived(revision.source?.title ?? revision.ofTitle);
  const gone = $derived(revision.source === null);
</script>

<div class="revision-bar" role="group" aria-label={t('editor.revision.label')} data-slot="revision-bar">
  <p class="revision-line" data-slot="revision-line">{t('editor.revision.line', { title: sourceTitle })}</p>
  <div class="revision-actions">
    <button
      type="button"
      class="button small-button"
      data-action="revision-open"
      disabled={gone}
      aria-describedby={gone ? 'revision-gone' : undefined}
      onclick={onopen}>{t('editor.revision.openOriginal')}</button
    >
    <button type="button" class="button small-button" data-action="revision-changes" onclick={onchanges}>
      {t('editor.revision.changes')}
    </button>
    <button
      type="button"
      class="button small-button"
      data-action="revision-compare"
      disabled={gone}
      aria-describedby={gone ? 'revision-gone' : undefined}
      onclick={oncompare}>{t('editor.revision.compare')}</button
    >
  </div>
  {#if gone}
    <p class="revision-gone" id="revision-gone" data-slot="revision-gone">{t('editor.revision.gone')}</p>
  {/if}
</div>
