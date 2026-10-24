//! Project automation: `cargo xtask check [--quick]`, `cargo xtask selftest`, `cargo xtask e2e` and
//! `cargo xtask golden`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

const USAGE: &str = "Usage: cargo xtask <command>

Commands:
  check [--quick]  frontend build, fmt, clippy, tests, frontend checks, release build
                   (--quick skips the release build)
  selftest         run target/release/ardoise.exe --self-test and check its size
  golden           regenerate the reference images in testdata/golden, then check them
  e2e              end-to-end tests of the main journeys, in French and English, on the
                   release executable (run `cargo xtask check` first)";

/// Maximum accepted size of the release executable, in bytes.
const MAX_EXE_BYTES: u64 = 30 * 1024 * 1024;
/// Time allowed for `ardoise.exe --self-test`.
const SELFTEST_TIMEOUT: Duration = Duration::from_secs(60);
/// npm's launcher: a `.cmd` script on Windows, a plain program elsewhere (cloud sessions run Linux).
const NPM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check"] => check(false),
        ["check", "--quick"] => check(true),
        ["selftest"] => selftest(),
        ["golden"] => golden(),
        ["e2e"] => e2e(),
        _ => {
            println!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Workspace root (the parent of the `xtask` crate directory).
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Runs a program with inherited stdio in `dir`; true if it exited with 0.
fn run(dir: &Path, program: &str, args: &[&str]) -> bool {
    match Command::new(program).args(args).current_dir(dir).status() {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!("cannot run {program}: {e}");
            false
        }
    }
}

/// True if `node_modules` is missing or older than `package-lock.json`.
fn node_modules_stale(app: &Path) -> bool {
    let modified = |p: PathBuf| fs::metadata(p).and_then(|m| m.modified()).ok();
    match (
        modified(app.join("node_modules")),
        modified(app.join("package-lock.json")),
    ) {
        (None, _) => true,
        (Some(nm), Some(lock)) => nm < lock,
        (Some(_), None) => false,
    }
}

/// Installs frontend dependencies when needed.
fn frontend_deps(app: &Path) -> bool {
    if !node_modules_stale(app) {
        println!("node_modules is up to date");
        return true;
    }
    if app.join("package-lock.json").exists() {
        run(app, NPM, &["ci"])
    } else {
        run(app, NPM, &["install"])
    }
}

/// One step of `check`: a label and the action that runs it.
struct Step<'a> {
    name: &'a str,
    action: Box<dyn Fn() -> bool + 'a>,
}

/// Runs the full verification pipeline, stopping at the first failure.
fn check(quick: bool) -> ExitCode {
    let root = root();
    let app = root.join("app");
    let (r, a) = (root.as_path(), app.as_path());
    let cargo = |args: &'static [&'static str]| move || run(r, "cargo", args);
    let npm = |args: &'static [&'static str]| move || run(a, NPM, args);

    let mut steps = vec![
        Step {
            name: "npm ci (frontend deps)",
            action: Box::new(move || frontend_deps(a)),
        },
        Step {
            name: "npm run build",
            action: Box::new(npm(&["run", "build"])),
        },
        Step {
            name: "cargo fmt --all -- --check",
            action: Box::new(cargo(&["fmt", "--all", "--", "--check"])),
        },
        Step {
            name: "cargo clippy --workspace --all-targets -- -D warnings",
            action: Box::new(cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])),
        },
        Step {
            name: "cargo test --workspace",
            action: Box::new(cargo(&["test", "--workspace"])),
        },
        Step {
            name: "npm run check",
            action: Box::new(npm(&["run", "check"])),
        },
        Step {
            name: "npm test",
            action: Box::new(npm(&["test"])),
        },
    ];
    if !quick {
        steps.push(Step {
            name: "cargo build --release -p ardoise --features custom-protocol",
            action: Box::new(cargo(&[
                "build",
                "--release",
                "-p",
                "ardoise",
                "--features",
                "custom-protocol",
            ])),
        });
    }

    let total = steps.len();
    let mut results: Vec<(&str, &str, Duration)> = Vec::new();
    let mut failed = None;
    for (i, step) in steps.iter().enumerate() {
        println!("==> [{}/{}] {}", i + 1, total, step.name);
        let start = Instant::now();
        let ok = (step.action)();
        results.push((step.name, if ok { "ok" } else { "FAILED" }, start.elapsed()));
        if !ok {
            failed = Some(step.name);
            break;
        }
    }

    println!("\n{:<58} {:<8} {:>9}", "Step", "Status", "Duration");
    for (name, status, d) in &results {
        println!("{name:<58} {status:<8} {:>8.1}s", d.as_secs_f64());
    }
    match failed {
        None => {
            println!("CHECK OK");
            ExitCode::SUCCESS
        }
        Some(name) => {
            println!("CHECK FAILED: {name}");
            ExitCode::FAILURE
        }
    }
}

