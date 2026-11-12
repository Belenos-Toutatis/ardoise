//! `cargo xtask privacy`: fails when the git history holds personal data.
//!
//! The repository is private today but may be published (milestone M10), and publishing exposes
//! the **whole history**, not only the current tree. This command therefore reads every object
//! reachable from any ref (`git rev-list --all --objects`, each unique blob once, through a single
//! `git cat-file --batch`), plus every commit message, and looks for:
//!
//! 1. paths that must never have been committed (the private test data, Inkspace databases,
//!    bundles, diagnostic captures, notes and images outside the folders of synthetic data);
//! 2. in text blobs and commit messages: e-mail addresses, access tokens, Bluetooth addresses;
//! 3. private markers read at run time: the UUIDs found in `testdata/private/` (file names and the
//!    files we are allowed to open), which must appear nowhere in the history.
//!
//! The commit identities (author and committer) are listed as information: publishing would
//! expose them, and whether to do it is the owner's decision.
//!
//! What is *not* scanned, and said in the report: binary blobs and blobs of more than
//! [`MAX_TEXT_BYTES`] (for patterns; markers are still searched in them up to
//! [`MAX_MARKER_BYTES`]), compressed content (the `.ardoise` samples are ZIP archives), and
//! objects that no ref reaches any more (reflog, garbage left by a history rewrite): those never
//! leave the repository on a push.
//!
//! Findings print their value masked (three characters then `…`). Nothing personal is written in
//! this file either: the personal values are found by patterns and by markers read at run time.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Blobs above this size are not searched for patterns (text blobs of the repository are far
/// smaller; the largest are lock files).
const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
/// Blobs above this size are not even searched for markers.
const MAX_MARKER_BYTES: u64 = 64 * 1024 * 1024;
/// A blob whose first bytes contain a NUL is binary.
const BINARY_PROBE: usize = 8000;
/// Findings printed at most (the count is always complete).
const MAX_PRINTED: usize = 60;

// ---------------------------------------------------------------------------------------------
// Allowlists
// ---------------------------------------------------------------------------------------------

/// Folders that hold synthetic data, the only places where notes, images and the like may be
/// committed. Built from what the repository holds today; each was checked:
///
/// * `testdata/format/`: `.ardoise` fixtures written by `generate_v1_fixtures`
///   (`crates/ardoise-core/src/format/tests.rs`): fixed ids, 1x1 PNG, a few bytes of audio.
/// * `testdata/golden/`: rendering reference images made by `cargo xtask golden` from synthetic
///   notes defined in `crates/ardoise-render/tests/golden.rs`.
/// * `app/src-tauri/icons/`: the application icons.
///
/// A new folder (docs illustrations, UI assets…) must be added here on purpose, after checking
/// that its content is synthetic.
const SYNTHETIC_DATA_DIRS: &[&str] = &[
    "testdata/format/",
    "testdata/golden/",
    "app/src-tauri/icons/",
];

/// The only folder where `.jsonl` diagnostic captures may be committed: captures that were
/// anonymised first (Bluetooth addresses and host identifier replaced). Empty today. The
/// contents are still scanned (addresses, identifiers).
const ANONYMISED_CAPTURES_DIR: &str = "testdata/captures/";

/// Extensions of notes, images, audio and video: files that can carry personal content.
const CONTENT_EXTENSIONS: &[&str] = &[
    "ardoise", "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "avif", "ico",
    "icns", "svg", "pdf", "mp4", "webm", "mkv", "mov", "wav", "mp3", "opus", "ogg", "m4a",
];

/// Files holding third-party licence data, whose e-mail addresses are public (authors named in
/// licence texts and package metadata), not ours: the generated `THIRD-PARTY-NOTICES` (copyright
/// lines of dependencies) and the lock files (maintainers' addresses are not even stored there,
/// but the check stays valid if a registry adds them).
const THIRD_PARTY_FILES: &[&str] = &["THIRD-PARTY-NOTICES", "Cargo.lock", "app/package-lock.json"];

/// Domains reserved for documentation (RFC 2606) and their subdomains.
const DOCUMENTATION_DOMAINS: &[&str] = &["example.com", "example.org", "example.net"];
/// Reserved top-level domains (RFC 2606).
const DOCUMENTATION_TLDS: &[&str] = &["example", "test", "invalid", "localhost"];
/// Exact addresses that are public by construction: the attribution line of Claude Code commits.
const ALLOWED_EMAILS: &[&str] = &["noreply@anthropic.com"];
/// Suffix of GitHub's private no-reply addresses.
const ALLOWED_EMAIL_SUFFIXES: &[&str] = &["@users.noreply.github.com"];
/// Not e-mail addresses: `name@2x.png` style names and the like end with a file extension.
const FILE_EXTENSIONS_AS_TLD: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "ico", "webp", "avif", "bmp", "woff", "woff2", "ttf",
    "otf", "eot", "mp3", "mp4", "webm", "wav", "ogg", "opus", "wasm", "js", "mjs", "css", "html",
    "json", "map", "tgz",
];

/// Bluetooth addresses that are synthetic, compared in upper case with `:` separators. Each was
/// checked:
///
/// * `AA:BB:CC:DD:EE:FF`, `00:11:22:33:44:55`, `00:00:00:00:00:00`, `FF:FF:FF:FF:FF:FF`:
///   placeholders.
/// * `00:00:5E:00:53:xx`: the range IANA reserves for documentation (RFC 7042).
/// * `E2:43:03:67:0E:01`: the example address of Tuhi's public `example.ini`, copied into
///   `docs/research/protocol.md` and used by the tests of `ardoise-device`/`ardoise-ble`.
/// * `00:1A:7D:DA:71:13`: bytes written in a test of `ardoise-ble` (`convert.rs`), the sample
///   address found in Bluetooth tutorials.
/// * `D4:5A:2E:7B:3C:91` and `…:92`: the addresses of the two simulated tablets of `ardoise-sim`
///   (`crates/ardoise-sim/src/tablet.rs`), invented for the simulator.
const SYNTHETIC_ADDRESSES: &[&str] = &[
    "AA:BB:CC:DD:EE:FF",
    "00:11:22:33:44:55",
    "00:00:00:00:00:00",
    "FF:FF:FF:FF:FF:FF",
    "E2:43:03:67:0E:01",
    "00:1A:7D:DA:71:13",
    "D4:5A:2E:7B:3C:91",
    "D4:5A:2E:7B:3C:92",
];
/// Prefixes of synthetic addresses (see [`SYNTHETIC_ADDRESSES`]).
const SYNTHETIC_ADDRESS_PREFIXES: &[&str] = &["00:00:5E:00:53:"];

