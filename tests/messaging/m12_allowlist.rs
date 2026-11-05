use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use openwiz_adapter_cloudroom::allowlist::{Invocation, ThreadId};
use openwiz_core::messaging::Mode;

use crate::m01_exact_invocation::{EXCLUDED, tell_argv};
use crate::support::*;

/// Subcommands that change something. Only `tell` may ever be passed.
const MUTATING: &[&str] = &[
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
    "queue",
    "retry",
    "steer",
    "resume",
    "pause",
];

fn all_reads() -> Vec<Invocation> {
    let id = ThreadId::parse("thr_syn_0001").unwrap();
    vec![
        Invocation::Version,
        Invocation::ThreadList {
            include_hidden: true,
            include_archived: true,
        },
        Invocation::ProjectList { hidden: true },
        Invocation::MachineList,
        Invocation::ThreadShow(id.clone()),
        Invocation::ThreadOutput(id),
        Invocation::CloudStatus,
    ]
}

#[test]
fn thread_tell_is_sends_only_write_in_exactly_its_shape() {
    for read in all_reads() {
        let argv = read.argv();
        for word in MUTATING {
            assert!(!argv.iter().any(|a| a == word), "{read:?} passes {word}");
        }
    }
    let id = ThreadId::parse("thr_syn_0001").unwrap();
    for (mode, word) in [(Mode::Queue, "queue"), (Mode::Steer, "steer")] {
        for text in [
            "Synthetic",
            "--file /synthetic/x",
            "-x",
            "auto",
            "--mode auto",
            "line\nbreak",
        ] {
            let argv = Invocation::ThreadTell {
                id: id.clone(),
                mode,
                text: text.to_owned(),
                attachments: vec![],
            }
            .argv();
            assert_eq!(argv, tell_argv("thr_syn_0001", word, text));
            for flag in EXCLUDED {
                assert!(
                    !argv[..7].iter().any(|a| a == flag),
                    "{flag} before the text: {argv:?}"
                );
            }
        }
    }
}

#[test]
fn the_fake_refuses_every_other_write_and_every_other_tell_shape() {
    let fake = SendFake::new("m12_fake_refuses", "send");
    let refused: Vec<Vec<&str>> = vec![
        vec!["thread", "spawn", "--json"],
        vec!["thread", "stop", "thr_syn_0001"],
        vec!["thread", "archive", "thr_syn_0001"],
        vec!["thread", "queue", "create", "thr_syn_0001", "--hard", "x"],
        vec!["thread", "queue", "list", "thr_syn_0001", "--json"],
        vec!["thread", "log", "thr_syn_0001", "--json"],
        vec![
            "thread",
            "tell",
            "thr_syn_0001",
            "--mode",
            "auto",
            "--json",
            "--",
            "x",
        ],
        vec!["thread", "tell", "thr_syn_0001", "--json", "--", "x"],
        vec![
            "thread",
            "tell",
            "thr_syn_0001",
            "--mode",
            "queue",
            "--",
            "x",
        ],
        vec![
            "thread",
            "tell",
            "thr_syn_0001",
            "--mode",
            "queue",
            "--json",
            "x",
        ],
        vec![
            "thread",
            "tell",
            "thr_syn_0001",
            "--mode",
            "queue",
            "--json",
            "--file",
            "x",
            "--",
            "y",
        ],
        vec![
            "thread",
            "tell",
            "--",
            "thr_syn_0001",
            "--mode",
            "queue",
            "--json",
            "x",
        ],
        vec![
            "thread", "tell", "-x", "--mode", "queue", "--json", "--", "x",
        ],
    ];
    for args in &refused {
        let out = Command::new(fake.program())
            .args(args)
            .output()
            .expect("run the fake");
        assert_eq!(out.status.code(), Some(97), "{args:?}");
    }
    assert_eq!(fake.tell_count(), 0, "a refused call is not counted");
}

