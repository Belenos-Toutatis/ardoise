//! `cargo xtask licenses [--write]`: checks the licence of every dependency (Rust and npm)
//! against an allowlist of GPL-3.0-compatible licences (SPEC §9) and keeps the committed
//! `THIRD-PARTY-NOTICES` file up to date.
//!
//! * Check: every package of the resolved graph (all platforms, dev and build dependencies
//!   included) must have a licence expression that is satisfiable with allowed licences.
//! * Notices: what ships inside `ardoise.exe` (normal Rust dependencies of the `ardoise` package
//!   for the Windows target, npm packages bundled into the interface) with their licence texts.
//!   The output is deterministic (sorted, no paths, no dates, LF line endings), so it is
//!   identical on Windows and Linux.
//!
//! Without `--write`, the notices file is compared with what would be generated and the command
//! fails when it is out of date.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Licences accepted in a dependency, all compatible with distributing the combined work under
/// GPL-3.0-or-later (the licence of Ardoise, SPEC §9).
///
/// * Permissive licences (MIT, MIT-0, BSD-2/3-Clause, ISC, Zlib, 0BSD, Unlicense, CC0-1.0,
///   BSL-1.0, Unicode-3.0, Unicode-DFS-2016, BlueOak-1.0.0): no copyleft clause; the FSF lists
///   them as GPL-compatible (BlueOak is the same kind of permissive text, only npm dev tools
///   use it today).
/// * Apache-2.0: compatible with GPL-3.0 (not with GPL-2.0-only), one way.
/// * MPL-2.0: compatible with GPL-3.0 through its "Secondary License" clause (MPL section 3.3).
/// * LGPL-3.0 and LGPL-2.1-or-later: may be combined with and relicensed under GPL-3.0. Plain
///   LGPL-2.1-only is deliberately absent (it converts to GPL-2.0 only).
/// * GPL-3.0-only/-or-later and GPL-2.0-or-later: same or higher-version copyleft we can follow.
///
/// Absent on purpose: every AGPL, GPL-2.0-only, SSPL, OpenSSL, Apache-1.x, proprietary and
/// unknown licences.
const ALLOWED_LICENSES: &[&str] = &[
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "BlueOak-1.0.0",
    "CC0-1.0",
    "GPL-2.0-or-later",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "ISC",
    "LGPL-2.1-or-later",
    "LGPL-3.0-only",
    "LGPL-3.0-or-later",
    "MIT",
    "MIT-0",
    "MPL-2.0",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Unlicense",
    "Zlib",
];

/// SPDX exceptions accepted after `WITH` (they only add permissions).
const ALLOWED_EXCEPTIONS: &[&str] = &["LLVM-exception"];

/// Packages accepted although their licence is not in the allowlist: (ecosystem, name, reason).
/// Empty: no real case needs an exception today. Add an entry only with a written justification.
const EXCEPTIONS: &[(&str, &str, &str)] = &[];

/// Workspace packages whose normal dependencies end up in the release, with the target they are
/// built for: the executable, and the WebAssembly module embedded in the interface.
const SHIPPED_ROOTS: &[(&str, &str)] = &[
    ("ardoise", "x86_64-pc-windows-msvc"),
    ("ardoise-ink-wasm", "wasm32-unknown-unknown"),
];
/// npm packages bundled into the interface besides `dependencies` (the Svelte runtime is a
/// build-time dependency of the app but its runtime ends up in the bundle).
const EXTRA_NPM_ROOTS: &[&str] = &["svelte"];

const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES";

// ---------------------------------------------------------------------------------------------
// SPDX licence expressions
// ---------------------------------------------------------------------------------------------