/// UUIDs that are public constants of a Bluetooth protocol, not private markers, although the
/// private captures hold them (a survey of a device lists its services), lower case:
///
/// * `6e40000x-b5a3-f393-e0a9-e50e24dcca9e`: the Nordic UART service and its characteristics
///   (1 to 3, published by Nordic Semiconductor and used by the Slate, `ardoise-ble/src/gatt.rs`),
///   and the fourth characteristic some tablets add to it (`docs/research/xiaomi-blackboard.md`).
const PUBLIC_UUIDS: &[&str] = &[
    "6e400001-b5a3-f393-e0a9-e50e24dcca9e",
    "6e400002-b5a3-f393-e0a9-e50e24dcca9e",
    "6e400003-b5a3-f393-e0a9-e50e24dcca9e",
    "6e400004-b5a3-f393-e0a9-e50e24dcca9e",
];

// ---------------------------------------------------------------------------------------------
// Path rules
// ---------------------------------------------------------------------------------------------

/// Why a path must never have been committed, or `None` when it may be.
fn forbidden_path(path: &str) -> Option<&'static str> {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("testdata/private/") || lower.contains("/testdata/private/") {
        return Some("private test data");
    }
    let components: Vec<&str> = normalized.split('/').collect();
    if components
        .iter()
        .any(|c| c.eq_ignore_ascii_case("LocalState") || c.eq_ignore_ascii_case("WacomCloudDb"))
    {
        return Some("Inkspace data");
    }
    let name = components.last().copied().unwrap_or_default();
    if name.to_ascii_lowercase().ends_with(".ldb")
        || name == "CURRENT"
        || name == "LOCK"
        || name.starts_with("MANIFEST-")
    {
        return Some("LevelDB file");
    }
    let name_lower = name.to_ascii_lowercase();
    if name_lower.ends_with(".bundle") {
        return Some("git bundle");
    }
    if name_lower.ends_with(".jsonl") && !lower.starts_with(ANONYMISED_CAPTURES_DIR) {
        return Some("diagnostic capture outside the anonymised captures folder");
    }
    let extension = name_lower.rsplit_once('.').map(|(_, e)| e);
    if extension.is_some_and(|e| CONTENT_EXTENSIONS.contains(&e))
        && !SYNTHETIC_DATA_DIRS.iter().any(|dir| lower.starts_with(dir))
    {
        return Some("note, image or media outside the synthetic data folders");
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Content patterns
// ---------------------------------------------------------------------------------------------

/// What a pattern found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hit {
    kind: &'static str,
    value: String,
}

