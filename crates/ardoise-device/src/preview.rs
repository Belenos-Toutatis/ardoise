//! A minimal SVG drawing of a downloaded page, saved next to the raw bytes during a guided
//! test so that a page can be checked by eye from any browser. The real renderer comes later
//! (`ardoise-render`); this one draws plain polylines in device units.

use std::fmt::Write as _;

use ardoise_proto::stroke::StrokeFile;

/// Line width in device units (0.3 mm).
const LINE_WIDTH: u32 = 30;

/// SVG of `file` on a `width` × `height` page (device units, 10 µm).
pub fn svg(file: &StrokeFile, width: u32, height: u32) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{}mm" height="{}mm">"#,
        width / 100,
        height / 100
    );
    let _ = writeln!(
        text,
        r#"<rect width="{width}" height="{height}" fill="white"/>"#
    );
    let _ = writeln!(
        text,
        r#"<g fill="none" stroke="black" stroke-width="{LINE_WIDTH}" stroke-linecap="round" stroke-linejoin="round">"#
    );
    for stroke in &file.strokes {
        if stroke.points.is_empty() {
            continue;
        }
        text.push_str(r#"<polyline points=""#);
        for (i, point) in stroke.points.iter().enumerate() {
            if i > 0 {
                text.push(' ');
            }
            let _ = write!(text, "{},{}", point.x, point.y);
        }
        text.push_str("\"/>\n");
    }
    text.push_str("</g>\n</svg>\n");
    text
}
