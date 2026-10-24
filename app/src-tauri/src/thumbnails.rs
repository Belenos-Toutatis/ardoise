//! Note thumbnails for the library grid, and PNG data URLs.
//!
//! A thumbnail comes from the note file's `thumb.png` (written at every save), or is rendered
//! when the file has none. Either way it is kept in a cache on this PC
//! (`%LOCALAPPDATA%\Ardoise\thumbnails\`), never in the library (SPEC §3.1: the library holds
//! notes only, and Nextcloud would sync the cache to every computer). A cache entry is named
//! after the note's id and the file's stamp (size and modification time), so a note changed
//! anywhere gets a new entry; older entries of the same note are removed when a new one is
//! written. The cache is a convenience: when it cannot be read or written, thumbnails are
//! simply made again.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use ardoise_core::format;
use ardoise_core::fsutil::write_atomic;
use ardoise_core::library::{FileStamp, Library, LibraryError};
use ardoise_core::note::Note;
use tauri::State;

use crate::AppContext;
use crate::commands::CommandResult;
use crate::library::{library_error, library_of};

/// Thumbnail width in pixels (the grid shows them at most about 240 px wide, sharper on
/// high-density screens).
pub const THUMBNAIL_WIDTH: u32 = 320;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A PNG thumbnail of `note`, or `None` if it cannot be made (the note is saved anyway).
pub fn thumbnail_png(note: &Note) -> Option<Vec<u8>> {
    let options = ardoise_render::RenderOptions::fit_width(note.page, THUMBNAIL_WIDTH);
    let image = ardoise_render::render_note(note, &options).ok()?;
    ardoise_render::encode_png(&image).ok()
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(PNG_SIGNATURE)
}

/// Standard base64 (RFC 4648, with padding).
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextet = |shift: u32| char::from(ALPHABET[((n >> shift) & 0x3f) as usize]);
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// `data:image/png;base64,…`
pub fn png_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64(png))
}

/// The thumbnail cache of this PC.
#[derive(Debug, Clone)]
pub struct ThumbnailCache {
    /// `None` when there is no place for it: thumbnails are then made at every request.
    dir: Option<PathBuf>,
}

impl ThumbnailCache {
    /// `%LOCALAPPDATA%\Ardoise\thumbnails` (local to the PC, never synced).
    pub fn default_location() -> Self {
        let dir = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(|dir| PathBuf::from(dir).join("Ardoise").join("thumbnails"));
        ThumbnailCache { dir }
    }

