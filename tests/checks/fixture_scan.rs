use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");
const FAKE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fake-room-cli");

/// Keys recorded in the room-cli 0.43.1 shapes (captures A, B and C).
const OBSERVED_KEYS: &[&str] = &[
    // thread list rows, thread show `thread`
    "id",
    "projectId",
    "providerId",
    "title",
    "titleFallback",
    "status",
    "visibility",
    "queuedWork",
    "environmentWorkspaceDisplayKind",
    "lastReadAt",
    "latestAttentionAt",
    "createdAt",
    "updatedAt",
    "hasPendingInteraction",
    "parentThreadId",
    "environmentId",
    "environmentHostId",
    "environmentPath",
    "environmentProviderId",
    "environmentIsWorktree",
    "environmentBranchName",
    "archivedAt",
    "sectionId",
    "sourceThreadId",
    "originKind",
    "originPluginId",
    "pinnedAt",
    "deletedAt",
    "pinSortKey",
    "environmentName",
    "purpose",
    "text",
    "percent",
    "sourceSeq",
    "activity",
    "activeBackgroundAgentCount",
    "activeBackgroundCommandCount",
    "activeGoalCount",
    "activePlanModeCount",
    "activeWorkflowCount",
    "runtime",
    "displayStatus",
    "hostReconnectGraceExpiresAt",
    "executionTarget",
    "projectCopy",
    "phase",
    "completed",
    "total",
    "queuedMessageCount",
    "canSpawnChild",
    // thread show
    "thread",
    "execution",
    "model",
    "reasoningLevel",
    "environment",
    "pendingTodos",
    "name",
    "hostId",
    "path",
    "isGitRepo",
    "isWorktree",
    "branchName",
    "baseBranch",
    "defaultBranch",
    "mergeBaseBranch",
    "lifecycle",
    "retireAt",
    "teardown",
    "environmentProviderSelection",
    "machine",
    "type",
    "inputs",
    "environmentProviderInstanceKey",
    "managed",
    "workspaceProvisionType",
    "pullRequest",
    // thread output
    "output",
    "error",
    // project list
    "kind",
    "gitRemoteUrl",
    "sources",
    "isDefault",
    // machine list
    "machineProviderId",
    "suspendedAt",
    "message",
    "pendingLog",
    "maxPermissionMode",
    "lastSeenAt",
    "lastRejectedProtocolVersion",
];

const OBSERVED_SENT_KEYS: &[&str] = &["threadId", "ok", "delivery", "mode"];

/// Queued keys/labels observed by PC probe c687079 , only in send/.
const OBSERVED_QUEUED_KEYS: &[&str] = &[
    "queuedMessage",
    "id",
    "initiator",
    "senderThreadId",
    "threadId",
    "content",
    "type",
    "text",
    "mentions",
    "model",
    "reasoningLevel",
    "permissionMode",
    "serviceTier",
    "hardQueue",
    "sendAt",
    "waitingOn",
    "kind",
    "failureReason",
    "payload",
    "editable",
    "createdAt",
    "updatedAt",
];
const OBSERVED_QUEUED_LABELS: &[&str] = &["agent", "user", "inline"];
const SYNTHETIC_QUEUED_UUID: &str = "00000000-0000-4000-8000-000000000001";
const SYNTHETIC_QUEUED_ID: &str = "qmsg_syn_0001";

/// Keys of the `thread tell` receipt, documented in the room-cli 0.43.1
/// guide (read 2026-10-23) and **not observed**; plus the keys of a `thread
/// log` event, observed in capture A (outside-observer shape), which OpenWiz
/// never reads: they appear only in `send/thread-log-one-match.json`.
const DOCUMENTED_SEND_KEYS: &[&str] = &[
    // thread tell --json receipt (documented, not observed)
    "delivery",
    "queuedMessage",
    "waitingOn",
    "sendAt",
    "initiator",
    "senderThreadId",
    // thread log event (capture A; not on the allowlist)
    "scope",
    "threadId",
    "seq",
    "data",
    "requestId",
    "source",
    "input",
    "target",
    "request",
    "serviceTier",
    "permissionMode",
];

