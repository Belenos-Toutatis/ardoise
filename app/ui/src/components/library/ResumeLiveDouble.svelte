<script lang="ts">
  /**
   * Stands in for `LiveView` in `ResumeLive.test.ts`: shows the note it was asked to resume.
   * Never part of the app.
   */
  let { resume = null, onclose }: { resume?: string | null; onclose: (notice?: string) => void } = $props();
</script>

<div data-double="live" data-resume={resume ?? ''}>
  <button type="button" data-double-close="live" onclick={() => onclose()}>close</button>
  <button type="button" data-double-close="live-notice" onclick={() => onclose('Narration lost')}>close with a notice</button>
</div>
