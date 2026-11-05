use openwiz_core::attachment::{AttachmentRef, FailReason, Kind, Readiness};
use serde_json::{Value, json};

use crate::support::validate_def;

const ID: &str = "att_syn_0001";
const HASH: &str = "5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a";

fn attachment() -> Value {
    json!({
        "id": ID,
        "sha256": HASH,
        "size_bytes": 1024,
        "kind": "file",
        "name": "synthetic.txt",
        "display_name": "synthetic.txt",
    })
}

fn with(field: &str, value: Value) -> Value {
    let mut v = attachment();
    v[field] = value;
    v
}

fn without(field: &str) -> Value {
    let mut v = attachment();
    v.as_object_mut().unwrap().remove(field);
    v
}

/// (schema `$defs` entry, the JSON, whether it is valid).
fn table() -> Vec<(&'static str, Value, bool)> {
    let mut rows = vec![
        // kind: a string from the list, nothing else.
        ("kind", json!("image"), true),
        ("kind", json!("file"), true),
        ("kind", json!("File"), false),
        ("kind", json!("video"), false),
        ("kind", json!({"file": null}), false),
        ("kind", json!(["file"]), false),
        ("kind", json!(0), false),
        ("kind", json!(null), false),
        ("kind", json!(true), false),
        // fail_reason: the same.
        ("fail_reason", json!("low_disk"), true),
        ("fail_reason", json!("type_unclear"), true),
        ("fail_reason", json!("LowDisk"), false),
        ("fail_reason", json!("too_big"), false),
        ("fail_reason", json!({"low_disk": null}), false),
        ("fail_reason", json!(["low_disk"]), false),
        ("fail_reason", json!(1), false),
        ("fail_reason", json!(null), false),
        // readiness: an object; state a string from the list; reason only
        // on failed, and there required.
        ("readiness", json!({"state": "ready"}), true),
        ("readiness", json!({"state": "selected"}), true),
        ("readiness", json!({"state": "expired"}), true),
        (
            "readiness",
            json!({"state": "failed", "reason": "low_disk"}),
            true,
        ),
        ("readiness", json!(["ready"]), false),
        ("readiness", json!(["failed", "low_disk"]), false),
        ("readiness", json!("ready"), false),
        ("readiness", json!(null), false),
        ("readiness", json!({}), false),
        ("readiness", json!({"state": {"ready": null}}), false),
        ("readiness", json!({"state": ["ready"]}), false),
        ("readiness", json!({"state": null}), false),
        ("readiness", json!({"state": "Ready"}), false),
        ("readiness", json!({"state": "sent"}), false),
        ("readiness", json!({"state": "ready", "extra": true}), false),
        (
            "readiness",
            json!({"state": "removed", "reason": "low_disk"}),
            false,
        ),
        (
            "readiness",
            json!({"state": "ready", "reason": null}),
            false,
        ),
        (
            "readiness",
            json!({"state": "selected", "reason": "folder"}),
            false,
        ),
        ("readiness", json!({"state": "failed"}), false),
        (
            "readiness",
            json!({"state": "failed", "reason": null}),
            false,
        ),
        (
            "readiness",
            json!({"state": "failed", "reason": {"low_disk": null}}),
            false,
        ),
        (
            "readiness",
            json!({"state": "failed", "reason": ["low_disk"]}),
            false,
        ),
        (
            "readiness",
            json!({"state": "failed", "reason": "too_big"}),
            false,
        ),
        (
            "readiness",
            json!({"state": "failed", "reason": "low_disk", "extra": 1}),
            false,
        ),
        ("readiness", json!({"reason": "low_disk"}), false),
        // attachment: an object with exactly its six keys.
        ("attachment", attachment(), true),
        ("attachment", with("kind", json!("image")), true),
        ("attachment", with("size_bytes", json!(0)), true),
        ("attachment", with("display_name", json!("")), true),
        (
            "attachment",
            with("display_name", json!("../報告\u{202e}.txt")),
            true,
        ),
        ("attachment", with("id", json!("A-0_z")), true),
        ("attachment", with("name", json!("_env")), true),
        ("attachment", with("name", json!("a.b-c_d")), true),
        (
            "attachment",
            with("sha256", json!("0123456789abcdef".repeat(4))),
            true,
        ),
        (
            "attachment",
            json!([ID, HASH, 1024, "file", "synthetic.txt", "synthetic.txt"]),
            false,
        ),
        ("attachment", json!(null), false),
        ("attachment", with("kind", json!({"file": null})), false),
        ("attachment", with("kind", json!(["file"])), false),
        ("attachment", with("kind", json!("video")), false),
        ("attachment", with("size_bytes", json!(-1)), false),
        ("attachment", with("size_bytes", json!(1.5)), false),
        ("attachment", with("size_bytes", json!("1024")), false),
        ("attachment", with("display_name", json!(null)), false),
        (
            "attachment",
            with("display_name", json!(["synthetic.txt"])),
            false,
        ),
        ("attachment", with("mime_type", json!("image/png")), false),
        ("attachment", with("id", json!("../escape")), false),
        ("attachment", with("id", json!("_a")), false),
        ("attachment", with("id", json!("a.b")), false),
        ("attachment", with("id", json!("a b")), false),
        ("attachment", with("id", json!("")), false),
        ("attachment", with("id", json!(["att_syn_0001"])), false),
        ("attachment", with("name", json!("../escape")), false),
        ("attachment", with("name", json!("-rf")), false),
        ("attachment", with("name", json!(".env")), false),
        ("attachment", with("name", json!("a b")), false),
        ("attachment", with("name", json!("é.txt")), false),
        ("attachment", with("name", json!("a".repeat(180))), false),
        ("attachment", with("sha256", json!("z".repeat(64))), false),
        ("attachment", with("sha256", json!("A".repeat(64))), false),
        (
            "attachment",
            with("sha256", json!("0".repeat(63) + " ")),
            false,
        ),
        ("attachment", with("sha256", json!("0".repeat(63))), false),
    ];
    for field in ["id", "sha256", "size_bytes", "kind", "name", "display_name"] {
        rows.push(("attachment", without(field), false));
    }
    rows
}

