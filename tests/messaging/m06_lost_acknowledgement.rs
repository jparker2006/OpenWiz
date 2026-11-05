//! M6 Lost acknowledgement: a `room-cli` that accepts and then times out
//! gives uncertain, keeps the text and makes no second `tell`. A fixture with
//! exactly one message matching on text and time still gives uncertain. A
//! new send of the same text to the same target is refused unless the owner
//! acknowledges a possible duplicate.

use std::fs;

use openwiz_core::messaging::Mode;
use openwiz_core::messaging::reconcile::{
    ATTRIBUTION_0_43_1, MATCH_WINDOW_MS, Observation, ObservationSource, Reconciliation, reconcile,
};
use openwiz_core::messaging::send::{Delivery, SendResult, Status, UncertainReason};
use openwiz_core::messaging::sha256_hex;
use openwiz_core::model::Timestamp;
use serde_json::{Value, json};

use crate::m04_outcome_mapping::{delivery, uncertain_reason};
use crate::support::*;

const TEXT: &str = "Synthetic message for the lost acknowledgement case.";

#[test]
fn accepted_then_timed_out_is_uncertain_keeps_the_text_and_never_resends() {
    let fake = SendFake::with(
        "m06_timeout",
        vec![tell_response(
            "thr_syn_0001",
            "queue",
            json!({"hang": true}),
        )],
    );
    let first = fake.report(&request("thr_syn_0001", Mode::Queue, TEXT, "op-m06-first"));
    assert_eq!(first.status, Status::Uncertain);
    assert_eq!(uncertain_reason(&first), UncertainReason::TimedOut);
    match delivery(&first) {
        Delivery::Uncertain {
            reconciliation,
            attribution,
            ..
        } => {
            assert_eq!(
                *reconciliation,
                Reconciliation::NotRead,
                "no reconciliation read exists"
            );
            assert_eq!(attribution, ATTRIBUTION_0_43_1);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fake.tell_count(), 1);
    let record = fake.record("op-m06-first");
    assert_eq!(record["text"], TEXT, "the text is kept");
    assert_eq!(record["delivery"]["kind"], "uncertain");

    // The same op id again: the recorded outcome, no call.
    let again = fake.report(&request("thr_syn_0001", Mode::Queue, TEXT, "op-m06-first"));
    assert!(
        matches!(again.result, SendResult::Outcome { replayed: true, .. }),
        "{again:#?}"
    );
    assert_eq!(again.status, Status::Uncertain);
    assert_eq!(fake.tell_count(), 1);

    // A new op id, same text and target: refused, the earlier op shown.
    let blind = fake.report(&request("thr_syn_0001", Mode::Steer, TEXT, "op-m06-blind"));
    assert_eq!(blind.status, Status::PossibleDuplicateRefused);
    match &blind.result {
        SendResult::PossibleDuplicateRefused { earlier } => {
            assert_eq!(earlier.len(), 1);
            assert_eq!(earlier[0].op_id.as_str(), "op-m06-first");
            assert_eq!(earlier[0].reconciliation, Reconciliation::NotRead);
        }
        other => panic!("{other:?}"),
    }
    assert!(
        !fake.record_path("op-m06-blind").exists(),
        "nothing claimed"
    );
    assert_eq!(fake.tell_count(), 1, "no blind resend");

    // Another text, or another target, is not a possible duplicate.
    let other = fake.report(&request("thr_syn_0006", Mode::Queue, TEXT, "op-m06-other"));
    assert_ne!(other.status, Status::PossibleDuplicateRefused);
    assert_eq!(fake.tell_count(), 2);

    // Acknowledged: a new, explicit send.
    let mut acknowledged = request("thr_syn_0001", Mode::Queue, TEXT, "op-m06-ack");
    acknowledged.accept_possible_duplicate = true;
    let _ = fake.report(&acknowledged);
    assert_eq!(fake.tell_count(), 3);
}

/// The fixture's one event, as the observation a reconciliation read would
/// make (if one were allowed).
fn fixture_observation(sent_text: &str) -> Observation {
    let path = send_fixtures().join("thread-log-one-match.json");
    let events: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let events = events.as_array().expect("events");
    assert_eq!(events.len(), 1, "exactly one message");
    let event = &events[0];
    let input = event["data"]["input"].as_str().expect("input text");
    Observation {
        source: ObservationSource::ThreadLog,
        same_text: sha256_hex(input.as_bytes()) == sha256_hex(sent_text.as_bytes()),
        observed_at: Timestamp(event["createdAt"].as_i64().expect("createdAt")),
        // room-cli 0.43.1 documents no identifier tying the logged requestId
        // to OpenWiz's request.
        attributed: false,
    }
}

#[test]
fn one_message_matching_on_text_and_time_is_still_uncertain() {
    let observation = fixture_observation(TEXT);
    assert!(observation.same_text, "the fixture matches on text");
    let dispatched_at = T0;
    assert!(
        (observation.observed_at.0 - dispatched_at.0).abs() <= MATCH_WINDOW_MS,
        "and on time"
    );
    assert_eq!(
        reconcile(dispatched_at, std::slice::from_ref(&observation)),
        Reconciliation::Observed {
            matching: 1,
            total: 1
        },
        "an observation, not proof"
    );
    assert_eq!(
        reconcile(dispatched_at, &[]),
        Reconciliation::Observed {
            matching: 0,
            total: 0
        }
    );
    let far = Observation {
        observed_at: Timestamp(dispatched_at.0 + MATCH_WINDOW_MS + 1),
        ..observation.clone()
    };
    assert_eq!(
        reconcile(dispatched_at, &[observation.clone(), far]),
        Reconciliation::Observed {
            matching: 1,
            total: 2
        }
    );
    let other_text = fixture_observation("Synthetic other text");
    assert!(!other_text.same_text);
    assert_eq!(
        reconcile(dispatched_at, &[other_text]),
        Reconciliation::Observed {
            matching: 0,
            total: 1
        }
    );
    // Only attribution establishes delivery, and none exists under 0.43.1.
    let attributed = Observation {
        attributed: true,
        ..observation
    };
    assert_eq!(
        reconcile(dispatched_at, &[attributed]),
        Reconciliation::Attributed
    );
}

#[test]
fn the_cli_refuses_a_blind_resend_and_shows_the_earlier_send() {
    let fake = SendFake::with(
        "m06_cli_blind",
        vec![tell_response(
            "thr_syn_0001",
            "queue",
            json!({"abort": true}),
        )],
    );
    let (code, first) = fake.cli_json(&["thr_syn_0001", "--op-id", "op-m06-cli", "--", TEXT], "");
    assert_eq!(code, 6, "{first}");
    assert_eq!(first["status"], "uncertain");

    let (code, out, err) = fake.cli(&["thr_syn_0001", "--", TEXT], "");
    assert_eq!(code, 9, "{out}{err}");
    assert!(out.contains("op-m06-cli"), "the earlier op is shown: {out}");
    assert!(out.contains("--accept-possible-duplicate"), "{out}");
    assert_eq!(fake.tell_count(), 1);

    let (code, json) = fake.cli_json(&["thr_syn_0001", "--", TEXT], "");
    assert_eq!(code, 9);
    assert_eq!(json["status"], "possible_duplicate_refused");
    assert_eq!(json["result"]["earlier"][0]["op_id"], "op-m06-cli");

    let (code, _, _) = fake.cli(
        &["thr_syn_0001", "--accept-possible-duplicate", "--", TEXT],
        "",
    );
    assert_eq!(code, 6, "sent again, and uncertain again");
    assert_eq!(fake.tell_count(), 2);
}
