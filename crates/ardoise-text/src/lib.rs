//! Text of the text frames (SPEC §4.4, « Texte tapé au clavier »; design:
//! `docs/design/page-objects.md`), shared by the screen (compiled to WebAssembly in
//! `ardoise-ink-wasm`) and every export, so that both draw exactly the same glyphs:
//!
//! - [`FontFamily`]: the two families shipped with Ardoise, Latin Modern Roman (the font of LaTeX,
//!   GUST Font License) and Andika (SIL Open Font License), each in regular and bold. The four files
//!   are embedded unmodified ([`FONT_FILES`], `fonts/README.md`): nothing depends on the fonts
//!   installed on the PC.
//! - [`layout`]: one line per line of the text (only a line break starts a new line: no wrapping),
//!   glyphs placed one after the other by their advance widths alone, without kerning nor
//!   ligatures (the editor's text field is styled the same way, so that its caret falls between
//!   the glyphs drawn). A character missing from the family is taken from the other one.
//! - [`Path`]: the glyphs' outlines with their curves, in the frame's own coordinates (origin at
//!   the top-left corner of the text, y down, the units of the text's size), to fill with the
//!   non-zero rule.

mod fonts;
mod layout;
mod path;

pub use fonts::{FONT_FILES, FontFamily, FontFile, font_file};
pub use layout::{
    LINE_HEIGHT, Layout, Line, PlacedGlyph, TAB_SPACES, TextStyle, layout, outline, text_outline,
};
pub use path::{Path, Segment};