/// A parsed SPDX licence expression.
#[derive(Debug, PartialEq)]
enum Expr {
    License {
        id: String,
        exception: Option<String>,
    },
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

#[derive(Debug, PartialEq)]
enum Token {
    Open,
    Close,
    And,
    Or,
    With,
    Id(String),
}

fn tokenize(s: &str) -> Vec<Token> {
    fn flush(word: &mut String, out: &mut Vec<Token>) {
        if word.is_empty() {
            return;
        }
        out.push(match word.to_ascii_uppercase().as_str() {
            "AND" => Token::And,
            "OR" => Token::Or,
            "WITH" => Token::With,
            _ => Token::Id(std::mem::take(word)),
        });
        word.clear();
    }
    let mut out = Vec::new();
    let mut word = String::new();
    for c in s.chars() {
        match c {
            '(' | ')' | '/' => {
                flush(&mut word, &mut out);
                // `/` is the legacy separator some crates use for "or".
                out.push(match c {
                    '(' => Token::Open,
                    ')' => Token::Close,
                    _ => Token::Or,
                });
            }
            c if c.is_whitespace() => flush(&mut word, &mut out),
            c => word.push(c),
        }
    }
    flush(&mut word, &mut out);
    out
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// expr := and ("OR" and)*
    fn expr(&mut self) -> Result<Expr, String> {
        let mut items = vec![self.and()?];
        while self.peek() == Some(&Token::Or) {
            self.pos += 1;
            items.push(self.and()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expr::Or(items)
        })
    }

    /// and := atom ("AND" atom)*
    fn and(&mut self) -> Result<Expr, String> {
        let mut items = vec![self.atom()?];
        while self.peek() == Some(&Token::And) {
            self.pos += 1;
            items.push(self.atom()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expr::And(items)
        })
    }

    /// atom := "(" expr ")" | id ["WITH" exception]
    fn atom(&mut self) -> Result<Expr, String> {
        match self.tokens.get(self.pos) {
            Some(Token::Open) => {
                self.pos += 1;
                let e = self.expr()?;
                if self.peek() != Some(&Token::Close) {
                    return Err("missing closing parenthesis".into());
                }
                self.pos += 1;
                Ok(e)
            }
            Some(Token::Id(id)) => {
                let id = id.clone();
                self.pos += 1;
                let mut exception = None;
                if self.peek() == Some(&Token::With) {
                    self.pos += 1;
                    match self.tokens.get(self.pos) {
                        Some(Token::Id(e)) => {
                            exception = Some(e.clone());
                            self.pos += 1;
                        }
                        _ => return Err("missing exception after WITH".into()),
                    }
                }
                Ok(Expr::License { id, exception })
            }
            Some(t) => Err(format!("unexpected {t:?}")),
            None => Err("unexpected end of expression".into()),
        }
    }
}

/// Parses an SPDX licence expression (`OR`, `AND`, parentheses, `WITH`, legacy `/`).
fn parse_expr(s: &str) -> Result<Expr, String> {
    let mut parser = Parser {
        tokens: tokenize(s),
        pos: 0,
    };
    let e = parser.expr()?;
    if parser.pos != parser.tokens.len() {
        return Err("trailing tokens".into());
    }
    Ok(e)
}

/// True if `id` (with optional exception) is in the allowlist. `GPL-2.0+` style identifiers are
/// read as `GPL-2.0-or-later`.
fn license_allowed(id: &str, exception: Option<&str>) -> bool {
    let id = match id.strip_suffix('+') {
        Some(base) => format!("{}-or-later", base.trim_end_matches("-only")),
        None => id.to_string(),
    };
    ALLOWED_LICENSES.contains(&id.as_str())
        && exception.is_none_or(|e| ALLOWED_EXCEPTIONS.contains(&e))
}

impl Expr {
    /// `OR`: one acceptable alternative is enough; `AND`: all must be acceptable.
    fn accepted(&self) -> bool {
        match self {
            Expr::License { id, exception } => license_allowed(id, exception.as_deref()),
            Expr::And(items) => items.iter().all(Expr::accepted),
            Expr::Or(items) => items.iter().any(Expr::accepted),
        }
    }
}

/// Why a package is refused, or `None` if it is accepted.
fn refusal(license: Option<&str>) -> Option<String> {
    let Some(license) = license else {
        return Some("no `license` field".into());
    };
    match parse_expr(license) {
        Ok(e) if e.accepted() => None,
        Ok(_) => Some("licence not in the GPL-3.0-compatible allowlist".into()),
        Err(e) => Some(format!("cannot parse the licence expression ({e})")),
    }
}

// ---------------------------------------------------------------------------------------------
// Packages
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Ecosystem {
    Rust,
    Npm,
}

impl Ecosystem {
    fn label(self) -> &'static str {
        match self {
            Ecosystem::Rust => "rust",
            Ecosystem::Npm => "npm",
        }
    }
}

