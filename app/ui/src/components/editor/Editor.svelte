<script lang="ts">
  /**
   * The note editor (milestone M3). Contract used by the library screen: it opens the note at
   * `path`, calls `onsaved` with the note's path after each save (it may change when the
   * title changes), and `onclose` when the user goes back to the library.
   * Placeholder until the editor lands.
   */
  import type { AppState } from '../../lib/api';
  import { t } from '../../lib/i18n.svelte';

  let {
    path,
    app,
    onclose,
    onsaved,
  }: { path: string; app: AppState; onclose: () => void; onsaved?: (path: string) => void } = $props();



</script>

<section class="editor" data-path={path} data-library={app.library} data-saves={onsaved ? "yes" : "no"}>
  <button class="button" onclick={onclose}>{t('common.close')}</button>
</section>
