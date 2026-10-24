/**
 * Stroke outlines computed by `ardoise-ink` compiled to WebAssembly (`crates/ardoise-ink-wasm`):
 * the screen draws exactly the geometry of the PNG/PDF/video exports (SPEC §6.2). Outlines
 * must be filled with the non-zero rule.
 */
import wasmUrl from '../assets/ardoise_ink.wasm?url';

/** Ink tools, as serialised by Rust (`ardoise_ink::Tool`). */
export type Tool = 'ballpoint' | 'fountainPen' | 'feltTip' | 'pencil' | 'brush' | 'highlighter' | 'chalk';

/** Order of `Tool::ALL` in Rust: the code passed to the module. */
export const TOOLS: Tool[] = ['ballpoint', 'fountainPen', 'feltTip', 'pencil', 'brush', 'highlighter', 'chalk'];

/** One pen sample: page coordinates in device units (10 µm), pressure 0..1, ms. */
export interface InkPoint {
  x: number;
  y: number;
  pressure: number;
  t: number;
}

/** What the geometry needs from a stroke. */
export interface InkStroke {
  tool: Tool;
  /** Nominal diameter at full pressure, device units. */
  width: number;
  points: InkPoint[];
}

export interface Ink {
  /** Closed contours, each a flat `[x0, y0, x1, y1…]` list. */
  contours(stroke: InkStroke): Float32Array[];
  /** The contours as SVG path data. */
  path(stroke: InkStroke): string;
  /** Line diameter of `tool` at `pressure` for a stroke of nominal `width`. */
  widthAt(tool: Tool, width: number, pressure: number): number;
  /** Inverse of `widthAt`. */
  pressureForWidth(tool: Tool, width: number, diameter: number): number;
}

interface Exports {
  memory: WebAssembly.Memory;
  ink_alloc(words: number): number;
  ink_free(ptr: number, words: number): void;
  ink_outline(ptr: number, count: number, tool: number, width: number): number;
  ink_result(): number;
  ink_width_at(tool: number, width: number, pressure: number): number;
  ink_pressure_for_width(tool: number, width: number, diameter: number): number;
}

function toolCode(tool: Tool): number {
  const code = TOOLS.indexOf(tool);
  return code < 0 ? 0 : code;
}

/** Instantiates the module from its bytes (tests read the file; the app fetches it). */
export async function instantiateInk(bytes: BufferSource): Promise<Ink> {
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const wasm = instance.exports as unknown as Exports;

  function contours(stroke: InkStroke): Float32Array[] {
    const count = stroke.points.length;
    const words = count * 4;
    const ptr = words > 0 ? wasm.ink_alloc(words) : 0;
    try {
      if (words > 0) {
        // Views are made after the allocation: growing the memory detaches older ones.
        const floats = new Float32Array(wasm.memory.buffer, ptr, words);
        const ints = new Uint32Array(wasm.memory.buffer, ptr, words);
        stroke.points.forEach((p, i) => {
          floats[i * 4] = p.x;
          floats[i * 4 + 1] = p.y;
          floats[i * 4 + 2] = p.pressure;
          ints[i * 4 + 3] = Math.max(0, Math.min(0xffffffff, Math.round(p.t)));
        });
      }
      const len = wasm.ink_outline(ptr, count, toolCode(stroke.tool), stroke.width);
      const result = new Float32Array(wasm.memory.buffer, wasm.ink_result(), len);
      const out: Float32Array[] = [];
      let at = 1;
      for (let c = 0; c < result[0]; c++) {
        const points = result[at];
        out.push(result.slice(at + 1, at + 1 + points * 2));
        at += 1 + points * 2;
      }
      return out;
    } finally {
      if (words > 0) wasm.ink_free(ptr, words);
    }
  }

  return {
    contours,
    path(stroke) {
      return contours(stroke)
        .map((c) => {
          let d = '';
          for (let i = 0; i < c.length; i += 2) {
            d += `${i === 0 ? 'M' : 'L'}${c[i].toFixed(1)} ${c[i + 1].toFixed(1)}`;
          }
          return d + 'Z';
        })
        .join('');
    },
    widthAt: (tool, width, pressure) => wasm.ink_width_at(toolCode(tool), width, pressure),
    pressureForWidth: (tool, width, diameter) => wasm.ink_pressure_for_width(toolCode(tool), width, diameter),
  };
}

let loading: Promise<Ink> | null = null;

/** The module, fetched once. */
export function loadInk(): Promise<Ink> {
  loading ??= fetch(wasmUrl)
    .then((response) => response.arrayBuffer())
    .then((bytes) => instantiateInk(bytes));
  return loading;
}