struct Pkg {
    eco: Ecosystem,
    name: String,
    version: String,
    license: Option<String>,
    /// `license_file` of a Rust manifest, shown when the `license` field is missing.
    license_file: Option<String>,
    repository: Option<String>,
    /// Folder holding the package source (and its licence files).
    dir: PathBuf,
}

impl Pkg {
    fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Runs `cargo metadata` in `root`.
fn cargo_metadata(root: &Path) -> Result<Value, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(["metadata", "--format-version", "1", "--locked"]);
    let out = cmd
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cannot read cargo metadata: {e}"))
}

fn rust_pkg(p: &Value) -> Pkg {
    let dir = str_of(p, "manifest_path")
        .map(Path::new)
        .and_then(Path::parent)
        .map_or_else(PathBuf::new, Path::to_path_buf);
    Pkg {
        eco: Ecosystem::Rust,
        name: str_of(p, "name").unwrap_or_default().to_string(),
        version: str_of(p, "version").unwrap_or_default().to_string(),
        license: str_of(p, "license").map(str::to_string),
        license_file: str_of(p, "license_file").map(str::to_string),
        repository: str_of(p, "repository")
            .or_else(|| str_of(p, "homepage"))
            .map(clean_url),
        dir,
    }
}

/// Rust packages: every third-party package of the graph, and the ones shipped in the release.
fn rust_packages(root: &Path) -> Result<(Vec<Pkg>, Vec<Pkg>), String> {
    let meta = cargo_metadata(root)?;
    let members: BTreeSet<&str> = meta["workspace_members"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let all: Vec<&Value> = meta["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?
        .iter()
        .collect();
    let (ours, third_party): (Vec<&Value>, Vec<&Value>) = all
        .iter()
        .partition(|p| str_of(p, "id").is_some_and(|id| members.contains(id)));
    let member_names: BTreeSet<&str> = ours.iter().filter_map(|p| str_of(p, "name")).collect();

    let mut shipped_keys = BTreeSet::new();
    for (package, target) in SHIPPED_ROOTS {
        shipped_keys.extend(shipped_rust_packages(root, package, target, &member_names)?);
    }
    let mut shipped = Vec::new();
    for (name, version) in &shipped_keys {
        let found = third_party
            .iter()
            .find(|p| str_of(p, "name") == Some(name) && str_of(p, "version") == Some(version))
            .ok_or_else(|| {
                format!("cargo tree lists {name} {version}, unknown to cargo metadata")
            })?;
        shipped.push(rust_pkg(found));
    }
    Ok((third_party.into_iter().map(rust_pkg).collect(), shipped))
}

/// (name, version) of the third-party packages that `package` needs at run time for `target`:
/// normal dependencies only (no dev, no build-only) and no procedural macros, which run at
/// compile time and are not part of the executable. `cargo tree` applies the resolver's real
/// feature rules (features of build dependencies and proc macros are not unified with the
/// program's), which `cargo metadata`'s single resolve graph does not.
fn shipped_rust_packages(
    root: &Path,
    package: &str,
    target: &str,
    member_names: &BTreeSet<&str>,
) -> Result<BTreeSet<(String, String)>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["tree", "--locked", "-p", package, "--target", target])
        .args([
            "-e",
            "normal,no-proc-macro",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo tree: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo tree failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse_cargo_tree(
        &String::from_utf8_lossy(&out.stdout),
        member_names,
    ))
}

/// Parses `cargo tree --prefix none --format "{p}"` lines (`name v1.2.3`, with a `(path)` suffix
/// for local packages and ` (*)` on repeated subtrees) into (name, version) pairs, leaving out
/// the workspace's own packages.
fn parse_cargo_tree(text: &str, member_names: &BTreeSet<&str>) -> BTreeSet<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (name, version) = (words.next()?, words.next()?.strip_prefix('v')?);
            (!member_names.contains(name)).then(|| (name.to_string(), version.to_string()))
        })
        .collect()
}

