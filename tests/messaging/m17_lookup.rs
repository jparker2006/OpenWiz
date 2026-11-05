//! M17 Lookup: one op id's send record, for "Check again".
//! A known op id gives its recorded report, an in-progress one says so, an
//! unknown one says "no record", and one bound to another target says that.
//! **No `tell`, ever, and no claim.**
//!
//! The CLI form is `openwiz send <id> --op-id <op> --check [--json]`: it
//! takes no text and makes no `room-cli` call at all.

use openwiz_cli::EXIT_USAGE;
use openwiz_cli::send::{
    EXIT_CONFLICTING_REUSE, EXIT_DELIVERED, EXIT_IN_PROGRESS, EXIT_NO_RECORD, EXIT_SEND_FAILED,
    EXIT_UNCERTAIN,
};
use openwiz_core::messaging::Mode;
use openwiz_core::messaging::lookup::{LOOKUP_SCHEMA_ID, LookupReport, LookupResult, lookup};
use openwiz_core::messaging::record::{RecordState, SendStore};
use openwiz_core::messaging::send::{Delivery, FailReason, SendResult, Status, UncertainReason};
use openwiz_store::FileStore;
use serde_json::{Value, json};

use crate::support::*;

const TEXT: &str = "Synthetic lookup";

/// The core's lookup through the read-only opener, as `--check` uses it.
fn look(fake: &SendFake, target: &str, op_id: &str) -> LookupReport {
    let store = FileStore::open_read_only(&fake.state).expect("open read-only");
    let store = store.as_ref().map(|s| s as &dyn SendStore);
    let report = lookup(store, &clock(), &sid(target), &op(op_id))
        .unwrap_or_else(|e| panic!("lookup failed: {e}"));
    assert_eq!(report.schema, LOOKUP_SCHEMA_ID);
    assert_eq!(report.op_id.as_str(), op_id);
    assert_eq!(report.target.0, target);
    assert!(report.summary.contains("no call was made"), "{report:#?}");
    report
}

#[test]
fn an_unknown_op_id_has_no_record_and_nothing_is_made() {
    let fake = SendFake::new("m17_unknown", "send");
    let report = look(&fake, "thr_syn_0001", "op-m17-unknown");
    assert_eq!(report.result, LookupResult::NoRecord);
    assert!(report.summary.contains("no record"), "{report:#?}");
    assert!(sends_listing(&fake).is_empty(), "a lookup made a file");
    assert!(fake.invocations().is_empty(), "a lookup read room-cli");
}

#[test]
fn a_known_op_id_gives_its_recorded_report_with_no_call() {
    let fake = SendFake::new("m17_known", "send");
    let sent = fake.report(&request("thr_syn_0001", Mode::Queue, TEXT, "op-m17-known"));
    assert_eq!(sent.status, Status::Accepted);
    let calls = fake.invocations().len();
    let files = sends_listing(&fake);

    let report = look(&fake, "thr_syn_0001", "op-m17-known");
    let LookupResult::Recorded { report: recorded } = report.result else {
        panic!("{report:#?}")
    };
    // The same report, marked as coming from the record.
    let mut expected = sent.clone();
    expected.result = SendResult::Outcome {
        delivery: Delivery::accepted(Mode::Queue),
        replayed: true,
        recovered: false,
    };
    assert_eq!(*recorded, expected);
    assert_eq!(fake.invocations().len(), calls, "a lookup read room-cli");
    assert_eq!(fake.tell_count(), 1);
    assert_eq!(
        sends_listing(&fake),
        files,
        "a lookup changed the send record"
    );
}

#[test]
fn an_in_progress_op_id_says_so_and_stays_its_owners() {
    let fake = SendFake::new("m17_in_progress", "send");
    let mut held = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m17-held");
    for state in [RecordState::Claimed, RecordState::Dispatching] {
        if state == RecordState::Dispatching {
            held.dispatching();
        }
        let report = look(&fake, "thr_syn_0001", "op-m17-held");
        let LookupResult::Recorded { report: recorded } = &report.result else {
            panic!("{report:#?}")
        };
        assert_eq!(recorded.status, Status::InProgress, "{report:#?}");
        assert_eq!(
            recorded.result,
            SendResult::InProgress { state: Some(state) }
        );
        assert_eq!(recorded.claimed_at, Some(T0));
        assert!(recorded.summary.contains("no call was made"), "{report:#?}");
    }
    // The owner still holds its claim, and finishes as if nothing happened.
    held.finish(Delivery::accepted(Mode::Queue));
    let report = look(&fake, "thr_syn_0001", "op-m17-held");
    let LookupResult::Recorded { report: recorded } = report.result else {
        panic!("{report:#?}")
    };
    assert_eq!(recorded.status, Status::Accepted);
    assert!(fake.invocations().is_empty());
}

