//! Conflict copies made by sync clients when a note changed on two computers at once.
//!
//! The Nextcloud desktop client (and ownCloud, which it derives from) keeps the server's
//! version under the original name and renames the local one (source:
//! `src/common/utility.cpp` of github.com/nextcloud/desktop, `makeConflictFileName`,
//! `makeCaseClashConflictFileName`, `conflictFileBaseNameFromPattern`). The tag goes just
//! before the extension and is never translated:
//!
//! - `Note (conflicted copy 2026-10-24 101530).ardoise` (client 2.5 and later), optionally
//!   with the user name: `Note (conflicted copy camille 2026-10-24 101530).ardoise`;
//! - `Note_conflict-20261024-101530.ardoise` (clients before 2.5);
//! - `Note (case clash from 2026-10-24 101530).ardoise` (client 3.8 and later, two names that
//!   differ only by case).
//!
//! Other sync tools (or other clients of the same server) use translated tags, such as
//! `Note (copie en conflit de Camille 2026-10-24).ardoise` or
//! `Note (conflict 2026-10-24 101530).ardoise`. They are recognised too when the last
//! parenthesised group before the extension holds a conflict word and a date, so that a title
//! like `Gestion de conflit (cours 2)` is never mistaken for one.

use serde::Serialize;

/// Which naming scheme produced a conflict copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    /// `… (conflicted copy [user ]YYYY-MM-DD HHMMSS)`: Nextcloud desktop 2.5+.
    Nextcloud,
    /// `…_conflict-YYYYMMDD-HHMMSS`: Nextcloud/ownCloud desktop before 2.5.
    NextcloudLegacy,
    /// `… (case clash from YYYY-MM-DD HHMMSS)`: Nextcloud desktop 3.8+.
    CaseClash,
    /// A parenthesised conflict word and date in another wording or language.
    Other,
}

/// What a conflict-copy file name says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictName {
    /// File name of the note it is a copy of, in the same folder.
    pub original_name: String,
    pub kind: ConflictKind,
    /// The tag as found in the name, e.g. `(conflicted copy 2026-10-24 101530)`.
    pub marker: String,
}

/// Words that mark a conflict in the wordings we know (lowercase): English, French
/// ("conflit"), Spanish/Italian/Portuguese ("conflicto", "conflitto", "conflito" contain
/// one of these), German ("Konflikt").
const CONFLICT_WORDS: [&str; 3] = ["conflict", "conflit", "konflikt"];

fn digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

/// `YYYY-MM-DD`.
fn is_date(bytes: &[u8]) -> bool {
    bytes.len() == 10
        && digits(&bytes[0..4])
        && bytes[4] == b'-'
        && digits(&bytes[5..7])
        && bytes[7] == b'-'
        && digits(&bytes[8..10])
}

/// `YYYY-MM-DD HHMMSS`.
fn is_date_time(bytes: &[u8]) -> bool {
    bytes.len() == 17 && is_date(&bytes[..10]) && bytes[10] == b' ' && digits(&bytes[11..])
}

/// Whether some `YYYY-MM-DD` appears in `text`.
fn contains_date(text: &str) -> bool {
    text.as_bytes().windows(10).any(is_date)
}

/// A tag found in a stem: `start..end` (byte offsets, on ASCII characters).
struct Tag {
    start: usize,
    end: usize,
    kind: ConflictKind,
}

/// Includes the single space Nextcloud puts before a parenthesised tag.
fn with_leading_space(stem: &str, start: usize) -> usize {
    if start > 0 && stem.as_bytes()[start - 1] == b' ' {
        start - 1
    } else {
        start
    }
}

/// `(<keyword>[ prefix ]<date-time>)`, the rightmost one.
fn parenthesised_tag(stem: &str, lower: &str, keyword: &str, kind: ConflictKind) -> Option<Tag> {
    let open = lower.rfind(keyword)?;
    let close = open + stem.get(open..)?.find(')')?;
    let inner = stem.get(open + keyword.len()..close)?.trim();
    let bytes = inner.as_bytes();
    let date_ok = match kind {
        ConflictKind::CaseClash => is_date_time(bytes),
        // An optional user name, then the date and time.
        _ => {
            bytes.len() >= 17
                && is_date_time(&bytes[bytes.len() - 17..])
                && (bytes.len() == 17 || bytes[bytes.len() - 18] == b' ')
        }
    };
    date_ok.then(|| Tag {
        start: with_leading_space(stem, open),
        end: close + 1,
        kind,
    })
}

