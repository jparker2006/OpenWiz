//! A7 (continued): no `room-cli` diagnostic text (its stderr, and what it
//! prints on stdout where JSON, a version or a recorded value was
//! expected) reaches any server response, error body or log, except a line
//! in 0.0.1's safe grammar. Session content (titles, purposes, final output,
//! a recorded error) is data, shown by design, and not covered here.
//!
//! `room-cli`'s stderr, and text it prints where JSON or a version was
//! expected, could otherwise reach `error.message` and envelope gaps. The
//! sanitizer is the adapter's (`crates/adapters/cloudroom`); these proofs
//! show it through
//! the server: every planted variant, for each read, through every route
//! and option below, in the in-process server's bodies and log and in the
//! built binary's bodies and stderr. Each body still equals
//! `openwiz … --json` (A6), and `every_read_the_server_makes_is_planted`
//! fails if the server starts a read that has no row here.
//!
//! The variants are P4b's (`tests/proof/p04b_source_diagnostics.rs`),
//! written at run time into each test's own directory, over the committed
//! `normal` scenario, so the fixture scan's committed-file rules hold.

use std::io::{BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::a6_semantics::compare;
use crate::fake::Fake;
use crate::http::{self, Response, serve};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/room-cli-0.43.1");

/// Every route, with the CLI arguments (before `--json`) it equals; `None`
/// for `health`, which reads nothing.
const ROUTES: &[(&str, Option<&[&str]>)] = &[
    ("/api/v1/inventory", Some(&["ls"])),
    ("/api/v1/inventory?hidden=1", Some(&["ls", "--hidden"])),
    ("/api/v1/inventory?archived=1", Some(&["ls", "-a"])),
    ("/api/v1/inventory?detail=2", Some(&["ls", "-l"])),
    (
        "/api/v1/inventory?detail=2&hidden=1&archived=1",
        Some(&["ls", "-l", "-a", "--hidden"]),
    ),
    (
        "/api/v1/sessions/thr_syn_0001",
        Some(&["show", "thr_syn_0001"]),
    ),
    (
        "/api/v1/sessions/thr_syn_0002",
        Some(&["show", "thr_syn_0002"]),
    ),
    (
        "/api/v1/sessions/thr_syn_9999",
        Some(&["show", "thr_syn_9999"]),
    ),
    // An id the adapter refuses: one `--version` read, then `400`.
    ("/api/v1/sessions/thr.syn", Some(&["show", "thr.syn"])),
    ("/api/v1/health", None),
];

/// A reason phrase of exactly 60 characters, the longest kept.
const PHRASE_60: &str = "Synthetic reason phrase of exactly sixty letters and spacesx";

/// What 0.0.1 shows when nothing in the line is safe.
const WITHHELD: &str = "room-cli error (detail withheld)";

/// What 0.0.1 shows for a line that contains the not-found line with more
/// after it, where a read's failure is shown. A detail read reports it as
/// not found instead: `404 not_found` for `show`, or OpenWiz's own gap,
/// `thread show reported <id> not found`, in a list or a show.
const NOT_FOUND_WITHHELD: &str = "Error: HTTP 404: Thread not found (detail withheld)";

/// What a failing read prints on stdout; a failed read's stdout is never
/// used.
const FAILED_STDOUT: &str =
    "Synthetic stdout of a failed read at /Users/synthetic-owner/failed-read-stdout";
const FAILED_STDOUT_MARKER: &str = "failed-read-stdout";

/// One planted diagnostic: what the fake prints, the text that must never
/// be shown, and the line shown instead.
struct Planted {
    name: &'static str,
    text: String,
    markers: Vec<String>,
    kept: String,
}

fn planted(name: &'static str, text: &str, markers: &[&str], kept: &str) -> Planted {
    Planted {
        name,
        text: text.to_owned(),
        markers: markers.iter().map(|m| (*m).to_owned()).collect(),
        kept: kept.to_owned(),
    }
}

/// A token-like string, assembled so that no scanner reads it as a real one.
fn token() -> String {
    format!("{}{}", "gh", "p_SyntheticTokenAbcDefGhiJklMnoPqrStu12")
}

/// P4b's planted stderr variants, and the committed diagnostics fixtures'
/// own text.
fn stderr_variants() -> Vec<Planted> {
    let token = token();
    let long_phrase = "Synthetic ".repeat(2_000);
    vec![
        planted(
            "the diagnostics fixtures' text",
            "Synthetic failure reading /synthetic/owner/.syn-room/state.json and ~/synthetic/notes with SYNTHETIC_TOKEN=synthetic-value-12345\n\u{1b}[31mSynthetic second line\u{1b}[0m",
            &[
                "/synthetic/owner",
                "state.json",
                "~/synthetic",
                "SYNTHETIC_TOKEN",
                "synthetic-value-12345",
                "Synthetic second line",
                "Synthetic failure",
                "\u{1b}",
                "[31m",
            ],
            WITHHELD,
        ),
        planted(
            "safe HTTP 500 line",
            "Error: HTTP 500: Synthetic server error",
            &[],
            "Error: HTTP 500: Synthetic server error",
        ),
        planted(
            "absolute macOS path",
            "Error: cannot read /Users/synthetic-owner/Library/syn-room/config.json",
            &[
                "/Users/",
                "synthetic-owner",
                "Library",
                "config.json",
                "cannot read",
            ],
            WITHHELD,
        ),
        planted(
            "absolute Windows path",
            "Error: cannot open C:\\Users\\synthetic-owner\\AppData\\syn-room\\auth.toml",
            &[
                "C:\\",
                "synthetic-owner",
                "AppData",
                "auth.toml",
                "cannot open",
            ],
            WITHHELD,
        ),
        planted(
            "home-relative path",
            "Error: no session file at ~/.syn-room/session.json",
            &["~/", ".syn-room", "session.json", "no session file"],
            WITHHELD,
        ),
        planted(
            "token-like string",
            &format!("Authorization: Bearer {token}"),
            &[&token, "Bearer", "Authorization"],
            WITHHELD,
        ),
        planted(
            "token in an HTTP 401 line",
            &format!("Error: HTTP 401: Unauthorized token {token}"),
            &[&token, "Unauthorized token"],
            "Error: HTTP 401 (detail withheld)",
        ),
        planted(
            "environment dump",
            "ROOM_CLI_TOKEN=synthetic-secret-value HOME=/home/synthetic-owner SHELL=/bin/zsh",
            &[
                "ROOM_CLI_TOKEN",
                "synthetic-secret-value",
                "HOME=",
                "/home/",
                "synthetic-owner",
                "/bin/zsh",
            ],
            WITHHELD,
        ),
        planted(
            "URL with credentials",
            "Error: request to https://synthetic-user:synthetic-pass@example.invalid/v1/threads failed",
            &[
                "https://",
                "synthetic-user",
                "synthetic-pass",
                "example.invalid",
                "/v1/threads",
            ],
            WITHHELD,
        ),
        planted(
            "email address",
            "Error: account synthetic.owner@example.invalid is signed out",
            &["synthetic.owner", "owner@example", "signed out"],
            WITHHELD,
        ),
        planted(
            "multi-line, safe first line",
            "Error: HTTP 502: Bad Gateway\nCaused by: open /Users/synthetic-owner/.syn-room/key.pem\n    at synthetic_frame (src/syn.rs:42:7)",
            &[
                "Caused by",
                "/Users/",
                "key.pem",
                "synthetic_frame",
                "src/syn.rs",
            ],
            "Error: HTTP 502: Bad Gateway",
        ),
        planted(
            "multi-line, unsafe first line",
            "\n   \nError: lookup failed for synthetic-owner\nError: HTTP 500: Synthetic server error",
            &["lookup failed", "synthetic-owner"],
            WITHHELD,
        ),
        planted(
            "terminal escape sequences",
            "\u{1b}[31mError: HTTP 503: Service Unavailable\u{1b}[0m \u{1b}]0;synthetic-window-title\u{7}",
            &[
                "\u{1b}",
                "\u{7}",
                "[31m",
                "[0m",
                "synthetic-window-title",
                "Service Unavailable",
            ],
            "Error: HTTP 503 (detail withheld)",
        ),
        planted(
            "very long line",
            &format!("Error: HTTP 500: {long_phrase}synthetic-tail-secret"),
            &["Synthetic Synthetic", "synthetic-tail-secret"],
            "Error: HTTP 500 (detail withheld)",
        ),
        planted(
            "very long line without a status",
            &"SyntheticLongToken".repeat(1_000),
            &["SyntheticLongToken"],
            WITHHELD,
        ),
        planted(
            "reason phrase of 60 characters",
            &format!("Error: HTTP 500: {PHRASE_60}"),
            &[],
            &format!("Error: HTTP 500: {PHRASE_60}"),
        ),
        planted(
            "reason phrase of 61 characters",
            &format!("Error: HTTP 500: {PHRASE_60}x"),
            &[PHRASE_60],
            "Error: HTTP 500 (detail withheld)",
        ),
        planted(
            "HTTP 4xx line with an unsafe suffix",
            "Error: HTTP 403: Forbidden for /Users/synthetic-owner (token=synthetic-secret-value)",
            &[
                "Forbidden for",
                "/Users/",
                "synthetic-owner",
                "token=",
                "synthetic-secret-value",
            ],
            "Error: HTTP 403 (detail withheld)",
        ),
        planted(
            "digits in the reason phrase",
            "Error: HTTP 429: Retry after 30 seconds for request 8f3a9c",
            &["Retry after", "30 seconds", "8f3a9c"],
            "Error: HTTP 429 (detail withheld)",
        ),
        planted(
            "status in a line of another shape",
            "error: HTTP 500 Internal Server Error at 10.0.0.7:8443",
            // "7:8443", not "8443": a bare number can appear in a real age (age_ms).
            &["Internal Server Error", "10.0.0.7", "7:8443"],
            "Error: HTTP 500 (detail withheld)",
        ),
        // The contract's
        // empty-stderr branch.
        planted("empty stderr", "", &[], "no error output"),
        planted(
            "not-found line with an unsafe suffix",
            "Error: HTTP 404: Thread not found at /Users/synthetic-owner/.syn-room (token=synthetic-secret-value)",
            &[
                "/Users/",
                "synthetic-owner",
                "token=",
                "synthetic-secret-value",
            ],
            NOT_FOUND_WITHHELD,
        ),
    ]
}

/// One read that fails: the start of the argv it answers (every matching
/// argv in `normal` fails), and the routes that must show the kept line.
struct FailingRead {
    read: &'static str,
    argv_prefix: &'static [&'static str],
    hits: &'static [&'static str],
}

