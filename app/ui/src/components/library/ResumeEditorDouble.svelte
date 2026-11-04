<script lang="ts">
  /**
   * Stands in for `Editor` in `ResumeLive.test.ts`: shows the note it was given and lets the test
   * press « Reprendre en mode direct » and the back button. Never part of the app.
   */
  let { path, onclose, onresume }: { path: string; onclose: () => void; onresume?: (path: string) => void } = $props();
</script>

<div data-double="editor" data-path={path} data-can-resume={onresume ? 'yes' : 'no'}>
  <button type="button" data-double-close="editor" onclick={onclose}>back</button>
  <button type="button" data-double-resume="editor" onclick={() => onresume?.(path)}>resume</button>
</div>
