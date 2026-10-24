//! File and folder names: titles made valid for Windows, note file names, free names.

use std::path::{Path, PathBuf};

/// Extension of note files, without the dot.
pub const NOTE_EXTENSION: &str = "ardoise";
/// Longest title part of a file name, or folder name made by [`sanitize_name`], in characters.
/// Keeps full paths well under the 260 characters many Windows programs still expect.
pub const MAX_NAME_CHARS: usize = 80;
/// Largest ` (n)` suffix tried when looking for a free name.
pub(crate) const MAX_SUFFIX: u32 = 9999;

/// Characters Windows refuses in file and folder names.
const FORBIDDEN: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Device names Windows reserves, with or without an extension (`CON`, `con.txt`…).
fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end();
    let upper = stem.to_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let mut chars = upper.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    let rest: Vec<char> = chars.collect();
    (prefix == "COM" || prefix == "LPT")
        && rest.len() == 1
        && (rest[0].is_ascii_digit() || matches!(rest[0], '¹' | '²' | '³'))
}

// Names Nextcloud's desktop client never synchronises: the patterns of its default
// `sync-exclude.lst` (https://github.com/nextcloud/desktop/blob/master/sync-exclude.lst, as of
// October 2026, plus `#*#` from older releases) and its built-in rules
// (`src/csync/csync_exclude.cpp`: `desktop.ini`, names over 254 UTF-16 units, and conflict
// files, which are not uploaded unless the server asks for them). Such a file stays on one
// computer without any warning. Only patterns a visible name can match are listed: names
// starting with a dot (`.~lock.*`, `._*`, `.DS_Store`, `.nfs*`…) or holding a control
// character (`Icon\r`) are refused anyway, and so are names ending with a dot
// (`My Saved Places.`). On Windows the client compares names ignoring case and lets `*` match
// any character.

/// `*.ext` patterns, plus `*~` (which also covers `*~lock~`).
const SYNC_EXCLUDED_ENDINGS: [&str; 11] = [
    "~",
    ".part",
    ".filepart",
    ".crdownload",
    ".kate-swp",
    ".unison",
    ".idlk",
    ".prlock",
    ".dwl",
    ".dwl2",
    ".textclipping",
];

/// `*text*` patterns and conflict markers, with the position of the character the sanitising
/// replaces by `-` to break them.
const SYNC_EXCLUDED_MARKERS: [(&str, usize); 6] = [
    (".~", 0),
    (".gnucash.tmp-", 0),
    (".sb-", 0),
    ("_conflict-", 0),
    ("(conflicted copy", 11),
    ("(case clash from", 5),
];

/// Whole names.
const SYNC_EXCLUDED_NAMES: [&str; 4] = [
    "thumbs.db",
    "photothumb.db",
    "desktop.ini",
    "system volume information",
];

/// Folds a character as Nextcloud's case-insensitive matching does, as far as the (ASCII)
/// patterns are concerned: the Kelvin sign and the long s match `k` and `s`.
fn fold(c: char) -> char {
    match c {
        '\u{212A}' => 'k',
        '\u{17F}' => 's',
        c => c.to_ascii_lowercase(),
    }
}

/// Whether Nextcloud would never synchronise a file or folder called `name`.
pub(crate) fn sync_excluded(name: &str) -> bool {
    let folded: String = name.chars().map(fold).collect();
    SYNC_EXCLUDED_ENDINGS.iter().any(|e| folded.ends_with(e))
        || SYNC_EXCLUDED_MARKERS
            .iter()
            .any(|(marker, _)| folded.contains(marker))
        || SYNC_EXCLUDED_NAMES.contains(&folded.as_str())
        || folded.starts_with("~$")
        || (folded.starts_with('~') && folded.ends_with(".tmp"))
        || (folded.len() >= 2 && folded.starts_with('#') && folded.ends_with('#'))
}

/// Inserts `_` after the part of `name` before its first dot (`aux.txt` → `aux_.txt`).
fn mark_stem(name: &str) -> String {
    let stem_end = name.find('.').unwrap_or(name.len());
    let (stem, rest) = name.split_at(stem_end);
    format!("{}_{rest}", stem.trim_end())
}

