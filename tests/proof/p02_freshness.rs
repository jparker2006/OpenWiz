//! P2 Freshness: the snapshot time, the source and its version, and each
//! row's age.

use openwiz_core::model::{Fact, Timestamp};
use openwiz_core::ops::ListRequest;

use crate::support::*;

#[test]
fn snapshot_time_and_source_version_are_reported() {
    let fake = Fake::new("p02_snapshot_time_and_source_version", "normal");
    let env = fake.list(&ListRequest::default()).expect("normal list");
    assert_eq!(env.freshness.snapshot_at, T0);
    assert_eq!(env.source.name, "room-cli");
    assert_eq!(env.source.version, Fact::Known("v127".to_owned()));
    assert_eq!(env.source.tested_version, "v127");
    assert!(env.source.version_tested);
}

#[test]
fn each_row_has_its_age_from_the_snapshot() {
    let fake = Fake::new("p02_each_row_has_its_age", "normal");
    let env = fake.list(&ListRequest::default()).expect("normal list");
    // The fixture puts thr_syn_000N at N minutes before T0.
    for (i, id) in DEFAULT_IDS.iter().enumerate() {
        let s = session(&env, id);
        let age = 60_000 * (i as i64 + 1);
        assert_eq!(s.updated_at, Fact::Known(Timestamp(T0.0 - age)), "{id}");
        assert_eq!(s.age_ms, Fact::Known(age), "{id}");
    }
}

/// The text header shows the source, its version and the snapshot time
/// (text contract 1 in `support.rs`).
#[test]
fn text_header_shows_the_source_version_and_snapshot_time() {
    assert_eq!(utc_text(T0.0), "2023-11-14T22:23:20Z", "the helper itself");
    let fake = Fake::new("p02_text_header", "normal");
    for (text_args, json_args) in [
        (&["ls"][..], &["ls", "--json"][..]),
        (&["ls", "-l"][..], &["ls", "-l", "--json"][..]),
        (
            &["show", "thr_syn_0001"][..],
            &["show", "thr_syn_0001", "--json"][..],
        ),
    ] {
        let json = fake.cli_json(json_args);
        assert_eq!(json["freshness"]["snapshot_at"], T0.0, "{json_args:?}");
        let (code, out, _) = fake.cli(text_args);
        assert_eq!(code, 0);
        let header = out.lines().next().unwrap_or_default();
        let source = format!(
            "{} {}",
            json["source"]["name"].as_str().expect("source name"),
            fact_text(&json["source"]["version"])
        );
        assert_eq!(source, "room-cli v127");
        let snapshot = format!(
            "snapshot {}",
            utc_text(json["freshness"]["snapshot_at"].as_i64().expect("ms"))
        );
        for wanted in [source.as_str(), snapshot.as_str()] {
            assert!(
                header.contains(wanted),
                "{text_args:?}: the header should contain {wanted:?}: {header}"
            );
        }
    }
}

/// Each row's age in the text (text contract 5), known or unknown, matches
/// the JSON.
#[test]
fn text_rows_show_their_age() {
    for scenario in ["normal", "thread-list-missing-fields"] {
        let fake = Fake::new(&format!("p02_text_row_age_{scenario}"), scenario);
        let json = fake.cli_json(&["ls", "--json"]);
        let (code, text, _) = fake.cli(&["ls"]);
        assert_eq!(code, 0);
        for row in json["data"]["sessions"].as_array().expect("sessions") {
            let id = row["id"].as_str().expect("id");
            let wanted = age_token(row);
            let lines = row_lines(&text, id);
            assert!(
                lines.iter().any(|l| l.contains(&wanted)),
                "{scenario} {id}: the row should show {wanted:?}:\n{text}"
            );
        }
    }
    // The fixture puts thr_syn_000N at N minutes before T0.
    let fake = Fake::new("p02_text_row_age_values", "normal");
    let (_, text, _) = fake.cli(&["ls"]);
    for (i, id) in DEFAULT_IDS.iter().enumerate() {
        let wanted = format!("age {}m", i + 1);
        assert!(
            row_lines(&text, id).iter().any(|l| l.contains(&wanted)),
            "{id}: {wanted:?}:\n{text}"
        );
    }
}

/// An update time so far out that the age cannot be computed gives an unknown age, never a
/// panic or a wrapped value.
#[test]
fn an_out_of_range_update_time_gives_an_unknown_age() {
    let mut records: Vec<_> = (1..=2).map(scripted_record).collect();
    records[0].updated_at = Fact::Known(Timestamp(i64::MIN));
    records[1].updated_at = Fact::Known(Timestamp(i64::MAX));
    let source = ScriptedSource::with_records(records);
    let env = list_scripted(&source, &ListRequest::default()).expect("list");
    unknown_reason(&session(&env, "thr_syn_0001").age_ms);
    assert_eq!(
        session(&env, "thr_syn_0002").age_ms,
        Fact::Known(T0.0 - i64::MAX),
        "an age that fits stays known"
    );
    let (code, text, err) = cli_with(&source, &["ls"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        row_lines(&text, "thr_syn_0001")
            .iter()
            .any(|l| l.contains("age unknown (")),
        "{text}"
    );
}
