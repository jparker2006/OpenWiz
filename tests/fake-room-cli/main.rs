use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use serde_json::Value;

const SCENARIO_FILE: &str = "fake-room-cli.scenario";
const LOG_FILE: &str = "invocations.log";
const ENV_LOG_FILE: &str = "environment.log";
const ENV_PREFIXES: &[&str] = &["ROOM_", "BB_", "CLOUDROOM_"];
const HANG_PID_FILE: &str = "hang.pid";
const REFUSED: u8 = 97;
const NO_FIXTURE: u8 = 98;
const NOT_FOUND_LINE: &str = "Error: HTTP 404: Thread not found";
const TELL_COUNT_FILE: &str = "tell-calls.count";
const STATE_DIR_ENV: &str = "OPENWIZ_STATE_DIR";
const STATE_FILE: &str = "fake-room-cli.state";
const SPAWN_COUNT_FILE: &str = "spawn-calls.count";
const THREADS_FILE: &str = "threads-created.count";
const REQUEST_IDS_FILE: &str = "request-ids.json";
const CONFLICT_LINE: &str =
    "Error: Failed to create thread: HTTP 409: Request ID was already used with different content";
const ATTACHMENTS_LOG: &str = "attachments.log";
const UPLOAD_FAILED: &str = "fake room-cli: an attachment could not be read for upload";

fn main() -> ExitCode {
    let mut args = std::env::args();
    let argv0 = PathBuf::from(args.next().unwrap_or_default());
    let argv: Vec<String> = args.collect();
    let dir = argv0.parent().unwrap_or(Path::new(".")).to_path_buf();

    log_invocation(&dir, &argv);
    log_environment(&dir);

    if !allowlisted(&argv) {
        eprintln!(
            "fake room-cli: refused, not on the OpenWiz allowlist: {}",
            argv.join(" ")
        );
        return ExitCode::from(REFUSED);
    }

    let tell = is_tell(&argv);
    if tell {
        let n = count(&dir, TELL_COUNT_FILE);
        snapshot_records(&dir, "sends", &format!("records-at-tell-{n}.json"));
    }
    if is_spawn(&argv) {
        let n = count(&dir, SPAWN_COUNT_FILE);
        snapshot_records(&dir, "new-threads", &format!("records-at-spawn-{n}.json"));
    }

    let scenario_path = match fs::read_to_string(dir.join(SCENARIO_FILE)) {
        Ok(p) => PathBuf::from(p.trim()),
        Err(e) => {
            eprintln!("fake room-cli: no {SCENARIO_FILE} next to {argv0:?}: {e}");
            return ExitCode::from(NO_FIXTURE);
        }
    };

    let found = find_response(&scenario_path, &argv);
    // The upload, before the request (F3): every attachment read whole. A
    // response may make it fail after `upload_fails_after` files.
    let fails_after = found
        .as_ref()
        .and_then(|(response, _)| response["upload_fails_after"].as_u64());
    if (tell || is_spawn(&argv)) && !upload(&dir, &argv, fails_after) {
        eprintln!("{UPLOAD_FAILED}");
        return ExitCode::from(1);
    }

    match found {
        Some((response, _)) if is_spawn(&argv) && !request_id_table(&dir, &argv, &response) => {
            eprintln!("{CONFLICT_LINE}");
            ExitCode::from(1)
        }
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

/// The names of the CloudRoom variables this call was started with.
fn log_environment(dir: &Path) {
    let mut names: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| ENV_PREFIXES.iter().any(|p| name.starts_with(p)))
        .collect();
    names.sort();
    let line = serde_json::to_string(&names).expect("names serialize");
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(ENV_LOG_FILE))
    {
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
        // The one write, in exactly the shape OpenWiz chose (documented, not
        // observed): no other flag, never `auto`, `--` before the text; B2's
        // attachment pairs after `--json`, never with `steer`.
        [
            "thread",
            "tell",
            id,
            "--mode",
            mode @ ("queue" | "steer"),
            "--json",
            rest @ ..,
        ] => {
            let Some((pairs, ["--", _text])) = rest.split_last_chunk::<2>() else {
                return false;
            };
            !id.starts_with('-') && staged_pairs(pairs) && (pairs.is_empty() || *mode == "queue")
        }

        ["cloud", "status", "--json"] => true,
        [
            "thread",
            "spawn",
            "--project",
            project,
            "--machine",
            "cloud",
            "--provider",
            "codex" | "claude-code",
            "--model",
            model,
            "--reasoning-level",
            "low" | "medium" | "high" | "xhigh" | "max",
            "--request-id",
            request_id,
            "--json",
            rest @ ..,
        ] => {
            let Some((pairs, ["--prompt", _text])) = rest.split_last_chunk::<2>() else {
                return false;
            };
            !project.starts_with('-')
                && !model.starts_with('-')
                && is_request_id(request_id)
                && staged_pairs(pairs)
        }
        _ => false,
    }
}

