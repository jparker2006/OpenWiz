//! M14 CLI: `--json` matches the text, the JSON matches
//! `contracts/messaging.v1.schema.json`, and each outcome's exit code is
//! documented.
//!
//! The text contract for `openwiz send` (provisional, like 0.0.1's):
//! 1. line 1: `openwiz send · op <op id> · <target id> · <title>`;
//! 2. `effects (read <age> before sending): <effect>` and `paused queue:
//! <words>`, when the target was read;
//! 3. the summary line: `<Mode> requested · <reported delivery>`;
//! 4. one `gap: <subject> · <detail>` line per gap;
//! 5. a refusal names what to do next (the acknowledgement flag), and a
//! replay says `replayed`.
//!
//! Unknowns print as `unknown (<reason>)`.

use openwiz_cli::send::{
    EXIT_CONFLICTING_REUSE, EXIT_DELIVERED, EXIT_IN_PROGRESS, EXIT_POSSIBLE_DUPLICATE,
    EXIT_RECORD_UNAVAILABLE, EXIT_SEND_FAILED, EXIT_UNCERTAIN,
};
use openwiz_cli::{EXIT_NOT_FOUND, EXIT_USAGE};
use serde_json::{Value, json};

use crate::support::*;

/// What the text must contain for a JSON report.
fn wanted(json: &Value) -> Vec<String> {
    let mut wanted = vec![
        format!("op {}", json["op_id"].as_str().unwrap()),
        json["target"]["id"].as_str().unwrap().to_owned(),
        json["summary"].as_str().unwrap().to_owned(),
    ];
    if let Some(title) = json["target"]["title"]["known"].as_str() {
        wanted.push(title.to_owned());
    }
    if !json["effects"].is_null() {
        let effects = &json["effects"];
        wanted.push(match effects["effect"]["known"]["text"].as_str() {
            Some(text) => text.to_owned(),
            None => format!(
                "unknown ({})",
                effects["effect"]["unknown"].as_str().unwrap()
            ),
        });
        wanted.push(match effects["paused_queue"]["known"]["text"].as_str() {
            Some(text) => text.to_owned(),
            None => format!(
                "unknown ({})",
                effects["paused_queue"]["unknown"].as_str().unwrap()
            ),
        });
    }
    for gap in json["gaps"].as_array().unwrap() {
        wanted.push(gap["detail"].as_str().unwrap().to_owned());
    }
    if json["result"]["replayed"] == true {
        wanted.push("replayed".to_owned());
    }
    if let Some(earlier) = json["result"]["earlier"].as_array() {
        for e in earlier {
            wanted.push(e["op_id"].as_str().unwrap().to_owned());
        }
    }
    wanted
}

/// Runs the same command with and without `--json`, each in its own fresh
/// fake and state (a second run on one state would replay the first).
fn both(
    test: &str,
    responses: Vec<Value>,
    setup: &[&[&str]],
    args: &[&str],
) -> (i32, Value, i32, String) {
    let mut runs = Vec::new();
    for json_flag in [true, false] {
        let fake = SendFake::with(&format!("{test}_{json_flag}"), responses.clone());
        for s in setup {
            let _ = fake.cli(s, "");
        }
        let mut all: Vec<&str> = args.to_vec();
        if json_flag {
            all.insert(1, "--json");
        }
        let (code, out, err) = fake.cli(&all, "");
        runs.push((code, out, err));
    }
    let (json_code, json_out, json_err) = runs.remove(0);
    let (text_code, text_out, _) = runs.remove(0);
    let json: Value = serde_json::from_str(&json_out)
        .unwrap_or_else(|e| panic!("{test}: not JSON ({e}): {json_out}{json_err}"));
    (json_code, json, text_code, text_out)
}

/// (test, responses, setup commands, command, exit code, status).
type Case<'a> = (
    &'a str,
    Vec<Value>,
    Vec<&'a [&'a str]>,
    Vec<&'a str>,
    i32,
    &'a str,
);