/// Labels of the `thread tell` receipt and of queued work, documented in the
/// room-cli 0.43.1 guide and changelog, and **not observed**.
const DOCUMENTED_SEND_LABELS: &[&str] = &[
    "sent",
    "queued",
    "waiting",
    "time",
    "thread-busy",
    "turn-starting",
    "provisioning",
    "host-offline",
    "interaction",
    "plugin",
];

const NEW_THREAD_KEYS: &[&str] = &[
    // top level
    "ready",
    "harnesses",
    "plan",
    "macAccessLevel",
    "planUsage",
    "credits",
    "creditsLimit",
    "extraCredits",
    "extraCreditsTotal",
    "og",
    "localLogins",
    "previews",
    "count",
    "explained",
    "state",
    "issue",
    "attachments",
    "compact",
    "copyLogins",
    "macAccess",
    "macAccessSupported",
    "queue_cancel",
    "queue_edit",
    "queue_reorder",
    "rewind",
    "signingIn",
    "steer",
    "teleport",
    "usedCloud",
    "workspaces",
    "macAccessLevels",
    "lastCloudAt",
    "cloudQueue",
    "repository",
    "signInError",
    "storage",
    "sync",
    // harnesses[]
    "reasoning_levels",
    "provider",
    "models",
    "files",
    "images",
    "goal",
    "prompt_reasoning",
    "provider_selection",
    "service_tier",
    "skill_mentions",
    "subagents",
    "usage",
    // thread show (source-read)
    "lastError",
];

/// The catalogue's harness ids and effort labels (observed, run 3), and the
/// `pending` and `starting` statuses (source-read). Only in
/// `room-cli-v127/new-thread/`.
const NEW_THREAD_LABELS: &[&str] = &[
    "pi", "cursor", "fx", "opencode", "hermes", "devin", "openclaw", "none", "minimal", "low",
    "medium", "xhigh", "max", "ultra", "pending", "starting",
];

/// Enum labels recorded in the shapes, plus `hidden`, a labelled synthetic
/// assumption (no hidden session was observed).
const LABELS: &[&str] = &[
    "idle",
    "active",
    "error",
    "claude-code",
    "codex",
    "acp-hermes-agent",
    "visible",
    "hidden",
    "none",
    "project-checkout",
    "personal-workspace",
    "other",
    "unmanaged-worktree",
    "cloud",
    "complete",
    "personal",
    "standard",
    "local_path",
    "persistent",
    "connected",
    "full",
    "high",
];

/// Keys of the scenario files the fake reads (not room-cli data).
const SCENARIO_KEYS: &[&str] = &[
    "scenario",
    "description",
    "extends",
    "synthetic_assumptions",
    "responses",
    "argv",
    "stdout_file",
    "stdout",
    "stderr",
    "exit",
    "hang",
    // thread tell responses (OpenWiz 0.0.3)
    "any_text",
];

/// Words of the allowlisted argument lists.
const ARGV_WORDS: &[&str] = &[
    "--version",
    "thread",
    "list",
    "--json",
    "--include-hidden",
    "--include-archived",
    "project",
    "--include-personal",
    "--hidden",
    "machine",
    "show",
    "output",
];

/// Words of the one write, `thread tell <id> --mode queue|steer --json --
/// <text>`: documented, not observed. The text is never in a fixture's
/// `argv` (responses match it with `any_text`).
const SEND_ARGV_WORDS: &[&str] = &["tell", "--mode", "queue", "steer", "--"];

/// Error lines the fake prints: the observed not-found line and synthetic ones.
const ERROR_LINES: &[&str] = &[
    "Error: HTTP 404: Thread not found",
    "Error: HTTP 500: Synthetic server error",
    "Error: HTTP 500: Synthetic private version marker",
];

