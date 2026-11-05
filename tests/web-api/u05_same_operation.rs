use serde_json::{Value, json};

use crate::new_support::{
    NO_REMOTE, NewFake, OPTIONS, PERSONAL, PROJECT, TEXT, THREAD, body, body_with, cli_args,
    cli_json, create, file, preview_json, serve_new, status,
};

struct Case {
    name: &'static str,
    responses: Vec<Value>,
    project: &'static str,
    body: Value,
}

fn show(name: &str) -> Value {
    json!({"argv": ["thread", "show", THREAD, "--json"], "stdout_file": file(name)})
}

fn catalogue(name: &str) -> Value {
    json!({"argv": ["cloud", "status", "--json"], "stdout_file": file(name)})
}

fn spawn(extra: Value) -> Value {
    let mut out = json!({"any_spawn": true});
    for (k, v) in extra.as_object().unwrap() {
        out[k] = v.clone();
    }
    out
}

fn cases() -> Vec<Case> {
    let op = |n: &str| format!("u5-{n}");
    let plain = |name: &'static str, responses: Vec<Value>| Case {
        name,
        responses,
        project: PROJECT,
        body: body(&op(name), TEXT),
    };
    let mut out = vec![plain("created-active", vec![])];
    for (name, fixture) in [
        ("created-pending", "thread-show-pending.json"),
        ("created-starting", "thread-show-starting.json"),
        ("created-idle", "thread-show-idle.json"),
        ("created-start-failed", "thread-show-error.json"),
        ("created-differs", "thread-show-differs.json"),
        ("created-unknown-status", "thread-show-unknown-status.json"),
        ("created-show-drift", "thread-show-drift.json"),
    ] {
        out.push(plain(name, vec![show(fixture)]));
    }
    out.push(plain(
        "created-show-fails",
        vec![json!({
            "argv": ["thread", "show", THREAD, "--json"],
            "stderr": "Error: HTTP 500",
            "exit": 1,
        })],
    ));
    for (name, harness, model, effort) in [
        ("not-offered-model", "codex", "syn-codex-model-99", "high"),
        (
            "not-offered-effort",
            "claude-code",
            "syn-claude-model-01",
            "ultra",
        ),
        (
            "not-offered-harness",
            "cursor",
            "syn-cursor-model-01",
            "high",
        ),
        ("unknown-harness", "synthetic-harness", "m", "high"),
    ] {
        out.push(Case {
            name,
            responses: vec![],
            project: PROJECT,
            body: body_with(&op(name), harness, model, effort, TEXT),
        });
    }
    for (name, project) in [
        ("no-remote", NO_REMOTE),
        ("personal", PERSONAL),
        ("not-github", "prj_syn_0006"),
        ("no-such-project", "prj_syn_9999"),
    ] {
        out.push(Case {
            name,
            responses: vec![],
            project,
            body: body(&op(name), TEXT),
        });
    }
    for (name, fixture) in [
        ("catalogue-not-ready", "cloud-status-not-ready.json"),
        (
            "catalogue-harnesses-null",
            "cloud-status-harnesses-null.json",
        ),
        ("catalogue-drift-array", "cloud-status-drift-array.json"),
        (
            "catalogue-drift-models",
            "cloud-status-drift-models-object.json",
        ),
    ] {
        out.push(plain(name, vec![catalogue(fixture)]));
    }
    out.push(plain(
        "projects-not-read",
        vec![json!({
            "argv": ["project", "list", "--json", "--include-personal"],
            "stderr": "Error: HTTP 503",
            "exit": 1,
        })],
    ));
    out.push(plain(
        "provider-refusal",
        vec![spawn(json!({
            "stderr": "Error: Failed to create thread: HTTP 400: This model is unavailable on Cloud",
            "exit": 1,
        }))],
    ));
    for (name, response) in [
        ("uncertain-abort", json!({"creates": true, "abort": true})),
        (
            "uncertain-not-json",
            json!({"creates": true, "stdout_file": file("spawn-receipt-not-json.txt")}),
        ),
        (
            "uncertain-undocumented",
            json!({
                "creates": true,
                "stderr": "Error: Failed to create thread: HTTP 500: Synthetic server error",
                "exit": 1,
            }),
        ),
        (
            "receipt-drift-array",
            json!({"creates": true, "stdout_file": file("spawn-receipt-drift-array.json")}),
        ),
        (
            "receipt-drift-no-id",
            json!({"creates": true, "stdout_file": file("spawn-receipt-drift-no-id.json")}),
        ),
        (
            "receipt-drift-other-project",
            json!({"creates": true, "stdout_file": file("spawn-receipt-drift-other-project.json")}),
        ),
        (
            "receipt-drift-unsafe-id",
            json!({"creates": true, "stdout_file": file("spawn-receipt-drift-unsafe-id.json")}),
        ),
    ] {
        out.push(plain(name, vec![spawn(response)]));
    }
    out
}

