<script lang="ts">
  /**
   * Live mode and the classroom's control window (SPEC §4.2). Placeholder of the contracts
   * commit; work package D (docs/design/tablet-live-classroom.md).
   */
  import type { AppState } from '../../lib/api';
  import { t } from '../../lib/i18n.svelte';

  let { onclose }: { app: AppState; onclose: () => void } = $props();
</script>

<section class="live-view" data-state="idle">
  <h2>{t('live.title')}</h2>
  <button type="button" class="button" data-action="stop-live" onclick={onclose}>{t('common.close')}</button>
</section>
