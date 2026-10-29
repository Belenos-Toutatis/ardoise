//! `cargo xtask dist [--reuse]`: the distribution archive `target/dist/ardoise-<version>-windows-x64.zip`
//! (SPEC §9, ROADMAP M10). It holds, under one folder `ardoise-<version>/`:
//!
//! * `ardoise.exe` (built like `check` does, with `--features custom-protocol`, unless `--reuse`
//!   says to take the one already in `target/release`);
//! * `LICENSE` and `THIRD-PARTY-NOTICES`;
//! * `LISEZMOI.txt`, French then English: what each file is and how to start;
//! * `guide/*.html`: every `docs/guide/*.md` as a standalone page (`pulldown-cmark`, tables on,
//!   links between guide pages rewritten from `.md` to `.html`, headings given GitHub-style
//!   anchors, one small embedded stylesheet that also reads well on paper).
//!
//! The archive is deterministic: entries sorted by path, fixed timestamps, fixed permissions, so
//! the same inputs give the same bytes (and the same SHA-256, which is printed with the size).
//! Nothing is uploaded or published.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use pulldown_cmark::{CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

/// One file of the archive: its path inside the folder, its bytes, whether it is a program.
#[derive(Debug, PartialEq)]
struct Entry {
    path: String,
    bytes: Vec<u8>,
    executable: bool,
}

