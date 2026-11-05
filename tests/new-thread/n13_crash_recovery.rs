//! N13 Crash recovery: a create stopped at each step gives the recovery
//! table's outcome, and nothing is dispatched or checked automatically on
//! restart or on server start.
//!
//! | The process stopped | On recovery |
//! | --- | --- |
//! | after the claim, before the dispatching mark | failed (interrupted before dispatch) |
//! | after the dispatching mark, before the call | uncertain |
//! | during the call | uncertain |
//! | after the call, before the outcome was recorded | uncertain |
//!
//! The release `openwiz new` is killed by the fake (`kill -9` of its parent)
//! during the version read (right after the claim), during the call, and
//! after the receipt is printed. No `room-cli` call sits right after the
//! dispatching mark, so that step uses the self-exec harness's crash point
//! (and the harness repeats the others). Recovery happens when the op id is
//! next used; a status read shows it without writing.

use std::process::Output;
use std::sync::Arc;

use openwiz_server::{Config, bind, start};
use serde_json::{Value, json};

use crate::fake::*;
use crate::support::clock;

const TEXT: &str = "Synthetic crash recovery";

fn killed(output: &Output) {
    use std::os::unix::process::ExitStatusExt;
    assert!(
        output.status.code().is_none() && output.status.signal().is_some(),
        "the process was killed: {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create(fake: &NewFake, op_id: &str) -> Output {
    fake.run_process(
        &new_args("Synthetic project one", op_id, &["--json"], TEXT),
        "",
    )
}

/// What every crash point must recover to, and that nothing runs again.
fn assert_recovered(fake: &NewFake, op_id: &str, status: &str, reason: &str, spawns: u64) {
    // A status read first: it shows the recovery without writing it.
    let before = std::fs::read(fake.record_path(op_id)).expect("a record");
    let (code, read) = json_of(&fake.run_process(&["new", "--status", op_id, "--json"], ""));
    assert_eq!(read["result"]["report"]["status"], status, "{read}");
    assert_eq!(code, Some(if status == "failed" { 5 } else { 6 }));
    assert_eq!(
        std::fs::read(fake.record_path(op_id)).unwrap(),
        before,
        "a status read never writes"
    );

    let (code, json) = json_of(&create(fake, op_id));
    assert_eq!(json["status"], status, "{json}");
    assert_eq!(json["result"]["kind"], "outcome");
    assert_eq!(json["result"]["recovered"], true, "{json}");
    assert_eq!(json["result"]["outcome"]["reason"], reason, "{json}");
    assert_eq!(code, Some(if status == "failed" { 5 } else { 6 }), "{json}");
    assert_eq!(fake.spawn_count(), spawns, "recovery made no call");
    let record = fake.record(op_id);
    assert_eq!(record["state"], "finished");
    assert_eq!(record["recovered"], true);
    assert_eq!(record["text"], TEXT, "the text is kept");

    // Nothing is dispatched by what runs next: the op id again, a status
    // read, the inventory, the options, or a server start and its reads.
    let (_, again) = json_of(&create(fake, op_id));
    assert_eq!(again["result"]["replayed"], true, "{again}");
    for args in [
        &["new", "--status", op_id, "--json"][..],
        &["ls", "-l", "--json"][..],
        &["show", "thr_syn_0001", "--json"][..],
        &["models", "--json"][..],
    ] {
        let out = fake.run_process(args, "");
        assert!(out.status.code().is_some(), "{args:?}");
    }
    let server = start(
        bind(0).expect("bind loopback"),
        Config::default(),
        Arc::new(fake.source()),
        Arc::new(clock()),
        Arc::new(|_: &str| {}),
    )
    .expect("start the server");
    for target in ["/api/v1/inventory", "/api/v1/sessions/thr_syn_0001"] {
        let mut stream = std::net::TcpStream::connect(server.local_addr()).unwrap();
        use std::io::{Read, Write};
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            server.local_addr().port()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{target}: {response}");
    }
    assert_eq!(fake.spawn_count(), spawns, "nothing ran by itself");
    // Only an explicit new create, under a new op id, makes a call.
    let (code, other) = json_of(&create(fake, &format!("{op_id}-new")));
    assert_eq!(code, Some(0), "{other}");
    assert_eq!(fake.spawn_count(), spawns + 1);
}

#[test]
fn stopped_after_the_claim_recovers_as_failed_before_dispatch() {
    let fake = NewFake::with(
        "n13_after_claim_cli",
        vec![json!({"argv": ["--version"], "stdout": "v127", "kill_parent": true})],
    );
    killed(&create(&fake, "op-n13-claim"));
    assert_eq!(fake.record("op-n13-claim")["state"], "claimed");
    fake.respond(vec![]);
    assert_recovered(
        &fake,
        "op-n13-claim",
        "failed",
        "interrupted_before_dispatch",
        0,
    );

    let fake = NewFake::new("n13_after_claim_harness");
    let (mut harness, _) = fake.harness("op-n13-claim-h", "after-claim", TEXT);
    killed(&harness.output().unwrap());
    assert_recovered(
        &fake,
        "op-n13-claim-h",
        "failed",
        "interrupted_before_dispatch",
        0,
    );
}

#[test]
fn stopped_after_the_dispatching_mark_recovers_as_uncertain() {
    let fake = NewFake::new("n13_after_mark");
    let (mut harness, _) = fake.harness("op-n13-mark", "after-dispatching-mark", TEXT);
    killed(&harness.output().unwrap());
    assert_eq!(fake.record("op-n13-mark")["state"], "dispatching");
    assert_eq!(fake.spawn_count(), 0);
    assert_recovered(
        &fake,
        "op-n13-mark",
        "uncertain",
        "interrupted_after_dispatch_mark",
        0,
    );
}

#[test]
fn stopped_during_the_call_recovers_as_uncertain() {
    let fake = NewFake::with(
        "n13_during_call",
        vec![json!({"any_spawn": true, "creates": true, "kill_parent": true})],
    );
    killed(&create(&fake, "op-n13-call"));
    assert_eq!(fake.record("op-n13-call")["state"], "dispatching");
    assert_eq!(fake.spawn_count(), 1);
    fake.respond(vec![]);
    assert_recovered(
        &fake,
        "op-n13-call",
        "uncertain",
        "interrupted_after_dispatch_mark",
        1,
    );
    assert_eq!(
        fake.threads_created(),
        2,
        "the lost one, and the new op id's"
    );
}

#[test]
fn stopped_after_the_call_recovers_as_uncertain() {
    let fake = NewFake::with(
        "n13_after_answer",
        vec![json!({
            "any_spawn": true,
            "creates": true,
            "stdout_file": file("spawn-receipt-created.json"),
            "kill_parent_after_answer": true,
        })],
    );
    killed(&create(&fake, "op-n13-answer"));
    assert_eq!(fake.record("op-n13-answer")["state"], "dispatching");
    fake.respond(vec![]);
    assert_recovered(
        &fake,
        "op-n13-answer",
        "uncertain",
        "interrupted_after_dispatch_mark",
        1,
    );

    let fake = NewFake::new("n13_after_call_harness");
    let (mut harness, _) = fake.harness("op-n13-call-h", "after-call", TEXT);
    killed(&harness.output().unwrap());
    assert_eq!(fake.spawn_count(), 1);
    assert_recovered(
        &fake,
        "op-n13-call-h",
        "uncertain",
        "interrupted_after_dispatch_mark",
        1,
    );
}

/// The dispatching mark is synced before the one call: what was on disk at
/// the moment the fake received it.
#[test]
fn the_dispatching_mark_is_on_disk_before_the_call() {
    let fake = NewFake::new("n13_mark_on_disk");
    let (code, json) = json_of(&create(&fake, "op-n13-disk"));
    assert_eq!(code, Some(0), "{json}");
    let records: Vec<Value> = fake.records_at_spawn(1);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["state"], "dispatching");
    assert_eq!(records[0]["op_id"], "op-n13-disk");
    assert_eq!(records[0]["text"], TEXT);
    assert!(records[0]["effects"].is_object(), "effects before the call");
}
