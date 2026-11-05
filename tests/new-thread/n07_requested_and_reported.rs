//! N7: requested and reported stay separate. A reported harness, model or
//! effort that differs from the request is shown as differing; the reported
//! configuration is never shown as the serving model.

use openwiz_adapter_cloudroom::new_thread::parse_shown_thread;
use openwiz_core::model::Fact;
use openwiz_core::new_thread::start::{REPORTED_LABEL, start_view};

use crate::support::*;

const REQUESTED: (&str, &str, &str) = ("codex", "syn-codex-model-01", "high");

#[test]
fn a_differing_report_is_shown_as_differing() {
    let shown = parse_shown_thread(&fixture("thread-show-differs.json"), "thr_syn_0021");
    let view = start_view("thr_syn_0021", REQUESTED, shown, T0);
    let reported = view.reported.expect("reported");
    assert_eq!(reported.label, REPORTED_LABEL);
    for (setting, requested, value) in [
        (&reported.provider, "codex", "claude-code"),
        (&reported.model, "syn-codex-model-01", "syn-codex-model-02"),
        (&reported.effort, "high", "max"),
    ] {
        assert_eq!(setting.requested, requested);
        assert_eq!(setting.reported, Fact::Known(value.to_owned()));
        assert_eq!(setting.differs_from_requested, Fact::Known(true));
        assert!(
            setting.text.contains("differs from requested"),
            "{}",
            setting.text
        );
    }
}

#[test]
fn a_matching_report_is_shown_as_requested() {
    let shown = parse_shown_thread(&fixture("thread-show-active.json"), "thr_syn_0021");
    let reported = start_view("thr_syn_0021", REQUESTED, shown, T0)
        .reported
        .unwrap();
    for setting in [&reported.provider, &reported.model, &reported.effort] {
        assert_eq!(setting.differs_from_requested, Fact::Known(false));
        assert!(setting.text.ends_with("as requested"), "{}", setting.text);
    }
}

#[test]
fn an_unread_report_is_never_called_as_requested() {
    let shown = parse_shown_thread(&fixture("thread-show-drift.json"), "thr_syn_0021");
    let view = start_view("thr_syn_0021", REQUESTED, shown, T0);
    let reported = view.reported.unwrap();
    for setting in [&reported.model, &reported.effort] {
        assert!(matches!(setting.reported, Fact::Unknown(_)));
        assert!(matches!(setting.differs_from_requested, Fact::Unknown(_)));
        assert!(!setting.text.contains("as requested"), "{}", setting.text);
    }
    assert!(!view.gaps.is_empty(), "the drift is a gap");
}

#[test]
fn reported_is_never_the_serving_model() {
    let shown = parse_shown_thread(&fixture("thread-show-differs.json"), "thr_syn_0021");
    let view = start_view("thr_syn_0021", REQUESTED, shown, T0);
    let json = serde_json::to_value(&view).unwrap();
    assert!(validate_def("start_view", &json).is_empty());
    let mut words = Vec::new();
    strings(&json, &mut words);
    for w in words {
        if w.contains("serving") {
            assert_eq!(
                w, REPORTED_LABEL,
                "only the label names serving, to deny it"
            );
        }
    }
}

#[test]
fn the_report_keeps_the_request_as_it_was_asked() {
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let r = request(offered(), "Synthetic", "op-n07");
    let report = create(&client, &store, &r);
    assert_eq!(report.requested, offered());
    assert_eq!(report.provider.as_deref(), Some("codex"));
    let json = serde_json::to_value(&report).unwrap();
    assert!(
        json.get("reported").is_none(),
        "a create reports no configuration"
    );
}