/// One round of fixes making `name` acceptable (see [`sanitize_name`]). `name` holds no
/// character [`fold`] maps to another letter, so its ASCII-lowercased form has the same
/// characters at the same places.
fn fix_once(name: &str) -> String {
    let mut chars: Vec<char> = name.chars().collect();
    for (marker, at) in SYNC_EXCLUDED_MARKERS {
        let marker: Vec<char> = marker.chars().collect();
        let mut start = 0;
        while start + marker.len() <= chars.len() {
            let window = &chars[start..start + marker.len()];
            if window
                .iter()
                .zip(&marker)
                .all(|(c, m)| c.to_ascii_lowercase() == *m)
            {
                chars[start + at] = '-';
            }
            start += 1;
        }
    }
    let mut name: String = chars.into_iter().collect();
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("~$") || (lower.starts_with('~') && lower.ends_with(".tmp")) {
        name = name.trim_start_matches('~').to_owned();
    }
    name = name.trim_end_matches('~').to_owned();
    let lower = name.to_ascii_lowercase();
    if let Some(ending) = SYNC_EXCLUDED_ENDINGS
        .iter()
        .find(|e| e.starts_with('.') && lower.ends_with(*e))
    {
        // `Plan.part` → `Plan_part`.
        name.replace_range(name.len() - ending.len()..=name.len() - ending.len(), "_");
    }
    if name.len() >= 2 && name.starts_with('#') && name.ends_with('#') {
        name.pop();
    }
    let trimmed = name.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if is_reserved(trimmed)
        || SYNC_EXCLUDED_NAMES.contains(&trimmed.to_ascii_lowercase().as_str())
        || trimmed.eq_ignore_ascii_case(super::META_DIR)
    {
        // `aux.txt` is reserved as much as `aux`: the mark goes right after the device name.
        mark_stem(trimmed)
    } else {
        trimmed.to_owned()
    }
}

/// Turns any text into a valid Windows file or folder name (possibly empty): forbidden
/// characters replaced (`/ \ | :` by `-`, `"` by `'`) or dropped (`< > ? *`), control
/// characters and runs of spaces turned into one space, leading and trailing dots and spaces
/// removed, at most [`MAX_NAME_CHARS`] characters, `_` appended to reserved device names.
/// Names Nextcloud would never synchronise are changed as little as possible: trailing `~`
/// and the `~` of a leading `~$` dropped, the dot of an excluded extension turned into `_`
/// (`Plan.part` → `Plan_part`), `.~`, `.sb-`, `.gnucash.tmp-` and conflict markers broken with
/// a `-`, `_` after the stem of `Thumbs.db`, `desktop.ini`…, the last `#` of `#…#` dropped.
/// The result also never makes a note file name (`date title.ardoise`) Nextcloud excludes.
pub fn sanitize_name(text: &str) -> String {
    let replaced: String = text
        .chars()
        .filter_map(|c| match c {
            '/' | '\\' | '|' | ':' => Some('-'),
            '"' => Some('\''),
            '<' | '>' | '?' | '*' => None,
            // Letters Nextcloud matches as `K` and `s`: spelled so, the fixes below see them.
            '\u{212A}' => Some('K'),
            '\u{17F}' => Some('s'),
            c if c.is_control() => Some(' '),
            c => Some(c),
        })
        .collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name: String = collapsed.chars().take(MAX_NAME_CHARS).collect();
    // Every round removes characters, turns a `.`, `_` or space into `_` or `-` (never the
    // other way), or marks a reserved name for good: this ends after a few rounds; the bound
    // is only a safety net.
    for _ in 0..=name.len() * 2 + 2 {
        let fixed = fix_once(&name);
        if fixed == name {
            break;
        }
        name = fixed;
    }
    name
}

/// Why `name` cannot be a folder or file name in the library, if it cannot.
pub fn invalid_name_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        Some("the name is empty")
    } else if name.encode_utf16().count() > 254 {
        // Nextcloud's limit; NTFS accepts 255.
        Some("the name is too long")
    } else if name
        .chars()
        .any(|c| FORBIDDEN.contains(&c) || c.is_control())
    {
        Some("the name contains a character Windows forbids")
    } else if name.starts_with('.') {
        Some("names starting with a dot are hidden")
    } else if name.ends_with('.') || name.ends_with(' ') {
        Some("Windows forbids names ending with a dot or a space")
    } else if name.eq_ignore_ascii_case(super::META_DIR) {
        Some("the name is reserved for Ardoise's own folder")
    } else if is_reserved(name) {
        Some("the name is reserved by Windows")
    } else if sync_excluded(name) {
        Some("Nextcloud never synchronises files or folders named like this")
    } else {
        None
    }
}

