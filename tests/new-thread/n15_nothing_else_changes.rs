//! N15 Nothing else changes: the inventory and `send` keep their behaviour
//! (P1 to P12 and M1 to M18 run unchanged in the same `cargo test
//! --workspace`), and no inventory path can reach a write: `ls`, `ls -l`,
//! `show`, `models` and `new --status` make no `spawn` and no `tell`, and
//! the server names nothing of new threads (its routes are A2's).

use std::fs;
use std::path::Path;

use serde_json::json;

use crate::fake::*;
use crate::support::ROOT;

#[test]
fn no_read_command_reaches_a_write() {
    let fake = NewFake::with(
        "n15_reads",
        vec![json!({
            "argv": ["thread", "list", "--json"],
            "stdout_file": file("thread-list.json"),
        })],
    );
    for args in [
        &["ls", "--json"][..],
        &["ls", "-l", "-a", "--hidden", "--json"][..],
        &["show", "thr_syn_0001", "--json"][..],
        &["models", "--json"][..],
        &["models"][..],
        &["new", "--status", "op-n15-none", "--json"][..],
        &[
            "new",
            "Synthetic project one",
            "--harness",
            "codex",
            "--model",
            "syn-codex-model-01",
            "--effort",
            "high",
            "--",
            "Synthetic preview",
        ][..],
    ] {
        let out = fake.run_process(args, "");
        assert!(out.status.code().is_some(), "{args:?}");
    }
    assert!(fake.writes().is_empty(), "{:?}", fake.writes());
    assert_eq!(fake.spawn_count(), 0);
    // `ls` and `show` never read the catalogue either.
    let catalogue_reads = fake
        .invocations()
        .iter()
        .filter(|a| starts(a, &["cloud", "status"]))
        .count();
    assert_eq!(catalogue_reads, 3, "models twice and the preview once");
}

/// Updated for A2, with this reason: A2 adds the server's new-thread
/// routes, which call the core's `new_thread`, `preview_new_thread`,
/// `new_thread_status` and `cloud_options` as the CLI does
/// (`tests/web-api/u05_same_operation.rs` finds no new-thread logic of the
/// server's own). The server still builds no `room-cli` call: it names no
/// allowlist invocation and no argument builder.
#[test]
fn the_server_builds_no_new_thread_call() {
    let mut stack = vec![Path::new(ROOT).join("crates/server/src")];
    let mut seen = String::new();
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            stack.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for word in [
            "ThreadSpawn",
            "CloudStatus",
            "spawn_argv",
            "spawn_invocation",
        ] {
            assert!(!text.contains(word), "{path:?} names {word}");
        }
        seen.push_str(&text);
    }
    assert!(
        seen.contains("new_thread::operation::"),
        "A2's routes call the core's operations"
    );
}