    /// A cache in `dir` (created when first needed).
    #[cfg(test)]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        ThumbnailCache {
            dir: Some(dir.into()),
        }
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        let bytes = fs::read(self.dir.as_ref()?.join(name)).ok()?;
        is_png(&bytes).then_some(bytes)
    }

    /// Stores `png` as `name` and removes the other entries of the same note. Failures are
    /// ignored: the cache only saves time.
    fn store(&self, id_prefix: &str, name: &str, png: &[u8]) {
        let Some(dir) = &self.dir else { return };
        if fs::create_dir_all(dir).is_err() || write_atomic(&dir.join(name), png).is_err() {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(other) = file_name.to_str() else {
                continue;
            };
            if other != name && other.starts_with(id_prefix) && other.ends_with(".png") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// The part of a cache file name that identifies a note: its id, reduced to characters safe in
/// a file name (ids come from files, which may be hostile), then `_`.
fn id_prefix(id: &str) -> String {
    let safe: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(64)
        .collect();
    format!("{}_", if safe.is_empty() { "note" } else { &safe })
}

/// `<id>_<len>_<secs>_<nanos>.png`
fn cache_name(id: &str, stamp: &FileStamp) -> String {
    let since = stamp
        .modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{}{}_{}_{}.png",
        id_prefix(id),
        stamp.len,
        since.as_secs(),
        since.subsec_nanos()
    )
}

/// PNG thumbnail of the note at `path`: from the cache, else from the file's `thumb.png`, else
/// rendered. `None` when the note has no thumbnail and cannot be rendered. The cache is keyed
/// by the file's current stamp (read here), so a stale stamp from the interface still gets
/// the current picture.
pub fn thumbnail_of(
    library: &Library,
    cache: &ThumbnailCache,
    path: &Path,
) -> Result<Option<Vec<u8>>, LibraryError> {
    let path = library.note_path(path)?;
    let stamp = FileStamp::of(&path).map_err(|source| match source.kind() {
        io::ErrorKind::NotFound => LibraryError::NotFound { path: path.clone() },
        _ => LibraryError::Io {
            path: path.clone(),
            source,
        },
    })?;
    let format_error = |source| LibraryError::Format {
        path: path.clone(),
        source,
    };
    let header = format::read_header(&path).map_err(format_error)?;
    let prefix = id_prefix(&header.note.id);
    let name = cache_name(&header.note.id, &stamp);
    if let Some(png) = cache.read(&name) {
        return Ok(Some(png));
    }
    let png = match format::read_thumbnail(&path).map_err(format_error)? {
        Some(png) if is_png(&png) => png,
        _ => {
            let file = format::read_note(&path).map_err(format_error)?;
            match thumbnail_png(&file.note) {
                Some(png) => png,
                None => return Ok(None),
            }
        }
    };
    cache.store(&prefix, &name, &png);
    Ok(Some(png))
}

/// PNG data URL of a note's thumbnail, or null. `stamp` is the one the interface listed; the
/// file's current stamp is what keys the cache.
#[tauri::command]
pub async fn note_thumbnail(
    context: State<'_, AppContext>,
    cache: State<'_, ThumbnailCache>,
    path: String,
    stamp: FileStamp,
) -> CommandResult<Option<String>> {
    let _ = stamp;
    let library = library_of(&context)?;
    thumbnail_of(&library, &cache, Path::new(&path))
        .map(|png| png.as_deref().map(png_data_url))
        .map_err(library_error)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::library::tests::{library, sample_note, write_sample};
    use ardoise_core::format::NoteFile;

    /// Decodes standard base64 (test helper).
    pub(crate) fn decode_base64(text: &str) -> Vec<u8> {
        let value = |c: u8| match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {c}"),
        };
        let mut out = Vec::new();
        for chunk in text.as_bytes().chunks(4) {
            let digits: Vec<u8> = chunk
                .iter()
                .filter(|&&c| c != b'=')
                .map(|&c| value(c))
                .collect();
            let n = digits
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, d)| n | (u32::from(*d) << (18 - 6 * i)));
            for i in 0..digits.len().saturating_sub(1) {
                out.push((n >> (16 - 8 * i)) as u8);
            }
        }
        out
    }

    /// PNG bytes of a data URL (test helper).
    pub(crate) fn png_of(url: &str) -> Vec<u8> {
        let encoded = url
            .strip_prefix("data:image/png;base64,")
            .expect("a PNG data URL");
        decode_base64(encoded)
    }

    #[test]
    fn base64_matches_rfc_4648() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
            assert_eq!(decode_base64(encoded), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode_base64(&base64(&all)), all);
        assert!(png_data_url(b"x").starts_with("data:image/png;base64,"));
    }

    fn cache_files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn thumbnails_are_cached_by_id_and_stamp_and_stale_entries_pruned() {
        let (_dir, library) = library();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = ThumbnailCache::at(cache_dir.path().join("thumbnails"));
        let saved = write_sample(&library, "", "Essai");

        // Miss: taken from the file's thumb.png and cached.
        let first = thumbnail_of(&library, &cache, &saved.path)
            .unwrap()
            .unwrap();
        let stored = library.read(&saved.path).unwrap().file.thumbnail.unwrap();
        assert_eq!(first, stored);
        let files = cache_files(&cache_dir.path().join("thumbnails"));
        assert_eq!(files.len(), 1);
        let id = library.read(&saved.path).unwrap().file.note.id;
        assert!(files[0].starts_with(&format!("{id}_")), "{files:?}");

        // Hit: served from the cache, not from the file.
        let marker = [PNG_SIGNATURE, b"cached"].concat();
        fs::write(cache_dir.path().join("thumbnails").join(&files[0]), &marker).unwrap();
        assert_eq!(
            thumbnail_of(&library, &cache, &saved.path).unwrap(),
            Some(marker.clone())
        );

        // The note changes: a new entry replaces the old one.
        library
            .set_tags(&saved.path, &["a".to_owned()], None)
            .unwrap();
        let after = thumbnail_of(&library, &cache, &saved.path)
            .unwrap()
            .unwrap();
        assert_ne!(after, marker);
        let files_after = cache_files(&cache_dir.path().join("thumbnails"));
        assert_eq!(files_after.len(), 1);
        assert_ne!(files_after, files);
    }

    #[test]
    fn a_note_without_thumbnail_is_rendered() {
        let (_dir, library) = library();
        let file = NoteFile::new(sample_note("Sans vignette"));
        let saved = library.create(Path::new(""), &file).unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let png = thumbnail_of(&library, &ThumbnailCache::at(cache_dir.path()), &saved.path)
            .unwrap()
            .unwrap();
        let image = ardoise_render::decode_png(&png).unwrap();
        assert_eq!(image.width, THUMBNAIL_WIDTH);
    }

    #[test]
    fn an_unwritable_cache_only_costs_time() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let blocker = tempfile::NamedTempFile::new().unwrap();
        // The "cache folder" is a file: nothing can be stored under it.
        let cache = ThumbnailCache::at(blocker.path().join("thumbnails"));
        assert!(
            thumbnail_of(&library, &cache, &saved.path)
                .unwrap()
                .is_some()
        );
        let none = ThumbnailCache { dir: None };
        assert!(
            thumbnail_of(&library, &none, &saved.path)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn thumbnails_only_for_notes_of_the_library() {
        let (_dir, library) = library();
        let cache = ThumbnailCache { dir: None };
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("2026-10-24 Ailleurs.ardoise");
        format::write_note(&path, &NoteFile::new(sample_note("Ailleurs"))).unwrap();
        let error = thumbnail_of(&library, &cache, &path).unwrap_err();
        assert_eq!(error.code(), "notInLibrary");
        let missing = library.root().join("2026-10-24 Absente.ardoise");
        let error = thumbnail_of(&library, &cache, &missing).unwrap_err();
        assert_eq!(error.code(), "noteNotFound");
    }

    #[test]
    fn cache_names_never_escape_the_cache_folder() {
        assert_eq!(id_prefix("../../evil"), "evil_");
        assert_eq!(id_prefix(""), "note_");
        assert_eq!(id_prefix("ab_cd"), "abcd_");
    }
}
