//! Recognises the notes of a folder with Windows' engine and prints what was read:
//!
//! ```text
//! cargo run -p ardoise-hwr --example recognize -- FOLDER [fr|en] [--scale 2,4] [--quiet]
//! ```
//!
//! Every `.ardoise` file below FOLDER is read; for each scale (enlargement beyond the physical
//! size, `ink::RECOGNITION_SCALE` by default) the note's text, its number of words and the
//! time taken are printed (`--quiet`: no text). Nothing is written anywhere.

use std::path::{Path, PathBuf};
use std::time::Instant;

use ardoise_hwr::ink::{RECOGNITION_SCALE, Scale, recognizable_strokes};
use ardoise_hwr::{Language, Recognizer, SystemRecognizer};

fn note_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            note_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "ardoise") {
            out.push(path);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut folder = None;
    let mut language = Language::Fr;
    let mut scales = vec![RECOGNITION_SCALE];
    let mut quiet = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "fr" => language = Language::Fr,
            "en" => language = Language::En,
            "--quiet" => quiet = true,
            "--scale" => {
                scales = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
            }
            _ => folder = Some(PathBuf::from(arg)),
        }
    }
    let Some(folder) = folder else {
        eprintln!("usage: recognize FOLDER [fr|en] [--scale 2,4] [--quiet]");
        std::process::exit(2);
    };
    let mut files = Vec::new();
    note_files(&folder, &mut files);
    files.sort();
    for scale in scales {
        let recognizer = SystemRecognizer::with_scale(Scale::enlarged(scale));
        let mut words = 0;
        let mut millis = 0;
        for file in &files {
            let note = match ardoise_core::format::read_note(file) {
                Ok(read) => read.note,
                Err(error) => {
                    println!("{}: {error}", file.display());
                    continue;
                }
            };
            let strokes = recognizable_strokes(&note);
            let started = Instant::now();
            match recognizer.recognize(&strokes, language) {
                Ok(recognition) => {
                    let elapsed = started.elapsed().as_millis();
                    millis += elapsed;
                    let count = recognition.words().count();
                    words += count;
                    println!(
                        "×{scale} {} strokes, {count} words, {elapsed} ms: {}",
                        strokes.len(),
                        file.file_name().unwrap_or_default().to_string_lossy()
                    );
                    if !quiet {
                        println!("{}\n", recognition.text());
                    }
                }
                Err(error) => println!("×{scale} {}: {error}", file.display()),
            }
        }
        println!(
            "×{scale}: {} notes, {words} words, {millis} ms in all",
            files.len()
        );
    }
}