/// The status the contract gives a CLI exit of the new-thread commands.
fn http_status(exit: i32) -> u16 {
    match exit {
        0 | 5 | 6 | 7 | 8 | 11 | 12 | 13 => 200,
        2 => 400,
        10 => 500,
        other => panic!("no contract status for exit {other}"),
    }
}

#[test]
fn every_create_preview_and_status_is_the_clis_json() {
    for case in cases() {
        let cli = NewFake::with(&format!("web_u5_cli_{}", case.name), case.responses.clone());
        let web = NewFake::with(&format!("web_u5_web_{}", case.name), case.responses.clone());
        let s = serve_new(&web);
        let op = case.body["op_id"].as_str().unwrap().to_owned();

        // The preview, before anything is created.
        let (code, want) = cli_json(&cli, &cli_args(case.project, &case.body, false));
        let r = preview_json(&s, case.project, &case.body);
        assert_eq!(
            r.status,
            http_status(code),
            "{} preview: {}",
            case.name,
            r.text()
        );
        assert_eq!(r.json(), want, "{} preview", case.name);
        assert!(web.writes().is_empty(), "{}: the preview wrote", case.name);
        assert!(!web.any_record(), "{}: the preview recorded", case.name);

        // The create, then its replay.
        for round in ["create", "replay"] {
            let (code, want) = cli_json(&cli, &cli_args(case.project, &case.body, true));
            let r = create(&s, case.project, &case.body);
            assert_eq!(
                r.status,
                http_status(code),
                "{} {round}: {}",
                case.name,
                r.text()
            );
            assert_eq!(r.json(), want, "{} {round}", case.name);
            if round == "replay" {
                assert_eq!(
                    r.json()["result"]["replayed"],
                    true,
                    "{}: {}",
                    case.name,
                    r.text()
                );
            }
        }
        assert_eq!(
            cli.spawn_count(),
            web.spawn_count(),
            "{}: the same calls",
            case.name
        );
        assert!(web.spawn_count() <= 1, "{}: at most one call", case.name);

        // The status of that op id.
        let (code, want) = cli_json(
            &cli,
            &["new".into(), "--status".into(), op.clone(), "--json".into()],
        );
        let r = s.get(&status(&op));
        assert_eq!(
            r.status,
            http_status(code),
            "{} status: {}",
            case.name,
            r.text()
        );
        assert_eq!(r.json(), want, "{} status", case.name);

        // A conflicting reuse of the op id.
        let mut other = case.body.clone();
        other["text"] = json!("Synthetic A2 different prompt");
        let (code, want) = cli_json(&cli, &cli_args(case.project, &other, true));
        let r = create(&s, case.project, &other);
        assert_eq!(
            r.status,
            http_status(code),
            "{} reuse: {}",
            case.name,
            r.text()
        );
        assert_eq!(r.json(), want, "{} reuse", case.name);
        assert_eq!(cli.spawn_count(), web.spawn_count(), "{}", case.name);
    }
}

#[test]
fn a_status_with_no_record_is_the_clis() {
    let cli = NewFake::new("web_u5_cli_no_record");
    let web = NewFake::new("web_u5_web_no_record");
    let s = serve_new(&web);
    let (code, want) = cli_json(
        &cli,
        &[
            "new".into(),
            "--status".into(),
            "u5-none".into(),
            "--json".into(),
        ],
    );
    assert_eq!(code, 11);
    let r = s.get(&status("u5-none"));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json(), want);
    assert!(web.invocations().is_empty(), "no record: nothing to read");
}

