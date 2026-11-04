<script lang="ts">
  /** One note of the grid: thumbnail, title, date, favourite star, tags, selection and menu. */
  import { tick, untrack } from 'svelte';
  import { startDragOut, type DragOutPhase } from '../../lib/drag-out';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { displayTitle, formatDate } from '../../lib/library-logic';
  import type { LibraryState } from '../../lib/library-state.svelte';
  import { whenNearViewport } from '../../lib/near-viewport';
  import type { NoteSummary } from '../../lib/note';
  import { noteDrawn } from '../../lib/perf';
  import { resumeBlock } from '../../lib/live-resume.svelte';
  import { tablet } from '../../lib/tablet-state.svelte';
  import Menu from './Menu.svelte';

  let {
    note,
    lib,
    index,
    active,
    onopen,
    onnavigate,
    onfocuscard,
    onrenamed,
    onresume,
  }: {
    note: NoteSummary;
    lib: LibraryState;
    index: number;
    /** The card that takes the Tab stop of the grid. */
    active: boolean;
    onopen: (note: NoteSummary) => void;
    onnavigate: (event: KeyboardEvent, index: number) => void;
    onfocuscard: (index: number) => void;
    onrenamed: (path: string) => void;
    /** « Reprendre en mode direct » (M12): live mode takes the window, on this note. */
    onresume?: (note: NoteSummary) => void;
  } = $props();

  let src = $state<string | null>(null);
  let host = $state<HTMLElement | null>(null);
  let renaming = $state(false);
  let draft = $state('');
  let input = $state<HTMLInputElement | null>(null);
  let dragOut = $state<DragOutPhase>('idle');

  const locale = $derived(currentLocale());
  const title = $derived(displayTitle(note, locale));
  const selected = $derived(lib.isSelected(note.path));
  const dragCount = $derived(lib.dragOutCount(note.path));
  const dragLabel = $derived(
    dragCount > 1 ? t('library.drag.handleMany', { count: dragCount }) : t('library.drag.handle'),
  );
  /** Why the note cannot be resumed in live mode now (a tablet is needed; one note at a time). */
  const resumeHint = $derived(
    lib.selected.length > 1
      ? t('live.resume.severalSelected')
      : (() => {
          const block = resumeBlock(tablet.status);
          return block === 'noTablet' ? t('live.resume.noTablet') : block === 'busy' ? t('live.resume.tabletBusy') : null;
        })(),
  );
  const stampKey = $derived(
    `${note.stamp.modified.secs_since_epoch}.${note.stamp.modified.nanos_since_epoch}.${note.stamp.len}`,
  );

  // The thumbnail is fetched when the card scrolls near the viewport (at once without IntersectionObserver).
  $effect(() => {
    const path = note.path;
    void stampKey;
    if (!host) return;
    let cancelled = false;
    const stop = whenNearViewport(host, () => {
      void lib.thumbnail(path, untrack(() => note.stamp)).then((url) => {
        if (!cancelled) src = url;
      });
    });
    return () => {
      cancelled = true;
      stop();
    };
  });

  $effect(() => {
    if (renaming) {
      input?.focus();
      input?.select();
    }
  });

  /** Click or Enter: Shift selects a range, Ctrl toggles, otherwise the note opens. */
  function activate(event: MouseEvent | KeyboardEvent) {
    if (event.shiftKey) lib.selectRange(note.path);
    else if (event.ctrlKey || event.metaKey) lib.toggle(note.path);
    else onopen(note);
  }

  function keydown(event: KeyboardEvent) {
    const key = event.key;
    if (key === 'Enter') {
      event.preventDefault();
      activate(event);
    } else if (key === ' ') {
      event.preventDefault();
      if (event.shiftKey) lib.selectRange(note.path);
      else lib.toggle(note.path);
    } else if ((event.ctrlKey || event.metaKey) && key.toLowerCase() === 'a') {
      event.preventDefault();
      lib.selectAll();
    } else if (key === 'Escape') {
      lib.clearSelection();
    } else if (key === 'Delete') {
      event.preventDefault();
      void lib.trashNotes(lib.isSelected(note.path) ? [...lib.selected] : [note.path]);
    } else if (key === 'F2') {
      event.preventDefault();
      startRename();
    } else if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(key)) {
      onnavigate(event, index);
    }
  }

  // Space activates a button on key release: keep it from opening the note.
  function keyup(event: KeyboardEvent) {
    if (event.key === ' ') event.preventDefault();
  }

  function dragstart(event: DragEvent) {
    const paths = lib.beginDrag(note.path);
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('application/x-ardoise-notes', JSON.stringify(paths));
      event.dataTransfer.setData('text/plain', paths.join('\n'));
    }
  }

  /** Pointer down on the drag handle: the note (or the whole selection) goes out as PNG files. */
  function dragOutStart(event: PointerEvent) {
    startDragOut(event, {
      paths: () => lib.dragOutPaths(note.path),
      onphase: (phase) => (dragOut = phase),
      onerror: (message) => (lib.error = message),
    });
  }

  function startRename() {
    draft = note.title;
    renaming = true;
  }

  async function finishRename(commit: boolean) {
    if (!renaming) return;
    renaming = false;
    if (!commit || draft.trim() === note.title.trim()) return;
    const path = await lib.retitle(note, draft);
    if (path) {
      await tick();
      onrenamed(path);
    }
  }

  function renameKey(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      void finishRename(true);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      void finishRename(false);
    }
  }
