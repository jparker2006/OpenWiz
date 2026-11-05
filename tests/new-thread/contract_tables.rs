//! One table of good and bad JSON per contract .
//!
//! **Readers:** no new-thread or prompt type has a serde reader, so
//! `serde_json::from_str` and `from_value` can't be called on them at all:
//! the core's compile-time guard (`new_thread::no_readers`) and
//! `structure::no_new_thread_or_prompt_type_has_a_reader` prove it. Every
//! bad form below is therefore refused by both serde entry points, by
//! construction, and the schema must refuse it too. The good forms are what
//! the core writes.
//!
//! **The one reader** (A1 phase 2) is the store's: the record, read back
//! through `NewThreadRecord::read` over a `StrictJson` tree. The record
//! table runs every row through it too: it accepts exactly the rows the
//! schema accepts. **Duplicate keys** are the tree's concern: a parsed
//! `Value` keeps one of them, so no schema can see them, and the tree
//! refuses them at every depth.

use openwiz_core::messaging::send::UncertainReason;
use openwiz_core::model::Fact;
use openwiz_core::new_thread::outcome::{Outcome, RefusalReason};
use openwiz_core::new_thread::record::NewThreadRecord;
use openwiz_core::new_thread::{Binding, ProjectView};
use openwiz_core::prompt::empty_attachments_digest;
use serde_json::{Value, json};

use crate::pattern;
use crate::record_store::read;
use crate::support::*;

fn prompt_schema() -> Value {
    serde_json::from_str(&std::fs::read_to_string(PROMPT_SCHEMA).unwrap()).unwrap()
}

fn at(mut schema: Value, def: &str) -> Value {
    schema["$ref"] = json!(format!("#/$defs/{def}"));
    schema
}

/// Runs `rows` of `(good, json)` against `schema`.
fn table(name: &str, schema: &Value, rows: &[(bool, Value)]) {
    for (good, value) in rows {
        let errors = pattern::validate(schema, value);
        assert_eq!(errors.is_empty(), *good, "{name}: {value}\n{errors:#?}");
    }
}

fn with(value: &Value, pointer: &str, new: Value) -> Value {
    let mut v = value.clone();
    *v.pointer_mut(pointer)
        .unwrap_or_else(|| panic!("{pointer}")) = new;
    v
}

fn without(value: &Value, pointer: &str, key: &str) -> Value {
    let mut v = value.clone();
    v.pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove(key);
    v
}

fn adding(value: &Value, key: &str, new: Value) -> Value {
    let mut v = value.clone();
    v.as_object_mut().unwrap().insert(key.to_owned(), new);
    v
}

fn positional(value: &Value) -> Value {
    Value::Array(value.as_object().unwrap().values().cloned().collect())
}

#[test]
fn the_prompt_contract() {
    let good = json!({"text": "Synthetic", "attachments": []});
    let rows = [
        (true, good.clone()),
        (false, json!(["Synthetic", []])),
        (false, positional(&good)),
        (false, adding(&good, "extra", json!(1))),
        (false, without(&good, "", "attachments")),
        (false, without(&good, "", "text")),
        (false, json!({"prompt": good})),
        (false, with(&good, "/text", json!(["Synthetic"]))),
        (false, with(&good, "/attachments", json!({}))),
        // B2 (the implementation), with this recorded reason: B2 relaxes A1's
        // empty list (sheet the check to the check). Ids are taken, each at most once,
        // each by its own rule.
        (true, with(&good, "/attachments", json!(["att-syn-0001"]))),
        (
            true,
            with(
                &good,
                "/attachments",
                json!(["att-syn-0001", "att-syn-0002"]),
            ),
        ),
        (
            false,
            with(
                &good,
                "/attachments",
                json!(["att-syn-0001", "att-syn-0001"]),
            ),
        ),
        (false, with(&good, "/attachments", json!(["-att"]))),
        (false, with(&good, "/attachments", json!([""]))),
        (false, json!("Synthetic")),
        (false, Value::Null),
    ];
    table("prompt.v1", &prompt_schema(), &rows);
    table("new-thread prompt", &at(schema(), "prompt"), &rows);
}