/// B2: zero or more `--file <path>` or `--image <path>` pairs, each path a
/// staged copy's: absolute, no `.` or `..` and no empty component, no
/// control character, never a `file:` URL, and ending in
/// `attachments/<id>/<safe name>` (the fake's own copy of the id and safe
/// name rules).
fn staged_pairs(pairs: &[&str]) -> bool {
    pairs.len().is_multiple_of(2)
        && pairs
            .chunks(2)
            .all(|pair| matches!(pair[0], "--file" | "--image") && is_staged_path(pair[1]))
}

fn is_staged_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let plain = parts
        .iter()
        .all(|p| !p.is_empty() && *p != "." && *p != "..");
    let id_ok = |id: &str| {
        !id.is_empty()
            && id.len() <= 64
            && id.as_bytes()[0].is_ascii_alphanumeric()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    let name_ok = |name: &str| {
        !name.is_empty()
            && name.len() <= 179
            && !name.starts_with(['.', '-'])
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    plain
        && !path.chars().any(char::is_control)
        && matches!(parts.as_slice(), [.., "attachments", id, name] if id_ok(id) && name_ok(name))
}

/// The attachment pairs of an accepted `tell` or `spawn`: `(flag, path)`.
fn attachment_pairs(argv: &[String]) -> Vec<(String, String)> {
    let Some(json) = argv.iter().position(|w| w == "--json") else {
        return vec![];
    };
    let end = argv.len() - 2;
    argv[json + 1..end]
        .chunks(2)
        .map(|p| (p[0].clone(), p[1].clone()))
        .collect()
}

/// The upload `room-cli` makes before its request (F3, F21, source-read):
/// every attachment read whole, all held at once, then one line in
/// `attachments.log`. `false` when a file can't be read: the call fails
/// there.
fn upload(dir: &Path, argv: &[String], fails_after: Option<u64>) -> bool {
    let pairs = attachment_pairs(argv);
    if pairs.is_empty() {
        return true;
    }
    let read = usize::try_from(fails_after.unwrap_or(u64::MAX)).unwrap_or(usize::MAX);
    let held: Vec<Option<Vec<u8>>> = pairs
        .iter()
        .enumerate()
        .map(|(i, (_, path))| if i < read { fs::read(path).ok() } else { None })
        .collect();
    let items: Vec<Value> = pairs
        .iter()
        .zip(&held)
        .map(|((flag, path), bytes)| {
            serde_json::json!({
                "flag": flag,
                "path": path,
                "sha256": bytes.as_deref().map(openwiz_core::messaging::sha256_hex),
                "size": bytes.as_ref().map(Vec::len),
            })
        })
        .collect();
    let line = serde_json::json!({
        "call": argv[1],
        "request_id": is_spawn(argv).then(|| argv[13].clone()),
        "items": items,
        "vm_hwm_kib": peak_memory_kib(),
    });
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(ATTACHMENTS_LOG))
    {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
    let read_all = held.iter().all(Option::is_some);
    drop(held);
    read_all
}

/// This process's peak resident memory in KiB (`VmHWM`), on Linux.
fn peak_memory_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

/// `ow1-` and 40 lowercase hex digits, as OpenWiz derives a request id.
fn is_request_id(id: &str) -> bool {
    id.strip_prefix("ow1-").is_some_and(|hex| {
        hex.len() == 40 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn is_spawn(argv: &[String]) -> bool {
    argv.len() >= 17 && argv[0] == "thread" && argv[1] == "spawn"
}

/// The request-ID table, under its own lock: `false` when the request id
/// was used with other content (refused, nothing created). The same
/// content again creates nothing; a new request id is recorded, and
/// counted as a thread, when the response `creates`.
fn request_id_table(dir: &Path, argv: &[String], response: &Value) -> bool {
    let mut lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(REQUEST_IDS_FILE))
        .expect("fake room-cli: open the request-ID table");
    lock.lock()
        .expect("fake room-cli: lock the request-ID table");
    let mut text = String::new();
    std::io::Read::read_to_string(&mut lock, &mut text).expect("read the request-ID table");
    let mut table: serde_json::Map<String, Value> = serde_json::from_str(&text).unwrap_or_default();
    // Everything but the request id: project, provider, model, effort, text.
    // Never the attachments: CloudRoom's dedup compares the text (F17).
    let last = argv.len() - 1;
    let content: Vec<&String> = [3, 7, 9, 11, last].iter().map(|&i| &argv[i]).collect();
    let content = serde_json::to_value(content).expect("content serializes");
    let request_id = &argv[13];
    match table.get(request_id) {
        Some(known) => return *known == content,
        None if response["creates"].as_bool() == Some(true) => {
            table.insert(request_id.clone(), content);
            count(dir, THREADS_FILE);
        }
        None => return true,
    }
    let body = serde_json::to_string(&table).expect("table serializes");
    std::io::Seek::seek(&mut lock, std::io::SeekFrom::Start(0)).expect("seek");
    lock.set_len(0).expect("truncate");
    lock.write_all(body.as_bytes()).expect("write the table");
    lock.sync_all().expect("sync the table");
    true
}

fn is_tell(argv: &[String]) -> bool {
    argv.len() >= 8 && argv[0] == "thread" && argv[1] == "tell"
}

/// Adds one to the cross-process counter in `name` and returns the new
/// count.
fn count(dir: &Path, name: &str) -> u64 {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(name))
        .expect("fake room-cli: open a counter");
    file.lock().expect("fake room-cli: lock a counter");
    let mut text = String::new();
    std::io::Read::read_to_string(&mut file, &mut text).expect("read a counter");
    let n = text.trim().parse::<u64>().unwrap_or(0) + 1;
    std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0)).expect("seek");
    file.set_len(0).expect("truncate");
    file.write_all(n.to_string().as_bytes())
        .expect("write a counter");
    file.sync_all().expect("sync a counter");
    n
}

