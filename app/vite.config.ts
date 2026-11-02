/// <reference types="vitest/config" />
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const page = (file: string) => fileURLToPath(new URL(file, import.meta.url));

// Tauri serves the built files from `dist/` and expects the dev server on a fixed port.
export default defineConfig(({ mode }) => ({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // Two pages: the app (`index.html`) and the viewer that browsers on the local network open
  // (`viewer.html`, docs/design/lan-share.md). The manifest tells the app which files the viewer
  // needs (its script, stylesheet, shared chunks, the WebAssembly module): `dist/.vite/manifest.json`.
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    manifest: true,
    rolldownOptions: { input: { index: page('index.html'), viewer: page('viewer.html') } },
  },
  // Component tests mount Svelte components, which needs the browser build of Svelte.
  resolve: mode === 'test' ? { conditions: ['browser'] } : undefined,
  // Half the processors: with one worker per core (the default), workers starting all at once
  // on a busy PC timed out ("Timeout waiting for worker to respond") during `cargo xtask check`.
  test: { include: ['ui/src/**/*.test.ts'], environment: 'node', maxWorkers: '50%' },
}));