#[test]
fn the_json_matches_the_text_and_the_schema() {
    let abort = vec![tell_response(
        "thr_syn_0001",
        "queue",
        json!({"abort": true}),
    )];
    let cases: Vec<Case> = vec![
        (
            "m14_sent",
            vec![],
            vec![],
            vec!["thr_syn_0001", "--op-id", "op-m14", "--", "Synthetic"],
            0,
            "accepted",
        ),
        (
            "m14_steer_sent",
            vec![],
            vec![],
            vec![
                "thr_syn_0001",
                "--op-id",
                "op-m14",
                "--steer",
                "--",
                "Synthetic",
            ],
            0,
            "accepted",
        ),
        (
            "m14_queued",
            vec![],
            vec![],
            vec!["thr_syn_0006", "--op-id", "op-m14", "--", "Synthetic"],
            0,
            "queued",
        ),
        (
            "m14_unknown_effect",
            vec![],
            vec![],
            vec![
                "thr_syn_0015",
                "--op-id",
                "op-m14",
                "--resume-paused-queue",
                "--",
                "Synthetic",
            ],
            0,
            "accepted",
        ),
        (
            "m14_unsupported",
            vec![tell_response(
                "thr_syn_0002",
                "steer",
                json!({"exit":1, "stderr":"Error: HTTP 400: Unsupported mode"}),
            )],
            vec![],
            vec![
                "thr_syn_0002",
                "--op-id",
                "op-m14",
                "--steer",
                "--",
                "Synthetic",
            ],
            5,
            "failed",
        ),
        (
            "m14_paused",
            vec![],
            vec![],
            vec!["thr_syn_0011", "--op-id", "op-m14", "--", "Synthetic"],
            5,
            "failed",
        ),
        (
            "m14_not_found",
            vec![],
            vec![],
            vec!["thr_syn_9999", "--op-id", "op-m14", "--", "Synthetic"],
            3,
            "failed",
        ),
        (
            "m14_uncertain",
            abort.clone(),
            vec![],
            vec!["thr_syn_0001", "--op-id", "op-m14", "--", "Synthetic"],
            6,
            "uncertain",
        ),
        (
            "m14_replay",
            vec![],
            vec![&["thr_syn_0001", "--op-id", "op-m14", "--", "Synthetic"]],
            vec!["thr_syn_0001", "--op-id", "op-m14", "--", "Synthetic"],
            0,
            "accepted",
        ),
        (
            "m14_conflict",
            vec![],
            vec![&["thr_syn_0001", "--op-id", "op-m14", "--", "Synthetic"]],
            vec!["thr_syn_0001", "--op-id", "op-m14", "--", "Synthetic other"],
            8,
            "conflicting_reuse",
        ),
        (
            "m14_duplicate",
            abort,
            vec![&["thr_syn_0001", "--op-id", "op-m14-first", "--", "Synthetic"]],
            vec!["thr_syn_0001", "--op-id", "op-m14", "--", "Synthetic"],
            9,
            "possible_duplicate_refused",
        ),
    ];
    let schema_errors_of = |json: &Value| validate_report(json);
    for (test, responses, setup, args, code, status) in cases {
        let (json_code, json, text_code, text) = both(test, responses, &setup, &args);
        assert_eq!(json_code, code, "{test}: {json}");
        assert_eq!(text_code, code, "{test}: {text}");
        assert_eq!(json["status"], status, "{test}: {json}");
        let errors = schema_errors_of(&json);
        assert!(errors.is_empty(), "{test}: {errors:#?}\n{json}");
        let first = text.lines().next().unwrap_or_default();
        assert!(
            first.starts_with("openwiz send · op op-m14 · "),
            "{test}: {first}"
        );
        for w in wanted(&json) {
            assert!(text.contains(&w), "{test}: {w:?} not in the text:\n{text}");
        }
        assert_eq!(
            text.lines().filter(|l| l.starts_with("gap: ")).count(),
            json["gaps"].as_array().unwrap().len(),
            "{test}"
        );
    }
}

#[test]
fn every_exit_code_is_documented() {
    assert_eq!(EXIT_DELIVERED, 0);
    let help = openwiz_cli::help();
    for (code, words) in [
        (EXIT_DELIVERED, "send: queued, or sent"),
        (EXIT_USAGE, "usage error"),
        (EXIT_NOT_FOUND, "send: the target was not found"),
        (EXIT_SEND_FAILED, "send: failed"),
        (EXIT_UNCERTAIN, "send: uncertain"),
        (EXIT_IN_PROGRESS, "send: in progress"),
        (
            EXIT_CONFLICTING_REUSE,
            "send: the op id is bound to a different",
        ),
        (EXIT_POSSIBLE_DUPLICATE, "send: possible duplicate"),
        (EXIT_RECORD_UNAVAILABLE, "send: the send record"),
    ] {
        let line = help
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{code} ")) && l.contains(words));
        assert!(
            line.is_some(),
            "exit {code} ({words}) is not documented:\n{help}"
        );
    }

    assert!(
        !help.lines().any(|l| l.trim_start().starts_with("4 ")),
        "exit 4 is still documented:\n{help}"
    );
}

