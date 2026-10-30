//! P9 Reported configuration: model and effort are always labelled as
//! reported, never as the serving model.

use openwiz_core::model::{Fact, ReportedConfiguration};

use crate::support::*;

#[test]
fn model_and_effort_come_from_thread_show_as_reported_configuration() {
    let fake = Fake::new("p09_reported_configuration", "normal");
    let env = fake.list(&long()).expect("ls -l");
    assert_eq!(
        session(&env, "thr_syn_0002").reported_configuration,
        ReportedConfiguration {
            model: Fact::Known("synthetic-model-b".to_owned()),
            effort: Fact::Known("high".to_owned()),
        }
    );
}

#[test]
fn json_keeps_model_and_effort_under_reported_configuration() {
    let fake = Fake::new("p09_json_label", "normal");
    for args in [
        &["ls", "-l", "--json"][..],
        &["show", "thr_syn_0001", "--json"][..],
    ] {
        let json = fake.cli_json(args);
        let sessions: Vec<&serde_json::Value> = match json["data"]["sessions"].as_array() {
            Some(rows) => rows.iter().collect(),
            None => vec![&json["data"]["session"]],
        };
        assert!(!sessions.is_empty());
        for s in sessions {
            for key in ["model", "effort", "reasoning_level", "serving_model"] {
                assert!(
                    s.get(key).is_none(),
                    "{args:?}: {key} outside reported_configuration"
                );
            }
            let config = &s["reported_configuration"];
            assert!(
                config.get("model").is_some() && config.get("effort").is_some(),
                "{args:?}: {s}"
            );
        }
    }
}

/// The label is the
/// configuration's own, tied to both values (text contract 6 in
/// `support.rs`), in `ls -l` and `show`, not just "reported" somewhere.
#[test]
fn text_labels_model_and_effort_as_reported_configuration() {
    for scenario in ["normal", "show-missing-fields"] {
        let fake = Fake::new(&format!("p09_text_label_{scenario}"), scenario);
        let json = fake.cli_json(&["ls", "-l", "--json"]);
        let (code, text, _) = fake.cli(&["ls", "-l"]);
        assert_eq!(code, 0);
        for row in json["data"]["sessions"].as_array().expect("sessions") {
            let id = row["id"].as_str().expect("id");
            let wanted = configuration_text(row);
            assert!(
                row_lines(&text, id).iter().any(|l| l.contains(&wanted)),
                "{scenario} {id}: the row should show {wanted:?}:\n{text}"
            );
        }
        for line in text.lines().filter(|l| l.contains("synthetic-model-")) {
            assert!(
                line.contains("reported configuration: model synthetic-model-"),
                "{scenario}: a model is shown outside its label: {line}"
            );
        }
        assert!(!text.to_lowercase().contains("serving model"), "{text}");

        for id in ["thr_syn_0001", "thr_syn_0006"] {
            let json = fake.cli_json(&["show", id, "--json"]);
            let (code, text, _) = fake.cli(&["show", id]);
            assert_eq!(code, 0);
            let wanted = configuration_text(&json["data"]["session"]);
            assert!(
                text.lines().any(|l| l.contains(&wanted)),
                "{scenario} show {id} should show {wanted:?}:\n{text}"
            );
            assert!(!text.to_lowercase().contains("serving model"), "{text}");
        }
    }
}
