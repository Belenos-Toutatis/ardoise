<script lang="ts">
  /**
   * Root of the editor's projection window (`index.html?view=projection`, full screen on the projector; milestone M17,
   * docs/design/paper-projection-narration.md §2.5): only the editor's page, drawn as live's presentation window draws
   * its own (`LiveState` and `LiveCanvas`), no interface, Esc closes it.
   *
   * Package 0: a black page; package D builds it.
   */
  import { t } from '../../lib/i18n.svelte';
</script>

<main class="projection" aria-label={t('presentation.editorTitle')}></main>

<style>
  .projection {
    position: fixed;
    inset: 0;
    overflow: hidden;
    background: #000;
  }
</style>
