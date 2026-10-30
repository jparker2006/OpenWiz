//! The pending list: every future-behaviour proof test is explicitly
//! pending, and none is skipped silently.
//!
//! Rules:
//! - In P1 to P7 and P9 to P12, a test is either `#[ignore = "pending: ticket 2"]`
//!   or runs. The marker is removed with the code that satisfies the test; in
//!   0.0.1 no test is pending.
//! - P8(a) tests and the foundation checks are never ignored.
//! - No other `#[ignore` exists in the repository's Rust sources.
//! - `tests/pending-ticket-2.txt` lists the pending tests exactly.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const PENDING: &str = "#[ignore = \"pending: ticket 2\"]";
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// (test name, its attribute lines) for every `#[test]` in `text`.
fn tests_in(text: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if *line != "#[test]" {
            continue;
        }
        let mut attrs = Vec::new();
        for next in &lines[i + 1..] {
            if let Some(rest) = next.strip_prefix("fn ") {
                let name = rest.split('(').next().unwrap_or_default().to_owned();
                out.push((name, attrs));
                break;
            }
            attrs.push((*next).to_owned());
        }
    }
    out
}

fn listed() -> BTreeSet<String> {
    fs::read_to_string(Path::new(ROOT).join("tests/pending-ticket-2.txt"))
        .expect("tests/pending-ticket-2.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_pending_list_matches_the_future_behaviour_tests_exactly() {
    let proof = Path::new(ROOT).join("tests/proof");
    let mut found = BTreeSet::new();
    let mut cases = BTreeSet::new();
    for entry in fs::read_dir(&proof).expect("tests/proof") {
        let path = entry.expect("entry").path();
        let module = path.file_stem().unwrap().to_string_lossy().into_owned();
        if !module.starts_with('p') {
            continue;
        }
        let text = fs::read_to_string(&path).expect("proof source");
        let tests = tests_in(&text);
        assert!(!tests.is_empty(), "{module} has no tests");
        let markers = text.lines().filter(|l| l.trim() == PENDING).count();
        let marked = tests
            .iter()
            .filter(|(_, attrs)| attrs.iter().any(|a| a == PENDING))
            .count();
        assert_eq!(
            markers, marked,
            "{module}: every {PENDING} must sit between a #[test] and its fn"
        );
        cases.insert(module[..3].to_owned());
        for (name, attrs) in tests {
            let pending = attrs.iter().any(|a| a == PENDING);
            if module.starts_with("p08a") {
                assert!(
                    attrs.is_empty(),
                    "{module}::{name} must pass now, not be ignored"
                );
            } else if pending {
                found.insert(format!("{module}::{name}"));
            }
        }
    }
    let expected: BTreeSet<String> = (1..=12).map(|n| format!("p{n:02}")).collect();
    assert_eq!(cases, expected, "every proof case P1 to P12 has a module");
    assert_eq!(
        found,
        listed(),
        "tests/pending-ticket-2.txt must list the pending tests exactly"
    );
}

#[test]
fn nothing_else_is_ignored() {
    let mut files = Vec::new();
    rust_files(&Path::new(ROOT).join("crates"), &mut files);
    rust_files(&Path::new(ROOT).join("tests"), &mut files);
    let proof = Path::new(ROOT).join("tests/proof");
    for path in files {
        let text = fs::read_to_string(&path).expect("source");
        let is_proof_case = path.starts_with(&proof)
            && path
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().starts_with('p'));
        for line in text.lines().map(str::trim) {
            if line.starts_with("#[ignore") {
                assert!(
                    is_proof_case && line == PENDING,
                    "{path:?}: only proof cases may be ignored, and only as {PENDING}: {line}"
                );
            }
        }
    }
}
