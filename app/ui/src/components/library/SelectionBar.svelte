<script lang="ts">
  /** Actions on the selected notes: move to a binder, tags, export, print, assemble, duplicate, turn, favourite, trash. */
  import { t } from '../../lib/i18n.svelte';
  import { flattenTree, tagsOfNotes } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import TagColorPicker from './TagColorPicker.svelte';

  let { lib }: { lib: LibraryState } = $props();

  let panel = $state<'move' | 'tags' | 'turn' | null>(null);
  let tagText = $state('');

  const notes = $derived(lib.selectedNotes);
  const allFavorite = $derived(notes.length > 0 && notes.every((n) => n.favorite));
  const binders = $derived(flattenTree(lib.tree));
  const selectionTags = $derived(tagsOfNotes(notes));
  const knownTags = $derived((lib.listing?.tags ?? []).map((entry) => entry.tag));

  // Nothing selected any more: close the panels.
  $effect(() => {
    if (notes.length === 0) panel = null;
  });

  function toggle(name: 'move' | 'tags' | 'turn') {
    panel = panel === name ? null : name;
  }

  async function move(binder: string) {
    panel = null;
    await lib.moveSelected(binder);
  }

  async function turn(direction: 'cw' | 'ccw') {
    panel = null;
    await lib.turnNotes(lib.selectedDisplayed, direction);
  }

  async function addTags(text: string) {
    tagText = '';
    await lib.addTags(notes, text);
  }

  function escape(event: KeyboardEvent) {
    if (event.key === 'Escape' && panel) {
      event.stopPropagation();
      panel = null;
    }
  }
</script>

<!-- Escape closes an open panel; the controls inside remain the focusable parts. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="selection-bar" role="toolbar" aria-label={t('library.selection.bar')} tabindex="-1" onkeydown={escape}>
  <strong class="selection-count">{t('library.selection.count', { count: notes.length })}</strong>

  <button type="button" class="button" aria-expanded={panel === 'move'} onclick={() => toggle('move')}>
    {t('library.selection.move')}
  </button>
  <button type="button" class="button" aria-expanded={panel === 'tags'} onclick={() => toggle('tags')}>
    {t('library.selection.tags')}
  </button>
  <button type="button" class="button" onclick={() => lib.openDialog('export')}>
    {t('library.selection.export')}
  </button>
  <button type="button" class="button" data-action="print" onclick={() => lib.openDialog('print')}>
    {t('library.selection.print')}
  </button>
  <button
    type="button"
    class="button"
    disabled={notes.length < 2}
    title={notes.length < 2 ? t('library.selection.assembleNeeds') : undefined}
    onclick={() => lib.openDialog('assemble')}
  >
    {t('library.selection.assemble')}
  </button>
  <button
    type="button"
    class="button"
    data-action="duplicate"
    disabled={lib.activity.busy}
    onclick={() => lib.duplicateNotes(lib.selectedDisplayed)}
  >
    {t('library.selection.duplicate')}
  </button>
  <button
    type="button"
    class="button"
    data-action="turn"
    aria-expanded={panel === 'turn'}
    disabled={lib.activity.busy}
    onclick={() => toggle('turn')}
  >
    {t('library.turn.button')}
  </button>
  <button type="button" class="button" onclick={() => lib.setFavorite(notes, !allFavorite)}>
    {allFavorite ? t('library.selection.unfavorite') : t('library.selection.favorite')}
  </button>
  <button type="button" class="button danger" onclick={() => lib.trashNotes(notes.map((n) => n.path))}>
    {t('library.selection.trash')}
  </button>
  <button type="button" class="button ghost" onclick={() => lib.clearSelection()}>
    {t('library.selection.clear')}
  </button>
  <p class="selection-help muted small">{t('library.selection.help')}</p>
  {#if lib.dragOutSupported}
    <p class="selection-help muted small">{t('library.drag.help')}</p>
  {/if}

  {#if panel === 'move'}
    <div class="selection-panel" role="group" aria-label={t('library.selection.moveTo')}>
      <ul class="picker">
        <li><button type="button" onclick={() => move('')}>{t('library.root')}</button></li>
        {#each binders as node (node.path)}
          <li style:padding-left="{node.depth * 1}rem">
            <button type="button" data-binder={node.path} onclick={() => move(node.path)}>{node.name}</button>
          </li>
        {/each}
      </ul>
    </div>
  {:else if panel === 'turn'}
    <div class="selection-panel" role="group" aria-label={t('library.turn.panel')}>
      <div class="actions">
        <button type="button" class="button" data-action="turn-ccw" onclick={() => turn('ccw')}>
          <span aria-hidden="true">↺</span> {t('library.turn.left')}
        </button>
        <button type="button" class="button" data-action="turn-cw" onclick={() => turn('cw')}>
          <span aria-hidden="true">↻</span> {t('library.turn.right')}
        </button>
      </div>
      <p class="muted small">{t('library.turn.hint')}</p>
    </div>
  {:else if panel === 'tags'}
    <div class="selection-panel" role="group" aria-label={t('library.selection.tags')}>
      <form
        class="inline-form"
        onsubmit={(event) => {
          event.preventDefault();
          void addTags(tagText);
        }}
      >
        <input
          type="text"
          bind:value={tagText}
          aria-label={t('library.selection.tagInput')}
          placeholder={t('library.selection.tagInput')}
        />
        <button type="submit" class="button primary">{t('library.selection.addTag')}</button>
      </form>
      <p class="muted small">{t('library.selection.tagHint')}</p>

      {#if selectionTags.length > 0}
        <h3 class="mini-heading">{t('library.selection.tagsOfSelection')}</h3>
        <ul class="chips">
          {#each selectionTags as entry (entry.tag)}
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
    </div>
  {/if}
</div>
