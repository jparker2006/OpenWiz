//! Schema parity: `contracts/prompt.v1.schema.json`
//! and `contracts/new-thread.v1.schema.json` accept exactly what the Rust
//! core accepts, for the prompt text, the op id, the attachment id, the
//! three settings and the project reference. The schemas use `pattern` and
//! the companion `maxUtf8Bytes`, enforced by [`crate::pattern`].

use openwiz_core::messaging::{MAX_TEXT_BYTES, OpId};
use openwiz_core::new_thread::ProjectRef;
use openwiz_core::prompt::{AttachmentId, PromptPayload};
use serde_json::{Value, json};

use crate::pattern;
use crate::support::*;

fn prompt_schema() -> Value {
    serde_json::from_str(&std::fs::read_to_string(PROMPT_SCHEMA).unwrap()).unwrap()
}

fn at(mut schema: Value, def: &str) -> Value {
    schema["$ref"] = json!(format!("#/$defs/{def}"));
    schema
}

fn schema_accepts(schema: &Value, value: &Value) -> bool {
    pattern::validate(schema, value).is_empty()
}

fn texts() -> Vec<String> {
    let mut t: Vec<String> = [
        "Synthetic",
        " Synthetic padded ",
        "-leading dash",
        "Line one\nLine two",
        "\u{FEFF}",
        "\u{200B}",
        "Synthétique 合成 🧪",
        "",
        " ",
        "\t\n\r\u{B}\u{C}",
        "\u{85}",
        "\u{A0}",
        "\u{3000}",
        "\u{2028}\u{2029}",
        "a\u{0}b",
        "\u{0}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    t.push("a".repeat(MAX_TEXT_BYTES));
    t.push("a".repeat(MAX_TEXT_BYTES + 1));
    t.push("é".repeat(MAX_TEXT_BYTES / 2));
    t.push("é".repeat(MAX_TEXT_BYTES / 2 + 1));
    t.push("🧪".repeat(MAX_TEXT_BYTES / 4));
    t.push("🧪".repeat(MAX_TEXT_BYTES / 4 + 1));
    t.push(format!("{} x", " ".repeat(MAX_TEXT_BYTES - 2)));
    t
}

fn label(s: &str) -> String {
    if s.len() > 40 {
        format!("{} chars, {} bytes", s.chars().count(), s.len())
    } else {
        format!("{s:?}")
    }
}

#[test]
fn both_contracts_lint_clean() {
    assert_eq!(pattern::lint(&prompt_schema()), Vec::<String>::new());
    assert_eq!(pattern::lint(&schema()), Vec::<String>::new());
}

#[test]
fn the_prompt_text_matches_rust_in_both_contracts() {
    for text in texts() {
        let rust = PromptPayload::text(text.clone()).validate().is_ok();
        let value = json!({"text": text, "attachments": []});
        assert_eq!(
            schema_accepts(&prompt_schema(), &value),
            rust,
            "prompt.v1: {}",
            label(&text)
        );
        assert_eq!(
            schema_accepts(&at(schema(), "prompt"), &value),
            rust,
            "new-thread prompt: {}",
            label(&text)
        );
    }
}

/// Every single character, as a whole prompt: the schema's whitespace and
/// NUL rules are Rust's, character for character.
#[test]
fn every_single_character_prompt_matches_rust() {
    let schema = prompt_schema();
    let p = schema
        .pointer("/$defs/prompt/properties/text/pattern")
        .and_then(Value::as_str)
        .expect("a text pattern");
    let p = pattern::Pattern::new(p);
    for c in (0..=0x10FFFF).filter_map(char::from_u32) {
        let text = c.to_string();
        let rust = PromptPayload::text(text.clone()).validate().is_ok();
        assert_eq!(p.is_match(&text), rust, "U+{:04X}", c as u32);
    }
}

#[test]
fn op_ids_match_rust() {
    let long = "a".repeat(64);
    let longer = "a".repeat(65);
    for id in [
        "op-1",
        "A",
        "0-0",
        long.as_str(),
        "",
        "../x",
        "-a",
        "_a",
        "a b",
        "é",
        "a/b",
        longer.as_str(),
    ] {
        assert_eq!(
            schema_accepts(&at(schema(), "op_id"), &json!(id)),
            OpId::parse(id).is_ok(),
            "{id:?}"
        );
    }
}

#[test]
fn attachment_ids_match_rust() {
    let longer = "a".repeat(65);
    for id in [
        "att-syn-0001",
        "A_1",
        "",
        "-a",
        "_a",
        "../x",
        "a b",
        longer.as_str(),
    ] {
        assert_eq!(
            schema_accepts(&at(prompt_schema(), "attachment_id"), &json!(id)),
            AttachmentId::parse(id).is_ok(),
            "{id:?}"
        );
    }
}

#[test]
fn settings_match_rust() {
    let long = "x".repeat(128);
    let longer = "x".repeat(129);
    for value in [
        "codex",
        "syn-codex-model-01",
        "syn-model[1m]",
        "!",
        "~",
        long.as_str(),
        "",
        "--model",
        "-x",
        "a b",
        "é",
        "a\tb",
        "a\u{7F}",
        longer.as_str(),
    ] {
        let mut r = request(offered(), "Synthetic", "op-parity");
        r.settings.model = value.to_owned();
        assert_eq!(
            schema_accepts(&at(schema(), "setting"), &json!(value)),
            r.validate().is_ok(),
            "{value:?}"
        );
    }
}

#[test]
fn project_references_match_rust() {
    for value in [
        "prj_syn_0001",
        "Synthetic shared~000005",
        "Synthétique 🧪",
        "",
        " ",
        "\u{3000}",
        "a\u{0}b",
    ] {
        for (kind, reference) in [
            ("id", ProjectRef::Id(value.to_owned())),
            ("name", ProjectRef::Name(value.to_owned())),
        ] {
            let mut r = request(offered(), "Synthetic", "op-parity");
            r.project = reference;
            assert_eq!(
                schema_accepts(
                    &at(schema(), "project_ref"),
                    &json!({"kind": kind, "value": value})
                ),
                r.validate().is_ok(),
                "{kind} {value:?}"
            );
        }
    }
}

#[test]
fn the_round_1_counterexamples_are_refused() {
    let valid = json!({
        "project": {"kind": "id", "value": "prj_syn_0001"},
        "harness": "codex",
        "model": "syn-codex-model-01",
        "effort": "high",
        "prompt": {"text": "Synthetic", "attachments": []},
        "op_id": "op-1",
    });
    assert!(schema_accepts(&at(schema(), "request"), &valid));
    for (pointer, bad) in [
        ("/prompt/text", json!(" \t\n")),
        ("/prompt/text", json!("a\u{0}b")),
        ("/prompt/text", json!("a".repeat(MAX_TEXT_BYTES + 1))),
        ("/op_id", json!("../x")),
        ("/model", json!("--model")),
        ("/harness", json!("a b")),
        ("/effort", json!("é")),
    ] {
        let mut value = valid.clone();
        *value.pointer_mut(pointer).unwrap() = bad;
        assert!(
            !schema_accepts(&at(schema(), "request"), &value),
            "accepted at {pointer}: {}",
            label(&value.to_string())
        );
    }
}

/// A pattern under a `oneOf` branch is enforced: a status read whose report
/// carries an unsafe op id is refused.
#[test]
fn patterns_under_one_of_are_enforced() {
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let report = create(
        &client,
        &store,
        &request(offered(), "Synthetic", "op-parity-one-of"),
    );
    let mut status = json!({
        "schema": "openwiz.new-thread-status.v1",
        "op_id": "op-parity-one-of",
        "result": {"kind": "recorded", "report": report, "start": null, "observations": null},
    });
    assert!(schema_accepts(&at(schema(), "status_read"), &status));
    status["result"]["report"]["op_id"] = json!("../x");
    assert!(!schema_accepts(&at(schema(), "status_read"), &status));
    status["result"]["report"]["op_id"] = json!("op-parity-one-of");
    status["result"]["report"]["text_sha256"] = json!("Z".repeat(64));
    assert!(!schema_accepts(&at(schema(), "status_read"), &status));
}
