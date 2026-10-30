//! Shared helpers for the proof tests: the fake `room-cli`, a fixed clock,
//! and a scripted in-process source for paging, concurrency and budget cases.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_core::envelope::{Envelope, Gap, GapKind};
use openwiz_core::model::{Fact, Inventory, ProjectId, Session, SessionId, SessionView, Timestamp};
use openwiz_core::ops::{self, Clock, Detail, ListRequest, OpError, ShowRequest};
use openwiz_core::source::{
    MachineRecord, OutputRecord, Placement, ProjectRecord, Rows, SessionDetailRecord,
    SessionRecord, SessionScope, SessionSource, SourceError,
};
use serde_json::Value;

/// The fixed snapshot time the fixtures are written against.
pub const T0: Timestamp = Timestamp(1_700_000_600_000);

/// Sessions in the normal default `thread list`, in fixture order.
pub const DEFAULT_IDS: [&str; 6] = [
    "thr_syn_0001",
    "thr_syn_0002",
    "thr_syn_0003",
    "thr_syn_0004",
    "thr_syn_0005",
    "thr_syn_0006",
];
pub const HIDDEN_ID: &str = "thr_syn_0007";
pub const ARCHIVED_ID: &str = "thr_syn_0008";
pub const MISSING_ID: &str = "thr_syn_9999";

pub struct FixedClock(pub Timestamp);

/// A fixed wall clock. Deadlines still need real monotonic time, because
/// the reads they bound really wait.
impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn monotonic(&self) -> Instant {
        Instant::now()
    }
}

pub fn clock() -> FixedClock {
    FixedClock(T0)
}

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/room-cli-0.43.1");

/// A private directory with the fake `room-cli` set to one scenario.
pub struct Fake {
    pub dir: PathBuf,
}

impl Fake {
    /// `test` names the directory, so each test has its own; `scenario` is a
    /// directory under `tests/fixtures/room-cli-0.43.1/`.
    pub fn new(test: &str, scenario: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("fake-room-cli")
            .join(test);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create fake dir");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-room-cli"), dir.join("room-cli"))
            .expect("link fake room-cli");
        let scenario_file = Path::new(FIXTURES).join(scenario).join("scenario.json");
        assert!(scenario_file.is_file(), "no scenario {scenario}");
        fs::write(
            dir.join("fake-room-cli.scenario"),
            scenario_file.to_str().expect("utf-8 path"),
        )
        .expect("write scenario pointer");
        Self { dir }
    }

    /// Points this fake at another scenario. Sources made before still read
    /// from the same directory, so a second run sees the new responses.
    pub fn set_scenario(&self, scenario: &str) {
        let scenario_file = Path::new(FIXTURES).join(scenario).join("scenario.json");
        assert!(scenario_file.is_file(), "no scenario {scenario}");
        fs::write(
            self.dir.join("fake-room-cli.scenario"),
            scenario_file.to_str().expect("utf-8 path"),
        )
        .expect("write scenario pointer");
    }

    pub fn program(&self) -> PathBuf {
        self.dir.join("room-cli")
    }

    pub fn source(&self) -> CloudRoomSource {
        CloudRoomSource::new(self.program())
    }

    /// Every argument list the fake received, in order.
    pub fn invocations(&self) -> Vec<Vec<String>> {
        let Ok(text) = fs::read_to_string(self.dir.join("invocations.log")) else {
            return vec![];
        };
        text.lines()
            .map(|l| serde_json::from_str(l).expect("log line is JSON"))
            .collect()
    }

    /// The process id of a fake that hung, if one did.
    pub fn hung_pid(&self) -> Option<String> {
        fs::read_to_string(self.dir.join("hang.pid")).ok()
    }

    pub fn list(&self, request: &ListRequest) -> Result<Envelope<Inventory>, OpError> {
        ops::list(&self.source(), &clock(), request)
    }

    pub fn show(&self, id: &str) -> Result<Envelope<SessionView>, OpError> {
        ops::show(
            &self.source(),
            &clock(),
            &ShowRequest {
                id: SessionId(id.to_owned()),
                budget: openwiz_core::limits::RUN_BUDGET,
            },
        )
    }

    /// Runs the real `openwiz` program with this fake first on `PATH` and
    /// this fake's directory as its `HOME`: (exit code, stdout, stderr).
    pub fn program_cli(&self, args: &[&str]) -> (i32, String, String) {
        let path = format!(
            "{}:{}",
            self.dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = std::process::Command::new(openwiz_binary())
            .args(args)
            .env("PATH", path)
            .env("HOME", &self.dir)
            .env_remove("XDG_STATE_HOME")
            .current_dir(&self.dir)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run openwiz");
        (
            output.status.code().expect("openwiz exited"),
            String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            String::from_utf8(output.stderr).expect("stderr is UTF-8"),
        )
    }

    /// Runs `openwiz <args>` against this fake: (exit code, stdout, stderr).
    pub fn cli(&self, args: &[&str]) -> (i32, String, String) {
        cli_with(&self.source(), args)
    }

    /// Runs `openwiz <args>` and parses stdout as JSON.
    pub fn cli_json(&self, args: &[&str]) -> Value {
        let (code, out, err) = self.cli(args);
        assert_eq!(code, 0, "openwiz {args:?} failed: {err}");
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {out}"))
    }
}