#[test]
fn every_catalogue_is_the_clis_models() {
    for name in [
        "cloud-status.json",
        "cloud-status-not-ready.json",
        "cloud-status-harnesses-null.json",
        "cloud-status-drift-array.json",
        "cloud-status-drift-levels-not-list.json",
        "cloud-status-drift-models-object.json",
        "cloud-status-drift-ready-not-bool.json",
    ] {
        let test = name.trim_end_matches(".json");
        let cli = NewFake::with(&format!("web_u5_cli_{test}"), vec![catalogue(name)]);
        let web = NewFake::with(&format!("web_u5_web_{test}"), vec![catalogue(name)]);
        let s = serve_new(&web);
        let (code, out, err) = cli.cli(&["models", "--json"]);
        let want: Value =
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("{name}: {e}: {err}"));
        let r = s.get(OPTIONS);
        assert_eq!(r.status, http_status(code), "{name}: {}", r.text());
        assert_eq!(r.json(), want, "{name}");
        assert_eq!(
            web.invocations(),
            vec![vec![
                "cloud".to_owned(),
                "status".to_owned(),
                "--json".to_owned()
            ]],
            "{name}: one catalogue read"
        );
    }
    // A failed read is the CLI's too.
    let failing =
        json!({"argv": ["cloud", "status", "--json"], "stderr": "Error: HTTP 503", "exit": 1});
    let cli = NewFake::with("web_u5_cli_catalogue_fails", vec![failing.clone()]);
    let web = NewFake::with("web_u5_web_catalogue_fails", vec![failing]);
    let s = serve_new(&web);
    let (code, out, _) = cli.cli(&["models", "--json"]);
    assert_eq!(code, 13);
    let r = s.get(OPTIONS);
    assert_eq!(r.status, 200);
    assert_eq!(r.json(), serde_json::from_str::<Value>(&out).unwrap());
}

#[test]
fn a_refused_request_carries_the_cores_words() {
    let cli = NewFake::new("web_u5_cli_refused");
    let web = NewFake::new("web_u5_web_refused");
    let s = serve_new(&web);
    for value in [
        body("u5 bad op", TEXT),
        body("u5-blank", " "),
        body_with("u5-setting", "codex", "a b", "high", TEXT),
    ] {
        let args = cli_args(PROJECT, &value, true);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, out, err) = cli.cli(&args);
        assert_eq!(code, 2, "{value}: {out}");
        let r = create(&s, PROJECT, &value);
        assert_eq!(r.status, 400, "{}", r.text());
        let (kind, message) = r.error();
        assert_eq!(kind, "invalid_request");
        assert!(
            err.contains(&format!("openwiz: {message}; nothing was created")),
            "the CLI said {err:?}, the route {message:?}"
        );
    }
    assert!(web.invocations().is_empty());
}

/// The server works out no option, effect, outcome or stage: it maps the
/// CLI's exit codes to statuses, and reads a created thread's start once.
#[test]
fn the_server_has_no_new_thread_logic_of_its_own() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src");
    let mut seen_new_thread = false;
    for entry in std::fs::read_dir(dir).expect("server sources") {
        let path = entry.expect("entry").path();
        let text = std::fs::read_to_string(&path).expect("source");
        if text.contains("new_thread") {
            seen_new_thread = true;
        }

        if path.ends_with("attachments.rs") {
            assert!(
                !text.contains("new_thread::"),
                "{path:?} reaches into new_thread"
            );
            continue;
        }
        for word in [
            "effects_for",
            "resolve_project",
            "follow_next",
            "follow_start",
            "StartStage",
            "OptionsState",
            "RefusalReason",
            "FailReason",
            "UnavailableReason",
            "NotOffered",
            "PreviewVerdict",
            "Status::",
            "HarnessOption",
            "ModelOption",
            ".offered(",
            "Binding",
        ] {
            assert!(
                !text.contains(word),
                "{path:?} works out new-thread rules: {word}"
            );
        }
        for (i, _) in text.match_indices("Outcome::") {
            assert!(
                text[i..].starts_with("Outcome::Created"),
                "{path:?}: an outcome other than Created is looked at"
            );
        }
    }
    assert!(seen_new_thread, "the server calls the core's new_thread");
}
