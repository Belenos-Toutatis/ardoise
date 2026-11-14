use super::*;
use crate::fonts::GREEK_BLOCK;

const LM: TextStyle = TextStyle {
    family: FontFamily::LatinModern,
    bold: false,
    size: 1000.0,
    wrap: None,
};

fn wrapped(text: &str, wrap: f32) -> Layout {
    layout(
        text,
        TextStyle {
            wrap: Some(wrap),
            ..LM
        },
    )
}

/// The text of each line (line breaks left out).
fn line_texts(text: &str, laid: &Layout) -> Vec<String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    laid.lines
        .iter()
        .map(|l| String::from_utf16_lossy(&units[l.start..l.end]))
        .collect()
}

#[test]
fn lines_break_only_at_line_breaks_without_a_wrap_width() {
    let laid = layout("Bonjour\nle monde\r\n\rfin", LM);
    assert_eq!(laid.lines.len(), 4);
    assert_eq!(
        laid.lines
            .iter()
            .map(|l| (l.start, l.end, l.wrapped))
            .collect::<Vec<_>>(),
        [
            (0, 7, false),
            (8, 16, false),
            (18, 18, false),
            (19, 22, false)
        ]
    );
    assert_eq!(laid.height, 4.0 * 1250.0);
    assert_eq!(laid.lines[2].glyphs.len(), 0);
    assert_eq!(laid.lines[1].top, 1250.0);
    assert!(laid.lines[0].baseline > 0.0 && laid.lines[0].baseline < 1250.0);
    assert_eq!(laid.lines[1].baseline - laid.lines[0].baseline, 1250.0);
    let widest = laid.lines.iter().map(|l| l.width).fold(0.0, f32::max);
    assert_eq!(laid.width, widest);
    // A very long line stays one line.
    let long = "mot ".repeat(200);
    assert_eq!(layout(&long, LM).lines.len(), 1);
}

#[test]
fn glyphs_follow_each_other_by_their_advances_without_kerning() {
    let laid = layout("AVAV", LM);
    let glyphs = &laid.lines[0].glyphs;
    assert_eq!(glyphs.len(), 4);
    for pair in glyphs.windows(2) {
        assert_eq!(pair[1].x, pair[0].x + pair[0].advance);
    }
    // Twice the same pair: no kerning makes them differ.
    assert_eq!(glyphs[2].x, 2.0 * glyphs[1].x - glyphs[0].x);
    // Double the size, double every distance.
    let twice = layout("AVAV", TextStyle { size: 2000.0, ..LM });
    assert!((twice.width - 2.0 * laid.width).abs() < 0.01);
}

#[test]
fn utf16_offsets_count_surrogate_pairs() {
    // U+1D400 (mathematical bold A) takes two UTF-16 units.
    let laid = layout("a\u{1D400}b", LM);
    let indices: Vec<usize> = laid.lines[0].glyphs.iter().map(|g| g.index).collect();
    assert_eq!(indices, [0, 1, 3]);
    assert_eq!(laid.lines[0].end, 4);
}

#[test]
fn tabs_go_to_the_next_stop_and_invisible_characters_vanish() {
    let space = layout(" ", LM).width;
    let laid = layout("a\tb\u{7}c\u{200B}d\u{AD}e\u{FE0F}f", LM);
    let glyphs = &laid.lines[0].glyphs;
    assert_eq!(glyphs.len(), 6, "{glyphs:?}");
    assert!((glyphs[1].x - 4.0 * space).abs() < 0.01, "{glyphs:?}");
    assert_eq!(glyphs[2].x, glyphs[1].x + glyphs[1].advance);
    assert_eq!(glyphs[2].index, 4);
    // The zero width space, the soft hyphen and the variation selector take no room.
    for pair in glyphs[2..].windows(2) {
        assert_eq!(pair[1].x, pair[0].x + pair[0].advance);
    }
    assert!(glyphs.iter().all(|g| g.look == GlyphLook::Plain));
}