/// B2 (sheet the check): the fake takes `--file` and `--image` only after
/// `--json` and before `--`, in queue mode, each with a staged copy's
/// absolute path; every other form is refused before anything is counted.
#[test]
fn the_fake_takes_attachment_flags_only_with_staged_paths_in_queue() {
    let fake = SendFake::new("m12_fake_attachments", "send");
    let staged = "/synthetic/state/attachments/att-syn-1/syn-notes.txt";
    let tell = |mode: &str, tail: &[&str]| -> Vec<String> {
        let mut argv: Vec<String> = ["thread", "tell", "thr_syn_0001", "--mode", mode, "--json"]
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        argv.extend(tail.iter().map(|w| (*w).to_owned()));
        argv.extend(["--".to_owned(), "x".to_owned()]);
        argv
    };
    let refused = [
        tell("steer", &["--file", staged]),
        tell("queue", &["--file", "syn-notes.txt"]),
        tell(
            "queue",
            &[
                "--file",
                "file:///synthetic/state/attachments/att-syn-1/syn-notes.txt",
            ],
        ),
        tell(
            "queue",
            &[
                "--file",
                "/synthetic/../attachments/att-syn-1/syn-notes.txt",
            ],
        ),
        tell(
            "queue",
            &["--file", "/synthetic/./attachments/att-syn-1/syn-notes.txt"],
        ),
        tell(
            "queue",
            &["--file", "/synthetic/state/other/att-syn-1/syn-notes.txt"],
        ),
        tell(
            "queue",
            &["--file", "/synthetic/state/attachments/-att/syn-notes.txt"],
        ),
        tell(
            "queue",
            &["--file", "/synthetic/state/attachments/att-syn-1/.hidden"],
        ),
        tell("queue", &["--file"]),
        tell("queue", &["--attach", staged]),
        tell("queue", &["--image", staged, "--model", "m"]),
    ];
    for args in &refused {
        let out = Command::new(fake.program())
            .args(args)
            .output()
            .expect("run the fake");
        assert_eq!(out.status.code(), Some(97), "{args:?}");
    }
    assert_eq!(fake.tell_count(), 0, "a refused call is not counted");
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_product_source_spells_an_excluded_flag_or_another_write() {
    let mut files = Vec::new();
    rust_files(&Path::new(ROOT).join("crates"), &mut files);
    assert!(files.len() > 10);
    let allowlist = Path::new(ROOT)
        .join("crates/adapters/cloudroom/src/allowlist.rs")
        .canonicalize()
        .unwrap();
    // OpenWiz 0.0.4 (the implementation, A1 phase 2): the catalogue key `steer`,
    // only in the adapter's `cloud status` reader; and `openwiz new`'s own
    // `--model` option, only in the CLI's `new` (not a room-cli argument
    // there).
    let catalogue = Path::new(ROOT)
        .join("crates/adapters/cloudroom/src/new_thread.rs")
        .canonicalize()
        .unwrap();
    let new_command = Path::new(ROOT)
        .join("crates/cli/src/new_thread.rs")
        .canonicalize()
        .unwrap();
    // `spawn`'s own flags, only inside `ThreadSpawn`'s argv in the allowlist.
    let spawn_arm = |text: &str| {
        text.split("Self::ThreadSpawn(")
            .nth(1)
            .and_then(|rest| rest.split("],").next())
            .unwrap_or_default()
            .to_owned()
    };
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let canonical = path.canonicalize().unwrap();
        for flag in EXCLUDED {
            let literal = format!("\"{flag}\"");
            if canonical == allowlist && matches!(*flag, "--model" | "--reasoning-level") {
                assert_eq!(text.matches(&literal).count(), 1, "{literal} once");
                assert!(
                    spawn_arm(&text).contains(&literal),
                    "{literal} outside ThreadSpawn"
                );
                continue;
            }
            if canonical == new_command && *flag == "--model" {
                continue;
            }
            if canonical == allowlist && matches!(*flag, "--file" | "--image") {
                assert_eq!(text.matches(&literal).count(), 1, "{literal} once");
                let builder = text
                    .split("pub fn attachment_args(")
                    .nth(1)
                    .and_then(|rest| rest.split("\n}\n").next())
                    .unwrap_or_default();
                assert!(
                    builder.contains(&literal),
                    "{literal} outside attachment_args"
                );
                continue;
            }
            assert!(!text.contains(&literal), "{path:?} contains {literal}");
        }
        for word in MUTATING {
            let literal = format!("\"{word}\"");
            let is_allowlist = canonical == allowlist;
            if *word == "tell" && is_allowlist {
                continue;
            }
            // `queue` and `steer` are the core's mode words, passed through
            // `Mode::as_str`; `send` is OpenWiz's own command, parsed by the
            // CLI. Neither is a room-cli subcommand there.
            let mode_words = path.ends_with("core/src/messaging/mod.rs");
            if matches!(*word, "queue" | "steer") && mode_words {
                continue;
            }
            if *word == "send" && path.starts_with(Path::new(ROOT).join("crates/cli")) {
                continue;
            }
            if *word == "spawn" && canonical == allowlist {
                assert!(spawn_arm(&text).contains(&literal), "spawn outside its arm");
                continue;
            }
            if *word == "steer" && canonical == catalogue {
                continue;
            }
            assert!(!text.contains(&literal), "{path:?} contains {literal}");
        }
    }
}
