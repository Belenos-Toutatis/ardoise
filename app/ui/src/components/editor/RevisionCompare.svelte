<script lang="ts">
  /**
   * « Comparer »: the original and this copy side by side (one above the other in a narrow
   * window). Rust draws each page as an image about the size it has here (`note_picture`: the
   * copy as last saved, with no zoom). The pictures are asked for again, with a delay, when the
   * window is resized to a clearly different size.
   */
  import { onDestroy, untrack } from 'svelte';
  import { api, errorMessage } from '../../lib/api';
  import { t } from '../../lib/i18n.svelte';
  import { revisionSourceTitle, type RevisionOfNote } from '../../lib/note';
  import Modal from '../Modal.svelte';

  let {
    revision,
    path,
    dirty = false,
    onclose,
  }: {
    revision: RevisionOfNote;
    /** The copy's file. */
    path: string;
    /** The copy has changes not saved yet: they are not in the picture. */
    dirty?: boolean;
    onclose: () => void;
  } = $props();

  /** Below this width of the dialog the two pages are one above the other (CSS follows). */
  const SIDE_BY_SIDE_MIN = 640;
  /** The box is rounded to this many pixels, so that a small change of size asks for nothing. */
  const STEP = 50;
  /** Height the dialog keeps for its title, captions, button and margins, in CSS pixels. */
  const CHROME = 230;

  let innerWidth = $state(1024);
  let innerHeight = $state(768);
  let stage = $state<HTMLElement | null>(null);
  /** The width the pages share, measured once laid out (0 until then, and in a test). */
  let stageWidth = $state(0);

  /** One side of the comparison. */
  interface Page {
    url: string | null;
    error: string | null;
  }
  let original = $state.raw<Page>({ url: null, error: null });
  let copy = $state.raw<Page>({ url: null, error: null });

  const source = $derived(revision.source);
  const width = $derived(stageWidth || Math.min(1100, innerWidth - 80));
  const stacked = $derived(width < SIDE_BY_SIDE_MIN);
  /** The box of one page, CSS pixels, rounded. */
  const box = $derived.by(() => {
    const available = Math.max(160, innerHeight - CHROME);
    const w = stacked ? width : (width - 16) / 2;
    const h = stacked ? (available - 24) / 2 : available;
    const round = (v: number) => Math.max(STEP, Math.round(v / STEP) * STEP);
    return { w: round(w), h: round(h) };
  });

  $effect(() => {
    if (!stage) return;
    const el = stage;
    const measure = () => (stageWidth = el.clientWidth);
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  });

  /** Only the answer to the latest request counts. */
  let request = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let first = true;

  async function load(target: { w: number; h: number }, ids: number) {
    if (!source) return;
    const ratio = Math.min(2, window.devicePixelRatio || 1);
    const [w, h] = [Math.round(target.w * ratio), Math.round(target.h * ratio)];
    const draw = (file: string): Promise<Page> =>
      api.notePicture(file, w, h).then(
        (url) => ({ url, error: null }),
        (error: unknown) => ({ url: null, error: errorMessage(error) }),
      );
    const [a, b] = await Promise.all([draw(source.path), draw(path)]);
    if (ids !== request) return;
    original = a;
    copy = b;
  }

  /**
   * The rounded box as a string: `box` is a new object at each recompute, so an effect that read it
   * would run (discarding the answer in flight, drawing both pages again) at every 1 px resize;
   * a string compares by value and only a change of the rounded size runs the effect.
   */
  const boxKey = $derived(`${box.w}x${box.h}`);

  $effect(() => {
    const [w, h] = boxKey.split('x').map(Number);
    const target = { w, h };
    const ids = ++request;
    clearTimeout(timer);
    // The first drawing starts at once; a resize waits until the window stops moving.
    if (first) {
      first = false;
      untrack(() => void load(target, ids));
    } else {
      timer = setTimeout(() => void load(target, ids), 300);
    }
  });

  onDestroy(() => {
    request += 1;
    clearTimeout(timer);
  });
</script>

<svelte:window bind:innerWidth bind:innerHeight />

<Modal title={t('editor.revision.comparison.title')} {onclose}>
  <div data-dialog="revision-compare">
    {#if dirty}
      <p class="muted small" data-slot="revision-compare-unsaved">{t('editor.revision.comparison.unsaved')}</p>
    {/if}
    <div class="compare-stage" class:stacked bind:this={stage} data-slot="revision-compare-stage">
      {#each [{ slot: 'original', page: original, caption: t('editor.revision.comparison.original', { title: revisionSourceTitle(revision, t('editor.untitled')) }), alt: t('editor.revision.comparison.originalAlt') }, { slot: 'copy', page: copy, caption: t('editor.revision.comparison.copy'), alt: t('editor.revision.comparison.copyAlt') }] as side (side.slot)}
        <figure class="compare-page" data-slot={`revision-compare-${side.slot}`} style:--compare-h={`${box.h}px`}>
          <figcaption>{side.caption}</figcaption>
          <div class="compare-frame">
            {#if side.page.url}
              <img src={side.page.url} alt={side.alt} />
            {:else if side.page.error !== null}
              <p class="banner error inline" role="alert">{t('editor.revision.comparison.failed', { message: side.page.error })}</p>
            {:else}
              <p class="muted" role="status">{t('editor.revision.comparison.loading')}</p>
            {/if}
          </div>
        </figure>
      {/each}
    </div>
    <div class="actions">
      <button type="button" class="button primary" data-autofocus data-action="revision-close" onclick={onclose}>
        {t('common.close')}
      </button>
    </div>
  </div>
</Modal>