fn hit(kind: &'static str, bytes: &[u8]) -> Hit {
    Hit {
        kind,
        value: String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// The first three characters of `value`, then `…`.
fn mask(value: &str) -> String {
    let head: String = value.chars().take(3).collect();
    format!("{head}…")
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

fn is_email_local(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_email_domain(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')
}

/// Characters of an access token (base64url, JWT, hex, opaque ids).
fn is_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'+' | b'/' | b'=' | b'-')
}

/// Every e-mail address in `data` (before any allowlist).
fn find_emails(data: &[u8]) -> Vec<Hit> {
    let mut out = Vec::new();
    for (at, _) in data.iter().enumerate().filter(|(_, b)| **b == b'@') {
        let mut start = at;
        while start > 0 && is_email_local(data[start - 1]) {
            start -= 1;
        }
        while start < at && matches!(data[start], b'.' | b'-') {
            start += 1;
        }
        let mut end = at + 1;
        while end < data.len() && is_email_domain(data[end]) {
            end += 1;
        }
        while end > at + 1 && matches!(data[end - 1], b'.' | b'-') {
            end -= 1;
        }
        if start == at || end <= at + 1 {
            continue;
        }
        let domain = &data[at + 1..end];
        let labels: Vec<&[u8]> = domain.split(|b| *b == b'.').collect();
        let Some(tld) = labels.last() else { continue };
        if labels.len() < 2
            || labels.iter().any(|l| l.is_empty())
            || tld.len() < 2
            || !tld.iter().all(u8::is_ascii_alphabetic)
        {
            continue;
        }
        let tld = String::from_utf8_lossy(tld).to_ascii_lowercase();
        if FILE_EXTENSIONS_AS_TLD.contains(&tld.as_str()) {
            continue;
        }
        out.push(hit("e-mail address", &data[start..end]));
    }
    out
}

/// True if `email` is public data (see the allowlist constants) or `paths` are all third-party
/// licence files.
fn email_allowed(email: &str, paths: &[String]) -> bool {
    let lower = email.to_ascii_lowercase();
    if ALLOWED_EMAILS.contains(&lower.as_str())
        || ALLOWED_EMAIL_SUFFIXES.iter().any(|s| lower.ends_with(s))
    {
        return true;
    }
    if let Some((_, domain)) = lower.split_once('@') {
        let documentation = DOCUMENTATION_DOMAINS
            .iter()
            .any(|d| domain == *d || domain.ends_with(&format!(".{d}")))
            || domain
                .rsplit('.')
                .next()
                .is_some_and(|tld| DOCUMENTATION_TLDS.contains(&tld));
        if documentation {
            return true;
        }
    }
    !paths.is_empty()
        && paths
            .iter()
            .all(|p| THIRD_PARTY_FILES.contains(&p.as_str()))
}

/// Access tokens: JWT-like strings, `access_token`-style fields with a long value, `Bearer`
/// credentials.
fn find_tokens(data: &[u8]) -> Vec<Hit> {
    let mut out = Vec::new();
    let lower = data.to_ascii_lowercase();

    // JWT: `eyJ` + base64url header, then two more dot-separated base64url parts.
    let mut from = 0;
    while let Some(pos) = find(&data[from..], b"eyJ") {
        let start = from + pos;
        let mut i = start;
        let mut parts = 0;
        let mut lengths = [0usize; 3];
        while i < data.len() && parts < 3 {
            let part_start = i;
            while i < data.len()
                && (data[i].is_ascii_alphanumeric() || matches!(data[i], b'_' | b'-'))
            {
                i += 1;
            }
            lengths[parts] = i - part_start;
            parts += 1;
            if parts < 3 && i < data.len() && data[i] == b'.' {
                i += 1;
            } else {
                break;
            }
        }
        if parts == 3 && lengths[0] >= 11 && lengths[1] >= 8 {
            out.push(hit("access token (JWT)", &data[start..i]));
        }
        from = start + 3;
    }

    // `"access_token": "<long value>"` and variants (JSON, query strings, `key=value`).
    for keyword in [
        &b"access_token"[..],
        b"refresh_token",
        b"id_token",
        b"accesstoken",
        b"refreshtoken",
        b"idtoken",
    ] {
        let mut from = 0;
        while let Some(pos) = find(&lower[from..], keyword) {
            let mut i = from + pos + keyword.len();
            from = i;
            let mut skipped = 0;
            while i < data.len() && matches!(data[i], b'"' | b'\'' | b'\\' | b' ') && skipped < 6 {
                i += 1;
                skipped += 1;
            }
            if i >= data.len() || !matches!(data[i], b':' | b'=') {
                continue;
            }
            i += 1;
            let mut skipped = 0;
            while i < data.len() && matches!(data[i], b'"' | b'\'' | b'\\' | b' ') && skipped < 6 {
                i += 1;
                skipped += 1;
            }
            let start = i;
            while i < data.len() && is_token_char(data[i]) {
                i += 1;
            }
            if looks_like_secret(&data[start..i], 20) {
                out.push(hit("access token (field)", &data[start..i]));
            }
        }
    }

    // `Bearer <credential>`.
    let mut from = 0;
    while let Some(pos) = find(&lower[from..], b"bearer ") {
        let mut i = from + pos + 7;
        from = i;
        while i < data.len() && data[i] == b' ' {
            i += 1;
        }
        let start = i;
        while i < data.len() && is_token_char(data[i]) {
            i += 1;
        }
        if looks_like_secret(&data[start..i], 20) {
            out.push(hit("access token (Bearer)", &data[start..i]));
        }
    }
    out
}

/// A long run of token characters with at least one digit and one letter: not prose, not a
/// placeholder such as `xxxxxxxx`.
fn looks_like_secret(run: &[u8], min_len: usize) -> bool {
    run.len() >= min_len
        && run.iter().any(u8::is_ascii_digit)
        && run.iter().any(u8::is_ascii_alphabetic)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Bluetooth (MAC) addresses: six pairs of hex digits separated by `:` or `-`.
fn find_addresses(data: &[u8]) -> Vec<Hit> {
    let mut out = Vec::new();
    if data.len() < 17 {
        return out;
    }
    for start in 0..=data.len() - 17 {
        let window = &data[start..start + 17];
        let sep = window[2];
        if sep != b':' && sep != b'-' {
            continue;
        }
        let shape = window
            .iter()
            .enumerate()
            .all(|(i, b)| if i % 3 == 2 { *b == sep } else { is_hex(*b) });
        if !shape {
            continue;
        }
        if start > 0 && (is_hex(data[start - 1]) || data[start - 1] == b':') {
            continue;
        }
        if let Some(next) = data.get(start + 17) {
            let longer =
                is_hex(*next) || (*next == sep && data.get(start + 18).is_some_and(|b| is_hex(*b)));
            if longer {
                continue;
            }
        }
        out.push(hit("Bluetooth address", window));
    }
    out
}

/// True if `address` is one of the synthetic addresses.
fn address_allowed(address: &str) -> bool {
    let normalized = address.replace('-', ":").to_ascii_uppercase();
    SYNTHETIC_ADDRESSES.contains(&normalized.as_str())
        || SYNTHETIC_ADDRESS_PREFIXES
            .iter()
            .any(|p| normalized.starts_with(p))
}

/// Every UUID (8-4-4-4-12 hex digits), lower-cased.
fn find_uuids(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if data.len() < 36 {
        return out;
    }
    for start in 0..=data.len() - 36 {
        if !is_hex(data[start]) || data[start + 8] != b'-' {
            continue;
        }
        let window = &data[start..start + 36];
        let shape = window.iter().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *b == b'-'
            } else {
                is_hex(*b)
            }
        });
        if !shape
            || (start > 0 && is_hex(data[start - 1]))
            || data.get(start + 36).is_some_and(|b| is_hex(*b))
        {
            continue;
        }
        out.push(String::from_utf8_lossy(window).to_ascii_lowercase());
    }
    out
}

/// Pattern findings in text (a blob or a commit message): addresses, tokens, Bluetooth
/// addresses, after the allowlists. `paths` are the known paths of the blob (empty for a message).
fn scan_text(data: &[u8], paths: &[String]) -> Vec<Hit> {
    let mut hits = Vec::new();
    hits.extend(
        find_emails(data)
            .into_iter()
            .filter(|h| !email_allowed(&h.value, paths)),
    );
    hits.extend(find_tokens(data));
    hits.extend(
        find_addresses(data)
            .into_iter()
            .filter(|h| !address_allowed(&h.value)),
    );
    hits
}

// ---------------------------------------------------------------------------------------------
// Private markers
// ---------------------------------------------------------------------------------------------

