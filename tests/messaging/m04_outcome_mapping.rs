//! M4 Outcome mapping, for every row of the receipt table. The requested
//! mode and the reported delivery are kept apart. "Sent" never implies
//! completion, and whether a steer joined the turn stays unknown.
//!
//! | The provider says | Reported delivery |
//! | --- | --- |
//! | busy Queue or Claude queued Steer, exact observed row | queued, with reason or explicitly not reported |
//! | observed sent shape, Queue or Steer, strictly bound to this request | accepted, shown as "sent" |
//! | not found, invalid input, unsupported mode | failed, with the reason |
//! | a timeout, a crash, non-JSON, unexpected fields, an undocumented error | uncertain |
//!
//! Nothing `room-cli` prints reaches any output: planted markers in its
//! stderr and stdout never appear in a report, the CLI's output or the
//! record.

use std::time::{Duration, Instant};

use openwiz_core::messaging::Mode;
use openwiz_core::messaging::send::{
    Delivery, FailReason, SendReport, SendResult, Status, UncertainReason,
};
use openwiz_core::model::Fact;
use serde_json::{Value, json};

use crate::support::*;

pub fn delivery(report: &SendReport) -> &Delivery {
    match &report.result {
        SendResult::Outcome { delivery, .. } => delivery,
        other => panic!("expected an outcome, got {other:?}"),
    }
}

pub fn uncertain_reason(report: &SendReport) -> UncertainReason {
    match delivery(report) {
        Delivery::Uncertain { reason, .. } => *reason,
        other => panic!("expected uncertain, got {other:?}"),
    }
}

#[test]
fn unobserved_documented_queued_receipts_are_uncertain() {
    for file in [
        "tell-queued-time.json",
        "tell-queued-thread-busy.json",
        "tell-queued-turn-starting.json",
        "tell-queued-provisioning.json",
        "tell-queued-host-offline.json",
        "tell-queued-interaction.json",
        "tell-queued-plugin.json",
    ] {
        let fake = SendFake::with(
            &format!("m04_{file}"),
            vec![tell_response("thr_syn_0001", "queue", receipt(file))],
        );
        let report = fake.report(&request(
            "thr_syn_0001",
            Mode::Queue,
            "Synthetic",
            "op-m04-q",
        ));
        assert_eq!(report.status, Status::Uncertain, "{file}");
        assert_eq!(
            uncertain_reason(&report),
            UncertainReason::UnexpectedReceipt
        );
        assert!(
            report.gaps.iter().any(|g| g.detail.contains("unobserved")),
            "{file}: {:?}",
            report.gaps
        );
        assert_eq!(fake.tell_count(), 1);
    }
}

