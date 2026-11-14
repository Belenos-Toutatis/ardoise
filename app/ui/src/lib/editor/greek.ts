/**
 * The Greek palette of the text tool (`GreekPalette.svelte`): the whole Greek alphabet in both cases, the variants physics
 * writes (ϵ ϑ ϕ ϖ ϱ), and the symbols of physics and chemistry the fonts now have (`crates/ardoise-text`, Latin Modern
 * Math). Each character has an accessible name built from the translations (`editor.text.greek.*`).
 */
import { t } from '../i18n.svelte';

/** One character of the palette: what it inserts, and the key of its name. */
export interface PaletteChar {
  char: string;
  /** `letters.<name>` for a Greek letter, `symbols.<name>` for a symbol; scripts are named from their digit or sign. */
  name: string;
  /** How the name is said: a letter in its case, a variant, a symbol, a superscript or subscript. */
  form: 'lower' | 'upper' | 'variant' | 'symbol' | 'superscript' | 'subscript';
}

export interface PaletteGroup {
  id: 'lower' | 'upper' | 'variants' | 'symbols' | 'scripts';
  chars: PaletteChar[];
}

/** The letters in alphabetical order, with the key of their name; final sigma after sigma. */
const LETTERS: [lower: string, upper: string | null, name: string][] = [
  ['α', 'Α', 'alpha'],
  ['β', 'Β', 'beta'],
  ['γ', 'Γ', 'gamma'],
  ['δ', 'Δ', 'delta'],
  ['ε', 'Ε', 'epsilon'],
  ['ζ', 'Ζ', 'zeta'],
  ['η', 'Η', 'eta'],
  ['θ', 'Θ', 'theta'],
  ['ι', 'Ι', 'iota'],
  ['κ', 'Κ', 'kappa'],
  ['λ', 'Λ', 'lambda'],
  ['μ', 'Μ', 'mu'],
  ['ν', 'Ν', 'nu'],
  ['ξ', 'Ξ', 'xi'],
  ['ο', 'Ο', 'omicron'],
  ['π', 'Π', 'pi'],
  ['ρ', 'Ρ', 'rho'],
  ['σ', 'Σ', 'sigma'],
  ['ς', null, 'finalSigma'],
  ['τ', 'Τ', 'tau'],
  ['υ', 'Υ', 'upsilon'],
  ['φ', 'Φ', 'phi'],
  ['χ', 'Χ', 'chi'],
  ['ψ', 'Ψ', 'psi'],
  ['ω', 'Ω', 'omega'],
];

const VARIANTS: [string, string][] = [
  ['ϵ', 'epsilon'],
  ['ϑ', 'theta'],
  ['ϕ', 'phi'],
  ['ϖ', 'pi'],
  ['ϱ', 'rho'],
];

const SYMBOLS: [string, string][] = [
  ['µ', 'micro'],
  ['Ω', 'ohm'],
  ['Δ', 'variation'],
  ['°', 'degree'],
  ['±', 'plusMinus'],
  ['×', 'times'],
  ['·', 'dot'],
  ['−', 'minus'],
  ['≈', 'approx'],
  ['≠', 'notEqual'],
  ['≤', 'lessEqual'],
  ['≥', 'greaterEqual'],
  ['→', 'arrow'],
  ['⇌', 'equilibrium'],
  ['∞', 'infinity'],
  ['√', 'root'],
  ['∝', 'proportional'],
  ['∂', 'partial'],
  ['∑', 'sum'],
  ['∫', 'integral'],
  ['′', 'prime'],
  ['″', 'doublePrime'],
  ['‰', 'perMille'],
];

const SUPERSCRIPTS = '⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻';
const SUBSCRIPTS = '₀₁₂₃₄₅₆₇₈₉₊₋';
/** What each script digit or sign is (`0`…`9`, `plus`, `minus`). */
const SCRIPT_NAMES = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'plus', 'minus'];

export const PALETTE: readonly PaletteGroup[] = [
  { id: 'lower', chars: LETTERS.map(([char, , name]) => ({ char, name: `letters.${name}`, form: 'lower' })) },
  {
    id: 'upper',
    chars: LETTERS.filter(([, upper]) => upper !== null).map(([, upper, name]) => ({ char: upper as string, name: `letters.${name}`, form: 'upper' })),
  },
  { id: 'variants', chars: VARIANTS.map(([char, name]) => ({ char, name: `letters.${name}`, form: 'variant' })) },
  { id: 'symbols', chars: SYMBOLS.map(([char, name]) => ({ char, name: `symbols.${name}`, form: 'symbol' })) },
  {
    id: 'scripts',
    chars: [
      ...Array.from(SUPERSCRIPTS).map((char, i): PaletteChar => ({ char, name: `scripts.${SCRIPT_NAMES[i]}`, form: 'superscript' })),
      ...Array.from(SUBSCRIPTS).map((char, i): PaletteChar => ({ char, name: `scripts.${SCRIPT_NAMES[i]}`, form: 'subscript' })),
    ],
  },
];

/** The first letter of `text` in upper case (« Alpha majuscule »). */
function capitalised(text: string): string {
  return text.charAt(0).toLocaleUpperCase() + text.slice(1);
}

/** The accessible name of a character of the palette, in the interface's language. */
export function paletteName(c: PaletteChar): string {
  const name = t(`editor.text.greek.${c.name}`);
  switch (c.form) {
    case 'lower':
      return t('editor.text.greek.lowerName', { name });
    case 'upper':
      return t('editor.text.greek.upperName', { name: capitalised(name) });
    case 'variant':
      return t('editor.text.greek.variantName', { name });
    case 'superscript':
      return t('editor.text.greek.superscriptName', { name });
    case 'subscript':
      return t('editor.text.greek.subscriptName', { name });
    default:
      return name;
  }
}