#[test]
fn the_settings_and_project_reference_contracts() {
    let settings = json!({"harness": "codex", "model": "syn-model", "effort": "high"});
    table(
        "settings",
        &at(schema(), "settings"),
        &[
            (true, settings.clone()),
            (false, json!(["codex", "syn-model", "high"])),
            (false, adding(&settings, "extra", json!("x"))),
            (false, without(&settings, "", "effort")),
            (false, json!({"settings": settings})),
            (false, with(&settings, "/effort", json!({"high": null}))),
        ],
    );
    let reference = json!({"kind": "id", "value": "prj_syn_0001"});
    table(
        "project_ref",
        &at(schema(), "project_ref"),
        &[
            (true, reference.clone()),
            (
                true,
                json!({"kind": "name", "value": "Synthetic project one"}),
            ),
            (false, json!(["id", "prj_syn_0001"])),
            (false, json!({"id": "prj_syn_0001"})),
            (false, json!({"kind": "id"})),
            (
                false,
                json!({"kind": "id", "value": "prj_syn_0001", "extra": 1}),
            ),
            (false, json!({"kind": "path", "value": "prj_syn_0001"})),
            (
                false,
                json!({"kind": {"id": null}, "value": "prj_syn_0001"}),
            ),
        ],
    );
}

#[test]
fn the_string_enum_contracts() {
    for (def, label) in [
        ("record_state", "claimed"),
        ("status", "created"),
        ("binding_field", "project"),
        ("refusal_reason", "no_options"),
        ("fail_reason", "call_not_started"),
        ("uncertain_reason", "timed_out"),
    ] {
        table(
            def,
            &at(schema(), def),
            &[
                (true, json!(label)),
                (false, json!({label: null})),
                (false, json!({label: {}})),
                (false, json!([label])),
                (false, json!(format!("{label}-synthetic"))),
                (false, Value::Null),
            ],
        );
    }
}

fn record() -> Value {
    let r = request(offered(), "Synthetic", "op-tables");
    let mut record = NewThreadRecord::claimed(
        r.op_id.clone(),
        Binding::new(&r, empty_attachments_digest()),
        r.settings.clone(),
        "Synthetic",
        T0,
    );
    record.project = Some(ProjectView {
        id: "prj_syn_0001".to_owned(),
        name: Fact::Known("Synthetic project one".to_owned()),
    });
    serde_json::to_value(record).unwrap()
}

#[test]
fn the_record_contract_with_nested_forms() {
    let claimed = record();
    let finished = with(
        &with(&claimed, "/state", json!("finished")),
        "/outcome",
        serde_json::to_value(Outcome::refused(RefusalReason::NoOptions)).unwrap(),
    );
    let uncertain = with(
        &finished,
        "/outcome",
        serde_json::to_value(Outcome::uncertain(UncertainReason::TimedOut)).unwrap(),
    );
    table(
        "record",
        &at(schema(), "record"),
        &[
            (true, claimed.clone()),
            (true, finished.clone()),
            (true, uncertain.clone()),
            (false, positional(&claimed)),
            (false, without(&claimed, "", "effects")),
            (false, without(&claimed, "", "outcome")),
            (false, with(&claimed, "/state", json!({"claimed": null}))),
            (false, adding(&claimed, "extra", json!(1))),
            (
                false,
                with(&claimed, "/binding", positional(&claimed["binding"])),
            ),
            (
                false,
                with(&claimed, "/binding/project", json!(["id", "prj_syn_0001"])),
            ),
            (
                false,
                with(&claimed, "/binding/project", json!({"id": "prj_syn_0001"})),
            ),
            (
                false,
                with(
                    &claimed,
                    "/requested",
                    json!(["codex", "syn-codex-model-01", "high"]),
                ),
            ),
            (false, without(&claimed, "/binding", "attachments_digest")),
            (
                false,
                with(&finished, "/outcome/reason", json!({"no_options": null})),
            ),
            (
                false,
                with(&finished, "/outcome/kind", json!({"refused": null})),
            ),
            (
                false,
                with(
                    &finished,
                    "/outcome",
                    json!({"refused": {"reason": "no_options"}}),
                ),
            ),
            (
                false,
                with(&uncertain, "/outcome/reason", json!(["timed_out"])),
            ),
            (
                false,
                with(
                    &claimed,
                    "/project",
                    json!(["prj_syn_0001", {"known": "Synthetic"}]),
                ),
            ),
            (false, json!({"record": claimed})),
        ],
    );
}

