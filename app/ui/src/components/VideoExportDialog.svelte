<script lang="ts">
  /**
   * Exports the replay of a note to MP4, GIF, WebP or WebM. Contract stub: filled in by the video
   * export work package.
   */
  import type { ExportSource } from '../lib/note';

  let {
    source,
    subject,
    onclose,
  }: {
    /** The note (with the editor's unsaved strokes) and its file, which holds the narration. */
    source: ExportSource;
    /** The note's title, as a sentence part. */
    subject: string;
    onclose: () => void;
  } = $props();
</script>
