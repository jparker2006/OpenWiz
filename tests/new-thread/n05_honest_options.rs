//! N5: honest options. The read time, the saved-copy label and the
//! unavailable harnesses with their reasons; `harnesses: null` gives no
//! options; account fields (and every other non-catalogue field) are never
//! parsed into the core. The CLI's half (`openwiz models`) is phase 2's.

use openwiz_adapter_cloudroom::new_thread::{CLI_REASONING_LEVELS, parse_catalogue};
use openwiz_core::new_thread::options::{
    NoOptionsReason, OptionsState, SAVED_COPY_LABEL, cloud_options,
};
use openwiz_core::source::SourceError;

use crate::support::*;

#[test]
fn the_options_carry_the_read_time_and_the_saved_copy_label() {
    let o = cloud_options(
        parse_catalogue(&fixture("cloud-status.json")),
        CLI_REASONING_LEVELS,
        T0,
    );
    assert_eq!(o.read_at, T0);
    assert_eq!(o.label, SAVED_COPY_LABEL);
    assert!(o.label.contains("saved copy"));
    let cursor = o
        .unavailable
        .iter()
        .find(|u| u.harness == "cursor")
        .unwrap();
    assert_eq!(cursor.text, "unavailable (catalogue not reported)");
    assert!(validate_def("options", &serde_json::to_value(&o).unwrap()).is_empty());
}

fn no_options(
    read: Result<openwiz_core::new_thread::options::Catalogue, SourceError>,
) -> NoOptionsReason {
    let o = cloud_options(read, CLI_REASONING_LEVELS, T0);
    assert!(o.harnesses.is_empty(), "{o:?}");
    assert!(validate_def("options", &serde_json::to_value(&o).unwrap()).is_empty());
    match o.state {
        OptionsState::NoOptions { reason, .. } => reason,
        OptionsState::Offered => panic!("offered: {o:?}"),
    }
}

#[test]
fn not_ready_null_harnesses_and_a_failed_read_give_no_options() {
    assert_eq!(
        no_options(parse_catalogue(&fixture("cloud-status-not-ready.json"))),
        NoOptionsReason::NotReady
    );
    assert_eq!(
        no_options(parse_catalogue(&fixture(
            "cloud-status-harnesses-null.json"
        ))),
        NoOptionsReason::HarnessesNotReported
    );
    assert_eq!(
        no_options(Err(SourceError::TimedOut { after_ms: 15_000 })),
        NoOptionsReason::ReadFailed
    );
}

/// The account and every other field are dropped at the adapter's boundary:
/// nothing of them reaches the core, whatever their type.
#[test]
fn account_and_other_fields_never_reach_the_core() {
    let mut doc = fixture_json("cloud-status.json");
    doc["account"] = serde_json::json!({
        "email": "someone@example.invalid",
        "name": "Synthetic Owner Name",
        "id": 42
    });
    doc["harnesses"][0]["secretToken"] = serde_json::json!("Synthetic token value");
    doc["harnesses"][0]["models"][0]["syntheticOwner"] = serde_json::json!("Synthetic owner");
    for planted in [doc.clone(), {
        let mut odd = doc.clone();
        odd["account"] = serde_json::json!(["Synthetic", 5, null]);
        odd["planUsage"] = serde_json::json!("Synthetic plan text");
        odd
    }] {
        let catalogue = parse_catalogue(planted.to_string().as_bytes()).expect("still parses");
        let debug = format!("{catalogue:?}");
        let options = cloud_options(Ok(catalogue), CLI_REASONING_LEVELS, T0);
        let json = serde_json::to_string(&options).unwrap();
        for leak in [
            "someone",
            "example.invalid",
            "Owner",
            "account",
            "token",
            "planUsage",
            "credits",
            "syn-plan",
            "syn-codex-default",
        ] {
            assert!(!debug.contains(leak), "{leak} reached the core: {debug}");
            assert!(!json.contains(leak), "{leak} in the options: {json}");
        }
    }
}
