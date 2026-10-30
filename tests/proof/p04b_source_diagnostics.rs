//! P4 (continued) Source diagnostics: text `room-cli` prints when a read
//! fails, or prints where JSON or a version was expected, never reaches
//! OpenWiz's output, except a line in the safe grammar.
//!
//! New in 0.0.2, for the web API's rule (A7) that errors and logs never
//! include paths, the environment, stack traces or credentials. The safe
//! grammar is provisional:
//!
//! - `Error: HTTP <3 digits>`, optionally `: ` and a reason phrase of ASCII
//!   letters and single spaces (at most 60 characters), is kept as it is;
//! - otherwise, if the line has `HTTP <3 digits>`, it becomes
//!   `Error: HTTP <code> (detail withheld)`;
//! - otherwise `room-cli error (detail withheld)`.
//!
//! Each failing read is checked through the core, the in-process `openwiz`
//! and the release `openwiz` binary, in text and `--json`. The committed
//! fixtures (`tests/fixtures/room-cli-0.43.1/diagnostics-*`) hold what the
//! fixture scan allows; the variants it forbids in committed files are
//! written here, at run time, into each test's own directory.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use openwiz_core::ops::{ListRequest, OpError};
use serde_json::{Value, json};

use crate::support::*;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/room-cli-0.43.1");

/// A reason phrase of exactly 60 characters, the longest kept.
const PHRASE_60: &str = "Synthetic reason phrase of exactly sixty letters and spacesx";

/// What the sanitizer prints when nothing in the line is safe.
const WITHHELD: &str = "room-cli error (detail withheld)";

/// One planted diagnostic: what the fake prints, the text that must never
/// be shown, and the line OpenWiz keeps instead.
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

/// The planted stderr variants. An empty text keeps the committed
/// fixture's own stderr.
fn stderr_variants() -> Vec<Planted> {
    let token = token();
    let long_phrase = "Synthetic ".repeat(2_000);
    vec![
        planted(
            "committed fixture",
            "",
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
        // The limits.
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
            "status below 100",
            "Error: HTTP 099: Synthetic server error",
            &["HTTP 099", "Synthetic server error"],
            WITHHELD,
        ),
        planted(
            "status above 599",
            "Error: HTTP 600: Synthetic server error",
            &["HTTP 600", "Synthetic server error"],
            WITHHELD,
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
    ]
}

/// One read that fails: its committed scenario, its name in messages, and
/// the commands and core operations that make it.
struct FailingRead {
    scenario: &'static str,
    read: &'static str,
    commands: &'static [&'static [&'static str]],
    core: fn(&Fake) -> Vec<String>,
}

fn core_list(fake: &Fake) -> Vec<String> {
    let result = fake.list(&ListRequest::default());
    texts(
        format!("{result:?}"),
        result.map(|e| serde_json::to_value(&e)),
    )
}

fn core_list_long(fake: &Fake) -> Vec<String> {
    let result = fake.list(&long());
    texts(
        format!("{result:?}"),
        result.map(|e| serde_json::to_value(&e)),
    )
}

fn core_show(fake: &Fake, id: &str) -> Vec<String> {
    let result = fake.show(id);
    texts(
        format!("{result:?}"),
        result.map(|e| serde_json::to_value(&e)),
    )
}

/// Every string a core result carries: an envelope's string values, or an
/// error's message parts, and the `Debug` text of either.
fn texts(debug: String, result: Result<serde_json::Result<Value>, OpError>) -> Vec<String> {
    let mut out = vec![debug];
    match result {
        Ok(envelope) => strings(&envelope.expect("an envelope serializes"), &mut out),
        Err(OpError::ListFailed(e) | OpError::ShowFailed(e)) => out.push(e.to_string()),
        Err(OpError::NotFound(id)) => out.push(id.0),
        Err(OpError::InvalidRequest(why)) => out.push(why),
    }
    out
}

/// Every key and string value in `value`, unescaped.
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

/// stdout, stderr and, for `--json`, every string in the JSON printed.
fn cli_texts(args: &[&str], code: i32, out: String, err: String) -> Vec<String> {
    let mut all = vec![format!("exit {code}")];
    if args.contains(&"--json")
        && let Ok(value) = serde_json::from_str::<Value>(&out)
    {
        strings(&value, &mut all);
    }
    all.extend([out, err]);
    all
}

/// A fake answering from a copy of `scenario`, written to its own
/// directory after `edit`. `extends` and every `stdout_file` are made
/// absolute, so the copy reads the committed fixtures.
fn fake_with(test: &str, scenario: &str, edit: impl FnOnce(&mut Value)) -> Fake {
    let fake = Fake::new(test, scenario);
    let dir = Path::new(FIXTURES).join(scenario);
    let mut doc: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("scenario.json")).expect("scenario"),
    )
    .expect("scenario JSON");
    if let Some(parent) = doc["extends"].as_str() {
        let parent = Path::new(FIXTURES).join(parent);
        doc["extends"] = Value::from(parent.to_str().expect("utf-8"));
    }
    for response in doc["responses"].as_array_mut().expect("responses") {
        if let Some(file) = response["stdout_file"].as_str() {
            response["stdout_file"] = Value::from(dir.join(file).to_str().expect("utf-8"));
        }
    }
    edit(&mut doc);
    let file = fake.dir.join("scenario.json");
    std::fs::write(&file, doc.to_string()).expect("scenario copy");
    std::fs::write(
        fake.dir.join("fake-room-cli.scenario"),
        file.to_str().expect("utf-8"),
    )
    .expect("pointer");
    fake
}

