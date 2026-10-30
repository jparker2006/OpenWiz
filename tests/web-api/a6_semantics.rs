//! A6: inventory semantics are unchanged. For every 0.0.1 fixture scenario,
//! each API body is byte for byte what `openwiz … --json` prints for the
//! same options and the same (fixed) clock, and passes the schema check.
//! Each error carries the CLI's own message, under the status the contract
//! gives the CLI's exit code. 0.0.1's P1 to P12 run unchanged in
//! `tests/proof`.

use std::thread;

use crate::fake::Fake;
use crate::http::{CLI_EXIT_TO_HTTP, Response, TestServer, serve};
use crate::schema;

/// The API target and the CLI arguments (before `--json`) that must agree.
fn list_cases() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("/api/v1/inventory", vec!["ls"]),
        (
            "/api/v1/inventory?detail=1&hidden=0&archived=0&limit=100&offset=0",
            vec!["ls"],
        ),
        ("/api/v1/inventory?detail=2", vec!["ls", "-l"]),
        ("/api/v1/inventory?archived=1", vec!["ls", "-a"]),
        ("/api/v1/inventory?hidden=1", vec!["ls", "--hidden"]),
        (
            "/api/v1/inventory?hidden=1&detail=2&archived=1",
            vec!["ls", "-l", "-a", "--hidden"],
        ),
        (
            "/api/v1/inventory?limit=2&offset=1",
            vec!["ls", "--limit", "2", "--offset", "1"],
        ),
        (
            "/api/v1/inventory?offset=3&limit=2&detail=2",
            vec!["ls", "-l", "--limit", "2", "--offset", "3"],
        ),
        ("/api/v1/inventory?limit=0", vec!["ls", "--limit", "0"]),
        (
            "/api/v1/inventory?offset=100",
            vec!["ls", "--offset", "100"],
        ),
        ("/api/v1/inventory?limit=200", vec!["ls", "--limit", "200"]),
        ("/api/v1/inventory?limit=201", vec!["ls", "--limit", "201"]),
    ]
}

fn show_cases() -> Vec<(String, Vec<String>)> {
    let mut ids: Vec<String> = (1..=8).map(|n| format!("thr_syn_{n:04}")).collect();
    ids.push("thr_syn_9999".to_owned());
    ids.push("thr%20syn".to_owned());
    ids.push("a".repeat(129));
    ids.into_iter()
        .map(|id| {
            (
                format!("/api/v1/sessions/{id}"),
                vec!["show".to_owned(), id],
            )
        })
        .collect()
}

/// The server's response to `target` equals the CLI's run of `args`.
fn assert_matches(server: &TestServer, fake: &Fake, target: &str, args: &[&str]) {
    let response = server.get(target);
    let mut argv: Vec<&str> = args.to_vec();
    argv.push("--json");
    let (code, out, err) = fake.cli(&argv);
    compare(target, &argv, &response, code, &out, &err);
}

pub(crate) fn compare(target: &str, argv: &[&str], r: &Response, code: i32, out: &str, err: &str) {
    if code == 0 {
        assert_eq!(r.status, 200, "{target}: {}", r.text());
        assert_eq!(
            r.text(),
            out,
            "{target}: the body differs from openwiz {argv:?}"
        );
        let errors = schema::validate(&schema::load_schema(), &r.json());
        assert!(errors.is_empty(), "{target}: schema: {errors:?}");
    } else {
        let (_, status, kind) = CLI_EXIT_TO_HTTP
            .iter()
            .find(|(exit, ..)| *exit == code)
            .unwrap_or_else(|| panic!("openwiz {argv:?} exited {code}: {err}"));
        assert_eq!(r.status, *status, "{target} (exit {code}): {}", r.text());
        let message = err
            .strip_prefix("openwiz: ")
            .and_then(|m| m.strip_suffix('\n'))
            .unwrap_or_else(|| panic!("unexpected stderr {err:?}"));
        assert_eq!(
            r.error(),
            ((*kind).to_owned(), message.to_owned()),
            "{target}: the error is not the CLI's"
        );
    }
}

#[test]
fn every_normal_case_matches_the_cli() {
    let fake = Fake::new("web_a6_normal", "normal");
    let server = serve(&fake);
    for (target, args) in list_cases() {
        assert_matches(&server, &fake, target, &args);
    }
    for (target, args) in show_cases() {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_matches(&server, &fake, &target, &args);
    }
}

/// The scenarios whose reads all answer: errors, drift, extra fields and
/// the untested version.
const ANSWERING: &[&str] = &[
    "project-list-error",
    "project-removed",
    "show-error",
    "show-missing-fields",
    "show-output-error",
    "thread-list-error",
    "thread-list-missing-fields",
    "thread-list-not-json",
    "thread-list-truncated",
    "thread-list-wrong-types",
    "unknown-fields",
    "version-0.44.0",
    "version-0.43.1",
    "version-unrecognised",
];

#[test]
fn every_error_drift_and_version_case_matches_the_cli() {
    for scenario in ANSWERING {
        let fake = Fake::new(&format!("web_a6_{scenario}"), scenario);
        let server = serve(&fake);
        for (target, args) in [
            ("/api/v1/inventory", vec!["ls"]),
            ("/api/v1/inventory?detail=2", vec!["ls", "-l"]),
            (
                "/api/v1/inventory?detail=2&hidden=1&archived=1",
                vec!["ls", "-l", "--hidden", "-a"],
            ),
            (
                "/api/v1/sessions/thr_syn_0001",
                vec!["show", "thr_syn_0001"],
            ),
            (
                "/api/v1/sessions/thr_syn_0006",
                vec!["show", "thr_syn_0006"],
            ),
            (
                "/api/v1/sessions/thr_syn_9999",
                vec!["show", "thr_syn_9999"],
            ),
        ] {
            assert_matches(&server, &fake, target, &args);
        }
    }
}

#[test]
fn every_timeout_case_matches_the_cli() {
    // Each case waits for a 15-second read timeout, so they run at once,
    // each with its own fake.
    let mut cases = Vec::new();
    for scenario in ["thread-list-timeout", "show-timeout", "show-output-timeout"] {
        for (n, (target, args)) in [
            ("/api/v1/inventory", vec!["ls"]),
            ("/api/v1/inventory?detail=2", vec!["ls", "-l"]),
            (
                "/api/v1/sessions/thr_syn_0001",
                vec!["show", "thr_syn_0001"],
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let name = format!("web_a6_{scenario}_{n}");
            cases.push(thread::spawn(move || {
                let fake = Fake::new(&name, scenario);
                let server = serve(&fake);
                let by_server = {
                    let addr = server.addr;
                    let target = target.to_owned();
                    thread::spawn(move || crate::http::get(addr, &target))
                };
                let mut argv = args.clone();
                argv.push("--json");
                let (code, out, err) = fake.cli(&argv);
                let r = by_server.join().expect("server request");
                compare(target, &argv, &r, code, &out, &err);
            }));
        }
    }
    for case in cases {
        case.join().expect("timeout case");
    }
}
