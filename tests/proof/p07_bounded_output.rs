//! P7 Bounded output: purpose at 120 characters, final output at 4 KiB, row
//! and field caps.

use openwiz_core::envelope::{Gap, GapKind};
use openwiz_core::limits::{
    FIELD_MAX_CHARS, FINAL_OUTPUT_MAX_BYTES, MAX_LIMIT, PURPOSE_MAX_CHARS, RUN_BUDGET,
};
use openwiz_core::model::{
    BoundedText, Fact, FinalOutput, Grouping, Location, ProjectId, Recap, ReportedConfiguration,
    SessionId,
};
use openwiz_core::ops::{self, ListRequest, OpError, ShowRequest};
use openwiz_core::source::{MachineRecord, OutputRecord, Placement, ProjectRecord, SourceError};
use serde_json::Value;

use crate::support::*;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/room-cli-0.43.1/normal"
);

fn fixture_json(name: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(format!("{FIXTURES}/{name}")).expect("fixture");
    serde_json::from_str(&text).expect("fixture JSON")
}

#[test]
fn purpose_is_cut_to_120_characters() {
    let fake = Fake::new("p07_purpose_cut", "normal");
    let env = fake.list(&ListRequest::default()).expect("normal list");
    let rows = fixture_json("thread-list.json");
    let original = rows[1]["purpose"]["text"].as_str().expect("purpose");
    assert_eq!(PURPOSE_MAX_CHARS, 120, "the approved recap length");
    assert!(original.chars().count() > PURPOSE_MAX_CHARS);
    let expected: String = original.chars().take(PURPOSE_MAX_CHARS).collect();
    assert_eq!(
        session(&env, "thr_syn_0002").recap,
        Fact::Known(Recap::PurposeAsReported(BoundedText {
            text: expected,
            truncated: true,
        }))
    );
    assert_eq!(
        session(&env, "thr_syn_0001").recap,
        Fact::Known(Recap::PurposeAsReported(BoundedText {
            text: "Synthetic purpose 1.".to_owned(),
            truncated: false,
        }))
    );
}

#[test]
fn a_session_without_purpose_reads_no_recap_yet() {
    let fake = Fake::new("p07_no_recap_yet", "normal");
    let env = fake
        .list(&ListRequest {
            include_archived: true,
            ..long()
        })
        .expect("ls -l -a");
    assert_eq!(
        session(&env, ARCHIVED_ID).recap,
        Fact::Known(Recap::NoRecapYet)
    );
    let (code, text, _) = fake.cli(&["ls", "-l", "-a"]);
    assert_eq!(code, 0);
    assert!(
        row_lines(&text, ARCHIVED_ID)
            .iter()
            .any(|l| l.contains("no recap yet")),
        "{text}"
    );
    assert!(
        row_lines(&text, "thr_syn_0001")
            .iter()
            .any(|l| l.contains("purpose, as reported")),
        "the recap is labelled:\n{text}"
    );
}

#[test]
fn final_output_is_cut_to_4_kib_on_a_character_boundary() {
    let fake = Fake::new("p07_final_output_cut", "normal");
    let env = fake.show("thr_syn_0002").expect("show");
    let original = fixture_json("thread-output-0002.json")["output"]
        .as_str()
        .expect("output")
        .to_owned();
    assert_eq!(FINAL_OUTPUT_MAX_BYTES, 4096, "the approved output size");
    assert!(original.len() > FINAL_OUTPUT_MAX_BYTES);
    match &env.data.final_output {
        Fact::Known(FinalOutput::Reported(BoundedText { text, truncated })) => {
            assert!(*truncated);
            assert!(text.len() <= FINAL_OUTPUT_MAX_BYTES, "{} bytes", text.len());
            assert!(
                original.starts_with(text.as_str()),
                "the cut keeps the start"
            );
            // The fixture puts a two-byte character across byte 4096.
            assert_eq!(text.len(), FINAL_OUTPUT_MAX_BYTES - 1);
        }
        other => panic!("expected a cut final output, got {other:?}"),
    }
}

#[test]
fn a_session_without_final_output_reads_no_final_output_yet() {
    let fake = Fake::new("p07_no_final_output_yet", "normal");
    let env = fake.show("thr_syn_0005").expect("show");
    assert_eq!(
        env.data.final_output,
        Fact::Known(FinalOutput::NoFinalOutputYet)
    );
    let (code, text, _) = fake.cli(&["show", "thr_syn_0005"]);
    assert_eq!(code, 0);
    assert!(text.contains("no final output yet"), "{text}");
}

