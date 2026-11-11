<script lang="ts">
  /**
   * The layers zone, the same in the editor's side panel and in the live control window
   * (docs/design/tablet-live-classroom.md, "The layers zone"). The rows list the layers top first;
   * the row is what chooses the layer the pen, the eraser and the selection act on (the frame),
   * its name button being the keyboard's way in. In live a second mark, the tablet's icon, says
   * where the tablet writes. Nothing here changes a note: every act goes up through a callback,
   * the owner (the editor's doc, the live session) does it and hands the new list back down.
   */
  import { tick } from 'svelte';
  import { layerWarnings, rowsTopFirst } from '../../lib/layers-zone';
  import { t } from '../../lib/i18n.svelte';
  import type { Layer } from '../../lib/note';
  import LayerIcon from './LayerIcon.svelte';

  let {
    where,
    layers,
    chosen,
    tablet,
    disabled = false,
    onchoose,
    onvisible,
    onrename,
    onmove,
    onmerge,
    onadd,
    ontablet,
  }: {
    where: 'editor' | 'live';
    /** Bottom first, as in the note. */
    layers: readonly Layer[];
    /** Editor: `doc.activeLayer`; live: `live.computerLayer`. */
    chosen: number;
    /** Live: the layer the tablet writes on. Absent in the editor: no tablet icon. */
    tablet?: number;
    /** The editor's split panel is open, or the live session is not running: nothing can change. */
    disabled?: boolean;
    onchoose: (index: number) => void;
    onvisible: (index: number, visible: boolean) => void;
    /** The name is trimmed, not empty and different from the old one. */
    onrename: (index: number, name: string) => void;
    /** `to` is `from` ± 1. */
    onmove: (from: number, to: number) => void;
    /** Merges the layer into the one below it (`index - 1`). */
    onmerge: (index: number) => void;
    onadd: () => void;
    ontablet?: (index: number) => void;
  } = $props();

  const rows = $derived(rowsTopFirst(layers));
  const warnings = $derived(layerWarnings(layers, chosen, tablet));
  const top = $derived(layers.length - 1);
  const titleId = $derived(`layers-zone-title-${where}`);

  /** The layer whose name is being edited (one at a time). */
  let renaming = $state<number | null>(null);
  let zone: HTMLElement | undefined = $state();

  // A closed zone, or a list that no longer has the layer, ends the edit.
  $effect(() => {
    if (renaming !== null && (disabled || renaming >= layers.length)) renaming = null;
  });

  function focusName(index: number) {
    zone?.querySelector<HTMLElement>(`li[data-layer="${index}"] [data-action="choose-layer"]`)?.focus();
  }

  function startRename(index: number) {
    if (!disabled) renaming = index;
  }

  /** Ends the edit of `index`: keeps `value` (trimmed, unless empty or unchanged) or gives the old name back. */
  function finishRename(index: number, value: string, keep: boolean, refocus: boolean) {
    // Enter, then the blur that removing the field raises: the second finds nothing to do.
    if (renaming !== index) return;
    renaming = null;
    const name = value.trim();
    if (keep && name !== '' && name !== layers[index]?.name) onrename(index, name);
    if (refocus) void tick().then(() => focusName(index));
  }

  function renameKey(event: KeyboardEvent, index: number) {
    const field = event.currentTarget as HTMLInputElement;
    if (event.key === 'Enter') {
      event.preventDefault();
      event.stopPropagation();
      finishRename(index, field.value, true, true);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      finishRename(index, field.value, false, true);
    }
  }

  /** A click on the row outside its buttons chooses the layer (the name button's click lands here too). */
  function chooseRow(index: number) {
    if (disabled || index === chosen) return;
    onchoose(index);
  }

  /** The other buttons of a row do their own thing: their click does not reach the row. */
  function act(event: MouseEvent, action: () => void) {
    event.stopPropagation();
    action();
  }

  function focusSelect(field: HTMLInputElement) {
    field.focus();
    field.select();
  }
</script>

