//! N6: outcome mapping for every row of the result table. Created never
//! implies started.

use openwiz_adapter_cloudroom::new_thread::{DOCUMENTED_REFUSALS, spawn_error, spawn_receipt};
use openwiz_core::messaging::send::UncertainReason;
use openwiz_core::new_thread::operation::NewThreadResult;
use openwiz_core::new_thread::outcome::{
    FailReason, Outcome, ProviderRefusal, RefusalReason, SpawnReply, Status,
};
use openwiz_core::new_thread::record::RecordState;

use crate::support::*;

fn outcome_of(answer: SpawnAnswer, op_id: &str) -> (Outcome, MemoryStore, FakeClient) {
    let client = FakeClient::answering(answer);
    let store = MemoryStore::new();
    let report = create(&client, &store, &request(offered(), "Synthetic", op_id));
    assert_eq!(client.spawns(), 1, "exactly one call");
    let NewThreadResult::Outcome { outcome, .. } = report.result else {
        panic!("{report:?}")
    };
    (outcome, store, client)
}

#[test]
fn a_receipt_is_created_and_never_started() {
    assert_eq!(
        spawn_receipt(&fixture("spawn-receipt-created.json"), "prj_syn_0001"),
        SpawnReply::Created {
            thread_id: "thr_syn_0021".to_owned()
        }
    );
    let (outcome, store, _) = outcome_of(
        SpawnAnswer::Receipt("spawn-receipt-created.json"),
        "op-n06-created",
    );
    let Outcome::Created { thread_id, text } = &outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(thread_id, "thr_syn_0021");
    assert!(!text.contains("started"), "{text}");
    assert!(text.contains("not a start"), "{text}");
    let record = store.get(&op("op-n06-created")).unwrap();
    assert_eq!(record.state, RecordState::Finished);
    assert_eq!(record.text, None, "the text is erased once created");
    let json = serde_json::to_string(&record).unwrap();
    assert!(!json.contains("\"started\""), "no start stage in a create");
}

#[test]
fn every_documented_refusal_is_refused_and_nothing_is_created() {
    assert_eq!(DOCUMENTED_REFUSALS.len(), 8);
    for (status, message, refusal) in DOCUMENTED_REFUSALS {
        let line = format!("Error: Failed to create thread: HTTP {status}: {message} (Synthetic)");
        assert_eq!(
            spawn_error(Some(1), &line),
            SpawnReply::Refused(*refusal),
            "{line}"
        );
        let (outcome, store, _) = outcome_of(
            SpawnAnswer::Error(Some(1), line.clone()),
            &format!("op-n06-r{status}"),
        );
        let Outcome::Refused { reason, text, .. } = &outcome else {
            panic!("{line}: {outcome:?}")
        };
        assert_eq!(*reason, RefusalReason::from(*refusal), "{line}");
        assert!(text.contains("nothing was created"), "{text}");
        assert!(
            !text.contains("Synthetic"),
            "room-cli's words are not echoed"
        );
        let record = store.get(&op(&format!("op-n06-r{status}"))).unwrap();
        assert!(record.text.is_some(), "the prompt is kept");
    }
    assert!(
        DOCUMENTED_REFUSALS
            .iter()
            .any(|(_, _, r)| *r == ProviderRefusal::RequestIdConflict)
    );
}

#[test]
fn anything_else_is_uncertain() {
    let documented = "Error: Failed to create thread: HTTP 400: Message is empty";
    for (code, line) in [
        (
            Some(1),
            "Error: Failed to create thread: HTTP 500: Synthetic server error",
        ),
        (
            Some(1),
            "Error: Failed to create thread: HTTP 404: Message is empty",
        ),
        (Some(2), documented),
        (Some(1), "Error: HTTP 400: Message is empty"),
        (Some(1), "error: unknown option '--synthetic'"),
        (Some(1), ""),
        (
            Some(1),
            "Synthetic Error: Failed to create thread: HTTP 400: Message is empty",
        ),
    ] {
        let reply = spawn_error(code, line);
        assert!(
            matches!(
                reply,
                SpawnReply::Unclear {
                    reason: UncertainReason::UndocumentedError,
                    ..
                }
            ),
            "{code:?} {line:?}: {reply:?}"
        );
    }
    assert!(matches!(
        spawn_error(None, documented),
        SpawnReply::Unclear {
            reason: UncertainReason::EndedWithoutExitCode,
            ..
        }
    ));
    for reason in [
        UncertainReason::TimedOut,
        UncertainReason::EndedWithoutExitCode,
        UncertainReason::CallNotConfirmed,
    ] {
        let (outcome, store, _) = outcome_of(
            SpawnAnswer::Reply(SpawnReply::Unclear {
                reason,
                gaps: vec![],
            }),
            "op-n06-unclear",
        );
        assert_eq!(outcome.status(), Status::Uncertain, "{reason:?}");
        assert!(outcome.text().contains("may or may not have been created"));
        assert!(store.get(&op("op-n06-unclear")).unwrap().text.is_some());
    }
}

#[test]
fn a_call_that_never_started_failed_and_sent_nothing() {
    let (outcome, _, _) = outcome_of(SpawnAnswer::Reply(SpawnReply::NotStarted), "op-n06-ns");
    assert_eq!(
        outcome,
        Outcome::failed(FailReason::CallNotStarted),
        "{outcome:?}"
    );
}