#[test]
fn show_reads_final_output_never_a_log_tail() {
    let fake = Fake::new("p07_never_a_log_tail", "normal");
    fake.show("thr_syn_0001").expect("show");
    let calls = fake.invocations();
    assert!(
        calls
            .iter()
            .any(|a| a.starts_with(&["thread".into(), "output".into()]))
    );
    assert!(
        calls.iter().all(|a| !a.iter().any(|w| w == "log")),
        "show read a log: {calls:?}"
    );
}

#[test]
fn rows_are_capped() {
    let source = ScriptedSource::new(MAX_LIMIT + 10);
    match list_scripted(
        &source,
        &ListRequest {
            limit: MAX_LIMIT + 1,
            ..ListRequest::default()
        },
    ) {
        Ok(env) => assert!(env.data.sessions.len() <= MAX_LIMIT),
        Err(OpError::InvalidRequest(reason)) => assert!(!reason.is_empty()),
        Err(other) => panic!("a too-large limit is capped or refused, got {other:?}"),
    }
}

/// The cut is shown
/// (text contract 7 in `support.rs`), for the final output and the purpose.
#[test]
fn printed_final_output_and_purpose_show_their_cut() {
    let fake = Fake::new("p07_printed_final_output_cut", "normal");
    let original = fixture_json("thread-output-0002.json")["output"]
        .as_str()
        .expect("output")
        .to_owned();
    let kept = &original[..FINAL_OUTPUT_MAX_BYTES - 1];
    let (code, text, _) = fake.cli(&["show", "thr_syn_0002"]);
    assert_eq!(code, 0);
    assert!(
        text.contains(&format!("{kept}{CUT_MARKER}")),
        "the first 4 KiB are printed, and the cut is shown"
    );
    assert!(
        !text.contains("Synthetic tail after the cut"),
        "nothing past 4 KiB is printed"
    );
    let json = fake.cli_json(&["show", "thr_syn_0002", "--json"]);
    let printed = json["data"]["final_output"]["known"]["text"]
        .as_str()
        .expect("text");
    assert!(printed.len() <= 4096);
    assert_eq!(json["data"]["final_output"]["known"]["truncated"], true);

    let rows = fixture_json("thread-list.json");
    let purpose = rows[1]["purpose"]["text"].as_str().expect("purpose");
    let kept: String = purpose.chars().take(PURPOSE_MAX_CHARS).collect();
    let (code, text, _) = fake.cli(&["ls", "-l"]);
    assert_eq!(code, 0);
    assert!(
        row_lines(&text, "thr_syn_0002")
            .iter()
            .any(|l| l.contains(&format!("{kept}{CUT_MARKER}"))),
        "the cut purpose is shown as cut:\n{text}"
    );
    assert!(
        !row_lines(&text, "thr_syn_0001")
            .iter()
            .any(|l| l.contains(&format!("Synthetic purpose 1.{CUT_MARKER}"))),
        "an uncut purpose has no marker:\n{text}"
    );
}

/// Ends every oversized value, so any uncut value is easy to find.
const TAIL: &str = "OVERSIZED-TAIL";

fn oversized(label: &str) -> String {
    format!("Synthetic oversized {label} {}{TAIL}", "x".repeat(400))
}

struct Oversized {
    title: String,
    provider: String,
    status: String,
    machine: String,
    project: String,
    model: String,
    effort: String,
    purpose: String,
    reported_error: String,
    error_line: String,
    reasons: [String; 6],
    problem_detail: String,
    unseen_project: String,
}