/// Stylesheet of the guide pages: system fonts, a readable column, light and dark, and a print
/// version (black on white, links spelled out, nothing split in the middle of a table row).
const GUIDE_CSS: &str = "\
:root { color-scheme: light dark; }
body { max-width: 46rem; margin: 2rem auto; padding: 0 1rem; font: 1rem/1.6 system-ui, \"Segoe UI\", sans-serif; }
h1, h2, h3, h4 { line-height: 1.25; margin: 1.8em 0 0.5em; }
h1 { margin-top: 0; }
a { color: #1f5fbf; }
code, pre { font-family: Consolas, \"Cascadia Mono\", monospace; font-size: 0.92em; }
code { padding: 0.1em 0.3em; border-radius: 3px; background: rgb(127 127 127 / 18%); }
pre { padding: 0.7em 0.9em; overflow-x: auto; border-radius: 4px; background: rgb(127 127 127 / 14%); }
pre code { padding: 0; background: none; }
blockquote { margin: 1em 0; padding: 0.1em 1em; border-left: 4px solid rgb(127 127 127 / 50%); }
table { border-collapse: collapse; margin: 1em 0; }
th, td { padding: 0.35em 0.7em; border: 1px solid rgb(127 127 127 / 55%); text-align: left; vertical-align: top; }
img { max-width: 100%; }
@media (prefers-color-scheme: dark) { a { color: #8ab4f8; } }
@media print {
  body { max-width: none; margin: 0; color: #000; background: #fff; font-size: 11pt; }
  a { color: #000; text-decoration: underline; }
  pre { white-space: pre-wrap; overflow: visible; }
  h1, h2, h3, h4 { break-after: avoid; }
  tr, pre, blockquote, img { break-inside: avoid; }
}
";

/// Timestamp of every entry (the earliest the zip format can hold).
fn fixed_time() -> DateTime {
    DateTime::default()
}

// ---------------------------------------------------------------------------------------------
// Markdown to HTML
// ---------------------------------------------------------------------------------------------

/// Escapes the characters that matter in HTML text and attribute values.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The anchor GitHub gives a heading: lower case, letters and digits kept, spaces and hyphens
/// become hyphens, everything else dropped. `used` makes repeats unique (`-1`, `-2`…).
fn slug(text: &str, used: &mut BTreeSet<String>) -> String {
    let base: String = text
        .chars()
        .filter_map(|c| match c {
            c if c.is_alphanumeric() || c == '_' => Some(c.to_lowercase().next().unwrap_or(c)),
            ' ' | '-' => Some('-'),
            _ => None,
        })
        .collect();
    let mut candidate = base.clone();
    let mut n = 0;
    while !used.insert(candidate.clone()) {
        n += 1;
        candidate = format!("{base}-{n}");
    }
    candidate
}

/// A link to another page of the guide (`other.fr.md`, `other.fr.md#part`) points at its HTML
/// page. Anything else (web address, anchor, a file outside the guide) is left alone; the second
/// result says the link looks like a file of the repository that the archive does not hold.
fn rewrite_link(dest: &str, pages: &BTreeSet<String>) -> (String, bool) {
    let is_external = dest.starts_with('#') || dest.contains("://") || dest.starts_with("mailto:");
    if is_external {
        return (dest.to_owned(), false);
    }
    let (path, fragment) = dest
        .split_once('#')
        .map_or((dest, None), |(p, f)| (p, Some(f)));
    let Some(stem) = path.strip_suffix(".md") else {
        return (dest.to_owned(), false);
    };
    if !pages.contains(path) {
        return (dest.to_owned(), true);
    }
    let fragment = fragment.map_or(String::new(), |f| format!("#{f}"));
    (format!("{stem}.html{fragment}"), false)
}

/// A guide page as a standalone HTML document. `pages` are the file names of all the guide's
/// Markdown pages (for the links); the second result lists the links that point outside them.
fn render_page(markdown: &str, file_name: &str, pages: &BTreeSet<String>) -> (String, Vec<String>) {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let mut events: Vec<Event> = Parser::new_ext(markdown, options).collect();
    let mut used = BTreeSet::new();
    let mut title = None;
    let mut dangling = Vec::new();

    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::Heading { .. }) => {
                let mut text = String::new();
                let mut j = i + 1;
                while j < events.len() && !matches!(events[j], Event::End(TagEnd::Heading(_))) {
                    if let Event::Text(t) | Event::Code(t) = &events[j] {
                        text.push_str(t);
                    }
                    j += 1;
                }
                let id = slug(&text, &mut used);
                if let Event::Start(Tag::Heading {
                    level,
                    id: heading_id,
                    ..
                }) = &mut events[i]
                {
                    if title.is_none() && *level == HeadingLevel::H1 {
                        title = Some(text.clone());
                    }
                    *heading_id = Some(CowStr::from(id));
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let (rewritten, dangles) = rewrite_link(dest_url, pages);
                if dangles {
                    dangling.push(dest_url.to_string());
                }
                if let Event::Start(Tag::Link { dest_url, .. }) = &mut events[i] {
                    *dest_url = CowStr::from(rewritten);
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    let title = title.unwrap_or_else(|| file_name.trim_end_matches(".md").to_owned());
    let lang = if file_name.contains(".fr.") {
        "fr"
    } else {
        "en"
    };
    let page = format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>\n{GUIDE_CSS}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        escape(&title)
    );
    (page, dangling)
}

// ---------------------------------------------------------------------------------------------
// LISEZMOI.txt
// ---------------------------------------------------------------------------------------------

/// The text of `LISEZMOI.txt`, French then English. `guide_pages` are the names of the HTML
/// pages in `guide/`; each language lists the ones written in it.
fn readme(version: &str, guide_pages: &[String]) -> String {
    let list = |marker: &str| -> String {
        let pages: Vec<&String> = guide_pages.iter().filter(|p| p.contains(marker)).collect();
        pages.iter().map(|p| format!("    guide\\{p}\n")).collect()
    };
    let rule = "=".repeat(70);
    format!(
        "\
ARDOISE {version}
{rule}

FRANÇAIS
--------

Ardoise remplace Wacom Inkspace pour la tablette Bamboo Slate : elle reçoit
vos pages, les range dans une bibliothèque (un dossier, par exemple dans
Nextcloud) et les exporte.

Contenu de ce dossier :
    ardoise.exe           le programme (il n'y a rien à installer)
    LICENSE               la licence d'Ardoise, GNU GPL version 3
    THIRD-PARTY-NOTICES   les composants libres utilisés et leurs licences
    LISEZMOI.txt          ce fichier
    guide\\                le guide, à ouvrir dans un navigateur :
{fr}
Pour commencer : placez ce dossier où vous voulez (disque, clé USB, dossier
synchronisé), puis double-cliquez sur ardoise.exe. Au premier lancement,
Ardoise demande de choisir le dossier de la bibliothèque. Windows 11 est
requis (Windows 10 doit fonctionner, avec le composant WebView2).

Ardoise est un logiciel libre, fourni sans aucune garantie.

ENGLISH
-------

Ardoise replaces Wacom Inkspace for the Bamboo Slate tablet: it receives your
pages, keeps them in a library (a folder, for example in Nextcloud) and
exports them.

What is in this folder:
    ardoise.exe           the program (nothing to install)
    LICENSE               Ardoise's licence, GNU GPL version 3
    THIRD-PARTY-NOTICES   the free components used and their licences
    LISEZMOI.txt          this file
    guide\\                the guide, to open in a web browser:
{en}
To start: put this folder anywhere you like (disk, USB stick, synchronised
folder), then double-click ardoise.exe. On first launch Ardoise asks you to
choose the library folder. Windows 11 is required (Windows 10 should work,
with the WebView2 component).

Ardoise is free software, provided without any warranty.
",
        fr = list(".fr."),
        en = list(".en."),
    )
}

// ---------------------------------------------------------------------------------------------
// The archive
// ---------------------------------------------------------------------------------------------

/// `version` of `[workspace.package]` in the root manifest.
fn workspace_version(manifest: &str) -> Option<String> {
    let section = manifest.split("[workspace.package]").nth(1)?;
    let section = section.split("\n[").next()?;
    section.lines().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix("version")?
            .trim_start()
            .strip_prefix('=')?;
        Some(value.trim().trim_matches('"').to_owned())
    })
}

/// Everything that goes into the archive, sorted by path. `exe` is the program to ship; `root`
/// holds `LICENSE`, `THIRD-PARTY-NOTICES` and `docs/guide`. Problems that do not stop the build
/// (a link to a file the archive lacks) are returned as warnings.
fn collect(root: &Path, exe: &Path, version: &str) -> Result<(Vec<Entry>, Vec<String>), String> {
    let read = |path: &Path| -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    let folder = format!("ardoise-{version}");
    let mut entries = vec![
        Entry {
            path: format!("{folder}/ardoise.exe"),
            bytes: read(exe)?,
            executable: true,
        },
        Entry {
            path: format!("{folder}/LICENSE"),
            bytes: read(&root.join("LICENSE"))?,
            executable: false,
        },
        Entry {
            path: format!("{folder}/THIRD-PARTY-NOTICES"),
            bytes: read(&root.join("THIRD-PARTY-NOTICES"))?,
            executable: false,
        },
    ];

    let guide_dir = root.join("docs").join("guide");
    let mut sources: Vec<(String, String)> = Vec::new();
    for entry in fs::read_dir(&guide_dir)
        .map_err(|e| format!("cannot read {}: {e}", guide_dir.display()))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && entry.path().is_file() {
            let text = String::from_utf8(read(&entry.path())?)
                .map_err(|e| format!("{name} is not UTF-8: {e}"))?;
            sources.push((name, text));
        }
    }
    sources.sort();
    if sources.is_empty() {
        return Err(format!("no guide page (*.md) in {}", guide_dir.display()));
    }
    let pages: BTreeSet<String> = sources.iter().map(|(n, _)| n.clone()).collect();
    let mut warnings = Vec::new();
    let mut html_names = Vec::new();
    for (name, markdown) in &sources {
        let (page, dangling) = render_page(&markdown.replace("\r\n", "\n"), name, &pages);
        warnings.extend(
            dangling
                .into_iter()
                .map(|link| format!("{name}: the link {link} points outside the guide")),
        );
        let html_name = format!("{}.html", name.trim_end_matches(".md"));
        entries.push(Entry {
            path: format!("{folder}/guide/{html_name}"),
            bytes: page.into_bytes(),
            executable: false,
        });
        html_names.push(html_name);
    }
    entries.push(Entry {
        path: format!("{folder}/LISEZMOI.txt"),
        // CRLF: the file is meant for Notepad.
        bytes: readme(version, &html_names)
            .replace('\n', "\r\n")
            .into_bytes(),
        executable: false,
    });
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((entries, warnings))
}

/// The zip file holding `entries` (already sorted): deflated, fixed timestamps and permissions.
fn build_archive(entries: &[Entry]) -> Result<Vec<u8>, String> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for entry in entries {
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(fixed_time())
            .unix_permissions(if entry.executable { 0o755 } else { 0o644 });
        zip.start_file(&entry.path, options)
            .map_err(|e| format!("cannot add {}: {e}", entry.path))?;
        zip.write_all(&entry.bytes)
            .map_err(|e| format!("cannot add {}: {e}", entry.path))?;
    }
    let cursor = zip
        .finish()
        .map_err(|e| format!("cannot finish the archive: {e}"))?;
    Ok(cursor.into_inner())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Builds the archive (and the executable first, unless `reuse`). Prints `DIST OK` and returns
/// true on success.
pub fn run(root: &Path, reuse: bool) -> bool {
    match run_inner(root, reuse) {
        Ok(()) => {
            println!("DIST OK");
            true
        }
        Err(e) => {
            eprintln!("{e}");
            println!("DIST FAILED");
            false
        }
    }
}

fn run_inner(root: &Path, reuse: bool) -> Result<(), String> {
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    let version = workspace_version(&manifest).ok_or("no version in [workspace.package]")?;
    if !crate::licenses::run(root, false) {
        return Err("THIRD-PARTY-NOTICES is out of date or a licence is refused".into());
    }
    if !reuse
        && !crate::run(
            root,
            "cargo",
            &[
                "build",
                "--release",
                "-p",
                "ardoise",
                "--features",
                "custom-protocol",
            ],
        )
    {
        return Err("the release build failed".into());
    }
    let exe: PathBuf = root.join("target").join("release").join("ardoise.exe");
    let size = fs::metadata(&exe)
        .map_err(|_| {
            format!(
                "{} not found: run `cargo xtask dist` without --reuse",
                exe.display()
            )
        })?
        .len();
    if size >= crate::MAX_EXE_BYTES {
        return Err("ardoise.exe is 30 MB or larger".into());
    }

    let (entries, warnings) = collect(root, &exe, &version)?;
    for warning in &warnings {
        println!("warning: {warning}");
    }
    let archive = build_archive(&entries)?;
    let out_dir = root.join("target").join("dist");
    fs::create_dir_all(&out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let out = out_dir.join(format!("ardoise-{version}-windows-x64.zip"));
    fs::write(&out, &archive).map_err(|e| format!("cannot write {}: {e}", out.display()))?;

    for entry in &entries {
        println!("  {:>10}  {}", entry.bytes.len(), entry.path);
    }
    println!("archive: {}", out.display());
    println!(
        "size: {} bytes ({:.1} MB)",
        archive.len(),
        archive.len() as f64 / (1024.0 * 1024.0)
    );
    println!("sha256: {}", sha256_hex(&archive));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A throwaway tree: a fake executable, the licence files and a guide of two pages.
    struct Tree(PathBuf);

    impl Tree {
        fn new(tag: &str) -> Tree {
            let dir =
                std::env::temp_dir().join(format!("ardoise-dist-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("docs/guide")).unwrap();
            fs::write(dir.join("fake-ardoise.exe"), b"MZ fake program").unwrap();
            fs::write(
                dir.join("LICENSE"),
                "GNU GENERAL PUBLIC LICENSE\r\nVersion 3\r\n",
            )
            .unwrap();
            fs::write(dir.join("THIRD-PARTY-NOTICES"), "THIRD-PARTY NOTICES\n").unwrap();
            fs::write(
                dir.join("docs/guide/guide.fr.md"),
                "# Guide d'Ardoise\n\nVoir l'[assistant](assistant.fr.md#installer) et [le site](https://example.org/a.md).\n\n\
                 | A | B |\n|---|---|\n| 1 | 2 |\n\n## Installer\n\n## Installer\n\n[hors guide](../../SPEC.md)\n",
            )
            .unwrap();
            fs::write(
                dir.join("docs/guide/assistant.fr.md"),
                "# L'assistant\n\n## Installer\n",
            )
            .unwrap();
            fs::write(dir.join("docs/guide/guide.en.md"), "# Ardoise guide\n").unwrap();
            fs::write(dir.join("docs/guide/notes.txt"), "not a guide page").unwrap();
            Tree(dir)
        }

        fn collect(&self) -> (Vec<Entry>, Vec<String>) {
            collect(&self.0, &self.0.join("fake-ardoise.exe"), "1.2.3").unwrap()
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_archive_holds_the_program_the_licences_the_readme_and_the_guides_sorted() {
        let tree = Tree::new("contents");
        let (entries, _) = tree.collect();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "ardoise-1.2.3/LICENSE",
                "ardoise-1.2.3/LISEZMOI.txt",
                "ardoise-1.2.3/THIRD-PARTY-NOTICES",
                "ardoise-1.2.3/ardoise.exe",
                "ardoise-1.2.3/guide/assistant.fr.html",
                "ardoise-1.2.3/guide/guide.en.html",
                "ardoise-1.2.3/guide/guide.fr.html",
            ]
        );
        let exe = entries
            .iter()
            .find(|e| e.path.ends_with("ardoise.exe"))
            .unwrap();
        assert_eq!(exe.bytes, b"MZ fake program");
        assert!(exe.executable);
    }

    #[test]
    fn the_zip_reads_back_and_is_the_same_every_time() {
        let tree = Tree::new("zip");
        let (entries, _) = tree.collect();
        let first = build_archive(&entries).unwrap();
        let second = build_archive(&tree.collect().0).unwrap();
        assert_eq!(first, second);
        assert_eq!(sha256_hex(&first), sha256_hex(&second));

        let mut zip = zip::ZipArchive::new(Cursor::new(first)).unwrap();
        assert_eq!(zip.len(), entries.len());
        let mut exe = Vec::new();
        {
            let mut file = zip.by_name("ardoise-1.2.3/ardoise.exe").unwrap();
            assert_eq!(file.last_modified(), Some(DateTime::default()));
            file.read_to_end(&mut exe).unwrap();
        }
        assert_eq!(exe, b"MZ fake program");
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn guide_pages_are_standalone_html_with_tables_anchors_and_page_links() {
        let tree = Tree::new("html");
        let (entries, warnings) = tree.collect();
        let page = entries
            .iter()
            .find(|e| e.path.ends_with("guide/guide.fr.html"))
            .unwrap();
        let html = String::from_utf8(page.bytes.clone()).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<html lang=\"fr\">") && html.contains("<meta charset=\"utf-8\">"));
        assert!(html.contains("<title>Guide d'Ardoise</title>"));
        assert!(html.contains("<style>") && html.contains("@media print"));
        assert!(html.contains("<table>") && html.contains("<td>2</td>"));
        // Pages of the guide link to each other as HTML, fragment kept; the web is untouched.
        assert!(html.contains("href=\"assistant.fr.html#installer\""));
        assert!(html.contains("href=\"https://example.org/a.md\""));
        // Headings carry GitHub-style anchors, repeats numbered.
        assert!(html.contains("<h2 id=\"installer\">") && html.contains("<h2 id=\"installer-1\">"));
        assert!(html.contains("<h1 id=\"guide-dardoise\">"));
        // A link to a repository file the archive lacks is reported, and left as it is.
        assert_eq!(
            warnings,
            ["guide.fr.md: the link ../../SPEC.md points outside the guide"]
        );
        assert!(html.contains("href=\"../../SPEC.md\""));
        assert!(
            entries
                .iter()
                .any(|e| e.path.ends_with("guide/guide.en.html")
                    && String::from_utf8_lossy(&e.bytes).contains("<html lang=\"en\">"))
        );
    }

    #[test]
    fn link_rewriting_only_touches_guide_pages() {
        let pages: BTreeSet<String> = ["a.fr.md".to_owned()].into();
        assert_eq!(rewrite_link("a.fr.md", &pages), ("a.fr.html".into(), false));
        assert_eq!(
            rewrite_link("a.fr.md#x", &pages),
            ("a.fr.html#x".into(), false)
        );
        assert_eq!(rewrite_link("#x", &pages), ("#x".into(), false));
        assert_eq!(rewrite_link("b.md", &pages), ("b.md".into(), true));
        assert_eq!(
            rewrite_link("image.png", &pages),
            ("image.png".into(), false)
        );
        assert_eq!(
            rewrite_link("mailto:a.md", &pages),
            ("mailto:a.md".into(), false)
        );
    }

    #[test]
    fn the_readme_says_what_each_file_is_in_both_languages() {
        let text = readme(
            "1.2.3",
            &[
                "guide.fr.html".into(),
                "guide.en.html".into(),
                "assistant.en.html".into(),
            ],
        );
        let (french, english) = text.split_once("ENGLISH\n").unwrap();
        for part in [french, english] {
            for file in [
                "ardoise.exe",
                "LICENSE",
                "THIRD-PARTY-NOTICES",
                "LISEZMOI.txt",
            ] {
                assert!(part.contains(file), "{file}");
            }
        }
        assert!(french.contains("guide\\guide.fr.html") && !french.contains(".en.html"));
        assert!(english.contains("guide\\assistant.en.html") && !english.contains(".fr.html"));
        assert!(french.contains("double-cliquez sur ardoise.exe"));
        assert!(english.contains("double-click ardoise.exe"));
        assert!(text.starts_with("ARDOISE 1.2.3"));
    }

    #[test]
    fn the_readme_uses_windows_line_endings_in_the_archive() {
        let tree = Tree::new("readme");
        let (entries, _) = tree.collect();
        let readme = entries
            .iter()
            .find(|e| e.path.ends_with("LISEZMOI.txt"))
            .unwrap();
        let text = String::from_utf8(readme.bytes.clone()).unwrap();
        assert!(text.contains("\r\n") && !text.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn missing_inputs_are_reported() {
        let tree = Tree::new("missing");
        assert!(
            collect(&tree.0, &tree.0.join("absent.exe"), "1.0.0")
                .unwrap_err()
                .contains("absent.exe")
        );
        fs::remove_file(tree.0.join("LICENSE")).unwrap();
        assert!(
            collect(&tree.0, &tree.0.join("fake-ardoise.exe"), "1.0.0")
                .unwrap_err()
                .contains("LICENSE")
        );
    }

    #[test]
    fn the_version_comes_from_the_workspace_manifest() {
        let manifest = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.4.1\"\nedition = \"2024\"\n\n[profile.release]\nversion = \"9\"\n";
        assert_eq!(workspace_version(manifest).as_deref(), Some("0.4.1"));
        assert_eq!(workspace_version("[package]\nversion = \"1\"\n"), None);
    }
}