/// Key of the parent `node_modules` level of a lockfile path (Node's resolution walks upwards).
fn npm_parent(key: &str) -> &str {
    key.rfind("/node_modules/").map_or("", |i| &key[..i])
}

/// Lockfile key of the package `dep` required by the package at `key`, using Node's lookup
/// order: `key/node_modules/dep`, then each enclosing level.
fn npm_resolve(packages: &serde_json::Map<String, Value>, key: &str, dep: &str) -> Option<String> {
    let mut base = key;
    loop {
        let candidate = if base.is_empty() {
            format!("node_modules/{dep}")
        } else {
            format!("{base}/node_modules/{dep}")
        };
        if packages.contains_key(&candidate) {
            return Some(candidate);
        }
        if base.is_empty() {
            return None;
        }
        base = npm_parent(base);
    }
}

/// Package name of a lockfile key (`node_modules/a/node_modules/@s/b` is `@s/b`).
fn npm_name(key: &str) -> &str {
    key.rsplit_once("node_modules/").map_or(key, |(_, n)| n)
}

fn npm_pkg(app: &Path, key: &str, entry: &Value) -> Pkg {
    let license = match entry.get("license") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => Some(other.to_string()),
    };
    Pkg {
        eco: Ecosystem::Npm,
        name: npm_name(key).to_string(),
        version: str_of(entry, "version").unwrap_or_default().to_string(),
        license,
        license_file: None,
        repository: None,
        dir: app.join(key),
    }
}

/// Repository URL from a `package.json` (string or object form, GitHub shorthand included).
fn npm_repository(package_json: &Value) -> Option<String> {
    let repo = package_json.get("repository")?;
    let url = repo.as_str().or_else(|| str_of(repo, "url"))?;
    Some(clean_url(url)).filter(|u| !u.is_empty())
}

/// Normalizes a repository URL: no `git+` prefix, no `.git` suffix, https for git URLs and
/// `owner/repo` shorthands.
fn clean_url(url: &str) -> String {
    let mut u = url.trim().to_string();
    if let Some(rest) = u.strip_prefix("git+") {
        u = rest.to_string();
    }
    if let Some(rest) = u.strip_prefix("git://") {
        u = format!("https://{rest}");
    }
    if let Some(rest) = u.strip_prefix("github:") {
        u = format!("https://github.com/{rest}");
    } else if !u.contains(':') && u.matches('/').count() == 1 && !u.starts_with('/') {
        u = format!("https://github.com/{u}");
    }
    let u = u.trim_end_matches('/');
    u.strip_suffix(".git").unwrap_or(u).to_string()
}

/// npm packages: every package of the lockfile, and the ones bundled into the interface.
fn npm_packages(root: &Path) -> Result<(Vec<Pkg>, Vec<Pkg>), String> {
    let app = root.join("app");
    let read = |name: &str| -> Result<Value, String> {
        let text = fs::read_to_string(app.join(name))
            .map_err(|e| format!("cannot read app/{name}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("cannot parse app/{name}: {e}"))
    };
    let lock = read("package-lock.json")?;
    let manifest = read("package.json")?;
    let packages = lock["packages"]
        .as_object()
        .ok_or("package-lock.json has no `packages` (lockfile version 3 expected)")?;

    let checked = packages
        .iter()
        .filter(|(k, v)| !k.is_empty() && v["link"] != Value::Bool(true))
        .map(|(k, v)| npm_pkg(&app, k, v))
        .collect();

    let mut roots: Vec<String> = manifest["dependencies"]
        .as_object()
        .map(|d| d.keys().cloned().collect())
        .unwrap_or_default();
    roots.extend(EXTRA_NPM_ROOTS.iter().map(|s| (*s).to_string()));
    let mut stack = Vec::new();
    for r in &roots {
        stack.push(
            npm_resolve(packages, "", r)
                .ok_or_else(|| format!("{r} is not in package-lock.json"))?,
        );
    }
    let mut seen: BTreeSet<String> = stack.iter().cloned().collect();
    while let Some(key) = stack.pop() {
        for field in ["dependencies", "optionalDependencies"] {
            for dep in packages[&key][field]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(d, _)| d)
            {
                // An optional dependency may be absent from the lockfile on this platform.
                if let Some(k) = npm_resolve(packages, &key, dep)
                    && seen.insert(k.clone())
                {
                    stack.push(k);
                }
            }
        }
    }
    let mut shipped = Vec::new();
    for key in &seen {
        let mut pkg = npm_pkg(&app, key, &packages[key]);
        if !pkg.dir.is_dir() {
            return Err(format!("app/{key} is missing: run `npm ci` in app/ first"));
        }
        let manifest_path = pkg.dir.join("package.json");
        pkg.repository = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|j| npm_repository(&j));
        shipped.push(pkg);
    }
    Ok((checked, shipped))
}