/// A scripted source with an oversized value on every displayed field path:
/// - `thr_syn_0001`: title, provider, status, machine name (local), project
///   name, purpose, model and effort, and the reported error in its output;
/// - `thr_syn_0002`: an oversized reason on every unknown, and a field
///   problem with an oversized detail;
/// - `thr_syn_0003`: an oversized unseen project id (so the ungrouped reason
///   is long), and a detail read that fails with an oversized error line.
fn oversized_source() -> (ScriptedSource, Oversized) {
    let o = Oversized {
        title: oversized("title"),
        provider: oversized("provider"),
        status: oversized("status"),
        machine: oversized("machine name"),
        project: oversized("project name"),
        model: oversized("model"),
        effort: oversized("effort"),
        purpose: oversized("purpose"),
        reported_error: oversized("reported error"),
        error_line: oversized("error line"),
        reasons: [
            oversized("title reason"),
            oversized("status reason"),
            oversized("location reason"),
            oversized("parent reason"),
            oversized("age reason"),
            oversized("project reason"),
        ],
        problem_detail: oversized("problem detail"),
        unseen_project: format!("prj_syn_{}", oversized("project id")),
    };
    let mut first = scripted_record(1);
    first.title = Fact::Known(o.title.clone());
    first.provider = Fact::Known(o.provider.clone());
    first.status = Fact::Known(o.status.clone());
    first.placement = Fact::Known(Placement::Host("mch_syn_0001".to_owned()));
    first.purpose_text = Fact::Known(Some(o.purpose.clone()));

    let mut second = scripted_record(2);
    second.title = Fact::Unknown(o.reasons[0].clone());
    second.status = Fact::Unknown(o.reasons[1].clone());
    second.placement = Fact::Unknown(o.reasons[2].clone());
    second.parent = Fact::Unknown(o.reasons[3].clone());
    second.updated_at = Fact::Unknown(o.reasons[4].clone());
    second.project_id = Fact::Unknown(o.reasons[5].clone());
    second.problems = vec![Gap {
        kind: GapKind::MissingField,
        subject: Some("thr_syn_0002 status".to_owned()),
        detail: o.problem_detail.clone(),
    }];

    let mut third = scripted_record(3);
    third.project_id = Fact::Known(ProjectId(o.unseen_project.clone()));

    let source = ScriptedSource::with_records(vec![first, second, third])
        .with_projects(vec![ProjectRecord {
            id: ProjectId("prj_syn_0001".to_owned()),
            name: Fact::Known(o.project.clone()),
        }])
        .with_machines(vec![MachineRecord {
            id: "mch_syn_0001".to_owned(),
            name: Fact::Known(o.machine.clone()),
        }])
        .with_configuration(Fact::Known(o.model.clone()), Fact::Known(o.effort.clone()))
        .with_output(OutputRecord {
            output: Fact::Known(Some("Synthetic final output.".to_owned())),
            error: Fact::Known(Some(o.reported_error.clone())),
            problems: vec![],
        })
        .with_failing_detail(
            "thr_syn_0003",
            SourceError::CommandFailed {
                exit_code: Some(1),
                detail: o.error_line.clone(),
            },
        );
    (source, o)
}

/// Every string in `json`, with its path, except the final output text
/// (capped in bytes, by P7's 4 KiB tests).
fn strings<'a>(json: &'a Value, path: String, out: &mut Vec<(String, &'a str)>) {
    match json {
        Value::String(s) if !path.ends_with("/final_output/known/text") => out.push((path, s)),
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                strings(v, format!("{path}/{i}"), out);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                strings(v, format!("{path}/{k}"), out);
            }
        }
        _ => {}
    }
}

fn assert_every_string_is_capped(label: &str, json: &Value) {
    let mut all = Vec::new();
    strings(json, String::new(), &mut all);
    for (path, s) in all {
        assert!(
            s.chars().count() <= FIELD_MAX_CHARS && !s.contains(TAIL),
            "{label}: {path} is not capped ({} characters): {s}",
            s.chars().count()
        );
    }
}

