/**
 * MathJax for the equation tool (`docs/design/equations.md`), loaded only by `loadMathJax` (`lib/equation.ts`): a chunk
 * of its own, fetched from the app's files the first time the tool converts, never from the network. MathJax 4 from
 * its sources (`@mathjax/src`, Apache 2.0): TeX input, SVG output with glyphs as paths (`fontCache: 'none'`, no
 * `<use>` indirection), its New Computer Modern font (`@mathjax/mathjax-newcm-font`) and the glyphs of mhchem's arrows
 * (`@mathjax/mathjax-mhchem-font-extension`). The DOM is MathJax's light one (`liteAdaptor`): its output never touches
 * the page; `lib/equation.ts` reads geometry out of the copy this module hands over.
 *
 * TeX packages: `base` and `ams` (fractions, `\dfrac`, matrices, `cases`, `aligned`, `\boxed`, `\xrightarrow`),
 * `newcommand`, `mhchem` (`\ce`, `\pu`), `textmacros` (accents and text-mode commands in `\text{…}`), `gensymb`
 * (`\degree`, `\celsius`, `\ohm`, `\micro`), `cancel`, `boldsymbol`, `upgreek`. Not loaded on purpose: `color` (an
 * equation takes the tool's colour), `html` and `unicode` (links, classes, arbitrary characters), `require` (no other
 * package can be fetched), `noerrors` and `noundefined` (an error is shown, not drawn).
 *
 * The font's characters beyond its base (accented Latin, more arrows and symbols, double-struck, calligraphic…) are in
 * files MathJax asks for when it meets one (`DYNAMIC_FONT_FILES`, each a chunk of its own). The others (Cyrillic,
 * Hebrew, Arabic, Greek with accents…) are not shipped: MathJax then has no glyph and the conversion says which
 * character (`missingGlyph`).
 */
import { mathjax } from '@mathjax/src/js/mathjax.js';
import { TeX } from '@mathjax/src/js/input/tex.js';
import { SVG } from '@mathjax/src/js/output/svg.js';
import { liteAdaptor, type LiteAdaptor } from '@mathjax/src/js/adaptors/liteAdaptor.js';
import { RegisterHTMLHandler } from '@mathjax/src/js/handlers/html.js';
import '@mathjax/src/js/input/tex/base/BaseConfiguration.js';
import '@mathjax/src/js/input/tex/ams/AmsConfiguration.js';
import '@mathjax/src/js/input/tex/newcommand/NewcommandConfiguration.js';
import '@mathjax/src/js/input/tex/mhchem/MhchemConfiguration.js';
import '@mathjax/src/js/input/tex/textmacros/TextMacrosConfiguration.js';
import '@mathjax/src/js/input/tex/gensymb/GensymbConfiguration.js';
import '@mathjax/src/js/input/tex/cancel/CancelConfiguration.js';
import '@mathjax/src/js/input/tex/boldsymbol/BoldsymbolConfiguration.js';
import '@mathjax/src/js/input/tex/upgreek/UpgreekConfiguration.js';
import { MathJaxNewcmFont } from '@mathjax/mathjax-newcm-font/js/svg.js';
import { MathJaxMhchemFontExtension } from '@mathjax/mathjax-mhchem-font-extension/js/svg.js';
import type { SvgNode, TexOutput } from './equation';

/** The TeX packages the tool understands (see above). */
export const TEX_PACKAGES = ['base', 'ams', 'newcommand', 'mhchem', 'textmacros', 'gensymb', 'cancel', 'boldsymbol', 'upgreek'];

