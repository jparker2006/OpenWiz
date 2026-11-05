//! N19: status is read-only. `new_thread_status` makes zero `spawn` calls
//! and no write, for every record state; its observations never change an
//! Uncertain outcome. With N8: creation and start are reported apart.

use std::time::Duration;

use openwiz_core::messaging::send::UncertainReason;
use openwiz_core::model::Fact;
use openwiz_core::new_thread::operation::{NewThreadResult, ObservedThread};
use openwiz_core::new_thread::outcome::{FailReason, Outcome, RefusalReason, Status};
use openwiz_core::new_thread::record::{NewThreadRecord, RecordState};
use openwiz_core::new_thread::start::StartStage;
use openwiz_core::new_thread::status::{
    NewThreadStatus, OBSERVATIONS_ONLY, Recorded, StatusResult, new_thread_status,
};
use openwiz_core::new_thread::{Binding, ProjectView};
use openwiz_core::prompt::empty_attachments_digest;

use crate::support::*;

fn record(op_id: &str, state: RecordState, outcome: Option<Outcome>) -> NewThreadRecord {
    let r = request(offered(), "Synthetic kept", op_id);
    let mut record = NewThreadRecord::claimed(
        r.op_id.clone(),
        Binding::new(&r, empty_attachments_digest()),
        r.settings.clone(),
        "Synthetic kept",
        T0,
    );
    record.project = Some(ProjectView {
        id: "prj_syn_0001".to_owned(),
        name: Fact::Known("Synthetic project one".to_owned()),
    });
    record.provider = Some("codex".to_owned());
    match (state, outcome) {
        (RecordState::Claimed, _) => record,
        (RecordState::Dispatching, _) => record.dispatching(T0),
        (RecordState::Finished, Some(outcome)) => record.dispatching(T0).finished(outcome, T0),
        (RecordState::Finished, None) => unreachable!(),
    }
}

fn status(client: &FakeClient, store: &MemoryStore, op_id: &str) -> NewThreadStatus {
    let s = new_thread_status(client, store, &clock(), &op(op_id), Duration::from_secs(5))
        .expect("a status");
    let errors = validate_def("status_read", &serde_json::to_value(&s).unwrap());
    assert!(errors.is_empty(), "{errors:#?}");
    s
}

#[test]
fn every_record_state_reads_with_no_spawn_and_no_write() {
    let created = Outcome::created("thr_syn_0021".to_owned());
    let cases = [
        ("op-n19-claimed-live", RecordState::Claimed, None, true),
        (
            "op-n19-dispatching-live",
            RecordState::Dispatching,
            None,
            true,
        ),
        ("op-n19-claimed-stopped", RecordState::Claimed, None, false),
        (
            "op-n19-dispatching-stopped",
            RecordState::Dispatching,
            None,
            false,
        ),
        (
            "op-n19-created",
            RecordState::Finished,
            Some(created),
            false,
        ),
        (
            "op-n19-refused",
            RecordState::Finished,
            Some(Outcome::refused(RefusalReason::EffortNotOffered)),
            false,
        ),
        (
            "op-n19-failed",
            RecordState::Finished,
            Some(Outcome::failed(FailReason::CallNotStarted)),
            false,
        ),
        (
            "op-n19-uncertain",
            RecordState::Finished,
            Some(Outcome::uncertain(UncertainReason::TimedOut)),
            false,
        ),
    ];
    for (op_id, state, outcome, live) in cases {
        let mut client = FakeClient::new();
        client.shown.insert(
            "thr_syn_0021".to_owned(),
            fixture("thread-show-pending.json"),
        );
        let store = MemoryStore::new();
        let seeded = record(op_id, state, outcome);
        store.seed(seeded.clone(), live);
        let s = status(&client, &store, op_id);
        assert_eq!(client.spawns(), 0, "{op_id}");
        assert_eq!(store.writes(), 0, "{op_id}: no write");
        assert_eq!(store.get(&op(op_id)), Some(seeded), "{op_id}: unchanged");
        let StatusResult::Recorded(recorded) = &s.result else {
            panic!("{op_id}: {s:?}")
        };
        let report = &recorded.report;
        let expected = match (op_id, live) {
            (_, true) => Status::InProgress,
            ("op-n19-claimed-stopped", _) | ("op-n19-failed", _) => Status::Failed,
            ("op-n19-dispatching-stopped", _) | ("op-n19-uncertain", _) => Status::Uncertain,
            ("op-n19-created", _) => Status::Created,
            _ => Status::Refused,
        };
        assert_eq!(report.status, expected, "{op_id}");
    }
    let client = FakeClient::new();
    let store = MemoryStore::new();
    assert_eq!(
        status(&client, &store, "op-n19-none").result,
        StatusResult::NoRecord
    );
    assert_eq!(client.calls(), 0);
}

#[test]
fn a_created_thread_shows_its_start_apart_from_its_creation() {
    let mut client = FakeClient::new();
    client.shown.insert(
        "thr_syn_0021".to_owned(),
        fixture("thread-show-pending.json"),
    );
    let store = MemoryStore::new();
    store.seed(
        record(
            "op-n19-start",
            RecordState::Finished,
            Some(Outcome::created("thr_syn_0021".to_owned())),
        ),
        false,
    );
    let s = status(&client, &store, "op-n19-start");
    let StatusResult::Recorded(recorded) = s.result else {
        panic!()
    };
    let Recorded { report, start, .. } = *recorded;
    assert_eq!(report.status, Status::Created);
    let start = start.expect("a start view");
    assert!(matches!(start.stage, StartStage::WaitingToStart { .. }));
    assert_eq!(
        client.show_calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(client.spawns(), 0);
}

#[test]
fn observations_never_settle_an_uncertain_create() {
    let mut client = FakeClient::new();
    client.threads = Ok(vec![ObservedThread {
        id: "thr_syn_0021".to_owned(),
        created_at: Fact::Known(T0),
    }]);
    let store = MemoryStore::new();
    store.seed(
        record(
            "op-n19-observe",
            RecordState::Finished,
            Some(Outcome::uncertain(UncertainReason::TimedOut)),
        ),
        false,
    );
    let s = status(&client, &store, "op-n19-observe");
    let StatusResult::Recorded(recorded) = s.result else {
        panic!()
    };
    let Recorded {
        report,
        observations,
        start,
    } = *recorded;
    assert_eq!(report.status, Status::Uncertain, "still uncertain");
    let NewThreadResult::Outcome { outcome, .. } = &report.result else {
        panic!()
    };
    assert!(matches!(outcome, Outcome::Uncertain { .. }));
    assert!(start.is_none());
    let observations = observations.expect("observed");
    assert_eq!(observations.threads.len(), 1);
    assert_eq!(observations.text, OBSERVATIONS_ONLY);
    assert_eq!(client.spawns(), 0);
    assert_eq!(store.writes(), 0);
    assert_eq!(
        store.get(&op("op-n19-observe")).unwrap().text.as_deref(),
        Some("Synthetic kept"),
        "the prompt is still kept"
    );
}
