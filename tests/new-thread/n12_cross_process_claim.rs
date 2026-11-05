//! N12 Claiming across processes: many processes racing on one op id make
//! exactly one `spawn` call, counted across processes by the fake, and the
//! fake's request-ID table counts one thread.
//!
//! The racers are release `openwiz new --yes` processes and the self-exec
//! harness, which calls the core's `new_thread` with the file-backed record
//! the way a server would. **No server route creates threads until A2**, so
//! no server process races here.

use std::fs;
use std::path::PathBuf;
use std::process::Child;

use serde_json::{Value, json};

use crate::fake::*;

const RACERS_CLI: usize = 8;
const RACERS_HARNESS: usize = 4;

fn racing_fake(test: &str) -> NewFake {
    // A slow provider, so every racer is running while the call is made.
    NewFake::with(
        test,
        vec![json!({
            "any_spawn": true,
            "delay_ms": 400,
            "stdout_file": file("spawn-receipt-created.json"),
            "creates": true,
        })],
    )
}

enum Racer {
    Cli(Child),
    Harness(Child, PathBuf),
}

/// Starts every racer before waiting for any; returns (is the CLI, exit
/// code, report) for each. Every third racer is the harness.
fn race(fake: &NewFake, texts: &[&str]) -> Vec<(bool, Option<i32>, String)> {
    openwiz_release();
    let mut racers = Vec::new();
    for i in 0..RACERS_CLI + RACERS_HARNESS {
        let text = texts[i % texts.len()];
        if i % 3 == 2 {
            let (mut command, out) = fake.harness("op-n12-race", "none", text);
            racers.push(Racer::Harness(command.spawn().expect("a harness"), out));
        } else {
            let args = new_args("Synthetic project one", "op-n12-race", &["--json"], text);
            racers.push(Racer::Cli(fake.process(&args).spawn().expect("a racer")));
        }
    }
    racers
        .into_iter()
        .map(|racer| match racer {
            Racer::Cli(child) => {
                let out = child.wait_with_output().expect("racer output");
                (
                    true,
                    out.status.code(),
                    String::from_utf8_lossy(&out.stdout).into_owned(),
                )
            }
            Racer::Harness(child, file) => {
                let out = child.wait_with_output().expect("harness output");
                let report = fs::read_to_string(&file).unwrap_or_default();
                (false, out.status.code(), report)
            }
        })
        .collect()
}

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
fn many_processes_racing_on_one_op_id_make_exactly_one_spawn() {
    let fake = racing_fake("n12_race");
    let results = race(&fake, &["Synthetic race"]);
    assert_eq!(fake.spawn_count(), 1, "one provider call: {results:#?}");
    assert_eq!(fake.spawns().len(), 1);
    assert_eq!(fake.threads_created(), 1);
    let results = reports(&results);
    let fresh: Vec<&Value> = results
        .iter()
        .map(|(_, _, json)| json)
        .filter(|json| json["result"]["kind"] == "outcome" && json["result"]["replayed"] == false)
        .collect();
    assert_eq!(fresh.len(), 1, "exactly one owner: {results:#?}");
    assert_eq!(fresh[0]["status"], "created");
    assert!(
        results
            .iter()
            .any(|(_, _, json)| json["result"]["kind"] == "in_progress"),
        "no racer overlapped the owner: {results:#?}"
    );
    for (cli, code, json) in &results {
        let kind = json["result"]["kind"].as_str().unwrap_or_default();
        assert!(kind == "outcome" || kind == "in_progress", "{kind}: {json}");
        if *cli {
            let want = if kind == "in_progress" { 7 } else { 0 };
            assert_eq!(*code, Some(want), "{json}");
        }
    }
    // The dispatching mark was on disk when the one call was made.
    let at_spawn = fake.records_at_spawn(1);
    assert_eq!(at_spawn.len(), 1, "{at_spawn:?}");
    assert_eq!(at_spawn[0]["state"], "dispatching");
}

#[test]
fn racing_on_one_op_id_with_different_texts_still_creates_once() {
    let fake = racing_fake("n12_race_conflict");
    let results = race(&fake, &["Synthetic race A", "Synthetic race B"]);
    assert_eq!(fake.spawn_count(), 1, "{results:#?}");
    assert_eq!(fake.threads_created(), 1);
    let results = reports(&results);
    let fresh = results
        .iter()
        .filter(|(_, _, json)| {
            json["result"]["kind"] == "outcome" && json["result"]["replayed"] == false
        })
        .count();
    assert_eq!(fresh, 1, "{results:#?}");
    assert!(
        results
            .iter()
            .any(|(_, _, json)| json["result"]["kind"] == "conflicting_reuse"),
        "the other text was refused: {results:#?}"
    );
}
