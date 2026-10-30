//! P1 Coverage: every session from `thread list` appears exactly once, and
//! the counts reconcile.

use std::collections::BTreeSet;

use openwiz_core::envelope::{Counts, GapKind};
use openwiz_core::ops::ListRequest;

use crate::support::*;

fn reconcile(c: &Counts) {
    assert_eq!(
        c.listed,
        c.unreadable + c.excluded_hidden + c.excluded_archived + c.matched,
        "listed must equal unreadable + excluded + matched: {c:?}"
    );
    let expected_shown = c.limit.min(c.matched.saturating_sub(c.offset));
    assert_eq!(
        c.shown, expected_shown,
        "shown must be min(limit, matched - offset): {c:?}"
    );
}

#[test]
fn every_listed_session_appears_exactly_once() {
    let fake = Fake::new("p01_every_listed_session_appears_exactly_once", "normal");
    let env = fake.list(&ListRequest::default()).expect("normal list");
    let listed = ids(&env);
    let unique: BTreeSet<_> = listed.iter().cloned().collect();
    assert_eq!(
        listed.len(),
        unique.len(),
        "a session appears twice: {listed:?}"
    );
    let expected: BTreeSet<String> = DEFAULT_IDS.iter().map(|s| s.to_string()).collect();
    assert_eq!(unique, expected);
}

#[test]
fn counts_reconcile_for_the_default_list() {
    let fake = Fake::new("p01_counts_reconcile_for_the_default_list", "normal");
    let env = fake.list(&ListRequest::default()).expect("normal list");
    let counts = env.coverage.counts.as_ref().expect("a list has counts");
    assert_eq!(counts.listed, 6);
    assert_eq!(counts.matched, 6);
    assert!(
        !counts.hidden_read && !counts.archived_read,
        "neither was asked for"
    );
    assert_eq!(counts.shown, env.data.sessions.len() as u64);
    reconcile(counts);
}

#[test]
fn counts_reconcile_with_hidden_and_archived_included() {
    let fake = Fake::new("p01_counts_reconcile_with_hidden_and_archived", "normal");
    let request = ListRequest {
        include_hidden: true,
        include_archived: true,
        ..ListRequest::default()
    };
    let env = fake.list(&request).expect("list with -a --hidden");
    let counts = env.coverage.counts.as_ref().expect("a list has counts");
    assert_eq!(counts.listed, 8);
    assert_eq!(counts.matched, 8);
    assert!(counts.hidden_read && counts.archived_read);
    assert_eq!(env.data.sessions.len(), 8);
    reconcile(counts);
}

#[test]
fn unreadable_rows_are_counted_not_dropped() {
    let fake = Fake::new(
        "p01_unreadable_rows_are_counted",
        "thread-list-missing-fields",
    );
    let env = fake
        .list(&ListRequest::default())
        .expect("honest partial list");
    let counts = env.coverage.counts.as_ref().expect("a list has counts");
    assert_eq!(counts.listed, 6, "every row the source returned is counted");
    assert_eq!(
        counts.unreadable, 1,
        "the row without an id is counted as unreadable"
    );
    assert_eq!(env.data.sessions.len(), 5);
    reconcile(counts);
    assert!(
        env.gaps
            .iter()
            .any(|g| g.kind == GapKind::MissingField && g.detail.contains("id")),
        "the unreadable row has a gap: {:?}",
        env.gaps
    );
}

#[test]
fn a_session_listed_twice_appears_once_with_a_gap() {
    let mut records = ScriptedSource::new(3).records;
    records.push(records[0].clone());
    let source = ScriptedSource::with_records(records);
    let env = list_scripted(&source, &ListRequest::default()).expect("list");
    let listed = ids(&env);
    assert_eq!(
        listed.iter().filter(|id| *id == "thr_syn_0001").count(),
        1,
        "{listed:?}"
    );
    assert_eq!(listed.len(), 3);
    reconcile(env.coverage.counts.as_ref().expect("counts"));
    assert!(
        env.gaps
            .iter()
            .any(|g| g.subject.as_deref() == Some("thr_syn_0001")
                || g.detail.contains("thr_syn_0001")),
        "the duplicate has a gap: {:?}",
        env.gaps
    );
}
