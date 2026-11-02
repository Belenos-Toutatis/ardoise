/// <reference types="node" />
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// The viewer runs in any browser on the local network (docs/design/lan-share.md): nothing of the
// desktop app may end up in its page. This walks the static imports from its entry.

const SRC = resolve(process.cwd(), 'ui/src');
const ENTRY = resolve(SRC, 'viewer/main.ts');

const IMPORT_FROM = /^\s*(?:import|export)\s[^;]*?\sfrom\s+['"]([^'"]+)['"]/gm;
const IMPORT_SIDE_EFFECT = /^\s*import\s+['"]([^'"]+)['"]/gm;
const IMPORT_DYNAMIC = /\bimport\(\s*['"]([^'"]+)['"]\s*\)/g;

function specifiers(source: string): string[] {
  const out: string[] = [];
  for (const re of [IMPORT_FROM, IMPORT_SIDE_EFFECT, IMPORT_DYNAMIC]) {
    for (const match of source.matchAll(re)) out.push(match[1]);
  }
  return out;
}

/** The file a relative specifier names (`?url` and the like removed), or null for a non-code asset. */
function resolveLocal(from: string, specifier: string): string | null {
  const base = resolve(dirname(from), specifier.replace(/\?.*$/, ''));
  if (/\.(json|css|wasm|svg|png)$/.test(base)) return null;
  for (const candidate of [base, `${base}.ts`, `${base}.svelte.ts`, `${base}.svelte`, resolve(base, 'index.ts')]) {
    if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
  }
  throw new Error(`${relative(SRC, from)} imports '${specifier}', which was not found`);
}

function walk(entry: string): { files: string[]; bare: string[] } {
  const files = new Set<string>();
  const bare = new Set<string>();
  const todo = [entry];
  while (todo.length > 0) {
    const file = todo.pop()!;
    if (files.has(file)) continue;
    files.add(file);
    for (const specifier of specifiers(readFileSync(file, 'utf8'))) {
      if (specifier.startsWith('.')) {
        const target = resolveLocal(file, specifier);
        if (target) todo.push(target);
      } else {
        bare.add(specifier);
      }
    }
  }
  return { files: [...files].map((f) => relative(SRC, f).replaceAll('\\', '/')).sort(), bare: [...bare].sort() };
}

describe('the viewer page', () => {
  const { files, bare } = walk(ENTRY);

  it('walks the real graph (the check is not empty)', () => {
    for (const expected of [
      'viewer/main.ts',
      'viewer/Viewer.svelte',
      'viewer/ViewerCanvas.svelte',
      'viewer/CodeForm.svelte',
      'lib/viewer-state.svelte.ts',
      'lib/viewer-stream.ts',
      'lib/live-state.svelte.ts',
      'lib/live-render.ts',
      'lib/ink.ts',
      'lib/i18n.svelte.ts',
    ]) {
      expect(files, expected).toContain(expected);
    }
  });

  it('imports nothing of Tauri', () => {
    expect(bare.filter((s) => s.startsWith('@tauri-apps/'))).toEqual([]);
    expect(bare.filter((s) => s !== 'svelte' && !s.startsWith('svelte/'))).toEqual([]);
  });

  it('never reaches lib/api.ts, the browser stand-ins or the app', () => {
    expect(files.filter((f) => f === 'lib/api.ts' || /(^|\/)mock[^/]*\.ts$/.test(f))).toEqual([]);
    expect(files.filter((f) => f.startsWith('components/') || f === 'App.svelte' || f === 'main.ts')).toEqual([]);
  });
});