#[test]
fn empty_text_is_one_empty_line() {
    let laid = layout("", LM);
    assert_eq!(laid.lines.len(), 1);
    assert_eq!((laid.width, laid.height), (0.0, 1250.0));
    assert!(outline(&laid).is_empty());
    for size in [f32::NAN, -5.0, 0.0] {
        let laid = layout("abc", TextStyle { size, ..LM });
        assert_eq!(laid.width, 0.0);
    }
    let laid = wrapped("", 3000.0);
    assert_eq!((laid.lines.len(), laid.width), (1, 0.0));
}

#[test]
fn outlines_lie_within_the_lines() {
    for family in FontFamily::ALL {
        for bold in [false, true] {
            let style = TextStyle {
                family,
                bold,
                size: 600.0,
                wrap: None,
            };
            let (laid, path) = text_outline("Élève ça\nœuvre « 12 € » αβ", style);
            assert!(!path.is_empty());
            let [x0, y0, x1, y1] = path.bounds().unwrap();
            assert!(
                x0 >= -60.0 && x1 <= laid.width + 60.0,
                "{family:?} {x0} {x1}"
            );
            assert!(
                // β descends a little below the last line's box.
                y0 >= -0.5 * laid.line_height && y1 <= laid.height + 0.25 * laid.line_height,
                "{family:?} {y0} {y1}"
            );
            // The second line is below the first.
            assert!(y1 > laid.lines[1].top);
        }
    }
}

#[test]
fn bold_is_wider_and_the_families_differ() {
    let text = "Les élèves";
    let regular = layout(text, LM).width;
    let bold = layout(text, TextStyle { bold: true, ..LM }).width;
    let andika = layout(
        text,
        TextStyle {
            family: FontFamily::Andika,
            ..LM
        },
    )
    .width;
    assert!(bold > regular, "{bold} {regular}");
    assert!((andika - regular).abs() > 1.0);
}

#[test]
fn the_offset_under_a_point_is_the_nearest_gap() {
    let laid = layout("ab\ncd", LM);
    let a = laid.lines[0].glyphs[0];
    assert_eq!(laid.offset_at(-10.0, 10.0), 0);
    assert_eq!(laid.offset_at(a.advance * 0.4, 10.0), 0);
    assert_eq!(laid.offset_at(a.advance * 0.6, 10.0), 1);
    assert_eq!(laid.offset_at(1.0e6, 10.0), 2);
    assert_eq!(laid.offset_at(0.0, 1300.0), 3);
    assert_eq!(laid.offset_at(1.0e6, 1.0e6), 5);
    assert_eq!(laid.offset_at(0.0, -100.0), 0);
}

// --- Wrapping (format 1.7) ---------------------------------------------------------------

#[test]
fn a_frame_wraps_between_words_and_the_spaces_hang() {
    let text = "Les élèves de troisième mesurent la tension";
    let one = layout(text, LM);
    // Half the line: two or three lines.
    let laid = wrapped(text, one.width / 2.0);
    assert!(laid.lines.len() >= 2, "{:?}", line_texts(text, &laid));
    let texts = line_texts(text, &laid);
    // Every line but the last ends with the space before the next word, and the lines put
    // together are the text.
    assert_eq!(texts.concat(), text);
    for line in &texts[..texts.len() - 1] {
        assert!(line.ends_with(' '), "{texts:?}");
        assert!(!line.starts_with(' '), "{texts:?}");
    }
    for (k, line) in laid.lines.iter().enumerate() {
        let last = k == laid.lines.len() - 1;
        assert_eq!(line.wrapped, !last);
        if !last {
            assert_eq!(line.end, laid.lines[k + 1].start);
        }
        // The first glyph of each line is at its left edge.
        assert_eq!(line.glyphs[0].x, 0.0);
        // The ink of each line fits in the width.
        let ink = line
            .glyphs
            .iter()
            .rev()
            .find(|g| texts[k].encode_utf16().nth(g.index - line.start) != Some(32))
            .map_or(0.0, |g| g.x + g.advance);
        assert!(ink <= one.width / 2.0, "{k} {ink}");
    }
    assert!(laid.width <= one.width / 2.0);
    assert_eq!(laid.height, laid.lines.len() as f32 * laid.line_height);
    // Greedy: the first word of each line would not have fitted on the line before.
    for k in 1..laid.lines.len() {
        let before = wrapped(&format!("{}{}", texts[k - 1], texts[k]), one.width / 2.0);
        assert!(before.lines.len() > 1);
    }
}

