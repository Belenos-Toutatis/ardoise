<script lang="ts">
  /**
   * « Étiquettes… » (the card menu): adds a tag to the notes the menu acted on, removes one they carry, or picks one that
   * exists in the library; the colour of a tag is set here too. Each change is made at once.
   */
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { displayTitle, tagsOfNotes } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import Modal from '../Modal.svelte';
  import TagColorPicker from './TagColorPicker.svelte';

  let { lib, paths, onclose }: { lib: LibraryState; paths: string[]; onclose: () => void } = $props();

  let tagText = $state('');

  const notes = $derived(lib.notes.filter((note) => paths.includes(note.path)));
  const noteTags = $derived(tagsOfNotes(notes));
  const knownTags = $derived((lib.listing?.tags ?? []).map((entry) => entry.tag));

  // The notes are gone (trashed, moved away by a sync): nothing left to tag.
  $effect(() => {
    if (notes.length === 0) onclose();
  });

  async function addTags(text: string) {
    tagText = '';
    await lib.addTags(notes, text);
  }
</script>

<Modal title={t('library.tagsDialog.title')} {onclose}>
  <div data-dialog="tags">
    <p class="muted">
      {notes.length === 1
        ? t('export.subjectNote', { title: displayTitle(notes[0], currentLocale()) })
        : t('export.subjectMany', { count: notes.length })}
    </p>
    <form
      class="inline-form"
      onsubmit={(event) => {
        event.preventDefault();
        void addTags(tagText);
      }}
    >
      <input
        type="text"
        data-autofocus
        bind:value={tagText}
        aria-label={t('library.selection.tagInput')}
        placeholder={t('library.selection.tagInput')}
      />
      <button type="submit" class="button primary">{t('library.selection.addTag')}</button>
    </form>
    <p class="muted small">{t('library.selection.tagHint')}</p>

    {#if noteTags.length > 0}
      <h3 class="mini-heading">{t('library.tagsDialog.ofNotes')}</h3>
      <ul class="chips">
        {#each noteTags as entry (entry.tag)}
          {@const color = lib.tagColors.of(entry.tag)}
          <li>
            <button
              type="button"
              class="chip removable"
              data-tag-color={color}
              aria-label={t('library.selection.removeTag', { tag: entry.tag })}
              onclick={() => lib.removeTag(notes, entry.tag)}
            >
              {#if color}<span class="tag-dot" aria-hidden="true"></span>{/if}{entry.tag}<span aria-hidden="true"> ×</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    {#if knownTags.length > 0}
      <h3 class="mini-heading">{t('library.selection.existingTags')}</h3>
      <ul class="chips">
        {#each knownTags as tag (tag)}
          {@const color = lib.tagColors.of(tag)}
          <li class="tag-entry">
            <button type="button" class="chip" data-add-tag={tag} data-tag-color={color} onclick={() => addTags(tag)}
              >{#if color}<span class="tag-dot" aria-hidden="true"></span>{/if}{tag}</button
            >
            <TagColorPicker {tag} colors={lib.tagColors} />
          </li>
        {/each}
      </ul>
      <p class="muted small">{t('library.tagColor.hint')}</p>
    {/if}

    <div class="actions">
      <button type="button" class="button primary" onclick={onclose}>{t('common.close')}</button>
    </div>
  </div>
</Modal>