#[test]
fn a_stopped_senders_record_is_recovered_never_resent() {
    let fake = SendFake::new("m17_recovered", "send");
    for (op_id, dispatching, status) in [
        ("op-m17-stopped-claimed", false, Status::Failed),
        ("op-m17-stopped-dispatching", true, Status::Uncertain),
    ] {
        let mut held = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, op_id);
        if dispatching {
            held.dispatching();
        }
        held.stop();
        let report = look(&fake, "thr_syn_0001", op_id);
        let LookupResult::Recorded { report: recorded } = report.result else {
            panic!("{report:#?}")
        };
        assert_eq!(recorded.status, status, "{op_id}");
        let SendResult::Outcome {
            delivery,
            recovered,
            ..
        } = &recorded.result
        else {
            panic!("{recorded:#?}")
        };
        assert!(recovered, "{op_id}");
        match delivery {
            Delivery::Failed { reason, .. } => {
                assert_eq!(*reason, FailReason::InterruptedBeforeDispatch);
            }
            Delivery::Uncertain { reason, .. } => {
                assert_eq!(*reason, UncertainReason::InterruptedAfterDispatchMark);
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(fake.invocations().is_empty());
    assert_eq!(fake.tell_count(), 0);
}

#[test]
fn an_op_id_bound_to_another_target_is_not_this_targets() {
    let fake = SendFake::new("m17_other", "send");
    fake.report(&request("thr_syn_0001", Mode::Queue, TEXT, "op-m17-other"));
    let calls = fake.invocations().len();
    let report = look(&fake, "thr_syn_0006", "op-m17-other");
    assert_eq!(report.result, LookupResult::OtherTarget, "{report:#?}");
    assert!(report.summary.contains("another target"), "{report:#?}");
    assert_eq!(fake.invocations().len(), calls);
}

/// (target, op id, exit code, result kind, report status).
type Case<'a> = (&'a str, &'a str, i32, &'a str, Option<&'a str>);

#[test]
fn the_cli_check_matches_the_schema_and_the_text_and_reads_nothing() {
    let fake = SendFake::with(
        "m17_cli",
        vec![tell_response(
            "thr_syn_0006",
            "queue",
            json!({"abort": true}),
        )],
    );
    fake.report(&request("thr_syn_0001", Mode::Queue, TEXT, "op-m17-sent"));
    fake.report(&request(
        "thr_syn_0006",
        Mode::Queue,
        TEXT,
        "op-m17-uncertain",
    ));
    fake.report(&request("thr_syn_0008", Mode::Queue, TEXT, "op-m17-failed"));
    let held = Held::claim(&fake, "thr_syn_0002", Mode::Queue, TEXT, "op-m17-flight");
    let calls = fake.invocations().len();
    let tells = fake.tell_count();
    let cases: Vec<Case> = vec![
        (
            "thr_syn_0001",
            "op-m17-sent",
            EXIT_DELIVERED,
            "recorded",
            Some("accepted"),
        ),
        (
            "thr_syn_0006",
            "op-m17-uncertain",
            EXIT_UNCERTAIN,
            "recorded",
            Some("uncertain"),
        ),
        (
            "thr_syn_0008",
            "op-m17-failed",
            EXIT_SEND_FAILED,
            "recorded",
            Some("failed"),
        ),
        (
            "thr_syn_0002",
            "op-m17-flight",
            EXIT_IN_PROGRESS,
            "recorded",
            Some("in_progress"),
        ),
        (
            "thr_syn_0001",
            "op-m17-none",
            EXIT_NO_RECORD,
            "no_record",
            None,
        ),
        (
            "thr_syn_0006",
            "op-m17-sent",
            EXIT_CONFLICTING_REUSE,
            "other_target",
            None,
        ),
    ];
    for (target, op_id, exit, kind, status) in cases {
        let args = [target, "--op-id", op_id, "--check"];
        let (code, json) = fake.cli_json(&args, "");
        let errors = validate_def("lookup", &json);
        assert!(errors.is_empty(), "{op_id}: {errors:#?}\n{json:#}");
        assert_eq!(code, exit, "{op_id}: {json:#}");
        assert_eq!(json["result"]["kind"], kind, "{op_id}");
        if let Some(status) = status {
            assert_eq!(json["result"]["report"]["status"], status, "{op_id}");
        }
        let (text_code, text, err) = fake.cli(&args, "");
        assert_eq!(text_code, code, "{op_id}: {err}");
        for wanted in wanted_lookup(&json) {
            assert!(text.contains(&wanted), "{op_id}: {wanted:?} not in\n{text}");
        }
    }
    assert_eq!(fake.invocations().len(), calls, "--check read room-cli");
    assert_eq!(fake.tell_count(), tells);
    held.stop();
}

/// What the text must contain for a JSON lookup.
fn wanted_lookup(json: &Value) -> Vec<String> {
    let mut wanted = vec![
        format!("op {}", json["op_id"].as_str().unwrap()),
        json["target"].as_str().unwrap().to_owned(),
        json["summary"].as_str().unwrap().to_owned(),
    ];
    let report = &json["result"]["report"];
    if !report.is_null() {
        wanted.push(report["summary"].as_str().unwrap().to_owned());
    }
    wanted
}

#[test]
fn a_check_needs_an_op_id_and_takes_no_text_or_send_option() {
    let fake = SendFake::new("m17_usage", "send");
    for args in [
        vec!["thr_syn_0001", "--check"],
        vec![
            "thr_syn_0001",
            "--op-id",
            "op-m17-u",
            "--check",
            "--",
            "Synthetic",
        ],
        vec!["thr_syn_0001", "--op-id", "op-m17-u", "--check", "--steer"],
        vec![
            "thr_syn_0001",
            "--op-id",
            "op-m17-u",
            "--check",
            "--resume-paused-queue",
        ],
        vec![
            "thr_syn_0001",
            "--op-id",
            "op-m17-u",
            "--check",
            "--accept-possible-duplicate",
        ],
        vec!["thr_syn_0001", "--op-id", "op-m17-u", "--check=yes"],
        vec!["thr_syn_0001", "--op-id", "../escape", "--check"],
    ] {
        let (code, out, err) = fake.cli(&args, "Synthetic from stdin");
        assert_eq!(code, EXIT_USAGE, "{args:?}: {out}{err}");
        assert!(out.is_empty(), "{out}");
        assert!(err.starts_with("openwiz: "), "{err}");
    }
    assert!(fake.invocations().is_empty());
    assert!(sends_listing(&fake).is_empty());
}

#[test]
fn the_release_build_checks_with_no_call() {
    let fake = SendFake::new("m17_release", "send");
    let sent = fake.run_process(
        &[
            "send",
            "thr_syn_0001",
            "--op-id",
            "op-m17-rel",
            "--json",
            "--",
            TEXT,
        ],
        "",
    );
    assert_eq!(sent.status.code(), Some(0));
    let calls = fake.invocations().len();
    let out = fake.run_process(
        &[
            "send",
            "thr_syn_0001",
            "--op-id",
            "op-m17-rel",
            "--check",
            "--json",
        ],
        "",
    );
    let (code, json) = json_of(&out);
    assert_eq!(code, Some(0), "{json}");
    assert!(validate_def("lookup", &json).is_empty(), "{json:#}");
    assert_eq!(json["result"]["report"]["result"]["replayed"], true);
    assert_eq!(fake.invocations().len(), calls, "--check ran room-cli");
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn a_check_with_no_send_record_creates_nothing() {
    let fake = SendFake::new("m17_no_store", "send");
    assert!(!fake.state.exists());
    assert!(FileStore::open_read_only(&fake.state).unwrap().is_none());
    let report = look(&fake, "thr_syn_0001", "op-m17-nothing");
    assert_eq!(report.result, LookupResult::NoRecord);
    for json in [false, true] {
        let mut args = vec!["thr_syn_0001", "--op-id", "op-m17-nothing", "--check"];
        if json {
            args.push("--json");
        }
        let (code, out, err) = fake.cli(&args, "");
        assert_eq!(code, EXIT_NO_RECORD, "{out}{err}");
        assert!(!fake.state.exists(), "--check created the state directory");
    }
    // An existing state directory without sends/ is not given one either.
    crate::dirs::private_dir(&fake.state).unwrap();
    std::fs::set_permissions(
        &fake.state,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let (code, _, err) = fake.cli(&["thr_syn_0001", "--op-id", "op-m17-x", "--check"], "");
    assert_eq!(code, EXIT_NO_RECORD, "{err}");
    assert!(!fake.state.join("sends").exists(), "--check created sends/");
    assert!(fake.invocations().is_empty());
}

#[test]
fn no_lookup_path_changes_the_send_record() {
    let fake = SendFake::new("m17_read_only", "send");
    fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        TEXT,
        "op-m17-ro-sent",
    ));
    Held::claim(
        &fake,
        "thr_syn_0001",
        Mode::Queue,
        TEXT,
        "op-m17-ro-claimed",
    )
    .stop();
    let mut marked = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m17-ro-marked");
    marked.dispatching();
    marked.stop();
    let live = Held::claim(&fake, "thr_syn_0002", Mode::Queue, TEXT, "op-m17-ro-live");
    let before = sends_listing(&fake);
    let calls = fake.invocations().len();
    for (target, op_id, status) in [
        ("thr_syn_0001", "op-m17-ro-sent", Some(Status::Accepted)),
        ("thr_syn_0001", "op-m17-ro-claimed", Some(Status::Failed)),
        ("thr_syn_0001", "op-m17-ro-marked", Some(Status::Uncertain)),
        ("thr_syn_0002", "op-m17-ro-live", Some(Status::InProgress)),
        ("thr_syn_0001", "op-m17-ro-unknown", None),
        ("thr_syn_0006", "op-m17-ro-sent", None),
    ] {
        let report = look(&fake, target, op_id);
        match (&report.result, status) {
            (LookupResult::Recorded { report }, Some(status)) => {
                assert_eq!(report.status, status, "{op_id}");
            }
            (LookupResult::NoRecord | LookupResult::OtherTarget, None) => {}
            other => panic!("{op_id}: {other:?}"),
        }
        assert_eq!(
            sends_listing(&fake),
            before,
            "the lookup of {op_id} changed the record"
        );
        for json in [false, true] {
            let mut args = vec![target, "--op-id", op_id, "--check"];
            if json {
                args.push("--json");
            }
            let _ = fake.cli(&args, "");
            assert_eq!(
                sends_listing(&fake),
                before,
                "--check {op_id} changed the record"
            );
        }
    }
    assert_eq!(fake.record("op-m17-ro-claimed")["state"], "claimed");
    assert_eq!(fake.record("op-m17-ro-marked")["state"], "dispatching");
    assert_eq!(fake.invocations().len(), calls);
    // The live owner still finishes its own send.
    live.finish(Delivery::accepted(Mode::Queue));
}

/// A store opened for reading refuses every write.
#[test]
fn a_read_only_store_refuses_every_write() {
    let fake = SendFake::new("m17_refuses", "send");
    let _ = fake.store();
    let store = FileStore::open_read_only(&fake.state).unwrap().unwrap();
    let record = openwiz_core::messaging::record::SendRecord::claimed(
        op("op-m17-w"),
        openwiz_core::messaging::Binding::new(sid("thr_syn_0001"), Mode::Queue, TEXT),
        TEXT,
        T0,
    );
    assert!(store.claim(&record, T0).is_err());
    assert!(store.update(&record).is_err());
    assert!(store.withdraw(&record.op_id).is_err());
    assert!(
        store
            .uncertain_matching(&record.binding, &op("op-m17-x"), T0)
            .is_err()
    );
    assert!(
        store
            .in_flight_matching(&record.binding, &op("op-m17-x"), T0)
            .is_err()
    );
    assert!(sends_listing(&fake).is_empty());
}