/// Runs `openwiz <args>` against any source: (exit code, stdout, stderr).
pub fn cli_with(source: &dyn SessionSource, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = openwiz_cli::run(&args, source, &clock(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).expect("stdout is UTF-8"),
        String::from_utf8(err).expect("stderr is UTF-8"),
    )
}

/// Runs `openwiz <args>` against any source and parses stdout as JSON.
pub fn cli_json_with(source: &dyn SessionSource, args: &[&str]) -> Value {
    let (code, out, err) = cli_with(source, args);
    assert_eq!(code, 0, "openwiz {args:?} failed: {err}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {out}"))
}

pub fn long() -> ListRequest {
    ListRequest {
        detail: Detail::Long,
        ..ListRequest::default()
    }
}

pub fn ids(envelope: &Envelope<Inventory>) -> Vec<String> {
    envelope
        .data
        .sessions
        .iter()
        .map(|s| s.id.0.clone())
        .collect()
}

pub fn id_set(envelope: &Envelope<Inventory>) -> BTreeSet<String> {
    ids(envelope).into_iter().collect()
}

pub fn session<'a>(envelope: &'a Envelope<Inventory>, id: &str) -> &'a Session {
    envelope
        .data
        .sessions
        .iter()
        .find(|s| s.id.0 == id)
        .unwrap_or_else(|| panic!("{id} is not in the result"))
}

pub fn has_gap(gaps: &[Gap], kind: GapKind, needle: &str) -> bool {
    gaps.iter().any(|g| {
        g.kind == kind
            && (g.subject.as_deref().unwrap_or("").contains(needle) || g.detail.contains(needle))
    })
}

pub fn unknown_reason<T: std::fmt::Debug>(fact: &Fact<T>) -> &str {
    match fact {
        Fact::Unknown(reason) => {
            assert!(!reason.trim().is_empty(), "an unknown must carry a reason");
            reason
        }
        Fact::Known(v) => panic!("expected unknown, got known {v:?}"),
    }
}

/// The lines of `text` that are `id`'s own row: the first session id on the
/// line is `id`. A child's row may also name its parent; that is not the
/// parent's row. Gap lines (text contract 3) name sessions too; they are not
/// rows.
pub fn row_lines<'a>(text: &'a str, id: &str) -> Vec<&'a str> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with(GAP_PREFIX))
        .filter(|l| {
            l.find("thr_syn_").is_some_and(|i| {
                l[i..].starts_with(id)
                    && !l[i + id.len()..].starts_with(|c: char| c.is_ascii_digit())
            })
        })
        .collect()
}

/// An in-process source with `n` synthetic sessions, for exact control of
/// paging, concurrency and timing.
pub struct ScriptedSource {
    pub records: Vec<SessionRecord>,
    pub detail_delay: Duration,
    in_flight: AtomicUsize,
    pub max_in_flight: AtomicUsize,
    pub detail_calls: Mutex<Vec<String>>,
    pub deadlines: Mutex<Vec<Instant>>,
    /// What `version` reports. Default `v127`.
    pub version: String,
    /// The project list. A test may change it between runs of one source.
    pub projects: Mutex<Vec<ProjectRecord>>,
    /// The machine list. Default empty.
    pub machines: Vec<MachineRecord>,
    /// Model and effort from every `session_detail`.
    pub configuration: (Fact<String>, Fact<String>),
    /// What every `session_output` returns.
    pub output: OutputRecord,
    /// Detail reads of these ids fail with the given error.
    pub failing_details: Vec<(String, SourceError)>,
}

pub fn scripted_project() -> ProjectRecord {
    ProjectRecord {
        id: ProjectId("prj_syn_0001".to_owned()),
        name: Fact::Known("Synthetic project one".to_owned()),
    }
}

