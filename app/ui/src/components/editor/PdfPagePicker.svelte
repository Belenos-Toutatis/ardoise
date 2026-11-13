<script lang="ts">
  /**
   * « Which page goes in the background? » for a PDF of several pages (`BackgroundImageSection.svelte`: the editor's
   * and live mode's Fond tab): a grid of page thumbnails, two to a row in the side panel, each with its number under
   * it. The thumbnails are drawn by Rust (`pdf_preview.rs`, Windows' PDF renderer) when they come into view, two at
   * a time (`lib/pdf-thumbnails.svelte.ts`), and forgotten with the PDF when the picker goes away or its file changes.
   *
   * A radio group, for pick-one: a single tab stop (the chosen page), the arrows move the choice (left and right by a
   * page, up and down by a row), Home and End go to the first and the last page, Enter and Space choose the page that
   * has the focus. Each page says « Page 3 » to a screen reader; the picture is decoration. A long PDF also gets a
   * number field beside the grid to go straight to a page, kept in step with it.
   */
  import { tick } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import { PdfThumbnails, thumbnailPixelWidth } from '../../lib/pdf-thumbnails.svelte';

  let {
    path,
    name,
    pages,
    selected = $bindable(1),
    disabled = false,
  }: {
    /** The PDF's file. */
    path: string;
    /** Its name, for the group's label. */
    name: string;
    /** Each page's size in points. */
    pages: { width: number; height: number }[];
    /** The chosen page, from 1. */
    selected?: number;
    disabled?: boolean;
  } = $props();

  /** Thumbnails to a row (the grid's columns), which the up and down arrows move by. */
  const COLUMNS = 2;
  /** From this many pages, the number field is shown (a short PDF is picked at a glance). */
  const JUMP_FROM = 8;
  /** Proportions drawn: a very wide or very tall page is shown in this range (the image sits inside, whole). */
  const RATIO_MIN = 0.4;
  const RATIO_MAX = 2.5;

  const count = $derived(pages.length);

  let strip = $state<HTMLElement | null>(null);
  let thumbs = $state.raw<PdfThumbnails | null>(null);

  // The PDF's preview lives as long as the picker shows this file: closed when the question goes away or another
  // file is chosen (the cleanup), which forgets the images and frees the PDF in Rust.
  $effect(() => {
    const preview = new PdfThumbnails(path, thumbnailPixelWidth(window.devicePixelRatio));
    thumbs = preview;
    return () => {
      preview.close();
      thumbs = null;
    };
  });

  // A page is drawn when it comes into view (within a row of the visible part of the grid), not before.
  $effect(() => {
    const root = strip;
    const preview = thumbs;
    void count;
    if (!root || !preview) return;
    const cells = root.querySelectorAll<HTMLElement>('[data-page]');
    if (typeof IntersectionObserver === 'undefined') {
      // No observer (a test): everything is in view.
      for (const cell of cells) preview.show(Number(cell.dataset.page));
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const page = Number((entry.target as HTMLElement).dataset.page);
          if (entry.isIntersecting) preview.show(page);
          else preview.hide(page);
        }
      },
      { root, rootMargin: '120px 0px' },
    );
    for (const cell of cells) observer.observe(cell);
    return () => observer.disconnect();
  });

  function ratioOf(size: { width: number; height: number }): number {
    const ratio = size.width / Math.max(size.height, 1);
    return Math.min(Math.max(Number.isFinite(ratio) ? ratio : 1, RATIO_MIN), RATIO_MAX);
  }

  function cell(page: number): HTMLElement | null {
    return strip?.querySelector<HTMLElement>(`[data-page="${page}"]`) ?? null;
  }

  /** Chooses `page`, and brings it into view (and gives it the focus when `focus`). */
  function choose(page: number, focus = false) {
    selected = Math.min(Math.max(page, 1), count);
    void tick().then(() => {
      const target = cell(selected);
      target?.scrollIntoView?.({ block: 'nearest' });
      if (focus) target?.focus();
    });
  }

  function keydown(event: KeyboardEvent) {
    if (disabled || event.altKey || event.ctrlKey || event.metaKey) return;
    const moves: Record<string, number> = {
      ArrowLeft: selected - 1,
      ArrowUp: selected - COLUMNS,
      ArrowRight: selected + 1,
      ArrowDown: selected + COLUMNS,
      Home: 1,
      End: count,
    };
    const target = moves[event.key];
    if (target === undefined) return;
    event.preventDefault();
    choose(target, true);
  }

  /** The number field: a valid page is chosen as it is typed; the field shows the choice again when it is left. */
  function jump(event: Event & { currentTarget: HTMLInputElement }) {
    const typed = Number(event.currentTarget.value);
    if (Number.isInteger(typed) && typed >= 1 && typed <= count) choose(typed);
  }
