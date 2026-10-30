//! P3 Unknowns: a missing status, model, parent or freshness prints as
//! `unknown (reason)`.

use openwiz_core::model::Fact;
use openwiz_core::ops::ListRequest;

use crate::support::*;

#[test]
fn missing_status_is_unknown_with_a_reason() {
    let fake = Fake::new("p03_missing_status", "thread-list-missing-fields");
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    unknown_reason(&session(&env, "thr_syn_0002").status);
}

#[test]
fn missing_model_is_unknown_with_a_reason() {
    let fake = Fake::new("p03_missing_model", "show-missing-fields");
    let env = fake.list(&long()).expect("ls -l");
    let config = &session(&env, "thr_syn_0001").reported_configuration;
    unknown_reason(&config.model);
    assert_eq!(config.effort, Fact::Known("high".to_owned()));
    let effortless = &session(&env, "thr_syn_0006").reported_configuration;
    unknown_reason(&effortless.effort);
}

#[test]
fn missing_parent_is_unknown_not_top_level() {
    let fake = Fake::new("p03_missing_parent", "thread-list-missing-fields");
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    unknown_reason(&session(&env, "thr_syn_0003").parent);
    // A null parent is a known top-level session, not an unknown.
    let normal = Fake::new("p03_missing_parent_normal", "normal");
    let env = normal.list(&ListRequest::default()).expect("normal list");
    assert_eq!(session(&env, "thr_syn_0001").parent, Fact::Known(None));
}

#[test]
fn missing_freshness_is_unknown_with_a_reason() {
    let fake = Fake::new("p03_missing_freshness", "thread-list-missing-fields");
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    let s = session(&env, "thr_syn_0004");
    unknown_reason(&s.updated_at);
    unknown_reason(&s.age_ms);
}

/// Finds `id`'s row in `ls --json` output.
fn json_row<'a>(json: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    json["data"]["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("{id} missing from JSON"))
}

/// The `unknown (reason)` text for an unknown at `pointer` in `value`.
fn unknown_text(value: &serde_json::Value, pointer: &str) -> String {
    let reason = value
        .pointer(pointer)
        .and_then(|v| v["unknown"].as_str())
        .unwrap_or_else(|| panic!("{pointer} should be unknown in JSON: {value}"));
    format!("unknown ({reason})")
}

#[test]
fn unknowns_print_as_unknown_with_their_reason_in_ls() {
    let fake = Fake::new("p03_unknowns_print_in_ls", "thread-list-missing-fields");
    let json = fake.cli_json(&["ls", "-l", "--json"]);
    let (code, text, _) = fake.cli(&["ls", "-l"]);
    assert_eq!(code, 0);
    for (id, pointer) in [("thr_syn_0002", "/status"), ("thr_syn_0004", "/age_ms")] {
        let wanted = unknown_text(json_row(&json, id), pointer);
        assert!(
            row_lines(&text, id).iter().any(|l| l.contains(&wanted)),
            "{id}: ls -l should print {wanted:?}:\n{text}"
        );
    }

    let fake = Fake::new("p03_unknown_model_prints_in_ls", "show-missing-fields");
    let json = fake.cli_json(&["ls", "-l", "--json"]);
    let (code, text, _) = fake.cli(&["ls", "-l"]);
    assert_eq!(code, 0);
    let wanted = unknown_text(
        json_row(&json, "thr_syn_0001"),
        "/reported_configuration/model",
    );
    assert!(
        row_lines(&text, "thr_syn_0001")
            .iter()
            .any(|l| l.contains(&wanted)),
        "ls -l should print {wanted:?}:\n{text}"
    );
}

#[test]
fn unknowns_print_as_unknown_with_their_reason_in_show() {
    let fake = Fake::new("p03_unknowns_print_in_show", "show-missing-fields");
    let json = fake.cli_json(&["show", "thr_syn_0001", "--json"]);
    let (code, text, _) = fake.cli(&["show", "thr_syn_0001"]);
    assert_eq!(code, 0);
    let session = &json["data"]["session"];
    for pointer in ["/parent", "/reported_configuration/model"] {
        let wanted = unknown_text(session, pointer);
        assert!(
            text.contains(&wanted),
            "show should print {wanted:?}:\n{text}"
        );
    }
}
