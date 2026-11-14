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
   *
   * With `multiple` (« Nouvelle note depuis un PDF »: each chosen page becomes a note) it is a group of checkboxes
   * instead, `picked` holding the chosen pages in order: a click toggles a page, Shift + click (or Shift + Space) sets
   * every page from the last one clicked to this one to the state of the last one clicked, Ctrl + A picks them all.
   * The arrows only move the focus (a single tab stop, as for the radios), Space and Enter toggle the page that has it,
   * and Shift + an arrow moves and extends the range. `onpick` hears of every change the user makes (not of those the
   * parent makes through `picked`), with the new list.
   */
  import { tick } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import { PdfThumbnails, thumbnailPixelWidth } from '../../lib/pdf-thumbnails.svelte';

  let {
    path,
    name,
    pages,
    selected = $bindable(1),
    multiple = false,
    picked = $bindable([]),
    onpick,
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
    /** Several pages can be chosen (`picked`) instead of one (`selected`). */
    multiple?: boolean;
    /** The chosen pages, from 1, in order, with `multiple`. */
    picked?: number[];
    /** The user changed the choice (`multiple`): the pages now chosen. */
    onpick?: (pages: number[]) => void;
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
  const pickedSet = $derived(new Set(picked));
  /** The page that has the tab stop and the focus with `multiple`, and the one a range starts from. */
  let active = $state(1);
  let anchor = $state<number | null>(null);

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
    reveal(selected, focus);
  }

  function reveal(page: number, focus: boolean) {
    void tick().then(() => {
      const target = cell(page);
      target?.scrollIntoView?.({ block: 'nearest' });
      if (focus) target?.focus();
    });
  }

  /** `pages` chosen (any order, repeats allowed), told to the parent. */
  function pick(pages: Iterable<number>) {
    picked = [...new Set(pages)].sort((a, b) => a - b);
    onpick?.(picked);
  }

  /** Every page from `from` to `to` set to `on`. */
  function setRange(from: number, to: number, on: boolean) {
    const next = new Set(picked);
    for (let page = Math.min(from, to); page <= Math.max(from, to); page++) {
      if (on) next.add(page);
      else next.delete(page);
    }
    pick(next);
  }

  /** A click on `page` with `multiple`: toggles it, or with Shift sets the range from the last one clicked. */
  function toggle(page: number, shift: boolean) {
    active = page;
    if (shift && anchor !== null) {
      setRange(anchor, page, pickedSet.has(anchor));
      return;
    }
    anchor = page;
    const next = new Set(picked);
    if (!next.delete(page)) next.add(page);
    pick(next);
  }

  /** The grid's columns as laid out now (the arrows up and down move by a row). */
  function columns(): number {
    const tracks = strip ? getComputedStyle(strip).gridTemplateColumns : '';
    const found = tracks.split(' ').filter((track) => /\d/.test(track)).length;
    return found > 0 ? found : COLUMNS;
  }

  function keydownMany(event: KeyboardEvent) {
    if (disabled || event.altKey || event.metaKey) return;
    if ((event.ctrlKey && event.key.toLowerCase() === 'a')) {
      event.preventDefault();
      pick(Array.from({ length: count }, (_, i) => i + 1));
      return;
    }
    if (event.ctrlKey) return;
    const here = Number((document.activeElement as HTMLElement | null)?.dataset.page ?? active);
    const moves: Record<string, number> = {
      ArrowLeft: here - 1,
      ArrowUp: here - columns(),
      ArrowRight: here + 1,
      ArrowDown: here + columns(),
      Home: 1,
      End: count,
    };
    const target = moves[event.key];
    if (target === undefined) return;
    event.preventDefault();
    const next = Math.min(Math.max(target, 1), count);
    if (event.shiftKey && anchor !== null) setRange(anchor, next, pickedSet.has(anchor));
    active = next;
    reveal(next, true);
  }

  function keydown(event: KeyboardEvent) {
    if (multiple) return keydownMany(event);
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
  {#if count >= JUMP_FROM && !multiple}
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
    class:pdf-many={multiple}
    role={multiple ? 'group' : 'radiogroup'}
    aria-label={t('background.editor.pagesGroup', { name })}
    bind:this={strip}
    onkeydown={keydown}
  >
    {#each pages as size, i (i)}
      {@const page = i + 1}
      {@const url = thumbs?.urls.get(page)}
      {@const failed = thumbs?.failed.has(page) ?? false}
      {@const on = multiple ? pickedSet.has(page) : page === selected}
      <button
        type="button"
        role={multiple ? 'checkbox' : 'radio'}
        class="pdf-thumb"
        class:pdf-picked={on}
        aria-checked={on}
        aria-label={t('background.editor.pageN', { page })}
        tabindex={(multiple ? page === active : page === selected) ? 0 : -1}
        data-page={page}
        {disabled}
        onclick={(event) => (multiple ? toggle(page, event.shiftKey) : choose(page))}
      >
        <span class="pdf-frame" class:pdf-failed={failed} style:aspect-ratio={ratioOf(size)} aria-hidden="true">
          {#if url}
            <img src={url} alt="" draggable="false" />
          {:else if failed}
            <span class="pdf-missing">{t('background.editor.noPreview')}</span>
          {/if}
        </span>
        <span class="pdf-number" aria-hidden="true">{multiple && on ? `✓ ${page}` : page}</span>
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

  /* Several pages: more columns, as many as the dialog's width holds. */
  .pdf-strip.pdf-many {
    grid-template-columns: repeat(auto-fill, minmax(6.5rem, 1fr));
    max-height: min(22rem, 45vh);
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