</script>

<div class="pdf-picker" data-slot="pdf-page-picker">
  {#if count >= JUMP_FROM}
    <label class="pane-row pdf-jump">
      <span>{t('background.editor.page')}</span>
      <span class="pdf-jump-field">
        <input
          type="number"
          min="1"
          max={count}
          step="1"
          inputmode="numeric"
          value={selected}
          {disabled}
          data-slot="pdf-page-number"
          oninput={jump}
          onchange={(e) => (e.currentTarget.value = String(selected))}
        />
        <span class="muted">{t('background.editor.pageOf', { count })}</span>
      </span>
    </label>
  {/if}
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="pdf-strip"
    role="radiogroup"
    aria-label={t('background.editor.pagesGroup', { name })}
    bind:this={strip}
    onkeydown={keydown}
  >
    {#each pages as size, i (i)}
      {@const page = i + 1}
      {@const url = thumbs?.urls.get(page)}
      {@const failed = thumbs?.failed.has(page) ?? false}
      <button
        type="button"
        role="radio"
        class="pdf-thumb"
        class:pdf-picked={page === selected}
        aria-checked={page === selected}
        aria-label={t('background.editor.pageN', { page })}
        tabindex={page === selected ? 0 : -1}
        data-page={page}
        {disabled}
        onclick={() => choose(page)}
      >
        <span class="pdf-frame" class:pdf-failed={failed} style:aspect-ratio={ratioOf(size)} aria-hidden="true">
          {#if url}
            <img src={url} alt="" draggable="false" />
          {:else if failed}
            <span class="pdf-missing">{t('background.editor.noPreview')}</span>
          {/if}
        </span>
        <span class="pdf-number" aria-hidden="true">{page}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .pdf-picker {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .pdf-jump-field {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .pdf-jump-field input {
    width: 4.2rem;
    padding: 0.25rem 0.4rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
  }

  /* The room around the cells keeps the chosen one's outline from being cut by the scroll. */
  .pdf-strip {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    align-items: start;
    gap: 0.5rem;
    max-height: min(24rem, 50vh);
    padding: 3px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .pdf-thumb {
    display: block;
    min-width: 0;
    padding: 0.3rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--text);
    font: inherit;
    cursor: pointer;
    scroll-margin: 6px;
  }

  .pdf-thumb:hover:not(:disabled) {
    border-color: var(--accent);
  }

  .pdf-thumb:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .pdf-thumb:disabled {
    cursor: default;
    opacity: 0.6;
  }

  /* The chosen page: an accent frame and a filled number, so that it does not rest on colour alone. */
  .pdf-thumb.pdf-picked {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent);
    background: var(--accent-soft);
  }

  /* The page's own white, whatever the theme: it is a picture of paper. */
  .pdf-frame {
    position: relative;
    display: block;
    width: 100%;
    overflow: hidden;
    border: 1px solid var(--border);
    background: #fff;
  }

  /* Waiting for its picture: a still, pale sheet (200 moving ones would cost more than they say). */
  .pdf-frame:not(:has(img, .pdf-missing)) {
    background: #f1f2f4;
  }

  .pdf-frame img {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .pdf-frame.pdf-failed {
    background: repeating-linear-gradient(135deg, #f1f2f4 0 6px, #e8eaed 6px 12px);
  }

  .pdf-missing {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.2rem;
    color: #5d6775;
    font-size: 0.66rem;
    line-height: 1.2;
    text-align: center;
  }

  .pdf-number {
    display: block;
    margin-top: 0.25rem;
    font-size: 0.8rem;
    text-align: center;
  }

  .pdf-picked .pdf-number {
    font-weight: 700;
  }
</style>