#[test]
fn a_word_longer_than_the_width_breaks_where_it_crosses_it() {
    let text = "anticonstitutionnellement";
    let full = layout(text, LM).width;
    let laid = wrapped(text, full / 3.0);
    assert!(laid.lines.len() >= 3, "{:?}", line_texts(text, &laid));
    assert_eq!(line_texts(text, &laid).concat(), text);
    for line in &laid.lines {
        assert!(!line.glyphs.is_empty());
        assert!(line.width <= full / 3.0 + 0.01);
    }
    // A width narrower than one letter: one letter per line, never an empty line.
    let laid = wrapped("abc", 1.0);
    assert_eq!(laid.lines.len(), 3);
    assert!(laid.lines.iter().all(|l| l.glyphs.len() == 1));
    // A long word after a short one goes to its own line first.
    let laid = wrapped(&format!("a {text}"), full / 2.0);
    assert_eq!(line_texts(&format!("a {text}"), &laid)[0], "a ");
}

#[test]
fn spaces_never_make_a_line_and_do_not_widen_a_wrapped_frame() {
    let word = layout("mot", LM).width;
    let laid = wrapped(&format!("mot{}", " ".repeat(40)), word * 1.5);
    assert_eq!(laid.lines.len(), 1);
    assert!(laid.lines[0].width > word * 1.5);
    assert_eq!(laid.width, word * 1.5);
    // A word after them goes to the next line; the spaces stay on the first.
    let text = format!("mot{}mot", " ".repeat(40));
    let laid = wrapped(&text, word * 1.5);
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(laid.lines[1].start, 43);
    assert_eq!(laid.width, word * 1.5);
}

#[test]
fn no_break_spaces_and_hyphens_do_not_break() {
    let one = layout("12\u{A0}km porte-monnaie", LM);
    let laid = wrapped("12\u{A0}km porte-monnaie", one.width - 1.0);
    let texts = line_texts("12\u{A0}km porte-monnaie", &laid);
    assert_eq!(texts, ["12\u{A0}km ", "porte-monnaie"]);
    // The narrow no-break space before a French « ! » holds too.
    let text = "Bravo\u{202F}!";
    let laid = wrapped(text, layout(text, LM).width - 1.0);
    // No break opportunity: the word breaks only where it crosses the width.
    assert_eq!(line_texts(text, &laid).concat(), text);
    assert!(line_texts(text, &laid)[0].starts_with("Bravo\u{202F}"));
}

#[test]
fn line_breaks_start_paragraphs_and_each_wraps_on_its_own() {
    let text = "un deux trois\nquatre cinq six";
    let full = layout("un deux trois", LM).width;
    let laid = wrapped(text, full * 0.7);
    let texts = line_texts(text, &laid);
    assert_eq!(
        texts,
        ["un deux ", "trois", "quatre ", "cinq six"].map(String::from)
    );
    let flags: Vec<bool> = laid.lines.iter().map(|l| l.wrapped).collect();
    assert_eq!(flags, [true, false, true, false]);
    // The hard line break is not in any line.
    assert_eq!(laid.lines[1].end + 1, laid.lines[2].start);
}

#[test]
fn a_wide_wrap_width_lays_out_like_none() {
    let text = "Bonjour le monde\nfin";
    let none = layout(text, LM);
    let wide = wrapped(text, none.width + 1.0);
    assert_eq!(none.lines.len(), wide.lines.len());
    for (a, b) in none.lines.iter().zip(&wide.lines) {
        assert_eq!(a.glyphs, b.glyphs);
        assert_eq!((a.start, a.end), (b.start, b.end));
    }
    // Not a number, zero or negative: no wrapping.
    for w in [f32::NAN, 0.0, -10.0, f32::INFINITY] {
        assert_eq!(wrapped(text, w).lines, none.lines);
    }
}

