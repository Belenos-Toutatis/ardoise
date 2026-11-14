<script lang="ts">
  /**
   * « Dans le sommaire »: what a binder's summary holds, in three independent choices (the names of the sub-binders, a line
   * per note, the bookmarks). The same group in the summary dialog and in the binder's export dialog, where it shapes the
   * PDF's outline and the summary page. `onchange` gets the new choices; the parent remembers them
   * (`lib/binder-options.ts`).
   */
  import { t } from '../lib/i18n.svelte';
  import type { SummaryChoices, SummaryNotes } from '../lib/library-logic';

  let {
    choices,
    binders = true,
    fallback = false,
    disabled = false,
    onchange,
  }: {
    choices: SummaryChoices;
    /** The binder has sub-binders (included): else their choice is not offered. */
    binders?: boolean;
    /** The notes are listed only because the binder has no bookmark and nothing was chosen yet: the group says so. */
    fallback?: boolean;
    disabled?: boolean;
    onchange: (choices: SummaryChoices) => void;
  } = $props();

  const NOTES: readonly SummaryNotes[] = ['title', 'titleOrDate', 'none'];
  const uid = $props.id();
</script>

<fieldset class="export-fieldset" data-slot="summary-choices" {disabled}>
  <legend>{t('summaryChoices.legend')}</legend>
  {#if binders}
    <label class="check" data-slot="summary-binders">
      <input
        type="checkbox"
        data-option="summary-binders"
        checked={choices.binders}
        onchange={(event) => onchange({ ...choices, binders: event.currentTarget.checked })}
      />
      {t('summaryChoices.binders')}
    </label>
  {/if}
  <div class="export-row" role="radiogroup" aria-label={t('summaryChoices.notes')} data-slot="summary-notes">
    <span>{t('summaryChoices.notes')}</span>
    {#each NOTES as mode (mode)}
      <label class="choice">
        <input
          type="radio"
          name="{uid}-notes"
          value={mode}
          data-option="summary-notes-{mode}"
          checked={choices.notes === mode}
          onchange={() => onchange({ ...choices, notes: mode })}
        />
        {t(`summaryChoices.notesChoices.${mode}`)}
      </label>
    {/each}
  </div>
  <p class="muted small">{t('summaryChoices.notesHint')}</p>
  <label class="check" data-slot="summary-bookmarks">
    <input
      type="checkbox"
      data-option="summary-bookmarks"
      checked={choices.bookmarks}
      onchange={(event) => onchange({ ...choices, bookmarks: event.currentTarget.checked })}
    />
    {t('summaryChoices.bookmarks')}
  </label>
  {#if fallback}
    <p class="muted small" role="status" data-slot="summary-fallback">{t('summaryChoices.noBookmarks')}</p>
  {/if}
</fieldset>
