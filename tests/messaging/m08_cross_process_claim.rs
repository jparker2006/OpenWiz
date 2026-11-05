//! M8 Claiming across processes: many processes racing on one op id make
//! exactly one `tell` call, counted across processes by the fake.
//!
//! The racers are release `openwiz send` processes and the store-level
//! harness (`send-harness`), which calls the core's `send` the way a server
//! would. **There is no server send route until the implementation**, so no server
//! process races here.

use std::process::Child;

use serde_json::{Value, json};

use crate::support::*;

const RACERS_CLI: usize = 8;
const RACERS_HARNESS: usize = 4;

fn racing_fake(test: &str) -> SendFake {
    // A slow provider, so every racer is running while the call is made.
    SendFake::with(
        test,
        vec![tell_response(
            "thr_syn_0001",
            "queue",
            json!({"delay_ms": 400, "stdout_file": send_fixtures().join("tell-sent.json").to_str().unwrap()}),
        )],
    )
}

/// Starts every racer before waiting for any, and returns (is the CLI,
/// exit code, stdout) for each. Every third racer is the harness.
fn race(fake: &SendFake, texts: &[&str]) -> Vec<(bool, Option<i32>, String)> {
    openwiz_release();
    let mut children: Vec<(bool, Child)> = Vec::new();
    for i in 0..RACERS_CLI + RACERS_HARNESS {
        let text = texts[i % texts.len()];
        let cli = i % 3 != 2;
        let child = if cli {
            fake.process(
                &[
                    "send",
                    "thr_syn_0001",
                    "--op-id",
                    "op-m08-race",
                    "--json",
                    "--",
                    text,
                ],
                "",
            )
            .spawn()
        } else {
            fake.harness("thr_syn_0001", "queue", "op-m08-race", "none", text)
                .spawn()
        };
        children.push((cli, child.expect("start a racer")));
    }
    assert_eq!(
        children.iter().filter(|(cli, _)| !cli).count(),
        RACERS_HARNESS
    );
    children
        .into_iter()
        .map(|(cli, child)| {
            let out = child.wait_with_output().expect("racer output");
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            (cli, out.status.code(), stdout)
        })
        .collect()
}

/// The racers' reports. Checked only after the tell count, so a racer that
/// failed cannot hide how many calls were made.
fn reports(results: &[(bool, Option<i32>, String)]) -> Vec<(bool, Option<i32>, Value)> {
    results
        .iter()
        .map(|(cli, code, out)| {
            let json = serde_json::from_str(out)
                .unwrap_or_else(|e| panic!("a racer printed no report ({e}): {out:?}"));
            (*cli, *code, json)
        })
        .collect()
}

#[test]
fn many_processes_racing_on_one_op_id_make_exactly_one_tell() {
    let fake = racing_fake("m08_race");
    let results = race(&fake, &["Synthetic race"]);
    assert_eq!(fake.tell_count(), 1, "one provider call: {results:#?}");
    assert_eq!(fake.tells().len(), 1);
    let results = reports(&results);
    let fresh: Vec<&Value> = results
        .iter()
        .map(|(_, _, json)| json)
        .filter(|json| json["result"]["kind"] == "outcome" && json["result"]["replayed"] == false)
        .collect();
    assert_eq!(fresh.len(), 1, "exactly one owner: {results:#?}");
    assert_eq!(fresh[0]["status"], "accepted");

    assert!(
        results
            .iter()
            .any(|(_, _, json)| json["result"]["kind"] == "in_progress"),
        "no racer overlapped the owner: {results:#?}"
    );
    for (cli, code, json) in &results {
        let kind = json["result"]["kind"].as_str().unwrap_or_default();
        assert!(kind == "outcome" || kind == "in_progress", "{kind}: {json}");
        if *cli && kind == "in_progress" {
            assert_eq!(*code, Some(7), "{json}");
        }
    }
}

#[test]
fn racing_on_one_op_id_with_different_texts_still_sends_once() {
    let fake = racing_fake("m08_race_conflict");
    let results = race(&fake, &["Synthetic race A", "Synthetic race B"]);
    assert_eq!(fake.tell_count(), 1, "{results:#?}");
    let results = reports(&results);
    let fresh = results
        .iter()
        .filter(|(_, _, json)| {
            json["result"]["kind"] == "outcome" && json["result"]["replayed"] == false
        })
        .count();
    assert_eq!(fresh, 1, "{results:#?}");
}
