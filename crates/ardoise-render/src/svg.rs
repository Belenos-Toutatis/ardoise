//! SVG export (SPEC §4.6): the page in millimetres, coordinates in device units (10 µm), one
//! `<path>` per fill of each visible stroke ([`crate::look`]), each note layer as an Inkscape
//! layer. Opens in Inkscape and in browsers.

use std::fmt::Write;

use ardoise_core::note::Note;

use crate::background::{Mark, Shape, VectorBackground};
use crate::look::{Blend, Fill};
use crate::vector::Scene;

/// JPEG quality of the embedded paper texture (a faint grain).
pub(crate) const TEXTURE_QUALITY: u8 = 85;

/// How to write a note as SVG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvgOptions {
    /// Draw the paper colour, template and texture; otherwise the page is transparent.
    pub background: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self { background: true }
    }
}

/// The note as an SVG document.
pub fn note_to_svg(note: &Note, options: &SvgOptions) -> String {
    let scene = Scene::of(note, options.background);
    let mut out = String::with_capacity(4096 + scene.fills().count() * 512);
    let (w, h) = (scene.width, scene.height);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n");
    out.push_str(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" \
         xmlns:sodipodi=\"http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd\" version=\"1.1\"",
    );
    let _ = writeln!(
        out,
        " width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\">",
        number(w / 100.0),
        number(h / 100.0),
        number(w),
        number(h)
    );
    let title = escape(note.title.trim());
    if !title.is_empty() {
        let _ = writeln!(out, "<title>{title}</title>");
    }
    if let Some(background) = &scene.background {
        write_background(&mut out, background, w, h);
    }
    for layer in &scene.layers {
        let label = match &layer.name {
            Some(name) => escape(name),
            None => format!("{}", layer.index + 1),
        };
        let _ = writeln!(
            out,
            "<g id=\"layer{}\" inkscape:groupmode=\"layer\" inkscape:label=\"{label}\">",
            layer.index + 1
        );
        for fill in &layer.fills {
            write_fill(&mut out, fill);
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

/// The paper, its texture and the template, in a locked Inkscape layer.
fn write_background(out: &mut String, background: &VectorBackground, w: f32, h: f32) {
    out.push_str(
        "<g id=\"background\" inkscape:groupmode=\"layer\" inkscape:label=\"Background\" \
         sodipodi:insensitive=\"true\">\n",
    );
    let _ = writeln!(
        out,
        "<rect width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        number(w),
        number(h),
        hex(background.paper.r, background.paper.g, background.paper.b)
    );
    if let Some(texture) = &background.texture
        && let Ok(jpeg) = crate::jpeg::encode_image(texture, TEXTURE_QUALITY, None)
    {
        let _ = writeln!(
            out,
            "<image width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" \
             xlink:href=\"data:image/jpeg;base64,{}\"/>",
            number(w),
            number(h),
            base64(&jpeg)
        );
    }
    // Consecutive marks of the same colour share a group.
    let mut style: Option<([u8; 3], u32)> = None;
    for mark in &background.marks {
        let key = (mark.rgb, mark.alpha.to_bits());
        if style != Some(key) {
            if style.is_some() {
                out.push_str("</g>\n");
            }
            let _ = write!(
                out,
                "<g fill=\"{}\"",
                hex(mark.rgb[0], mark.rgb[1], mark.rgb[2])
            );
            write_opacity(out, "fill-opacity", mark.alpha);
            out.push_str(">\n");
            style = Some(key);
        }
        write_mark(out, mark);
    }
    if style.is_some() {
        out.push_str("</g>\n");
    }
    out.push_str("</g>\n");
}

fn write_mark(out: &mut String, mark: &Mark) {
    let _ = match mark.shape {
        Shape::Rect {
            x,
            y,
            width,
            height,
        } => writeln!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
            number(x),
            number(y),
            number(width),
            number(height)
        ),
        Shape::Disc { cx, cy, r } => writeln!(
            out,
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
            number(cx),
            number(cy),
            number(r)
        ),
    };
}

fn write_fill(out: &mut String, fill: &Fill) {
    let [r, g, b] = fill.rgb;
    let _ = write!(
        out,
        "<path d=\"{}\" fill=\"{}\" fill-rule=\"nonzero\"",
        fill.outline.to_svg_path(),
        hex(r, g, b)
    );
    write_opacity(out, "fill-opacity", fill.flat_opacity());
    match fill.blend {
        Blend::Normal => {}
        Blend::Multiply => out.push_str(" style=\"mix-blend-mode:multiply\""),
        Blend::Screen => out.push_str(" style=\"mix-blend-mode:screen\""),
    }
    out.push_str("/>\n");
}

/// ` name="0.45"`, left out when fully opaque.
fn write_opacity(out: &mut String, name: &str, opacity: f32) {
    let opacity = if opacity.is_finite() {
        opacity.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if opacity < 0.9995 {
        let text = format!("{opacity:.3}");
        let _ = write!(out, " {name}=\"{}\"", trim_decimals(&text));
    }
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `value` with at most two decimals, no trailing zeros, `0` for anything not finite.
pub(crate) fn number(value: f32) -> String {
    if !value.is_finite() {
        return "0".into();
    }
    let text = format!("{value:.2}");
    let text = trim_decimals(&text);
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

fn trim_decimals(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

/// Text for XML content and attribute values: markup characters escaped, characters that
/// XML 1.0 forbids dropped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Standard base64 (RFC 4648, with padding).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::note::{Background, Layer, Origin, PageSize, Paper, Template, Timing};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    fn note(strokes: Vec<Stroke>, background: Background) -> Note {
        Note {
            id: "id".into(),
            title: "Essai <1> & \"2\"\u{1}".into(),
            created: 0,
            modified: 0,
            page: PageSize {
                width: 5000,
                height: 4000,
            },
            background,
            tags: vec![],
            favorite: false,
            layers: vec![Layer {
                name: "Calque & co".into(),
                visible: true,
            }],
            strokes,
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        }
    }

    fn line(tool: Tool, y: f32) -> Stroke {
        Stroke {
            tool,
            color: Rgba::rgb(200, 30, 60),
            width: 80.0,
            layer: 0,
            points: (0..10)
                .map(|i| Point {
                    x: 500.0 + 300.0 * i as f32,
                    y,
                    pressure: 0.7,
                    t: 5 * i,
                })
                .collect(),
        }
    }

    #[test]
    fn numbers_and_text() {
        assert_eq!(number(29_700.0), "29700");
        assert_eq!(number(0.125), "0.12");
        assert_eq!(number(-0.001), "0");
        assert_eq!(number(f32::NAN), "0");
        assert_eq!(number(2.5), "2.5");
        assert_eq!(escape("a<b>&\"'\u{1}\n"), "a&lt;b&gt;&amp;&quot;&apos; ");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
    }

    #[test]
    fn writes_one_path_per_fill_with_the_tool_look() {
        let n = note(
            vec![
                line(Tool::Ballpoint, 500.0),
                line(Tool::FeltTip, 1500.0),
                line(Tool::Highlighter, 2500.0),
                line(Tool::Pencil, 3500.0),
            ],
            Background {
                paper: Paper::Cream,
                template: Template::Grid5mm,
                texture: false,
            },
        );
        let svg = note_to_svg(&n, &SvgOptions::default());
        assert!(svg.starts_with("<?xml"));
        assert!(svg.contains("width=\"50mm\" height=\"40mm\" viewBox=\"0 0 5000 4000\""));
        assert_eq!(svg.matches("<path ").count(), 5);
        assert_eq!(svg.matches("mix-blend-mode:multiply").count(), 1);
        assert!(svg.contains("<title>Essai &lt;1&gt; &amp; &quot;2&quot;</title>"));
        assert!(svg.contains("inkscape:label=\"Calque &amp; co\""));
        // Cream paper and grid lines.
        assert!(svg.contains("fill=\"#faf4e2\""));
        assert!(svg.matches("<rect ").count() > 10);
        assert!(!svg.contains("<image"));
        // The felt tip's halo at 0.3 and core at 0.9.
        assert!(svg.contains("fill-opacity=\"0.3\""));
        assert!(svg.contains("fill-opacity=\"0.9\""));

        let bare = note_to_svg(&n, &SvgOptions { background: false });
        assert!(!bare.contains("<rect"));
        assert_eq!(bare.matches("<path ").count(), 5);
    }

    #[test]
    fn the_texture_is_an_embedded_image() {
        let n = note(
            vec![],
            Background {
                paper: Paper::Kraft,
                template: Template::Dotted,
                texture: true,
            },
        );
        let svg = note_to_svg(&n, &SvgOptions::default());
        assert_eq!(svg.matches("<image ").count(), 1);
        assert!(svg.contains("xlink:href=\"data:image/jpeg;base64,/9j/"));
        assert!(svg.matches("<circle ").count() > 50);
        assert_eq!(svg.matches("<path ").count(), 0);
    }
}
