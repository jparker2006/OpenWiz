use std::fs;
use std::path::{Path, PathBuf};

use openwiz_adapter_cloudroom::allowlist::{Invocation, SpawnArgs, THREAD_ID_MAX_LEN, ThreadId};
use openwiz_core::attachment::{
    AttachmentId, Kind, PromptAttachment, SafeName, Sha256, StagedAttachment, StagedPath,
};
use openwiz_core::messaging::Mode;

/// The two staged copies of the attachment forms (B2): a file, then an
/// image.
const STAGED_FILE: &str = "/synthetic/state/attachments/att-syn-0001/syn-notes.txt";
const STAGED_IMAGE: &str = "/synthetic/state/attachments/att-syn-0002/syn-photo.png";

fn staged() -> Vec<StagedAttachment> {
    [
        ("att-syn-0001", "syn-notes.txt", Kind::File),
        ("att-syn-0002", "syn-photo.png", Kind::Image),
    ]
    .into_iter()
    .map(|(id, name, kind)| {
        let id = AttachmentId::parse(id).unwrap();
        let name = SafeName::parse(name).unwrap();
        StagedAttachment {
            path: StagedPath::new(Path::new("/synthetic/state/attachments"), &id, &name).unwrap(),
            attachment: PromptAttachment {
                id,
                sha256: Sha256::parse(&"0".repeat(64)).unwrap(),
                size_bytes: 1,
                kind,
                name,
            },
        }
    })
    .collect()
}

/// The request id of the spawn forms below.
const RID: &str = "ow1-0123456789abcdef0123456789abcdef01234567";

