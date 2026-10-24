//! WebAssembly entry points of `ardoise-ink`, so that the screen draws strokes with exactly the
//! geometry of the exports (SPEC §6.2). Plain exported functions over the module's linear
//! memory, without wasm-bindgen (no extra build tool). The interface side is
//! `app/ui/src/lib/ink.ts`.
//!
//! Protocol, for one stroke:
//! 1. `ink_alloc(4 × n)` reserves `n` points of four 32-bit words: `x`, `y`, `pressure` (f32
//!    bit patterns) and `t` (u32, milliseconds).
//! 2. The caller writes the points there and calls `ink_outline(ptr, n, tool, width)`, which
//!    returns the number of f32 values in the result and keeps the result until the next call.
//! 3. `ink_result()` points to that result: `[contour count, then for each contour its point
//!    count followed by x0, y0, x1, y1…]`, to be filled with the non-zero rule.
//! 4. `ink_free(ptr, 4 × n)` releases the points.
//!
//! Tool codes are indices into [`Tool::ALL`]; an unknown code means the ballpoint.

use std::cell::RefCell;

use ardoise_ink::{Point, Rgba, Stroke, Tool, outline};

thread_local! {
    static RESULT: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

/// The tool for a code (index into [`Tool::ALL`]).
pub fn tool_from_code(code: u32) -> Tool {
    Tool::ALL
        .get(code as usize)
        .copied()
        .unwrap_or(Tool::Ballpoint)
}

/// Outline of the stroke whose points are `words` (four words per point, see the module
/// documentation), flattened as `[contours, len, x, y…, len, x, y…]`.
pub fn outline_words(words: &[u32], tool: u32, width: f32) -> Vec<f32> {
    let points = words
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| Point {
            x: f32::from_bits(w[0]),
            y: f32::from_bits(w[1]),
            pressure: f32::from_bits(w[2]),
            t: w[3],
        })
        .collect();
    let stroke = Stroke {
        tool: tool_from_code(tool),
        color: Rgba::BLACK,
        width,
        layer: 0,
        points,
    };
    let contours = outline(&stroke).contours;
    let mut flat = Vec::with_capacity(1 + contours.iter().map(|c| 1 + 2 * c.len()).sum::<usize>());
    flat.push(contours.len() as f32);
    for contour in &contours {
        flat.push(contour.len() as f32);
        flat.extend(contour.iter().flatten());
    }
    flat
}

/// Reserves `words` 32-bit words and returns their address (null for 0).
#[unsafe(no_mangle)]
pub extern "C" fn ink_alloc(words: usize) -> *mut u32 {
    if words == 0 {
        return std::ptr::null_mut();
    }
    let mut buffer = Vec::<u32>::with_capacity(words);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Releases words reserved by [`ink_alloc`].
///
/// # Safety
/// `ptr` must come from `ink_alloc(words)` with the same `words`, and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ink_free(ptr: *mut u32, words: usize) {
    if !ptr.is_null() && words > 0 {
        // SAFETY: the caller guarantees ptr/words come from `ink_alloc`.
        drop(unsafe { Vec::from_raw_parts(ptr, 0, words) });
    }
}

/// Computes the outline of the `count` points at `points`; returns the length of the result
/// (see [`ink_result`]).
///
/// # Safety
/// `points` must point to `4 × count` initialised words (from [`ink_alloc`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ink_outline(
    points: *const u32,
    count: usize,
    tool: u32,
    width: f32,
) -> usize {
    let words = if points.is_null() || count == 0 {
        &[][..]
    } else {
        // SAFETY: the caller guarantees `4 × count` initialised words at `points`.
        unsafe { std::slice::from_raw_parts(points, count * 4) }
    };
    let flat = outline_words(words, tool, width);
    let len = flat.len();
    RESULT.with(|result| *result.borrow_mut() = flat);
    len
}

/// Address of the last result of [`ink_outline`].
#[unsafe(no_mangle)]
pub extern "C" fn ink_result() -> *const f32 {
    RESULT.with(|result| result.borrow().as_ptr())
}

/// Line diameter of `tool` for a stroke of nominal `width` at `pressure`.
#[unsafe(no_mangle)]
pub extern "C" fn ink_width_at(tool: u32, width: f32, pressure: f32) -> f32 {
    tool_from_code(tool).width_at(width, pressure)
}

/// Pressure that gives a line of `diameter` (inverse of [`ink_width_at`]).
#[unsafe(no_mangle)]
pub extern "C" fn ink_pressure_for_width(tool: u32, width: f32, diameter: f32) -> f32 {
    tool_from_code(tool).pressure_for_width(width, diameter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(tool: Tool) -> Stroke {
        Stroke {
            tool,
            color: Rgba::BLACK,
            width: 90.0,
            layer: 0,
            points: (0..30)
                .map(|i| Point {
                    x: 1000.0 + 120.0 * i as f32,
                    y: 2000.0 + 300.0 * (i as f32 * 0.4).sin(),
                    pressure: (i as f32 / 29.0).clamp(0.0, 1.0),
                    t: 5 * i,
                })
                .collect(),
        }
    }

    fn words(stroke: &Stroke) -> Vec<u32> {
        stroke
            .points
            .iter()
            .flat_map(|p| [p.x.to_bits(), p.y.to_bits(), p.pressure.to_bits(), p.t])
            .collect()
    }

    #[test]
    fn gives_exactly_the_native_outline_for_every_tool() {
        for (code, tool) in Tool::ALL.into_iter().enumerate() {
            let stroke = stroke(tool);
            let flat = outline_words(&words(&stroke), code as u32, stroke.width);
            let native = outline(&stroke).to_flat();
            let mut expected = vec![native.len() as f32];
            for contour in &native {
                expected.push((contour.len() / 2) as f32);
                expected.extend(contour);
            }
            assert_eq!(flat, expected, "{tool:?}");
        }
    }

    #[test]
    fn exports_round_trip_through_memory() {
        let stroke = stroke(Tool::Brush);
        let data = words(&stroke);
        let ptr = ink_alloc(data.len());
        // SAFETY: ptr holds `data.len()` words reserved just above.
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            let len = ink_outline(ptr, stroke.points.len(), 4, stroke.width);
            let result = std::slice::from_raw_parts(ink_result(), len);
            assert_eq!(result, outline_words(&data, 4, stroke.width).as_slice());
            ink_free(ptr, data.len());
        }
        assert_eq!(ink_width_at(0, 50.0, 1.0), 50.0);
        let p = ink_pressure_for_width(0, 50.0, ink_width_at(0, 50.0, 0.3));
        assert!((p - 0.3).abs() < 1e-5);
    }

    #[test]
    fn empty_and_unknown_input_is_harmless() {
        // SAFETY: a null pointer with no points is accepted.
        let len = unsafe { ink_outline(std::ptr::null(), 0, 99, 50.0) };
        assert_eq!(len, 1);
        assert_eq!(tool_from_code(99), Tool::Ballpoint);
        assert!(ink_alloc(0).is_null());
    }
}
