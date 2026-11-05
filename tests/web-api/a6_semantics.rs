//! A6: inventory semantics are unchanged. For every 0.0.1 fixture scenario,
//! each inventory and session body is byte for byte what `openwiz … --json` prints for the
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
                compare_timeout(target, &argv, &r, code, &out, &err);
            }));
        }
    }
    for case in cases {
        case.join().expect("timeout case");
    }
}

/// [`compare`] for a timeout case. The server and the CLI each measure
/// their own read timeout, so "timed out after N ms" may differ by a
/// millisecond: N is ignored on both sides, everything else is
/// compared byte for byte.
fn compare_timeout(target: &str, argv: &[&str], r: &Response, code: i32, out: &str, err: &str) {
    let r = Response {
        body: any_ms(&r.text()).into_bytes(),
        ..r.clone()
    };
    compare(target, argv, &r, code, &any_ms(out), &any_ms(err));
}

fn map_ms(s: &str, f: impl Fn(u64) -> String) -> String {
    const AT: &str = "timed out after ";
    const UNIT: &str = " ms";
    let mut rest = s;
    let mut mapped = String::new();
    while let Some(i) = rest.find(AT) {
        let (head, tail) = rest.split_at(i + AT.len());
        let digits = tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        mapped.push_str(head);
        if digits > 0 && tail[digits..].starts_with(UNIT) {
            let ms: u64 = tail[..digits].parse().expect("a millisecond count");
            mapped.push_str(&f(ms));
            rest = &tail[digits..];
        } else {
            rest = tail;
        }
    }
    mapped.push_str(rest);
    mapped
}

#[test]
fn only_the_millisecond_count_is_ignored() {
    assert_eq!(
        any_ms("thread list timed out after 15000 ms"),
        any_ms("thread list timed out after 15001 ms")
    );
    for (a, b) in [
        (
            "thread list timed out after 3 retries",
            "thread list timed out after 4 retries",
        ),
        (
            "thread list timed out after 15 s",
            "thread list timed out after 16 s",
        ),
        (
            "thread list timed out after 15000ms",
            "thread list timed out after 15001ms",
        ),
        (
            "thread list timed out after 15000 ms",
            "thread show timed out after 15000 ms",
        ),
        (
            "thread list timed out after ms",
            "thread list timed out after 1 ms",
        ),
    ] {
        assert_ne!(any_ms(a), any_ms(b), "{a:?} vs {b:?} must stay distinct");
    }
}

fn any_ms(s: &str) -> String {
    map_ms(s, |_| "N".to_owned())
}

fn one_ms_later(s: &str) -> String {
    map_ms(s, |ms| (ms + 1).to_string())
}

#[test]
fn a_timeout_one_ms_apart_still_matches() {
    let cases = [
        ("thread-list-timeout", "/api/v1/inventory", vec!["ls"]),
        (
            "show-timeout",
            "/api/v1/inventory?detail=2",
            vec!["ls", "-l"],
        ),
    ]
    .map(|(scenario, target, args)| {
        thread::spawn(move || {
            let fake = Fake::new(&format!("web_a6_one_ms_{scenario}"), scenario);
            let mut argv = args;
            argv.push("--json");
            let (code, out, err) = fake.cli(&argv);
            assert!(
                out.contains("timed out after ") || err.contains("timed out after "),
                "{scenario}: no timeout in {out}{err}"
            );
            let body = if code == 0 {
                one_ms_later(&out)
            } else {
                let message = err.strip_prefix("openwiz: ").expect("stderr").trim_end();
                serde_json::json!({ "error": { "kind": "read_failed", "message": one_ms_later(message) } })
                    .to_string()
            };
            let r = Response {
                status: if code == 0 { 200 } else { 502 },
                status_line: String::new(),
                headers: Vec::new(),
                body: body.into_bytes(),
            };
            compare_timeout(target, &argv, &r, code, &out, &err);
        })
    });
    for case in cases {
        case.join().expect("one ms apart");
    }
    // Only the number is ignored: which read timed out still counts.
    assert_eq!(
        any_ms("thread list timed out after 15000 ms"),
        any_ms("thread list timed out after 15001 ms")
    );
    assert_ne!(
        any_ms("thread list timed out after 15000 ms"),
        any_ms("thread show timed out after 15000 ms")
    );
}
