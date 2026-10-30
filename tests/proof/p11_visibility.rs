//! P11 Visibility: hidden sessions are excluded unless `--hidden` is given;
//! archived sessions unless `-a` is given.

use openwiz_core::model::Fact;
use openwiz_core::ops::ListRequest;

use crate::support::*;

fn thread_list_calls(fake: &Fake) -> Vec<Vec<String>> {
    fake.invocations()
        .into_iter()
        .filter(|a| a.len() >= 2 && a[0] == "thread" && a[1] == "list")
        .collect()
}

#[test]
fn hidden_and_archived_sessions_are_excluded_by_default() {
    let fake = Fake::new("p11_default", "normal");
    let (code, text, _) = fake.cli(&["ls"]);
    assert_eq!(code, 0);
    assert!(
        !text.contains(HIDDEN_ID) && !text.contains(ARCHIVED_ID),
        "{text}"
    );
    for argv in thread_list_calls(&fake) {
        assert_eq!(argv, ["thread", "list", "--json"], "flags only on request");
    }
}

#[test]
fn hidden_sessions_appear_only_with_hidden() {
    let fake = Fake::new("p11_hidden", "normal");
    let (code, text, _) = fake.cli(&["ls", "--hidden"]);
    assert_eq!(code, 0);
    assert!(text.contains(HIDDEN_ID), "{text}");
    assert!(!text.contains(ARCHIVED_ID), "{text}");
    assert!(
        thread_list_calls(&fake)
            .iter()
            .all(|a| a.iter().any(|w| w == "--include-hidden")
                && !a.iter().any(|w| w == "--include-archived")),
        "{:?}",
        fake.invocations()
    );
}

#[test]
fn archived_sessions_appear_only_with_a() {
    let fake = Fake::new("p11_archived", "normal");
    let (code, text, _) = fake.cli(&["ls", "-a"]);
    assert_eq!(code, 0);
    assert!(text.contains(ARCHIVED_ID), "{text}");
    assert!(!text.contains(HIDDEN_ID), "{text}");
    assert!(
        thread_list_calls(&fake)
            .iter()
            .all(|a| a.iter().any(|w| w == "--include-archived")
                && !a.iter().any(|w| w == "--include-hidden")),
        "{:?}",
        fake.invocations()
    );
}

#[test]
fn hidden_or_archived_rows_are_excluded_even_if_the_source_returns_them() {
    let mut hidden = scripted_record(1);
    hidden.hidden = Fact::Known(true);
    let mut archived = scripted_record(2);
    archived.archived = Fact::Known(true);
    let visible = scripted_record(3);
    let source = ScriptedSource::with_records(vec![hidden, archived, visible]);
    let env = list_scripted(&source, &ListRequest::default()).expect("list");
    assert_eq!(ids(&env), ["thr_syn_0003"]);
    let c = env.coverage.counts.as_ref().expect("counts");
    assert_eq!(
        (c.listed, c.excluded_hidden, c.excluded_archived, c.matched),
        (3, 1, 1, 1)
    );
}
