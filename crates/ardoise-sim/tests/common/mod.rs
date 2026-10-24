//! Helpers shared by the integration tests.
//!
//! Tests run in parallel threads: each test collects its report lines with `say!` and prints
//! them as one block when its [`Report`] guard is dropped (also when it fails), so the output
//! of `cargo test -p ardoise-sim -- --nocapture` reads test by test.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::fmt::Write as _;

use ardoise_proto::stroke::{Point, Stroke};

thread_local! {
    static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Adds a line to the report of the running test.
macro_rules! say {
    ($($arg:tt)*) => {
        $crate::common::say(format!($($arg)*))
    };
}

pub fn say(line: String) {
    LINES.with(|lines| lines.borrow_mut().push(line));
}

/// Prints the lines collected by the current test when dropped.
pub struct Report(&'static str);

/// Starts the report of a test.
pub fn report(title: &'static str) -> Report {
    LINES.with(|lines| lines.borrow_mut().clear());
    Report(title)
}

impl Drop for Report {
    fn drop(&mut self) {
        let lines = LINES.with(|lines| std::mem::take(&mut *lines.borrow_mut()));
        let mut out = format!("---- {} ----\n", self.0);
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
        print!("{out}");
    }
}

/// `"e6 06 ab"` -> bytes.
pub fn bytes(hex: &str) -> Vec<u8> {
    ardoise_proto::hex::decode(hex).unwrap()
}

/// Bytes -> `"e6 06 ab"`.
pub fn hex(bytes: &[u8]) -> String {
    ardoise_proto::hex::encode(bytes)
}

/// The points of each stroke, without the stroke times.
pub fn points(strokes: &[Stroke]) -> Vec<Vec<Point>> {
    strokes.iter().map(|s| s.points.clone()).collect()
}
