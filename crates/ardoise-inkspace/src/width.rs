//! Stroke widths: Inkspace stores the drawn diameter at each point, Ardoise stores a pressure
//! per point and a nominal width per stroke, and the ballpoint turns them back into a
//! diameter. The importer picks the nominal width so that the round trip reproduces the
//! stored diameters (scaled by [`width_factor`]).

use ardoise_ink::Tool;

/// Diameter of the ballpoint at `pressure` (0..=1) for a stroke of nominal width
/// `stroke_width`: exactly what the renderer draws at each sample.
pub fn width_at(stroke_width: f32, pressure: f32) -> f32 {
    Tool::Ballpoint.width_at(stroke_width, pressure)
}

/// Exact inverse of [`width_at`]: the pressure that draws `diameter`, clamped to 0..=1.
pub fn pressure_for_width(stroke_width: f32, diameter: f32) -> f32 {
    if stroke_width <= 0.0 || !diameter.is_finite() {
        return 0.0;
    }
    Tool::Ballpoint.pressure_for_width(stroke_width, diameter)
}

/// Nominal width for a stroke whose diameters are `diameters`: the smallest width whose
/// full-pressure diameter reaches the thickest point, so every point maps to a pressure in
/// 0..=1. Points thinner than the response can draw (below `width_at(width, 0)`) are clipped
/// to pressure 0; [`is_representable`] tells which.
pub fn stroke_width_for(diameters: impl IntoIterator<Item = f32>) -> f32 {
    let max = diameters
        .into_iter()
        .filter(|d| d.is_finite())
        .fold(0.0f32, f32::max);
    let full = width_at(1.0, 1.0);
    if full > 0.0 { max / full } else { max }
}

/// Whether `diameter` survives the round trip through a pressure (within 0.5 %).
pub fn is_representable(stroke_width: f32, diameter: f32) -> bool {
    let back = width_at(stroke_width, pressure_for_width(stroke_width, diameter));
    (back - diameter).abs() <= diameter.abs() * 0.005 + 1e-3
}

/// Scale from Inkspace's stored width to the diameter Ardoise draws, per stroke.
///
/// Tuned with `ardoise_render::render_note` (the ballpoint fills one outline whose diameter is
/// exactly `width_at` at each sample) on the private copy of 22 notes against Inkspace's
/// previews: timed strokes (`pointsPerSecond > 0`: synced from the Slate at 200 pps, or drawn
/// in the app at 60 pps) match best at the stored width (factor 1.0, IoU 0.942–0.966), untimed
/// strokes (no or zero `pointsPerSecond`) slightly thicker (1.1, IoU 0.955–0.969). Factors
/// 0.95/1.1 and 1.0/1.2 were worse (mean IoU 0.93 and 0.915).
pub fn width_factor(points_per_second: Option<f64>) -> f32 {
    match points_per_second {
        Some(pps) if pps > 0.0 => TIMED_WIDTH_FACTOR,
        _ => UNTIMED_WIDTH_FACTOR,
    }
}

/// See [`width_factor`].
pub const TIMED_WIDTH_FACTOR: f32 = 1.0;
/// See [`width_factor`].
pub const UNTIMED_WIDTH_FACTOR: f32 = 1.1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_reproduces_the_widths() {
        // Inkspace's observed range: 1.0 to 2.12 page units, ×25 device units.
        let diameters: Vec<f32> = [1.0f32, 1.06, 1.5, 2.0, 2.1, 2.12]
            .iter()
            .map(|w| w * 25.0)
            .collect();
        let width = stroke_width_for(diameters.iter().copied());
        assert!((width - 53.0).abs() < 1e-3);
        for &d in &diameters {
            let p = pressure_for_width(width, d);
            assert!((0.0..=1.0).contains(&p));
            assert!((width_at(width, p) - d).abs() < 1e-3, "{d}");
            assert!(is_representable(width, d));
        }
        assert_eq!(pressure_for_width(width, 53.0), 1.0);
        // Thinner than the response allows: clipped to pressure 0.
        assert_eq!(pressure_for_width(width, 10.0), 0.0);
        assert!(!is_representable(width, 10.0));
    }

    #[test]
    fn width_factor_per_stroke_class() {
        assert_eq!(width_factor(Some(200.0)), TIMED_WIDTH_FACTOR);
        assert_eq!(width_factor(Some(60.0)), TIMED_WIDTH_FACTOR);
        assert_eq!(width_factor(Some(0.0)), UNTIMED_WIDTH_FACTOR);
        assert_eq!(width_factor(None), UNTIMED_WIDTH_FACTOR);
    }

    #[test]
    fn degenerate_inputs() {
        assert_eq!(pressure_for_width(0.0, 5.0), 0.0);
        assert_eq!(pressure_for_width(10.0, f32::NAN), 0.0);
        assert_eq!(stroke_width_for([]), 0.0);
        assert_eq!(stroke_width_for([f32::NAN, 2.0]), 2.0);
        assert_eq!(width_at(10.0, 2.0), 10.0);
        assert_eq!(width_at(10.0, -1.0), 4.5);
    }
}