/** The parts of the font shipped with the app, by the name MathJax asks for them under. */
const DYNAMIC_FONT_FILES: Record<string, () => Promise<unknown>> = {
  latin: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/latin.js'),
  'latin-b': () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/latin-b.js'),
  'latin-i': () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/latin-i.js'),
  'latin-bi': () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/latin-bi.js'),
  accents: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/accents.js'),
  'accents-b-i': () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/accents-b-i.js'),
  arrows: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/arrows.js'),
  marrows: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/marrows.js'),
  math: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/math.js'),
  symbols: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/symbols.js'),
  'symbols-b-i': () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/symbols-b-i.js'),
  shapes: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/shapes.js'),
  mshapes: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/mshapes.js'),
  variants: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/variants.js'),
  'double-struck': () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/double-struck.js'),
  calligraphic: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/calligraphic.js'),
  script: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/script.js'),
  fraktur: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/fraktur.js'),
  'sans-serif': () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/sans-serif.js'),
  monospace: () => import('@mathjax/mathjax-newcm-font/js/svg/dynamic/monospace.js'),
};

/** The file name at the end of what MathJax asks for (`…/svg/dynamic/latin.js` → `latin`). */
export function dynamicFileName(request: string): string {
  return (request.split('/').pop() ?? '').replace(/\.js$/, '');
}

interface Engine {
  adaptor: LiteAdaptor;
  convert(latex: string): unknown;
}

let engine: Engine | null = null;

function start(): Engine {
  if (engine) return engine;
  mathjax.asyncLoad = (request: string) => {
    const load = DYNAMIC_FONT_FILES[dynamicFileName(request)];
    // A part of the font that is not shipped: MathJax then has no glyph for the character (and says so).
    return load ? load() : Promise.reject(new Error(`${request} is not shipped with Ardoise`));
  };
  const adaptor = liteAdaptor();
  RegisterHTMLHandler(adaptor);
  const font = new MathJaxNewcmFont();
  font.addExtension(MathJaxMhchemFontExtension);
  const svg = new SVG({ fontData: font, fontCache: 'none' });
  engine = {
    adaptor,
    convert: (latex) => {
      // A TeX input of its own for each conversion (about 2 ms): what one equation defines (`\newcommand`, `\def`,
      // `\label`) must not change how another is drawn, nor make it depend on what was typed before in the session.
      const tex = new TeX({
        packages: TEX_PACKAGES,
        // An error is reported, not drawn in red in the formula.
        formatError: (_jax: unknown, error: unknown) => {
          throw error;
        },
      });
      const document = mathjax.document('', { InputJax: tex, OutputJax: svg });
      return mathjax.handleRetriesFor(() => document.convert(latex, { display: true }));
    },
  };
  return engine;
}

/** A copy of a node of MathJax's light DOM: tag, attributes, children (geometry is read from it, never the DOM). */
function copy(adaptor: LiteAdaptor, node: unknown, depth = 0): SvgNode {
  // MathJax's trees are shallow (a few dozen levels for deeply nested formulas); this bounds a pathological one.
  if (depth > 400) throw new Error('formula nested too deeply');
  const n = node as Parameters<LiteAdaptor['kind']>[0];
  const tag = adaptor.kind(n).toLowerCase();
  if (tag === '#text') return { tag, attrs: {}, children: [], text: adaptor.value(n as never) ?? '' };
  const attrs: Record<string, string> = {};
  for (const { name, value } of adaptor.allAttributes(n as never)) attrs[name] = String(value);
  const children = adaptor.childNodes(n as never).map((child) => copy(adaptor, child, depth + 1));
  return { tag, attrs, children, text: tag === 'text' ? adaptor.textContent(n as never) : '' };
}

/** MathJax's SVG of `latex` in display style, as a copy, or its error message. */
export async function texToSvg(latex: string): Promise<TexOutput> {
  const { adaptor, convert } = start();
  let container: unknown;
  try {
    container = await convert(latex);
  } catch (error) {
    // MathJax's TeX errors are plain objects with a message (`TexError`), not `Error`s.
    const message = (error as { message?: unknown } | null)?.message;
    return { ok: false, message: typeof message === 'string' ? message : String(error) };
  }
  const root = copy(adaptor, container);
  const svg = root.tag === 'svg' ? root : root.children.find((child) => child.tag === 'svg');
  return svg ? { ok: true, svg } : { ok: false, message: 'MathJax made no drawing' };
}
