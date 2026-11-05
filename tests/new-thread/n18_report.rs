//! N18, the core's half (the CLI's is phase 2's): the report, the record
//! and the preview match `contracts/new-thread.v1.schema.json`, and the
//! effects are worked out, and recorded, before anything is created. A
//! preview claims nothing and calls nothing.

use openwiz_core::messaging::send::UncertainReason;
use openwiz_core::new_thread::effects::EffectKind;
use openwiz_core::new_thread::operation::{PreviewVerdict, preview_new_thread};
use openwiz_core::new_thread::outcome::{RefusalReason, SpawnReply};
use openwiz_core::new_thread::record::RecordState;
use serde_json::Value;

use crate::support::*;

fn valid(def: &str, value: &Value) {
    let errors = validate_def(def, value);
    assert!(errors.is_empty(), "{def}: {errors:#?}\n{value:#}");
}

#[test]
fn every_outcome_matches_the_schema() {
    let answers = [
        SpawnAnswer::Receipt("spawn-receipt-created.json"),
        SpawnAnswer::Error(
            Some(1),
            "Error: Failed to create thread: HTTP 404: Project not found".to_owned(),
        ),
        SpawnAnswer::Reply(SpawnReply::NotStarted),
        SpawnAnswer::Reply(SpawnReply::Unclear {
            reason: UncertainReason::TimedOut,
            gaps: vec![],
        }),
        SpawnAnswer::Receipt("spawn-receipt-drift-no-id.json"),
    ];
    for answer in answers {
        let client = FakeClient::answering(answer);
        let store = MemoryStore::new();
        let r = request(offered(), "Synthetic", "op-n18");
        let report = create(&client, &store, &r);
        valid("report", &serde_json::to_value(&report).unwrap());
        for record in store.history.lock().unwrap().iter() {
            valid("record", &serde_json::to_value(record).unwrap());
        }
        let replay = create(&client, &store, &r);
        valid("report", &serde_json::to_value(&replay).unwrap());
    }
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let mut r = request(offered(), "Synthetic", "op-n18-refused");
    r.settings.effort = "ultra".to_owned();
    valid(
        "report",
        &serde_json::to_value(create(&client, &store, &r)).unwrap(),
    );
    let mut other = r.clone();
    other.settings.effort = "high".to_owned();
    valid(
        "report",
        &serde_json::to_value(create(&client, &store, &other)).unwrap(),
    );
}

#[test]
fn the_schema_uses_only_checked_keywords() {
    let schema = schema();
    assert_eq!(schema["title"], "openwiz.new-thread.v1");
    assert_eq!(crate::pattern::lint(&schema), Vec::<String>::new());
    for def in [
        "request",
        "record",
        "report",
        "preview",
        "status_read",
        "start_view",
        "options",
        "effects",
        "outcome",
    ] {
        assert!(schema["$defs"][def].is_object(), "{def}");
    }
    let request = serde_json::json!({
        "project": {"kind": "id", "value": "prj_syn_0001"},
        "harness": "codex",
        "model": "syn-codex-model-01",
        "effort": "high",
        "prompt": {"text": "Synthetic", "attachments": []},
        "op_id": "op-1",
    });
    valid("request", &request);
    // B2 (the implementation), with this recorded reason: the request may carry
    // staged attachment ids (the check to the check), each at most once.
    let mut attached = request.clone();
    attached["prompt"]["attachments"] = serde_json::json!(["att-syn-0001"]);
    valid("request", &attached);
    attached["prompt"]["attachments"] = serde_json::json!(["att-syn-0001", "att-syn-0001"]);
    assert!(!validate_def("request", &attached).is_empty());
    let mut no_effort = request.clone();
    no_effort.as_object_mut().unwrap().remove("effort");
    assert!(
        !validate_def("request", &no_effort).is_empty(),
        "nothing defaulted"
    );
}

#[test]
fn the_effects_are_recorded_before_the_call() {
    let store = MemoryStore::leaked();
    let mut client = FakeClient::new();
    client.watched = Some(store);
    let r = request(offered(), "Synthetic", "op-n18-effects");
    create(&client, store, &r);
    let writes = client.writes_at_spawn.lock().unwrap().clone();
    assert_eq!(writes.len(), 1);
    let history = store.history.lock().unwrap();
    let before = &history[writes[0] - 1];
    assert_eq!(
        before.state,
        RecordState::Dispatching,
        "marked before the call"
    );
    let effects = before.effects.as_ref().expect("effects before the call");
    let kinds: Vec<EffectKind> = effects.effects.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            EffectKind::CreatesAndStarts,
            EffectKind::StartsFromGithub,
            EffectKind::CopiesEnvFiles,
            EffectKind::FullPermissions,
            EffectKind::UsesCloudAgent,
            EffectKind::MacStaysAwake,
            EffectKind::CloudRoomNamesIt,
        ]
    );
    let words: Vec<&str> = effects.effects.iter().map(|e| e.text.as_str()).collect();
    assert!(words[0].contains("Synthetic project one"), "{words:?}");
    for needle in [
        "GitHub",
        ".env",
        "full permissions",
        "Cloud agent",
        "awake",
        "names it",
    ] {
        assert!(
            words.iter().any(|w| w.contains(needle)),
            "{needle}: {words:?}"
        );
    }
}

#[test]
fn a_preview_shows_the_effects_and_claims_and_calls_nothing() {
    let client = FakeClient::new();
    let preview = preview_new_thread(
        &client,
        &clock(),
        &request(offered(), "Synthetic", "op-n18-p"),
    )
    .expect("a preview");
    assert_eq!(client.spawns(), 0);
    assert_eq!(
        preview.verdict,
        PreviewVerdict::Ready {
            provider: "codex".to_owned()
        }
    );
    assert_eq!(preview.effects.as_ref().unwrap().effects.len(), 7);
    valid("preview", &serde_json::to_value(&preview).unwrap());

    let mut r = request(offered(), "Synthetic", "op-n18-p2");
    r.project = openwiz_core::new_thread::ProjectRef::Id("prj_syn_0002".to_owned());
    let stopped = preview_new_thread(&client, &clock(), &r).unwrap();
    let PreviewVerdict::Stopped { outcome } = &stopped.verdict else {
        panic!("{stopped:?}")
    };
    assert!(matches!(
        outcome,
        openwiz_core::new_thread::outcome::Outcome::Refused {
            reason: RefusalReason::ProjectNoGithubRemote,
            ..
        }
    ));
    assert!(stopped.effects.is_none());
    assert_eq!(client.spawns(), 0);
    valid("preview", &serde_json::to_value(&stopped).unwrap());
}