const READS: &[FailingRead] = &[
    FailingRead {
        read: "room-cli --version",
        argv_prefix: &["--version"],
        // A failed version read never stops the run, so a refused
        // id (`thr.syn`) is answered as `invalid_request`, which shows nothing
        // of the version read. The leak check still covers that route.
        hits: &["/api/v1/inventory", "/api/v1/sessions/thr_syn_0001"],
    },
    FailingRead {
        read: "thread list",
        argv_prefix: &["thread", "list"],
        hits: &[
            "/api/v1/inventory",
            "/api/v1/inventory?hidden=1",
            "/api/v1/inventory?archived=1",
            "/api/v1/inventory?detail=2",
        ],
    },
    FailingRead {
        read: "project list",
        argv_prefix: &["project", "list"],
        hits: &[
            "/api/v1/inventory",
            "/api/v1/inventory?hidden=1",
            "/api/v1/sessions/thr_syn_0001",
        ],
    },
    FailingRead {
        read: "machine list",
        argv_prefix: &["machine", "list"],
        hits: &["/api/v1/inventory", "/api/v1/sessions/thr_syn_0002"],
    },
    FailingRead {
        read: "thread show",
        argv_prefix: &["thread", "show"],
        hits: &[
            "/api/v1/inventory?detail=2",
            "/api/v1/sessions/thr_syn_0002",
        ],
    },
    FailingRead {
        read: "thread output",
        argv_prefix: &["thread", "output"],
        hits: &["/api/v1/sessions/thr_syn_0002"],
    },
];