#[test]
fn help_documents_send_its_flags_and_its_limits() {
    let help = openwiz_cli::help();
    for words in [
        "openwiz send <id> [<text>]",
        "--steer",
        "--op-id",
        "--resume-paused-queue",
        "--accept-possible-duplicate",
        "16384 bytes",
        "from stdin",
        "never auto",
        "the provider accepted the message, not that the work completed",
        "can change between the read and the dispatch",
        // Send attaches staged files in Queue mode only, all ready or nothing.
        "--attach <path>",
        "--attachment <id>",
        "Steer can't carry attachments",
        "Delivery to the agent's machine is never",
        "A file path in the text is sent as text",
        "one target",
    ] {
        assert!(help.contains(words), "{words:?} not in --help:\n{help}");
    }

    assert!(
        help.contains("send prints its report on stdout for every outcome"),
        "{help}"
    );
    let fake = SendFake::new("m14_help", "send");
    let (code, out, _) = fake.cli(&["--help"], "");
    assert_eq!(code, 0);
    assert_eq!(out, help);
    assert!(fake.invocations().is_empty());
}

#[test]
fn errors_go_to_stderr_with_stdout_empty() {
    let fake = SendFake::new("m14_errors", "send");
    for args in [
        vec![],
        vec!["thr_syn_0001", "--bogus", "--", "Synthetic"],
        vec!["thr_syn_0001", "--op-id"],
        vec!["thr_syn_0001", "--json=yes", "--", "Synthetic"],
    ] {
        let (code, out, err) = fake.cli(&args, "");
        assert_eq!(code, EXIT_USAGE, "{args:?}");
        assert!(out.is_empty(), "{out}");
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.starts_with("openwiz: "), "{err}");
    }
}

#[test]
fn the_release_build_sends_through_the_fake() {
    let fake = SendFake::new("m14_release", "send");
    let out = fake.run_process(&["send", "thr_syn_0001", "--json"], "Synthetic from stdin");
    let (code, json) = json_of(&out);
    assert_eq!(code, Some(0), "{json}");
    assert!(validate_report(&json).is_empty(), "{json}");
    let op_id = json["op_id"].as_str().unwrap();
    assert!(op_id.starts_with("op-") && op_id.len() == 35, "{op_id}");
    assert_eq!(fake.record(op_id)["delivery"]["kind"], "accepted");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(op_id),
        "a generated op id is shown before sending"
    );
    let tells = fake.tells();
    assert_eq!(tells.len(), 1);
    assert_eq!(tells[0][7], "Synthetic from stdin");
}

#[test]
fn help_documents_preview_and_check() {
    use openwiz_cli::send::EXIT_NO_RECORD;
    let help = openwiz_cli::help();
    for words in [
        "openwiz send <id> --preview [--steer] [--json]",
        "openwiz send <id> --op-id <op id> --check [--json]",
        "--preview",
        "--check",
        "no claim and no call",
        "or one in the same mode is still in progress",
        "openwiz.send-preview.v1",
        "openwiz.send-lookup.v1",
    ] {
        assert!(help.contains(words), "{words:?} not in --help:\n{help}");
    }
    assert_eq!(EXIT_NO_RECORD, 11);
    for (code, words) in [
        (EXIT_NO_RECORD, "send --check: no record"),
        (EXIT_POSSIBLE_DUPLICATE, "uncertain or in-progress send"),
        (EXIT_DELIVERED, "send --preview: a send would go ahead"),
        (EXIT_NOT_FOUND, "send --preview: the target was not found"),
        (EXIT_SEND_FAILED, "send --preview: a send would be refused"),
    ] {
        let line = help
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{code} ")) && l.contains(words));
        assert!(
            line.is_some(),
            "exit {code} ({words}) is not documented:\n{help}"
        );
    }
}