/// Checks the release executable's size and runs its `--self-test`.
fn selftest() -> ExitCode {
    let fail = |reason: &str| {
        println!("SELFTEST FAILED: {reason}");
        ExitCode::FAILURE
    };
    let exe = root().join("target").join("release").join("ardoise.exe");
    let Ok(meta) = fs::metadata(&exe) else {
        eprintln!(
            "{} not found: run `cargo xtask check` first.",
            exe.display()
        );
        return fail("ardoise.exe missing");
    };
    let size = meta.len();
    println!(
        "ardoise.exe size: {:.1} MB",
        size as f64 / (1024.0 * 1024.0)
    );
    if size >= MAX_EXE_BYTES {
        return fail("executable is 30 MB or larger");
    }

    let mut child = match Command::new(&exe)
        .arg("--self-test")
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return fail(&format!("cannot start ardoise.exe: {e}")),
    };
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() >= SELFTEST_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return fail("timeout after 60 s");
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return fail(&format!("wait error: {e}")),
        }
    };
    if status.success() {
        println!("SELFTEST OK");
        ExitCode::SUCCESS
    } else {
        fail(&format!("ardoise.exe exited with {status}"))
    }
}

/// Regenerates the rendering reference images (`testdata/golden/*.png`) from the synthetic
/// notes in `crates/ardoise-render/tests/golden.rs`, then runs the comparison test.
fn golden() -> ExitCode {
    let root = root();
    let regenerate = [
        "test",
        "-p",
        "ardoise-render",
        "--test",
        "golden",
        "--",
        "--ignored",
        "--exact",
        "regenerate_golden_images",
        "--nocapture",
    ];
    let compare = [
        "test",
        "-p",
        "ardoise-render",
        "--test",
        "golden",
        "--",
        "--nocapture",
    ];
    if run(&root, "cargo", &regenerate) && run(&root, "cargo", &compare) {
        println!("Look at the images in testdata/golden before committing them.");
        println!("GOLDEN OK");
        ExitCode::SUCCESS
    } else {
        println!("GOLDEN FAILED");
        ExitCode::FAILURE
    }
}

/// Runs the end-to-end journeys (`app/e2e/run.mjs`) on the release executable: a demo library
/// is created in a temporary folder and the real window is driven through WebView2's DevTools
/// protocol, once in French and once in English.
fn e2e() -> ExitCode {
    let root = root();
    let exe = root.join("target").join("release").join("ardoise.exe");
    if !exe.exists() {
        eprintln!(
            "{} not found: run `cargo xtask check` first.",
            exe.display()
        );
        println!("E2E FAILED: ardoise.exe missing");
        return ExitCode::FAILURE;
    }
    let script = root.join("app").join("e2e").join("run.mjs");
    let exe = exe.display().to_string();
    let script = script.display().to_string();
    if run(&root, "node", &[script.as_str(), exe.as_str()]) {
        println!("E2E OK");
        ExitCode::SUCCESS
    } else {
        println!("E2E FAILED");
        ExitCode::FAILURE
    }
}
