/**
 * The equation tool's conversion (`lib/equation.ts`, `lib/equation-path.ts`, `lib/mathjax.ts`): the stored grammar is
 * Rust's, the walk of MathJax's output keeps geometry only, and the real MathJax turns a teacher's LaTeX into paths.
 */
import { describe, expect, it } from 'vitest';
import { convertLatex, drawingOfSvg, parseSvgPath, parseTransform, type SvgNode } from './equation';
import {
  EQUATION_MAX_PATH_BYTES,
  EQUATION_MAX_SOURCE_CHARS,
  equationBox,
  readEquationPath,
  stepsBounds,
  tenth,
  writeEquationPath,
} from './equation-path';
import { dynamicFileName } from './mathjax';

const el = (tag: string, attrs: Record<string, string> = {}, children: SvgNode[] = [], text = ''): SvgNode => ({
  tag,
  attrs,
  children,
  text,
});

/** MathJax's root: the formula flipped (its font has y up). */
const root = (...children: SvgNode[]) => el('svg', { viewBox: '0 -1000 2000 1500' }, [el('g', { transform: 'scale(1,-1)' }, children)]);

describe('stored path data (Rust’s grammar)', () => {
  it('reads what Rust reads and refuses what it refuses', () => {
    expect(readEquationPath('M1 -2.5L2.5 3.25Q4 5 6 7C1 2 3 4 5 6ZM10 10L11 10Z')).toHaveLength(8);
    expect(readEquationPath(' M 1,2 3 4 L5-6 .5+1 Q1 2 3 4 5 6 7 8\nC1 2 3 4 5 6 Z ')).toHaveLength(8);
    expect(readEquationPath('')).toEqual([]);
    for (const bad of [
      'L0 0',
      'M0',
      'M0 0L1',
      'M0 0 Z L1 1',
      'M0 0ZZ',
      'm0 0',
      'M0 0h5',
      'M0 0A1 1 0 0 1 2 2',
      'M1e3 0',
      'M- 1',
      'M. 1',
      'M0 0 #',
      '<script>',
      'M0 2000000',
      // Just beyond the limit (Rust compares before rounding to f32, as here).
      'M1000000.02 0',
      'M0 0 é',
    ]) {
      expect(readEquationPath(bad), bad).toBeNull();
    }
    expect(readEquationPath(`M0 0${'L1 1'.repeat(EQUATION_MAX_PATH_BYTES / 4)}`)).toBeNull();
  });

  it('writes tenths and measures the box from the origin', () => {
    expect([0.04, -0.04, 12.25, -3.5, 1234567.89, 7].map(tenth)).toEqual(['0', '0', '12.3', '-3.5', '1234567.9', '7']);
    const steps = readEquationPath('M0 0L100 0Q150 50 100 100Z')!;
    expect(writeEquationPath(steps)).toBe('M0 0L100 0Q150 50 100 100Z');
    expect(equationBox(steps)).toEqual({ width: 150, height: 100 });
    expect(equationBox([])).toEqual({ width: 0, height: 0 });
  });
});