/// The committed `normal` scenario's responses, with absolute file paths.
fn normal_responses() -> Vec<Value> {
    let dir = Path::new(FIXTURES).join("normal");
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("scenario.json")).expect("normal scenario"),
    )
    .expect("normal scenario JSON");
    let mut responses = doc["responses"].as_array().expect("responses").clone();
    for response in &mut responses {
        if let Some(file) = response["stdout_file"].as_str() {
            response["stdout_file"] = Value::from(dir.join(file).to_str().expect("utf-8"));
        }
    }
    responses
}

fn argv_of(response: &Value) -> Vec<&str> {
    response["argv"]
        .as_array()
        .expect("argv")
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

/// Points `fake` at a scenario over `normal` whose own `responses` come
/// first.
fn plant(fake: &Fake, responses: Vec<Value>) {
    let normal = Path::new(FIXTURES).join("normal");
    let doc = json!({
        "scenario": "planted",
        "description": "Written at run time by tests/web-api/a7_source_diagnostics.rs.",
        "extends": normal.to_str().expect("utf-8"),
        "responses": responses,
    });
    let file = fake.dir.join("planted-scenario.json");
    std::fs::write(&file, doc.to_string()).expect("planted scenario");
    std::fs::write(
        fake.dir.join("fake-room-cli.scenario"),
        file.to_str().expect("utf-8"),
    )
    .expect("pointer");
}

/// Every argv of `normal` that starts with `prefix` fails with `stderr`
/// and prints [`FAILED_STDOUT`].
fn failing(prefix: &[&str], stderr: &str) -> Vec<Value> {
    normal_responses()
        .iter()
        .filter(|r| argv_of(r).starts_with(prefix))
        .map(|r| {
            json!({
                "argv": r["argv"],
                "exit": 1,
                "stdout": FAILED_STDOUT,
                "stderr": stderr,
            })
        })
        .collect()
}

/// Every key and string value in `value`.
fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                strings(v, out);
            }
        }
        _ => {}
    }
}