/// The allowlist as approved in written out by hand.
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
    // The one write (OpenWiz 0.0.3). Documented, not observed.
    &[
        "thread",
        "tell",
        "thr_syn_0001",
        "--mode",
        "queue",
        "--json",
        "--",
        "Synthetic",
    ],
    &[
        "thread",
        "tell",
        "thr_syn_0001",
        "--mode",
        "steer",
        "--json",
        "--",
        "Synthetic",
    ],
    &["cloud", "status", "--json"],
    &[
        "thread",
        "spawn",
        "--project",
        "prj_syn_0001",
        "--machine",
        "cloud",
        "--provider",
        "codex",
        "--model",
        "syn-codex-model-01",
        "--reasoning-level",
        "high",
        "--request-id",
        RID,
        "--json",
        "--prompt",
        "Synthetic",
    ],
    // B2: each write with attachments, staged
    // absolute paths only, the tail before the text's placement.
    &[
        "thread",
        "tell",
        "thr_syn_0001",
        "--mode",
        "queue",
        "--json",
        "--file",
        STAGED_FILE,
        "--image",
        STAGED_IMAGE,
        "--",
        "Synthetic",
    ],
    &[
        "thread",
        "spawn",
        "--project",
        "prj_syn_0001",
        "--machine",
        "cloud",
        "--provider",
        "codex",
        "--model",
        "syn-codex-model-01",
        "--reasoning-level",
        "high",
        "--request-id",
        RID,
        "--json",
        "--file",
        STAGED_FILE,
        "--image",
        STAGED_IMAGE,
        "--prompt",
        "Synthetic",
    ],
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
        Invocation::ThreadOutput(id.clone()),
        Invocation::ThreadTell {
            id: id.clone(),
            mode: Mode::Queue,
            text: "Synthetic".to_owned(),
            attachments: vec![],
        },
        Invocation::ThreadTell {
            id,
            mode: Mode::Steer,
            text: "Synthetic".to_owned(),
            attachments: vec![],
        },
        Invocation::CloudStatus,
        Invocation::ThreadSpawn(
            SpawnArgs::new(
                "prj_syn_0001",
                "codex",
                "syn-codex-model-01",
                "high",
                RID,
                "Synthetic",
            )
            .expect("valid spawn arguments"),
        ),
        Invocation::ThreadTell {
            id: ThreadId::parse("thr_syn_0001").expect("valid id"),
            mode: Mode::Queue,
            text: "Synthetic".to_owned(),
            attachments: staged(),
        },
        Invocation::ThreadSpawn(
            SpawnArgs::new(
                "prj_syn_0001",
                "codex",
                "syn-codex-model-01",
                "high",
                RID,
                "Synthetic",
            )
            .expect("valid spawn arguments")
            .with_attachments(staged()),
        ),
    ];
    for invocation in &all {
        match invocation {
            Invocation::Version
            | Invocation::ThreadList { .. }
            | Invocation::ProjectList { .. }
            | Invocation::MachineList
            | Invocation::ThreadShow(_)
            | Invocation::ThreadOutput(_)
            | Invocation::ThreadTell { .. }
            | Invocation::CloudStatus
            | Invocation::ThreadSpawn(_) => {}
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
        // The two writes: `tell` and `spawn` are their subcommands, and the
        // text after `--` or `--prompt` is not an argument to `room-cli` (it
        // is the message or the prompt).
        // B2: the attachment tail sits before the text's placement; its
        // paths are staged copies, never a word room-cli reads as a command.
        let (checked, allowed): (&[String], &[&str]) = match invocation {
            Invocation::ThreadTell { .. } => {
                assert_eq!(argv[1], "tell");
                let end = argv.len() - 2;
                assert_eq!(argv[end], "--", "the text follows --");
                (&argv[..=end], &["tell"])
            }
            Invocation::ThreadSpawn(_) => {
                assert_eq!(argv[1], "spawn");
                let end = argv.len() - 2;
                assert_eq!(argv[end], "--prompt", "the text follows --prompt");
                (&argv[..=end], &["spawn"])
            }
            _ => (&argv[..], &[]),
        };
        for word in checked {
            assert!(
                !FORBIDDEN.contains(&word.as_str()) || allowed.contains(&word.as_str()),
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

/// The store's attachment module: the one source outside the adapter that
/// may start a process, `/bin/df` only.
const FREE_SPACE_MEASURE: &str = "../crates/store/src/attachments.rs";

#[test]
fn only_the_adapter_can_start_a_process_and_only_from_the_allowlist() {
    let adapter = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/adapters/cloudroom")
        .canonicalize()
        .expect("adapter dir");
    let measure = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FREE_SPACE_MEASURE)
        .canonicalize()
        .expect("the free-space measure");
    let mut measured = false;
    for (path, text) in product_sources() {
        let starts_process = text.contains("process::Command")
            || text.contains("Command::new")
            || text.contains("process::{");
        let canonical = path.canonicalize().expect("path");
        let in_adapter = canonical.starts_with(&adapter);
        if canonical == measure {
            the_free_space_measure_runs_only_bin_df(&text);
            measured = true;
            continue;
        }
        assert!(
            !starts_process || in_adapter,
            "{path:?} starts a process; only the CloudRoom adapter may (and the store's \
             free-space measure, /bin/df)"
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
    assert!(measured, "{FREE_SPACE_MEASURE} was not checked");
}

/// The process allowlist’s one exception, and nothing more: one process, of `/bin/df`, with
/// `-P -k`, the directory, a cleared environment and `LC_ALL=C`, in exactly
/// that call. No other program, argument or variable can be given.
fn the_free_space_measure_runs_only_bin_df(text: &str) {
    let once = |needle: &str| {
        assert_eq!(
            text.matches(needle).count(),
            1,
            "{FREE_SPACE_MEASURE} names {needle:?} once"
        );
    };
    once("Command::new(");
    once("Command::new(DF_PROGRAM)");
    once("pub const DF_PROGRAM: &str = \"/bin/df\";");
    once("pub const DF_ARGS: [&str; 2] = [\"-P\", \"-k\"];");
    once("use std::process::{Command, Stdio};");
    once(".args(");
    once(".arg(");
    once(".env(");
    let call = text
        .split("Command::new(DF_PROGRAM)")
        .nth(1)
        .and_then(|rest| rest.split(".output()").next())
        .expect("the call");
    let call: String = call.split_whitespace().collect();
    assert_eq!(
        call,
        ".args(DF_ARGS).arg(dir).env_clear().env(\"LC_ALL\",\"C\").stdin(Stdio::null())\
         .stdout(Stdio::piped()).stderr(Stdio::null())",
        "the measure's call is exactly /bin/df -P -k <dir>, LC_ALL=C, nothing else"
    );
    for word in [
        "process::Command",
        "process::Child",
        "CommandExt",
        ".spawn(",
        ".status(",
        ".envs(",
        ".current_dir(",
        ".env_remove(",
        "room-cli",
        "room_cli",
    ] {
        assert!(!text.contains(word), "{FREE_SPACE_MEASURE} names {word}");
    }
}

#[test]
fn no_product_source_spells_a_forbidden_argument() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates");
    let allowlist = root
        .join("adapters/cloudroom/src/allowlist.rs")
        .canonicalize()
        .expect("allowlist");
    let cli = root.join("cli").canonicalize().expect("cli");
    for (path, text) in product_sources() {
        let path = path.canonicalize().expect("path");
        for word in FORBIDDEN {
            // The two writes' subcommands, only where the allowlist builds
            // them, each exactly once, inside its variant's argv; and
            // OpenWiz's own `send` command, only in the CLI.
            let arm = |variant: &str| {
                text.split(variant)
                    .nth(1)
                    .and_then(|rest| rest.split("],").next())
                    .unwrap_or_default()
                    .to_owned()
            };
            if *word == "tell" && path == allowlist {
                assert_eq!(text.matches("\"tell\"").count(), 1, "{path:?}");
                assert!(
                    arm("Self::ThreadTell {").contains("\"tell\""),
                    "{path:?}: tell outside ThreadTell"
                );
                continue;
            }
            if *word == "spawn" && path == allowlist {
                assert_eq!(text.matches("\"spawn\"").count(), 1, "{path:?}");
                assert!(
                    arm("Self::ThreadSpawn(").contains("\"spawn\""),
                    "{path:?}: spawn outside ThreadSpawn"
                );
                continue;
            }
            if *word == "send" && path.starts_with(&cli) {
                continue;
            }
            let literal = format!("\"{word}\"");
            assert!(!text.contains(&literal), "{path:?} contains {literal}");
        }
    }
}
