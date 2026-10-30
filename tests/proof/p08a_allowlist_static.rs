//! P8(a) Allowlist, static: no invocation outside the allowlist.
//!
//! It checks every [`Invocation`] the adapter can
//! build, the id guard that stops an id becoming a flag, and that only the
//! adapter's allowlist can start a process. Which invocations a real run
//! makes is checked against the fake's log in the other proof cases.

use std::fs;
use std::path::{Path, PathBuf};

use openwiz_adapter_cloudroom::allowlist::{Invocation, THREAD_ID_MAX_LEN, ThreadId};

/// The approved allowlist, written out by hand.
const ALLOWED: &[&[&str]] = &[
    &["--version"],
    &["thread", "list", "--json"],
    &["thread", "list", "--json", "--include-hidden"],
    &["thread", "list", "--json", "--include-archived"],
    &[
        "thread",
        "list",
        "--json",
        "--include-hidden",
        "--include-archived",
    ],
    &["project", "list", "--json", "--include-personal"],
    &[
        "project",
        "list",
        "--json",
        "--include-personal",
        "--hidden",
    ],
    &["machine", "list", "--json"],
    &["thread", "show", "thr_syn_0001", "--json"],
    &["thread", "output", "thr_syn_0001", "--json"],
];

/// Arguments that must never be passed, whatever the invocation.
const FORBIDDEN: &[&str] = &[
    "--work-status",
    "--git-diff",
    "log",
    "spawn",
    "fork",
    "tell",
    "stop",
    "archive",
    "unarchive",
    "update",
    "move",
    "hide",
    "unhide",
    "delete",
    "create",
    "send",
    "mark-read",
];

/// Every invocation the adapter can build. The `match` makes a new variant
/// a compile error here, as a reminder to add its forms to `all`; it cannot
/// force that by itself.
fn every_invocation() -> Vec<Invocation> {
    let id = ThreadId::parse("thr_syn_0001").expect("valid id");
    let all = vec![
        Invocation::Version,
        Invocation::ThreadList {
            include_hidden: false,
            include_archived: false,
        },
        Invocation::ThreadList {
            include_hidden: true,
            include_archived: false,
        },
        Invocation::ThreadList {
            include_hidden: false,
            include_archived: true,
        },
        Invocation::ThreadList {
            include_hidden: true,
            include_archived: true,
        },
        Invocation::ProjectList { hidden: false },
        Invocation::ProjectList { hidden: true },
        Invocation::MachineList,
        Invocation::ThreadShow(id.clone()),
        Invocation::ThreadOutput(id),
    ];
    for invocation in &all {
        match invocation {
            Invocation::Version
            | Invocation::ThreadList { .. }
            | Invocation::ProjectList { .. }
            | Invocation::MachineList
            | Invocation::ThreadShow(_)
            | Invocation::ThreadOutput(_) => {}
        }
    }
    all
}

#[test]
fn every_invocation_is_on_the_allowlist() {
    let built: Vec<Vec<String>> = every_invocation().iter().map(Invocation::argv).collect();
    let allowed: Vec<Vec<String>> = ALLOWED
        .iter()
        .map(|a| a.iter().map(|w| (*w).to_owned()).collect())
        .collect();
    assert_eq!(built, allowed);
}

#[test]
fn hidden_and_archived_flags_are_added_only_on_request() {
    let plain = Invocation::ThreadList {
        include_hidden: false,
        include_archived: false,
    }
    .argv();
    assert!(
        !plain
            .iter()
            .any(|a| a == "--include-hidden" || a == "--include-archived")
    );
    assert!(
        !Invocation::ProjectList { hidden: false }
            .argv()
            .iter()
            .any(|a| a == "--hidden")
    );
}

#[test]
fn no_invocation_passes_a_forbidden_argument() {
    for invocation in every_invocation() {
        let argv = invocation.argv();
        for word in &argv {
            assert!(
                !FORBIDDEN.contains(&word.as_str()),
                "{invocation:?} passes {word}"
            );
        }
        // Every read asks for JSON, except --version, which prints plain text.
        assert!(
            argv == ["--version"] || argv.iter().any(|a| a == "--json"),
            "{argv:?}"
        );
    }
}

#[test]
fn ids_cannot_become_flags_or_extra_arguments() {
    for bad in [
        "",
        "-",
        "--work-status",
        "-x",
        "thr syn",
        "thr\nsyn",
        "thr;syn",
        "thr/syn",
        "thr=syn",
        "thr\u{0}syn",
        "thr_sÿn",
    ] {
        assert!(ThreadId::parse(bad).is_err(), "accepted {bad:?}");
    }
    assert!(ThreadId::parse(&"a".repeat(THREAD_ID_MAX_LEN + 1)).is_err());
    for good in ["thr_syn_0001", "a", "A-1_b", &"a".repeat(THREAD_ID_MAX_LEN)] {
        let id = ThreadId::parse(good).expect(good);
        let argv = Invocation::ThreadShow(id).argv();
        assert_eq!(argv.len(), 4, "the id stays one argument");
        assert_eq!(argv[2], good);
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn product_sources() -> Vec<(PathBuf, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates");
    let mut files = Vec::new();
    rust_files(&crates, &mut files);
    assert!(!files.is_empty());
    files
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).expect("source");
            (p, text)
        })
        .collect()
}

#[test]
fn only_the_adapter_can_start_a_process_and_only_from_the_allowlist() {
    let adapter = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/adapters/cloudroom")
        .canonicalize()
        .expect("adapter dir");
    for (path, text) in product_sources() {
        let starts_process = text.contains("process::Command") || text.contains("Command::new");
        let in_adapter = path.canonicalize().expect("path").starts_with(&adapter);
        assert!(
            !starts_process || in_adapter,
            "{path:?} starts a process; only the CloudRoom adapter may"
        );
        if in_adapter {
            assert!(
                !text.contains(".arg("),
                "{path:?}: arguments come only from Invocation::argv"
            );
            for (i, _) in text.match_indices(".args(") {
                let call = &text[i..text[i..].find(')').map_or(text.len(), |j| i + j + 1)];
                assert!(
                    call.contains("argv("),
                    "{path:?}: {call} is not Invocation::argv"
                );
            }
        }
    }
}

#[test]
fn no_product_source_spells_a_forbidden_argument() {
    for (path, text) in product_sources() {
        for word in FORBIDDEN {
            let literal = format!("\"{word}\"");
            assert!(!text.contains(&literal), "{path:?} contains {literal}");
        }
    }
}
