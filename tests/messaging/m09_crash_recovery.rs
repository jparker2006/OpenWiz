//! M9 Crash recovery: a sender killed at each step gives the recovery
//! table's outcome, and nothing is ever dispatched automatically.
//!
//! | The sender stopped | On recovery |
//! | --- | --- |
//! | after the claim, before the dispatching mark | failed (interrupted before dispatch) |
//! | after the dispatching mark, before the call | uncertain |
//! | during the call | uncertain |
//! | after the call, before the outcome was recorded | uncertain |
//!
//! The release `openwiz` is killed by the fake `room-cli` (`kill -9` of its
//! parent) during the version read (after the claim), during the call, and
//! after printing the receipt. No `room-cli` call sits between the
//! dispatching mark and the call, so that step uses the harness's test-only
//! crash point. Recovery runs when the op id is next used. **No server
//! exists (the implementation is not on `main`), so "server start" is not tested.**

use std::fs;
use std::process::Output;

use serde_json::{Value, json};

use crate::support::*;

const TEXT: &str = "Synthetic crash recovery";

fn killed(output: &Output) {
    use std::os::unix::process::ExitStatusExt;
    assert!(
        output.status.code().is_none(),
        "the sender was killed: {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.signal().is_some());
}

fn send_again(fake: &SendFake, op_id: &str) -> (Option<i32>, Value) {
    json_of(&fake.run_process(
        &[
            "send",
            "thr_syn_0001",
            "--op-id",
            op_id,
            "--json",
            "--",
            TEXT,
        ],
        "",
    ))
}

/// Recovery outcome checks shared by every crash point.
fn assert_recovered(fake: &SendFake, op_id: &str, status: &str, reason: &str, tells: u64) {
    let (code, json) = send_again(fake, op_id);
    assert_eq!(json["status"], status, "{json}");
    assert_eq!(json["result"]["kind"], "outcome");
    assert_eq!(json["result"]["recovered"], true, "{json}");
    assert_eq!(json["result"]["delivery"]["reason"], reason, "{json}");
    assert_eq!(code, Some(if status == "failed" { 5 } else { 6 }), "{json}");
    assert_eq!(fake.tell_count(), tells, "recovery made no call");
    let record = fake.record(op_id);
    assert_eq!(record["state"], "finished");
    assert_eq!(record["recovered"], true);
    assert_eq!(record["text"], TEXT, "the text is kept");

    // Nothing is dispatched automatically by anything that runs next: the
    // same op id again, the inventory, or a send to another target.
    let (_, again) = send_again(fake, op_id);
    assert_eq!(again["result"]["replayed"], true, "{again}");
    for args in [
        &["ls", "-l", "--json"][..],
        &["show", "thr_syn_0001", "--json"][..],
    ] {
        let out = fake.run_process(args, "");
        assert_eq!(out.status.code(), Some(0), "{args:?}");
    }
    let other = fake.run_process(
        &[
            "send",
            "thr_syn_0006",
            "--json",
            "--",
            "Synthetic other target",
        ],
        "",
    );
    assert_eq!(other.status.code(), Some(0));
    assert_eq!(json_of(&other).1["status"], "queued");
    assert_eq!(
        fake.tell_count(),
        tells + 1,
        "only the explicit new send made a call"
    );
    let to_target = fake
        .tells()
        .iter()
        .filter(|t| t[2] == "thr_syn_0001")
        .count() as u64;
    assert_eq!(to_target, tells, "thr_syn_0001 was never sent to again");
}

#[test]
fn stopped_after_the_claim_recovers_as_failed_before_dispatch() {
    // The release openwiz is killed during its version read, which comes
    // right after the claim.
    let fake = SendFake::with(
        "m09_after_claim_cli",
        vec![json!({"argv": ["--version"], "stdout": "0.43.1", "kill_parent": true})],
    );
    let out = fake.run_process(
        &[
            "send",
            "thr_syn_0001",
            "--op-id",
            "op-m09-claim",
            "--json",
            "--",
            TEXT,
        ],
        "",
    );
    killed(&out);
    assert_eq!(fake.record("op-m09-claim")["state"], "claimed");
    fake.override_responses(vec![]);
    assert_recovered(
        &fake,
        "op-m09-claim",
        "failed",
        "interrupted_before_dispatch",
        0,
    );

    // The same through the harness's crash point right after the claim.
    let fake = SendFake::new("m09_after_claim_harness", "send");
    let out = fake
        .harness(
            "thr_syn_0001",
            "queue",
            "op-m09-claim-h",
            "after-claim",
            TEXT,
        )
        .output()
        .unwrap();
    killed(&out);
    assert_recovered(
        &fake,
        "op-m09-claim-h",
        "failed",
        "interrupted_before_dispatch",
        0,
    );
}

#[test]
fn stopped_after_the_dispatching_mark_recovers_as_uncertain() {
    let fake = SendFake::new("m09_after_mark", "send");
    let out = fake
        .harness(
            "thr_syn_0001",
            "queue",
            "op-m09-mark",
            "after-dispatching-mark",
            TEXT,
        )
        .output()
        .unwrap();
    killed(&out);
    assert_eq!(fake.record("op-m09-mark")["state"], "dispatching");
    assert_eq!(fake.tell_count(), 0);
    assert_recovered(
        &fake,
        "op-m09-mark",
        "uncertain",
        "interrupted_after_dispatch_mark",
        0,
    );
}

#[test]
fn stopped_during_the_call_recovers_as_uncertain() {
    let fake = SendFake::with(
        "m09_during_call",
        vec![tell_response(
            "thr_syn_0001",
            "queue",
            json!({"kill_parent": true}),
        )],
    );
    let out = fake.run_process(
        &[
            "send",
            "thr_syn_0001",
            "--op-id",
            "op-m09-during",
            "--json",
            "--",
            TEXT,
        ],
        "",
    );
    killed(&out);
    assert_eq!(fake.tell_count(), 1);
    assert_eq!(fake.record("op-m09-during")["state"], "dispatching");
    fake.override_responses(vec![]);
    assert_recovered(
        &fake,
        "op-m09-during",
        "uncertain",
        "interrupted_after_dispatch_mark",
        1,
    );
}

#[test]
fn stopped_after_the_call_before_the_outcome_recovers_as_uncertain() {
    // The receipt says sent, but the sender dies before recording it: the
    // outcome is uncertain, never accepted on the receipt's word.
    let mut response = receipt("tell-sent.json");
    response["kill_parent_after_answer"] = json!(true);
    let fake = SendFake::with(
        "m09_after_call_cli",
        vec![tell_response("thr_syn_0001", "queue", response)],
    );
    let out = fake.run_process(
        &[
            "send",
            "thr_syn_0001",
            "--op-id",
            "op-m09-after",
            "--json",
            "--",
            TEXT,
        ],
        "",
    );
    killed(&out);
    assert_eq!(fake.tell_count(), 1);
    fake.override_responses(vec![]);
    assert_recovered(
        &fake,
        "op-m09-after",
        "uncertain",
        "interrupted_after_dispatch_mark",
        1,
    );

    let fake = SendFake::new("m09_after_call_harness", "send");
    let out = fake
        .harness(
            "thr_syn_0001",
            "queue",
            "op-m09-after-h",
            "after-call",
            TEXT,
        )
        .output()
        .unwrap();
    killed(&out);
    assert_eq!(fake.tell_count(), 1);
    assert_recovered(
        &fake,
        "op-m09-after-h",
        "uncertain",
        "interrupted_after_dispatch_mark",
        1,
    );
}

#[test]
fn an_abandoned_record_is_recovered_by_the_duplicate_check_too() {
    // A crash after the mark, then a new op id with the same text: the
    // abandoned record is recovered as uncertain and the new send refused
    // as a possible duplicate. Still no call.
    let fake = SendFake::new("m09_scan_recovers", "send");
    let out = fake
        .harness(
            "thr_syn_0001",
            "queue",
            "op-m09-scan",
            "after-dispatching-mark",
            TEXT,
        )
        .output()
        .unwrap();
    killed(&out);
    let (code, json) =
        json_of(&fake.run_process(&["send", "thr_syn_0001", "--json", "--", TEXT], ""));
    assert_eq!(code, Some(9), "{json}");
    assert_eq!(json["result"]["earlier"][0]["op_id"], "op-m09-scan");
    let record: Value =
        serde_json::from_str(&fs::read_to_string(fake.record_path("op-m09-scan")).unwrap())
            .unwrap();
    assert_eq!(record["state"], "finished");
    assert_eq!(record["delivery"]["kind"], "uncertain");
    assert_eq!(fake.tell_count(), 0);
}
