// @vitest-environment jsdom
/// <reference types="node" />
// How the editor's canvas draws the tools, as the exports and the live view do (audit R3-2): the felt tip and the chalk
// a little transparent, the highlighter screened (not multiplied) over a dark paper.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { DEFAULT_TOOLS, docFromNote } from '../../lib/editor/state';
import { hline, note } from '../../lib/editor/test-fixtures';
import { instantiateInk, type Ink } from '../../lib/ink';
import type { Paper } from '../../lib/note';
import Canvas from './Canvas.svelte';

let ink: Ink;
beforeAll(async () => {
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
});

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
});

async function render(tool: 'feltTip' | 'chalk' | 'highlighter' | 'ballpoint', paper: Paper = 'white') {
  const stroke = { ...hline(1000, 5000, 3000), tool };
  const base = note([stroke]);
  const doc = docFromNote({ ...base, background: { ...base.background, paper } });
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Canvas, { target, props: { doc, tools: { ...DEFAULT_TOOLS }, ink, readonly: false, onchange: vi.fn(), onselect: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  return { svg, path: svg.querySelector<SVGPathElement>('path[data-stroke]')! };
}

describe('the canvas draws the tools like the exports', () => {
  it('makes the felt tip and the chalk a little transparent, the other tools opaque', async () => {
    expect((await render('feltTip')).path.getAttribute('fill-opacity')).toBe('0.9');
    cleanup?.();
    expect((await render('chalk')).path.getAttribute('fill-opacity')).toBe('0.95');
    cleanup?.();
    expect((await render('ballpoint')).path.getAttribute('fill-opacity')).toBe('1');
  });

  it('marks the page as dark for a slate or a dark custom paper, not for white or kraft', async () => {
    expect((await render('highlighter', 'slate')).svg.classList.contains('dark-paper')).toBe(true);
    cleanup?.();
    expect((await render('highlighter', { custom: { r: 10, g: 10, b: 60, a: 255 } })).svg.classList.contains('dark-paper')).toBe(true);
    cleanup?.();
    const white = await render('highlighter', 'white');
    expect(white.svg.classList.contains('dark-paper')).toBe(false);
    expect(white.path.classList.contains('highlighter')).toBe(true);
    cleanup?.();
    expect((await render('highlighter', 'kraft')).svg.classList.contains('dark-paper')).toBe(false);
  });
});