describe('the walk of MathJax’s output', () => {
  it('reads transform lists and SVG path data', () => {
    expect(parseTransform('translate(10,5) scale(2)')).toEqual([2, 0, 0, 2, 10, 5]);
    expect(parseTransform('translate(2431,0) translate(0 250)')).toEqual([1, 0, 0, 1, 2431, 250]);
    const turned = parseTransform('rotate(90 10 0)');
    expect(turned.map((v) => Math.round(v * 1e6) / 1e6)).toEqual([0, 1, -1, 0, 10, -10]);
    expect(() => parseTransform('perspective(3)')).toThrow();
    // Relative, implicit, H/V, S/T: all made absolute.
    const steps = parseSvgPath('m10 10l5 0h5v5s5 5 10 0t10 0zl1 1');
    expect(steps.map((s) => s.op).join('')).toBe('MLLLCQZML');
    expect(steps[6]).toEqual({ op: 'Z' });
    expect(steps[7]).toEqual({ op: 'M', x: 10, y: 10 });
    expect(steps[8]).toEqual({ op: 'L', x: 11, y: 11 });
    expect(() => parseSvgPath('M0 0A5 5 0 0 1 10 10')).toThrow();
  });

  it('keeps glyphs, rules and frames, flipped, moved to the origin and rounded', () => {
    const result = drawingOfSvg(
      root(
        el('g', { 'data-mml-node': 'mi', transform: 'translate(100,0)' }, [el('path', { d: 'M0 0L50 0L50 100Z', 'data-c': '78' })]),
        // A fraction bar, and a frame (`\boxed`): a ring of the stroke's width.
        el('rect', { x: '0', y: '220', width: '300', height: '60' }),
        el('rect', { x: '0', y: '-200', width: '400', height: '200', fill: 'none', 'stroke-width': '20' }),
      ),
    );
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const steps = readEquationPath(result.drawing.path)!;
    const box = stepsBounds(steps)!;
    expect([box.minX, box.minY]).toEqual([0, 0]);
    // The frame reaches 10 beyond its rectangle on each side (420 wide); the bar is 80 below the frame's bottom
    // (its 220 + 60 flipped, against the frame's 200 + 10): 490 in all.
    expect([result.drawing.width, result.drawing.height]).toEqual([420, 490]);
    expect(steps.filter((s) => s.op === 'M')).toHaveLength(4);
  });

  it('draws table lines with their dashes, and clips a delimiter’s extender to its viewport', () => {
    const lines = drawingOfSvg(
      root(
        el('line', { 'data-line': 'h', class: 'mjx-solid', x1: '0', y1: '250', x2: '1000', y2: '250' }),
        el('line', { 'data-line': 'h', class: 'mjx-dashed', x1: '0', y1: '0', x2: '1000', y2: '0' }),
      ),
    );
    expect(lines.ok && readEquationPath(lines.drawing.path)!.filter((s) => s.op === 'M').length).toBe(1 + 4);
    expect(lines.ok && lines.drawing.height).toBe(250 + 70);
    // The extender is scaled to 1942 high and clipped to the 1295 of its viewport.
    const extender = drawingOfSvg(
      root(
        el('svg', { width: '902', height: '1295', y: '1000', x: '0', viewBox: '0 323.8 902 1295' }, [
          el('path', { d: 'M397 0L505 0L505 748L397 748Z', transform: 'scale(1,2.597)' }),
        ]),
      ),
    );
    expect(extender.ok).toBe(true);
    if (!extender.ok) return;
    expect(extender.drawing.width).toBeCloseTo(108, 0);
    expect(extender.drawing.height).toBeCloseTo(1295, 0);
  });

  it('refuses what it cannot draw and names a character without a glyph', () => {
    expect(drawingOfSvg(root(el('text', { 'data-variant': 'normal' }, [], '漢')))).toEqual({
      ok: false,
      problem: { kind: 'missingGlyph', chars: '漢' },
    });
    expect(drawingOfSvg(root(el('foreignObject')))).toEqual({ ok: false, problem: { kind: 'unsupported', what: 'foreignObject' } });
    expect(drawingOfSvg(root(el('use', { href: '#g' })))).toEqual({ ok: false, problem: { kind: 'unsupported', what: 'use' } });
    expect(drawingOfSvg(root(el('path', { d: 'M0 0L5 5', fill: 'none' })))).toMatchObject({ ok: false, problem: { kind: 'unsupported' } });
    expect(drawingOfSvg(root(el('g', { 'data-mml-node': 'mphantom' })))).toEqual({ ok: false, problem: { kind: 'nothingDrawn' } });
  });
});