/// The server's own status lines, from its fixed reason table. Their
/// reasons are its words, not `room-cli`'s ("Internal Server Error" is also
/// a planted marker), so only the code of one of them is scanned; any other
/// status line is scanned whole.
const STATUS_LINES: &[&str] = &[
    "HTTP/1.1 200 OK",
    "HTTP/1.1 400 Bad Request",
    "HTTP/1.1 404 Not Found",
    "HTTP/1.1 500 Internal Server Error",
    "HTTP/1.1 502 Bad Gateway",
    "HTTP/1.1 503 Service Unavailable",
    "HTTP/1.1 504 Gateway Timeout",
];

/// The raw response (status line, headers and body) and every string of
/// its JSON body, unescaped.
fn texts(r: &Response) -> Vec<String> {
    let raw = String::from_utf8_lossy(&r.body).into_owned();
    let status_line = if STATUS_LINES.contains(&r.status_line.as_str()) {
        format!("HTTP/1.1 {}", r.status)
    } else {
        r.status_line.clone()
    };
    let mut all = vec![status_line, format!("{:?}", r.headers), raw.clone()];
    if let Ok(value) = serde_json::from_str::<Value>(&raw) {
        strings(&value, &mut all);
    }
    all
}

/// The planted markers `texts` shows, each with some context.
fn shown(label: &str, texts: &[String], markers: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for marker in markers {
        if let Some(text) = texts.iter().find(|t| t.contains(marker.as_str())) {
            let at = text.find(marker.as_str()).unwrap_or(0);
            let from = (0..=at.saturating_sub(40))
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(0);
            let context: String = text[from..].chars().take(120).collect();
            found.push(format!(
                "{label} shows the planted {marker:?}: …{context:?}"
            ));
        }
    }
    found
}

