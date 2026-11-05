//! M18 The in-flight guard: a new op id whose target, mode
//! and text match a send still claimed or dispatching is refused as a
//! possible duplicate, with **no call**, unless the owner accepts a
//! possible duplicate. Two tabs, or the CLI and the web, then can't send the
//! same text twice under two op ids while the first is still running.
//!
//! The in-flight guard runs before the claim (nothing is written) and again after it,
//! so a sender that slips in between is still caught; the refused op id's
//! claim is withdrawn, so it stays unused either way.

use std::process::Child;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use openwiz_cli::send::EXIT_POSSIBLE_DUPLICATE;
use openwiz_core::messaging::record::{Claim, SendRecord, SendStore, StoreError};
use openwiz_core::messaging::send::{self, Delivery, SendResult, Status};
use openwiz_core::messaging::{Binding, Mode, OpId};
use openwiz_core::model::Timestamp;
use openwiz_store::FileStore;
use serde_json::json;

use crate::support::*;

const TEXT: &str = "Synthetic in flight";

fn refused_by(report: &send::SendReport) -> Vec<(String, bool)> {
    match &report.result {
        SendResult::PossibleDuplicateRefused { earlier } => earlier
            .iter()
            .map(|e| (e.op_id.as_str().to_owned(), e.in_flight))
            .collect(),
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn a_new_op_id_is_refused_while_the_same_send_is_claimed_or_dispatching() {
    let fake = SendFake::new("m18_refused", "send");
    let mut held = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m18-first");
    for dispatching in [false, true] {
        if dispatching {
            held.dispatching();
        }
        let op_id = format!("op-m18-second-{dispatching}");
        let report = fake.report(&request("thr_syn_0001", Mode::Queue, TEXT, &op_id));
        assert_eq!(
            report.status,
            Status::PossibleDuplicateRefused,
            "{report:#?}"
        );
        assert_eq!(refused_by(&report), vec![("op-m18-first".to_owned(), true)]);
        assert!(report.summary.contains("still in progress"), "{report:#?}");
        assert!(report.summary.contains("op-m18-first"), "{report:#?}");
        assert!(report.summary.contains("nothing was sent"), "{report:#?}");
        assert!(report.effects.is_none());
        assert!(
            !fake.record_path(&op_id).exists(),
            "a refused op id stays unused"
        );
        let errors = validate_report(&serde_json::to_value(&report).unwrap());
        assert!(errors.is_empty(), "{errors:#?}");
    }
    assert_eq!(fake.tell_count(), 0, "no call while the first is in flight");
    assert!(fake.invocations().is_empty(), "nothing was read either");
    held.finish(Delivery::accepted(Mode::Queue));
}

#[test]
fn accepting_a_possible_duplicate_sends_anyway() {
    let fake = SendFake::new("m18_accepted", "send");
    let held = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m18-first");
    let mut r = request("thr_syn_0001", Mode::Queue, TEXT, "op-m18-accepted");
    r.accept_possible_duplicate = true;
    let report = fake.report(&r);
    assert_eq!(report.status, Status::Accepted, "{report:#?}");
    assert_eq!(fake.tell_count(), 1);
    held.stop();
}

#[test]
fn only_the_same_target_mode_and_text_is_in_flight() {
    let fake = SendFake::new("m18_same_binding", "send");
    let held = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m18-first");
    for (i, (target, mode, text)) in [
        ("thr_syn_0001", Mode::Queue, "Synthetic other text"),
        ("thr_syn_0001", Mode::Steer, TEXT),
        ("thr_syn_0006", Mode::Queue, TEXT),
    ]
    .into_iter()
    .enumerate()
    {
        let report = fake.report(&request(target, mode, text, &format!("op-m18-other-{i}")));
        assert!(
            !matches!(report.status, Status::PossibleDuplicateRefused),
            "{target} {mode:?} {text}: {report:#?}"
        );
    }
    assert_eq!(fake.tell_count(), 3);
    held.stop();
}

#[test]
fn a_finished_or_stopped_send_is_not_in_flight() {
    let fake = SendFake::new("m18_not_in_flight", "send");
    // Finished: a new op id is a new, explicit send.
    Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m18-done")
        .finish(Delivery::accepted(Mode::Queue));
    let report = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        TEXT,
        "op-m18-after-done",
    ));
    assert_eq!(report.status, Status::Accepted, "{report:#?}");
    // Stopped before its dispatching mark: recovered as failed, nothing was
    // sent, so nothing can be duplicated.
    Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m18-stopped").stop();
    let report = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        TEXT,
        "op-m18-after-stop",
    ));
    assert_eq!(report.status, Status::Accepted, "{report:#?}");
    assert_eq!(fake.record("op-m18-stopped")["delivery"]["kind"], "failed");
    // Stopped after its mark: recovered as uncertain, so the uncertain
    // guard refuses, as before.
    let mut held = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m18-marked");
    held.dispatching();
    held.stop();
    let report = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        TEXT,
        "op-m18-after-mark",
    ));
    assert_eq!(
        refused_by(&report),
        vec![("op-m18-marked".to_owned(), false)]
    );
    assert_eq!(fake.tell_count(), 2);
}

