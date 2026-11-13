/// <reference types="node" />
/**
 * The text frames' glyphs and layout from the WebAssembly module (`lib/text.ts`, `crates/ardoise-ink-wasm/src/text.rs`):
 * the same rules as the exports (`ardoise-text`), and what the editor's text field needs to follow them.
 */
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { instantiateInk, type Ink } from './ink';
import { LINE_HEIGHT, TEXT_SIZES, offsetAt, parsePath, textFieldStyle, tracePath, type TextStyle } from './text';

let ink: Ink;
const LM: TextStyle = { font: 'latinModern', bold: false, size: 1000 };

beforeAll(async () => {
  ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
});

describe('layout', () => {
  it('breaks lines only at line breaks and places glyphs by their advances', () => {
    const laid = ink.text.layout('Bonjour\nle monde', LM);
    expect(laid.lines.length).toBe(2);
    expect(laid.lineHeight).toBe(1000 * LINE_HEIGHT);
    expect(laid.height).toBe(2 * 1000 * LINE_HEIGHT);
    expect(laid.lines.map((l) => [l.start, l.end])).toEqual([
      [0, 7],
      [8, 16],
    ]);
    const glyphs = laid.lines[0].glyphs;
    for (let i = 1; i < glyphs.length; i++) expect(glyphs[i].x).toBeCloseTo(glyphs[i - 1].x + glyphs[i - 1].advance, 3);
    expect(laid.width).toBeCloseTo(Math.max(...laid.lines.map((l) => l.width)), 3);
    // A long line never wraps.
    expect(ink.text.layout('x'.repeat(500), LM).lines.length).toBe(1);
  });

  it('differs by family and weight, and scales with the size', () => {
    const width = (style: TextStyle) => ink.text.layout('Les élèves', style).width;
    expect(width({ ...LM, bold: true })).toBeGreaterThan(width(LM));
    expect(Math.abs(width({ ...LM, font: 'andika' }) - width(LM))).toBeGreaterThan(1);
    expect(width({ ...LM, size: 2000 })).toBeCloseTo(2 * width(LM), 1);
  });

  it('counts UTF-16 units, as the text field does', () => {
    const laid = ink.text.layout('a\u{1D400}b', LM);
    expect(laid.lines[0].glyphs.map((g) => g.index)).toEqual([0, 1, 3]);
  });

  it('finds the caret under a point', () => {
    const laid = ink.text.layout('ab\ncd', LM);
    const a = laid.lines[0].glyphs[0];
    expect(offsetAt(laid, a.advance * 0.4, 10)).toBe(0);
    expect(offsetAt(laid, a.advance * 0.6, 10)).toBe(1);
    expect(offsetAt(laid, 1e6, 10)).toBe(2);
    expect(offsetAt(laid, 0, 1300)).toBe(3);
    expect(offsetAt(laid, 1e6, 1e6)).toBe(5);
  });
});

describe('glyphs', () => {
  it('are closed paths with curves within the lines, none for an empty text', () => {
    for (const font of ['latinModern', 'andika'] as const) {
      const d = ink.text.path('Élève « œ » 12 €', { font, bold: false, size: 600 });
      expect(d.startsWith('M')).toBe(true);
      expect(d).toMatch(/[QC]/);
      expect(d.endsWith('Z')).toBe(true);
    }
    expect(ink.text.path('', LM)).toBe('');
    expect(ink.text.path('   ', LM)).toBe('');
  });

  it('parse back into steps that a 2D context traces, curves included', () => {
    const d = ink.text.path('o', { font: 'andika', bold: false, size: 600 });
    const steps = parsePath(d);
    expect(steps.length).toBeGreaterThan(4);
    expect(steps[0].op).toBe('M');
    const calls: string[] = [];
    tracePath(
      {
        moveTo: () => calls.push('M'),
        lineTo: () => calls.push('L'),
        closePath: () => calls.push('Z'),
        quadraticCurveTo: () => calls.push('Q'),
      },
      steps,
    );
    expect(calls.join('')).toBe(steps.map((s) => s.op).join(''));
    // A context without curves gets straight lines.
    const plain: string[] = [];
    tracePath({ moveTo: () => plain.push('M'), lineTo: () => plain.push('L'), closePath: () => plain.push('Z') }, steps);
    expect(plain.length).toBe(steps.length);
    expect(parsePath('M1 2L3.5 -4Q1 2 3 4C1 2 3 4 5 6Z')).toEqual([
      { op: 'M', x: 1, y: 2 },
      { op: 'L', x: 3.5, y: -4 },
      { op: 'Q', cx: 1, cy: 2, x: 3, y: 4 },
      { op: 'C', c1x: 1, c1y: 2, c2x: 3, c2y: 4, x: 5, y: 6 },
      { op: 'Z' },
    ]);
  });
});

describe('fonts', () => {
  it('are the four files shipped with the app', () => {
    const magic = (bytes: Uint8Array) => Array.from(bytes.slice(0, 4)).map((b) => b.toString(16).padStart(2, '0')).join('');
    // OpenType with CFF outlines ("OTTO"), TrueType (0x00010000).
    expect(magic(ink.text.fontBytes('latinModern', false))).toBe('4f54544f');
    expect(magic(ink.text.fontBytes('latinModern', true))).toBe('4f54544f');
    expect(magic(ink.text.fontBytes('andika', false))).toBe('00010000');
    expect(ink.text.fontBytes('andika', true).length).toBe(748_748);
    expect(ink.text.fontBytes('latinModern', false).length).toBe(111_536);
  });

  it('style the text field like the layout: no wrapping, kerning nor ligature, the same line height', () => {
    const css = textFieldStyle({ font: 'andika', bold: true, size: TEXT_SIZES.medium }, 0.1);
    expect(css).toContain('font-family: "Ardoise Andika", "Ardoise Latin Modern"');
    expect(css).toContain('font-weight: 700');
    expect(css).toContain('font-size: 60px');
    expect(css).toContain('line-height: 75px');
    expect(css).toContain('white-space: pre');
    expect(css).toContain('font-kerning: none');
    expect(css).toContain('font-variant-ligatures: none');
    expect(css).toContain('tab-size: 4');
  });
});