/// Every string field in the result is capped; plain text fields keep
/// their first `FIELD_MAX_CHARS - 1` characters and end with `…`, and
/// bounded texts set `truncated`.
#[test]
fn every_field_is_capped() {
    assert_eq!(FIELD_MAX_CHARS, 200, "the provisional field cap");
    let (source, o) = oversized_source();
    let env = list_scripted(&source, &long()).expect("ls -l");
    assert_every_string_is_capped("ls -l", &serde_json::to_value(&env).expect("JSON"));

    let first = session(&env, "thr_syn_0001");
    assert_eq!(first.title, Fact::Known(capped(&o.title)));
    assert_eq!(first.provider, Fact::Known(capped(&o.provider)));
    assert_eq!(first.status, Fact::Known(capped(&o.status)));
    assert_eq!(
        first.location,
        Fact::Known(Location::Local {
            machine: Fact::Known(capped(&o.machine)),
        })
    );
    assert_eq!(
        first.group,
        Grouping::Project {
            id: ProjectId("prj_syn_0001".to_owned()),
            name: Fact::Known(capped(&o.project)),
        }
    );
    assert_eq!(
        first.reported_configuration,
        ReportedConfiguration {
            model: Fact::Known(capped(&o.model)),
            effort: Fact::Known(capped(&o.effort)),
        }
    );
    assert_eq!(
        first.recap,
        Fact::Known(Recap::PurposeAsReported(BoundedText {
            text: o.purpose.chars().take(PURPOSE_MAX_CHARS).collect(),
            truncated: true,
        }))
    );

    let second = session(&env, "thr_syn_0002");
    let reasons: Vec<String> = o.reasons.iter().map(|r| capped(r)).collect();
    assert_eq!(second.title, Fact::Unknown(reasons[0].clone()));
    assert_eq!(second.status, Fact::Unknown(reasons[1].clone()));
    assert_eq!(second.location, Fact::Unknown(reasons[2].clone()));
    assert_eq!(second.parent, Fact::Unknown(reasons[3].clone()));
    assert_eq!(second.updated_at, Fact::Unknown(reasons[4].clone()));
    assert!(
        env.gaps
            .iter()
            .any(|g| g.detail == capped(&o.problem_detail)),
        "the field problem's detail is capped: {:?}",
        env.gaps
    );

    let third = session(&env, "thr_syn_0003");
    match &third.group {
        Grouping::Ungrouped { reason } => assert!(
            reason.ends_with(CUT_MARKER),
            "the reason with the long id is cut: {reason}"
        ),
        other => panic!("the long project id is unseen, got {other:?}"),
    }
    assert!(
        env.gaps.iter().any(|g| g.kind == GapKind::CommandFailed
            && g.subject
                .as_deref()
                .is_some_and(|s| s.contains("thr_syn_0003"))
            && g.detail.ends_with(CUT_MARKER)),
        "the failed read's error line is cut: {:?}",
        env.gaps
    );

    let shown = ops::show(
        &source,
        &clock(),
        &ShowRequest {
            id: SessionId("thr_syn_0001".to_owned()),
            budget: RUN_BUDGET,
        },
    )
    .expect("show");
    assert_every_string_is_capped("show", &serde_json::to_value(&shown).expect("JSON"));
    match &shown.data.reported_error {
        Fact::Known(Some(BoundedText { text, truncated })) => {
            assert!(*truncated);
            assert!(text.chars().count() <= FIELD_MAX_CHARS, "{text}");
            assert!(o.reported_error.starts_with(text.as_str()), "{text}");
        }
        other => panic!("expected a cut reported error, got {other:?}"),
    }
}

/// Every cut value is shown as cut in the printed output (text contract 7),
/// and nothing past a cap is printed, in `ls -l`, `show`, `--json` and on
/// stderr. That includes the ungrouped reason with the long project id in
/// `ls -l`, and the cut project name in `show`.
#[test]
fn printed_output_shows_every_cut() {
    let (source, o) = oversized_source();

    let (code, text, err) = cli_with(&source, &["ls", "-l"]);
    assert_eq!(code, 0, "{err}");
    assert!(!text.contains(TAIL) && !err.contains(TAIL), "{text}{err}");
    let row = |id: &str| row_lines(&text, id).join("\n");
    for value in [&o.status, &o.machine, &o.model, &o.effort] {
        let wanted = capped(value);
        assert!(
            row("thr_syn_0001").contains(&wanted),
            "ls -l shows {wanted:?} as cut:\n{text}"
        );
    }
    let purpose: String = o.purpose.chars().take(PURPOSE_MAX_CHARS).collect();
    assert!(
        row("thr_syn_0001").contains(&format!("{purpose}{CUT_MARKER}")),
        "{text}"
    );
    assert!(
        text.contains(&format!("{GROUP_ROOT}{}/", capped(&o.project))),
        "the project name is shown as cut:\n{text}"
    );
    for reason in &o.reasons[1..4] {
        let wanted = format!("unknown ({})", capped(reason));
        assert!(
            row("thr_syn_0002").contains(&wanted),
            "ls -l shows {wanted:?}:\n{text}"
        );
    }
    let gaps = gap_lines(&text);
    assert!(
        gaps.iter().any(|l| l.contains(&capped(&o.problem_detail))),
        "the field problem's detail is shown as cut:\n{text}"
    );
    assert!(
        gaps.iter()
            .any(|l| l.contains("thr_syn_0003") && l.contains(CUT_MARKER)),
        "the failed read's error line is shown as cut:\n{text}"
    );

    let json = cli_json_with(&source, &["ls", "-l", "--json"]);
    assert_every_string_is_capped("ls -l --json", &json);
    let reason = json["data"]["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|s| s["id"] == "thr_syn_0003")
        .and_then(|s| s["group"]["reason"].as_str())
        .expect("thr_syn_0003 is ungrouped");
    assert!(reason.ends_with(CUT_MARKER), "{reason}");
    assert!(
        row("thr_syn_0003").contains(reason),
        "the cut ungrouped reason is shown:\n{text}"
    );

    let (code, text, err) = cli_with(&source, &["show", "thr_syn_0001"]);
    assert_eq!(code, 0, "{err}");
    assert!(!text.contains(TAIL) && !err.contains(TAIL), "{text}{err}");
    for value in [&o.title, &o.status, &o.machine, &o.model, &o.effort] {
        let wanted = capped(value);
        assert!(text.contains(&wanted), "show shows {wanted:?}:\n{text}");
    }
    assert!(
        text.contains(&format!("{GROUP_ROOT}{}/", capped(&o.project))),
        "show shows the project name as cut:\n{text}"
    );
    let json = cli_json_with(&source, &["show", "thr_syn_0001", "--json"]);
    assert_every_string_is_capped("show --json", &json);
    let error = json["data"]["reported_error"]["known"]["text"]
        .as_str()
        .expect("reported error");
    assert!(
        text.contains(&format!("{error}{CUT_MARKER}")),
        "the reported error is shown as cut:\n{text}"
    );

    let (code, text, err) = cli_with(&source, &["show", "thr_syn_0002"]);
    assert_eq!(code, 0, "{err}");
    let wanted = format!("unknown ({})", capped(&o.reasons[0]));
    assert!(text.contains(&wanted), "show shows {wanted:?}:\n{text}");

    // An oversized version (never a gate): not a recognised version, so
    // it is unknown with a reason and never echoed; the run goes on.
    let version = oversized("version");
    let source = ScriptedSource::new(1).with_version(&version);
    let (code, out, err) = cli_with(&source, &["ls"]);
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains(TAIL) && !err.contains(TAIL), "{out}{err}");
    let header = out.lines().next().unwrap_or_default();
    assert!(header.contains("room-cli unknown ("), "{header}");
}

