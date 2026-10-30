//! The fake `room-cli` the tests run in place of the real one. Test-only.
//!
//! A test makes a directory holding a symlink named `room-cli` to this
//! binary, plus a file `fake-room-cli.scenario` with the absolute path of a
//! fixture `scenario.json`. The fake finds that file next to the path it was
//! started as (`argv[0]`), so tests running in parallel never share state.
//!
//! For each call it:
//! 1. appends its arguments, as one JSON array per line, to `invocations.log`
//!    in the same directory;
//! 2. refuses anything outside the OpenWiz allowlist with exit 97;
//! 3. answers from the scenario: stdout (inline or from a file), stderr and an
//!    exit code, or never answers (`"hang": true`, after writing its process
//!    id to `hang.pid`).
//!
//! A scenario may `extends` another; its own responses are tried first.
//! A `thread show` or `thread output` of an id with no response gets the
//! not-found answer observed from `room-cli` 0.43.1.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use serde_json::Value;

const SCENARIO_FILE: &str = "fake-room-cli.scenario";
const LOG_FILE: &str = "invocations.log";
const HANG_PID_FILE: &str = "hang.pid";
const REFUSED: u8 = 97;
const NO_FIXTURE: u8 = 98;
const NOT_FOUND_LINE: &str = "Error: HTTP 404: Thread not found";

fn main() -> ExitCode {
    let mut args = std::env::args();
    let argv0 = PathBuf::from(args.next().unwrap_or_default());
    let argv: Vec<String> = args.collect();
    let dir = argv0.parent().unwrap_or(Path::new(".")).to_path_buf();

    log_invocation(&dir, &argv);

    if !allowlisted(&argv) {
        eprintln!(
            "fake room-cli: refused, not on the OpenWiz allowlist: {}",
            argv.join(" ")
        );
        return ExitCode::from(REFUSED);
    }

    let scenario_path = match fs::read_to_string(dir.join(SCENARIO_FILE)) {
        Ok(p) => PathBuf::from(p.trim()),
        Err(e) => {
            eprintln!("fake room-cli: no {SCENARIO_FILE} next to {argv0:?}: {e}");
            return ExitCode::from(NO_FIXTURE);
        }
    };

    match find_response(&scenario_path, &argv) {
        Some((response, base)) => answer(&response, &base, &dir),
        None if is_detail_read(&argv) => {
            eprintln!("{NOT_FOUND_LINE}");
            ExitCode::from(1)
        }
        None => {
            eprintln!("fake room-cli: no fixture for: {}", argv.join(" "));
            ExitCode::from(NO_FIXTURE)
        }
    }
}

fn log_invocation(dir: &Path, argv: &[String]) {
    let line = serde_json::to_string(argv).expect("argv serializes");
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(LOG_FILE))
    {
        // One write per line, so concurrent calls never interleave lines.
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

/// The fake's own copy of the allowlist, kept independent of the adapter's
/// so that a mistake in one is caught by the other.
fn allowlisted(argv: &[String]) -> bool {
    let a: Vec<&str> = argv.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["--version"] => true,
        ["thread", "list", "--json", rest @ ..] => matches!(
            rest,
            [] | ["--include-hidden"]
                | ["--include-archived"]
                | ["--include-hidden", "--include-archived"]
        ),
        ["project", "list", "--json", "--include-personal", rest @ ..] => {
            matches!(rest, [] | ["--hidden"])
        }
        ["machine", "list", "--json"] => true,
        ["thread", "show" | "output", id, "--json"] => !id.starts_with('-'),
        _ => false,
    }
}

fn is_detail_read(argv: &[String]) -> bool {
    matches!(argv.get(1).map(String::as_str), Some("show" | "output"))
}

/// Finds the response for `argv`, and the directory its files are relative to.
fn find_response(scenario_path: &Path, argv: &[String]) -> Option<(Value, PathBuf)> {
    let text = fs::read_to_string(scenario_path)
        .unwrap_or_else(|e| panic!("fake room-cli: cannot read {scenario_path:?}: {e}"));
    let scenario: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("fake room-cli: {scenario_path:?} is not JSON: {e}"));
    let base = scenario_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let own = scenario["responses"].as_array().into_iter().flatten();
    for response in own {
        let wanted: Vec<&str> = response["argv"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if wanted == argv.iter().map(String::as_str).collect::<Vec<_>>() {
            return Some((response.clone(), base));
        }
    }
    let parent = scenario["extends"].as_str()?;
    find_response(&base.join("..").join(parent).join("scenario.json"), argv)
}

fn answer(response: &Value, base: &Path, dir: &Path) -> ExitCode {
    if response["hang"].as_bool() == Some(true) {
        // Lets a test check that the hung read was stopped.
        let _ = fs::write(dir.join(HANG_PID_FILE), std::process::id().to_string());
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }
    if let Some(file) = response["stdout_file"].as_str() {
        let body = fs::read(base.join(file))
            .unwrap_or_else(|e| panic!("fake room-cli: cannot read {file}: {e}"));
        std::io::stdout().write_all(&body).expect("stdout");
    }
    if let Some(text) = response["stdout"].as_str() {
        print!("{text}");
    }
    if let Some(text) = response["stderr"].as_str() {
        eprintln!("{text}");
    }
    let code = response["exit"].as_u64().unwrap_or(0);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