#[test]
fn any_width_between_the_ink_and_the_wrap_width_lays_out_the_same() {
    // What the side handle relies on: a drag that starts at the frame's edge changes nothing.
    let text = "La lumière se propage en ligne droite dans un milieu homogène.";
    let w = layout(text, LM).width / 2.5;
    let laid = wrapped(text, w);
    for k in [laid.width, (laid.width + w) / 2.0, w] {
        let again = wrapped(text, k);
        assert_eq!(again.lines, laid.lines, "{k}");
    }
    // A line wrapped is placed as the same text alone would be: the same positions to the bit.
    for (line, text_of_line) in laid.lines.iter().zip(line_texts(text, &laid)) {
        let alone = layout(&text_of_line, LM);
        let xs: Vec<f32> = line.glyphs.iter().map(|g| g.x).collect();
        let alone_xs: Vec<f32> = alone.lines[0].glyphs.iter().map(|g| g.x).collect();
        assert_eq!(xs, alone_xs, "{text_of_line:?}");
    }
}

// --- Fallback faces, Greek and symbols -----------------------------------------------------

/// The Greek alphabet, the variants and the symbols a physics-chemistry teacher types.
pub(crate) const GREEK_AND_SYMBOLS: &str = concat!(
    "ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ",
    "αβγδεζηθικλμνξοπρςστυφχψω",
    "ϵϑϕϖϱ",
    "µΩ∆Δ°±×·≈≠≤≥→⇌∞√∑∫∂∝‰′″−←↔⇒⇔∇",
    "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻₀₁₂₃₄₅₆₇₈₉₊₋",
);

fn styles() -> Vec<TextStyle> {
    FontFamily::ALL
        .into_iter()
        .flat_map(|family| {
            [false, true].map(|bold| TextStyle {
                family,
                bold,
                size: 1000.0,
                wrap: None,
            })
        })
        .collect()
}

#[test]
fn the_faces_lack_what_latin_modern_math_and_andika_give() {
    // What each shipped face lacks of the list (so that a font replaced by another version is
    // noticed). Latin Modern Roman has the Greek capitals but no small Greek letter; Andika's
    // Greek is partial (and not used); Latin Modern Math has every letter and symbol but the
    // superscript and subscript digits, which Andika has.
    let lacks = |face_id: FontFace| -> String {
        let f = face(face_id).unwrap();
        GREEK_AND_SYMBOLS
            .chars()
            .filter(|c| f.glyph_index(*c).is_none_or(|g| g.0 == 0))
            .collect()
    };
    assert_eq!(
        lacks(FontFace::LatinModern),
        "αβγδεζηθικλμνξοπρςστυφχψωϵϑϕϖϱ∆≈≠≤≥⇌∑∫∂∝′″↔⇒⇔∇⁰⁴⁵⁶⁷⁸⁹⁺⁻₀₁₂₃₄₅₆₇₈₉₊₋"
    );
    assert_eq!(
        lacks(FontFace::LatinModern),
        lacks(FontFace::LatinModernBold)
    );
    assert_eq!(lacks(FontFace::LatinModernMath), "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻₀₁₂₃₄₅₆₇₈₉₊₋");
    assert_eq!(lacks(FontFace::Andika), lacks(FontFace::AndikaBold));
    assert_eq!(
        lacks(FontFace::Andika),
        "ΑΒΕΖΗΙΚΛΜΝΟΡΣΤΥΦΧεζηικνξοςτυψϵϕϖϱ⇌∝∇"
    );
}