/// Synthetic placeholders for the documented `thread tell` rejections, whose
/// wording is not documented (OpenWiz 0.0.3). Only in `send/`.
const SEND_ERROR_LINES: &[&str] = &[
    "Error: HTTP 400: Invalid input",
    "Error: HTTP 400: Unsupported mode",
];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn all_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    files(Path::new(FIXTURES), &mut out);
    files(Path::new(FAKE), &mut out);
    out.sort();
    out
}

fn is_synthetic_id(s: &str) -> bool {
    ["thr_syn_", "prj_syn_", "mch_syn_", "env_syn_", "src_syn_"]
        .iter()
        .any(|p| {
            s.strip_prefix(p)
                .is_some_and(|n| n.len() == 4 && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

fn is_version(s: &str) -> bool {
    // `v<N>`: the format `room-cli` prints since v127 .
    if let Some(n) = s.strip_prefix('v') {
        return !n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit());
    }
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn is_synthetic_value(s: &str) -> bool {
    s.is_empty()
        || LABELS.contains(&s)
        || ARGV_WORDS.contains(&s)
        || ERROR_LINES.contains(&s)
        || is_synthetic_id(s)
        || is_version(s)
        || s.starts_with("Synthetic")
        || s.starts_with("synthetic")
        || s.starts_with("syn-")
        || s.starts_with("/synthetic/")
        || s.starts_with("https://example.invalid/synthetic/")
}

fn is_allowed_key(k: &str) -> bool {
    OBSERVED_KEYS.contains(&k) || k.starts_with("synthetic")
}

/// Which documented or newly observed shapes a fixture may use.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Folder {
    Other,
    /// `room-cli-0.43.1/send/` (OpenWiz 0.0.3).
    Send,
    /// `room-cli-v127/new-thread/` (OpenWiz 0.0.4).
    NewThread,
}

fn folder(path: &Path) -> Folder {
    if is_send_fixture(path) {
        Folder::Send
    } else if path.starts_with(Path::new(FIXTURES).join("room-cli-v127/new-thread")) {
        Folder::NewThread
    } else {
        Folder::Other
    }
}

/// [`value_ok`] and [`key_ok`] for a fixture in `folder`.
fn value_in(s: &str, folder: Folder) -> bool {
    value_ok(s, folder == Folder::Send)
        || (folder == Folder::NewThread && NEW_THREAD_LABELS.contains(&s))
}

fn key_in(k: &str, folder: Folder) -> bool {
    key_ok(k, folder == Folder::Send)
        || (folder == Folder::NewThread && NEW_THREAD_KEYS.contains(&k))
}

/// A value allowed in a fixture: synthetic, or (only in `send/`) one of the
/// documented send labels, words or placeholder lines.
fn value_ok(s: &str, send: bool) -> bool {
    is_synthetic_value(s)
        || (send
            && (DOCUMENTED_SEND_LABELS.contains(&s)
                || SEND_ARGV_WORDS.contains(&s)
                || SEND_ERROR_LINES.contains(&s)
                || OBSERVED_QUEUED_LABELS.contains(&s)
                || s == SYNTHETIC_QUEUED_ID
                || s == SYNTHETIC_QUEUED_UUID))
}

/// A key allowed in a fixture: observed, or (only in `send/`) documented.
fn key_ok(k: &str, send: bool) -> bool {
    is_allowed_key(k)
        || (send
            && (DOCUMENTED_SEND_KEYS.contains(&k)
                || OBSERVED_SENT_KEYS.contains(&k)
                || OBSERVED_QUEUED_KEYS.contains(&k)))
}

/// Whether `path` is a `send/` fixture (OpenWiz 0.0.3).
fn is_send_fixture(path: &Path) -> bool {
    path.starts_with(Path::new(FIXTURES).join("room-cli-0.43.1/send"))
}

/// Fixture context is explicit; the global guard remains unchanged.
fn private_looking_in(text: &str, context: Folder) -> Vec<String> {
    if context == Folder::Send {
        // Only an entire quoted JSON token is the reserved fixture UUID.
        // Prefixes/suffixes and every unrelated UUID remain visible.
        let quoted = format!("\"{SYNTHETIC_QUEUED_UUID}\"");
        let quoted_id = format!("\"{SYNTHETIC_QUEUED_ID}\"");
        return private_looking(
            &text
                .replace(&quoted, "\"syn-queued-uuid\"")
                .replace(&quoted_id, "\"syn-queued-id\""),
        );
    }
    private_looking(text)
}

/// Rule 1: patterns that must never appear anywhere. Returns what matched.
fn private_looking(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for needle in [
        "/Users/",
        "/home/",
        "C:\\",
        "gho_",
        "ghp_",
        "github_pat_",
        "sk-",
        "xox",
        "BEGIN ",
    ] {
        if text.contains(needle) {
            found.push(needle.to_owned());
        }
    }
    let b = text.as_bytes();
    for (i, _) in text.match_indices('@') {
        let word = |j: usize| b.get(j).is_some_and(|c| c.is_ascii_alphanumeric());
        if i > 0 && word(i - 1) && word(i + 1) {
            found.push(format!("an email address at byte {i}"));
        }
    }
    for (i, _) in text.match_indices("://") {
        let rest = &text[i..];
        if !rest.starts_with("://example.invalid/") {
            found.push(format!("URL at byte {i}"));
        }
    }
    for prefix in ["thr_", "prj_", "mch_", "env_", "src_"] {
        for (i, _) in text.match_indices(prefix) {
            let start_ok = i == 0 || !text.as_bytes()[i - 1].is_ascii_alphanumeric();
            if start_ok && !text[i + prefix.len()..].starts_with("syn_") {
                found.push(format!("non-synthetic id {prefix}… at byte {i}"));
            }
        }
    }
    // Only entire quoted reserved queued ids are exempt, under send/ above.
    if text.contains("qmsg_") {
        found.push("a queued message id".to_owned());
    }
    if has_uuid(text) {
        found.push("a UUID".to_owned());
    }
    found
}

fn has_uuid(text: &str) -> bool {
    let b = text.as_bytes();
    let shape = [8, 4, 4, 4, 12];
    let total = 36;
    (0..b.len().saturating_sub(total - 1)).any(|start| {
        let mut i = start;
        for (n, len) in shape.iter().enumerate() {
            if !b[i..i + len].iter().all(u8::is_ascii_hexdigit) {
                return false;
            }
            i += len;
            if n < 4 {
                if b[i] != b'-' {
                    return false;
                }
                i += 1;
            }
        }
        true
    })
}

/// Rule 2 for room-cli JSON that parses: every key and string value.
fn walk(value: &Value, path: &str, bad: &mut Vec<String>, send: bool) {
    let folder = if send { Folder::Send } else { Folder::Other };
    walk_in(value, path, bad, folder)
}

fn walk_in(value: &Value, path: &str, bad: &mut Vec<String>, folder: Folder) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if !key_in(k, folder) {
                    bad.push(format!("{path}: key {k:?} is not a recorded key"));
                }
                walk_in(v, &format!("{path}.{k}"), bad, folder);
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk_in(item, &format!("{path}[{i}]"), bad, folder);
            }
        }
        Value::String(s) if !value_in(s, folder) => {
            bad.push(format!("{path}: value {s:?} is not synthetic"));
        }
        _ => {}
    }
}

