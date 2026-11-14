//! Text of the text frames (SPEC §4.4, « Texte tapé au clavier »; design:
//! `docs/design/page-objects.md`), shared by the screen (compiled to WebAssembly in
//! `ardoise-ink-wasm`) and every export, so that both draw exactly the same glyphs:
//!
//! - [`FontFamily`]: the two families shipped with Ardoise, Latin Modern Roman (the font of LaTeX,
//!   GUST Font License) and Andika (SIL Open Font License), each in regular and bold, and Latin
//!   Modern Math (GUST Font License), where both take Greek and the mathematical symbols from. The
//!   five files are embedded unmodified ([`FONT_FILES`], `fonts/README.md`): nothing depends on the
//!   fonts installed on the PC.
//! - [`layout`]: the lines of a frame: a line break starts one, and a frame with a wrap width
//!   (format 1.7) wraps between words; glyphs placed one after the other by their advance widths
//!   alone, without kerning nor ligatures (the editor's text field is styled the same way, so
//!   that its caret falls between the glyphs drawn). A character missing from the family is taken
//!   from the next face of [`fallback_chain`]; one that no face has is a white square, reported.
//! - [`Path`]: the glyphs' outlines with their curves, in the frame's own coordinates (origin at
//!   the top-left corner of the text, y down, the units of the text's size), to fill with the
//!   non-zero rule. Also the drawing of an equation (page objects of kind `equation`), read from
//!   its stored path data by [`Path::from_svg`].

mod fonts;
mod layout;
mod path;

pub use fonts::{
    FONT_FILES, FontFace, FontFamily, FontFile, GREEK_BLOCK, fallback_chain, font_file,
};
pub use layout::{
    EMBOLDEN, GlyphLook, LINE_HEIGHT, Layout, Line, MISSING_CHAR, PlacedGlyph, TAB_SPACES,
    TextStyle, embolden, is_break_space, layout, outline, text_outline,
};
pub use path::{Path, PathDataError, Segment};