/// The store's reader on the same table: it accepts exactly what the
/// schema accepts, and refuses a key given twice at any depth.
#[test]
fn the_records_reader_follows_the_table() {
    let schema = at(schema(), "record");
    let claimed = record();
    let finished = with(
        &with(&claimed, "/state", json!("finished")),
        "/outcome",
        serde_json::to_value(Outcome::refused(RefusalReason::NoOptions)).unwrap(),
    );
    let mut rows = vec![claimed.clone(), finished.clone()];
    for (pointer, bad) in [
        ("/state", json!({"claimed": null})),
        ("/state", json!(["claimed"])),
        ("/state", json!("Claimed")),
        ("/binding", positional(&claimed["binding"])),
        ("/binding/project", json!(["id", "prj_syn_0001"])),
        ("/binding/project", json!({"id": "prj_syn_0001"})),
        ("/binding/project/kind", json!("path")),
        ("/binding/text_sha256", json!("ABC")),
        ("/binding/harness", json!("-codex")),
        ("/requested", json!(["codex", "syn-codex-model-01", "high"])),
        ("/requested/effort", json!(" high")),
        ("/op_id", json!("-op")),
        ("/request_id", json!("ow1-xyz")),
        ("/text", json!(7)),
        ("/text_bytes", json!(-1)),
        ("/text_bytes", json!(1.5)),
        ("/recovered", json!("false")),
        ("/claimed_at", json!(null)),
        ("/gaps", json!({})),
        ("/gaps", json!([{"subject": "s"}])),
        ("/project", json!(["prj_syn_0001", {"known": "Synthetic"}])),
        ("/project/name", json!("Synthetic")),
        ("/project/name", json!({"known": "a", "unknown": "b"})),
        ("/project/name", json!({"unknown": ""})),
        ("/schema", json!("openwiz.new-thread-record.v0")),
    ] {
        rows.push(with(&claimed, pointer, bad));
    }
    for (pointer, bad) in [
        ("/outcome/reason", json!({"no_options": null})),
        ("/outcome/kind", json!({"refused": null})),
        ("/outcome", json!({"refused": {"reason": "no_options"}})),
        ("/outcome/offered", json!("none")),
        ("/outcome/text", json!("")),
    ] {
        rows.push(with(&finished, pointer, bad));
    }
    rows.push(adding(&claimed, "extra", json!(1)));
    rows.push(without(&claimed, "", "effects"));
    rows.push(without(&claimed, "/binding", "attachments_digest"));
    rows.push(adding(
        &finished["outcome"].clone(),
        "kind",
        json!("created"),
    ));
    rows.push(positional(&claimed));
    for row in rows {
        let schema_ok = pattern::validate(&schema, &row).is_empty();
        let reader = read(&row);
        assert_eq!(
            reader.is_ok(),
            schema_ok,
            "{row}
{reader:?}"
        );
    }

    let text = claimed.to_string();
    // `Value` writes its keys in order: binding's come first.
    for (from, to) in [
        ("\"schema\":", "\"schema\":\"x\",\"schema\":"),
        ("\"harness\":", "\"harness\":\"codex\",\"harness\":"),
        ("\"kind\":", "\"kind\":\"id\",\"kind\":"),
        ("\"known\":", "\"known\":\"a\",\"known\":"),
    ] {
        let twice = text.replacen(from, to, 1);
        assert_ne!(twice, text, "{from}");
        let tree: Result<openwiz_core::new_thread::read::StrictJson, _> =
            serde_json::from_str(&twice);
        assert!(tree.is_err(), "a key given twice: {twice}");
    }
}