#[test]
fn every_greek_letter_and_symbol_has_a_glyph_in_both_families_and_weights() {
    for style in styles() {
        let laid = layout(GREEK_AND_SYMBOLS, style);
        let glyphs = &laid.lines[0].glyphs;
        assert_eq!(glyphs.len(), GREEK_AND_SYMBOLS.chars().count(), "{style:?}");
        assert!(
            laid.missing_chars(GREEK_AND_SYMBOLS).is_empty(),
            "{style:?}"
        );
        for (glyph, c) in glyphs.iter().zip(GREEK_AND_SYMBOLS.chars()) {
            assert_ne!(glyph.look, GlyphLook::Missing, "{style:?} {c}");
            assert!(glyph.glyph != 0 && glyph.advance > 0.0, "{style:?} {c}");
            // Greek never comes from Andika.
            if GREEK_BLOCK.contains(&u32::from(c)) {
                assert!(
                    !matches!(glyph.face, FontFace::Andika | FontFace::AndikaBold),
                    "{style:?} {c}"
                );
            }
            // Bold frames embolden what comes from the face without bold.
            if glyph.face == FontFace::LatinModernMath && glyph.look != GlyphLook::Missing {
                assert_eq!(
                    glyph.look == GlyphLook::Emboldened,
                    style.bold,
                    "{style:?} {c}"
                );
            }
        }
        // Every glyph draws something.
        let path = outline(&laid);
        assert!(!path.is_empty());
    }
}

#[test]
fn small_greek_letters_of_latin_modern_frames_come_from_latin_modern_math() {
    let laid = layout("aα Δ", LM);
    let faces: Vec<FontFace> = laid.lines[0].glyphs.iter().map(|g| g.face).collect();
    assert_eq!(
        faces,
        [
            FontFace::LatinModern,
            FontFace::LatinModernMath,
            FontFace::LatinModern,
            FontFace::LatinModern
        ]
    );
    let andika = layout(
        "aαΣ",
        TextStyle {
            family: FontFamily::Andika,
            ..LM
        },
    );
    let faces: Vec<FontFace> = andika.lines[0].glyphs.iter().map(|g| g.face).collect();
    assert_eq!(
        faces,
        [
            FontFace::Andika,
            FontFace::LatinModernMath,
            FontFace::LatinModernMath
        ]
    );
}

#[test]
fn emboldened_glyphs_keep_their_advance_and_grow() {
    let regular = layout("α", LM);
    let bold = layout("α", TextStyle { bold: true, ..LM });
    let (r, b) = (regular.lines[0].glyphs[0], bold.lines[0].glyphs[0]);
    assert_eq!(
        (r.face, b.face),
        (FontFace::LatinModernMath, FontFace::LatinModernMath)
    );
    assert_eq!(r.advance, b.advance);
    assert_eq!((r.look, b.look), (GlyphLook::Plain, GlyphLook::Emboldened));
    let [rx0, ry0, rx1, ry1] = outline(&regular).bounds().unwrap();
    let [bx0, by0, bx1, by1] = outline(&bold).bounds().unwrap();
    let grow = EMBOLDEN * 1000.0;
    assert!((rx0 - bx0 - grow).abs() < grow, "{rx0} {bx0}");
    assert!((bx1 - rx1 - grow).abs() < grow, "{rx1} {bx1}");
    assert!(by0 < ry0 && by1 > ry1);
    // Same number of segments, the points moved.
    assert_eq!(
        outline(&regular).segments.len(),
        outline(&bold).segments.len()
    );
}

#[test]
fn embolden_moves_a_square_out_and_its_hole_in() {
    // A square with a square hole, in both turns.
    let square = |x0: f32, y0: f32, x1: f32, y1: f32, reverse: bool| {
        let mut points = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        if reverse {
            points.reverse();
        }
        let mut segments = vec![Segment::MoveTo(points[0])];
        segments.extend(points[1..].iter().map(|p| Segment::LineTo(*p)));
        segments.push(Segment::Close);
        segments
    };
    for turn in [false, true] {
        let mut path = Path::default();
        path.segments.extend(square(0.0, 0.0, 100.0, 100.0, turn));
        path.segments.extend(square(40.0, 40.0, 60.0, 60.0, !turn));
        let bold = embolden(&path, 5.0);
        let points: Vec<[f32; 2]> = bold
            .segments
            .iter()
            .filter_map(|s| match s {
                Segment::MoveTo(p) | Segment::LineTo(p) => Some(*p),
                _ => None,
            })
            .collect();
        assert_eq!(bold.bounds(), Some([-5.0, -5.0, 105.0, 105.0]), "{turn}");
        // The hole is now 45..55.
        for p in &points[4..] {
            assert!(
                (p[0] - 45.0).abs() < 1e-3 || (p[0] - 55.0).abs() < 1e-3,
                "{p:?}"
            );
        }
    }
}