/// The text never
/// prints characters that drive or reorder the terminal (escape, bidi
/// controls, line and paragraph separators, zero-width marks). The JSON
/// keeps the reported value.
#[test]
fn the_text_never_prints_terminal_control_or_bidi_characters() {
    const UNSAFE: [char; 9] = [
        '\u{1b}', '\u{202E}', '\u{2066}', '\u{2069}', '\u{200F}', '\u{200B}', '\u{2028}',
        '\u{2029}', '\u{FEFF}',
    ];
    let mixed: String = UNSAFE.iter().map(|c| format!("{c}Synthetic")).collect();
    let mut record = scripted_record(1);
    record.title = Fact::Known(mixed.clone());
    record.purpose_text = Fact::Known(Some(mixed.clone()));
    let source = ScriptedSource::with_records(vec![record]).with_output(OutputRecord {
        output: Fact::Known(Some(format!("line one\n{mixed}\nline three"))),
        error: Fact::Known(Some(mixed.clone())),
        problems: vec![],
    });
    for args in [&["ls", "-l"][..], &["show", "thr_syn_0001"][..]] {
        let (code, text, err) = cli_with(&source, args);
        assert_eq!(code, 0, "{err}");
        for c in UNSAFE {
            assert!(
                !text.contains(c),
                "{args:?} prints U+{:04X}:\n{text}",
                c as u32
            );
        }
    }
    let (_, text, _) = cli_with(&source, &["show", "thr_syn_0001"]);
    assert!(
        text.contains("line one\n"),
        "line breaks in the output are kept"
    );
    let json = cli_json_with(&source, &["show", "thr_syn_0001", "--json"]);
    assert_eq!(json["data"]["session"]["title"]["known"], mixed.as_str());
}

/// Replacing unsafe characters
/// never makes the printed final output longer than the 4 KiB it was cut to.
#[test]
fn the_printed_final_output_stays_within_4_kib() {
    for (label, unit) in [
        ("escape", "\u{1b}"),
        ("C1 control", "\u{85}"),
        ("bidi", "\u{202E}"),
    ] {
        let output = unit.repeat(5_000);
        let source = ScriptedSource::new(1).with_output(OutputRecord {
            output: Fact::Known(Some(output)),
            error: Fact::Known(None),
            problems: vec![],
        });
        let (code, text, err) = cli_with(&source, &["show", "thr_syn_0001"]);
        assert_eq!(code, 0, "{err}");
        let block = text
            .split_once("final output:\n")
            .map(|(_, rest)| rest)
            .unwrap_or_else(|| panic!("{label}: no final output block:\n{text}"));
        let printed = block.split_once(CUT_MARKER).map_or(block, |(kept, _)| kept);
        assert!(
            printed.len() <= 4096,
            "{label}: {} bytes of final output printed",
            printed.len()
        );
        assert!(block.contains(CUT_MARKER), "{label}: the cut is shown");
    }
}
