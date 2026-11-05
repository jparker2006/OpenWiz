//! AP11, the CLI half. `openwiz attachment add` reports every path, in the
//! order given, and one failure hides no other; the JSON matches the text;
//! every exit code is documented; `contracts/attachment.v1.schema.json`
//! matches every `--json` report; a usage error opens nothing. A path in a
//! message's text is never an attachment (M15 still proves it for `send`).

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

use openwiz_cli::attachment::*;
use openwiz_core::attachment::{Claim, IMAGE_MAX_BYTES, UNSENT_TTL};
use openwiz_core::model::Timestamp;
use serde_json::Value;

use crate::bench::{Bench, CONTENT, ClaimsOf, Free, PLENTY, T0, Unmeasurable};
use crate::support::{JPEG, PNG, heic, padded, validate_def};

const TTL_MS: i64 = UNSENT_TTL.as_millis() as i64;

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// A bench with one of each kind of path: two that stage, and five that
/// fail for different reasons.
fn mixed(bench: &Bench) -> Vec<String> {
    let folder = bench.sources.join("syn-ap11-folder");
    crate::dirs::private_dir(&folder).unwrap();
    [
        bench.source("syn-ap11-photo.png", &padded(PNG, 3_000)),
        folder,
        bench.source("syn-ap11-photo.heic", &heic()),
        bench.source("syn-ap11-large.png", &padded(PNG, IMAGE_MAX_BYTES + 1)),
        bench.source("syn-ap11-named.jpg", &padded(PNG, 100)),
        bench.sources.join("syn-ap11-missing.txt"),
        bench.source("syn-ap11-notes.txt", CONTENT.as_bytes()),
    ]
    .iter()
    .map(|p| p.to_str().unwrap().to_owned())
    .collect()
}

const MIXED_REASONS: [(usize, &str); 5] = [
    (1, "folder"),
    (2, "unsupported_heif"),
    (3, "image_too_large"),
    (4, "extension_mismatch"),
    (5, "unreadable"),
];

