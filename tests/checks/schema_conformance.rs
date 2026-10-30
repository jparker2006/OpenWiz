//! Every fixture envelope, and every envelope the core types can produce,
//! matches the schema's required fields and types.

use std::fs;
use std::path::Path;

use openwiz_core::envelope::{
    Completeness, Counts, Coverage, Envelope, Freshness, Gap, GapKind, SCHEMA_ID, SourceInfo,
};
use openwiz_core::limits::PURPOSE_MAX_CHARS;
use openwiz_core::model::{
    BoundedText, Fact, FinalOutput, Grouping, Inventory, Location, ProjectId, Recap,
    ReportedConfiguration, Session, SessionId, SessionView, Timestamp,
};
use serde_json::{Value, json};

use crate::schema::{lint, load_schema, validate};

const ENVELOPES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/envelopes");

fn envelope_fixtures() -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = fs::read_dir(ENVELOPES)
        .expect("envelopes dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let text = fs::read_to_string(&p).expect("fixture");
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, serde_json::from_str(&text).expect("fixture is JSON"))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn the_whole_schema_uses_only_checked_keywords() {
    let schema = load_schema();
    assert_eq!(schema["title"], SCHEMA_ID);
    assert_eq!(lint(&schema), Vec::<String>::new());
    // Every $ref resolves: an empty envelope fails on fields, not on the schema.
    let errors = validate(&schema, &json!({}));
    assert!(!errors.is_empty());
    assert!(
        errors.iter().all(|e| !e.contains("unresolvable")),
        "{errors:#?}"
    );
}

#[test]
fn the_lint_finds_rules_the_checker_would_ignore() {
    let mut unreferenced = load_schema();
    unreferenced["$defs"]["bogus"] = json!({ "type": "string", "pattern": "^x$" });
    assert!(
        !lint(&unreferenced).is_empty(),
        "a keyword in an unreferenced $defs entry"
    );

    let mut dangling = load_schema();
    dangling["$defs"]["orphan"] = json!({ "$ref": "#/$defs/does_not_exist" });
    assert!(
        !lint(&dangling).is_empty(),
        "an unresolvable $ref nothing reaches"
    );

    let mut open = load_schema();
    open["$defs"]["freshness"]["additionalProperties"] = json!({ "type": "string" });
    assert!(!lint(&open).is_empty(), "additionalProperties as a schema");
}

#[test]
fn the_schema_limits_match_the_core_limits() {
    let schema = load_schema();
    let recap_text = &schema["$defs"]["recap"]["oneOf"][0]["properties"]["text"];
    assert_eq!(recap_text["maxLength"], PURPOSE_MAX_CHARS as u64);
    assert_eq!(PURPOSE_MAX_CHARS, 120, "the approved recap length");
}

#[test]
fn every_envelope_fixture_matches_the_schema() {
    let schema = load_schema();
    let fixtures = envelope_fixtures();
    assert!(
        fixtures.len() >= 4,
        "list and show envelopes, complete and partial"
    );
    for (name, value) in fixtures {
        let errors = validate(&schema, &value);
        assert!(errors.is_empty(), "{name}: {errors:#?}");
    }
}