/// Rule 2 for a scenario file: its own keys, allowlisted argument words,
/// file names in its directory and synthetic stdout and stderr. The prose in
/// `description` and `synthetic_assumptions` is covered by rule 1.
fn scan_scenario(path: &Path, value: &Value, bad: &mut Vec<String>) {
    let dir = path.parent().expect("scenario dir");
    let send = is_send_fixture(path);
    let keys_ok = |v: &Value, bad: &mut Vec<String>| {
        for k in v.as_object().into_iter().flatten().map(|(k, _)| k) {
            if !SCENARIO_KEYS.contains(&k.as_str()) || (k == "any_text" && !send) {
                bad.push(format!("unexpected scenario key {k:?}"));
            }
        }
    };
    keys_ok(value, bad);
    for key in ["scenario", "extends"] {
        if let Some(name) = value[key].as_str()
            && !dir.join("..").join(name).join("scenario.json").is_file()
        {
            bad.push(format!("{key} {name:?} is not a scenario"));
        }
    }
    for response in value["responses"].as_array().into_iter().flatten() {
        keys_ok(response, bad);
        for word in response["argv"].as_array().into_iter().flatten() {
            let w = word.as_str().unwrap_or_default();
            if !(ARGV_WORDS.contains(&w)
                || (send && SEND_ARGV_WORDS.contains(&w))
                || is_synthetic_id(w))
            {
                bad.push(format!("argument {w:?} is not allowlisted or synthetic"));
            }
        }
        if let Some(file) = response["stdout_file"].as_str()
            && (file.contains('/') || !dir.join(file).is_file())
        {
            bad.push(format!("stdout_file {file:?} is not a file in {dir:?}"));
        }
        for key in ["stdout", "stderr"] {
            if let Some(text) = response[key].as_str()
                && !value_ok(text, send)
            {
                bad.push(format!("{key} {text:?} is not synthetic"));
            }
        }
    }
}

