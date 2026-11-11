<script lang="ts">
  /** The small icons of the layers zone (the validated mockup's): eye, eye barred, pencil, tablet. Decorative: the button around them has the name. */
  let { name }: { name: 'eye' | 'eyeOff' | 'pencil' | 'tablet' } = $props();
</script>

<svg
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
  data-icon={name}
>
  {#if name === 'eye'}
    <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === 'eyeOff'}
    <path
      d="M10.6 5.1A10.6 10.6 0 0 1 12 5c6.4 0 10 7 10 7a17.6 17.6 0 0 1-2.6 3.6M6.6 6.6C3.8 8.4 2 12 2 12s3.6 7 10 7a9.7 9.7 0 0 0 5.4-1.6"
    />
    <path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" />
    <path d="M3 3l18 18" />
  {:else if name === 'pencil'}
    <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4z" />
    <path d="M14.5 5.5l3 3" />
  {:else}
    <rect x="2.5" y="6" width="13" height="15.5" rx="1.8" />
    <path d="M5.5 10h7M5.5 13.5h5" />
    <path d="M21.2 2.8l.9.9-6.6 6.6-1.9.7.7-1.9z" />
  {/if}
</svg>
