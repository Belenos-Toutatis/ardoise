<script lang="ts">
  /** Layers, top first: active layer, show/hide, rename, reorder, merge down, add. */
  import { t } from '../../lib/i18n.svelte';
  import type { Layer } from '../../lib/note';

  let {
    layers,
    active,
    onactive,
    onvisible,
    onrename,
    onmove,
    onmerge,
    onadd,
  }: {
    layers: Layer[];
    active: number;
    onactive: (index: number) => void;
    onvisible: (index: number, visible: boolean) => void;
    onrename: (index: number, name: string) => void;
    onmove: (from: number, to: number) => void;
    onmerge: (index: number) => void;
    onadd: () => void;
  } = $props();

  /** Indices from the top layer down. */
  const order = $derived(layers.map((_, i) => layers.length - 1 - i));

  function rename(index: number, input: HTMLInputElement) {
    const name = input.value.trim();
    if (name) onrename(index, name);
    else input.value = layers[index].name;
  }
</script>

<section class="side-section" aria-labelledby="editor-layers-heading">
  <h3 id="editor-layers-heading">{t('editor.layers.label')}</h3>
  <ul class="layers">
    {#each order as index (index)}
      {@const layer = layers[index]}
      {@const n = String(index + 1)}
      <li class="layer" class:active={index === active} data-layer={index}>
        <input
          type="radio"
          name="editor-active-layer"
          checked={index === active}
          aria-label={t('editor.layers.active', { name: layer.name })}
          title={t('editor.layers.active', { name: layer.name })}
          onchange={() => onactive(index)}
        />
        <input
          type="checkbox"
          checked={layer.visible}
          aria-label={t('editor.layers.visible', { name: layer.name })}
          title={t('editor.layers.visible', { name: layer.name })}
          onchange={(e) => onvisible(index, e.currentTarget.checked)}
        />
        <input
          class="name"
          type="text"
          value={layer.name}
          aria-label={t('editor.layers.name', { n })}
          onchange={(e) => rename(index, e.currentTarget)}
          onkeydown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur();
          }}
        />
        <button
          type="button"
          class="icon"
          disabled={index === layers.length - 1}
          aria-label={t('editor.layers.up', { name: layer.name })}
          title={t('editor.layers.up', { name: layer.name })}
          onclick={() => onmove(index, index + 1)}>↑</button
        >
        <button
          type="button"
          class="icon"
          disabled={index === 0}
          aria-label={t('editor.layers.down', { name: layer.name })}
          title={t('editor.layers.down', { name: layer.name })}
          onclick={() => onmove(index, index - 1)}>↓</button
        >
        <button
          type="button"
          class="icon"
          disabled={index === 0}
          aria-label={t('editor.layers.merge', { name: layer.name })}
          title={t('editor.layers.merge', { name: layer.name })}
          onclick={() => onmerge(index)}>⤓</button
        >
      </li>
    {/each}
  </ul>
  <button type="button" class="button small-button" onclick={onadd}>+ {t('editor.layers.add')}</button>
</section>