// ---------------------------------------------------------------------------------------------
// Check
// ---------------------------------------------------------------------------------------------

/// Checks every package; prints the problems and the summary. True if all are accepted.
fn check_packages(rust: &[Pkg], npm: &[Pkg]) -> bool {
    let mut ok = true;
    for pkgs in [rust, npm] {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut problems = Vec::new();
        for p in pkgs {
            if let Some((_, _, reason)) = EXCEPTIONS
                .iter()
                .find(|(eco, name, _)| *eco == p.eco.label() && *name == p.name)
            {
                println!("exception: {} {} ({reason})", p.eco.label(), p.label());
                *counts.entry("(exception)").or_default() += 1;
                continue;
            }
            match refusal(p.license.as_deref()) {
                None => {
                    *counts
                        .entry(p.license.as_deref().unwrap_or_default())
                        .or_default() += 1
                }
                Some(reason) => {
                    let shown = match (&p.license, &p.license_file) {
                        (Some(l), _) => format!("licence `{l}`"),
                        (None, Some(f)) => {
                            format!("no licence expression, license_file = {f}: decide by hand")
                        }
                        (None, None) => "no licence information".to_string(),
                    };
                    problems.push(format!(
                        "{} package {}: {shown} -> {reason}",
                        p.eco.label(),
                        p.label()
                    ));
                }
            }
        }
        if let Some(first) = pkgs.first() {
            let mut rows: Vec<_> = counts.iter().collect();
            rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            println!("{} packages checked ({}):", pkgs.len(), first.eco.label());
            for (license, n) in rows {
                println!("  {n:>4}  {license}");
            }
        }
        for p in &problems {
            println!("REFUSED: {p}");
        }
        ok &= problems.is_empty();
    }
    ok
}

// ---------------------------------------------------------------------------------------------
// THIRD-PARTY-NOTICES
// ---------------------------------------------------------------------------------------------

/// Licence-like files at the top of a package folder (`LICENSE*`, `LICENCE*`, `COPYING*`,
/// `NOTICE*`, `UNLICENSE*`), plus the manifest's `license_file`, sorted by name.
fn license_files(pkg: &Pkg) -> Vec<PathBuf> {
    let mut names = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(&pkg.dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let upper = name.to_ascii_uppercase();
            let wanted = ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"]
                .iter()
                .any(|p| upper.starts_with(p));
            if wanted && entry.path().is_file() {
                names.insert(name);
            }
        }
    }
    if let Some(f) = &pkg.license_file
        && pkg.dir.join(f).is_file()
    {
        names.insert(f.clone());
    }
    names.into_iter().map(|n| pkg.dir.join(n)).collect()
}

/// Text with LF line endings, no trailing spaces and no leading or trailing blank lines, so the
/// same licence compares equal whatever its origin.
fn normalize_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let text = text.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_string()
}

fn sorted(mut pkgs: Vec<&Pkg>) -> Vec<&Pkg> {
    pkgs.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    pkgs
}

