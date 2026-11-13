<script lang="ts">
  /**
   * « Changer la date… » on one note: its creation date (what the grid sorts by, and the date at the start of its file
   * name), set to now with « Aujourd'hui » or to a date and time typed in the field. The ink and the sound are not
   * touched: their times count from the date, so they follow it.
   */
  import { untrack } from 'svelte';
  import { errorMessage } from '../../lib/api';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { displayTitle, formatDate } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import type { NoteSummary } from '../../lib/note';
  import Modal from '../Modal.svelte';

  let { lib, note, onclose }: { lib: LibraryState; note: NoteSummary; onclose: () => void } = $props();

  /** A time as the field wants it: `2026-11-13T09:30`, in the computer's time zone, to the minute. */
  function fieldValue(ms: number): string {
    const date = new Date(ms);
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
  }

  const initial = untrack(() => fieldValue(note.created));
  let value = $state(initial);
  let running = $state(false);
  let error = $state<string | null>(null);

  /** What the field holds, in Unix milliseconds; null when it is empty or not a date. */
  const wanted = $derived.by(() => {
    const ms = new Date(value).getTime();
    return value && Number.isFinite(ms) ? ms : null;
  });
  const unchanged = $derived(value === initial);

  function today() {
    value = fieldValue(Date.now());
  }

  async function run() {
    if (running || wanted === null) return;
    // The field has no seconds: leaving it as it was keeps the date exactly as it is.
    if (unchanged) return onclose();
    running = true;
    error = null;
    try {
      await lib.setCreated(note, wanted);
      onclose();
    } catch (e) {
      error = errorMessage(e);
      running = false;
    }
  }
</script>

<Modal title={t('library.date.title')} {onclose} dismissible={!running}>
  <form
    aria-busy={running}
    data-dialog="change-date"
    onsubmit={(event) => {
      event.preventDefault();
      void run();
    }}
  >
    <p class="muted">{t('library.date.intro', { title: displayTitle(note, currentLocale()) })}</p>
    <p class="muted small">{t('library.date.current', { date: formatDate(note.created, currentLocale()) })}</p>

    <div class="field date-field">
      <label for="note-date">{t('library.date.label')}</label>
      <div class="inline-form">
        <input id="note-date" class="text-input" type="datetime-local" bind:value disabled={running} required />
        <button type="button" class="button" data-action="date-today" disabled={running} onclick={today}>
          {t('library.date.today')}
        </button>
      </div>
    </div>
    <p class="muted small">{t('library.date.hint')}</p>

    {#if error}
      <p class="banner error inline" role="alert">{t('library.date.error', { message: error })}</p>
    {/if}

    <div class="actions">
      <button type="submit" class="button primary" data-autofocus data-action="date-apply" disabled={running || wanted === null}>
        {running ? t('library.date.applying') : t('library.date.apply')}
      </button>
      <button type="button" class="button" disabled={running} onclick={onclose}>{t('library.cancel')}</button>
    </div>
  </form>
</Modal>
