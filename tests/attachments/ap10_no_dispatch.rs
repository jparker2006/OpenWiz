//! AP10 No dispatch path. Phase 1 adds no `room-cli` call: the model names
//! no invocation, no process and no attachment flag. P8(a) and M12 run in
//! `cargo test --workspace`.
//!
//! **Updated for B2 (the implementation)**, with this recorded reason: sheet the check
//!  adds `--file` and `--image` to `tell`
//! and `spawn`, with staged absolute paths only. So the allowlist now names
//! each flag exactly once, inside its one `attachment_args`, and uses the
//! model's `StagedAttachment` and `Kind` for it; the adapter's `send` and
//! `new` pass the core's `ReadyPrompt`. AD11
//! (`tests/attachments/ad_static.rs`) proves there is one builder. The
//! model itself still names no invocation, process or flag, the server
//! still uses no attachment, and the store and the CLI still dispatch
//! nothing themselves.

use std::fs;
use std::path::Path;

use crate::support::ROOT;

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(ROOT).join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn the_model_has_no_dispatch_path_and_no_io() {
    let model = read("crates/core/src/attachment.rs");
    for word in [
        "room-cli",
        "room_cli",
        "Invocation",
        "Command",
        "openwiz_adapter",
        "allowlist",
        "\"tell\"",
        "\"spawn\"",
        "\"--file\"",
        "\"--image\"",
        "std::fs",
        "std::io",
        "std::net",
        "std::process",
        "std::env",
        "std::os",
        "SystemTime",
        "Instant",
        "println!",
        "eprintln!",
        "unsafe",
    ] {
        assert!(!model.contains(word), "attachment.rs names {word}");
    }
    // It imports only `std::time`, `std::fmt` and serde, and nothing from
    // another crate or module of the workspace.
    for line in model.lines().filter(|l| l.starts_with("use ")) {
        assert!(
            line.contains("use std::time::Duration")
                || line.contains("use std::fmt")
                || line.contains("use serde::"),
            "attachment.rs: {line}"
        );
    }
}

#[test]
fn the_allowlist_names_each_attachment_flag_once_inside_its_one_builder() {
    let allowlist = read("crates/adapters/cloudroom/src/allowlist.rs");
    let builder = allowlist
        .split("pub fn attachment_args(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("attachment_args");
    for flag in ["\"--file\"", "\"--image\""] {
        assert_eq!(allowlist.matches(flag).count(), 1, "{flag} once");
        assert!(builder.contains(flag), "{flag} outside attachment_args");
    }
}

/// Phase 2: the model reaches the store and the CLI, and nothing else. The
/// store's staging and the CLI's attachment commands name no `room-cli`
/// call, no adapter and no write: nothing is sent or uploaded.
#[test]
fn only_the_store_and_the_cli_use_the_model_and_neither_dispatches() {
    for dir in ["crates/adapters/cloudroom/src"] {
        let mut files = Vec::new();
        rust_files(&Path::new(ROOT).join(dir), &mut files);
        for path in files {
            let text = fs::read_to_string(&path).unwrap();
            if path.ends_with("adapters/cloudroom/src/allowlist.rs") {
                continue;
            }
            assert!(
                !text.contains("attachment::"),
                "{path:?} uses the attachment model"
            );
        }
    }
    for file in [
        "crates/store/src/attachments.rs",
        "crates/cli/src/attachment.rs",
    ] {
        let text = read(file);
        assert!(text.contains("openwiz_core::attachment::"), "{file}");
        for word in [
            "room-cli",
            "room_cli",
            "Invocation",
            "openwiz_adapter",
            "CloudRoomSource",
            "allowlist",
            "\"tell\"",
            "\"spawn\"",
            "\"--file\"",
            "\"--image\"",
            "Messenger",
            "NewThreadClient",
            "std::net",
        ] {
            assert!(!text.contains(word), "{file} names {word}");
        }
    }
    // The CLI's module starts no process. The store's starts one program,
    // `/bin/df`, for the free space : P8(a) checks
    // that call word for word, and `program` that nothing on `PATH` runs.
    let cli = read("crates/cli/src/attachment.rs");
    assert!(!cli.contains("Command::new") && !cli.contains("process::Command"));
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