/// What one planted scenario must show: its markers never, and on each
/// route in `hits` the kept line (or, for a not-found line, `404`).
struct Expect<'a> {
    label: &'a str,
    markers: &'a [String],
    kept: &'a str,
    hits: &'a [&'a str],
}

/// Every route for one planted scenario, through `get`: no marker shown,
/// a whole body (`Content-Length`), the kept line on each route that makes
/// the read, and, if `parity`, the CLI's `--json` (A6; only in process,
/// where both use the fixed clock).
fn check_routes(
    get: &dyn Fn(&str) -> Response,
    fake: &Fake,
    expect: &Expect,
    parity: bool,
) -> Vec<String> {
    let label = expect.label;
    let mut found = Vec::new();
    for (target, args) in ROUTES {
        let r = get(target);
        let texts = texts(&r);
        found.extend(shown(
            &format!("{label}: GET {target}"),
            &texts,
            expect.markers,
        ));
        if r.header("Content-Length") != Some(r.body.len().to_string().as_str()) {
            found.push(format!(
                "{label}: GET {target}: Content-Length {:?}, but {} body bytes",
                r.header("Content-Length"),
                r.body.len()
            ));
        }
        let not_found = expect.kept.ends_with(NOT_FOUND_WITHHELD)
            && ((r.status == 404 && r.json()["error"]["kind"] == "not_found")
                || texts.iter().any(|t| {
                    t.ends_with(" not found")
                        && (t.starts_with("thread show reported ")
                            || t.starts_with("thread output reported "))
                }));
        let kept = not_found || texts.iter().any(|t| t.contains(expect.kept));
        if expect.hits.contains(target) && !kept {
            found.push(format!(
                "{label}: GET {target} does not show {:?}: {}",
                expect.kept,
                r.text()
            ));
        }
        if let Some(args) = args.filter(|_| parity) {
            let mut argv = args.to_vec();
            argv.push("--json");
            let (code, out, err) = fake.cli(&argv);
            if let Err(panic) = std::panic::catch_unwind(|| {
                compare(target, &argv, &r, code, &out, &err);
            }) {
                let why = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| "a panic".to_owned());
                let why: String = why.chars().take(300).collect();
                found.push(format!("{label}: GET {target} is not the CLI's: {why}"));
            }
        }
    }
    found
}

/// Waits until the log has `n` lines.
fn wait_for_log(s: &http::TestServer, n: usize) {
    let until = Instant::now() + Duration::from_secs(10);
    while s.log.lock().unwrap().len() < n && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
}

fn report(found: Vec<String>) {
    assert!(
        found.is_empty(),
        "{} problem(s):\n{}",
        found.len(),
        found.join("\n")
    );
}

/// Every stderr variant of one failing read, through every route of one
/// in-process server; then its log.
fn check_failing_read(read: &FailingRead) {
    let fake = Fake::new(
        &format!("web_a7_diag_{}", read.read.replace([' ', '-'], "_")),
        "normal",
    );
    let s = serve(&fake);
    let mut found = Vec::new();
    let mut all_markers = vec![
        fake.dir.display().to_string(),
        FAILED_STDOUT_MARKER.to_owned(),
    ];
    let cases = stderr_cases(read);
    for case in &cases {
        found.extend(case.check(&|target| s.get(target), &fake, true));
        all_markers.extend(case.markers.clone());
    }
    let requests = cases.len() * ROUTES.len();
    wait_for_log(&s, requests);
    let log = s.log_text();
    if log.lines().count() != requests {
        found.push(format!(
            "{}: {} log lines, not {requests}",
            read.read,
            log.lines().count()
        ));
    }
    found.extend(shown(
        &format!("{}: the log", read.read),
        &[log],
        &all_markers,
    ));
    report(found);
}

