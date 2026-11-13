<script lang="ts">
  /**
   * The options of one tablet (`[tablet]` of `ardoise.toml`), the one `kind` says. Both tablets have automatic sync and
   * deleting pages after a verified import (on by default): those two are one setting for the library, not one each,
   * and the panel says so when both tablets are known. The Bamboo Slate has besides the button's action in live mode
   * and how it is held (four positions, each with its picture); the Xiaomi blackboard has its own four positions
   * (`boardPosition`, with its own pictures) and a hint about its pages. Shared by the tablet panel and the settings;
   * changes go through the shared tablet state. `compact` (the panel, which must not scroll) folds the long hints
   * behind « More details »; they stay in the page for assistive technology.
   */
  import { onMount } from 'svelte';
  import { BOARD_POSITIONS, boardPositionOf } from '../../lib/board-position';
  import { t } from '../../lib/i18n.svelte';
  import { ORIENTATIONS } from '../../lib/orientation';
  import type { LiveButton, TabletKind } from '../../lib/tablet';
  import { tablet } from '../../lib/tablet-state.svelte';
  import BoardDrawing from './BoardDrawing.svelte';
  import OrientationIcon from './OrientationIcon.svelte';

  let { kind, compact = false }: { kind: TabletKind; compact?: boolean } = $props();

  onMount(() => tablet.start());

  const uid = $props.id();
  const options = $derived(tablet.status?.options ?? null);
  /** Both tablets are known: the two common options are shared, which is worth saying. */
  const both = $derived(tablet.slate !== null && tablet.board !== null);

  const BUTTONS: LiveButton[] = ['newPage', 'newLayer'];
</script>

<!-- A hint under its control (`aria-describedby` points to it); a long one is folded when `compact`. -->
{#snippet hint(id: string, key: string, long: boolean, slot?: string)}
  {#if compact && long}
    <details class="more">
      <summary>{t('tablet.options.more')}</summary>
      <p {id} class="muted small" data-slot={slot}>{t(key)}</p>
    </details>
  {:else}
    <p {id} class="muted small hint" data-slot={slot}>{t(key)}</p>
  {/if}
{/snippet}

{#if options}
  <div class="tablet-options" class:compact data-slot="tablet-options" data-kind={kind}>
    <label class="choice">
      <input
        type="checkbox"
        role="switch"
        data-option="autoSync"
        checked={options.autoSync}
        aria-describedby="{uid}-auto"
        onchange={(event) => tablet.setOptions({ autoSync: event.currentTarget.checked })}
      />
      {t('tablet.options.autoSync')}
    </label>
    {@render hint(`${uid}-auto`, 'tablet.options.autoSyncHint', true)}

    <label class="choice">
      <input
        type="checkbox"
        role="switch"
        data-option="deleteAfterImport"
        checked={options.deleteAfterImport}
        aria-describedby="{uid}-delete"
        onchange={(event) => tablet.setOptions({ deleteAfterImport: event.currentTarget.checked })}
      />
      {t('tablet.options.deleteAfterImport')}
    </label>
    {#if kind === 'blackboard'}
      {@render hint(`${uid}-delete`, 'tablet.options.deleteAfterImportBoard', true, 'board-delete-hint')}
    {:else}
      {@render hint(`${uid}-delete`, 'tablet.options.deleteAfterImportHint', true)}
    {/if}
    {#if both}
      <p class="muted small hint" data-slot="shared-options">{t('tablet.options.shared')}</p>
    {/if}

    {#if kind === 'slate'}
      <div class="group" role="radiogroup" aria-label={t('tablet.options.button')}>
        <span class="group-label">{t('tablet.options.button')}</span>
        <div class="row">
          {#each BUTTONS as button (button)}
            <label class="choice">
              <input
                type="radio"
                name="{uid}-button"
                data-option="button"
                value={button}
                checked={options.button === button}
                onchange={() => tablet.setOptions({ button })}
              />
              {t(button === 'newPage' ? 'tablet.options.buttonNewPage' : 'tablet.options.buttonNewLayer')}
            </label>
          {/each}
        </div>
      </div>

      <div
        class="group"
        role="radiogroup"
        aria-label={t('tablet.options.orientation')}
        aria-describedby="{uid}-orientation"
      >
        <span class="group-label">{t('tablet.options.orientation')}</span>
        {#each ORIENTATIONS as orientation (orientation)}
          <label class="choice position">
            <input
              type="radio"
              name="{uid}-orientation"
              data-option="orientation"
              value={orientation}
              checked={options.orientation === orientation}
              onchange={() => tablet.setOptions({ orientation })}
            />
            <span class="drawing"><OrientationIcon {orientation} size={compact ? 28 : 34} /></span>
            {t(`tablet.orientation.${orientation}`)}
          </label>
        {/each}
        {@render hint(`${uid}-orientation`, 'tablet.options.orientationHint', true)}
      </div>
    {:else}
      <div
        class="group"
        role="radiogroup"
        aria-label={t('tablet.options.boardPosition')}
        aria-describedby="{uid}-board-position"
      >
        <span class="group-label">{t('tablet.options.boardPosition')}</span>
        {#each BOARD_POSITIONS as position (position)}
          <label class="choice position">
            <input
              type="radio"
              name="{uid}-board-position"
              data-option="boardPosition"
              value={position}
              checked={boardPositionOf(options) === position}
              onchange={() => tablet.setOptions({ boardPosition: position })}
            />
            <span class="drawing"><BoardDrawing {position} size={compact ? 30 : 35} /></span>
            {t(`tablet.boardPosition.${position}`)}
          </label>
        {/each}
        {@render hint(`${uid}-board-position`, 'tablet.options.boardPositionHint', false)}
      </div>
    {/if}
  </div>
{/if}

<style>
  .tablet-options {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .hint {
    margin: 0 0 0.6rem 1.6rem;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    margin-top: 0.4rem;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 1.1rem;
  }

  .position {
    align-items: center;
    gap: 0.5rem;
  }

  /* A frame of the same size for every picture, so that the labels line up. */
  .drawing {
    display: grid;
    flex: none;
    place-items: center;
    width: 35px;
    height: 35px;
  }

  .compact .position {
    gap: 0.4rem;
    font-size: 0.875rem;
  }

  .compact .drawing {
    width: 30px;
    height: 30px;
  }

  .group-label {
    font-size: 0.875rem;
    font-weight: 600;
  }

  /* « More details »: the long hint, folded. */
  .more {
    margin: 0 0 0.45rem 1.6rem;
    font-size: 0.8125rem;
  }

  .more summary {
    color: var(--muted);
    cursor: pointer;
  }

  .more summary:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .more p {
    margin: 0.25rem 0 0;
  }
</style>
