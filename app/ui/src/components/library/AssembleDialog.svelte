<script lang="ts">
  /**
   * Assembles the selected notes (at least two, oldest first) into a new note: one on top of the
   * other on a single page, or end to end on a long page. The original notes are not changed.
   */
  import { onMount, untrack } from 'svelte';
  import { api, errorMessage } from '../../lib/api';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { formatDate, oldestFirst } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import type { MergeLayout, NoteSummary } from '../../lib/note';
  import Modal from '../Modal.svelte';

  let { lib, notes, onclose }: { lib: LibraryState; notes: NoteSummary[]; onclose: () => void } = $props();

  /** Proposed title, from the oldest note. No time in a date: the title becomes part of a file name. */
  function defaultTitle(): string {
    const first = oldestFirst(notes)[0];
    const base = first.title.trim() || formatDate(first.created, currentLocale(), false);
    return t('library.assemble.defaultTitle', { title: base });
  }

  /** Background images of the notes after the oldest, which the assembly does not keep. */
  let droppedImages = $state(0);
  onMount(() => {
    api
      .backgroundImageNames(oldestFirst(notes).map((n) => n.path))
      .then((names) => (droppedImages = names.slice(1).filter((name) => name !== null).length))
      .catch(() => {
        // Not known: nothing to say.
      });
  });

  let layout = $state<MergeLayout>('overlay');
  let title = $state(untrack(defaultTitle));
  let running = $state(false);
  let error = $state<string | null>(null);

  async function run() {
    if (running || title.trim() === '') return;
    running = true;
    error = null;
    try {
      await lib.assemble(notes, layout, title);
      onclose();
    } catch (e) {
      error = errorMessage(e);
      running = false;
    }
  }
</script>

<Modal title={t('library.assemble.title')} {onclose} dismissible={!running}>
  <form
    aria-busy={running}
    onsubmit={(event) => {
      event.preventDefault();
      void run();
    }}
  >
    <p class="muted">{t('library.assemble.intro', { count: notes.length })}</p>

    <fieldset class="export-fieldset" disabled={running}>
      <legend>{t('library.assemble.layout')}</legend>
      <label class="choice">
        <input type="radio" name="assemble-layout" value="overlay" bind:group={layout} />
        {t('library.assemble.overlay')}
      </label>
      <p class="muted small export-hint">{t('library.assemble.overlayHint')}</p>
      <label class="choice">
        <input type="radio" name="assemble-layout" value="endToEnd" bind:group={layout} />
        {t('library.assemble.endToEnd')}
      </label>
      <p class="muted small export-hint">{t('library.assemble.endToEndHint')}</p>
    </fieldset>

    {#if droppedImages > 0}
      <p class="muted small" data-slot="assemble-dropped-images">{t('background.assemble.dropped', { count: droppedImages })}</p>
    {/if}

    <label class="field">
      <span>{t('library.assemble.titleLabel')}</span>
      <input class="text-input" type="text" bind:value={title} disabled={running} />
    </label>

    {#if error}
      <p class="banner error inline" role="alert">{t('library.assemble.error', { message: error })}</p>
    {/if}

    <div class="actions">
      <button type="submit" class="button primary" data-autofocus disabled={running || title.trim() === ''}>
        {running ? t('library.assemble.running') : t('library.assemble.run')}
      </button>
      <button type="button" class="button" disabled={running} onclick={onclose}>{t('library.cancel')}</button>
    </div>
  </form>
</Modal>
