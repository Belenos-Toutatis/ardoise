<script lang="ts">
  /**
   * Stands in for `VideoExportDialog` in the editor's tests (`Editor.test.ts`), so that they check
   * what the editor gives the dialog whatever the dialog looks like. Never part of the app.
   */
  import type { ExportSource } from '../../lib/note';

  let {
    source,
    subject,
    onclose,
    onrunning,
  }: { source: ExportSource; subject: string; onclose: () => void; onrunning?: (running: boolean) => void } = $props();
</script>

<div class="video-dialog-double" data-path={source.path} data-subject={subject} data-strokes={source.note?.strokes.length ?? -1}>
  <button type="button" onclick={onclose}>close-video</button>
  <button type="button" onclick={() => onrunning?.(true)}>run-video</button>
  <button type="button" onclick={() => onrunning?.(false)}>end-video</button>
</div>
