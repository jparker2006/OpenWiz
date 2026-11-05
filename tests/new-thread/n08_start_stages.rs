//! N8: start stages for every status row, separate from creation, within
//! the follow bound (sheet the check). Unknowns stay unknown.

use std::time::Duration;

use openwiz_adapter_cloudroom::new_thread::parse_shown_thread;
use openwiz_core::model::Fact;
use openwiz_core::new_thread::start::{
    FOLLOW_MAX_READS, FOLLOW_WINDOW, StartStage, follow_next, start_view,
};
use openwiz_core::source::SourceError;

use crate::support::*;

const REQUESTED: (&str, &str, &str) = ("codex", "syn-codex-model-01", "high");

fn stage(file: &str) -> StartStage {
    let shown = parse_shown_thread(&fixture(file), "thr_syn_0021");
    start_view("thr_syn_0021", REQUESTED, shown, T0).stage
}

#[test]
fn every_status_row_has_its_stage() {
    assert!(matches!(
        stage("thread-show-pending.json"),
        StartStage::WaitingToStart { .. }
    ));
    assert!(matches!(
        stage("thread-show-starting.json"),
        StartStage::WaitingToStart { .. }
    ));
    let StartStage::Started { text } = stage("thread-show-active.json") else {
        panic!()
    };
    assert!(text.contains("as reported"));
    let StartStage::IdleAsReported { text } = stage("thread-show-idle.json") else {
        panic!()
    };
    assert!(text.contains("not confirmed"), "{text}");
    let StartStage::StartFailed { last_error, text } = stage("thread-show-error.json") else {
        panic!()
    };
    assert_eq!(last_error, Fact::Known("Synthetic start error".to_owned()));
    assert!(text.contains("prompt stays saved"), "{text}");
    assert!(matches!(
        stage("thread-show-unknown-status.json"),
        StartStage::Unknown { .. }
    ));
    assert!(matches!(
        stage("thread-show-drift.json"),
        StartStage::Unknown { .. }
    ));
}

#[test]
fn a_failed_read_is_unknown_with_a_gap() {
    let view = start_view(
        "thr_syn_0021",
        REQUESTED,
        Err(SourceError::TimedOut { after_ms: 15_000 }),
        T0,
    );
    let StartStage::Unknown { text, .. } = &view.stage else {
        panic!("{view:?}")
    };
    assert!(text.starts_with("unknown ("), "{text}");
    assert!(view.reported.is_none());
    assert_eq!(view.gaps.len(), 1);
}

#[test]
fn a_long_last_error_is_cut_to_the_field_cap() {
    let mut doc = fixture_json("thread-show-error.json");
    doc["thread"]["lastError"] = serde_json::json!(format!("Synthetic {}", "x".repeat(500)));
    let shown = parse_shown_thread(doc.to_string().as_bytes(), "thr_syn_0021");
    let StartStage::StartFailed {
        last_error: Fact::Known(error),
        ..
    } = start_view("thr_syn_0021", REQUESTED, shown, T0).stage
    else {
        panic!()
    };
    assert_eq!(error.chars().count(), openwiz_core::limits::FIELD_MAX_CHARS);
}

#[test]
fn a_thread_show_of_another_thread_is_not_read() {
    assert!(parse_shown_thread(&fixture("thread-show-active.json"), "thr_syn_0022").is_err());
    assert!(parse_shown_thread(b"Synthetic", "thr_syn_0021").is_err());
}

#[test]
fn following_stops_within_the_bound() {
    let waiting = stage("thread-show-pending.json");
    let mut reads = 1;
    let mut elapsed = Duration::ZERO;
    while let Some(wait) = follow_next(reads, elapsed, &waiting) {
        elapsed += wait;
        reads += 1;
    }
    assert!(reads <= FOLLOW_MAX_READS, "{reads} reads");
    assert!(elapsed <= FOLLOW_WINDOW, "{elapsed:?}");
    assert!(reads >= 2, "a waiting start is followed");
    for settled in [
        "thread-show-active.json",
        "thread-show-idle.json",
        "thread-show-error.json",
    ] {
        assert_eq!(
            follow_next(1, Duration::ZERO, &stage(settled)),
            None,
            "{settled}"
        );
    }
    let unknown = stage("thread-show-unknown-status.json");
    assert_eq!(
        follow_next(FOLLOW_MAX_READS, Duration::ZERO, &unknown),
        None
    );
    assert_eq!(follow_next(1, FOLLOW_WINDOW, &unknown), None);
}