</script>

{#snippet thumb()}
  <span class="thumb" bind:this={host}>
    {#if src}
      <img {src} alt="" draggable="false" onload={noteDrawn} />
    {:else}
      <span class="thumb-placeholder" aria-hidden="true"></span>
    {/if}
  </span>
{/snippet}

<li class="note-card" class:selected class:conflict={note.conflict} data-path={note.path}>
  <label class="card-check" title={t('library.card.select', { title })}>
    <input
      type="checkbox"
      checked={selected}
      aria-label={t('library.card.select', { title })}
      onchange={() => lib.toggle(note.path)}
    />
  </label>

  {#if renaming}
    <div class="card-open card-renaming">
      {@render thumb()}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="rename-input"
        bind:this={input}
        bind:value={draft}
        aria-label={t('library.card.renameLabel')}
        autofocus
        onkeydown={renameKey}
        onblur={() => finishRename(true)}
      />
    </div>
  {:else}
    <button
      type="button"
      class="card-open"
      data-index={index}
      tabindex={active ? 0 : -1}
      draggable="true"
      aria-label={t('library.card.open', { title })}
      onclick={activate}
      onkeydown={keydown}
      onkeyup={keyup}
      onfocus={() => onfocuscard(index)}
      ondragstart={dragstart}
      ondragend={() => lib.endDrag()}
    >
      {@render thumb()}
      <span class="card-title">{title}</span>
      <span class="card-date">{formatDate(note.created, locale)}</span>
    </button>
  {/if}

  {#if lib.dragOutSupported && !renaming}
    <button
      type="button"
      class="icon-button drag-handle"
      class:busy={dragOut !== 'idle'}
      tabindex="-1"
      aria-label={dragLabel}
      title={dragLabel}
      onpointerdown={dragOutStart}
    >
      <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false">
        <path d="M2 9v4.5h12V9M8 2v7.5M5 6.5 8 9.5l3-3" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
  {/if}
  {#if dragOut === 'preparing'}
    <span class="badge drag-status" role="status">
      {dragCount > 1 ? t('library.drag.preparingMany', { count: dragCount }) : t('library.drag.preparing')}
    </span>
  {/if}

  <button
    type="button"
    class="icon-button star"
    class:on={note.favorite}
    aria-label={note.favorite ? t('library.card.unfavorite') : t('library.card.favorite')}
    onclick={() => lib.toggleFavorite(note)}
  >
    <span aria-hidden="true">{note.favorite ? '★' : '☆'}</span>
  </button>

  <div class="card-menu">
    <Menu
      label={t('library.card.menu', { title })}
      items={[
        { id: 'rename', label: t('library.card.rename'), onselect: startRename },
        {
          id: 'favorite',
          label: note.favorite ? t('library.card.unfavorite') : t('library.card.favorite'),
          onselect: () => void lib.toggleFavorite(note),
        },
        ...(onresume
          ? [
              {
                id: 'resume',
                label: t('live.resume.action'),
                disabled: resumeHint !== null,
                hint: resumeHint ?? undefined,
                onselect: () => onresume(note),
              },
            ]
          : []),
        { id: 'trash', label: t('library.card.trash'), danger: true, onselect: () => void lib.trashNotes([note.path]) },
      ]}
    />
  </div>

  {#if note.conflict}
    <span class="badge conflict-badge" title={t('library.card.conflictHint', { name: note.conflict.originalName })}>
      {t('library.card.conflict')}
    </span>
  {/if}

  {#if note.tags.length > 0}
    <ul class="chips" aria-label={t('library.tags')}>
      {#each note.tags as tag (tag)}
        {@const color = lib.tagColors.of(tag)}
        <li>
          <button
            type="button"
            class="chip"
            data-tag-color={color}
            aria-label={t('library.card.filterTag', { tag })}
            onclick={() => lib.setView({ kind: 'tag', tag })}
            >{#if color}<span class="tag-dot" aria-hidden="true"></span>{/if}{tag}</button
          >
        </li>
      {/each}
    </ul>
  {/if}
</li>