/// May the content of this file (path relative to `testdata/private/`) be opened? Names may
/// always be listed. Under an Inkspace `db` folder only `db/notes` may be opened (the `profile`
/// key of that database holds an access token and an e-mail address), and nothing called
/// `profile` (or starting with it) ever.
fn may_open_content(relative: &[String]) -> bool {
    if relative
        .iter()
        .any(|c| c.to_ascii_lowercase().starts_with("profile"))
    {
        return false;
    }
    match relative.iter().position(|c| c.eq_ignore_ascii_case("db")) {
        None => true,
        Some(i) => relative
            .get(i + 1)
            .is_some_and(|c| c.eq_ignore_ascii_case("notes")),
    }
}

/// Where `testdata/private/` is: in the checkout, or, in a git worktree (where that ignored
/// folder is not copied), in the main working tree.
fn private_dir(root: &Path) -> Option<PathBuf> {
    let here = root.join("testdata").join("private");
    if here.is_dir() {
        return Some(here);
    }
    let out = git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?;
    let common = PathBuf::from(String::from_utf8_lossy(&out).trim());
    let main = common.parent()?;
    let candidate = main.join("testdata").join("private");
    candidate.is_dir().then_some(candidate)
}

/// The UUIDs of the private folder: in every file and folder name, and in the content of the
/// files we may open, without the public constants of [`PUBLIC_UUIDS`]. Returns them with the
/// number of files whose content was read. The values are never printed.
fn collect_markers(dir: &Path) -> io::Result<(BTreeSet<String>, usize)> {
    fn walk(
        dir: &Path,
        relative: &mut Vec<String>,
        markers: &mut BTreeSet<String>,
        opened: &mut usize,
    ) -> io::Result<()> {
        let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<io::Result<_>>()?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            markers.extend(find_uuids(name.as_bytes()));
            relative.push(name);
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(&path, relative, markers, opened)?;
            } else if kind.is_file()
                && may_open_content(relative)
                && fs::metadata(&path)?.len() <= MAX_MARKER_BYTES
            {
                markers.extend(find_uuids(&fs::read(&path)?));
                *opened += 1;
            }
            relative.pop();
        }
        Ok(())
    }
    let mut markers = BTreeSet::new();
    let mut opened = 0;
    walk(dir, &mut Vec::new(), &mut markers, &mut opened)?;
    markers.retain(|uuid| !PUBLIC_UUIDS.contains(&uuid.as_str()));
    Ok((markers, opened))
}

// ---------------------------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------------------------

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// One finding: where it was introduced and what it is. The value is already masked.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    path: String,
    kind: String,
    masked: String,
    commit: String,
}

/// A commit identity (author or committer).
#[derive(Debug, Default)]
struct Identity {
    name: String,
    authored: usize,
    committed: usize,
}

#[derive(Debug, Default)]
struct Report {
    commits: usize,
    blobs: usize,
    tags: usize,
    bytes: u64,
    scanned_as_text: usize,
    skipped_binary: usize,
    skipped_large: usize,
    skipped_huge: usize,
    markers: Option<usize>,
    private_files_read: usize,
    shallow: bool,
    findings: Vec<Finding>,
    identities: BTreeMap<String, Identity>,
}

/// Blob oid -> (introducing commit, all known paths), and every path ever committed.
struct Attribution {
    blobs: BTreeMap<String, (String, Vec<String>)>,
    paths: BTreeMap<String, String>,
}

/// Reads `git log --raw` over all refs (every parent of merges too): which commit introduced
/// each blob under which paths, and the commit that first touched each path.
fn attribution(root: &Path) -> Result<Attribution, String> {
    let out = git(
        root,
        &[
            "-c",
            "core.quotepath=off",
            "log",
            "--all",
            "-m",
            "--no-renames",
            "--raw",
            "--no-abbrev",
            "--format=%x01%H",
        ],
    )?;
    let text = String::from_utf8_lossy(&out);
    let mut attribution = Attribution {
        blobs: BTreeMap::new(),
        paths: BTreeMap::new(),
    };
    let mut commit = String::new();
    // Newest first: the last write wins, so the oldest commit is kept.
    for line in text.lines() {
        if let Some(hash) = line.strip_prefix('\u{1}') {
            commit = hash.to_owned();
        } else if let Some(rest) = line.strip_prefix(':')
            && let Some((meta, path)) = rest.split_once('\t')
        {
            attribution.paths.insert(path.to_owned(), commit.clone());
            let fields: Vec<&str> = meta.split_whitespace().collect();
            // :<old mode> <new mode> <old oid> <new oid> <status>
            if let [_, new_mode, _, new_oid, _] = fields.as_slice()
                && *new_mode != "160000"
                && new_oid.bytes().any(|b| b != b'0')
            {
                let entry = attribution
                    .blobs
                    .entry((*new_oid).to_owned())
                    .or_insert_with(|| (commit.clone(), Vec::new()));
                entry.0 = commit.clone();
                if !entry.1.iter().any(|p| p == path) {
                    entry.1.push(path.to_owned());
                }
            }
        }
    }
    Ok(attribution)
}

/// `Name <email>` of an `author`/`committer` header value (`Name <email> 1700000000 +0100`).
fn parse_identity(value: &str) -> Option<(String, String)> {
    let open = value.rfind('<')?;
    let close = value[open..].find('>')? + open;
    Some((
        value[..open].trim().to_owned(),
        value[open + 1..close].to_owned(),
    ))
}

