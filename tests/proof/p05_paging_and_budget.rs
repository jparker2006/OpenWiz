//! P5 Paging and budget: stable ordering, `--limit` and `--offset`,
//! enrichment of the requested page only, at most 4 concurrent detail reads,
//! and a 30-second budget.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use openwiz_core::envelope::{Completeness, GapKind};
use openwiz_core::limits::{MAX_CONCURRENT_DETAIL_READS, RUN_BUDGET};
use openwiz_core::ops::{Detail, ListRequest};

use crate::support::*;

fn page(offset: usize, limit: usize, detail: Detail) -> ListRequest {
    ListRequest {
        detail,
        offset,
        limit,
        ..ListRequest::default()
    }
}

#[test]
fn ordering_is_stable_whatever_order_the_source_returns() {
    let forward = ScriptedSource::new(12);
    let mut reversed_records = forward.records.clone();
    reversed_records.reverse();
    let reversed = ScriptedSource::with_records(reversed_records);
    let request = ListRequest::default();
    let a = ids(&list_scripted(&forward, &request).expect("list"));
    let b = ids(&list_scripted(&reversed, &request).expect("list"));
    let again = ids(&list_scripted(&forward, &request).expect("list"));
    assert_eq!(a, b, "the order must not depend on the source's row order");
    assert_eq!(a, again, "the order must be the same on every run");
}

#[test]
fn limit_and_offset_select_the_page() {
    let source = ScriptedSource::new(25);
    let env = list_scripted(&source, &page(20, 10, Detail::Short)).expect("list");
    assert_eq!(env.data.sessions.len(), 5);
    let c = env.coverage.counts.as_ref().expect("counts");
    assert_eq!((c.matched, c.offset, c.limit, c.shown), (25, 20, 10, 5));

    let past_the_end = list_scripted(&source, &page(40, 10, Detail::Short)).expect("list");
    assert!(past_the_end.data.sessions.is_empty());
    assert_eq!(
        past_the_end.coverage.counts.as_ref().expect("counts").shown,
        0
    );
}

#[test]
fn pages_cover_every_session_once() {
    let source = ScriptedSource::new(25);
    let all = ids(&list_scripted(&source, &page(0, 100, Detail::Short)).expect("list"));
    let mut paged = Vec::new();
    for offset in (0..25).step_by(7) {
        paged.extend(ids(&list_scripted(
            &source,
            &page(offset, 7, Detail::Short),
        )
        .expect("list")));
    }
    assert_eq!(
        paged, all,
        "consecutive pages must join up with no gap or repeat"
    );
}

#[test]
fn short_listing_makes_no_detail_reads() {
    let source = ScriptedSource::new(10);
    list_scripted(&source, &page(0, 10, Detail::Short)).expect("list");
    assert!(source.detail_ids().is_empty(), "ls must not read details");
}

#[test]
fn long_listing_enriches_only_the_requested_page() {
    let source = ScriptedSource::new(25);
    let env = list_scripted(&source, &page(10, 5, Detail::Long)).expect("list");
    let shown: BTreeSet<String> = ids(&env).into_iter().collect();
    assert_eq!(shown.len(), 5);
    assert_eq!(
        source.detail_ids(),
        shown,
        "detail reads must be for the page only"
    );

    let fake = Fake::new("p05_long_listing_enriches_only_the_page", "normal");
    let env = fake
        .list(&page(0, 2, Detail::Long))
        .expect("ls -l --limit 2");
    let shown: BTreeSet<String> = ids(&env).into_iter().collect();
    let shown_ids: BTreeSet<String> = fake
        .invocations()
        .into_iter()
        .filter(|argv| argv.len() > 2 && argv[0] == "thread" && argv[1] == "show")
        .map(|argv| argv[2].clone())
        .collect();
    assert_eq!(shown_ids, shown, "room-cli thread show only for the page");
}

#[test]
fn at_most_four_detail_reads_run_at_once() {
    let source = ScriptedSource::new(20).with_delay(Duration::from_millis(100));
    list_scripted(&source, &page(0, 20, Detail::Long)).expect("list");
    let max = source.max_in_flight.load(Ordering::SeqCst);
    assert!(max >= 1, "details were read");
    assert_eq!(MAX_CONCURRENT_DETAIL_READS, 4, "the approved limit");
    assert!(max <= 4, "{max} detail reads ran at once");
}

