//! N17: drift. An unexpected receipt shape gives uncertain with a gap; an
//! unexpected catalogue shape gives no options. The version is reported,
//! never a gate: the draft's "an untested version means nothing is
//! created" is dropped (sheet, "Already decided").

use openwiz_adapter_cloudroom::new_thread::{parse_catalogue, spawn_receipt};
use openwiz_core::messaging::send::UncertainReason;
use openwiz_core::model::Fact;
use openwiz_core::new_thread::operation::NewThreadResult;
use openwiz_core::new_thread::outcome::{Outcome, RefusalReason, SpawnReply, Status};
use openwiz_core::source::SourceError;

use crate::support::*;

#[test]
fn an_unexpected_receipt_is_uncertain_with_a_gap() {
    for (file, reason) in [
        (
            "spawn-receipt-drift-no-id.json",
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "spawn-receipt-drift-other-project.json",
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "spawn-receipt-drift-unsafe-id.json",
            UncertainReason::UnexpectedReceipt,
        ),
        (
            "spawn-receipt-drift-array.json",
            UncertainReason::UnexpectedReceipt,
        ),
        ("spawn-receipt-not-json.txt", UncertainReason::NotJson),
    ] {
        let reply = spawn_receipt(&fixture(file), "prj_syn_0001");
        let SpawnReply::Unclear { reason: got, gaps } = &reply else {
            panic!("{file}: {reply:?}")
        };
        assert_eq!(*got, reason, "{file}");
        assert_eq!(gaps.len(), 1, "{file}");
        let client = FakeClient::answering(SpawnAnswer::Receipt(file));
        let store = MemoryStore::new();
        let report = create(
            &client,
            &store,
            &request(offered(), "Synthetic", "op-n17-r"),
        );
        assert_eq!(report.status, Status::Uncertain, "{file}");
        assert!(!report.gaps.is_empty(), "{file}: a gap");
        let json = serde_json::to_string(&report).unwrap();
        for echoed in ["prj_syn_0002", "Synthetic receipt", "Synthetic unsafe id"] {
            assert!(!json.contains(echoed), "{file}: {echoed} echoed");
        }
    }
}

#[test]
fn an_unexpected_catalogue_gives_no_options_and_no_call() {
    for file in [
        "cloud-status-drift-ready-not-bool.json",
        "cloud-status-drift-models-object.json",
        "cloud-status-drift-levels-not-list.json",
        "cloud-status-drift-array.json",
    ] {
        assert!(
            matches!(
                parse_catalogue(&fixture(file)),
                Err(SourceError::Malformed { .. })
            ),
            "{file}"
        );
        let client = FakeClient::with_catalogue(file);
        let store = MemoryStore::new();
        let report = create(
            &client,
            &store,
            &request(offered(), "Synthetic", "op-n17-c"),
        );
        let NewThreadResult::Outcome {
            outcome: Outcome::Refused { reason, .. },
            ..
        } = &report.result
        else {
            panic!("{file}: {report:?}")
        };
        assert_eq!(*reason, RefusalReason::NoOptions, "{file}");
        assert_eq!(client.spawns(), 0, "{file}");
    }
}

#[test]
fn an_untested_or_unreadable_version_is_reported_never_a_gate() {
    for version in [
        Ok("v999".to_owned()),
        Ok("Synthetic".to_owned()),
        Err(SourceError::TimedOut { after_ms: 15_000 }),
    ] {
        let mut client = FakeClient::new();
        client.version = version.clone();
        let store = MemoryStore::new();
        let report = create(
            &client,
            &store,
            &request(offered(), "Synthetic", "op-n17-v"),
        );
        assert_eq!(report.status, Status::Created, "{version:?}");
        assert_eq!(client.spawns(), 1, "{version:?}");
        let source = report.source.expect("reported");
        assert!(!source.version_tested, "{version:?}");
        if version.is_err() || version == Ok("Synthetic".to_owned()) {
            assert!(matches!(source.version, Fact::Unknown(_)));
            assert!(!report.gaps.is_empty(), "{version:?}: a gap");
        }
    }
}