<section bind:this={zone} class="layers-zone pane-block" data-section="layers" data-where={where} aria-labelledby={titleId}>
  <!-- The pane's title already says « Calques »: the heading stays for the section's name, out of sight. -->
  <h3 id={titleId} class="visually-hidden">{t('layers.title')}</h3>
  <ul class="layers" aria-label={t('layers.list')}>
    {#each rows as { index, layer } (index)}
      {@const isChosen = index === chosen}
      {@const isTablet = tablet === index}
      {@const chosenLabel =
        where === 'live' ? t('layers.chosen.live', { name: layer.name }) : t('layers.chosen.editor', { name: layer.name })}
      {@const eyeLabel = layer.visible ? t('layers.hide', { name: layer.name }) : t('layers.show', { name: layer.name })}
      {@const tabletLabel = isTablet
        ? t('layers.tabletHere', { name: layer.name })
        : t('layers.tabletSend', { name: layer.name })}
      <!-- The row's click is a mouse shortcut: the keyboard's control is the name button inside it. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <li
        class="layer"
        class:chosen={isChosen}
        data-layer={index}
        data-chosen={isChosen ? 'true' : undefined}
        data-hidden={layer.visible ? undefined : 'true'}
        data-tablet={isTablet ? 'true' : undefined}
        onclick={() => chooseRow(index)}
      >
        <button
          type="button"
          class="zone-button eye"
          class:off={!layer.visible}
          data-action="layer-visible"
          data-visible={layer.visible ? 'true' : 'false'}
          {disabled}
          aria-label={eyeLabel}
          title={eyeLabel}
          onclick={(e) => act(e, () => onvisible(index, !layer.visible))}
        >
          <LayerIcon name={layer.visible ? 'eye' : 'eyeOff'} />
        </button>

        {#if renaming === index}
          <input
            class="layer-rename"
            type="text"
            value={layer.name}
            aria-label={t('layers.nameField', { n: index + 1 })}
            use:focusSelect
            onclick={(e) => e.stopPropagation()}
            onkeydown={(e) => renameKey(e, index)}
            onblur={(e) => finishRename(index, e.currentTarget.value, true, false)}
          />
        {:else}
          <button
            type="button"
            class="layer-name"
            data-action="choose-layer"
            aria-pressed={isChosen}
            {disabled}
            aria-label={isChosen ? chosenLabel : t('layers.choose', { name: layer.name })}
            title={isChosen ? chosenLabel : t('layers.choose', { name: layer.name })}
          >
            {layer.name}
          </button>
          <button
            type="button"
            class="zone-button pencil"
            data-action="rename-layer"
            {disabled}
            aria-label={t('layers.rename', { name: layer.name })}
            title={t('layers.rename', { name: layer.name })}
            onclick={(e) => act(e, () => startRename(index))}
          >
            <LayerIcon name="pencil" />
          </button>
        {/if}

        {#if where === 'live' && tablet !== undefined}
          <button
            type="button"
            class="zone-button tablet-button"
            class:here={isTablet}
            class:elsewhere={!isTablet}
            class:warn={isTablet && !layer.visible}
            data-action="tablet-layer"
            data-here={isTablet ? 'true' : undefined}
            {disabled}
            aria-label={tabletLabel}
            title={tabletLabel}
            onclick={(e) =>
              act(e, () => {
                if (!isTablet) ontablet?.(index);
              })}
          >
            <LayerIcon name="tablet" />
          </button>
        {/if}

        {#if isChosen}
          <span class="order">
            <button
              type="button"
              class="zone-button step"
              data-action="layer-up"
              disabled={disabled || index === top}
              aria-label={t('layers.up', { name: layer.name })}
              title={t('layers.up', { name: layer.name })}
              onclick={(e) => act(e, () => onmove(index, index + 1))}>↑</button
            >
            <button
              type="button"
              class="zone-button step"
              data-action="layer-down"
              disabled={disabled || index === 0}
              aria-label={t('layers.down', { name: layer.name })}
              title={t('layers.down', { name: layer.name })}
              onclick={(e) => act(e, () => onmove(index, index - 1))}>↓</button
            >
            <button
              type="button"
              class="zone-button step"
              data-action="layer-merge"
              disabled={disabled || index === 0}
              aria-label={t('layers.merge', { name: layer.name })}
              title={where === 'live'
                ? `${t('layers.merge', { name: layer.name })} ${t('layers.mergeNoUndo')}`
                : t('layers.merge', { name: layer.name })}
              onclick={(e) => act(e, () => onmerge(index))}>⤓</button
            >
          </span>
        {/if}
      </li>
    {/each}
  </ul>

  <button
    type="button"
    class="button add"
    data-action="add-layer"
    {disabled}
    aria-label={t('layers.add')}
    onclick={onadd}
  >
    + {t('layers.add')}
  </button>

  {#each warnings as warning (warning.kind)}
    <p class="notice" role="status" data-slot="layer-warning" data-kind={warning.kind}>
      {warning.kind === 'tabletHidden'
        ? t('layers.warn.tabletHidden', { name: warning.name })
        : t('layers.warn.chosenHidden', { name: warning.name })}
    </p>
  {/each}

  <p class="help" data-slot="layers-help">{where === 'live' ? t('layers.help.live') : t('layers.help.editor')}</p>
</section>

<style>
  .layers-zone {
    --zone-warn: #b54708;
    --zone-warn-bg: #fef0c7;
    /* The block's padding, gap and rule are the pane's (`.pane-block`, SidePanel.svelte). */
    display: flex;
    flex-direction: column;
  }

  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light'])) .layers-zone {
      --zone-warn: #fdb022;
      --zone-warn-bg: #3b2a0b;
    }
  }

  :global(:root[data-theme='dark']) .layers-zone {
    --zone-warn: #fdb022;
    --zone-warn-bg: #3b2a0b;
  }

  .layers {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .layer {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    min-height: 2.2rem;
    padding: 0.2rem 0.3rem;
    border: 1px solid transparent;
    border-radius: 6px;
    cursor: pointer;
  }

  .layer:hover {
    background: var(--surface-2);
  }

  .layer.chosen {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
  }

  .layer-name {
    flex: 1;
    min-width: 0;
    padding: 0 0.2rem;
    overflow: hidden;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  .layer-name:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .layer[data-hidden='true'] .layer-name {
    color: var(--muted);
    font-style: italic;
  }

  .layer-rename {
    flex: 1;
    min-width: 0;
    padding: 0.1rem 0.3rem;
    border: 1px solid var(--accent);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
  }

  .zone-button {
    display: grid;
    flex: none;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 5px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }

  .zone-button:hover:not(:disabled) {
    border-color: var(--border);
    background: var(--surface);
  }

  .zone-button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .zone-button:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .zone-button :global(svg) {
    width: 1.05rem;
    height: 1.05rem;
  }

  .eye.off,
  .pencil {
    color: var(--muted);
  }

  .tablet-button.here {
    color: var(--accent);
  }

  .tablet-button.here.warn {
    color: var(--zone-warn);
  }

  /* The tablet's other places show on hover or focus; a finger has no hover, so they are always there then. */
  .tablet-button.elsewhere {
    color: var(--muted);
    opacity: 0;
  }

  .layer:hover .tablet-button.elsewhere:not(:disabled),
  .tablet-button.elsewhere:focus-visible {
    opacity: 0.55;
  }

  @media (hover: none) {
    .tablet-button.elsewhere:not(:disabled) {
      opacity: 0.55;
    }
  }

  .order {
    display: flex;
    gap: 0.1rem;
  }

  .step {
    width: 1.45rem;
    font-size: 0.85rem;
  }

  .add {
    align-self: flex-start;
    padding: 0.25rem 0.6rem;
    font-size: 0.875rem;
  }

  .add:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .notice {
    margin: 0;
    padding: 0.4rem 0.55rem;
    border-radius: 6px;
    background: var(--zone-warn-bg);
    color: var(--zone-warn);
    font-size: 0.82rem;
  }

  .help {
    margin: 0;
    color: var(--muted);
    font-size: 0.82rem;
  }
</style>