describe('LaTeX to drawing, with the real MathJax', () => {
  /** The drawing of `latex`, which must succeed. */
  async function draw(latex: string) {
    const result = await convertLatex(latex);
    if (!result.ok) throw new Error(`${latex}: ${JSON.stringify(result.problem)}`);
    const steps = readEquationPath(result.drawing.path);
    expect(steps, latex).not.toBeNull();
    expect(equationBox(steps!)).toEqual({ width: result.drawing.width, height: result.drawing.height });
    return result.drawing;
  }

  it('draws a teacher’s physics and chemistry as paths', async () => {
    const half = await draw('\\frac{1}{2}');
    // About 0.6 em wide and 2 em high (units of 1/1000 em).
    expect(half.width).toBeGreaterThan(300);
    expect(half.width).toBeLessThan(1500);
    expect(half.height).toBeGreaterThan(1000);
    const water = await draw('\\ce{2H2 + O2 -> 2H2O}');
    expect(water.width).toBeGreaterThan(5000);
    expect(water.path.length).toBeLessThan(40_000);
    for (const latex of [
      'E_c = \\dfrac{1}{2} m v^2',
      '\\vec{F} = m\\vec{a}',
      '\\rho = \\dfrac{m}{V} = 1{,}0\\ \\text{g}\\cdot\\text{cm}^{-3}',
      '\\ce{Cu^2+ + 2e- <=> Cu}',
      '\\ce{CO2 ^} \\quad \\pu{9.81 m/s^2}',
      '\\alpha + \\beta = \\Delta\\Omega',
      '\\sqrt[3]{x} \\leq \\infty',
      '\\begin{cases} x = 1 \\\\ y = 2 \\end{cases}',
      '\\left( \\dfrac{a}{b} \\right)^2',
      '\\boxed{E = mc^2} \\quad \\cancel{x}',
      '20\\,\\celsius',
    ]) {
      await draw(latex);
    }
  });

  it('draws French accents in \\text from the font shipped with the app', async () => {
    const accents = await draw('\\text{La vitesse est égale à } \\dfrac{d}{t}');
    const plain = await draw('\\text{La vitesse est egale a } \\dfrac{d}{t}');
    // Same width give or take the accents, and the accents add outlines.
    expect(Math.abs(accents.width - plain.width)).toBeLessThan(200);
    expect(accents.path).not.toBe(plain.path);
  });

  it('shows MathJax’s error for invalid LaTeX and refuses what cannot be placed', async () => {
    expect(await convertLatex('\\frac{1')).toEqual({ ok: false, problem: { kind: 'latex', message: 'Missing close brace' } });
    expect(await convertLatex('\\color{red}{x}')).toMatchObject({ ok: false, problem: { kind: 'latex' } });
    expect(await convertLatex('\\def\\a{\\a}\\a')).toMatchObject({ ok: false, problem: { kind: 'latex' } });
    expect(await convertLatex('   ')).toEqual({ ok: false, problem: { kind: 'empty' } });
    expect(await convertLatex('x'.repeat(EQUATION_MAX_SOURCE_CHARS + 1))).toEqual({
      ok: false,
      problem: { kind: 'tooLong', max: EQUATION_MAX_SOURCE_CHARS },
    });
    expect(await convertLatex('\\text{漢}')).toEqual({ ok: false, problem: { kind: 'missingGlyph', chars: '漢' } });
    expect(await convertLatex('\\,')).toEqual({ ok: false, problem: { kind: 'nothingDrawn' } });
    // A numbered equation is laid out against a page width: refused, with its own reason.
    expect(await convertLatex('x = 1 \\tag{1}')).toEqual({ ok: false, problem: { kind: 'unsupported', what: 'tag' } });
    // A converter that throws (MathJax could not load): said, not thrown.
    expect(await convertLatex('x', () => Promise.reject(new Error('boom')))).toEqual({ ok: false, problem: { kind: 'latex', message: 'boom' } });
  });

  it('forgets what an equation defines before the next one is drawn', async () => {
    await draw('\\newcommand{\\R}{\\mathbb{R}} x \\in \\R');
    expect(await convertLatex('x \\in \\R')).toEqual({ ok: false, problem: { kind: 'latex', message: 'Undefined control sequence \\R' } });
    await draw('\\renewcommand{\\frac}[2]{#2} \\frac{1}{2}');
    const half = await draw('\\frac{1}{2}');
    expect(half.height).toBeGreaterThan(1000);
    // A label typed twice (each keystroke draws the preview again) is not « multiply defined ».
    await draw('\\label{a} x');
    await draw('\\label{a} x');
  });

  it('asks for the parts of the font by their file names', () => {
    expect(dynamicFileName('@mathjax/mathjax-newcm-font/js/svg/dynamic/latin-b.js')).toBe('latin-b');
  });
});