pub fn scripted_record(n: usize) -> SessionRecord {
    SessionRecord {
        id: SessionId(format!("thr_syn_{n:04}")),
        project_id: Fact::Known(ProjectId("prj_syn_0001".to_owned())),
        title: Fact::Known(format!("Synthetic session {n}")),
        provider: Fact::Known("codex".to_owned()),
        status: Fact::Known("idle".to_owned()),
        hidden: Fact::Known(false),
        archived: Fact::Known(false),
        parent: Fact::Known(None),
        placement: Fact::Known(Placement::Cloud),
        purpose_text: Fact::Known(Some(format!("Synthetic purpose {n}."))),
        // Distinct times, so any sensible order is total.
        updated_at: Fact::Known(Timestamp(T0.0 - 1_000 * n as i64)),
        problems: vec![],
    }
}

impl ScriptedSource {
    pub fn new(n: usize) -> Self {
        Self::with_records((1..=n).map(scripted_record).collect())
    }

    pub fn with_records(records: Vec<SessionRecord>) -> Self {
        Self {
            records,
            detail_delay: Duration::ZERO,
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
            detail_calls: Mutex::new(vec![]),
            deadlines: Mutex::new(vec![]),
            version: "v127".to_owned(),
            projects: Mutex::new(vec![scripted_project()]),
            machines: vec![],
            configuration: (
                Fact::Known("synthetic-model-a".to_owned()),
                Fact::Known("high".to_owned()),
            ),
            output: OutputRecord {
                output: Fact::Known(Some("Synthetic final output.".to_owned())),
                error: Fact::Known(None),
                problems: vec![],
            },
            failing_details: vec![],
        }
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.detail_delay = delay;
        self
    }

    pub fn with_version(mut self, version: &str) -> Self {
        self.version = version.to_owned();
        self
    }

    pub fn with_projects(self, projects: Vec<ProjectRecord>) -> Self {
        self.set_projects(projects);
        self
    }

    /// Replaces the project list, for the next run of this same source.
    pub fn set_projects(&self, projects: Vec<ProjectRecord>) {
        *self.projects.lock().unwrap() = projects;
    }

    pub fn with_machines(mut self, machines: Vec<MachineRecord>) -> Self {
        self.machines = machines;
        self
    }

    pub fn with_configuration(mut self, model: Fact<String>, effort: Fact<String>) -> Self {
        self.configuration = (model, effort);
        self
    }

    pub fn with_output(mut self, output: OutputRecord) -> Self {
        self.output = output;
        self
    }

    pub fn with_failing_detail(mut self, id: &str, error: SourceError) -> Self {
        self.failing_details.push((id.to_owned(), error));
        self
    }

    fn failure(&self, id: &SessionId) -> Option<SourceError> {
        let (_, error) = self.failing_details.iter().find(|(f, _)| *f == id.0)?;
        self.detail_calls.lock().unwrap().push(id.0.clone());
        Some(error.clone())
    }

    pub fn detail_ids(&self) -> BTreeSet<String> {
        self.detail_calls.lock().unwrap().iter().cloned().collect()
    }

    fn note_deadline(&self, deadline: Instant) {
        self.deadlines.lock().unwrap().push(deadline);
    }

    fn detail_read<T>(
        &self,
        id: &SessionId,
        deadline: Instant,
        make: impl FnOnce() -> T,
    ) -> Result<T, SourceError> {
        self.note_deadline(deadline);
        self.detail_calls.lock().unwrap().push(id.0.clone());
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
        let finish = Instant::now() + self.detail_delay;
        let result = if finish > deadline {
            thread::sleep(deadline.saturating_duration_since(Instant::now()));
            Err(SourceError::TimedOut {
                after_ms: self.detail_delay.as_millis() as u64,
            })
        } else {
            thread::sleep(self.detail_delay);
            Ok(make())
        };
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        result
    }

    fn record(&self, id: &SessionId) -> Result<SessionRecord, SourceError> {
        self.records
            .iter()
            .find(|r| &r.id == id)
            .cloned()
            .ok_or(SourceError::NotFound {
                subject: id.0.clone(),
            })
    }
}

impl SessionSource for ScriptedSource {
    fn name(&self) -> &str {
        "room-cli"
    }

    fn tested_version(&self) -> &str {
        "v127"
    }

    fn version(&self, deadline: Instant) -> Result<String, SourceError> {
        self.note_deadline(deadline);
        Ok(self.version.clone())
    }

    fn list_sessions(
        &self,
        _scope: SessionScope,
        deadline: Instant,
    ) -> Result<Rows<SessionRecord>, SourceError> {
        self.note_deadline(deadline);
        Ok(Rows {
            records: self.records.clone(),
            unreadable: vec![],
        })
    }