#[test]
fn the_checker_rejects_missing_and_mistyped_fields() {
    let schema = load_schema();
    let (_, good) = envelope_fixtures()
        .into_iter()
        .find(|(n, _)| n == "ls-long-complete.json")
        .expect("ls-long-complete.json");
    type Mutation = (&'static str, fn(&mut Value));
    let mutations: [Mutation; 8] = [
        ("missing coverage", |v| {
            v.as_object_mut().unwrap().remove("coverage");
        }),
        ("wrong schema id", |v| {
            v["schema"] = json!("openwiz.inventory.v2")
        }),
        ("status not a fact", |v| {
            v["data"]["sessions"][0]["status"] = json!("idle")
        }),
        ("age as a string", |v| {
            v["data"]["sessions"][0]["age_ms"] = json!({"known": "60s"})
        }),
        ("an empty unknown reason", |v| {
            v["data"]["sessions"][0]["title"] = json!({"unknown": ""})
        }),
        ("unknown without reason", |v| {
            v["data"]["sessions"][0]["title"] = json!({"unknown": null})
        }),
        ("an unexpected field on a session", |v| {
            v["data"]["sessions"][0]["model"] = json!({"known": "synthetic-model-a"})
        }),
        ("recap over 120 characters", |v| {
            v["data"]["sessions"][0]["recap"]["known"]["text"] = json!("Synthetic ".repeat(13))
        }),
    ];
    for (what, mutate) in mutations {
        let mut bad = good.clone();
        mutate(&mut bad);
        assert!(
            !validate(&schema, &bad).is_empty(),
            "the checker accepted: {what}"
        );
    }
}

fn sample_session(n: u32) -> Session {
    Session {
        id: SessionId(format!("thr_syn_{n:04}")),
        title: Fact::Known(format!("Synthetic session {n}")),
        provider: Fact::Known("codex".to_owned()),
        status: Fact::Unknown("not reported by room-cli: status is missing".to_owned()),
        location: Fact::Known(Location::Local {
            machine: Fact::Unknown("host is not in the machine list".to_owned()),
        }),
        group: Grouping::Project {
            id: ProjectId("prj_syn_0001".to_owned()),
            name: Fact::Known("Synthetic project one".to_owned()),
        },
        parent: Fact::Known(Some(SessionId("thr_syn_0001".to_owned()))),
        hidden: Fact::Known(false),
        archived: Fact::Unknown("archivedAt has the wrong type".to_owned()),
        recap: Fact::Known(Recap::PurposeAsReported(BoundedText {
            text: "Synthetic purpose.".to_owned(),
            truncated: false,
        })),
        reported_configuration: ReportedConfiguration {
            model: Fact::Known("synthetic-model-a".to_owned()),
            effort: Fact::Unknown("not read: ls without -l".to_owned()),
        },
        updated_at: Fact::Known(Timestamp(1_700_000_000_000)),
        age_ms: Fact::Known(60_000),
    }
}

/// Every gap kind, listed by hand. Add a new kind here.
const EVERY_GAP_KIND: [GapKind; 8] = [
    GapKind::UntestedVersion,
    GapKind::CommandFailed,
    GapKind::TimedOut,
    GapKind::BudgetExhausted,
    GapKind::Malformed,
    GapKind::MissingField,
    GapKind::WrongType,
    GapKind::NotFound,
];

fn sample_envelope<T>(data: T, counts: Option<Counts>) -> Envelope<T> {
    Envelope {
        schema: SCHEMA_ID.to_owned(),
        source: SourceInfo {
            name: "room-cli".to_owned(),
            version: Fact::Unknown("room-cli --version timed out".to_owned()),
            tested_version: "v127".to_owned(),
            version_tested: false,
        },
        coverage: Coverage {
            state: Completeness::Partial,
            counts,
        },
        freshness: Freshness {
            snapshot_at: Timestamp(1_700_000_600_000),
        },
        gaps: EVERY_GAP_KIND
            .iter()
            .map(|&kind| Gap {
                kind,
                subject: Some("thr_syn_0001".to_owned()),
                detail: "Synthetic gap".to_owned(),
            })
            .collect(),
        data,
    }
}

#[test]
fn the_core_types_serialize_to_schema_valid_json() {
    let schema = load_schema();
    let mut no_recap = sample_session(2);
    no_recap.recap = Fact::Known(Recap::NoRecapYet);
    no_recap.location = Fact::Known(Location::Cloud);
    no_recap.group = Grouping::Ungrouped {
        reason: "Synthetic reason".to_owned(),
    };
    no_recap.parent = Fact::Known(None);
    let list = sample_envelope(
        Inventory {
            sessions: vec![sample_session(1), no_recap.clone()],
        },
        Some(Counts {
            listed: 2,
            unreadable: 0,
            excluded_hidden: 0,
            excluded_archived: 0,
            hidden_read: true,
            archived_read: false,
            matched: 2,
            offset: 0,
            limit: 50,
            shown: 2,
        }),
    );
    let shows = [
        SessionView {
            session: sample_session(1),
            final_output: Fact::Known(FinalOutput::Reported(BoundedText {
                text: "Synthetic output".to_owned(),
                truncated: true,
            })),
            reported_error: Fact::Known(Some(BoundedText {
                text: "Synthetic error".to_owned(),
                truncated: false,
            })),
        },
        SessionView {
            session: no_recap,
            final_output: Fact::Known(FinalOutput::NoFinalOutputYet),
            reported_error: Fact::Known(None),
        },
        SessionView {
            session: sample_session(3),
            final_output: Fact::Unknown("thread output failed".to_owned()),
            reported_error: Fact::Unknown("thread output failed".to_owned()),
        },
    ];
    let mut complete = list.clone();
    complete.coverage.state = Completeness::Complete;
    complete.gaps.clear();
    let mut values = vec![
        serde_json::to_value(&list).unwrap(),
        serde_json::to_value(&complete).unwrap(),
    ];
    for show in shows {
        values.push(serde_json::to_value(sample_envelope(show, None)).unwrap());
    }
    for value in values {
        let errors = validate(&schema, &value);
        assert!(errors.is_empty(), "{errors:#?}\n{value:#}");
    }
    assert!(Path::new(crate::schema::SCHEMA_PATH).is_file());
}
