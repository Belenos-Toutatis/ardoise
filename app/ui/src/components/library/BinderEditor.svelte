<script lang="ts">
  /**
   * Under a binder of the sidebar (milestone M10): its new name, or the confirmation of its
   * deletion, which says how many notes go to the trash with it.
   */
  import { onMount, tick } from 'svelte';
  import { errorMessage } from '../../lib/api';
  import { t, tn } from '../../lib/i18n.svelte';
  import { flattenTree, type BinderNode } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';

  let {
    lib,
    node,
    mode,
    onrenamed,
    onclose,
  }: {
    lib: LibraryState;
    node: BinderNode;
    mode: 'rename' | 'delete';
    /** The binder was renamed from `from` to `to`. */
    onrenamed: (from: string, to: string) => void;
    onclose: () => void;
  } = $props();

  let name = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);
  let input = $state<HTMLInputElement | null>(null);
  let confirmButton = $state<HTMLButtonElement | null>(null);

  const subBinders = $derived(flattenTree(node.children).length);

  onMount(async () => {
    name = node.name;
    await tick();
    if (mode === 'rename') {
      input?.focus();
      input?.select();
    } else {
      confirmButton?.focus();
    }
  });

  async function rename(event: SubmitEvent) {
    event.preventDefault();
    const wanted = name.trim();
    if (!wanted || busy) return;
    if (wanted === node.name) {
      onclose();
      return;
    }
    busy = true;
    error = null;
    try {
      const renamed = await lib.renameBinder(node.path, wanted);
      onrenamed(node.path, renamed);
    } catch (e) {
      error = errorMessage(e);
      busy = false;
    }
  }

  async function remove() {
    busy = true;
    onclose();
    await lib.deleteBinder(node.path, node.name);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onclose();
    }
  }
</script>

<!-- Escape cancels; the controls inside remain the focusable parts. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="binder-editor" onkeydown={keydown}>
  {#if mode === 'rename'}
    <form class="inline-form binder-form" onsubmit={rename}>
      <input
        bind:this={input}
        bind:value={name}
        type="text"
        aria-label={t('library.binder.newName', { name: node.name })}
        oninput={() => (error = null)}
      />
      <button type="submit" class="button primary" disabled={!name.trim() || busy}>{t('library.binder.renameDo')}</button>
      <button type="button" class="button ghost" onclick={onclose}>{t('library.cancel')}</button>
    </form>
    {#if error}
      <p class="error small binder-error" role="alert">{error}</p>
    {/if}
  {:else}
    <div class="confirm binder-confirm" role="alertdialog" aria-label={t('library.binder.deleteTitle', { name: node.name })}>
      <p>
        {node.count === 0
          ? t('library.binder.confirmEmpty', { name: node.name })
          : tn('library.binder.confirm', node.count, { name: node.name })}
        {#if subBinders > 0}{tn('library.binder.confirmSubBinders', subBinders)}{/if}
      </p>
      <p class="muted small">{t('library.binder.confirmHint')}</p>
      <div class="actions">
        <button bind:this={confirmButton} type="button" class="button primary danger-fill" disabled={busy} onclick={remove}>
          {t('library.binder.deleteDo')}
        </button>
        <button type="button" class="button" onclick={onclose}>{t('library.cancel')}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .binder-editor {
    margin: 0.25rem 0 0.4rem 0.4rem;
  }

  .binder-editor .binder-form {
    margin: 0;
  }

  .binder-error {
    margin: 0.3rem 0 0;
  }

  .binder-confirm p {
    margin: 0 0 0.5rem;
  }
</style>