    fn list_projects(
        &self,
        _include_hidden: bool,
        deadline: Instant,
    ) -> Result<Rows<ProjectRecord>, SourceError> {
        self.note_deadline(deadline);
        Ok(Rows {
            records: self.projects.lock().unwrap().clone(),
            unreadable: vec![],
        })
    }

    fn list_machines(&self, deadline: Instant) -> Result<Rows<MachineRecord>, SourceError> {
        self.note_deadline(deadline);
        Ok(Rows {
            records: self.machines.clone(),
            unreadable: vec![],
        })
    }

    fn session_detail(
        &self,
        id: &SessionId,
        deadline: Instant,
    ) -> Result<SessionDetailRecord, SourceError> {
        let session = self.record(id)?;
        if let Some(error) = self.failure(id) {
            return Err(error);
        }
        let (model, effort) = self.configuration.clone();
        self.detail_read(id, deadline, || SessionDetailRecord {
            session,
            model,
            effort,
        })
    }

    fn session_output(
        &self,
        id: &SessionId,
        deadline: Instant,
    ) -> Result<OutputRecord, SourceError> {
        self.record(id)?;
        if let Some(error) = self.failure(id) {
            return Err(error);
        }
        let output = self.output.clone();
        self.detail_read(id, deadline, || output)
    }
}

/// Lists through the core with a scripted source and the fixed clock.
pub fn list_scripted(
    source: &ScriptedSource,
    request: &ListRequest,
) -> Result<Envelope<Inventory>, OpError> {
    ops::list(source, &clock(), request)
}

// The text contract.
//
// Provisional: the text output may still change. Text tests rely on these
// points only, and every value they check comes from the JSON:
//
// 1. Line 1 of `ls`, `ls -l` and `show` is the header. It contains
//    `<source> <version>`, `snapshot <UTC time>` (RFC 3339, whole seconds,
//    `Z`) and `complete` or `partial`.
// 2. `ls` prints one line `counts: <key> <value>, <key> <value>, ...` with
//    every key of `coverage.counts`, under its JSON name, in any order.
// 3. Each gap is one line: `gap: <kind>`, then its subject (if any) and its
//    detail. The number of gap lines equals the number of gaps.
// 4. Rows follow their group's header line: `/factories/<display name>/`,
//    or `/factories/_ungrouped/`. The display name is the project name, or
//    `name~<last 6 characters of the id>` when two or more projects in the
//    same run's project list share that name.
// 5. A session's row is the line whose first session id is its own. It
//    contains `age <age>`, `location <location>` and `parent <parent>`, and
//    an ungrouped row its reason. `show` prints the same items, and its
//    group's header line.
// 6. `ls -l` rows and `show` contain
//    `reported configuration: model <model>, effort <effort>`.
// 7. A cut value is printed with `…` right after the kept text.
// 8. The final output is printed verbatim, as one block.
//
// Unknowns print as `unknown (<reason>)` everywhere.

pub const GAP_PREFIX: &str = "gap: ";
pub const COUNTS_PREFIX: &str = "counts: ";
pub const GROUP_ROOT: &str = "/factories/";
pub const UNGROUPED_HEADER: &str = "/factories/_ungrouped/";
pub const CUT_MARKER: char = '…';