/// Rule 2 for text that is not valid JSON: every double-quoted token, and
/// otherwise the whole text.
fn scan_unparsed(text: &str, bad: &mut Vec<String>, send: bool) {
    let mut tokens = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        match after.find('"') {
            Some(end) => {
                tokens.push(&after[..end]);
                rest = &after[end + 1..];
            }
            None => {
                tokens.push(after);
                break;
            }
        }
    }
    if tokens.is_empty() {
        let t = text.trim();
        if !value_ok(t, send) {
            bad.push(format!("text {t:?} is not synthetic"));
        }
    }
    for t in tokens {
        if !(key_ok(t, send) || value_ok(t, send)) {
            bad.push(format!("token {t:?} is not synthetic"));
        }
    }
}

fn scan_file(path: &Path) -> Vec<String> {
    let text = fs::read_to_string(path).expect("fixtures are UTF-8 text");
    let mut bad: Vec<String> = private_looking_in(&text, folder(path));
    let in_fake = path.starts_with(FAKE);
    let is_envelope = path.components().any(|c| c.as_os_str() == "envelopes");
    if in_fake || path.file_name().is_some_and(|n| n == "README.md") {
        return bad; // source code and prose: rule 1 only
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(value) if is_envelope => walk_envelope(&value, &schema_words(), &mut bad),
        Ok(value) if path.file_name().is_some_and(|n| n == "scenario.json") => {
            scan_scenario(path, &value, &mut bad)
        }
        Ok(value) => walk_in(&value, "$", &mut bad, folder(path)),
        Err(_) => scan_unparsed(&text, &mut bad, is_send_fixture(path)),
    }
    bad
}

