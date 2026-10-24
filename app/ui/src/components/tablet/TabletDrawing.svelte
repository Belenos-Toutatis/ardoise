<script lang="ts">
  /**
   * The tablet seen from above, for the registration wizard. `hold`: the button is held while a
   * ring fills over 6 seconds, then the light blinks blue; `press`: a short press, repeated;
   * `wait`: the light blinks while Ardoise searches and connects; `done`: registered.
   * Plain CSS animation; with `prefers-reduced-motion` it shows the end state without moving.
   */
  import { t } from '../../lib/i18n.svelte';

  let { mode }: { mode: 'hold' | 'press' | 'wait' | 'done' } = $props();
</script>

<svg class="tablet-drawing" data-mode={mode} viewBox="0 0 240 160" role="img" aria-label={t('tablet.register.drawingLabel')}>
  <!-- Body and drawing area. -->
  <rect class="body" x="10" y="8" width="220" height="132" rx="12" />
  <rect class="paper" x="26" y="20" width="188" height="108" rx="4" />
  <path class="scribble" d="M48 96 q18 -34 36 -4 t36 -2 t36 -18 t30 6" />

  <!-- Light. -->
  <circle class="led" cx="44" cy="150" r="4.5" />

  <!-- Button, its ring and the finger. -->
  <g class="button-group">
    <circle class="ring-track" cx="196" cy="150" r="12" />
    <circle class="ring" cx="196" cy="150" r="12" pathLength="100" />
    <circle class="button" cx="196" cy="150" r="7" />
    <circle class="finger" cx="196" cy="150" r="10" />
  </g>

  {#if mode === 'done'}
    <g class="tick" transform="translate(120 74)">
      <circle r="22" />
      <path d="M-10 0 -3 8 11 -8" />
    </g>
  {/if}
</svg>

<style>
  .tablet-drawing {
    display: block;
    width: min(100%, 22rem);
    height: auto;
    margin: 0.5rem auto;
  }

  .body {
    fill: var(--slate);
  }

  .paper {
    fill: var(--surface);
    stroke: var(--border);
  }

  .scribble {
    fill: none;
    stroke: var(--muted);
    stroke-width: 3;
    stroke-linecap: round;
    opacity: 0.6;
  }

  .led {
    fill: #3b82f6;
    opacity: 0.2;
  }

  .ring-track {
    fill: none;
    stroke: rgb(255 255 255 / 25%);
    stroke-width: 3;
  }

  .ring {
    fill: none;
    stroke: #3b82f6;
    stroke-width: 3;
    stroke-linecap: round;
    stroke-dasharray: 100;
    stroke-dashoffset: 100;
    transform: rotate(-90deg);
    transform-origin: 196px 150px;
  }

  .button {
    fill: #cfd6e0;
  }

  .finger {
    fill: rgb(255 214 170 / 85%);
    opacity: 0;
  }

  .tick circle {
    fill: #2fa866;
  }

  .tick path {
    fill: none;
    stroke: #ffffff;
    stroke-width: 5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  /* hold: the finger stays on the button, the ring fills in 6 s, then the light blinks. */
  [data-mode='hold'] .finger {
    opacity: 1;
  }

  [data-mode='hold'] .ring {
    animation: fill 6s linear forwards;
  }

  [data-mode='hold'] .led {
    animation: blink 1s ease-in-out 6s infinite;
  }

  /* wait: the tablet is in registration mode; Ardoise is talking to it. */
  [data-mode='wait'] .led {
    animation: blink 1s ease-in-out infinite;
  }

  /* press: a short press, again and again. */
  [data-mode='press'] .led {
    opacity: 1;
  }

  [data-mode='press'] .finger {
    animation: tap 1.6s ease-in-out infinite;
  }

  [data-mode='press'] .button {
    animation: sink 1.6s ease-in-out infinite;
    transform-origin: 196px 150px;
  }

  [data-mode='done'] .led {
    fill: #2fa866;
    opacity: 1;
  }

  @keyframes fill {
    to {
      stroke-dashoffset: 0;
    }
  }

  @keyframes blink {
    0%,
    100% {
      opacity: 0.2;
    }
    50% {
      opacity: 1;
    }
  }

  @keyframes tap {
    0%,
    60%,
    100% {
      opacity: 0;
      transform: translateY(-6px);
    }
    20%,
    35% {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @keyframes sink {
    0%,
    60%,
    100% {
      transform: scale(1);
    }
    20%,
    35% {
      transform: scale(0.8);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    [data-mode='hold'] .ring {
      animation: none;
      stroke-dashoffset: 0;
    }

    [data-mode='hold'] .led,
    [data-mode='wait'] .led {
      animation: none;
      opacity: 1;
    }

    [data-mode='press'] .finger {
      animation: none;
      opacity: 1;
    }

    [data-mode='press'] .button {
      animation: none;
      transform: scale(0.8);
    }
  }
</style>
