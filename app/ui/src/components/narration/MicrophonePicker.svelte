<script lang="ts">
  /**
   * Microphone list, level meter and capture status (with the « Windows bloque le micro »
   * explanation). Contract stub: filled in by the editor replay and narration work package.
   */
  let {
    device,
    onchange,
    monitor = false,
  }: {
    /** Chosen microphone id; null for the system's default. */
    device: string | null;
    onchange: (device: string | null) => void;
    /** Show a live level meter (starts a level-only capture while mounted). */
    monitor?: boolean;
  } = $props();
</script>
