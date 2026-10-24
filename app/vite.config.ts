/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri serves the built files from `dist/` and expects the dev server on a fixed port.
export default defineConfig(({ mode }) => ({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true, target: 'es2022' },
  // Component tests mount Svelte components, which needs the browser build of Svelte.
  resolve: mode === 'test' ? { conditions: ['browser'] } : undefined,
  test: { include: ['ui/src/**/*.test.ts'], environment: 'node' },
}));