#[test]
fn version_read_diagnostics_reach_no_response_or_log() {
    check_failing_read(&READS[0]);
}

#[test]
fn thread_list_diagnostics_reach_no_response_or_log() {
    check_failing_read(&READS[1]);
}

#[test]
fn project_list_diagnostics_reach_no_response_or_log() {
    check_failing_read(&READS[2]);
}

#[test]
fn machine_list_diagnostics_reach_no_response_or_log() {
    check_failing_read(&READS[3]);
}

#[test]
fn thread_show_diagnostics_reach_no_response_or_log() {
    check_failing_read(&READS[4]);
}

#[test]
fn thread_output_diagnostics_reach_no_response_or_log() {
    check_failing_read(&READS[5]);
}

/// One planted scenario: what the fake answers, and what must show.
struct Case {
    label: String,
    responses: Vec<Value>,
    markers: Vec<String>,
    kept: String,
    hits: &'static [&'static str],
}

impl Case {
    fn check(&self, get: &dyn Fn(&str) -> Response, fake: &Fake, parity: bool) -> Vec<String> {
        plant(fake, self.responses.clone());
        let mut markers = self.markers.clone();
        markers.push(FAILED_STDOUT_MARKER.to_owned());
        markers.push(fake.dir.display().to_string());
        check_routes(
            get,
            fake,
            &Expect {
                label: &self.label,
                markers: &markers,
                kept: &self.kept,
                hits: self.hits,
            },
            parity,
        )
    }
}

/// Every stderr variant of one failing read.
fn stderr_cases(read: &FailingRead) -> Vec<Case> {
    stderr_variants()
        .into_iter()
        .map(|variant| {
            let mut markers = variant.markers;
            // A failed version read keeps safe facts only, so even the
            // sanitizer's safe line is never shown for it.
            let kept = if read.read == "room-cli --version" {
                markers.push(variant.kept.clone());
                "exited 1 (detail withheld)".to_owned()
            } else {
                format!("exited 1: {}", variant.kept)
            };
            Case {
                label: format!("{} / {}", read.read, variant.name),
                responses: failing(read.argv_prefix, &variant.text),
                markers,
                kept,
                hits: read.hits,
            }
        })
        .collect()
}

