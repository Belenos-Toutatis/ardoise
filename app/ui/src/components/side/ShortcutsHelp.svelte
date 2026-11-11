<script lang="ts">
  /**
   * The Aide tab's content (milestone M16; docs/design/side-panel.md, §2 "Help, share block"): the window's shortcuts,
   * rendered from its table (`lib/editor/shortcuts.ts`, `lib/live-shortcuts.ts`), which the tests confront with the
   * keys the window really handles. The entries without a group first, then one `h4` and list per group present.
   * Each entry is a `dt[data-shortcut]` with the chords it shows (one per display, duplicates merged, separated by
   * « , »; each chord as `<kbd>Ctrl</kbd> + <kbd>Z</kbd>`) or the text of its `keysLabel`, and a `dd` with what it does.
   * Package 0's skeleton: the content and the hooks, no style (package A).
   */
  import { t } from '../../lib/i18n.svelte';
  import { chordParts, type Shortcut } from '../../lib/shortcuts';
  import type { SideWhere } from '../../lib/side-panel.svelte';

  let { where, shortcuts }: { where: SideWhere; shortcuts: readonly Shortcut<object>[] } = $props();

  const main = $derived(shortcuts.filter((entry) => !entry.group));
  const groups = $derived(
    (['replay', 'presentation'] as const)
      .map((group) => ({ group, entries: shortcuts.filter((entry) => entry.group === group) }))
      .filter(({ entries }) => entries.length > 0),
  );

  /** The chords an entry shows, as parts: the keys not marked hidden, one per display (duplicates merged). */
  function chordsOf(entry: Shortcut<object>): string[][] {
    const seen = new Set<string>();
    const chords: string[][] = [];
    for (const key of entry.keys) {
      if (key.shown === false) continue;
      const parts = chordParts(key.chord);
      const display = parts.join('\u0000');
      if (seen.has(display)) continue;
      seen.add(display);
      chords.push(parts);
    }
    return chords;
  }
</script>

{#snippet list(entries: readonly Shortcut<object>[])}
  <dl class="keys">
    {#each entries as entry (entry.id)}
      <dt data-shortcut={entry.id}>
        {#if entry.keysLabel}
          {t(entry.keysLabel)}
        {:else}
          {#each chordsOf(entry) as parts, i (i)}
            {#if i > 0}{', '}{/if}
            {#each parts as part, j (j)}
              {#if j > 0}{' + '}{/if}<kbd>{part}</kbd>
            {/each}
          {/each}
        {/if}
      </dt>
      <dd>{t(entry.label)}</dd>
    {/each}
  </dl>
{/snippet}

<section class="pane-block" data-section="shortcuts" aria-labelledby="side-help-title-{where}">
  <h3 class="pane-title" id="side-help-title-{where}">{t(where === 'editor' ? 'side.help.editor' : 'side.help.live')}</h3>
  {@render list(main)}
  {#each groups as { group, entries } (group)}
    <h4>{t(`side.help.${group}`)}</h4>
    {@render list(entries)}
  {/each}
  <p class="pane-help">{t(where === 'editor' ? 'side.help.typingEditor' : 'side.help.typingLive')}</p>
</section>
