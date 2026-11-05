//! N4: offered combinations only. A harness, model or effort outside the
//! same-run catalogue is refused with zero `spawn` calls; an off-catalogue
//! Claude Code model is refused; efforts are the catalogue's and the CLI's
//! intersection; nothing is defaulted or substituted.

use openwiz_adapter_cloudroom::new_thread::{CLI_REASONING_LEVELS, parse_catalogue};
use openwiz_core::new_thread::operation::NewThreadError;
use openwiz_core::new_thread::options::{OptionsState, UnavailableReason, cloud_options};
use openwiz_core::new_thread::outcome::{Outcome, RefusalReason, Status};

use crate::support::*;

fn options() -> openwiz_core::new_thread::options::CloudOptions {
    cloud_options(
        parse_catalogue(&fixture("cloud-status.json")),
        CLI_REASONING_LEVELS,
        T0,
    )
}

#[test]
fn only_codex_and_claude_code_are_offered_with_the_intersection_of_efforts() {
    let o = options();
    assert_eq!(o.state, OptionsState::Offered);
    let offered: Vec<&str> = o.harnesses.iter().map(|h| h.harness.as_str()).collect();
    assert_eq!(offered, ["codex", "claude-code"]);
    assert_eq!(o.harnesses[0].models.len(), 10);
    assert_eq!(o.harnesses[1].models.len(), 9);
    for h in &o.harnesses {
        assert_eq!(h.provider, h.harness, "the verified mapping");
        for m in &h.models {
            assert_eq!(m.efforts, CLI_REASONING_LEVELS, "{}", m.model);
        }
    }
    let unavailable: Vec<(&str, UnavailableReason)> = o
        .unavailable
        .iter()
        .map(|u| (u.harness.as_str(), u.reason))
        .collect();
    assert_eq!(unavailable.len(), 7);
    assert!(
        unavailable
            .iter()
            .all(|(_, r)| *r == UnavailableReason::CatalogueNotReported),
        "{unavailable:?}"
    );
}

fn refused(settings: openwiz_core::new_thread::Settings, why: RefusalReason) {
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let label = format!("{settings:?}");
    let report = create(&client, &store, &request(settings, "Synthetic", "op-n04"));
    assert_eq!(report.status, Status::Refused, "{label}");
    let openwiz_core::new_thread::operation::NewThreadResult::Outcome { outcome, .. } =
        &report.result
    else {
        panic!("{label}: {report:?}")
    };
    let Outcome::Refused { reason, .. } = outcome else {
        panic!("{label}: {outcome:?}")
    };
    assert_eq!(*reason, why, "{label}");
    assert_eq!(client.spawns(), 0, "{label}: zero spawn calls");
    assert!(report.summary.contains("nothing was created"), "{label}");
}

#[test]
fn an_unoffered_harness_model_or_effort_is_refused_with_zero_calls() {
    refused(
        settings("cursor", "syn-cursor-default", "high"),
        RefusalReason::HarnessNotOffered,
    );
    refused(
        settings("syn-harness", "syn-codex-model-01", "high"),
        RefusalReason::HarnessNotOffered,
    );
    refused(
        settings("codex", "syn-claude-model-01", "high"),
        RefusalReason::ModelNotOffered,
    );
    refused(
        settings("codex", "syn-codex-default", "high"),
        RefusalReason::ModelNotOffered,
    );
    for effort in ["ultra", "none", "minimal", "syn-effort"] {
        refused(
            settings("codex", "syn-codex-model-01", effort),
            RefusalReason::EffortNotOffered,
        );
    }
}

#[test]
fn an_off_catalogue_claude_code_model_is_refused() {
    refused(
        settings("claude-code", "syn-claude-model-99", "high"),
        RefusalReason::ModelNotOffered,
    );
}

#[test]
fn the_refusal_names_what_is_offered_and_substitutes_nothing() {
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let report = create(
        &client,
        &store,
        &request(
            settings("codex", "syn-codex-model-01", "ultra"),
            "Synthetic",
            "op-n04-o",
        ),
    );
    let openwiz_core::new_thread::operation::NewThreadResult::Outcome {
        outcome: Outcome::Refused { offered, text, .. },
        ..
    } = &report.result
    else {
        panic!("{report:?}")
    };
    assert_eq!(offered, &CLI_REASONING_LEVELS);
    assert!(text.contains("Nothing was substituted"), "{text}");
    assert_eq!(
        report.requested.effort, "ultra",
        "the request is kept as it was"
    );
}

#[test]
fn no_options_refuses_everything() {
    for file in [
        "cloud-status-not-ready.json",
        "cloud-status-harnesses-null.json",
    ] {
        let client = FakeClient::with_catalogue(file);
        let store = MemoryStore::new();
        let report = create(
            &client,
            &store,
            &request(offered(), "Synthetic", "op-n04-none"),
        );
        assert_eq!(report.status, Status::Refused, "{file}");
        assert_eq!(client.spawns(), 0, "{file}");
    }
}

#[test]
fn an_unverified_mapping_is_unavailable_even_with_models() {
    let mut doc = fixture_json("cloud-status.json");
    doc["harnesses"][1]["models"] =
        serde_json::json!([{"model": "syn-pi-model-01", "reasoning_levels": ["high"]}]);
    let o = cloud_options(
        parse_catalogue(doc.to_string().as_bytes()),
        CLI_REASONING_LEVELS,
        T0,
    );
    let pi = o
        .unavailable
        .iter()
        .find(|u| u.harness == "pi")
        .expect("pi");
    assert_eq!(pi.reason, UnavailableReason::IdMappingNotVerified);
    assert!(o.harnesses.iter().all(|h| h.harness != "pi"));
}

#[test]
fn a_model_with_no_shared_effort_is_not_offered() {
    let mut doc = fixture_json("cloud-status.json");
    doc["harnesses"][0]["models"] = serde_json::json!([{"model": "syn-codex-model-01", "reasoning_levels": ["ultra", "minimal"]}]);
    let o = cloud_options(
        parse_catalogue(doc.to_string().as_bytes()),
        CLI_REASONING_LEVELS,
        T0,
    );
    let codex = o
        .unavailable
        .iter()
        .find(|u| u.harness == "codex")
        .expect("codex");
    assert_eq!(codex.reason, UnavailableReason::NoOfferedEffort);
}

#[test]
fn nothing_is_defaulted() {
    let client = FakeClient::new();
    let store = MemoryStore::new();
    for s in [
        settings("", "syn-codex-model-01", "high"),
        settings("codex", "", "high"),
        settings("codex", "syn-codex-model-01", ""),
    ] {
        let label = format!("{s:?}");
        assert!(
            matches!(
                try_create(&client, &store, &request(s, "Synthetic", "op-n04-d")),
                Err(NewThreadError::Invalid(_))
            ),
            "{label}"
        );
    }
    assert_eq!(client.calls(), 0, "nothing was read or called");
    let json = serde_json::to_value(options()).unwrap();
    let mut words = Vec::new();
    strings(&json, &mut words);
    assert!(
        !words
            .iter()
            .any(|w| w == "default" || w.contains("-default")),
        "no default is offered: {words:?}"
    );
}