/// Text `room-cli` prints on stdout where a version, JSON or a recorded
/// value is expected: only `N.N.N`, OpenWiz's own words and recognised
/// labels reach a response. And stderr printed by reads that succeed,
/// which is never used.
fn stdout_cases() -> Vec<Case> {
    let token = token();
    let leak = format!(
        "/Users/synthetic-owner/.syn-room {token} https://synthetic-user:synthetic-pass@example.invalid"
    );
    let normal = Path::new(FIXTURES).join("normal");
    let read_json = |name: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(normal.join(name)).expect(name)).expect(name)
    };
    let mut show = read_json("thread-show-0002.json");
    show["thread"]["id"] = Value::from(leak.clone());
    let mut rows = read_json("thread-list.json");
    rows[0]["visibility"] = Value::from(leak.clone());
    rows[5]["executionTarget"] = Value::from(leak.clone());
    // `normal`'s two not-found answers (exit 1) are left to `normal`.
    let succeeding: Vec<Value> = normal_responses()
        .into_iter()
        .filter(|r| r.get("exit").is_none())
        .map(|mut r| {
            r["stderr"] = Value::from(format!("Synthetic warning: {leak}"));
            r
        })
        .collect();
    let case = |label: &str, responses: Vec<Value>, kept: &str, hits| Case {
        label: label.to_owned(),
        responses,
        markers: stdout_markers(),
        kept: kept.to_owned(),
        hits,
    };
    vec![
        case(
            "version text",
            vec![json!({"argv": ["--version"], "stdout": format!("room-cli 0.43.1 ({leak})\n")})],
            "printed a version that is not N.N.N or v<N> (detail withheld)",
            &["/api/v1/inventory", "/api/v1/sessions/thr_syn_0001"],
        ),
        case(
            "thread list that is not JSON",
            vec![json!({
                "argv": ["thread", "list", "--json"],
                "stdout": format!("Synthetic text that is not JSON: {leak}\n"),
            })],
            "not JSON (a syntax error at line 1 column",
            &["/api/v1/inventory", "/api/v1/inventory?detail=2"],
        ),
        case(
            "thread show of another session",
            vec![json!({
                "argv": ["thread", "show", "thr_syn_0002", "--json"],
                "stdout": show.to_string(),
            })],
            "a different session (id withheld)",
            &[
                "/api/v1/inventory?detail=2",
                "/api/v1/sessions/thr_syn_0002",
            ],
        ),
        case(
            "unrecognised values in thread list",
            vec![json!({"argv": ["thread", "list", "--json"], "stdout": rows.to_string()})],
            "has a value that is not recognised (value withheld)",
            &["/api/v1/inventory"],
        ),
        // Every read that succeeds in `normal` answers as there, exit 0,
        // and prints stderr.
        case("stderr on reads that succeed", succeeding, "", &[]),
    ]
}

fn stdout_markers() -> Vec<String> {
    let token = token();
    [
        "/Users/",
        "synthetic-owner",
        ".syn-room",
        &token,
        "https://",
        "synthetic-pass",
    ]
    .iter()
    .map(|m| (*m).to_owned())
    .collect()
}

#[test]
fn unexpected_stdout_reaches_no_response_or_log() {
    let fake = Fake::new("web_a7_diag_stdout", "normal");
    let s = serve(&fake);
    let mut found = Vec::new();
    let mut all_markers = vec![fake.dir.display().to_string()];
    let cases = stdout_cases();
    for case in &cases {
        found.extend(case.check(&|target| s.get(target), &fake, true));
        all_markers.extend(case.markers.clone());
    }
    let requests = cases.len() * ROUTES.len();
    wait_for_log(&s, requests);
    let log = s.log_text();
    if log.lines().count() != requests {
        found.push(format!("{} log lines, not {requests}", log.lines().count()));
    }
    found.extend(shown("the log", &[log], &all_markers));
    report(found);
}

/// Every read the server makes, on every route, is one `READS` plants, and
/// every `READS` row is made by some route: a new read fails here until it
/// has a row.
#[test]
fn every_read_the_server_makes_is_planted() {
    let fake = Fake::new("web_a7_diag_reads", "normal");
    let s = serve(&fake);
    for (target, _) in ROUTES {
        s.get(target);
    }
    let made = fake.invocations();
    let unplanted: Vec<&Vec<String>> = made
        .iter()
        .filter(|argv| {
            !READS.iter().any(|r| {
                argv.iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .starts_with(r.argv_prefix)
            })
        })
        .collect();
    assert!(
        unplanted.is_empty(),
        "reads with no READS row: {unplanted:?}"
    );
    for read in READS {
        assert!(
            made.iter().any(|argv| argv
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .starts_with(read.argv_prefix)),
            "no route makes {}",
            read.read
        );
    }
}