/// Every `const` and `enum` word in the schema: OpenWiz's own labels.
fn schema_words() -> Vec<String> {
    fn collect(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    match (k.as_str(), child) {
                        ("const", Value::String(s)) => out.push(s.clone()),
                        ("enum", Value::Array(items)) => {
                            out.extend(items.iter().filter_map(Value::as_str).map(str::to_owned))
                        }
                        _ => collect(child, out),
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|i| collect(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    collect(&crate::schema::load_schema(), &mut out);
    out
}

/// Envelopes use OpenWiz's own keys, so only their string values are
/// checked: a schema label, the source name, or a synthetic value. Reasons
/// and gap details are OpenWiz's own words quoting synthetic values, so only
/// rule 1 applies to them.
fn walk_envelope(value: &Value, words: &[String], bad: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if !matches!(k.as_str(), "unknown" | "reason" | "detail") {
                    walk_envelope(v, words, bad);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| walk_envelope(i, words, bad)),
        Value::String(s) if !(is_synthetic_value(s) || words.contains(s) || s == "room-cli") => {
            bad.push(format!("value {s:?} is not synthetic"));
        }
        _ => {}
    }
}

#[test]
fn fixtures_contain_only_synthetic_values() {
    let files = all_files();
    assert!(files.len() > 20, "the fixtures are there");
    let mut problems = Vec::new();
    for path in &files {
        for p in scan_file(path) {
            problems.push(format!("{}: {p}", path.display()));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn every_scenario_records_its_synthetic_assumptions() {
    let dir = Path::new(FIXTURES).join("room-cli-0.43.1");
    let mut count = 0;
    for entry in fs::read_dir(&dir).expect("scenarios") {
        let path = entry.expect("entry").path().join("scenario.json");
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).expect("scenario"))
            .expect("scenario JSON");
        let name = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(value["scenario"], name.as_str(), "{path:?}");
        assert!(
            value["description"].as_str().is_some_and(|d| !d.is_empty()),
            "{path:?}"
        );
        let assumptions = value["synthetic_assumptions"]
            .as_array()
            .expect("assumptions list");
        assert!(
            !assumptions.is_empty(),
            "{path:?}: label what is not observed"
        );
        count += 1;
    }
    assert!(count >= 13, "every variant has a scenario");
}

#[test]
fn the_scan_catches_planted_private_data() {
    for planted in [
        r#"{"title": "Fix the login page"}"#,
        r#"{"id": "thr_a1b2c3"}"#,
        r#"{"path": "/Users/someone/project"}"#,
        r#"{"gitRemoteUrl": "https://github.com/someone/repo.git"}"#,
        r#"{"email": "someone@example.com"}"#,
        r#"{"id": "123e4567-e89b-12d3-a456-426614174000"}"#,
        r#"{"secretToken": "Synthetic"}"#,
    ] {
        let value: Value = serde_json::from_str(planted).unwrap();
        let mut bad = private_looking(planted);
        walk(&value, "$", &mut bad, false);
        assert!(!bad.is_empty(), "the scan missed {planted}");
    }
    let mut bad = Vec::new();
    scan_unparsed(
        r#"[{"id": "thr_syn_0001", "title": "Real title"#,
        &mut bad,
        false,
    );
    assert!(!bad.is_empty(), "the scan missed a value in truncated JSON");
}

#[test]
fn the_observed_sent_keys_pass_only_in_send_fixtures() {
    for planted in [
        r#"{"ok": true}"#,
        r#"{"mode": "queue"}"#,
        r#"{"threadId":"thr_syn_0001","ok":true,"delivery":"sent","mode":"queue"}"#,
    ] {
        let value: Value = serde_json::from_str(planted).unwrap();
        for outside in [Folder::Other, Folder::NewThread] {
            let mut bad = Vec::new();
            walk_in(&value, "$", &mut bad, outside);
            assert!(!bad.is_empty(), "accepted outside send/: {planted}");
        }
        let mut inside = Vec::new();
        walk_in(&value, "$", &mut inside, Folder::Send);
        assert!(inside.is_empty(), "{planted}: {inside:?}");
    }
}

/// OpenWiz 0.0.3: the documented `thread tell` keys and labels pass only in
/// `send/`; anywhere else they are refused as before.
#[test]
fn the_documented_send_shapes_pass_only_in_send_fixtures() {
    for planted in [
        r#"{"delivery": "sent"}"#,
        r#"{"queuedMessage": {"waitingOn": {"kind": "thread-busy"}}}"#,
        r#"{"requestId": "syn-request-0001"}"#,
        r#"{"status": "queued"}"#,
    ] {
        let value: Value = serde_json::from_str(planted).unwrap();
        let mut outside = Vec::new();
        walk(&value, "$", &mut outside, false);
        assert!(!outside.is_empty(), "accepted outside send/: {planted}");
        let mut inside = Vec::new();
        walk(&value, "$", &mut inside, true);
        assert!(inside.is_empty(), "{planted}: {inside:?}");
    }
    assert!(!value_ok("Error: HTTP 400: Unsupported mode", false));
    assert!(!value_ok("tell", false));
}

/// OpenWiz 0.0.4: the observed `cloud status` keys and labels, and the
/// source-read statuses, pass only in `room-cli-v127/new-thread/`.
#[test]
fn the_new_thread_shapes_pass_only_in_new_thread_fixtures() {
    for planted in [
        r#"{"ready": true, "harnesses": null}"#,
        r#"{"reasoning_levels": ["xhigh", "ultra"]}"#,
        r#"{"status": "pending", "lastError": null}"#,
        r#"{"id": "openclaw"}"#,
    ] {
        let value: Value = serde_json::from_str(planted).unwrap();
        for outside in [Folder::Other, Folder::Send] {
            let mut bad = Vec::new();
            walk_in(&value, "$", &mut bad, outside);
            assert!(!bad.is_empty(), "accepted outside new-thread/: {planted}");
        }
        let mut inside = Vec::new();
        walk_in(&value, "$", &mut inside, Folder::NewThread);
        assert!(inside.is_empty(), "{planted}: {inside:?}");
    }
    for planted in [
        r#"{"account": {"email": "Synthetic"}}"#,
        r#"{"title": "Real title"}"#,
    ] {
        let value: Value = serde_json::from_str(planted).unwrap();
        let mut bad = Vec::new();
        walk_in(&value, "$", &mut bad, Folder::NewThread);
        assert!(!bad.is_empty(), "accepted in new-thread/: {planted}");
    }
}

#[test]
fn observed_queued_identifiers_and_keys_are_scoped() {
    for id in ["qmsg_syn_0001", "00000000-0000-4000-8000-000000000001"] {
        let text = serde_json::to_string(id).unwrap();
        assert!(private_looking_in(&text, Folder::Send).is_empty(), "{id}");
        assert!(value_in(id, Folder::Send), "{id}");
        for outside in [Folder::Other, Folder::NewThread] {
            assert!(!value_in(id, outside), "identifier accepted outside send/");
        }
    }
    for key in [
        "content",
        "mentions",
        "hardQueue",
        "failureReason",
        "editable",
    ] {
        assert!(key_in(key, Folder::Send), "{key}");
        // These are new send-only keys, rather than keys already shared
        // with independently observed inventory/catalogue shapes.
        assert!(!key_in(key, Folder::Other), "{key} accepted outside send/");
    }
    assert!(key_in("payload", Folder::Send));
    assert!(!key_in("payload", Folder::Other));
    for id in [
        "qmsg_syn_0002",
        "qmsg_private",
        "00000000-0000-4000-8000-000000000002",
        "10000000-0000-4000-8000-000000000001",
    ] {
        assert!(!value_in(id, Folder::Send), "other queued id accepted");
    }
    for text in [
        r#"{"synthetic_note":"Synthetic qmsg_private"}"#,
        r#"{"synthetic_note":"00000000-0000-4000-8000-000000000002"}"#,
        r#"{"synthetic_note":"10000000-0000-4000-8000-000000000001"}"#,
        r#"{"synthetic_note":"prefix00000000-0000-4000-8000-000000000001"}"#,
        r#"{"synthetic_note":"00000000-0000-4000-8000-000000000001suffix"}"#,
    ] {
        assert!(!private_looking_in(text, Folder::Send).is_empty(), "{text}");
    }
    for context in [Folder::Send, Folder::Other, Folder::NewThread] {
        for text in [
            "Synthetic qmsg_syn_0002",
            "Synthetic qmsg_syn_0001",
            "Synthetic prefixqmsg_syn_0001",
            r#""qmsg_syn_0002""#,
        ] {
            assert!(
                !private_looking_in(text, context).is_empty(),
                "queued id escaped prose guard: {text}"
            );
        }
    }
    assert!(
        !private_looking_in(r#""00000000-0000-4000-8000-000000000001""#, Folder::Other).is_empty()
    );
    assert!(!private_looking(r#""00000000-0000-4000-8000-000000000001""#).is_empty());
    assert!(
        scan_file(
            &Path::new(FIXTURES).join("room-cli-0.43.1/send/tell-steer-queued-observed.json")
        )
        .is_empty()
    );
}