/// Scans the history of the repository at `root`. `private` is the folder of private test data
/// when it exists.
fn scan(root: &Path, private: Option<&Path>) -> Result<Report, String> {
    let mut report = Report::default();
    let markers: BTreeSet<String> = match private {
        Some(dir) => {
            let (found, opened) = collect_markers(dir)
                .map_err(|e| format!("cannot read the private test data: {e}"))?;
            report.private_files_read = opened;
            found
        }
        None => BTreeSet::new(),
    };
    if private.is_some() {
        report.markers = Some(markers.len());
    }
    report.shallow = git(root, &["rev-parse", "--is-shallow-repository"])
        .is_ok_and(|out| String::from_utf8_lossy(&out).trim() == "true");

    let attribution = attribution(root)?;
    let listing = git(root, &["rev-list", "--all", "--objects"])?;
    let mut oids: Vec<String> = Vec::new();
    let mut listed_paths: BTreeMap<String, String> = BTreeMap::new();
    for line in String::from_utf8_lossy(&listing).lines() {
        let (oid, path) = line.split_once(' ').unwrap_or((line, ""));
        oids.push(oid.to_owned());
        if !path.is_empty() {
            listed_paths.insert(oid.to_owned(), path.to_owned());
        }
    }
    oids.sort();
    oids.dedup();

    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("cannot run git: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file has no stdin")?;
    let stdout = child.stdout.take().ok_or("git cat-file has no stdout")?;
    let mut reader = BufReader::with_capacity(1 << 16, stdout);

    let mut raw_findings: Vec<Finding> = Vec::new();
    let mut blob_paths: Vec<String> = Vec::new();
    let result: Result<(), String> = std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            for oid in &oids {
                if writeln!(stdin, "{oid}").is_err() {
                    break;
                }
            }
            // `stdin` is dropped here: git sees the end of the input.
        });
        let outcome: Result<(), String> = (|| {
            let mut buffer: Vec<u8> = Vec::new();
            let mut header = String::new();
            loop {
                header.clear();
                if reader.read_line(&mut header).map_err(|e| e.to_string())? == 0 {
                    break;
                }
                let fields: Vec<&str> = header.split_whitespace().collect();
                let [oid, kind, size] = fields.as_slice() else {
                    return Err(format!("unexpected git cat-file answer: {}", header.trim()));
                };
                let size: u64 = size.parse().map_err(|_| "bad object size".to_owned())?;
                let wanted = match *kind {
                    "blob" => size <= MAX_MARKER_BYTES,
                    "commit" | "tag" => true,
                    _ => false,
                };
                buffer.clear();
                if wanted {
                    buffer.resize(size as usize, 0);
                    reader.read_exact(&mut buffer).map_err(|e| e.to_string())?;
                } else {
                    io::copy(&mut reader.by_ref().take(size), &mut io::sink())
                        .map_err(|e| e.to_string())?;
                }
                let mut newline = [0u8; 1];
                reader.read_exact(&mut newline).map_err(|e| e.to_string())?;

                match *kind {
                    "blob" => {
                        report.blobs += 1;
                        report.bytes += size;
                        let (commit, mut paths) = match attribution.blobs.get(*oid) {
                            Some((commit, paths)) => (commit.clone(), paths.clone()),
                            None => (String::new(), Vec::new()),
                        };
                        if let Some(listed) = listed_paths.get(*oid) {
                            blob_paths.push(listed.clone());
                            if !paths.contains(listed) {
                                paths.push(listed.clone());
                            }
                        }
                        paths.sort();
                        let shown = paths.first().cloned().unwrap_or_else(|| (*oid).to_owned());
                        let mut push = |hit_kind: &str, value: &str| {
                            raw_findings.push(Finding {
                                path: shown.clone(),
                                kind: hit_kind.to_owned(),
                                masked: mask(value),
                                commit: commit.clone(),
                            });
                        };
                        if !wanted {
                            report.skipped_huge += 1;
                            continue;
                        }
                        for uuid in find_uuids(&buffer) {
                            if markers.contains(&uuid) {
                                push("private marker", &uuid);
                            }
                        }
                        if size > MAX_TEXT_BYTES {
                            report.skipped_large += 1;
                        } else if buffer[..buffer.len().min(BINARY_PROBE)].contains(&0) {
                            report.skipped_binary += 1;
                        } else {
                            report.scanned_as_text += 1;
                            for h in scan_text(&buffer, &paths) {
                                push(h.kind, &h.value);
                            }
                        }
                    }
                    "commit" | "tag" => {
                        let commit_label = if *kind == "commit" {
                            report.commits += 1;
                            (*oid).to_owned()
                        } else {
                            report.tags += 1;
                            format!("tag {oid}")
                        };
                        let split = find(&buffer, b"\n\n").unwrap_or(buffer.len());
                        let headers = String::from_utf8_lossy(&buffer[..split]).into_owned();
                        let message = buffer.get(split + 2..).unwrap_or_default();
                        for line in headers.lines() {
                            let (role, value) = line.split_once(' ').unwrap_or((line, ""));
                            if matches!(role, "author" | "committer")
                                && let Some((name, email)) = parse_identity(value)
                            {
                                let identity = report.identities.entry(email).or_default();
                                identity.name = name;
                                if role == "author" {
                                    identity.authored += 1;
                                } else {
                                    identity.committed += 1;
                                }
                            }
                        }
                        let mut push = |hit_kind: &str, value: &str| {
                            raw_findings.push(Finding {
                                path: "(commit message)".to_owned(),
                                kind: hit_kind.to_owned(),
                                masked: mask(value),
                                commit: commit_label.clone(),
                            });
                        };
                        for uuid in find_uuids(message) {
                            if markers.contains(&uuid) {
                                push("private marker", &uuid);
                            }
                        }
                        for h in scan_text(message, &[]) {
                            push(h.kind, &h.value);
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        })();
        if outcome.is_err() {
            // Unblock the writer thread if git is still waiting.
            let _ = child.kill();
        }
        writer
            .join()
            .map_err(|_| "writer thread panicked".to_owned())?;
        outcome
    });
    let status = child.wait().map_err(|e| e.to_string())?;
    result?;
    if !status.success() {
        return Err("git cat-file failed".to_owned());
    }

    // Paths: every path ever committed (history of changes) and every path of a listed blob.
    let mut all_paths: BTreeMap<String, String> = attribution.paths.clone();
    for path in blob_paths {
        all_paths.entry(path).or_default();
    }
    for (path, commit) in &all_paths {
        if let Some(reason) = forbidden_path(path) {
            raw_findings.push(Finding {
                path: path.clone(),
                kind: format!("forbidden path: {reason}"),
                masked: mask(path),
                commit: commit.clone(),
            });
        }
        for uuid in find_uuids(path.as_bytes()) {
            if markers.contains(&uuid) {
                raw_findings.push(Finding {
                    path: path.clone(),
                    kind: "private marker in a path".to_owned(),
                    masked: mask(&uuid),
                    commit: commit.clone(),
                });
            }
        }
    }

    raw_findings.sort();
    raw_findings.dedup();
    report.findings = raw_findings;
    Ok(report)
}

/// Is this address one of the public no-reply addresses?
fn is_noreply(email: &str) -> bool {
    let lower = email.to_ascii_lowercase();
    ALLOWED_EMAILS.contains(&lower.as_str())
        || ALLOWED_EMAIL_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

fn short(commit: &str) -> &str {
    commit.get(..10).unwrap_or(commit)
}

/// Runs the check on the repository at `root`, prints the report, returns true when it passes.
pub fn run(root: &Path) -> bool {
    let start = Instant::now();
    let private = private_dir(root);
    let report = match scan(root, private.as_deref()) {
        Ok(report) => report,
        Err(e) => {
            eprintln!("privacy: {e}");
            println!("PRIVACY FAILED");
            return false;
        }
    };
    let elapsed = start.elapsed();

    println!(
        "scanned {} commits, {} tags and {} blobs ({:.1} MiB) in {:.1} s",
        report.commits,
        report.tags,
        report.blobs,
        report.bytes as f64 / (1024.0 * 1024.0),
        elapsed.as_secs_f64()
    );
    println!(
        "  patterns searched in {} text blobs; skipped for patterns: {} binary, {} over {} MiB; \
         {} over {} MiB skipped entirely",
        report.scanned_as_text,
        report.skipped_binary,
        report.skipped_large,
        MAX_TEXT_BYTES / (1024 * 1024),
        report.skipped_huge,
        MAX_MARKER_BYTES / (1024 * 1024)
    );
    println!(
        "  not scanned: objects no ref reaches (reflog, rewritten history): they are not pushed"
    );
    if report.shallow {
        println!("warning: shallow clone: the history before the cut is not scanned");
    }
    match report.markers {
        Some(count) => println!(
            "private markers: {count} UUIDs read from {} files of testdata/private (values not shown)",
            report.private_files_read
        ),
        None => println!(
            "private markers: skipped (no testdata/private folder here: cloud session or fresh clone)"
        ),
    }

    // Identities: information, not a failure.
    let exposed: Vec<_> = report
        .identities
        .iter()
        .filter(|(email, _)| !is_noreply(email))
        .collect();
    println!(
        "commit identities ({} addresses, {} not a no-reply address):",
        report.identities.len(),
        exposed.len()
    );
    for (email, identity) in &report.identities {
        let note = if is_noreply(email) {
            "no-reply address"
        } else {
            "publishing would expose this address"
        };
        println!(
            "  {} <{}>  {} authored, {} committed  ({note})",
            identity.name, email, identity.authored, identity.committed
        );
    }
    if !exposed.is_empty() {
        println!(
            "warning: the history carries {} personal commit address(es); whether to publish it \
             as is, rewrite it or start a clean repository is the owner's decision",
            exposed.len()
        );
    }

    // Findings, one line per (path, kind, value) with the oldest commit and the revision count.
    let mut grouped: BTreeMap<(String, String, String), (String, usize)> = BTreeMap::new();
    for f in &report.findings {
        let entry = grouped
            .entry((f.path.clone(), f.kind.clone(), f.masked.clone()))
            .or_insert_with(|| (f.commit.clone(), 0));
        entry.1 += 1;
    }
    for (i, ((path, kind, masked), (commit, count))) in grouped.iter().enumerate() {
        if i == MAX_PRINTED {
            println!("  … {} more findings", grouped.len() - MAX_PRINTED);
            break;
        }
        let revisions = if *count > 1 {
            format!(" (+{} other revisions)", count - 1)
        } else {
            String::new()
        };
        println!("FOUND {} {path}: {kind} {masked}{revisions}", short(commit));
    }

    if grouped.is_empty() {
        println!("PRIVACY OK");
        true
    } else {
        println!("PRIVACY FAILED: {} finding(s)", grouped.len());
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values that would match a pattern are assembled at run time, so that this source file does
    // not itself contain a token, a personal-looking address or an unknown Bluetooth address.
    fn email(local: &str, domain: &str) -> String {
        format!("{local}@{domain}")
    }

    fn mac(bytes: [u8; 6]) -> String {
        let parts: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
        parts.join(":")
    }

    fn kinds(hits: &[Hit]) -> Vec<&'static str> {
        hits.iter().map(|h| h.kind).collect()
    }

    #[test]
    fn forbidden_paths() {
        assert!(forbidden_path("testdata/private/inkspace/x.txt").is_some());
        assert!(forbidden_path("Testdata\\Private\\x").is_some());
        assert!(forbidden_path("a/LocalState/db/notes/x").is_some());
        assert!(forbidden_path("WacomCloudDb/_/CURRENT").is_some());
        assert!(forbidden_path("x/000031.ldb").is_some());
        assert!(forbidden_path("x/MANIFEST-000041").is_some());
        assert!(forbidden_path("x/LOCK").is_some());
        assert!(forbidden_path("backup.bundle").is_some());
        assert!(forbidden_path("diagnostics/2026-10-24_140312.jsonl").is_some());
        assert!(forbidden_path("notes/work.ardoise").is_some());
        assert!(forbidden_path("docs/capture.PNG").is_some());

        assert_eq!(forbidden_path("testdata/captures/anon.jsonl"), None);
        assert_eq!(forbidden_path("testdata/format/v1/full.ardoise"), None);
        assert_eq!(forbidden_path("testdata/golden/lined_white.png"), None);
        assert_eq!(forbidden_path("app/src-tauri/icons/icon.ico"), None);
        assert_eq!(forbidden_path("crates/ardoise-core/src/lib.rs"), None);
        assert_eq!(forbidden_path("docs/design/device-stack.md"), None);
        assert_eq!(forbidden_path("THIRD-PARTY-NOTICES"), None);
        // `private` elsewhere, or a file merely named like the folder, is fine.
        assert_eq!(forbidden_path("docs/private-notes.md"), None);
    }

    #[test]
    fn emails_found_and_allowed() {
        let someone = email("someone", "gmail.com");
        let hits = find_emails(format!("author: {someone}, thanks").as_bytes());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, someone);
        assert!(!email_allowed(&someone, &[]));
        assert!(!email_allowed(&someone, &["src/lib.rs".to_owned()]));

        for ok in [
            "noreply@anthropic.com",
            "NoReply@Anthropic.com",
            "12345+name@users.noreply.github.com",
            "a@example.com",
            "a@mail.example.org",
            "a@example.net",
            "a@host.test",
            "a@host.invalid",
        ] {
            assert!(email_allowed(ok, &[]), "{ok}");
        }
        // Not the allowed domains merely containing the word.
        assert!(!email_allowed(&email("a", "notexample.com"), &[]));
        assert!(!email_allowed(&email("a", "example.com.evil.io"), &[]));

        // Third-party licence data: only when every known path is one of those files.
        let notice = vec!["THIRD-PARTY-NOTICES".to_owned()];
        let mixed = vec!["THIRD-PARTY-NOTICES".to_owned(), "docs/x.md".to_owned()];
        assert!(email_allowed(&someone, &notice));
        assert!(!email_allowed(&someone, &mixed));
    }

    #[test]
    fn things_that_are_not_emails() {
        for text in [
            "icons/128x128@2x.png",
            "pkg@1.2.3",
            "@types/node",
            "x @ y.z",
            "user@host",
            "see @babel/core",
            "a@b.c",
        ] {
            assert!(find_emails(text.as_bytes()).is_empty(), "{text}");
        }
        // Trailing punctuation is not part of the address.
        let text = format!("write to {}.", email("who", "corp.fr"));
        assert_eq!(
            find_emails(text.as_bytes())[0].value,
            email("who", "corp.fr")
        );
    }

    #[test]
    fn tokens() {
        // JWT-like.
        let jwt = format!(
            "{}{}",
            "eyJ", "hbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2lnbmF0dXJl"
        );
        let hits = find_tokens(format!("Authorization: {jwt}").as_bytes());
        assert!(kinds(&hits).contains(&"access token (JWT)"), "{hits:?}");
        let truncated = format!("{}{}", "eyJ", "…");
        assert!(find_tokens(truncated.as_bytes()).is_empty());

        // Fields with a long value, JSON-escaped or not.
        let value = "q7Zx9".repeat(8);
        for text in [
            format!("{{\"access_token\": \"{value}\"}}"),
            format!("{{\\\"refresh_token\\\":\\\"{value}\\\"}}"),
            format!("id_token = {value}"),
            format!("?accessToken={value}&x=1"),
        ] {
            let hits = find_tokens(text.as_bytes());
            assert!(
                kinds(&hits).contains(&"access token (field)"),
                "{text}: {hits:?}"
            );
        }
        // Short values, types and placeholders are not tokens.
        for text in [
            "access_token: String",
            "\"access_token\": \"abc123\"",
            "refresh_token = <your token here>",
            &format!("\"id_token\": \"{}\"", "x".repeat(40)),
        ] {
            assert!(find_tokens(text.as_bytes()).is_empty(), "{text}");
        }

        // Bearer credentials.
        let bearer = format!("Bearer {}", "a1B2c3D4e5".repeat(3));
        assert!(kinds(&find_tokens(bearer.as_bytes())).contains(&"access token (Bearer)"));
        assert!(find_tokens(b"a bearer of good news, authentication-scheme-identifier").is_empty());
        assert!(find_tokens(b"format!(\"Bearer {token}\")").is_empty());
    }

    #[test]
    fn bluetooth_addresses() {
        let unknown = mac([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        let hits = find_addresses(format!("found at {unknown} (rssi -58)").as_bytes());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, unknown);
        assert!(!address_allowed(&unknown));
        assert!(!address_allowed(&unknown.replace(':', "-")));

        for ok in [
            "AA:BB:CC:DD:EE:FF",
            "aa-bb-cc-dd-ee-ff",
            "00:00:5E:00:53:01",
            "E2:43:03:67:0E:01",
            "D4:5A:2E:7B:3C:92",
        ] {
            assert!(address_allowed(ok), "{ok}");
        }

        // Longer hex sequences (EUI-64, IPv6, hashes) and plain times are not addresses.
        for text in [
            "fe80::1122:3344:5566:7788",
            "11:22:33:44:55:66:77:88",
            "12:34:56",
            "2026-10-24 13:00:00",
        ] {
            assert!(find_addresses(text.as_bytes()).is_empty(), "{text}");
        }
        assert!(find_addresses(b"short").is_empty());
    }

    #[test]
    fn uuids() {
        let text =
            "id=0A1B2C3D-0000-4000-8000-00000000ABCD, other 123e4567-e89b-12d3-a456-426614174000.";
        assert_eq!(
            find_uuids(text.as_bytes()),
            [
                "0a1b2c3d-0000-4000-8000-00000000abcd",
                "123e4567-e89b-12d3-a456-426614174000"
            ]
        );
        // Not a UUID: wrong shape, or embedded in a longer hex run.
        assert!(find_uuids(b"0a1b2c3d-0000-4000-8000-00000000abc").is_empty());
        assert!(find_uuids(b"ff0a1b2c3d-0000-4000-8000-00000000abcd").is_empty());
        assert!(find_uuids(b"0a1b2c3d-0000-4000-8000-00000000abcdef").is_empty());
        assert!(find_uuids(b"").is_empty());
    }

    #[test]
    fn masking() {
        assert_eq!(mask("abcdef"), "abc…");
        assert_eq!(mask("ab"), "ab…");
        assert_eq!(mask("été-x"), "été…");
    }

    #[test]
    fn identity_parsing() {
        assert_eq!(
            parse_identity("Some Body <some@example.com> 1700000000 +0100"),
            Some(("Some Body".to_owned(), "some@example.com".to_owned()))
        );
        assert_eq!(parse_identity("no email 1700000000 +0100"), None);
    }

    #[test]
    fn private_content_rules() {
        let p = |s: &str| -> Vec<String> { s.split('/').map(str::to_owned).collect() };
        assert!(may_open_content(&p(
            "inkspace/LocalState/db/notes/x/preview.png"
        )));
        assert!(may_open_content(&p(
            "inkspace/LocalState/WacomCloudDb/_/000042.log"
        )));
        assert!(!may_open_content(&p("inkspace/LocalState/db/profile")));
        assert!(!may_open_content(&p("inkspace/LocalState/db/000005.ldb")));
        assert!(!may_open_content(&p("inkspace/LocalState/DB/other")));
        assert!(!may_open_content(&p("anything/profile.json")));
        assert!(!may_open_content(&p("x/profile/y")));
    }

    /// A unique scratch folder under the system temporary folder, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("ardoise-privacy-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn markers_come_from_names_and_allowed_files_only() {
        let scratch = Scratch::new("markers");
        let dir = scratch.0.join("private");
        let from_name = "11111111-1111-4111-8111-111111111111";
        let from_content = "22222222-2222-4222-8222-222222222222";
        let from_profile = "33333333-3333-4333-8333-333333333333";
        let from_other_db = "44444444-4444-4444-8444-444444444444";
        let from_profile_name = "55555555-5555-4555-8555-555555555555";
        fs::create_dir_all(dir.join("x/db/notes").join(from_name)).unwrap();
        fs::write(
            dir.join("x/db/notes").join(from_name).join("note.txt"),
            format!("see {}", from_content.to_uppercase()),
        )
        .unwrap();
        fs::write(dir.join("x/db/profile"), from_profile).unwrap();
        fs::write(dir.join("x/db/000001.ldb"), from_other_db).unwrap();
        fs::write(dir.join("x").join(format!("{from_profile_name}.txt")), "").unwrap();
        // A survey of a device lists its services: the Nordic UART ones are public constants.
        fs::write(
            dir.join("x/survey.jsonl"),
            format!("{{\"uuid\":\"{}\"}}", PUBLIC_UUIDS[2].to_uppercase()),
        )
        .unwrap();

        let (markers, opened) = collect_markers(&dir).unwrap();
        assert!(
            PUBLIC_UUIDS.iter().all(|uuid| !markers.contains(*uuid)),
            "public protocol constants are not private markers"
        );
        assert!(markers.contains(from_name));
        assert!(markers.contains(from_content));
        assert!(markers.contains(from_profile_name), "names are listed");
        assert!(
            !markers.contains(from_profile),
            "db/profile must not be opened"
        );
        assert!(
            !markers.contains(from_other_db),
            "only db/notes may be opened"
        );
        assert_eq!(opened, 3);
    }

    fn git_ok(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.com",
            ])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// End to end on a throw-away repository holding synthetic bad data.
    #[test]
    fn scans_a_whole_history() {
        let scratch = Scratch::new("repo");
        let repo = scratch.0.join("repo");
        fs::create_dir_all(&repo).unwrap();
        if !git_ok(&repo, &["init", "-q"]) {
            return; // git is not available here
        }
        let marker = "66666666-6666-4666-8666-666666666666";
        let private = scratch.0.join("private");
        fs::create_dir_all(&private).unwrap();
        fs::write(private.join(format!("{marker}.txt")), "").unwrap();

        // Commit 1: an unknown address in a note, a path that is forbidden, an allowed file.
        let unknown = email("someone", "gmail.com");
        fs::write(repo.join("notes.md"), format!("contact {unknown}\n")).unwrap();
        fs::create_dir_all(repo.join("testdata/private")).unwrap();
        fs::write(repo.join("testdata/private/x.txt"), "hello\n").unwrap();
        fs::write(repo.join("fine.md"), "nothing\n").unwrap();
        assert!(git_ok(&repo, &["add", "-A", "-f"]));
        assert!(git_ok(&repo, &["commit", "-q", "-m", "first"]));
        // Commit 2: everything removed again (it must stay found), a marker in the message.
        assert!(git_ok(&repo, &["rm", "-q", "-r", "notes.md", "testdata"]));
        assert!(git_ok(
            &repo,
            &["commit", "-q", "-m", &format!("remove things {marker}")]
        ));

        let report = scan(&repo, Some(&private)).unwrap();
        let has = |path: &str, kind: &str| {
            report
                .findings
                .iter()
                .any(|f| f.path == path && f.kind.starts_with(kind))
        };
        assert!(has("notes.md", "e-mail address"), "{:?}", report.findings);
        assert!(has("testdata/private/x.txt", "forbidden path"));
        assert!(has("(commit message)", "private marker"));
        assert!(!report.findings.iter().any(|f| f.path == "fine.md"));
        // The e-mail finding names the first commit, with a masked value.
        let found = report
            .findings
            .iter()
            .find(|f| f.path == "notes.md" && f.kind == "e-mail address")
            .unwrap();
        assert_eq!(found.masked, "som…");
        assert_eq!(found.commit.len(), 40);
        assert_eq!(report.commits, 2);
        assert_eq!(report.markers, Some(1));
        let identity = &report.identities["test@example.com"];
        assert_eq!((identity.authored, identity.committed), (2, 2));

        // Without the private folder the marker part is skipped, the rest still works.
        let report = scan(&repo, None).unwrap();
        assert_eq!(report.markers, None);
        assert!(!has_kind(&report, "private marker"));
        assert!(has_kind(&report, "e-mail address"));
    }

    fn has_kind(report: &Report, kind: &str) -> bool {
        report.findings.iter().any(|f| f.kind == kind)
    }
}