/// What the failing response prints on stdout; a failed read's stdout is
/// never used.
const FAILED_STDOUT: &str =
    "Synthetic stdout of a failed read at /Users/synthetic-owner/failed-read-stdout";
const FAILED_STDOUT_MARKER: &str = "failed-read-stdout";

/// Replaces the failing response's stderr (the committed one is kept when
/// `text` is empty), and plants [`FAILED_STDOUT`] on its stdout.
fn set_stderr(text: &str) -> impl FnOnce(&mut Value) + '_ {
    move |doc| {
        for response in doc["responses"].as_array_mut().expect("responses") {
            if response["exit"] == 1 {
                response["stdout"] = Value::from(FAILED_STDOUT);
                if !text.is_empty() {
                    response["stderr"] = Value::from(text);
                }
            }
        }
    }
}

/// The release `openwiz`, built once for this test binary (as P10 does).
fn release_openwiz() -> &'static Path {
    static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
    PROGRAM.get_or_init(|| {
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
        target.join("release").join("openwiz")
    })
}

/// Runs the release `openwiz` with this fake first on `PATH`.
fn binary(fake: &Fake, args: &[&str]) -> (i32, String, String) {
    let path = format!(
        "{}:{}",
        fake.dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(release_openwiz())
        .args(args)
        .env("PATH", path)
        .current_dir(&fake.dir)
        .output()
        .expect("run the release openwiz");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Every surface of one fake: the core, then each command in process and
/// through the release binary. Returns (surface, its strings).
fn surfaces(fake: &Fake, read: &FailingRead) -> Vec<(String, Vec<String>)> {
    let mut all = vec![("core".to_owned(), (read.core)(fake))];
    for args in read.commands {
        let (code, out, err) = fake.cli(args);
        all.push((format!("openwiz {args:?}"), cli_texts(args, code, out, err)));
        let (code, out, err) = binary(fake, args);
        all.push((
            format!("release openwiz {args:?}"),
            cli_texts(args, code, out, err),
        ));
    }
    all
}

/// The problems with one surface: a planted marker shown, or the kept
/// line missing.
fn problems(
    label: &str,
    surface: &str,
    texts: &[String],
    markers: &[String],
    kept: &str,
) -> Vec<String> {
    let mut found = Vec::new();
    for marker in markers {
        if let Some(text) = texts.iter().find(|t| t.contains(marker.as_str())) {
            let at = text.find(marker.as_str()).unwrap_or(0);
            let from = (0..=at.saturating_sub(40))
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(0);
            let shown: String = text[from..].chars().take(120).collect();
            found.push(format!(
                "{label}: {surface} shows the planted {marker:?}: …{shown:?}"
            ));
        }
    }
    if !texts.iter().any(|t| t.contains(kept)) {
        found.push(format!("{label}: {surface} does not show {kept:?}"));
    }
    found
}

/// Runs every stderr variant against one failing read.
fn check_failing_read(read: FailingRead) {
    let mut found = Vec::new();
    for (i, variant) in stderr_variants().into_iter().enumerate() {
        let label = format!("{} / {}", read.read, variant.name);
        let fake = fake_with(
            &format!("p04b_{}_{i}", read.scenario),
            read.scenario,
            set_stderr(&variant.text),
        );
        let mut markers = variant.markers;
        markers.push(FAILED_STDOUT_MARKER.to_owned());
        // A failed version read keeps safe facts only, so even the safe
        // line is never shown for it.
        let kept = if read.read == "room-cli --version" {
            markers.push(variant.kept.clone());
            "exited 1 (detail withheld)".to_owned()
        } else {
            format!("exited 1: {}", variant.kept)
        };
        for (surface, texts) in surfaces(&fake, &read) {
            found.extend(problems(&label, &surface, &texts, &markers, &kept));
        }
    }
    assert!(
        found.is_empty(),
        "{} problem(s):\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn thread_list_diagnostics_never_reach_the_output() {
    check_failing_read(FailingRead {
        scenario: "diagnostics-thread-list",
        read: "thread list",
        commands: &[&["ls"], &["ls", "--json"], &["ls", "-l", "--json"]],
        core: core_list,
    });
}

#[test]
fn project_list_diagnostics_never_reach_the_output() {
    check_failing_read(FailingRead {
        scenario: "diagnostics-project-list",
        read: "project list",
        commands: &[
            &["ls"],
            &["ls", "--json"],
            &["show", "thr_syn_0001"],
            &["show", "thr_syn_0001", "--json"],
        ],
        core: |fake| {
            let mut all = core_list(fake);
            all.extend(core_show(fake, "thr_syn_0001"));
            all
        },
    });
}

#[test]
fn machine_list_diagnostics_never_reach_the_output() {
    check_failing_read(FailingRead {
        scenario: "diagnostics-machine-list",
        read: "machine list",
        commands: &[
            &["ls"],
            &["ls", "--json"],
            &["show", "thr_syn_0002"],
            &["show", "thr_syn_0002", "--json"],
        ],
        core: |fake| {
            let mut all = core_list(fake);
            all.extend(core_show(fake, "thr_syn_0002"));
            all
        },
    });
}

#[test]
fn thread_show_diagnostics_never_reach_the_output() {
    check_failing_read(FailingRead {
        scenario: "diagnostics-thread-show",
        read: "thread show",
        commands: &[
            &["ls", "-l"],
            &["ls", "-l", "--json"],
            &["show", "thr_syn_0002"],
            &["show", "thr_syn_0002", "--json"],
        ],
        core: |fake| {
            let mut all = core_list_long(fake);
            all.extend(core_show(fake, "thr_syn_0002"));
            all
        },
    });
}

#[test]
fn thread_output_diagnostics_never_reach_the_output() {
    check_failing_read(FailingRead {
        scenario: "diagnostics-thread-output",
        read: "thread output",
        commands: &[
            &["show", "thr_syn_0002"],
            &["show", "thr_syn_0002", "--json"],
        ],
        core: |fake| core_show(fake, "thr_syn_0002"),
    });
}

/// The version read goes through the same runner. Its failure shows safe
/// facts only: `exited 1 (detail withheld)`, never even the safe line.
#[test]
fn version_read_diagnostics_never_reach_the_output() {
    check_failing_read(FailingRead {
        scenario: "diagnostics-version",
        read: "room-cli --version",
        commands: &[
            &["ls"],
            &["ls", "--json"],
            &["show", "thr_syn_0001", "--json"],
        ],
        core: |fake| {
            let mut all = core_list(fake);
            all.extend(core_show(fake, "thr_syn_0001"));
            all
        },
    });
}

/// Text `room-cli` prints on stdout where OpenWiz expects a version, JSON or
/// a recorded value: only a version of the form `N.N.N`, OpenWiz's own
/// words and recognised labels are shown.
#[test]
fn unexpected_stdout_never_reaches_the_output() {
    let token = token();
    let leak = format!(
        "/Users/synthetic-owner/.syn-room {token} https://synthetic-user:synthetic-pass@example.invalid"
    );
    let markers: Vec<String> = [
        "/Users/",
        "synthetic-owner",
        ".syn-room",
        &token,
        "https://",
        "synthetic-pass",
    ]
    .iter()
    .map(|m| (*m).to_owned())
    .collect();
    let normal = Path::new(FIXTURES).join("normal");
    let read_json = |name: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(normal.join(name)).expect(name)).expect(name)
    };
    let mut show = read_json("thread-show-0002.json");
    show["thread"]["id"] = Value::from(leak.clone());
    let mut rows = read_json("thread-list.json");
    rows[0]["visibility"] = Value::from(leak.clone());
    rows[5]["executionTarget"] = Value::from(leak.clone());

    // (name, the argv answered, its planted stdout, the failing read, the
    // text OpenWiz shows instead).
    let cases: Vec<(&str, Value, String, FailingRead, &str)> = vec![
        (
            "version text",
            json!(["--version"]),
            format!("room-cli 0.43.1 ({leak})\n"),
            FailingRead {
                scenario: "normal",
                read: "room-cli --version",
                commands: &[&["ls"], &["ls", "--json"]],
                core: core_list,
            },
            "printed a version that is not N.N.N or v<N> (detail withheld)",
        ),
        (
            "thread list that is not JSON",
            json!(["thread", "list", "--json"]),
            format!("Synthetic text that is not JSON: {leak}\n"),
            FailingRead {
                scenario: "normal",
                read: "thread list",
                commands: &[&["ls"], &["ls", "--json"]],
                core: core_list,
            },
            "not JSON (a syntax error at line 1 column",
        ),
        (
            "thread show of another session",
            json!(["thread", "show", "thr_syn_0002", "--json"]),
            show.to_string(),
            FailingRead {
                scenario: "normal",
                read: "thread show",
                commands: &[
                    &["ls", "-l", "--json"],
                    &["show", "thr_syn_0002"],
                    &["show", "thr_syn_0002", "--json"],
                ],
                core: |fake| {
                    let mut all = core_list_long(fake);
                    all.extend(core_show(fake, "thr_syn_0002"));
                    all
                },
            },
            "a different session (id withheld)",
        ),
        (
            "unrecognised values in thread list",
            json!(["thread", "list", "--json"]),
            rows.to_string(),
            FailingRead {
                scenario: "normal",
                read: "thread list",
                commands: &[&["ls"], &["ls", "--json"]],
                core: core_list,
            },
            "has a value that is not recognised (value withheld)",
        ),
    ];

    let planted_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("p04b_planted_stdout");
    let _ = std::fs::remove_dir_all(&planted_dir);
    std::fs::create_dir_all(&planted_dir).expect("planted dir");
    let mut found = Vec::new();
    for (i, (name, argv, stdout, read, kept)) in cases.into_iter().enumerate() {
        let file = planted_dir.join(format!("stdout-{i}"));
        std::fs::write(&file, stdout).expect("planted stdout");
        let response = json!({"argv": argv, "stdout_file": file.to_str().expect("utf-8")});
        let fake = fake_with(&format!("p04b_unexpected_stdout_{i}"), "normal", |doc| {
            doc["responses"]
                .as_array_mut()
                .expect("responses")
                .insert(0, response);
        });
        for (surface, texts) in surfaces(&fake, &read) {
            found.extend(problems(name, &surface, &texts, &markers, kept));
        }
    }
    assert!(
        found.is_empty(),
        "{} problem(s):\n{}",
        found.len(),
        found.join("\n")
    );
}

/// Not-found is still recognised from the raw line (it contains `HTTP 404:
/// Thread not found`), so `show` of a missing session still exits 3, while only the
/// safe line is shown.
#[test]
fn a_not_found_line_with_an_unsafe_suffix_is_still_not_found() {
    let planted = "Error: HTTP 404: Thread not found at /Users/synthetic-owner/.syn-room (token=synthetic-secret-value)";
    let markers: Vec<String> = [
        "/Users/",
        "synthetic-owner",
        "token=",
        "synthetic-secret-value",
    ]
    .iter()
    .map(|m| (*m).to_owned())
    .collect();
    let fake = fake_with(
        "p04b_not_found_suffix",
        "diagnostics-thread-show",
        set_stderr(planted),
    );
    let mut found = Vec::new();
    match fake.show("thr_syn_0002") {
        Err(OpError::NotFound(_)) => {}
        other => found.push(format!("core show: expected not found, got {other:?}")),
    }
    for args in [
        &["show", "thr_syn_0002"][..],
        &["show", "thr_syn_0002", "--json"][..],
    ] {
        for (surface, (code, out, err)) in [
            (format!("openwiz {args:?}"), fake.cli(args)),
            (format!("release openwiz {args:?}"), binary(&fake, args)),
        ] {
            if code != 3 {
                found.push(format!("{surface}: exit {code}, not 3: {err}"));
            }
            let texts = cli_texts(args, code, out, err);
            found.extend(problems(
                "not found",
                &surface,
                &texts,
                &markers,
                "not found",
            ));
        }
    }
    let read = FailingRead {
        scenario: "diagnostics-thread-show",
        read: "thread show",
        commands: &[&["ls", "-l"], &["ls", "-l", "--json"]],
        core: core_list_long,
    };
    for (surface, texts) in surfaces(&fake, &read) {
        found.extend(problems(
            "not found",
            &surface,
            &texts,
            &markers,
            "not found",
        ));
    }
    assert!(
        found.is_empty(),
        "{} problem(s):\n{}",
        found.len(),
        found.join("\n")
    );
}

/// The sanitizer's version grammar is `N.N.N` (1 to 4 digits each) or `v<N>`
/// (1 to 6 digits), the same as the core's. A version in it is shown as it is;
/// anything else is withheld, and the run goes on.
#[test]
fn the_version_grammar_is_n_n_n_or_v_n() {
    let cases: &[(&str, bool)] = &[
        ("0.43.1", true),
        ("v127", true),
        ("v1", true),
        ("v999999", true),
        ("9999.9999.9999", true),
        ("v", false),
        ("v1234567", false),
        ("V127", false),
        ("v12a", false),
        ("v-1", false),
        ("127", false),
        ("0.43", false),
        ("10000.0.0", false),
        ("v127 (synthetic update notice)", false),
        ("vv127", false),
    ];
    for (i, (version, shown)) in cases.iter().enumerate() {
        let response = json!({"argv": ["--version"], "stdout": format!("{version}\n")});
        let fake = fake_with(&format!("p04b_version_grammar_{i}"), "normal", |doc| {
            doc["responses"]
                .as_array_mut()
                .expect("responses")
                .insert(0, response);
        });
        let env = fake
            .list(&ListRequest::default())
            .unwrap_or_else(|e| panic!("{version:?}: the run goes on, got {e:?}"));
        let json = serde_json::to_value(&env).expect("envelope");
        if *shown {
            assert_eq!(json["source"]["version"]["known"], *version, "{version:?}");
        } else {
            let why = json["source"]["version"]["unknown"]
                .as_str()
                .unwrap_or_else(|| panic!("{version:?} is withheld: {}", json["source"]));
            assert!(
                why.contains("printed a version that is not N.N.N or v<N> (detail withheld)"),
                "{version:?}: {why}"
            );
            assert!(!json.to_string().contains("synthetic update notice"));
        }
        assert!(
            !env.data.sessions.is_empty(),
            "{version:?}: sessions listed"
        );
    }
}
