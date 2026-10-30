//! P10 JSON matches the text: both outputs come from the same envelope and
//! say the same thing.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::schema;
use crate::support::*;

#[test]
fn ls_json_and_text_list_the_same_sessions() {
    let fake = Fake::new("p10_same_sessions", "normal");
    let json = fake.cli_json(&["ls", "--json"]);
    let (code, text, _) = fake.cli(&["ls"]);
    assert_eq!(code, 0);
    let from_json: BTreeSet<String> = json["data"]["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|s| s["id"].as_str().expect("id").to_owned())
        .collect();
    let from_text: BTreeSet<String> = DEFAULT_IDS
        .iter()
        .chain([HIDDEN_ID, ARCHIVED_ID].iter())
        .filter(|id| !row_lines(&text, id).is_empty())
        .map(|id| (*id).to_owned())
        .collect();
    assert_eq!(from_json, from_text);
}

#[test]
fn ls_long_rows_say_the_same_in_json_and_text() {
    let fake = Fake::new("p10_long_rows", "normal");
    let json = fake.cli_json(&["ls", "-l", "--json"]);
    let (code, text, _) = fake.cli(&["ls", "-l"]);
    assert_eq!(code, 0);
    for row in json["data"]["sessions"].as_array().expect("sessions") {
        let id = row["id"].as_str().expect("id");
        let lines = row_lines(&text, id);
        assert_eq!(lines.len(), 1, "{id} should have one row:\n{text}");
        let line = lines[0];
        for pointer in [
            "/status",
            "/reported_configuration/model",
            "/reported_configuration/effort",
        ] {
            let wanted = fact_text(row.pointer(pointer).expect(pointer));
            assert!(
                line.contains(&wanted),
                "{id}{pointer}: {wanted:?} not in {line:?}"
            );
        }
        let recap = &row["recap"]["known"];
        if recap["kind"] == "purpose_as_reported" {
            let words = recap["text"].as_str().expect("recap text");
            assert!(
                line.contains(words),
                "{id}: recap {words:?} not in {line:?}"
            );
        }
    }
    let state = json["coverage"]["state"].as_str().expect("state");
    let header = text.lines().next().unwrap_or_default();
    assert!(
        header.contains(state),
        "header {header:?} should say {state}"
    );
}

#[test]
fn show_says_the_same_in_json_and_text() {
    let fake = Fake::new("p10_show", "normal");
    let json = fake.cli_json(&["show", "thr_syn_0003", "--json"]);
    let (code, text, _) = fake.cli(&["show", "thr_syn_0003"]);
    assert_eq!(code, 0);
    let s = &json["data"]["session"];
    let status = fact_text(&s["status"]);
    assert!(
        text.lines()
            .any(|l| l.contains("status") && l.contains(&status)),
        "a status line shows {status:?}:\n{text}"
    );
    for pointer in [
        "/title",
        "/reported_configuration/model",
        "/reported_configuration/effort",
    ] {
        let wanted = fact_text(s.pointer(pointer).expect(pointer));
        assert!(
            text.contains(&wanted),
            "{pointer}: {wanted:?} not in show text"
        );
    }
    let output = json["data"]["final_output"]["known"]["text"]
        .as_str()
        .expect("final output");
    assert!(text.contains(output), "the final output is printed");
    let error = json["data"]["reported_error"]["known"]["text"]
        .as_str()
        .expect("reported error");
    assert!(text.contains(error), "the reported error is printed");
}

#[test]
fn json_output_matches_the_schema() {
    let schema = schema::load_schema();
    let fake = Fake::new("p10_json_schema", "show-error");
    for args in [
        &["ls", "--json"][..],
        &["ls", "-l", "--json"][..],
        &["ls", "-l", "-a", "--hidden", "--json"][..],
        &["show", "thr_syn_0001", "--json"][..],
    ] {
        let json = fake.cli_json(args);
        let errors = schema::validate(&schema, &json);
        assert!(errors.is_empty(), "{args:?}: {errors:#?}");
    }
}

/// The header, counts, gaps, freshness, location, grouping and parent in the
/// text say what the JSON says (text contract in `support.rs`).
fn assert_envelope_text_matches(label: &str, json: &Value, text: &str) {
    let header = text.lines().next().unwrap_or_default();
    let source = format!(
        "{} {}",
        json["source"]["name"].as_str().expect("source name"),
        fact_text(&json["source"]["version"])
    );
    let snapshot = format!(
        "snapshot {}",
        utc_text(json["freshness"]["snapshot_at"].as_i64().expect("ms"))
    );
    let state = json["coverage"]["state"].as_str().expect("state");
    for wanted in [source.as_str(), snapshot.as_str(), state] {
        assert!(
            header.contains(wanted),
            "{label}: the header should contain {wanted:?}: {header}"
        );
    }

    let gaps = json["gaps"].as_array().expect("gaps");
    let lines = gap_lines(text);
    assert_eq!(
        lines.len(),
        gaps.len(),
        "{label}: one gap line per gap:\n{text}"
    );
    for gap in gaps {
        let kind = format!("{GAP_PREFIX}{}", gap["kind"].as_str().expect("kind"));
        let subject = gap["subject"].as_str().unwrap_or_default();
        let detail = gap["detail"].as_str().expect("detail");
        assert!(
            lines.iter().any(|l| l.trim_start().starts_with(&kind)
                && l.contains(subject)
                && l.contains(detail)),
            "{label}: no gap line for {gap}:\n{text}"
        );
    }
}

fn assert_row_text_matches(label: &str, row: &Value, text: &str, shared: &BTreeSet<String>) {
    let id = row["id"].as_str().expect("id");
    let lines = row_lines(text, id);
    assert_eq!(lines.len(), 1, "{label}: {id} should have one row:\n{text}");
    let line = lines[0];
    let mut wanted = vec![age_token(row), location_token(row), parent_token(row)];
    if let Some(reason) = row["group"]["reason"].as_str() {
        wanted.push(reason.to_owned());
    }
    for w in wanted {
        assert!(line.contains(&w), "{label}: {id}: {w:?} not in {line:?}");
    }
    let header = rows_by_group(text)
        .into_iter()
        .find(|(row_id, _)| row_id == id)
        .and_then(|(_, header)| header);
    assert_eq!(
        header,
        Some(group_header(row, shared)),
        "{label}: {id} is under the wrong group:\n{text}"
    );
}

#[test]
fn ls_text_matches_the_json_envelope_and_rows() {
    let shared = shared_project_names();
    for (scenario, args) in [
        ("normal", &["ls"][..]),
        ("normal", &["ls", "-l", "-a", "--hidden"][..]),
        ("normal", &["ls", "--limit", "2", "--offset", "3"][..]),
        ("show-error", &["ls", "-l"][..]),
        ("thread-list-missing-fields", &["ls"][..]),
        ("project-list-error", &["ls"][..]),
    ] {
        let label = format!("{scenario} {args:?}");
        let fake = Fake::new(
            &format!("p10_envelope_{scenario}_{}", args.join("_")),
            scenario,
        );
        let mut json_args = args.to_vec();
        json_args.push("--json");
        let json = fake.cli_json(&json_args);
        let (code, text, _) = fake.cli(args);
        assert_eq!(code, 0, "{label}");
        assert_envelope_text_matches(&label, &json, &text);

        let from_json: BTreeMap<String, String> = json["coverage"]["counts"]
            .as_object()
            .expect("counts")
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        let from_text: BTreeMap<String, String> = text_counts(&text).into_iter().collect();
        assert_eq!(from_text, from_json, "{label}: counts:\n{text}");

        let rows = json["data"]["sessions"].as_array().expect("sessions");
        let row_ids: Vec<String> = rows_by_group(&text).into_iter().map(|(id, _)| id).collect();
        let json_ids: Vec<String> = rows
            .iter()
            .map(|r| r["id"].as_str().expect("id").to_owned())
            .collect();
        assert_eq!(
            row_ids.iter().collect::<BTreeSet<_>>(),
            json_ids.iter().collect::<BTreeSet<_>>(),
            "{label}: the same rows:\n{text}"
        );
        for row in rows {
            assert_row_text_matches(&label, row, &text, &shared);
        }
    }
}

#[test]
fn show_text_matches_the_json_envelope() {
    let shared = shared_project_names();
    for (scenario, id) in [
        ("normal", "thr_syn_0002"),
        ("normal", "thr_syn_0003"),
        ("normal", "thr_syn_0004"),
        ("normal", "thr_syn_0005"),
        ("show-missing-fields", "thr_syn_0001"),
        ("show-output-error", "thr_syn_0002"),
    ] {
        let label = format!("{scenario} show {id}");
        let fake = Fake::new(&format!("p10_show_envelope_{scenario}_{id}"), scenario);
        let json = fake.cli_json(&["show", id, "--json"]);
        let (code, text, _) = fake.cli(&["show", id]);
        assert_eq!(code, 0, "{label}");
        assert_envelope_text_matches(&label, &json, &text);
        let session = &json["data"]["session"];
        let mut wanted = vec![
            age_token(session),
            location_token(session),
            parent_token(session),
            group_header(session, &shared),
            configuration_text(session),
        ];
        if let Some(reason) = session["group"]["reason"].as_str() {
            wanted.push(reason.to_owned());
        }
        for w in wanted {
            assert!(text.contains(&w), "{label}: {w:?} not in:\n{text}");
        }
    }
}

/// `cargo build --release` produces `openwiz`,
/// and its `--json` output passes the schema check. The test builds the
/// release binary into this workspace's target directory, runs it with the
/// fake `room-cli` first on `PATH`, and checks every envelope against the
/// schema. It also shows the program's own wiring: the system clock for the
/// snapshot, and the exit code of an untested version.
#[test]
fn the_release_build_prints_schema_valid_json() {
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let target = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .expect("the target directory")
        .to_path_buf();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let build = Command::new(cargo)
        .args(["build", "--release", "--locked", "-p", "openwiz-cli"])
        .args(["--bin", "openwiz", "--target-dir"])
        .arg(&target)
        .current_dir(&root)
        .output()
        .expect("run cargo build --release");
    assert!(
        build.status.success(),
        "cargo build --release failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let program = target.join("release").join("openwiz");
    assert!(program.is_file(), "no {program:?}");

    let fake = Fake::new("p10_release_build", "normal");
    let path = format!(
        "{}:{}",
        fake.dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = |args: &[&str]| {
        // Found through PATH, the fake sees a bare `room-cli` as its argv[0]
        // and looks for its scenario in the working directory.
        let out = Command::new(&program)
            .args(args)
            .env("PATH", &path)
            .current_dir(&fake.dir)
            .output()
            .expect("run the release openwiz");
        (
            out.status.code(),
            String::from_utf8(out.stdout).expect("UTF-8 stdout"),
            String::from_utf8(out.stderr).expect("UTF-8 stderr"),
        )
    };
    let now = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after 1970")
            .as_millis() as i64
    };

    let schema = schema::load_schema();
    for args in [
        &["ls", "--json"][..],
        &["ls", "-l", "--json"][..],
        &["ls", "-l", "-a", "--hidden", "--json"][..],
        &["show", "thr_syn_0002", "--json"][..],
    ] {
        let before = now();
        let (code, out, err) = run(args);
        let after = now();
        assert_eq!(code, Some(0), "release openwiz {args:?}: {err}");
        let json: Value = serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("{args:?}: stdout is not JSON ({e}): {out}"));
        let errors = schema::validate(&schema, &json);
        assert!(errors.is_empty(), "{args:?}: {errors:#?}");
        assert_eq!(json["source"]["version"]["known"], "v127", "{args:?}");
        let snapshot = json["freshness"]["snapshot_at"].as_i64().expect("ms");
        assert!(
            (before - 1_000..=after + 1_000).contains(&snapshot),
            "{args:?}: the snapshot {snapshot} is not the system time ({before}..{after})"
        );
    }
    let (code, text, _) = run(&["ls"]);
    assert_eq!(code, Some(0));
    assert!(
        text.lines()
            .next()
            .unwrap_or_default()
            .contains("room-cli v127"),
        "{text}"
    );

    // An untested version is never a gate; the release binary lists and
    // marks it untested.
    fake.set_scenario("version-0.43.1");
    let (code, out, err) = run(&["ls", "--json"]);
    assert_eq!(code, Some(0), "{err}");
    let json: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(json["source"]["version"]["known"], "0.43.1", "{out}");
    assert_eq!(json["source"]["version_tested"], false, "{out}");
}
