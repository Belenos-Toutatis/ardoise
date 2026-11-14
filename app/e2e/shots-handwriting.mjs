// A tiny single-stroke handwriting for the pictures of the README (`screenshots.mjs`).
//
// The demo notes hold scribbles, not words, and a picture of an editor full of scribbles shows nothing. So the picture script
// writes real words with the pen events of the DevTools protocol, from the strokes defined here: one polyline per pen stroke
// in a grid where the baseline is y = 0, the x-height 10, the ascenders 16 and the descenders -6 (y up). The polylines are
// smoothed (Catmull-Rom), slanted and slightly jittered with a seeded generator, so the same text always gives the same ink.
// Only the letters the pictures write are defined; an unknown character is skipped.

/** `w`: the advance of the glyph (without the space between two letters); `s`: its strokes. */
const GLYPHS = {
  a: { w: 7, s: [[[6, 8], [4, 10], [2, 9.5], [0.8, 7], [0.8, 3], [2.5, 0.5], [4.5, 0.3], [6, 2.5]], [[6, 10], [6, 0], [7, 0]]] },
  b: { w: 7, s: [[[1, 16], [1, 0]], [[1, 7], [3, 9.8], [5, 9.8], [6.8, 7], [6.8, 3], [5, 0.3], [3, 0.3], [1, 3]]] },
  c: { w: 6, s: [[[5.5, 8], [3.5, 10], [1.5, 9], [0.6, 6], [0.6, 4], [1.5, 1], [3.5, 0], [5.5, 2]]] },
  d: { w: 7, s: [[[6, 8], [4, 10], [2, 9.5], [0.8, 7], [0.8, 3], [2.5, 0.5], [4.5, 0.3], [6, 2.5]], [[6, 16], [6, 0], [7, 0]]] },
  e: { w: 6.5, s: [[[1, 5], [5.5, 5.5], [6, 7.5], [4.5, 9.8], [2.5, 9.8], [0.8, 7], [0.6, 3.5], [2, 0.5], [4, 0], [6, 1.8]]] },
  f: { w: 4.5, s: [[[5, 15], [3.5, 16], [2.2, 14.5], [2, 10], [2, 0]], [[0, 10], [4.5, 10]]] },
  g: { w: 7, s: [[[6, 8], [4, 10], [2, 9.5], [0.8, 7], [0.8, 3], [2.5, 0.5], [4.5, 0.3], [6, 2.5]], [[6, 10], [6, -3], [5, -5.5], [3, -6], [1, -5]]] },
  h: { w: 7, s: [[[1, 16], [1, 0]], [[1, 7], [3, 9.8], [5.5, 9.5], [6.5, 7], [6.5, 0]]] },
  i: { w: 2.5, s: [[[1, 10], [1, 0]], [[1, 13.2], [1.05, 13.7]]] },
  j: { w: 3, s: [[[2, 10], [2, -4], [1, -5.8], [-0.5, -5.5]], [[2, 13.2], [2.05, 13.7]]] },
  k: { w: 6.5, s: [[[1, 16], [1, 0]], [[6, 10], [1, 4.5]], [[2.5, 6], [6.5, 0]]] },
  l: { w: 2.5, s: [[[1, 16], [1, 0.5], [1.8, 0]]] },
  m: { w: 10, s: [[[1, 10], [1, 0]], [[1, 7], [2.5, 9.8], [4, 9], [4.5, 7], [4.5, 0]], [[4.5, 7], [6, 9.8], [8, 9.3], [8.8, 7], [8.8, 0]]] },
  n: { w: 7, s: [[[1, 10], [1, 0]], [[1, 7], [3, 9.8], [5.5, 9.4], [6.5, 7], [6.5, 0]]] },
  o: { w: 7, s: [[[3.5, 10], [1.5, 8.5], [0.7, 5], [1.5, 1.5], [3.5, 0], [5.5, 1.5], [6.3, 5], [5.5, 8.5], [3.5, 10]]] },
  p: { w: 7, s: [[[1, 10], [1, -6]], [[1, 7], [3, 9.8], [5, 9.8], [6.8, 7], [6.8, 3], [5, 0.3], [3, 0.3], [1, 3]]] },
  q: { w: 7, s: [[[6, 8], [4, 10], [2, 9.5], [0.8, 7], [0.8, 3], [2.5, 0.5], [4.5, 0.3], [6, 2.5]], [[6, 10], [6, -6]]] },
  r: { w: 5, s: [[[1, 10], [1, 0]], [[1, 6.5], [2.5, 9.3], [4.5, 9.8]]] },
  s: { w: 5.5, s: [[[5, 9], [3.5, 10], [1.5, 9.5], [1, 8], [2.5, 5.8], [4.5, 4.8], [5.2, 3], [4, 0.6], [2, 0], [0.5, 1.2]]] },
  t: { w: 5, s: [[[2, 14], [2, 1.5], [3, 0], [4.5, 0.8]], [[0, 10], [4.5, 10]]] },
  u: { w: 7, s: [[[1, 10], [1, 3], [2.5, 0.4], [4.5, 0.4], [6, 3]], [[6, 10], [6, 0], [7, 0]]] },
  v: { w: 6.5, s: [[[0.5, 10], [3.2, 0], [6, 10]]] },
  w: { w: 9.5, s: [[[0.5, 10], [2.5, 0], [4.7, 8], [7, 0], [9, 10]]] },
  x: { w: 6, s: [[[0.5, 10], [5.5, 0]], [[5.5, 10], [0.5, 0]]] },
  y: { w: 6.5, s: [[[0.5, 10], [3.2, 0.5]], [[6, 10], [3, -2], [2, -5.8], [0.5, -6]]] },
  z: { w: 6, s: [[[0.5, 10], [5.5, 10], [0.5, 0], [5.5, 0]]] },
  C: { w: 8, s: [[[7.5, 12.5], [5, 15], [2.5, 14], [1, 10], [1, 5], [2.5, 1], [5, 0], [7.5, 2.5]]] },
  E: { w: 6.5, s: [[[6, 15], [1, 15], [1, 0], [6, 0]], [[1, 7.5], [4.5, 7.5]]] },
  '0': { w: 6, s: [[[3, 12], [1, 10.5], [0.5, 6], [1, 1.5], [3, 0], [5, 1.5], [5.5, 6], [5, 10.5], [3, 12]]] },
  '1': { w: 4, s: [[[1, 9], [3, 12], [3, 0]]] },
  '2': { w: 6, s: [[[0.8, 9.5], [2.5, 11.8], [4.5, 11], [5, 8.5], [0.8, 0], [5.5, 0]]] },
  '=': { w: 6.5, s: [[[0.5, 7], [6, 7]], [[0.5, 3.5], [6, 3.5]]] },
  '/': { w: 4, s: [[[0.5, -1], [3.5, 13]]] },
  ':': { w: 2, s: [[[1, 8], [1.05, 8.4]], [[1, 1], [1.05, 1.4]]] },
  '.': { w: 2, s: [[[1, 0.2], [1.05, 0.6]]] },
  ',': { w: 2, s: [[[1.3, 1], [0.6, -2]]] },
};