#[test]
fn sent_is_accepted_and_never_completion() {
    let fake = SendFake::new("m04_sent", "send");
    let queue = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        "Synthetic",
        "op-m04-sent-q",
    ));
    assert_eq!(queue.status, Status::Accepted);
    assert!(
        queue.summary.starts_with("Queue requested · sent"),
        "{}",
        queue.summary
    );
    match delivery(&queue) {
        Delivery::Accepted {
            joined_running_turn,
            text,
        } => {
            assert!(
                matches!(joined_running_turn, Fact::Unknown(why) if why.contains("not reported")),
                "{joined_running_turn:?}"
            );
            assert!(text.contains("not that the work completed"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    let json = serde_json::to_string(&queue).unwrap();
    assert!(!json.contains("complete\""), "{json}");

    let fake = SendFake::with(
        "m04_sent_steer",
        vec![tell_response(
            "thr_syn_0001",
            "steer",
            receipt("tell-steer-sent-observed.json"),
        )],
    );
    let steer = fake.report(&request(
        "thr_syn_0001",
        Mode::Steer,
        "Synthetic",
        "op-m04-sent-s",
    ));
    assert_eq!(steer.status, Status::Accepted);
    assert!(
        steer.summary.starts_with(
            "Steer requested · sent; whether it joined the running turn is not reported"
        ),
        "{}",
        steer.summary
    );
    match delivery(&steer) {
        Delivery::Accepted {
            joined_running_turn,
            text,
        } => {
            assert!(
                matches!(joined_running_turn, Fact::Unknown(why) if why.contains("not reported")),
                "{joined_running_turn:?}"
            );
            assert!(text.contains("not that the work completed"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    let json = serde_json::to_string(&steer).unwrap();
    assert!(!json.contains("complete\""), "{json}");
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn documented_rejections_are_failed_with_the_reason() {
    let fake = SendFake::with(
        "m04_rejections",
        vec![
            tell_response(
                "thr_syn_0001",
                "queue",
                json!({"exit": 1, "stderr": "Error: HTTP 404: Thread not found"}),
            ),
            tell_response(
                "thr_syn_0002",
                "steer",
                json!({"exit":1, "stderr":"Error: HTTP 400: Unsupported mode"}),
            ),
        ],
    );
    for (target, mode, reason) in [
        ("thr_syn_0005", Mode::Queue, FailReason::InvalidInput),
        ("thr_syn_0002", Mode::Steer, FailReason::UnsupportedMode),
        // Found by the read, gone by the call.
        ("thr_syn_0001", Mode::Queue, FailReason::NotFound),
    ] {
        let report = fake.report(&request(
            target,
            mode,
            "Synthetic",
            &format!("op-m04-{target}"),
        ));
        assert_eq!(report.status, Status::Failed, "{target}");
        match delivery(&report) {
            Delivery::Failed { reason: got, .. } => assert_eq!(*got, reason, "{target}"),
            other => panic!("{target}: {other:?}"),
        }
    }
    assert_eq!(fake.tell_count(), 3, "one call each");
}

#[test]
fn everything_else_is_uncertain() {
    let cases: Vec<(&str, Value, UncertainReason)> = vec![
        ("hang", json!({"hang": true}), UncertainReason::TimedOut),
        (
            "abort",
            json!({"abort": true}),
            UncertainReason::EndedWithoutExitCode,
        ),
        (
            "not_json",
            receipt("tell-not-json.txt"),
            UncertainReason::NotJson,
        ),
        ("empty", json!({"stdout": ""}), UncertainReason::NotJson),
        (
            "extra_field",
            receipt("tell-drift-extra-field.json"),
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "http_500",
            json!({"exit": 1, "stderr": "Error: HTTP 500: Synthetic server error"}),
            UncertainReason::UndocumentedError,
        ),
        (
            "exit_2_silent",
            json!({"exit": 2}),
            UncertainReason::UndocumentedError,
        ),
        (
            "404_other_wording",
            json!({"exit": 1, "stderr": "Error: HTTP 404: Synthetic other wording"}),
            UncertainReason::UndocumentedError,
        ),
    ];
    for (name, response, reason) in cases {
        let fake = SendFake::with(
            &format!("m04_uncertain_{name}"),
            vec![tell_response("thr_syn_0001", "queue", response)],
        );
        let started = Instant::now();
        let report = fake.report(&request(
            "thr_syn_0001",
            Mode::Queue,
            "Synthetic",
            "op-m04-u",
        ));
        assert_eq!(report.status, Status::Uncertain, "{name}: {report:#?}");
        assert_eq!(uncertain_reason(&report), reason, "{name}");
        assert!(
            report.summary.starts_with("Queue requested · uncertain"),
            "{name}: {}",
            report.summary
        );
        assert_eq!(fake.tell_count(), 1, "{name}: no second call");
        if name == "hang" {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "bounded by the budget"
            );
        }
    }
}

#[test]
fn the_requested_mode_and_the_reported_delivery_are_kept_apart() {
    let fake = SendFake::new("m04_kept_apart", "send");
    let report = fake.report(&request(
        "thr_syn_0012",
        Mode::Steer,
        "Synthetic",
        "op-m04-apart",
    ));
    assert_eq!(report.requested_mode, Mode::Steer);
    assert_eq!(report.status, Status::Uncertain);
    assert!(report.summary.starts_with("Steer requested · uncertain"));
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["requested_mode"], "steer");
    assert_eq!(json["result"]["delivery"]["kind"], "uncertain");
    assert!(json["result"]["delivery"]["waiting_on"].is_null());
}

/// Markers planted in everything `room-cli` prints on failure.
const MARKERS: &[&str] = &["syn-marker", "/synthetic/marker-path", "Synthetic failure"];

fn assert_no_marker(label: &str, text: &str) {
    for marker in MARKERS {
        assert!(
            !text.contains(marker),
            "{label}: {marker:?} leaked into:\n{text}"
        );
    }
}

#[test]
fn no_raw_room_cli_text_reaches_any_output() {
    let stderr =
        "Synthetic failure syn-marker-stderr at /synthetic/marker-path TOKEN=syn-marker-token";
    let cases: Vec<(&str, Vec<Value>)> = vec![
        (
            "tell_stderr",
            vec![tell_response(
                "thr_syn_0001",
                "queue",
                json!({"exit": 1, "stderr": stderr}),
            )],
        ),
        (
            "tell_stdout",
            vec![tell_response(
                "thr_syn_0001",
                "queue",
                receipt("tell-not-json.txt"),
            )],
        ),
        (
            "tell_unsafe_queued_id",
            vec![tell_response(
                "thr_syn_0001",
                "queue",
                receipt("tell-drift-unsafe-queued-id.json"),
            )],
        ),
        (
            "show_stderr",
            vec![
                json!({"argv": ["thread", "show", "thr_syn_0001", "--json"], "exit": 1, "stderr": stderr}),
            ],
        ),
        (
            "list_stderr",
            vec![json!({"argv": ["thread", "list", "--json"], "exit": 1, "stderr": stderr})],
        ),
        (
            "version_stdout",
            vec![json!({"argv": ["--version"], "stdout": "Synthetic failure syn-marker-version"})],
        ),
        (
            "version_stderr",
            vec![json!({"argv": ["--version"], "exit": 1, "stderr": stderr})],
        ),
    ];
    for (name, responses) in cases {
        let fake = SendFake::with(&format!("m04_markers_{name}"), responses);
        for (i, json_flag) in [false, true].into_iter().enumerate() {
            let op_id = format!("op-m04-marker-{i}");
            let mut args = vec!["thr_syn_0001", "--op-id", &op_id];
            if json_flag {
                args.push("--json");
            }
            args.extend(["--accept-possible-duplicate", "--", "Synthetic"]);
            let (code, out, err) = fake.cli(&args, "");
            assert!(code == 0 || code >= 3, "{name}: exit {code}: {out}{err}");
            assert!(!out.is_empty(), "{name}: a report is printed: {err}");
            assert_no_marker(&format!("{name} stdout"), &out);
            assert_no_marker(&format!("{name} stderr"), &err);
            if fake.record_path(&op_id).exists() {
                let record = std::fs::read_to_string(fake.record_path(&op_id)).unwrap();
                assert_no_marker(&format!("{name} record"), &record);
            }
        }
    }
}

#[test]
fn a_call_that_never_started_is_failed_not_uncertain() {
    use openwiz_adapter_cloudroom::source::CloudRoomSource;
    use openwiz_core::messaging::effects::TargetRecord;
    use openwiz_core::messaging::send::{Messenger, TellReply, send};
    use openwiz_core::messaging::{InvalidRequest, Mode as M};
    use openwiz_core::model::SessionId;
    use openwiz_core::source::SourceError;
    use std::time::Instant;

    /// Reads through the fake; the one call through a program that does not exist.
    struct NoProgram {
        reads: CloudRoomSource,
        missing: CloudRoomSource,
    }
    impl Messenger for NoProgram {
        fn name(&self) -> &str {
            Messenger::name(&self.reads)
        }
        fn tested_version(&self) -> &str {
            Messenger::tested_version(&self.reads)
        }
        fn check_target_id(&self, id: &SessionId) -> Result<(), InvalidRequest> {
            self.reads.check_target_id(id)
        }
        fn version(&self, deadline: Instant) -> Result<String, SourceError> {
            Messenger::version(&self.reads, deadline)
        }
        fn read_target(&self, id: &SessionId, d: Instant) -> Result<TargetRecord, SourceError> {
            self.reads.read_target(id, d)
        }
        fn tell(&self, id: &SessionId, mode: M, text: &str, d: Instant) -> TellReply {
            self.missing.tell(id, mode, text, d)
        }
    }
    let fake = SendFake::new("m04_never_started", "send");
    let messenger = NoProgram {
        reads: fake.source(),
        missing: CloudRoomSource::new(fake.dir.join("no-such-room-cli")),
    };
    let report = send(
        &messenger,
        &fake.store(),
        &clock(),
        &request("thr_syn_0001", M::Queue, "Synthetic", "op-m04-never"),
    )
    .unwrap();
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["status"], "failed", "{json}");
    assert_eq!(json["result"]["delivery"]["reason"], "call_not_started");
    assert_eq!(fake.tell_count(), 0);
}

#[test]
fn the_target_reads_not_found_is_an_exact_line() {
    let fake = SendFake::with(
        "m04_not_found_exact",
        vec![json!({
            "argv": ["thread", "show", "thr_syn_0001", "--json"],
            "exit": 1,
            "stderr": "Synthetic proxy says Error: HTTP 404: Thread not found upstream"
        })],
    );
    let report = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        "Synthetic",
        "op-m04-nf",
    ));
    match delivery(&report) {
        Delivery::Failed { reason, .. } => assert_eq!(*reason, FailReason::TargetNotRead),
        other => panic!("{other:?}"),
    }
    assert_eq!(fake.tell_count(), 0);
}

#[test]
fn observed_sent_receipt_is_accepted_once() {
    let fake = SendFake::with(
        "m04_observed_sent",
        vec![tell_response(
            "thr_syn_0001",
            "queue",
            receipt("tell-sent-observed.json"),
        )],
    );
    let (code, report) = fake.cli_json(
        &[
            "thr_syn_0001",
            "--op-id",
            "op-observed-sent",
            "--",
            "Synthetic",
        ],
        "",
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["status"], "accepted");
    assert_eq!(report["result"]["delivery"]["kind"], "accepted");
    assert!(
        report["summary"]
            .as_str()
            .unwrap()
            .starts_with("Queue requested · sent")
    );
    assert_eq!(fake.tell_count(), 1);
    assert_eq!(
        fake.record("op-observed-sent")["delivery"]["kind"],
        "accepted"
    );
}

#[test]
fn observed_receipt_guards_fail_closed() {
    let good = json!({"threadId": "thr_syn_0001", "ok": true, "delivery": "sent", "mode": "queue"});
    let mut cases = vec![
        ("old_sent".to_owned(), r#"{"delivery":"sent"}"#.to_owned()),
        (
            "old_sent_null".to_owned(),
            r#"{"delivery":"sent","queuedMessage":null}"#.to_owned(),
        ),
        (
            "array".to_owned(),
            r#"["thr_syn_0001",true,"sent","queue"]"#.to_owned(),
        ),
        ("null".to_owned(), "null".to_owned()),
    ];
    for (key, bad_values) in [
        (
            "threadId",
            vec![
                json!("thr_syn_0002"),
                json!(null),
                json!(7),
                json!({"id":"thr_syn_0001"}),
            ],
        ),
        (
            "ok",
            vec![json!(false), json!(null), json!("true"), json!(1)],
        ),
        (
            "delivery",
            vec![
                json!("queued"),
                json!("unsupported"),
                json!(null),
                json!({"sent":null}),
            ],
        ),
        (
            "mode",
            vec![
                json!("steer"),
                json!("auto"),
                json!(null),
                json!({"queue":null}),
            ],
        ),
    ] {
        let mut missing = good.clone();
        missing.as_object_mut().unwrap().remove(key);
        cases.push((format!("missing_{key}"), missing.to_string()));
        for (i, bad) in bad_values.into_iter().enumerate() {
            let mut value = good.clone();
            value[key] = bad;
            cases.push((format!("bad_{key}_{i}"), value.to_string()));
        }
        // Raw JSON preserves duplicate keys instead of Value overwriting them.
        cases.push((
            format!("duplicate_{key}"),
            format!("{{\"{key}\":{},{}", good[key], &good.to_string()[1..]),
        ));
    }
    let mut extra = good;
    extra["syntheticExtra"] = json!(true);
    cases.push(("extra".to_owned(), extra.to_string()));
    for (name, stdout) in cases {
        let fake = SendFake::with(
            &format!("m04_guard_{name}"),
            vec![tell_response(
                "thr_syn_0001",
                "queue",
                json!({"stdout":stdout}),
            )],
        );
        let report = fake.report(&request(
            "thr_syn_0001",
            Mode::Queue,
            "Synthetic",
            "op-guard",
        ));
        assert_eq!(report.status, Status::Uncertain, "{name}: {report:?}");
        assert_eq!(
            uncertain_reason(&report),
            UncertainReason::UnexpectedReceipt,
            "{name}"
        );
        assert_eq!(
            fake.record("op-guard")["delivery"]["kind"],
            "uncertain",
            "{name}"
        );
        let again = fake.report(&request(
            "thr_syn_0001",
            Mode::Queue,
            "Synthetic",
            "op-guard",
        ));
        assert_eq!(again.status, Status::Uncertain, "{name}: replay");
        assert_eq!(fake.tell_count(), 1, "{name}: never retry");
    }
}

#[test]
fn unobserved_receipts_stay_uncertain_and_say_so() {
    for (name, mode, stdout) in [
        (
            "queued",
            Mode::Queue,
            r#"{"threadId":"thr_syn_0001","ok":true,"delivery":"queued","mode":"queue"}"#,
        ),
        (
            "mode_mismatch",
            Mode::Steer,
            r#"{"threadId":"thr_syn_0001","ok":true,"delivery":"sent","mode":"queue"}"#,
        ),
        (
            "unsupported",
            Mode::Queue,
            r#"{"threadId":"thr_syn_0001","ok":false,"delivery":"unsupported","mode":"queue"}"#,
        ),
    ] {
        let mode_word = if mode == Mode::Steer {
            "steer"
        } else {
            "queue"
        };
        let fake = SendFake::with(
            &format!("m04_unobserved_{name}"),
            vec![tell_response(
                "thr_syn_0001",
                mode_word,
                json!({"stdout":stdout}),
            )],
        );
        let report = fake.report(&request("thr_syn_0001", mode, "Synthetic", "op-unobserved"));
        assert_eq!(report.status, Status::Uncertain, "{name}: {report:?}");
        assert_eq!(
            uncertain_reason(&report),
            UncertainReason::UnexpectedReceipt
        );
        assert!(
            report
                .gaps
                .iter()
                .any(|gap| gap.detail.contains("unobserved")),
            "{name}: {:?}",
            report.gaps
        );
        assert_eq!(fake.tell_count(), 1);
    }
}

/// The 2026-11-02 PC probe (c687079) observed all three forms; values here
/// are synthetic. Queue and Steer keep their requested mode on replay.
fn check_pc_probe(name: &str, mode: Mode, status: &str, waiting: Option<&str>, id: Option<&str>) {
    let fake = SendFake::with(
        &format!("m04_pc_{name}"),
        vec![tell_response("thr_syn_0001", mode.as_str(), receipt(name))],
    );
    let mut args = vec!["thr_syn_0001", "--op-id", "op-pc-probe"];
    if mode == Mode::Steer {
        args.push("--steer");
    }
    args.extend(["--", "Synthetic"]);
    for round in 0..2 {
        let (code, report) = fake.cli_json(&args, "");
        assert_eq!(code, 0, "{name}: {report}");
        assert_eq!(report["status"], status, "{name}");
        assert_eq!(report["requested_mode"], mode.as_str());
        assert_eq!(report["result"]["replayed"], round == 1);
        assert_eq!(validate_report(&report), Vec::<String>::new(), "{name}");
        if let Some(waiting) = waiting {
            assert_eq!(report["result"]["delivery"]["kind"], "queued");
            assert_eq!(report["result"]["delivery"]["waiting_on"], waiting);
            assert_eq!(
                report["result"]["delivery"]["queued_message_id"],
                id.unwrap()
            );
            assert!(
                report["summary"]
                    .as_str()
                    .unwrap()
                    .contains("requested · queued")
            );
        }
        assert_eq!(
            fake.tell_count(),
            1,
            "{name}: only one write, including replay"
        );
        assert!(
            fake.record("op-pc-probe")["text"].is_null(),
            "{name}: confirmed acceptance removes prompt"
        );
    }
}

#[test]
fn pc_probe_sent_queue() {
    check_pc_probe(
        "tell-sent-observed.json",
        Mode::Queue,
        "accepted",
        None,
        None,
    );
}

#[test]
fn pc_probe_sent_steer() {
    check_pc_probe(
        "tell-steer-sent-observed.json",
        Mode::Steer,
        "accepted",
        None,
        None,
    );
}

#[test]
fn pc_probe_busy_queue() {
    check_pc_probe(
        "tell-queued-observed.json",
        Mode::Queue,
        "queued",
        Some("thread_busy"),
        Some("qmsg_syn_0001"),
    );
}

#[test]
fn pc_probe_claude_queued_steer() {
    check_pc_probe(
        "tell-steer-queued-observed.json",
        Mode::Steer,
        "queued",
        Some("not_reported"),
        Some("00000000-0000-4000-8000-000000000001"),
    );
}

#[test]
fn pc_probe_cloud_queued_queue() {
    check_pc_probe(
        "tell-cloud-queue-observed.json",
        Mode::Queue,
        "queued",
        Some("not_reported"),
        Some("00000000-0000-4000-8000-000000000001"),
    );
}

/// Drift is tested through the public Messenger seam. Expected rejection
/// comes from the observed shape, not from the parser under test.
#[test]
fn pc_probe_shapes_reject_drift_at_every_depth() {
    use openwiz_core::messaging::send::{Messenger, TellReply};
    for (file, mode) in [
        ("tell-queued-observed.json", Mode::Queue),
        ("tell-steer-queued-observed.json", Mode::Steer),
        ("tell-cloud-queue-observed.json", Mode::Queue),
    ] {
        let good: Value =
            serde_json::from_slice(&std::fs::read(send_fixtures().join(file)).unwrap()).unwrap();
        let mut cases = Vec::<(String, String)>::new();
        for path in [
            "",
            "/queuedMessage",
            "/queuedMessage/content/0",
            "/queuedMessage/payload",
        ] {
            let object = good.pointer(path).unwrap().as_object().unwrap();
            for key in object.keys() {
                let mut bad = good.clone();
                bad.pointer_mut(path)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                cases.push((format!("missing {path}/{key}"), bad.to_string()));
                // Duplicate the original wire key; Value must not erase it.
                let original = good.pointer(path).unwrap().to_string();
                let duplicate = format!(
                    "{{{}:{},{}",
                    serde_json::to_string(key).unwrap(),
                    object[key],
                    &original[1..]
                );
                cases.push((
                    format!("duplicate {path}/{key}"),
                    good.to_string().replacen(&original, &duplicate, 1),
                ));
            }
            let mut extra = good.clone();
            extra.pointer_mut(path).unwrap()["syntheticExtra"] = json!(true);
            cases.push((format!("extra {path}"), extra.to_string()));
            let mut tuple = good.clone();
            *tuple.pointer_mut(path).unwrap() = Value::Array(object.values().cloned().collect());
            cases.push((format!("array {path}"), tuple.to_string()));
        }
        for (path, bad_values) in [
            (
                "/threadId",
                vec![json!("thr_syn_0002"), json!(null), json!(5)],
            ),
            (
                "/ok",
                vec![json!(false), json!("true"), json!(1), json!(null)],
            ),
            (
                "/mode",
                vec![json!("auto"), json!({"steer":null}), json!(null)],
            ),
            (
                "/delivery",
                vec![
                    json!("sent"),
                    json!("unsupported"),
                    json!({"queued":null}),
                    json!(null),
                ],
            ),
            (
                "/queuedMessage/threadId",
                vec![json!("thr_syn_0002"), json!(null), json!(9)],
            ),
            (
                "/queuedMessage/id",
                vec![
                    json!(""),
                    json!("qmsg_"),
                    json!("syn-queued-0001"),
                    json!("/synthetic/marker"),
                    json!(null),
                    json!(7),
                ],
            ),
            (
                "/queuedMessage/initiator",
                vec![json!("system"), json!(null), json!(true)],
            ),
            (
                "/queuedMessage/senderThreadId",
                vec![
                    json!(42),
                    json!({"id":"thr_syn_0003"}),
                    json!("/synthetic/marker"),
                    json!("not_a_thread"),
                ],
            ),
            (
                "/queuedMessage/content",
                vec![json!([]), json!(null), json!({"type":"Synthetic"})],
            ),
            (
                "/queuedMessage/content/0/type",
                vec![json!(null), json!({"text":null})],
            ),
            (
                "/queuedMessage/content/0/text",
                vec![json!(null), json!([])],
            ),
            (
                "/queuedMessage/content/0/mentions",
                vec![json!(null), json!({}), json!(["Synthetic mention"])],
            ),
            ("/queuedMessage/model", vec![json!(null), json!(17)]),
            (
                "/queuedMessage/reasoningLevel",
                vec![json!(null), json!(false)],
            ),
            (
                "/queuedMessage/permissionMode",
                vec![json!(null), json!({})],
            ),
            ("/queuedMessage/serviceTier", vec![json!(null), json!([])]),
            (
                "/queuedMessage/editable",
                vec![json!(false), json!(null), json!("true")],
            ),
            (
                "/queuedMessage/sendAt",
                vec![json!(1700000600000_i64), json!("Synthetic time")],
            ),
            (
                "/queuedMessage/failureReason",
                vec![json!("Synthetic failure"), json!({})],
            ),
            (
                "/queuedMessage/payload/kind",
                vec![json!("file"), json!(null), json!({"inline":null})],
            ),
            (
                "/queuedMessage/createdAt",
                vec![json!(null), json!("Synthetic time"), json!(1.5)],
            ),
            (
                "/queuedMessage/updatedAt",
                vec![json!(null), json!(false), json!(1.5)],
            ),
        ] {
            for (index, value) in bad_values.into_iter().enumerate() {
                let mut bad = good.clone();
                *bad.pointer_mut(path).unwrap() = value;
                cases.push((format!("bad {path} {index}"), bad.to_string()));
            }
        }
        let mut two_blocks = good.clone();
        let block = two_blocks["queuedMessage"]["content"][0].clone();
        two_blocks["queuedMessage"]["content"] = json!([block, block]);
        cases.push(("unobserved content count".into(), two_blocks.to_string()));
        if good["queuedMessage"].get("hardQueue").is_some() {
            for value in [json!(true), json!(null), json!("false")] {
                let mut bad = good.clone();
                bad["queuedMessage"]["hardQueue"] = value;
                cases.push(("bad hardQueue".into(), bad.to_string()));
            }
            for value in [
                json!(null),
                json!("thread-busy"),
                json!({"kind":"plugin"}),
                json!({"kind":"thread-busy","syntheticExtra":true}),
                json!(["thread-busy"]),
            ] {
                let mut bad = good.clone();
                bad["queuedMessage"]["waitingOn"] = value;
                cases.push(("bad waitingOn".into(), bad.to_string()));
            }
            let wire = good.to_string().replace(
                r#"{"kind":"thread-busy"}"#,
                r#"{"kind":"thread-busy","kind":"thread-busy"}"#,
            );
            cases.push(("duplicate waitingOn.kind".into(), wire));
        } else {
            for (key, value) in [
                ("hardQueue", json!(false)),
                ("waitingOn", json!({"kind":"thread-busy"})),
                ("initiator", json!("agent")),
                ("senderThreadId", json!("thr_syn_0003")),
            ] {
                let mut bad = good.clone();
                bad["queuedMessage"][key] = value;
                cases.push((format!("unobserved Cloud queued {key}"), bad.to_string()));
            }
        }
        for (index, (why, stdout)) in cases.into_iter().enumerate() {
            let fake = SendFake::with(
                &format!("m04_pc_drift_{file}_{index}"),
                vec![tell_response(
                    "thr_syn_0001",
                    mode.as_str(),
                    json!({"stdout":stdout}),
                )],
            );
            let reply = fake.source().tell(
                &sid("thr_syn_0001"),
                mode,
                "Synthetic",
                Instant::now() + Duration::from_secs(3),
            );
            assert!(
                matches!(
                    reply,
                    TellReply::Unclear {
                        reason: UncertainReason::UnexpectedReceipt,
                        ..
                    }
                ),
                "{file} {why}: {reply:?}"
            );
            assert!(!format!("{reply:?}").contains("Synthetic queued prompt"));
            assert_eq!(fake.tell_count(), 1, "{file} {why}");
        }
    }
}

#[test]
fn pc_probe_missing_wait_reason_round_trips_without_inventing_one() {
    let value = json!({"kind":"queued", "waiting_on":"not_reported", "queued_message_id":"syn-queued-0001", "text":"queued: waiting reason not reported"});
    let parsed = serde_json::from_value::<Delivery>(value.clone());
    assert!(
        parsed.is_ok(),
        "the public model must represent the observed missing reason: {parsed:?}"
    );
    assert_eq!(serde_json::to_value(parsed.unwrap()).unwrap(), value);
    assert_eq!(validate_def("delivery", &value), Vec::<String>::new());
}

#[test]
fn pc_probe_busy_queue_allows_the_owner_noted_user_sender() {
    let mut value: Value = serde_json::from_slice(
        &std::fs::read(send_fixtures().join("tell-queued-observed.json")).unwrap(),
    )
    .unwrap();
    value["queuedMessage"]["initiator"] = json!("user");
    value["queuedMessage"]["senderThreadId"] = Value::Null;
    let fake = SendFake::with(
        "m04_pc_user_queue",
        vec![tell_response(
            "thr_syn_0001",
            "queue",
            json!({"stdout":value.to_string()}),
        )],
    );
    let report = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        "Synthetic",
        "op-pc-user",
    ));
    assert_eq!(report.status, Status::Queued);
    let report = serde_json::to_value(report).unwrap();
    assert_eq!(report["result"]["delivery"]["waiting_on"], "thread_busy");
    assert_eq!(validate_report(&report), Vec::<String>::new());
    assert_eq!(fake.tell_count(), 1);
}