#[test]
fn the_budget_bounds_the_whole_run() {
    let source = ScriptedSource::new(20).with_delay(Duration::from_secs(1));
    let budget = Duration::from_millis(1500);
    let started = Instant::now();
    let env = list_scripted(
        &source,
        &ListRequest {
            budget,
            ..page(0, 20, Detail::Long)
        },
    )
    .expect("honest partial list");
    assert!(
        started.elapsed() < budget + Duration::from_secs(1),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(env.coverage.state, Completeness::Partial);
    assert_eq!(
        env.data.sessions.len(),
        20,
        "unenriched rows are still listed"
    );
    assert!(
        env.gaps
            .iter()
            .any(|g| g.kind == GapKind::BudgetExhausted || g.kind == GapKind::TimedOut),
        "{:?}",
        env.gaps
    );
    let unenriched = env
        .data
        .sessions
        .iter()
        .filter(|s| {
            matches!(
                s.reported_configuration.model,
                openwiz_core::model::Fact::Unknown(_)
            )
        })
        .count();
    assert!(
        unenriched > 0,
        "rows not read in time have unknown configuration"
    );
}

#[test]
fn the_default_budget_is_thirty_seconds() {
    assert_eq!(RUN_BUDGET, Duration::from_secs(30), "the approved budget");
    let source = ScriptedSource::new(3);
    let started = Instant::now();
    list_scripted(&source, &page(0, 3, Detail::Long)).expect("list");
    let finished = Instant::now();
    let deadlines = source.deadlines.lock().unwrap().clone();
    assert!(!deadlines.is_empty(), "every read gets a deadline");
    for deadline in deadlines {
        // The run's deadline is set between the start and the end of the call.
        assert!(
            deadline >= started + RUN_BUDGET && deadline <= finished + RUN_BUDGET,
            "a read's deadline is {:?} after the start, not the 30-second run budget",
            deadline.duration_since(started)
        );
    }
}

#[test]
fn the_cli_passes_limit_and_offset() {
    let fake = Fake::new("p05_cli_limit_and_offset", "normal");
    let all = fake.cli_json(&["ls", "--json"]);
    let all: Vec<&str> = all["data"]["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|s| s["id"].as_str().expect("id"))
        .collect();
    let page = fake.cli_json(&["ls", "--limit", "2", "--offset", "3", "--json"]);
    let shown: Vec<&str> = page["data"]["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|s| s["id"].as_str().expect("id"))
        .collect();
    assert_eq!(shown, all[3..5]);
    assert_eq!(page["coverage"]["counts"]["offset"], 3);
    assert_eq!(page["coverage"]["counts"]["limit"], 2);
    let (code, text, _) = fake.cli(&["ls", "--limit", "2", "--offset", "3"]);
    assert_eq!(code, 0);
    for id in DEFAULT_IDS {
        assert_eq!(
            !row_lines(&text, id).is_empty(),
            shown.contains(&id),
            "{id}:\n{text}"
        );
    }
}

/// `--limit`
/// defaults to 100. With more than 100 sessions and no limit given, exactly
/// the first 100 in the stable order are shown, whatever the source order.
#[test]
fn the_default_page_is_the_first_100_in_stable_order() {
    let source = ScriptedSource::new(150);
    let env = list_scripted(&source, &ListRequest::default()).expect("list");
    let c = env.coverage.counts.as_ref().expect("counts");
    assert_eq!((c.matched, c.offset, c.limit, c.shown), (150, 0, 100, 100));
    let default_page = ids(&env);
    assert_eq!(default_page.len(), 100);

    let everything = ids(&list_scripted(&source, &page(0, 150, Detail::Short)).expect("list"));
    assert_eq!(
        default_page,
        everything[..100],
        "the default page is the first 100 in the stable order"
    );

    let mut reversed_records = source.records.clone();
    reversed_records.reverse();
    let reversed = ScriptedSource::with_records(reversed_records);
    assert_eq!(
        ids(&list_scripted(&reversed, &ListRequest::default()).expect("list")),
        default_page,
        "the source's row order does not change the default page"
    );

    // The same through `openwiz ls` with no --limit.
    let json = cli_json_with(&source, &["ls", "--json"]);
    let shown: Vec<&str> = json["data"]["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|s| s["id"].as_str().expect("id"))
        .collect();
    assert_eq!(shown, default_page);
    assert_eq!(json["coverage"]["counts"]["limit"], 100);
    let (code, text, _) = cli_with(&source, &["ls"]);
    assert_eq!(code, 0);
    let rows: Vec<String> = text.lines().filter_map(row_id).collect();
    assert_eq!(rows.len(), 100, "ls prints 100 rows:\n{text}");
    assert_eq!(
        rows.iter().collect::<BTreeSet<_>>(),
        default_page.iter().collect::<BTreeSet<_>>()
    );
}

/// The sort key, documented in `openwiz --help`: most
/// recently updated first; ties by session id; sessions whose update time is
/// unknown last, by session id. The same through `openwiz ls --json` and in
/// the text rows.
#[test]
fn the_order_is_most_recently_updated_first_then_by_id() {
    use openwiz_core::model::{Fact, Timestamp};
    let updated = [
        Fact::Known(Timestamp(T0.0 - 5_000)),
        Fact::Unknown("synthetic unknown time".to_owned()),
        Fact::Known(Timestamp(T0.0 - 1_000)),
        Fact::Known(Timestamp(T0.0 - 5_000)),
        Fact::Unknown("synthetic unknown time".to_owned()),
        Fact::Known(T0),
    ];
    let records: Vec<_> = updated
        .iter()
        .enumerate()
        .map(|(i, at)| {
            let mut r = scripted_record(i + 1);
            r.updated_at = at.clone();
            r
        })
        .collect();
    let expected = [
        "thr_syn_0006",
        "thr_syn_0003",
        "thr_syn_0001",
        "thr_syn_0004",
        "thr_syn_0002",
        "thr_syn_0005",
    ];
    let mut shuffled = records.clone();
    shuffled.rotate_left(2);
    shuffled.swap(0, 3);
    for (label, rows) in [("as listed", records), ("shuffled", shuffled)] {
        let source = ScriptedSource::with_records(rows);
        let env = list_scripted(&source, &ListRequest::default()).expect("list");
        assert_eq!(ids(&env), expected, "{label}");

        let json = cli_json_with(&source, &["ls", "--json"]);
        let shown: Vec<&str> = json["data"]["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .map(|s| s["id"].as_str().expect("id"))
            .collect();
        assert_eq!(shown, expected, "{label}: ls --json");

        let (code, text, _) = cli_with(&source, &["ls"]);
        assert_eq!(code, 0);
        let rows: Vec<String> = text.lines().filter_map(row_id).collect();
        assert_eq!(
            rows, expected,
            "{label}: one group, so the text keeps the order:\n{text}"
        );
    }
}