/// `YYYY-MM-DD` of a Unix time in milliseconds, in the computer's time zone.
pub fn local_date(unix_ms: i64) -> Option<String> {
    let utc = chrono::DateTime::from_timestamp_millis(unix_ms)?;
    Some(
        utc.with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string(),
    )
}

/// File name of a note without extension: `2026-10-24 Title`, from its creation date
/// (local time) and its title made valid for Windows. An empty title gives the date alone.
pub fn note_file_stem(created_ms: i64, title: &str) -> String {
    let title = sanitize_name(title);
    match (local_date(created_ms), title.is_empty()) {
        (Some(date), true) => date,
        (Some(date), false) => format!("{date} {title}"),
        (None, false) => title,
        (None, true) => "Note".to_owned(),
    }
}

/// `stem`, then `stem (2)`, `stem (3)`…
pub(crate) fn with_suffix(stem: &str, n: u32) -> String {
    if n <= 1 {
        stem.to_owned()
    } else {
        format!("{stem} ({n})")
    }
}

/// Whether `current` is `desired` or `desired (n)`.
pub(crate) fn stem_matches(current: &str, desired: &str) -> bool {
    current == desired
        || current
            .strip_prefix(desired)
            .and_then(|rest| rest.strip_prefix(" ("))
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|n| n.parse::<u32>().is_ok_and(|n| n >= 2))
}

/// Whether anything (file, folder, link) exists at `path`.
pub(crate) fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

/// Path of a note file named `stem.ardoise` in `dir`.
pub(crate) fn note_path(dir: &Path, stem: &str) -> PathBuf {
    dir.join(format!("{stem}.{NOTE_EXTENSION}"))
}

/// Whether a file name is a note's: ends with `.ardoise` (any case), not hidden.
pub(crate) fn is_note_file_name(name: &str) -> bool {
    !name.starts_with('.')
        && name
            .rsplit_once('.')
            .is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case(NOTE_EXTENSION))
}