/// Builds the content of `THIRD-PARTY-NOTICES`.
fn notices(rust: &[Pkg], npm: &[Pkg]) -> Result<String, String> {
    let mut out = String::new();
    let rule = "=".repeat(78);
    let thin = "-".repeat(78);
    let w = |out: &mut String, s: &str| {
        out.push_str(s);
        out.push('\n');
    };
    w(&mut out, "THIRD-PARTY NOTICES");
    w(&mut out, &rule);
    w(&mut out, "");
    w(
        &mut out,
        "Ardoise is free software, distributed under the GNU General Public License,",
    );
    w(
        &mut out,
        "version 3 or (at your option) any later version. It includes the components",
    );
    w(
        &mut out,
        "listed below, each under its own licence, which the licence texts at the end of",
    );
    w(
        &mut out,
        "this file reproduce. This file is generated by `cargo xtask licenses --write`:",
    );
    w(&mut out, "do not edit it by hand.");
    w(&mut out, "");
    w(&mut out, "1. Tuhi");
    w(&mut out, &thin);
    w(&mut out, "");
    w(
        &mut out,
        "The Bamboo Slate Bluetooth protocol was understood thanks to Tuhi, the free",
    );
    w(
        &mut out,
        "Linux driver for Wacom SmartPad devices by Peter Hutterer, Benjamin Tissoires",
    );
    w(
        &mut out,
        "and contributors (https://github.com/tuhiproject/tuhi, GPL-2.0-or-later).",
    );
    w(
        &mut out,
        "Ardoise does not contain Tuhi code: the protocol is a clean-room",
    );
    w(
        &mut out,
        "reimplementation written from our own protocol notes and captures.",
    );
    w(&mut out, "");

    // Packages sorted for the lists; texts grouped across both ecosystems.
    let rust_sorted = sorted(rust.iter().collect());
    let npm_sorted = sorted(npm.iter().collect());
    for (n, title, pkgs) in [
        (2, "Rust packages", &rust_sorted),
        (3, "JavaScript packages (interface bundle)", &npm_sorted),
    ] {
        w(&mut out, &format!("{n}. {title}: {}", pkgs.len()));
        w(&mut out, &thin);
        w(&mut out, "");
        w(&mut out, "name version | licence | repository");
        w(&mut out, "");
        for p in pkgs {
            let _ = writeln!(
                out,
                "{} | {} | {}",
                p.label(),
                p.license.as_deref().unwrap_or("-"),
                p.repository.as_deref().unwrap_or("-")
            );
        }
        w(&mut out, "");
    }

    // Licence texts, deduplicated by identical text.
    let mut texts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for p in rust_sorted.iter().chain(npm_sorted.iter()) {
        let tag = if p.eco == Ecosystem::Npm {
            " (npm)"
        } else {
            ""
        };
        let files = license_files(p);
        if files.is_empty() {
            missing.insert(format!(
                "{}{tag} | {}",
                p.label(),
                p.license.as_deref().unwrap_or("-")
            ));
        }
        for f in files {
            let bytes = fs::read(&f)
                .map_err(|e| format!("cannot read a licence file of {}: {e}", p.label()))?;
            let text = normalize_text(&bytes);
            if !text.is_empty() {
                texts
                    .entry(text)
                    .or_default()
                    .insert(format!("{}{tag}", p.label()));
            }
        }
    }
    let mut groups: Vec<(String, BTreeSet<String>)> = texts.into_iter().collect();
    groups.sort_by(|a, b| {
        a.1.iter()
            .next()
            .cmp(&b.1.iter().next())
            .then(a.0.cmp(&b.0))
    });

    w(&mut out, &format!("4. Licence texts: {}", groups.len()));
    w(&mut out, &thin);
    w(&mut out, "");
    w(
        &mut out,
        "Each distinct text appears once, followed by the packages it covers. A",
    );
    w(
        &mut out,
        "package may be covered by several texts (for example one per licence of a",
    );
    w(
        &mut out,
        "dual-licensed package, or a separate NOTICE file).",
    );
    w(&mut out, "");
    for (i, (text, pkgs)) in groups.iter().enumerate() {
        w(&mut out, &rule);
        w(
            &mut out,
            &format!("Licence text {} of {}", i + 1, groups.len()),
        );
        w(&mut out, &rule);
        w(&mut out, "");
        w(&mut out, "Covers:");
        for p in pkgs {
            w(&mut out, &format!("  - {p}"));
        }
        w(&mut out, "");
        w(&mut out, text);
        w(&mut out, "");
    }

    w(&mut out, &rule);
    w(
        &mut out,
        &format!(
            "5. Packages whose licence text is not shipped by the package: {}",
            missing.len()
        ),
    );
    w(&mut out, &rule);
    w(&mut out, "");
    if missing.is_empty() {
        w(&mut out, "None.");
    } else {
        w(
            &mut out,
            "name version | licence expression (the standard text of the licence applies)",
        );
        w(&mut out, "");
        for m in &missing {
            w(&mut out, m);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------------------------

/// Runs the licence check; with `write`, regenerates `THIRD-PARTY-NOTICES`, otherwise fails if
/// it is out of date. Prints `LICENSES OK` on success. True on success.
pub fn run(root: &Path, write: bool) -> bool {
    match run_inner(root, write) {
        Ok(true) => {
            println!("LICENSES OK");
            true
        }
        Ok(false) => {
            println!("LICENSES FAILED");
            false
        }
        Err(e) => {
            eprintln!("{e}");
            println!("LICENSES FAILED");
            false
        }
    }
}

fn run_inner(root: &Path, write: bool) -> Result<bool, String> {
    let (rust_all, rust_shipped) = rust_packages(root)?;
    let (npm_all, npm_shipped) = npm_packages(root)?;
    if !check_packages(&rust_all, &npm_all) {
        return Ok(false);
    }
    let content = notices(&rust_shipped, &npm_shipped)?;
    let path = root.join(NOTICES_FILE);
    println!(
        "{NOTICES_FILE}: {} Rust and {} npm packages ship in the app",
        rust_shipped.len(),
        npm_shipped.len()
    );
    if write {
        fs::write(&path, &content).map_err(|e| format!("cannot write {NOTICES_FILE}: {e}"))?;
        println!("{NOTICES_FILE} written ({} bytes)", content.len());
        return Ok(true);
    }
    // Git may check the file out with CRLF on Windows: compare line contents only.
    let current = fs::read(&path)
        .map(|b| normalize_text(&b))
        .unwrap_or_default();
    if current == normalize_text(content.as_bytes()) {
        Ok(true)
    } else {
        println!(
            "{NOTICES_FILE} is missing or out of date: run `cargo xtask licenses --write` and commit it."
        );
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> bool {
        parse_expr(s).unwrap().accepted()
    }

    #[test]
    fn single_licences() {
        assert!(ok("MIT"));
        assert!(ok("Apache-2.0"));
        assert!(ok("GPL-3.0-or-later"));
        assert!(!ok("AGPL-3.0-or-later"));
        assert!(!ok("AGPL-3.0-only"));
        assert!(!ok("GPL-2.0-only"));
        assert!(!ok("SSPL-1.0"));
        assert!(!ok("LGPL-2.1-only"));
        assert!(!ok("Proprietary"));
    }

    #[test]
    fn or_needs_one_alternative() {
        assert!(ok("MIT OR AGPL-3.0-only"));
        assert!(ok("AGPL-3.0-only OR Apache-2.0"));
        assert!(!ok("AGPL-3.0-only OR SSPL-1.0"));
    }

    #[test]
    fn and_needs_all() {
        assert!(ok("MIT AND Unicode-3.0"));
        assert!(!ok("MIT AND AGPL-3.0-only"));
        assert!(!ok("AGPL-3.0-only AND MIT"));
    }

    #[test]
    fn precedence_and_parentheses() {
        // AND binds tighter than OR.
        assert!(ok("MIT OR AGPL-3.0-only AND SSPL-1.0"));
        assert!(!ok("(MIT OR AGPL-3.0-only) AND SSPL-1.0"));
        assert!(ok("(MIT OR Apache-2.0) AND Unicode-3.0"));
        assert!(parse_expr("((MIT)").is_err());
    }

    #[test]
    fn with_exceptions() {
        assert!(ok("Apache-2.0 WITH LLVM-exception"));
        assert!(ok("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"));
        assert!(!ok("Apache-2.0 WITH Unknown-exception"));
        assert!(!ok("AGPL-3.0-only WITH LLVM-exception"));
    }

    #[test]
    fn legacy_slash_is_or() {
        assert!(ok("MIT/Apache-2.0"));
        assert!(ok("Apache-2.0 / MIT"));
        assert!(ok("MIT/AGPL-3.0-only"));
        assert!(!ok("AGPL-3.0-only/SSPL-1.0"));
        assert_eq!(
            parse_expr("A/B").unwrap(),
            Expr::Or(vec![
                Expr::License {
                    id: "A".into(),
                    exception: None
                },
                Expr::License {
                    id: "B".into(),
                    exception: None
                },
            ])
        );
    }

    #[test]
    fn plus_suffix_means_or_later() {
        assert!(ok("GPL-2.0+"));
        assert!(!ok("GPL-2.0"));
        assert!(!ok("AGPL-3.0+"));
    }

    #[test]
    fn malformed_expressions() {
        assert!(parse_expr("").is_err());
        assert!(parse_expr("MIT OR").is_err());
        assert!(parse_expr("MIT MIT").is_err());
        assert!(parse_expr("MIT WITH").is_err());
        assert!(parse_expr("(MIT").is_err());
        assert!(parse_expr("MIT)").is_err());
    }

    #[test]
    fn refusal_reasons() {
        assert!(refusal(Some("MIT")).is_none());
        assert!(refusal(None).is_some());
        assert!(refusal(Some("AGPL-3.0-only")).is_some());
        assert!(refusal(Some("MIT OR")).is_some());
    }

    #[test]
    fn npm_lock_resolution() {
        let lock: Value = serde_json::json!({
            "": {},
            "node_modules/a": {},
            "node_modules/b": {},
            "node_modules/a/node_modules/b": {},
            "node_modules/a/node_modules/c": {},
        });
        let p = lock.as_object().unwrap();
        // The nested copy wins for `a`, the top-level one elsewhere.
        assert_eq!(
            npm_resolve(p, "node_modules/a", "b").unwrap(),
            "node_modules/a/node_modules/b"
        );
        assert_eq!(npm_resolve(p, "", "b").unwrap(), "node_modules/b");
        assert_eq!(
            npm_resolve(p, "node_modules/b", "a").unwrap(),
            "node_modules/a"
        );
        assert_eq!(
            npm_resolve(p, "node_modules/a/node_modules/c", "b").unwrap(),
            "node_modules/a/node_modules/b"
        );
        assert!(npm_resolve(p, "node_modules/a", "zzz").is_none());
        assert_eq!(npm_name("node_modules/a/node_modules/@s/b"), "@s/b");
        assert_eq!(npm_name("node_modules/a"), "a");
    }

    #[test]
    fn cargo_tree_output() {
        let text = r"ardoise v0.1.0 (C:/work/app)
ardoise-core v0.1.0 (C:/work/core)
serde v1.0.229
serde v1.0.229 (*)
tauri v2.12.5
";
        let members: BTreeSet<&str> = ["ardoise", "ardoise-core"].into();
        let pkgs: Vec<_> = parse_cargo_tree(text, &members).into_iter().collect();
        assert_eq!(
            pkgs,
            [
                ("serde".into(), "1.0.229".into()),
                ("tauri".into(), "2.12.5".into())
            ]
        );
    }

    #[test]
    fn text_normalization() {
        assert_eq!(
            normalize_text(b"\xef\xbb\xbf\r\n\r\nLine one  \r\nLine two\r\n\r\n"),
            "Line one\nLine two"
        );
        assert_eq!(normalize_text(b"A\nB"), normalize_text(b"A\r\nB\r\n"));
    }

    #[test]
    fn repository_urls() {
        assert_eq!(
            clean_url("git+https://github.com/a/b.git"),
            "https://github.com/a/b"
        );
        assert_eq!(
            clean_url("git://github.com/a/b.git"),
            "https://github.com/a/b"
        );
        assert_eq!(clean_url("github:a/b"), "https://github.com/a/b");
        assert_eq!(clean_url("a/b"), "https://github.com/a/b");
        assert_eq!(
            clean_url("https://github.com/a/b/"),
            "https://github.com/a/b"
        );
        assert_eq!(
            npm_repository(&serde_json::json!({ "repository": { "type": "git", "url": "git+https://x.org/a.git" } })).unwrap(),
            "https://x.org/a"
        );
    }
}
