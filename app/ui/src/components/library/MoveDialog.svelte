<script lang="ts">
  /** « Déplacer vers… » (the card menu): the binders to choose from, for the notes the menu acted on. */
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { displayTitle, flattenTree } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import Modal from '../Modal.svelte';

  let { lib, paths, onclose }: { lib: LibraryState; paths: string[]; onclose: () => void } = $props();

  const binders = $derived(flattenTree(lib.tree));
  const notes = $derived(lib.notes.filter((note) => paths.includes(note.path)));

  async function move(binder: string) {
    // The dialog goes first (and with it its props): the notes are kept.
    const chosen = [...paths];
    onclose();
    await lib.moveNotes(chosen, binder);
  }
</script>

<Modal title={t('library.selection.moveTo')} {onclose}>
  <div data-dialog="move">
    <p class="muted">
      {notes.length === 1
        ? t('export.subjectNote', { title: displayTitle(notes[0], currentLocale()) })
        : t('export.subjectMany', { count: notes.length })}
    </p>
    <ul class="picker" aria-label={t('library.selection.moveTo')}>
      <li><button type="button" data-autofocus onclick={() => move('')}>{t('library.root')}</button></li>
      {#each binders as node (node.path)}
        <li style:padding-left="{node.depth * 1}rem">
          <button type="button" data-binder={node.path} onclick={() => move(node.path)}>{node.name}</button>
        </li>
      {/each}
    </ul>
    <div class="actions">
      <button type="button" class="button" onclick={onclose}>{t('library.cancel')}</button>
    </div>
  </div>
</Modal>