/// File name without its `.ardoise` extension.
pub(crate) fn note_stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case(NOTE_EXTENSION) => stem,
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_forbidden_characters() {
        assert_eq!(sanitize_name("Cours 1/2"), "Cours 1-2");
        assert_eq!(sanitize_name(r"a\b|c:d"), "a-b-c-d");
        assert_eq!(sanitize_name("Pourquoi ?"), "Pourquoi");
        assert_eq!(sanitize_name("Il a dit \"oui\""), "Il a dit 'oui'");
        assert_eq!(sanitize_name("<b>gras</b>*"), "bgras-b");
        assert_eq!(sanitize_name("ligne 1\nligne 2\t!"), "ligne 1 ligne 2 !");
        assert_eq!(
            sanitize_name("  beaucoup   d'espaces  "),
            "beaucoup d'espaces"
        );
        assert_eq!(
            sanitize_name("Réunion « budget » été"),
            "Réunion « budget » été"
        );
    }

    #[test]
    fn sanitize_trims_dots_and_spaces() {
        assert_eq!(sanitize_name("fin..."), "fin");
        assert_eq!(sanitize_name("...caché"), "caché");
        assert_eq!(sanitize_name(" . "), "");
        assert_eq!(sanitize_name("???"), "");
        assert_eq!(sanitize_name(""), "");
    }

    #[test]
    fn sanitize_avoids_reserved_names() {
        for (name, expected) in [
            ("CON", "CON_"),
            ("con", "con_"),
            ("Nul", "Nul_"),
            ("COM1", "COM1_"),
            ("lpt9", "lpt9_"),
            ("COM²", "COM²_"),
            ("aux.txt", "aux_.txt"),
            ("PRN ", "PRN_"),
            ("nul .x", "nul_.x"),
        ] {
            let sanitized = sanitize_name(name);
            assert_eq!(sanitized, expected);
            assert!(
                invalid_name_reason(&sanitized).is_none(),
                "{name} -> {sanitized}"
            );
        }
        for name in ["CONSOLE", "COM10", "LPT", "auxiliaire", "nul part"] {
            assert_eq!(sanitize_name(name), name);
        }
    }

    #[test]
    fn sanitize_limits_length_on_character_boundaries() {
        let long = "é".repeat(200);
        let sanitized = sanitize_name(&long);
        assert_eq!(sanitized.chars().count(), MAX_NAME_CHARS);
        // Truncation never leaves a trailing space or dot.
        let tricky = format!("{} .x", "a".repeat(MAX_NAME_CHARS - 1));
        assert_eq!(sanitize_name(&tricky), "a".repeat(MAX_NAME_CHARS - 1));
    }

    #[test]
    fn sanitized_names_are_always_valid_or_empty() {
        let mut rng = crate::testutil::Lcg::new(3);
        let alphabet: Vec<char> = "aZ09 .-_<>:\"/\\|?*\u{0}\u{1f}\té«»€😀COMLPTNUL"
            .chars()
            .collect();
        for _ in 0..2000 {
            let len = rng.below(30) as usize;
            let text: String = (0..len)
                .map(|_| alphabet[rng.below(alphabet.len() as u64) as usize])
                .collect();
            let name = sanitize_name(&text);
            if !name.is_empty() {
                assert_eq!(invalid_name_reason(&name), None, "{text:?} -> {name:?}");
            }
        }
        // Pieces of the names Nextcloud never synchronises, assembled at random.
        let tokens = [
            ".",
            "~",
            "$",
            "#",
            " ",
            "a",
            "part",
            "filepart",
            "sb-",
            "tmp",
            "db",
            "thumbs",
            "desktop",
            "ini",
            "con",
            "_conflict-",
            "(conflicted copy",
            "(case clash from",
            "gnucash",
            "idl",
            "\u{212A}",
            "\u{17F}",
            "dwl2",
            "kate-swp",
            "*",
            "/",
        ];
        for _ in 0..5000 {
            let count = rng.below(8) as usize;
            let text: String = (0..count)
                .map(|_| tokens[rng.below(tokens.len() as u64) as usize])
                .collect();
            let name = sanitize_name(&text);
            if !name.is_empty() {
                assert_eq!(invalid_name_reason(&name), None, "{text:?} -> {name:?}");
            }
            // Inside a note file name too.
            let file_name = format!("{}.{NOTE_EXTENSION}", note_file_stem(0, &text));
            assert!(!sync_excluded(&file_name), "{text:?} -> {file_name:?}");
        }
    }

    #[test]
    fn invalid_names_are_explained() {
        assert!(invalid_name_reason("").is_some());
        assert!(invalid_name_reason("a/b").is_some());
        assert!(invalid_name_reason(".ardoise").is_some());
        assert!(invalid_name_reason("_ardoise").is_some());
        assert!(invalid_name_reason("_ARDOISE").is_some());
        assert_ne!(sanitize_name("_ardoise").to_ascii_lowercase(), "_ardoise");
        assert!(invalid_name_reason("fin.").is_some());
        assert!(invalid_name_reason("fin ").is_some());
        assert!(invalid_name_reason("LPT1").is_some());
        assert!(invalid_name_reason(&"x".repeat(256)).is_some());
        assert_eq!(invalid_name_reason("Classe de 3e B"), None);
    }

    /// Names Nextcloud's desktop client never synchronises (`sync-exclude.lst`, built-in rules).
    const NEVER_SYNCED: [&str; 27] = [
        "Brouillon~",
        "Cours~lock~",
        "~$Budget",
        "~classe.tmp",
        "Cours.~1",
        "a.~",
        "Classe.part",
        "Classe.PART",
        "Export.filepart",
        "Image.crdownload",
        "x.kate-swp",
        "x.unison",
        "Plan.idlk",
        "Plan.prlock",
        "Plan.dwl",
        "Plan.dwl2",
        "Lien.textClipping",
        "a.gnucash.tmp-1",
        "a.sb-12-b",
        "#brouillon#",
        "Thumbs.db",
        "photothumb.db",
        "desktop.ini",
        "System Volume Information",
        "Cours (conflicted copy 2026-10-24 101530)",
        "Cours_conflict-20261024-101530",
        "Cours (case clash from 2026-10-24 101530)",
    ];

    #[test]
    fn names_nextcloud_never_syncs_are_refused() {
        for name in NEVER_SYNCED {
            assert!(invalid_name_reason(name).is_some(), "{name}");
        }
        assert!(invalid_name_reason(&"x".repeat(255)).is_some());
        assert_eq!(invalid_name_reason(&"x".repeat(254)), None);
        for fine in [
            "~ environ 3 km",
            "Cours ~ suite",
            "Notes.partiel",
            "a.sb",
            "#3e B",
            "C#",
            "Thumbs",
            "Partie 2 - copie",
            "Conflit de dates",
        ] {
            assert_eq!(invalid_name_reason(fine), None, "{fine}");
        }
    }

    #[test]
    fn sanitize_avoids_names_nextcloud_never_syncs() {
        for name in NEVER_SYNCED {
            let sanitized = sanitize_name(name);
            assert!(!sanitized.is_empty(), "{name}");
            assert_eq!(
                invalid_name_reason(&sanitized),
                None,
                "{name} -> {sanitized}"
            );
        }
        assert_eq!(sanitize_name("Brouillon~"), "Brouillon");
        assert_eq!(sanitize_name("~$Budget"), "$Budget");
        assert_eq!(sanitize_name("Cours.~1"), "Cours-~1");
        assert_eq!(sanitize_name("Classe.part"), "Classe_part");
        assert_eq!(sanitize_name("Thumbs.db"), "Thumbs_.db");
        assert_eq!(sanitize_name("#brouillon#"), "#brouillon");
        assert_eq!(sanitize_name("~ environ 3 km"), "~ environ 3 km");
        // Characters that Nextcloud's case-insensitive matching folds to ASCII letters.
        assert_eq!(sanitize_name("Plan.idl\u{212A}"), "Plan_idlK");
        assert_eq!(sanitize_name("a.\u{17F}b-1"), "a-sb-1");
        // Fixes that uncover another problem are applied until none is left.
        assert_eq!(sanitize_name("x.part~"), "x_part");
        assert_eq!(sanitize_name("a.~ ~"), "a-");
        assert_eq!(sanitize_name("CON~"), "CON_");
        assert_eq!(sanitize_name("~.tmp"), "tmp");
    }

    #[test]
    fn note_stems() {
        // Noon UTC is the same calendar day in every time zone from UTC-11 to UTC+11.
        let noon = 1_792_843_200_000;
        assert_eq!(
            note_file_stem(noon, "Titre de la note"),
            "2026-10-24 Titre de la note"
        );
        assert_eq!(note_file_stem(noon, "  "), "2026-10-24");
        assert_eq!(note_file_stem(noon, "a/b"), "2026-10-24 a-b");
        assert_eq!(note_file_stem(i64::MAX, "x"), "x");
        assert_eq!(note_file_stem(i64::MAX, ""), "Note");
    }

    #[test]
    fn suffixes() {
        assert_eq!(with_suffix("a", 1), "a");
        assert_eq!(with_suffix("a", 2), "a (2)");
        assert!(stem_matches("a", "a"));
        assert!(stem_matches("a (2)", "a"));
        assert!(stem_matches("a (17)", "a"));
        assert!(!stem_matches("a (1)", "a"));
        assert!(!stem_matches("a (x)", "a"));
        assert!(!stem_matches("ab", "a"));
        assert!(!stem_matches("A", "a"));
    }

    #[test]
    fn note_file_names() {
        assert!(is_note_file_name("2026-10-24 Titre.ardoise"));
        assert!(is_note_file_name("x.ARDOISE"));
        assert!(!is_note_file_name(".x.ardoise"));
        assert!(!is_note_file_name(".ardoise"));
        assert!(!is_note_file_name("x.ardoise.part"));
        assert!(!is_note_file_name("x.ardoise.123.0.part"));
        assert!(!is_note_file_name("x.pdf"));
        assert!(!is_note_file_name("ardoise"));
        assert_eq!(note_stem("a b.ardoise"), "a b");
        assert_eq!(note_stem("a.b.Ardoise"), "a.b");
        assert_eq!(note_stem("x.pdf"), "x.pdf");
    }
}