#[test]
fn the_same_op_id_again_is_a_replay_not_a_duplicate() {
    let fake = SendFake::new("m18_same_op", "send");
    let held = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m18-same");
    let report = fake.report(&request("thr_syn_0001", Mode::Queue, TEXT, "op-m18-same"));
    assert_eq!(report.status, Status::InProgress, "{report:#?}");
    assert_eq!(fake.tell_count(), 0);
    held.stop();
}

/// A store that lets another sender claim the same send just before this
/// one's claim: after validation before the claim, as a racer in
/// another process could.
struct SlipIn<'a> {
    inner: FileStore,
    fake: &'a SendFake,
    racer: Mutex<Option<Held>>,
}

impl SendStore for SlipIn<'_> {
    fn load(&self, op_id: &OpId) -> Result<Option<SendRecord>, StoreError> {
        self.inner.load(op_id)
    }

    fn claim(&self, record: &SendRecord, now: Timestamp) -> Result<Claim, StoreError> {
        let mut racer = self.racer.lock().unwrap();
        if racer.is_none() {
            *racer = Some(Held::claim(
                self.fake,
                "thr_syn_0001",
                Mode::Queue,
                TEXT,
                "op-m18-racer",
            ));
        }
        self.inner.claim(record, now)
    }

    fn update(&self, record: &SendRecord) -> Result<(), StoreError> {
        self.inner.update(record)
    }

    fn release(&self, op_id: &OpId) {
        self.inner.release(op_id);
    }

    fn uncertain_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError> {
        self.inner.uncertain_matching(binding, except, now)
    }

    fn in_flight_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError> {
        self.inner.in_flight_matching(binding, except, now)
    }

    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<SendRecord>, StoreError> {
        self.inner.inspect(op_id, now)
    }

    fn withdraw(&self, op_id: &OpId) -> Result<(), StoreError> {
        self.inner.withdraw(op_id)
    }
}