/// The built binary, with the fake first on `PATH`: every stderr variant of
/// every read, and the stdout cases, through every route, with the same
/// checks as in process (markers, whole bodies, the kept line) except CLI
/// parity: the binary uses the system clock, so its snapshot times differ.
/// Neither its stdout nor its stderr (its log) shows a marker.
#[test]
fn the_binary_shows_no_diagnostics_in_responses_or_its_log() {
    let program = http::server_binary();
    let fake = Fake::new("web_a7_diag_binary", "normal");
    let path = format!(
        "{}:{}",
        fake.dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(&program)
        .args(["--port", "0"])
        .env("PATH", path)
        .current_dir(&fake.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start openwiz-server");
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut first = String::new();
    stderr.read_line(&mut first).unwrap();
    let addr: Option<SocketAddr> = first
        .trim()
        .strip_prefix("openwiz-server: listening on http://")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|a| a.parse().ok());
    let mut all_markers = stdout_markers();
    all_markers.push(fake.dir.display().to_string());
    all_markers.push(FAILED_STDOUT_MARKER.to_owned());
    let mut cases: Vec<Case> = READS.iter().flat_map(stderr_cases).collect();
    cases.extend(stdout_cases());
    for case in &cases {
        all_markers.extend(case.markers.clone());
    }
    let requests = cases.len() * ROUTES.len();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let addr = addr.unwrap_or_else(|| panic!("unexpected first line {first:?}"));
        let mut found = Vec::new();
        for case in &cases {
            let get = |target: &str| http::get(addr, target);
            for problem in case.check(&get, &fake, false) {
                found.push(format!("binary: {problem}"));
            }
        }
        found
    }));
    // The log lines can trail the responses by a moment: read them all
    // before stopping the server.
    let mut rest = String::new();
    if result.is_ok() {
        for _ in 0..requests {
            let _ = stderr.read_line(&mut rest);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = stderr.read_to_string(&mut rest);
    let mut stdout = String::new();
    let _ = child.stdout.take().unwrap().read_to_string(&mut stdout);
    let mut found = match result {
        Ok(found) => found,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    if !stdout.is_empty() {
        found.push(format!("binary stdout: {stdout:?}"));
    }
    if rest.lines().count() != requests {
        found.push(format!(
            "binary: {} log lines, not {requests}",
            rest.lines().count()
        ));
    }
    for line in rest.lines() {
        let ok = line.strip_prefix("openwiz-server: ").is_some_and(|entry| {
            let parts: Vec<&str> = entry.split(' ').collect();
            parts.len() == 4 && parts[0] == "GET" && parts[3].ends_with("ms")
        });
        if !ok {
            found.push(format!(
                "binary: a log line that is not a request line: {line:?}"
            ));
        }
    }
    found.extend(shown("binary: its stderr", &[rest], &all_markers));
    report(found);
}

/// A failed version read never stops a run, so
/// its failure lands in a `200` envelope (`source.version` and a gap, which
/// the viewer renders). Only safe facts reach it, never the source's text,
/// even a line in the sanitizer's general grammar; nor the server's log.
#[test]
fn a_failed_version_read_never_reaches_a_200_envelope() {
    const MARKER: &str = "Synthetic private version marker";
    let fake = Fake::new("web_a7_failed_version", "version-failed");
    let s = serve(&fake);
    let targets = [
        "/api/v1/inventory",
        "/api/v1/inventory?detail=2&hidden=1&archived=1",
        "/api/v1/sessions/thr_syn_0001",
    ];
    for target in targets {
        let r = s.get(target);
        assert_eq!(r.status, 200, "{target}: {}", r.text());
        assert!(!r.text().contains(MARKER), "{target}: {}", r.text());
        let body = r.json();
        let why = body["source"]["version"]["unknown"]
            .as_str()
            .unwrap_or_default();
        assert_eq!(
            why, "room-cli --version exited 1 (detail withheld)",
            "{target}"
        );
        assert!(
            body["gaps"]
                .as_array()
                .is_some_and(|g| g.iter().any(|g| g["detail"] == why)),
            "{target}: {body}"
        );
    }
    wait_for_log(&s, targets.len());
    assert!(!s.log_text().contains(MARKER), "{}", s.log_text());
}