/// Copies the records under the state directory's `kind/` on disk at the
/// moment of a write call into `into`.
fn snapshot_records(dir: &Path, kind: &str, into: &str) {
    let state = match fs::read_to_string(dir.join(STATE_FILE)) {
        Ok(path) => PathBuf::from(path.trim()),
        Err(_) => match std::env::var_os(STATE_DIR_ENV) {
            Some(path) => PathBuf::from(path),
            None => return,
        },
    };
    let mut records = Vec::new();
    if let Ok(entries) = fs::read_dir(Path::new(&state).join(kind)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(text) = fs::read_to_string(&path)
                && let Ok(value) = serde_json::from_str::<Value>(&text)
            {
                records.push(value);
            }
        }
    }
    let _ = fs::write(
        dir.join(into),
        serde_json::to_string(&records).expect("records serialize"),
    );
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
        let mut got: Vec<&str> = argv.iter().map(String::as_str).collect();
        if response["any_attachments"].as_bool() == Some(true) && is_tell(argv) {
            // The call without its attachment pairs (B2).
            let pairs = 2 * attachment_pairs(argv).len();
            got.drain(6..6 + pairs);
        }
        let got = if response["any_text"].as_bool() == Some(true) && is_tell(argv) {
            &got[..got.len() - 1]
        } else {
            &got[..]
        };
        let any_spawn = response["any_spawn"].as_bool() == Some(true) && is_spawn(argv);
        if any_spawn || wanted == got {
            return Some((response.clone(), base));
        }
    }
    let parent = scenario["extends"].as_str()?;
    find_response(&base.join("..").join(parent).join("scenario.json"), argv)
}

/// Kills the process that started this fake (the sender), with SIGKILL,
/// and waits until `kill` has delivered it.
fn kill_parent() {
    let parent = std::os::unix::process::parent_id();
    let status = std::process::Command::new("kill")
        .args(["-9", &parent.to_string()])
        .status()
        .expect("fake room-cli: run kill");
    assert!(status.success(), "fake room-cli: kill {parent} failed");
    // The parent is gone; give the kernel a moment, then end quietly.
    thread::sleep(Duration::from_millis(50));
}

fn answer(response: &Value, base: &Path, dir: &Path) -> ExitCode {
    if let Some(ms) = response["delay_ms"].as_u64() {
        thread::sleep(Duration::from_millis(ms));
    }
    if response["kill_parent"].as_bool() == Some(true) {
        kill_parent();
    }
    if response["abort"].as_bool() == Some(true) {
        std::process::abort();
    }
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
    if response["kill_parent_after_answer"].as_bool() == Some(true) {
        std::io::stdout().flush().expect("stdout");
        kill_parent();
    }
    let code = response["exit"].as_u64().unwrap_or(0);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