#[test]
fn a_sender_that_slips_in_before_the_claim_is_still_caught() {
    let fake = SendFake::new("m18_slip_in", "send");
    let store = SlipIn {
        inner: fake.store(),
        fake: &fake,
        racer: Mutex::new(None),
    };
    let r = request("thr_syn_0001", Mode::Queue, TEXT, "op-m18-late");
    let report = send::send(&fake.source(), &store, &clock(), &r).unwrap();
    assert_eq!(
        report.status,
        Status::PossibleDuplicateRefused,
        "{report:#?}"
    );
    assert_eq!(refused_by(&report), vec![("op-m18-racer".to_owned(), true)]);
    assert_eq!(fake.tell_count(), 0);
    // The refused op id's claim was withdrawn: no record, no claim held.
    assert!(!fake.record_path("op-m18-late").exists());
    let racer = store.racer.lock().unwrap().take().unwrap();
    racer.finish(Delivery::accepted(Mode::Queue));
    // So the same op id, once the racer has finished, is a fresh send.
    let report = fake.report(&r);
    assert_eq!(report.status, Status::Accepted, "{report:#?}");
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn the_cli_refuses_with_exit_9_and_names_the_send_in_flight() {
    let fake = SendFake::new("m18_cli", "send");
    let held = Held::claim(&fake, "thr_syn_0001", Mode::Queue, TEXT, "op-m18-cli-first");
    let (code, json) = fake.cli_json(&["thr_syn_0001", "--", TEXT], "");
    assert_eq!(code, EXIT_POSSIBLE_DUPLICATE, "{json:#}");
    assert!(validate_report(&json).is_empty(), "{json:#}");
    assert_eq!(json["result"]["earlier"][0]["op_id"], "op-m18-cli-first");
    assert_eq!(json["result"]["earlier"][0]["in_flight"], true);
    let (code, text, _) = fake.cli(&["thr_syn_0001", "--", TEXT], "");
    assert_eq!(code, EXIT_POSSIBLE_DUPLICATE);
    assert!(text.contains("op-m18-cli-first"), "{text}");
    assert!(text.contains("still in progress"), "{text}");
    assert!(text.contains("--accept-possible-duplicate"), "{text}");
    assert_eq!(fake.tell_count(), 0);
    held.stop();
}

/// Waits until `op_id`'s sender is in its call: its record is dispatching
/// and the fake has counted the call (the mark comes just before it).
fn wait_in_call(fake: &SendFake, op_id: &str) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        let marked = std::fs::read_to_string(fake.record_path(op_id))
            .is_ok_and(|text| text.contains("\"dispatching\""));
        if marked && fake.tell_count() == 1 {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("{op_id} never reached its call");
}

#[test]
fn across_processes_a_second_op_id_waits_for_nothing_and_sends_nothing() {
    let fake = SendFake::with(
        "m18_processes",
        vec![tell_response(
            "thr_syn_0001",
            "queue",
            json!({"delay_ms": 3000, "stdout_file": send_fixtures().join("tell-sent.json").to_str().unwrap()}),
        )],
    );
    openwiz_release();
    let send = |op_id: &str, extra: &[&str]| -> Child {
        let mut args = vec!["send", "thr_syn_0001", "--op-id", op_id, "--json"];
        args.extend_from_slice(extra);
        args.extend_from_slice(&["--", TEXT]);
        fake.process(&args, "").spawn().expect("start openwiz")
    };
    let first = send("op-m18-p1", &[]);
    wait_in_call(&fake, "op-m18-p1");
    // The first is in its call: a second op id is refused at once.
    let second = send("op-m18-p2", &[]).wait_with_output().unwrap();
    let (code, json) = json_of(&second);
    assert_eq!(code, Some(EXIT_POSSIBLE_DUPLICATE), "{json:#}");
    assert_eq!(json["result"]["earlier"][0]["op_id"], "op-m18-p1");
    assert_eq!(fake.tell_count(), 1, "only the first is calling");
    // With the acknowledgement, a third goes ahead while the first is still
    // in its call.
    let third = send("op-m18-p3", &["--accept-possible-duplicate"]);
    let first = first.wait_with_output().unwrap();
    let third = third.wait_with_output().unwrap();
    assert_eq!(fake.tell_count(), 2, "the first and the acknowledged third");
    assert_eq!(json_of(&first).0, Some(0));
    assert_eq!(json_of(&third).0, Some(0));
    assert!(!fake.record_path("op-m18-p2").exists());
}

#[test]
fn many_new_op_ids_racing_on_one_send_make_at_most_one_tell() {
    let fake = SendFake::with(
        "m18_race",
        vec![tell_response(
            "thr_syn_0001",
            "queue",
            json!({"delay_ms": 1500, "stdout_file": send_fixtures().join("tell-sent.json").to_str().unwrap()}),
        )],
    );
    openwiz_release();
    let racers: Vec<Child> = (0..8)
        .map(|i| {
            let op_id = format!("op-m18-race-{i}");
            fake.process(
                &[
                    "send",
                    "thr_syn_0001",
                    "--op-id",
                    &op_id,
                    "--json",
                    "--",
                    TEXT,
                ],
                "",
            )
            .spawn()
            .expect("start a racer")
        })
        .collect();
    let codes: Vec<Option<i32>> = racers
        .into_iter()
        .map(|c| c.wait_with_output().unwrap().status.code())
        .collect();
    assert!(
        fake.tell_count() <= 1,
        "{} calls: {codes:?}",
        fake.tell_count()
    );
    for code in &codes {
        assert!(
            *code == Some(0) || *code == Some(EXIT_POSSIBLE_DUPLICATE),
            "{codes:?}"
        );
    }
}

/// A scanner whose hook makes the owner of `op_id` withdraw its claim just
/// after the scanner read the record and just before it tries the lock.
fn scanner_racing_a_withdrawal(fake: &SendFake, op_id: &'static str) -> FileStore {
    let owner = Arc::new(Mutex::new(Some(Held::claim(
        fake,
        "thr_syn_0001",
        Mode::Queue,
        TEXT,
        op_id,
    ))));
    fake.store().with_before_lock(move |seen| {
        if seen.as_str() != op_id {
            return;
        }
        if let Some(held) = owner.lock().unwrap().take() {
            held.withdraw();
        }
    })
}

#[test]
fn a_scan_never_resurrects_a_withdrawn_claim() {
    let fake = SendFake::new("m18_withdrawn", "send");
    let binding = Binding::new(sid("thr_syn_0001"), Mode::Queue, TEXT);
    let other = op("op-m18-scanner");

    let scanner = scanner_racing_a_withdrawal(&fake, "op-m18-w1");
    let found = scanner.in_flight_matching(&binding, &other, T0).unwrap();
    assert!(found.is_empty(), "{found:#?}");
    assert!(
        !fake.record_path("op-m18-w1").exists(),
        "in_flight_matching resurrected it"
    );

    let scanner = scanner_racing_a_withdrawal(&fake, "op-m18-w2");
    let found = scanner.uncertain_matching(&binding, &other, T0).unwrap();
    assert!(found.is_empty(), "{found:#?}");
    assert!(
        !fake.record_path("op-m18-w2").exists(),
        "uncertain_matching resurrected it"
    );

    let scanner = scanner_racing_a_withdrawal(&fake, "op-m18-w3");
    assert_eq!(scanner.inspect(&op("op-m18-w3"), T0).unwrap(), None);
    assert!(
        !fake.record_path("op-m18-w3").exists(),
        "inspect resurrected it"
    );

    for op_id in ["op-m18-w1", "op-m18-w2", "op-m18-w3"] {
        let report = fake.report(&request("thr_syn_0001", Mode::Queue, TEXT, op_id));
        assert_eq!(report.status, Status::Accepted, "{op_id}: {report:#?}");
        let SendResult::Outcome { replayed, .. } = report.result else {
            panic!("{op_id}")
        };
        assert!(!replayed, "{op_id} replayed a resurrected record");
    }
    assert_eq!(fake.tell_count(), 3);
}

/// A record withdrawn and claimed again under another binding between the
/// read and the lock is checked again: the scan matches what is there now.
#[test]
fn a_record_reread_under_the_lock_is_matched_again() {
    let fake = SendFake::new("m18_reread", "send");
    let binding = Binding::new(sid("thr_syn_0001"), Mode::Queue, TEXT);
    let owner = Arc::new(Mutex::new(Some(Held::claim(
        &fake,
        "thr_syn_0001",
        Mode::Queue,
        TEXT,
        "op-m18-rebound",
    ))));
    let state = fake.state.clone();
    let scanner = fake.store().with_before_lock(move |seen| {
        if seen.as_str() != "op-m18-rebound" {
            return;
        }
        if let Some(held) = owner.lock().unwrap().take() {
            held.withdraw();
            // Claimed again for another text, by a sender that stops after
            // its dispatching mark: it recovers as uncertain.
            let store = FileStore::open(&state).unwrap();
            let record = SendRecord::claimed(
                op("op-m18-rebound"),
                Binding::new(sid("thr_syn_0001"), Mode::Queue, "Synthetic other text"),
                "Synthetic other text",
                T0,
            );
            assert_eq!(store.claim(&record, T0).unwrap(), Claim::Owned);
            store.update(&record.dispatching(T0)).unwrap();
            store.release(&record.op_id);
            wait_lock_free(&store, &record.op_id);
        }
    });
    let found = scanner
        .uncertain_matching(&binding, &op("op-m18-scanner"), T0)
        .unwrap();
    assert!(found.is_empty(), "another text matched: {found:#?}");
}
