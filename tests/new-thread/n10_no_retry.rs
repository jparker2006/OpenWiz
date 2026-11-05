//! N10, the refusal half only (sheet the check: no retry in 0.0.4). There is no
//! `--retry` and no retry operation: asking for one is a usage error that
//! reads and calls nothing. An uncertain create stays uncertain whatever
//! runs next, and the owner resolves it in CloudRoom. With the fake's
//! request-ID table, the total stays one thread.

use std::fs;
use std::path::Path;

use serde_json::json;

use crate::fake::*;
use crate::support::ROOT;

const TEXT: &str = "Synthetic uncertain create";

#[test]
fn there_is_no_retry() {
    let fake = NewFake::new("n10_no_retry");
    for args in [
        &["new", "--retry", "op-n10", "--yes"][..],
        &["new", "--retry=op-n10", "--yes", "--json"][..],
        &["new", "--status", "op-n10", "--retry"][..],
        &[
            "new",
            "Synthetic project one",
            "--retry",
            "--harness",
            "codex",
        ][..],
    ] {
        let (code, out, err) = fake.cli(args, "");
        assert_eq!(code, 2, "{args:?}: {out}");
        assert!(out.is_empty(), "{out}");
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.starts_with("openwiz: unknown option --retry"), "{err}");
    }
    assert!(
        fake.invocations().is_empty(),
        "a refused retry reads nothing"
    );
    let help = openwiz_cli::help();
    assert!(!help.contains("--retry"), "{help}");
    assert!(
        help.contains("no retry"),
        "the help says there is none:\n{help}"
    );
    // Nothing in the product can retry a create (a tripwire).
    for dir in [
        "crates/core/src/new_thread",
        "crates/cli/src",
        "crates/adapters",
    ] {
        let mut stack = vec![Path::new(ROOT).join(dir)];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                stack.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            for word in ["retry_new_thread", "\"--retry\"", "retry-start"] {
                assert!(!text.contains(word), "{path:?} names {word}");
            }
        }
    }
}

#[test]
fn an_uncertain_create_stays_uncertain_and_one_thread_in_all() {
    let fake = NewFake::with(
        "n10_uncertain",
        vec![json!({"any_spawn": true, "creates": true, "abort": true})],
    );
    let args = new_args("Synthetic project one", "op-n10-u", &["--json"], TEXT);
    let (code, json) = json_of(&fake.run_process(&args, ""));
    assert_eq!(code, Some(6), "{json}");
    assert_eq!(fake.spawn_count(), 1);
    assert_eq!(fake.threads_created(), 1);

    // Everything the owner can run next, the fake now answering at once.
    fake.respond(vec![]);
    let (code, again) = json_of(&fake.run_process(&args, ""));
    assert_eq!(code, Some(6), "the same op id: still uncertain: {again}");
    assert_eq!(again["result"]["replayed"], true);
    let other_text = new_args(
        "Synthetic project one",
        "op-n10-u",
        &["--json"],
        "Synthetic other",
    );
    let (code, conflict) = json_of(&fake.run_process(&other_text, ""));
    assert_eq!(code, Some(8), "a different text is not a retry: {conflict}");
    assert_eq!(conflict["result"]["differs"], json!(["text"]));
    let (code, status) = json_of(&fake.run_process(&["new", "--status", "op-n10-u", "--json"], ""));
    assert_eq!(code, Some(6), "{status}");
    assert_eq!(status["result"]["report"]["status"], "uncertain");
    assert_eq!(fake.spawn_count(), 1, "no second call, ever");
    assert_eq!(fake.threads_created(), 1, "one thread in all");
    let record = fake.record("op-n10-u");
    assert_eq!(record["outcome"]["kind"], "uncertain");
    assert_eq!(record["text"], TEXT);
}

#[test]
fn the_fakes_request_id_table() {
    let fake = NewFake::new("n10_table");
    let program = fake.program();
    let run = |text: &str| {
        let argv = spawn_argv(PROJECT, "op-n10-table", text);
        std::process::Command::new(&program)
            .args(&argv)
            .output()
            .expect("run the fake")
    };
    let first = run(TEXT);
    assert_eq!(first.status.code(), Some(0));
    let same = run(TEXT);
    assert_eq!(same.status.code(), Some(0));
    assert_eq!(fake.threads_created(), 1, "the same content: no new thread");
    let other = run("Synthetic other content");
    assert_eq!(other.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&other.stderr).trim(),
        "Error: Failed to create thread: HTTP 409: Request ID was already used with different content"
    );
    assert_eq!(fake.threads_created(), 1);
    assert_eq!(fake.spawn_count(), 3);
}
