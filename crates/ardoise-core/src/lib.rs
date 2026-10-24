//! Notes, .ardoise file format, library, portable settings and note operations.
//!
//! - [`note`]: the note model (fixed contract).
//! - [`format`]: the `.ardoise` file (ZIP with `note.json`, `strokes.bin`…).
//! - [`library`]: the user's folder of notes: listing, binders, tags, trash, conflict copies,
//!   folder watching.
//! - [`fsutil`]: atomic writes and no-clobber renames.
//! - [`ops`]: split and merge notes (pure functions on [`note::Note`]).
//! - [`settings`]: portable `ardoise.toml`.

pub mod format;
pub mod fsutil;
pub mod library;
pub mod note;
pub mod ops;
pub mod settings;

#[cfg(test)]
mod testutil;
