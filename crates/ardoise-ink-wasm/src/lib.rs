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
//!
//! Straightening shapes ([`ardoise_ink::straighten_with`]): the caller writes strokes into
//! words reserved by `ink_alloc` (see [`decode_strokes`]) and calls
//! `ink_straighten_tuned(ptr, words, tolerance, min_size_mm, snap_angle_deg,
//! snap_round_percent)`, the four last arguments being the user's tuning as `f64` (see
//! [`tuning_from`]: a negative angle or percentage turns that alignment off, values out of
//! their limits are brought within them; the default tuning is `3, 10, 4, 10`). It returns the
//! length in words of the result, at `ink_straighten_result()` until the next call: `[group
//! count, then for each group: source count, the sources (indices of the given strokes), shape
//! code (index into Shape::ALL), stroke count, the strokes]`, the strokes encoded like the
//! input.

use std::cell::RefCell;

use ardoise_ink::{Point, Rgba, Shape, ShapeTuning, Stroke, Tool, outline, straighten_with};

thread_local! {
    static RESULT: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static SHAPES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
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

/// The code of a tool (index into [`Tool::ALL`]).
pub fn tool_code(tool: Tool) -> u32 {
    Tool::ALL.iter().position(|&t| t == tool).unwrap_or(0) as u32
}

/// Strokes from words: `[count, then for each stroke: tool code, colour (0xRRGGBBAA), width
/// (f32 bits), layer, point count, then four words per point as for an outline]`. Input that
/// ends early gives the strokes complete so far.
pub fn decode_strokes(words: &[u32]) -> Vec<Stroke> {
    let mut rest = words;
    let mut take = |n: usize| -> Option<&[u32]> {
        let (head, tail) = rest.split_at_checked(n)?;
        rest = tail;
        Some(head)
    };
    let Some(&[count]) = take(1) else {
        return Vec::new();
    };
    let mut strokes = Vec::new();
    for _ in 0..count {
        let Some(&[tool, color, width, layer, points]) = take(5) else {
            break;
        };
        let Some(points) = (points as usize).checked_mul(4).and_then(&mut take) else {
            break;
        };
        strokes.push(Stroke {
            tool: tool_from_code(tool),
            color: Rgba::from_rgba_u32(color),
            width: f32::from_bits(width),
            layer,
            points: points
                .as_chunks::<4>()
                .0
                .iter()
                .map(|w| Point {
                    x: f32::from_bits(w[0]),
                    y: f32::from_bits(w[1]),
                    pressure: f32::from_bits(w[2]),
                    t: w[3],
                })
                .collect(),
        });
    }
    strokes
}

/// Appends `stroke` encoded as [`decode_strokes`] reads it (without the count).
pub fn encode_stroke(out: &mut Vec<u32>, stroke: &Stroke) {
    out.extend([
        tool_code(stroke.tool),
        stroke.color.to_rgba_u32(),
        stroke.width.to_bits(),
        stroke.layer,
        stroke.points.len() as u32,
    ]);
    for p in &stroke.points {
        out.extend([p.x.to_bits(), p.y.to_bits(), p.pressure.to_bits(), p.t]);
    }
}

/// The tuning given to `ink_straighten_tuned`: the tolerance step (rounded; not a number: the
/// default step), the minimum size in millimetres, and the alignments, a negative angle or
/// percentage meaning "off". [`straighten_with`] brings every value within its limits
/// ([`ShapeTuning::clamped`]: a value that is not a number takes its default).
pub fn tuning_from(
    tolerance: f64,
    min_size_mm: f64,
    snap_angle_deg: f64,
    snap_round_percent: f64,
) -> ShapeTuning {
    // Not a number stays on (and takes its default in `clamped`).
    let off_if_negative = |value: f64| (value >= 0.0 || value.is_nan()).then_some(value);
    ShapeTuning {
        tolerance: if tolerance.is_finite() {
            tolerance.round().clamp(0.0, f64::from(u8::MAX)) as u8
        } else {
            ShapeTuning::default().tolerance
        },
        min_size_mm,
        snap_angle_deg: off_if_negative(snap_angle_deg),
        snap_round_percent: off_if_negative(snap_round_percent),
    }
}

/// The shapes recognised with `tuning` among the strokes in `words`, encoded as described in
/// the module documentation.
pub fn straighten_words(words: &[u32], tuning: &ShapeTuning) -> Vec<u32> {
    let groups = straighten_with(&decode_strokes(words), tuning);
    let mut out = vec![groups.len() as u32];
    for group in &groups {
        out.push(group.sources.len() as u32);
        out.extend(group.sources.iter().map(|&i| i as u32));
        out.push(
            Shape::ALL
                .iter()
                .position(|&s| s == group.shape)
                .unwrap_or(0) as u32,
        );
        out.push(group.strokes.len() as u32);
        for stroke in &group.strokes {
            encode_stroke(&mut out, stroke);
        }
    }
    out
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

/// Straightens the strokes held in the `count` words at `words` (see [`decode_strokes`]) with
/// the tuning of the four last arguments (see [`tuning_from`]); returns the length of the result
/// (see [`ink_straighten_result`]).
///
/// # Safety
/// `words` must point to `count` initialised words (from [`ink_alloc`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ink_straighten_tuned(
    words: *const u32,
    count: usize,
    tolerance: f64,
    min_size_mm: f64,
    snap_angle_deg: f64,
    snap_round_percent: f64,
) -> usize {
    let words = if words.is_null() || count == 0 {
        &[][..]
    } else {
        // SAFETY: the caller guarantees `count` initialised words at `words`.
        unsafe { std::slice::from_raw_parts(words, count) }
    };
    let tuning = tuning_from(tolerance, min_size_mm, snap_angle_deg, snap_round_percent);
    let result = straighten_words(words, &tuning);
    let len = result.len();
    SHAPES.with(|shapes| *shapes.borrow_mut() = result);
    len
}

/// Address of the last result of [`ink_straighten_tuned`].
#[unsafe(no_mangle)]
pub extern "C" fn ink_straighten_result() -> *const u32 {
    SHAPES.with(|shapes| shapes.borrow().as_ptr())
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
    fn straightens_through_memory_exactly_like_the_native_code() {
        // A wobbly line on layer 2 and a short stroke that stays as it is.
        let mut line = stroke(Tool::Highlighter);
        line.layer = 2;
        line.color = Rgba::from_rgba_u32(0xffd4_0080);
        for (i, p) in line.points.iter_mut().enumerate() {
            p.y = 2000.0 + 15.0 * (i as f32 * 0.9).sin();
        }
        let mut short = stroke(Tool::Pencil);
        short.points.truncate(3);
        let strokes = [short, line];
        let mut data = vec![2];
        for s in &strokes {
            encode_stroke(&mut data, s);
        }
        assert_eq!(decode_strokes(&data), strokes);

        let result = straighten_through_memory(&data, [3.0, 10.0, 4.0, 10.0]);
        let native = straighten_with(&strokes, &ShapeTuning::default());
        assert_eq!(native.len(), 1);
        let mut expected = vec![1, 1, 1, 0, 1];
        encode_stroke(&mut expected, &native[0].strokes[0]);
        assert_eq!(result, expected);
        assert_eq!(native[0].strokes[0].layer, 2);
        assert_eq!(native[0].strokes[0].tool, Tool::Highlighter);
    }

    /// What `ink_straighten_tuned` gives for `data` with the tuning `[tolerance, min size,
    /// angle, percentage]`.
    fn straighten_through_memory(data: &[u32], tuning: [f64; 4]) -> Vec<u32> {
        let ptr = ink_alloc(data.len());
        let [tolerance, min_size, angle, percent] = tuning;
        // SAFETY: ptr holds `data.len()` words reserved just above.
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            let len = ink_straighten_tuned(ptr, data.len(), tolerance, min_size, angle, percent);
            let result = std::slice::from_raw_parts(ink_straighten_result(), len).to_vec();
            ink_free(ptr, data.len());
            result
        }
    }

    #[test]
    fn the_tuning_passed_through_memory_is_the_native_one() {
        // A line of 7 mm turned by 3 degrees: straightened with a minimum of 5 mm only, and
        // made horizontal unless the alignment is off.
        let mut line = stroke(Tool::Ballpoint);
        for (i, p) in line.points.iter_mut().enumerate() {
            let x = 24.0 * i as f32;
            p.x = 1000.0 + x;
            p.y = 2000.0 + x * 0.052 + 2.0 * (i as f32 * 0.9).sin();
        }
        let mut data = vec![1];
        encode_stroke(&mut data, &line);
        let strokes = [line];
        for tuning in [
            [3.0, 10.0, 4.0, 10.0],
            [3.0, 5.0, 4.0, 10.0],
            [3.0, 5.0, -1.0, 10.0],
            [5.0, 5.0, 10.0, -1.0],
            [1.0, 5.0, 2.0, 2.0],
        ] {
            let [tolerance, min_size, angle, percent] = tuning;
            let native =
                straighten_with(&strokes, &tuning_from(tolerance, min_size, angle, percent));
            let mut expected = vec![native.len() as u32];
            for group in &native {
                expected.extend([1, 0, 0, group.strokes.len() as u32]);
                for s in &group.strokes {
                    encode_stroke(&mut expected, s);
                }
            }
            assert_eq!(
                straighten_through_memory(&data, tuning),
                expected,
                "{tuning:?}"
            );
        }
        assert_eq!(
            straighten_through_memory(&data, [3.0, 10.0, 4.0, 10.0]),
            [0]
        );
        let horizontal = straighten_through_memory(&data, [3.0, 5.0, 4.0, 10.0]);
        let tilted = straighten_through_memory(&data, [3.0, 5.0, -1.0, 10.0]);
        // [groups, sources, source, shape, strokes, tool, colour, width, layer, points, x, y...]
        let first_y = |words: &[u32]| f32::from_bits(words[11]);
        let last_y = |words: &[u32]| f32::from_bits(words[words.len() - 3]);
        assert_eq!(first_y(&horizontal), last_y(&horizontal));
        assert!(last_y(&tilted) - first_y(&tilted) > 20.0);
    }

    #[test]
    fn tuning_arguments_are_read_as_documented() {
        assert_eq!(tuning_from(3.0, 10.0, 4.0, 10.0), ShapeTuning::default());
        assert_eq!(
            tuning_from(2.6, 7.5, -1.0, -0.5),
            ShapeTuning {
                tolerance: 3,
                min_size_mm: 7.5,
                snap_angle_deg: None,
                snap_round_percent: None,
            }
        );
        // Out of range or not numbers: brought within the limits by `clamped`.
        assert_eq!(
            tuning_from(f64::NAN, f64::NAN, f64::NAN, 1e9).clamped(),
            ShapeTuning {
                snap_round_percent: Some(ShapeTuning::SNAP_ROUND_PERCENT_MAX),
                ..ShapeTuning::default()
            }
        );
        assert_eq!(tuning_from(-7.0, 0.0, 0.0, 0.0).clamped().tolerance, 1);
        assert_eq!(tuning_from(1e12, 0.0, 0.0, 0.0).clamped().tolerance, 5);
        assert_eq!(tuning_from(f64::INFINITY, 0.0, 0.0, 0.0).tolerance, 3);
    }

    #[test]
    fn truncated_stroke_words_are_harmless() {
        let mut data = vec![3];
        encode_stroke(&mut data, &stroke(Tool::Ballpoint));
        encode_stroke(&mut data, &stroke(Tool::Chalk));
        data.truncate(data.len() - 7);
        assert_eq!(decode_strokes(&data).len(), 1);
        assert!(decode_strokes(&[]).is_empty());
        let default = ShapeTuning::default();
        assert_eq!(straighten_words(&[], &default), [0]);
        assert_eq!(straighten_words(&[5, 1, 2], &default), [0]);
        // SAFETY: a null pointer with no words is accepted.
        let len = unsafe { ink_straighten_tuned(std::ptr::null(), 0, 3.0, 10.0, 4.0, 10.0) };
        assert_eq!(len, 1);
        for tool in Tool::ALL {
            assert_eq!(tool_from_code(tool_code(tool)), tool);
        }
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