/// `ms` since the Unix epoch as RFC 3339 UTC, whole seconds.
pub fn utc_text(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to a civil date (H. Hinnant's algorithm).
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

/// A known age: the whole number of the largest unit (d, h, m, s) it reaches.
pub fn age_text(ms: i64) -> String {
    assert!(ms >= 0, "the text contract defines non-negative ages only");
    let s = ms / 1_000;
    for (unit, size) in [("d", 86_400), ("h", 3_600), ("m", 60)] {
        if s >= size {
            return format!("{}{unit}", s / size);
        }
    }
    format!("{s}s")
}

/// A JSON fact as text: its value, or `unknown (reason)`.
pub fn fact_text(fact: &Value) -> String {
    if let Some(reason) = fact["unknown"].as_str() {
        return format!("unknown ({reason})");
    }
    match &fact["known"] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `age <age>` for a JSON row.
pub fn age_token(row: &Value) -> String {
    match row["age_ms"]["known"].as_i64() {
        Some(ms) => format!("age {}", age_text(ms)),
        None => format!("age {}", fact_text(&row["age_ms"])),
    }
}

/// `location cloud`, `location local on <machine>` or `location unknown (reason)`.
pub fn location_token(row: &Value) -> String {
    let location = &row["location"];
    if location.get("unknown").is_some() {
        return format!("location {}", fact_text(location));
    }
    let known = &location["known"];
    match known["kind"].as_str() {
        Some("cloud") => "location cloud".to_owned(),
        Some("local") => format!("location local on {}", fact_text(&known["machine"])),
        other => panic!("unexpected location kind {other:?}: {location}"),
    }
}

/// `parent none` (top level), `parent <id>` or `parent unknown (reason)`.
pub fn parent_token(row: &Value) -> String {
    match &row["parent"]["known"] {
        Value::Null if row["parent"].get("known").is_some() => "parent none".to_owned(),
        Value::String(id) => format!("parent {id}"),
        _ => format!("parent {}", fact_text(&row["parent"])),
    }
}

/// `reported configuration: model <model>, effort <effort>` for a JSON row.
pub fn configuration_text(row: &Value) -> String {
    let config = &row["reported_configuration"];
    format!(
        "reported configuration: model {}, effort {}",
        fact_text(&config["model"]),
        fact_text(&config["effort"])
    )
}

/// Names shared by two or more projects in the `normal` project list, which
/// every fixture scenario uses.
pub fn shared_project_names() -> BTreeSet<String> {
    let path = Path::new(FIXTURES).join("normal/project-list.json");
    let projects: Value =
        serde_json::from_str(&fs::read_to_string(path).expect("project list")).expect("JSON");
    let names: Vec<&str> = projects
        .as_array()
        .expect("project array")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    names
        .iter()
        .filter(|n| names.iter().filter(|m| m == n).count() > 1)
        .map(|n| (*n).to_owned())
        .collect()
}

/// The last 6 characters of an id.
pub fn last_six(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    chars[chars.len().saturating_sub(6)..].iter().collect()
}

/// The group header line expected for a JSON row's `group`.
pub fn group_header(row: &Value, shared: &BTreeSet<String>) -> String {
    let group = &row["group"];
    match group["kind"].as_str() {
        Some("ungrouped") => UNGROUPED_HEADER.to_owned(),
        Some("project") => {
            let name = fact_text(&group["name"]);
            let id = group["id"].as_str().expect("project id");
            if shared.contains(&name) {
                format!("{GROUP_ROOT}{name}~{}/", last_six(id))
            } else {
                format!("{GROUP_ROOT}{name}/")
            }
        }
        other => panic!("unexpected group kind {other:?}: {group}"),
    }
}

/// The session id that starts a row line, if the line is a row.
pub fn row_id(line: &str) -> Option<String> {
    if line.trim_start().starts_with(GAP_PREFIX) {
        return None;
    }
    let i = line.find("thr_syn_")?;
    let digits: String = line[i + 8..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then(|| format!("thr_syn_{digits}"))
}

/// Each row's id and the group header line above it.
pub fn rows_by_group(text: &str) -> Vec<(String, Option<String>)> {
    let mut header = None;
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with(GROUP_ROOT) && t.ends_with('/') {
            header = Some(t.to_owned());
        } else if let Some(id) = row_id(line) {
            out.push((id, header.clone()));
        }
    }
    out
}

/// The `counts:` line as key, value pairs.
pub fn text_counts(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix(COUNTS_PREFIX))
        .collect();
    assert_eq!(lines.len(), 1, "one counts line:\n{text}");
    lines[0]
        .split(", ")
        .map(|pair| {
            let (k, v) = pair
                .split_once(' ')
                .unwrap_or_else(|| panic!("counts pair {pair:?}"));
            (k.to_owned(), v.to_owned())
        })
        .collect()
}

/// The gap lines of `text`.
pub fn gap_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| l.trim_start().starts_with(GAP_PREFIX))
        .collect()
}

/// A plain string field after the field cap: unchanged if short enough,
/// otherwise its first `FIELD_MAX_CHARS - 1` characters and `…`.
pub fn capped(original: &str) -> String {
    if original.chars().count() <= openwiz_core::limits::FIELD_MAX_CHARS {
        return original.to_owned();
    }
    let mut kept: String = original
        .chars()
        .take(openwiz_core::limits::FIELD_MAX_CHARS - 1)
        .collect();
    kept.push(CUT_MARKER);
    kept
}

/// Builds the `openwiz` program into the test's target directory, once, and
/// returns its path.
pub fn openwiz_binary() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let target = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .parent()
                .expect("the target directory")
                .to_path_buf();
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
            let build = std::process::Command::new(cargo)
                .args(["build", "--locked", "-p", "openwiz-cli"])
                .args(["--bin", "openwiz", "--target-dir"])
                .arg(&target)
                .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
                .output()
                .expect("run cargo build");
            assert!(
                build.status.success(),
                "cargo build failed:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );
            let program = target.join("debug").join("openwiz");
            assert!(program.is_file(), "no {program:?}");
            program
        })
        .clone()
}