#[test]
fn latin_modern_draws_the_scripts_it_lacks_from_its_own_digits_in_andikas_advance() {
    for bold in [false, true] {
        let style = TextStyle { bold, ..LM };
        let laid = layout("10⁴ H₂O ²", style);
        let glyphs = &laid.lines[0].glyphs;
        let own = FontFace::of(FontFamily::LatinModern, bold);
        let sup = glyphs[2];
        let sub = glyphs[5];
        let real = glyphs[8];
        assert_eq!((sup.face, sup.look), (own, GlyphLook::Superscript));
        assert_eq!((sub.face, sub.look), (own, GlyphLook::Subscript));
        // ² is Latin Modern's own.
        assert_eq!((real.face, real.look), (own, GlyphLook::Plain));
        // The advance is Andika's, the face the browser takes ⁴ from.
        let andika = face(FontFace::of(FontFamily::Andika, bold)).unwrap();
        let expected = advance(
            andika,
            andika.glyph_index('⁴').unwrap(),
            units_scale(andika, 1000.0),
        );
        assert_eq!(sup.advance, expected);
        // Drawn above and below the baseline, smaller than the digits.
        let alone = |text: &str| outline(&layout(text, style)).bounds().unwrap();
        let [_, four_top, _, four_bottom] = alone("4");
        let [_, sup_top, _, sup_bottom] = alone("⁴");
        let [_, sub_top, _, sub_bottom] = alone("₂");
        let [_, two_top, _, two_bottom] = alone("2");
        assert!(sup_bottom - sup_top < 0.8 * (four_bottom - four_top));
        assert!(sup_top < four_top && sup_bottom < four_bottom - 100.0);
        assert!(sub_bottom > two_bottom && sub_top > two_top + 100.0);
        // Like Latin Modern's own superscripts.
        let [_, own_top, _, own_bottom] = alone("³");
        assert!((sup_top - own_top).abs() < 30.0, "{sup_top} {own_top}");
        assert!(
            (sup_bottom - own_bottom).abs() < 30.0,
            "{sup_bottom} {own_bottom}"
        );
    }
    // Andika has them all: its own glyphs.
    let andika = layout(
        "10⁴ H₂O",
        TextStyle {
            family: FontFamily::Andika,
            ..LM
        },
    );
    assert!(
        andika.lines[0]
            .glyphs
            .iter()
            .all(|g| g.face == FontFace::Andika && g.look == GlyphLook::Plain)
    );
}

#[test]
fn a_character_no_face_has_is_a_white_square_and_is_reported() {
    let text = "a\u{1F600}b\u{4E2D}\u{1F600}";
    let laid = layout(text, LM);
    let glyphs = &laid.lines[0].glyphs;
    assert_eq!(glyphs.len(), 5);
    assert_eq!(glyphs[1].look, GlyphLook::Missing);
    assert_eq!(glyphs[1].face, FontFace::LatinModernMath);
    assert!(glyphs[1].advance > 0.0);
    assert_eq!(glyphs[2].index, 3);
    let math = face(FontFace::LatinModernMath).unwrap();
    assert_eq!(glyphs[1].glyph, math.glyph_index(MISSING_CHAR).unwrap().0);
    assert_eq!(laid.missing_chars(text), ['\u{1F600}', '\u{4E2D}']);
    assert!(layout("abc", LM).missing_chars("abc").is_empty());
    // It draws (a box, not nothing as Latin Modern's empty .notdef would).
    let alone = outline(&layout("\u{1F600}", LM));
    assert!(!alone.is_empty());
}
