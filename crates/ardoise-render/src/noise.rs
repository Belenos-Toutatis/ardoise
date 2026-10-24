//! Deterministic noise for paper texture and ink grain: integer hashes, no state, no
//! randomness, so the same note always renders to the same bytes.

/// Integer hash with good avalanche.
pub(crate) fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// Hash of a lattice cell.
pub(crate) fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    hash((x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca77) ^ seed)
}

/// A hash mapped to 0..1.
pub(crate) fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// `x.floor()` as an integer, without the C library call `floor` compiles to on the
/// baseline x86-64 target (it matters in per-pixel loops). Saturates far from zero.
pub(crate) fn floor_i32(x: f32) -> i32 {
    let i = x as i32;
    if (i as f32) > x { i - 1 } else { i }
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Smooth 2D value noise in 0..1, one lattice cell per unit. With `period` set to
/// `(px, py)`, the lattice wraps every `px` cells along x and `py` along y (seamless tiles).
pub(crate) fn value_noise(x: f32, y: f32, seed: u32, period: Option<(i32, i32)>) -> f32 {
    let (ix, iy) = (floor_i32(x), floor_i32(y));
    let (tx, ty) = (smooth(x - ix as f32), smooth(y - iy as f32));
    let wrap = |i: i32, p: Option<i32>| p.map_or(i, |p| i.rem_euclid(p.max(1)));
    let at = |dx: i32, dy: i32| {
        unit(hash2(
            wrap(ix.wrapping_add(dx), period.map(|p| p.0)),
            wrap(iy.wrapping_add(dy), period.map(|p| p.1)),
            seed,
        ))
    };
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * tx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * tx;
    top + (bottom - top) * ty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_bounded_and_periodic() {
        for i in 0..500 {
            let (x, y) = (i as f32 * 0.37 - 50.0, i as f32 * 0.91 - 120.0);
            let n = value_noise(x, y, 7, None);
            assert!((0.0..=1.0).contains(&n));
            assert_eq!(n, value_noise(x, y, 7, None));
            let p = value_noise(x, y, 7, Some((16, 8)));
            assert!((p - value_noise(x + 16.0, y - 32.0, 7, Some((16, 8)))).abs() < 1e-4);
        }
        assert_ne!(hash2(1, 2, 3), hash2(2, 1, 3));
        for x in [-2.5f32, -1.0, -0.2, 0.0, 0.7, 3.0, 1e6 + 0.5] {
            assert_eq!(floor_i32(x), x.floor() as i32, "{x}");
        }
    }
}