/** Accents drawn above a base letter: `[base, strokes of the mark]` (the mark is placed over a letter of width `w`). */
const ACCENTS = {
  é: ['e', [[[2.4, 12], [4.4, 14.2]]]],
  è: ['e', [[[4.4, 12], [2.4, 14.2]]]],
  à: ['a', [[[4.6, 12], [2.6, 14.2]]]],
  É: ['E', [[[3, 17.5], [5, 19.7]]]],
};

const SPACE = 4.5;
const GAP = 1.9;

/** A seeded generator (mulberry32): the same text always gives the same ink. */
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Catmull-Rom through `points`, `per` samples between two of them. */
function smooth(points, per = 4) {
  if (points.length < 3) {
    const [a, b] = points;
    return Array.from({ length: 5 }, (_, i) => [a[0] + ((b[0] - a[0]) * i) / 4, a[1] + ((b[1] - a[1]) * i) / 4]);
  }
  const out = [];
  for (let i = 0; i < points.length - 1; i++) {
    const p0 = points[Math.max(0, i - 1)];
    const p1 = points[i];
    const p2 = points[i + 1];
    const p3 = points[Math.min(points.length - 1, i + 2)];
    for (let k = 0; k < per; k++) {
      const t = k / per;
      const t2 = t * t;
      const t3 = t2 * t;
      out.push([0, 1].map((c) => 0.5 * (2 * p1[c] + (-p0[c] + p2[c]) * t + (2 * p0[c] - 5 * p1[c] + 4 * p2[c] - p3[c]) * t2 + (-p0[c] + 3 * p1[c] - 3 * p2[c] + p3[c]) * t3)));
    }
  }
  out.push(points.at(-1));
  return out;
}

/** The width of `text` in glyph units (without the final gap). */
export function widthOf(text) {
  let width = 0;
  for (const ch of text) {
    if (ch === ' ') width += SPACE;
    else {
      const glyph = GLYPHS[ACCENTS[ch]?.[0] ?? ch];
      if (glyph) width += glyph.w + GAP;
    }
  }
  return Math.max(0, width - GAP);
}

/**
 * The pen strokes that write `text` with its baseline starting at page point (`x`, `y`): arrays of `{ x, y }` in the
 * page's units (y down), one glyph unit being `size` page units. `slant`: how much the letters lean to the right.
 */
export function writeText(text, { x, y, size, slant = 0.14, seed = 7 }) {
  const next = random(seed);
  const strokes = [];
  let cursor = 0;
  for (const ch of text) {
    if (ch === ' ') {
      cursor += SPACE;
      continue;
    }
    const accent = ACCENTS[ch];
    const glyph = GLYPHS[accent?.[0] ?? ch];
    if (!glyph) continue;
    const rise = (next() - 0.5) * 0.7;
    const scale = 0.96 + next() * 0.08;
    const place = ([gx, gy]) => ({
      x: x + (cursor + gx * scale + gy * scale * slant + (next() - 0.5) * 0.12) * size,
      y: y - (gy * scale + rise + (next() - 0.5) * 0.12) * size,
    });
    for (const stroke of [...glyph.s, ...(accent?.[1] ?? [])]) strokes.push(smooth(stroke).map(place));
    cursor += glyph.w + GAP;
  }
  return strokes;
}

/** A straight line from (`x0`, `y0`) to (`x1`, `y1`) in `count` points (an underline, an arrow's shaft). */
export function lineTo(x0, y0, x1, y1, count = 24) {
  return Array.from({ length: count + 1 }, (_, i) => ({ x: x0 + ((x1 - x0) * i) / count, y: y0 + ((y1 - y0) * i) / count }));
}