/// Every way the core reads `json` as `$t`: from text and from a value.
macro_rules! core_reads {
    ($t:ty, $json:expr) => {
        (
            serde_json::from_str::<$t>(&$json.to_string()).is_ok(),
            serde_json::from_value::<$t>($json.clone()).is_ok(),
        )
    };
}

#[test]
fn the_core_reads_exactly_the_shapes_the_schema_allows() {
    let mut disagreements = Vec::new();
    let rows = table();
    for (def, json, valid) in &rows {
        let schema = validate_def(def, json).is_empty();
        let (text, value) = match *def {
            "kind" => core_reads!(Kind, json),
            "fail_reason" => core_reads!(FailReason, json),
            "readiness" => core_reads!(Readiness, json),
            "attachment" => core_reads!(AttachmentRef, json),
            other => panic!("no reader for {other}"),
        };
        if schema != *valid || text != *valid || value != *valid {
            disagreements.push(format!(
                "{def} {json}: expected {}, schema {}, core from text {}, core from value {}",
                verdict(*valid),
                verdict(schema),
                verdict(text),
                verdict(value)
            ));
        }
    }
    assert!(rows.len() > 80, "the table has {} rows", rows.len());
    assert!(
        disagreements.is_empty(),
        "{} of {} rows disagree:\n{}",
        disagreements.len(),
        rows.len(),
        disagreements.join("\n")
    );
}

fn verdict(ok: bool) -> &'static str {
    if ok { "accepts" } else { "refuses" }
}

/// Text the schema can't see (a parsed value has one entry per key): a key
/// given twice is refused by the core.
#[test]
fn a_key_given_twice_is_refused() {
    let mut accepted = Vec::new();
    for text in [
        r#"{"state":"ready","state":"ready"}"#,
        r#"{"state":"failed","reason":"low_disk","reason":"folder"}"#,
    ] {
        if serde_json::from_str::<Readiness>(text).is_ok() {
            accepted.push(text.to_owned());
        }
    }
    let twice = attachment()
        .to_string()
        .replacen('{', r#"{"kind":"image","#, 1);
    if serde_json::from_str::<AttachmentRef>(&twice).is_ok() {
        accepted.push(twice);
    }
    assert!(accepted.is_empty(), "accepted: {accepted:#?}");
}

/// The core's label lists are the schema's enums, in the same order, so
/// the strings each side accepts are one list.
#[test]
fn the_label_lists_are_the_schemas_enums() {
    let schema = crate::support::attachment_schema();
    let enum_of = |pointer: &str| -> Vec<String> {
        schema
            .pointer(pointer)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("no enum at {pointer}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };
    let kinds: Vec<String> = Kind::labels()
        .iter()
        .map(|(l, _)| (*l).to_owned())
        .collect();
    assert_eq!(kinds, enum_of("/$defs/kind/enum"));
    let reasons: Vec<String> = FailReason::labels()
        .iter()
        .map(|(l, _)| (*l).to_owned())
        .collect();
    assert_eq!(reasons, enum_of("/$defs/fail_reason/enum"));
    // And each label writes back as itself.
    for (label, kind) in Kind::labels() {
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(label));
    }
    for (label, reason) in FailReason::labels() {
        assert_eq!(serde_json::to_value(reason).unwrap(), json!(label));
    }
}