#[test]
fn every_path_is_reported_in_order_and_one_failure_hides_no_other() {
    let bench = Bench::new("ap11_every_path");
    let paths = mixed(&bench);
    let mut args = vec!["add"];
    args.extend(paths.iter().map(String::as_str));
    let text = bench.cli(&args);
    assert_eq!(text.code, EXIT_ATTACHMENT_FAILED, "{text:?}");
    let lines: Vec<&str> = text.out.lines().collect();
    assert_eq!(
        lines[0],
        "openwiz attachment add · 7 paths: 2 ready, 5 failed, 0 not staged"
    );
    for (i, path) in paths.iter().enumerate() {
        assert!(
            lines[i + 1].starts_with(&format!("{path}: ")),
            "line {} is not {path}'s:\n{}",
            i + 1,
            text.out
        );
    }
    for (i, reason) in MIXED_REASONS {
        assert!(
            lines[i + 1].contains(&format!(": failed ({reason}) · ")),
            "{}",
            lines[i + 1]
        );
    }
    for i in [0, 6] {
        assert!(lines[i + 1].contains(": ready · att-"), "{}", lines[i + 1]);
    }
    assert_eq!(lines.len(), 8, "{}", text.out);

    let json = bench.cli(&{
        let mut a = args.clone();
        a.insert(1, "--json");
        a
    });
    assert_eq!(json.code, EXIT_ATTACHMENT_FAILED);
    let json = json.json();
    let indices = |key: &str| -> Vec<u64> {
        json[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["index"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(indices("ready"), [0, 6]);
    assert_eq!(indices("failed"), [1, 2, 3, 4, 5]);
    assert!(indices("not_staged").is_empty());
    for (entry, (i, reason)) in json["failed"].as_array().unwrap().iter().zip(MIXED_REASONS) {
        assert_eq!(entry["path"], paths[i].as_str());
        assert_eq!(entry["readiness"]["reason"], reason);
    }
    // Both runs staged the two that can be, each under its own id.
    assert_eq!(bench.ids().len(), 4);
}

/// Every value of a staged attachment, as its text line shows it.
fn staged_words(staged: &Value) -> Vec<String> {
    let a = &staged["attachment"];
    let mut words = vec![
        a["id"].as_str().unwrap().to_owned(),
        format!(" · {} · ", a["name"].as_str().unwrap()),
        format!(" · {} · ", a["kind"].as_str().unwrap()),
        format!(" · {} bytes · ", a["size_bytes"]),
        format!(" · sha256 {}", a["sha256"].as_str().unwrap()),
        format!(" · from {}", a["display_name"].as_str().unwrap()),
        format!(" · ready {}", utc(staged["ready_at"].as_i64().unwrap())),
    ];
    match staged["expires_at"].as_i64() {
        Some(at) => words.push(format!(" · expires {}", utc(at))),
        None => words.push(" · kept while its operation is in flight".to_owned()),
    }
    words
}

/// RFC 3339 UTC, whole seconds, as the CLI prints times (`civil`, as
/// `crates/cli`'s `utc`).
fn utc(ms: i64) -> String {
    let secs = ms.div_euclid(1_000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// The line of `text` that starts with `start`; exactly one.
fn line<'t>(text: &'t str, start: &str) -> &'t str {
    let found: Vec<&str> = text.lines().filter(|l| l.starts_with(start)).collect();
    assert_eq!(found.len(), 1, "one line starts {start:?}:\n{text}");
    found[0]
}

fn assert_has(line: &str, words: &[String]) {
    for word in words {
        assert!(line.contains(word.as_str()), "{word:?} not in {line:?}");
    }
}

#[test]
fn the_json_matches_the_text() {
    let bench = Bench::new("ap11_json_text");
    let claims = ClaimsOf::default();
    let old = bench.source("syn-ap11-old.txt", CONTENT.as_bytes());
    let old_run = bench.cli(&["add", "--json", old.to_str().unwrap()]);
    let old_id = old_run.json()["ready"][0]["staged"]["attachment"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let later = Timestamp(T0.0 + TTL_MS);
    let paths = mixed(&bench);
    let run = |args: Vec<&str>, json: bool| {
        let mut args = args;
        if json {
            args.insert(1, "--json");
        }
        bench.cli_with(&os(&args), later, &Free::new(PLENTY), &claims)
    };

    // add: the old one expires in this run.
    let mut add = vec!["add"];
    add.extend(paths.iter().map(String::as_str));
    let (text, json) = (run(add.clone(), false), run(add, true));
    assert_eq!(text.code, json.code);
    let (text, json) = (text.out, json.json());
    assert_eq!(json["schema"], ADD_SCHEMA_ID);
    // The old one expired in the text's run, so only that run reports it.
    assert!(json["expired"].as_array().unwrap().is_empty());
    for entry in json["ready"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let line = line(&text, &format!("{path}: ready · "));
        // The text's own run staged its own copy, under another id.
        assert_has(line, &staged_words(&entry["staged"])[1..]);
    }
    for entry in json["failed"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let reason = entry["readiness"]["reason"].as_str().unwrap();
        assert_eq!(
            line(&text, &format!("{path}: ")),
            format!(
                "{path}: failed ({reason}) · {}",
                entry["message"].as_str().unwrap()
            )
        );
    }
    let mut expired = old_run.json()["ready"][0]["staged"].clone();
    expired["expires_at"] = Value::from(T0.0 + TTL_MS);
    let expired_line = line(&text, &format!("expired {old_id} · "));
    assert_has(expired_line, &staged_words(&expired));

    // ls: ready ones, one kept by its claim (no expiry shown), one broken.
    let ls_json = run(vec!["ls"], true).json();
    let ids: Vec<String> = ls_json["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["attachment"]["id"].as_str().unwrap().to_owned())
        .collect();
    claims.set(&ids[0], Claim::Dispatching);
    let broken = bench.attachments().join(&ids[1]).join(".meta.json");
    std::fs::write(&broken, "not JSON").unwrap();
    let (text, json) = (run(vec!["ls"], false), run(vec!["ls"], true));
    assert_eq!((text.code, json.code), (0, 0));
    let (text, json) = (text.out, json.json());
    assert_eq!(json["schema"], LIST_SCHEMA_ID);
    let attachments = json["attachments"].as_array().unwrap();
    let header = format!(
        "openwiz attachment ls · {} ready, 1 broken, 0 expired",
        attachments.len()
    );
    assert_eq!(text.lines().next(), Some(header.as_str()));
    for staged in attachments {
        let id = staged["attachment"]["id"].as_str().unwrap();
        let line = line(&text, &format!("{id} · ready · "));
        assert_has(line, &staged_words(staged));
        let age = staged["age_ms"].as_i64().unwrap();
        assert!(line.contains(" · age "), "{line}: age {age}");
    }
    assert!(attachments[0]["expires_at"].is_null());
    let broken = &json["broken"][0];
    assert_eq!(broken["id"], ids[1].as_str());
    assert_eq!(
        line(&text, &format!("{} · ", ids[1])),
        format!(
            "{} · failed (unreadable) · {}",
            ids[1],
            broken["message"].as_str().unwrap()
        )
    );

    // rm: removed, refused, not found. Removing is done once, so the text
    // and the JSON each remove their own.
    let missing = "att-synthetic-missing";
    let text = run(vec!["rm", ids[2].as_str(), ids[0].as_str(), missing], false);
    let json = run(vec!["rm", ids[3].as_str(), ids[0].as_str(), missing], true);
    assert_eq!(text.code, EXIT_ATTACHMENT_REFUSED, "{text:?}");
    assert_eq!(json.code, EXIT_ATTACHMENT_REFUSED);
    let json = json.json();
    assert_eq!(json["schema"], REMOVE_SCHEMA_ID);
    let entries = json["ids"].as_array().unwrap();
    let results: Vec<&str> = entries
        .iter()
        .map(|e| e["result"].as_str().unwrap())
        .collect();
    assert_eq!(results, ["removed", "refused", "not_found"]);
    let text = text.out;
    assert_eq!(
        text.lines().next(),
        Some(
            "openwiz attachment rm · 3 ids: 1 removed, 0 expired, 1 refused, 1 not found, 0 \
             not removed"
        )
    );
    for (entry, id) in entries
        .iter()
        .zip([ids[2].as_str(), ids[0].as_str(), missing])
    {
        let result = entry["result"].as_str().unwrap().replace('_', " ");
        assert_eq!(
            line(&text, &format!("{id}: ")),
            format!("{id}: {result} · {}", entry["message"].as_str().unwrap())
        );
    }
}

#[test]
fn every_report_matches_the_schema() {
    let bench = Bench::new("ap11_schema");
    let claims = ClaimsOf::default();
    let old = bench.source("syn-ap11-schema-old.jpg", &padded(JPEG, 10));
    assert_eq!(bench.cli(&["add", old.to_str().unwrap()]).code, 0);
    let later = Timestamp(T0.0 + TTL_MS);
    let mut reports = Vec::new();
    let mut run =
        |args: &[&str], now: Timestamp, free: &dyn openwiz_store::attachments::FreeSpace| {
            let run = bench.cli_with(&os(args), now, free, &claims);
            let json = run.json();
            reports.push((args[0].to_owned(), json.clone()));
            json
        };
    let paths = mixed(&bench);
    let mut add = vec!["add", "--json"];
    add.extend(paths.iter().map(String::as_str));
    let added = run(&add, later, &Free::new(PLENTY));
    assert_eq!(added["expired"].as_array().unwrap().len(), 1);
    let not_staged = run(&["add", "--json", &paths[0]], later, &Unmeasurable);
    assert_eq!(not_staged["not_staged"].as_array().unwrap().len(), 1);
    let ids: Vec<String> = added["ready"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["staged"]["attachment"]["id"].as_str().unwrap().to_owned())
        .collect();
    claims.set(&ids[0], Claim::Uncertain);
    std::fs::write(
        bench.attachments().join(&ids[1]).join(".meta.json"),
        "not JSON",
    )
    .unwrap();
    run(&["ls", "--json"], later, &Free::new(PLENTY));
    run(
        &["rm", "--json", &ids[0], &ids[1], "att-synthetic-missing"],
        later,
        &Free::new(PLENTY),
    );
    run(
        &["ls", "--json"],
        Timestamp(later.0 + TTL_MS),
        &Free::new(PLENTY),
    );
    for (command, report) in &reports {
        let def = match command.as_str() {
            "add" => "add_report",
            "ls" => "list_report",
            "rm" => "remove_report",
            other => panic!("{other}"),
        };
        let errors = validate_def(def, report);
        assert!(errors.is_empty(), "{def}: {errors:?}\n{report:#}");
    }
    // Each shape was seen at least once.
    let seen = |key: &str| {
        reports
            .iter()
            .any(|(_, r)| r[key].as_array().is_some_and(|a| !a.is_empty()))
    };
    for key in [
        "ready",
        "failed",
        "not_staged",
        "expired",
        "attachments",
        "broken",
        "ids",
    ] {
        assert!(seen(key), "no report had a {key} entry");
    }
    // And the schema refuses what the reports never print.
    let mut extra = reports[0].1.clone();
    extra["extra"] = Value::Bool(true);
    assert!(!validate_def("add_report", &extra).is_empty());
    let mut wrong = reports[0].1.clone();
    wrong["schema"] = Value::from(LIST_SCHEMA_ID);
    assert!(!validate_def("add_report", &wrong).is_empty());
}

#[test]
fn a_usage_error_opens_nothing() {
    let bench = Bench::new("ap11_usage");
    let source = bench.source("syn-ap11-usage.txt", CONTENT.as_bytes());
    let path = source.to_str().unwrap();
    for args in [
        vec![],
        vec!["add"],
        vec!["add", "--json"],
        vec!["add", "--bogus", path],
        vec!["add", "--json=yes", path],
        vec!["ls", path],
        vec!["ls", "--all"],
        vec!["rm"],
        vec!["rm", "../escape"],
        vec!["rm", "-rf"],
        vec!["upload", path],
    ] {
        let run = bench.cli(&args);
        assert_eq!(run.code, 2, "{args:?}: {run:?}");
        assert!(run.out.is_empty(), "{args:?}: {}", run.out);
        assert_eq!(run.err.lines().count(), 1, "{args:?}: {}", run.err);
        assert!(run.err.starts_with("openwiz: "), "{}", run.err);
        assert!(!bench.state.exists(), "{args:?} opened the store");
    }
}

#[test]
fn options_paths_and_names() {
    let bench = Bench::new("ap11_paths");
    // Help opens nothing.
    for args in [&["--help"][..], &["add", "--help"], &["rm", "-h"]] {
        let help = bench.cli(args);
        assert_eq!(help.code, 0, "{args:?}");
        assert_eq!(help.out, openwiz_cli::help());
    }
    assert!(!bench.state.exists());
    // `--` ends the options: a path may start with -.
    let dashed = bench.source("-syn-ap11-dashed.txt", CONTENT.as_bytes());
    let run = bench.cli(&["add", "--", dashed.to_str().unwrap(), "--json"]);
    assert_eq!(
        run.code, 5,
        "the second is a path, which is missing: {run:?}"
    );
    assert!(run.out.contains(": ready · "), "{}", run.out);
    assert!(
        run.out.contains("--json: failed (unreadable)"),
        "{}",
        run.out
    );

    // A name that is not UTF-8 is staged; its display name is shown with
    // the replacement character.
    let mut raw = bench.sources.clone().into_os_string().into_vec();
    raw.extend_from_slice(b"/syn-ap11-\xff.txt");
    let odd = std::path::PathBuf::from(OsString::from_vec(raw));
    std::fs::write(&odd, CONTENT).unwrap();
    let run = bench.cli_with(
        &[
            OsString::from("add"),
            OsString::from("--json"),
            odd.into_os_string(),
        ],
        T0,
        &Free::new(PLENTY),
        &crate::bench::ClaimsOf::default(),
    );
    assert_eq!(run.code, 0, "{run:?}");
    let attachment = &run.json()["ready"][0]["staged"]["attachment"];
    assert_eq!(attachment["display_name"], "syn-ap11-\u{fffd}.txt");
    assert_eq!(attachment["name"], "syn-ap11-_.txt");
}

#[test]
fn every_exit_code_is_documented() {
    let help = openwiz_cli::help();
    for usage in [
        "openwiz attachment add [--json] [--] <path>...",
        "openwiz attachment ls [--json]",
        "openwiz attachment rm [--json] <id>...",
    ] {
        assert!(help.contains(usage), "{usage:?} not in --help:\n{help}");
    }
    for (code, words) in [
        (
            0,
            "attachment: every path ready, every id removed or expired, or the list printed",
        ),
        (2, "usage error"),
        (
            EXIT_ATTACHMENT_NOT_FOUND,
            "attachment rm: an id was not found",
        ),
        (EXIT_ATTACHMENT_FAILED, "attachment add: a path failed"),
        (
            EXIT_ATTACHMENT_STORE_UNAVAILABLE,
            "attachment: the attachment store could not be used",
        ),
        (EXIT_ATTACHMENT_REFUSED, "attachment rm: refused"),
    ] {
        let line = format!("  {code:<2} {words}");
        assert!(help.contains(&line), "{line:?} not in --help:\n{help}");
    }
    for words in [
        "never sends or uploads anything",
        "24 hours",
        "1 GiB free",
        "read-only",
        "10 MiB",
        "25 MiB",
    ] {
        assert!(help.contains(words), "{words:?} not in --help:\n{help}");
    }
}