/// `_conflict-YYYYMMDD-HHMMSS`, the rightmost one.
fn legacy_tag(stem: &str) -> Option<Tag> {
    const KEYWORD: &str = "_conflict-";
    let start = stem.rfind(KEYWORD)?;
    let after = stem.as_bytes().get(start + KEYWORD.len()..)?;
    let ok = after.len() >= 15 && digits(&after[..8]) && after[8] == b'-' && digits(&after[9..15]);
    ok.then(|| Tag {
        start,
        end: start + KEYWORD.len() + 15,
        kind: ConflictKind::NextcloudLegacy,
    })
}

/// A last `(…)` group holding a conflict word and a date.
fn other_tag(stem: &str) -> Option<Tag> {
    let trimmed = stem.trim_end();
    if !trimmed.ends_with(')') {
        return None;
    }
    let open = trimmed.rfind('(')?;
    let inner = trimmed.get(open + 1..trimmed.len() - 1)?;
    let lower = inner.to_lowercase();
    let worded = CONFLICT_WORDS.iter().any(|w| lower.contains(w));
    (worded && contains_date(inner)).then(|| Tag {
        start: with_leading_space(stem, open),
        end: trimmed.len(),
        kind: ConflictKind::Other,
    })
}

/// Recognises a conflict-copy file name and gives the name of the original. Like Nextcloud,
/// only the rightmost tag is removed, so a conflict copy of a conflict copy points to the
/// first copy.
pub fn parse_conflict_name(file_name: &str) -> Option<ConflictName> {
    let (stem, extension) = match file_name.rfind('.') {
        Some(dot) if dot > 0 => file_name.split_at(dot),
        _ => (file_name, ""),
    };
    // ASCII lowercase keeps byte offsets unchanged.
    let lower = stem.to_ascii_lowercase();
    let tag = [
        parenthesised_tag(stem, &lower, "(conflicted copy", ConflictKind::Nextcloud),
        parenthesised_tag(stem, &lower, "(case clash from", ConflictKind::CaseClash),
        legacy_tag(stem),
        other_tag(stem),
    ]
    .into_iter()
    .flatten()
    // The rightmost tag; on a tie, the most specific scheme (listed first).
    .fold(None, |best: Option<Tag>, tag| match best {
        Some(best) if best.start >= tag.start => Some(best),
        _ => Some(tag),
    })?;
    let before = stem.get(..tag.start)?;
    let after = stem.get(tag.end..)?;
    let marker = stem.get(tag.start..tag.end)?.trim().to_owned();
    let original_stem = format!("{before}{after}");
    if original_stem.trim().is_empty() {
        return None;
    }
    Some(ConflictName {
        original_name: format!("{original_stem}{extension}"),
        kind: tag.kind,
        marker,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original(name: &str) -> Option<(String, ConflictKind)> {
        parse_conflict_name(name).map(|c| (c.original_name, c.kind))
    }

    fn is(name: &str, expected: &str, kind: ConflictKind) {
        assert_eq!(original(name), Some((expected.to_owned(), kind)), "{name}");
    }

    #[test]
    fn nextcloud_desktop_names() {
        is(
            "2026-10-24 Cours (conflicted copy 2026-10-24 101530).ardoise",
            "2026-10-24 Cours.ardoise",
            ConflictKind::Nextcloud,
        );
        is(
            "2026-10-24 Cours (conflicted copy camille 2026-10-24 101530).ardoise",
            "2026-10-24 Cours.ardoise",
            ConflictKind::Nextcloud,
        );
        is(
            "Note (conflicted copy Jean_Pierre Dupont 2026-01-02 030405).ardoise",
            "Note.ardoise",
            ConflictKind::Nextcloud,
        );
        is(
            "Note (case clash from 2026-10-24 101530).ardoise",
            "Note.ardoise",
            ConflictKind::CaseClash,
        );
        is(
            "Note_conflict-20261024-101530.ardoise",
            "Note.ardoise",
            ConflictKind::NextcloudLegacy,
        );
        let parsed =
            parse_conflict_name("Note (conflicted copy 2026-10-24 101530).ardoise").unwrap();
        assert_eq!(parsed.marker, "(conflicted copy 2026-10-24 101530)");
    }

    #[test]
    fn conflict_of_a_conflict_points_to_the_first_copy() {
        is(
            "Note (conflicted copy 2026-10-24 101530) (conflicted copy 2026-10-25 080000).ardoise",
            "Note (conflicted copy 2026-10-24 101530).ardoise",
            ConflictKind::Nextcloud,
        );
        is(
            "Note_conflict-20261024-101530 (conflicted copy 2026-10-25 080000).ardoise",
            "Note_conflict-20261024-101530.ardoise",
            ConflictKind::Nextcloud,
        );
    }

    #[test]
    fn translated_and_other_wordings() {
        is(
            "Note (copie en conflit de Camille 2026-10-24).ardoise",
            "Note.ardoise",
            ConflictKind::Other,
        );
        is(
            "Note (conflict 2026-10-24 101530).ardoise",
            "Note.ardoise",
            ConflictKind::Other,
        );
        is(
            "Note (Camille's conflicted copy 2026-10-24).ardoise",
            "Note.ardoise",
            ConflictKind::Other,
        );
        is(
            "Note (Konflikt-Kopie von Camille 2026-10-24).ardoise",
            "Note.ardoise",
            ConflictKind::Other,
        );
        is(
            "Nota (copia en conflicto de Ana 2026-10-24).ardoise",
            "Nota.ardoise",
            ConflictKind::Other,
        );
        is(
            "Note (CONFLICTED COPY 2026-10-24 101530).ARDOISE",
            "Note.ARDOISE",
            ConflictKind::Nextcloud,
        );
        // A damaged Nextcloud tag is still a conflict word with a date.
        is(
            "Note (conflicted copy 2026-10-24 1015).ardoise",
            "Note.ardoise",
            ConflictKind::Other,
        );
    }

    #[test]
    fn ordinary_titles_are_not_conflicts() {
        for name in [
            "2026-10-24 Cours.ardoise",
            "2026-10-24 Gestion de conflit (cours 2).ardoise",
            "2026-10-24 Conflicts (2).ardoise",
            "2026-10-24 Notes (conflicted copy of my thesis).ardoise",
            "x_conflict-2026.ardoise",
            "Note (case clash from bob 2026-10-24 101530).ardoise",
            "(conflicted copy 2026-10-24 101530).ardoise",
            "Note (conflicted copy 2026-10-24 101530.ardoise",
            "",
            ".",
            "Réunion « conflit » été 😀.ardoise",
        ] {
            assert_eq!(parse_conflict_name(name), None, "{name}");
        }
        // The date alone names a note that has no title: it is a valid original.
        is(
            "2026-10-24 (conflicted copy 2026-10-25 101530).ardoise",
            "2026-10-24.ardoise",
            ConflictKind::Nextcloud,
        );
    }

    #[test]
    fn never_panics_on_odd_text() {
        let mut rng = crate::testutil::Lcg::new(11);
        let pieces = [
            "(conflicted copy ",
            "(case clash from ",
            "_conflict-",
            "2026-10-24",
            " 101530",
            ")",
            "(",
            " ",
            "é",
            "😀",
            ".ardoise",
            "20261024-101530",
            "conflit",
            "x",
        ];
        for _ in 0..5000 {
            let name: String = (0..rng.below(8))
                .map(|_| pieces[rng.below(pieces.len() as u64) as usize])
                .collect();
            if let Some(parsed) = parse_conflict_name(&name) {
                assert!(parsed.original_name.len() < name.len(), "{name}");
            }
        }
    }
}
