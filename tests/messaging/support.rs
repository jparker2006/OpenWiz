//! Shared helpers for the messaging proofs: the fake `room-cli` set to a
//! send scenario, a private state directory for the send record, the core's
//! `send`, `openwiz send` in-process and as a release process, and the
//! store-level harness.

#![allow(dead_code)]

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_cli::send::{SendContext, random_op_id, run_send_with};
use openwiz_core::messaging::record::{Claim, SendRecord, SendStore, StoreError};
use openwiz_core::messaging::send::{self, Delivery, SendError, SendReport, SendRequest};
use openwiz_core::messaging::{Binding, Mode, OpId};
use openwiz_core::model::{SessionId, Timestamp};
use openwiz_core::ops::Clock;
use openwiz_store::{AttachmentStore, FileStore};
use serde_json::{Value, json};

/// The fixed wall-clock time the fixtures are written against.
pub const T0: Timestamp = Timestamp(1_700_000_600_000);

pub struct FixedClock(pub Timestamp);

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

pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/room-cli-0.43.1");
pub const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// The `send` scenario's directory.
pub fn send_fixtures() -> PathBuf {
    Path::new(FIXTURES).join("send")
}

pub fn op(id: &str) -> OpId {
    OpId::parse(id).unwrap_or_else(|e| panic!("op id {id}: {e}"))
}

pub fn sid(id: &str) -> SessionId {
    SessionId(id.to_owned())
}

/// A request with a short budget, so a hung call ends in seconds.
pub fn request(target: &str, mode: Mode, text: &str, op_id: &str) -> SendRequest {
    let mut r = SendRequest::new(sid(target), text, op(op_id));
    r.mode = mode;
    r.budget = Duration::from_secs(3);
    r
}

/// A tell response for `target` in `mode`, matching any text.
pub fn tell_response(target: &str, mode: &str, rest: Value) -> Value {
    let mut response = json!({
        "argv": ["thread", "tell", target, "--mode", mode, "--json", "--"],
        "any_text": true,
    });
    for (k, v) in rest.as_object().expect("an object") {
        response[k] = v.clone();
    }
    response
}

/// `stdout_file` set to a file of the `send` scenario.
pub fn receipt(file: &str) -> Value {
    json!({ "stdout_file": send_fixtures().join(file).to_str().expect("utf-8") })
}

/// This test run's own directory under the system temp directory,
/// `openwiz-messaging-tests/<process id>-<nanoseconds>`, made once per
/// process. Send records must be outside any repository (M10), so they are
/// not under the target directory; and two suites running at once on one
/// machine must never share them, or one wipes the other's claim mid-test.
/// `private_dir` fails rather than reuse a directory that already exists, so
/// this is never another run's; it is private to its owner, and a planted
/// symlink in place of the shared parent is refused.
pub fn run_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let parent = std::env::temp_dir().join("openwiz-messaging-tests");
        crate::dirs::private_dir_all(&parent).expect("create the messaging tests' temp dir");
        let planted = fs::symlink_metadata(&parent).map_or(true, |m| !m.is_dir());
        assert!(!planted, "{parent:?} is not a plain directory");
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = parent.join(format!("{}-{nanos}", std::process::id()));
        crate::dirs::private_dir(&root).expect("create this run's own temp dir");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("make it private");
        root
    })
}

/// A private fake `room-cli` and send-record directory for one test.
pub struct SendFake {
    pub dir: PathBuf,
    pub state: PathBuf,
}

impl SendFake {
    /// `scenario` is a directory under `tests/fixtures/room-cli-0.43.1/`.
    pub fn new(test: &str, scenario: &str) -> Self {
        let fake = Self::bare(test);
        fake.set_scenario(&Path::new(FIXTURES).join(scenario).join("scenario.json"));
        fake
    }

    /// The `send` scenario with `responses` tried first.
    pub fn with(test: &str, responses: Vec<Value>) -> Self {
        let fake = Self::bare(test);
        fake.override_responses(responses);
        fake
    }

    fn bare(test: &str) -> Self {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("send-fake");
        let dir = base.join(test);
        // The send record must be outside any repository (M10), and the
        // target directory may be inside this one.
        let state = run_root().join(format!("{test}-state"));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&state);
        crate::dirs::private_dir_all(&dir).expect("create fake dir");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-room-cli"), dir.join("room-cli"))
            .expect("link fake room-cli");
        fs::write(
            dir.join("fake-room-cli.state"),
            state.to_str().expect("utf-8 path"),
        )
        .expect("write state pointer");
        Self { dir, state }
    }

    fn set_scenario(&self, file: &Path) {
        assert!(file.is_file(), "no scenario {file:?}");
        fs::write(
            self.dir.join("fake-room-cli.scenario"),
            file.to_str().expect("utf-8 path"),
        )
        .expect("write scenario pointer");
    }

    /// Writes this test's own scenario, extending `send`, and points the
    /// fake at it.
    pub fn override_responses(&self, responses: Vec<Value>) {
        self.override_scenario("send", responses);
    }

    /// Writes this test's own scenario, extending `parent`.
    pub fn override_scenario(&self, parent: &str, responses: Vec<Value>) {
        let own = self.dir.join("own");
        crate::dirs::private_dir_all(&own).expect("own scenario dir");
        let parent = Path::new(FIXTURES).join(parent);
        let scenario = json!({
            "scenario": "own",
            "description": "A test's own responses.",
            "extends": parent.to_str().expect("utf-8"),
            "synthetic_assumptions": ["Written by the test."],
            "responses": responses,
        });
        let file = own.join("scenario.json");
        fs::write(&file, serde_json::to_string_pretty(&scenario).unwrap()).expect("scenario");
        self.set_scenario(&file);
    }

    pub fn program(&self) -> PathBuf {
        self.dir.join("room-cli")
    }

    pub fn source(&self) -> CloudRoomSource {
        CloudRoomSource::new(self.program())
    }

    pub fn store(&self) -> FileStore {
        FileStore::open(&self.state).expect("open the send record")
    }

    /// The core's `send`, in this process, with the fixed clock.
    pub fn send(&self, request: &SendRequest) -> Result<SendReport, SendError> {
        send::send(&self.source(), &self.store(), &clock(), request)
    }

    /// `send`, expecting a report.
    pub fn report(&self, request: &SendRequest) -> SendReport {
        self.send(request)
            .unwrap_or_else(|e| panic!("send failed: {e:?}"))
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

    /// Every `thread tell` the fake received.
    pub fn tells(&self) -> Vec<Vec<String>> {
        self.invocations()
            .into_iter()
            .filter(|a| a.get(1).is_some_and(|w| w == "tell"))
            .collect()
    }

    /// The cross-process tell counter: calls the fake accepted.
    pub fn tell_count(&self) -> u64 {
        fs::read_to_string(self.dir.join("tell-calls.count"))
            .map(|t| t.trim().parse().expect("a count"))
            .unwrap_or(0)
    }

    /// The send records on disk when the fake received tell number `n`.
    pub fn records_at_tell(&self, n: u64) -> Vec<Value> {
        let text = fs::read_to_string(self.dir.join(format!("records-at-tell-{n}.json")))
            .unwrap_or_else(|e| panic!("no records at tell {n}: {e}"));
        serde_json::from_str(&text).expect("records JSON")
    }

    pub fn record_path(&self, op_id: &str) -> PathBuf {
        self.state.join("sends").join(format!("{op_id}.json"))
    }

    /// The record of `op_id`, as JSON.
    pub fn record(&self, op_id: &str) -> Value {
        let text = fs::read_to_string(self.record_path(op_id))
            .unwrap_or_else(|e| panic!("no record for {op_id}: {e}"));
        serde_json::from_str(&text).expect("record JSON")
    }

    /// `openwiz send <args>` in this process: (exit code, stdout, stderr).
    pub fn cli(&self, args: &[&str], stdin: &str) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let source = self.source();
        let state = self.state.clone();
        let read_only_state = state.clone();
        let open_store = move || -> Result<Box<dyn SendStore>, StoreError> {
            Ok(Box::new(FileStore::open(&state)?))
        };
        let open_store_read_only = move || -> Result<Option<Box<dyn SendStore>>, StoreError> {
            let store = FileStore::open_read_only(&read_only_state)?;
            Ok(store.map(|s| Box::new(s) as Box<dyn SendStore>))
        };
        let attachments_state = self.state.clone();
        let open_attachments = move || -> Result<AttachmentStore, StoreError> {
            AttachmentStore::open(&attachments_state)
        };
        let clock = clock();
        let mut input = stdin.as_bytes();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_send_with(
            &args,
            SendContext {
                messenger: &source,
                open_store: &open_store,
                open_store_read_only: &open_store_read_only,
                clock: &clock,
                stdin: &mut input,
                stdin_is_terminal: false,
                new_op_id: &random_op_id,
            },
            &open_attachments,
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).expect("stdout is UTF-8"),
            String::from_utf8(err).expect("stderr is UTF-8"),
        )
    }

    /// `openwiz send <args> --json` in this process, parsed.
    pub fn cli_json(&self, args: &[&str], stdin: &str) -> (i32, Value) {
        let mut all: Vec<&str> = args.to_vec();
        if let Some(i) = all.iter().position(|a| *a == "--") {
            all.insert(i, "--json");
        } else {
            all.push("--json");
        }
        let (code, out, err) = self.cli(&all, stdin);
        let json = serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {out}\nstderr: {err}"));
        (code, json)
    }

    /// The release `openwiz`, as a process, with the fake first on `PATH`,
    /// the fake's directory as its working directory, and this test's state
    /// directory.
    pub fn process(&self, args: &[&str], stdin: &str) -> Command {
        let _ = stdin;
        let mut command = Command::new(openwiz_release());
        command
            .args(args)
            .env("PATH", self.path())
            .env("OPENWIZ_STATE_DIR", &self.state)
            .current_dir(&self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// Runs the release `openwiz` to its end.
    pub fn run_process(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = self.process(args, stdin).spawn().expect("start openwiz");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("openwiz output")
    }

    pub fn path(&self) -> String {
        format!(
            "{}:{}",
            self.dir.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    /// The store-level harness, as a process.
    pub fn harness(
        &self,
        target: &str,
        mode: &str,
        op_id: &str,
        crash: &str,
        text: &str,
    ) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_send-harness"));
        command
            .args([
                self.state.to_str().expect("utf-8"),
                self.program().to_str().expect("utf-8"),
                target,
                mode,
                op_id,
                crash,
                text,
            ])
            .env("OPENWIZ_STATE_DIR", &self.state)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

/// Best-effort clean-up: a test that passed removes its own send records;
/// one that failed keeps them for diagnosis. Only this run's directory is
/// touched.
impl Drop for SendFake {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.state);
        }
    }
}

/// Builds the release `openwiz` once per test run, as P10 does.
pub fn openwiz_release() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    BUILT
        .get_or_init(|| {
            let target = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .parent()
                .expect("the target directory")
                .to_path_buf();
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
            let build = Command::new(cargo)
                .args(["build", "--release", "--locked", "-p", "openwiz-cli"])
                .args(["--bin", "openwiz", "--target-dir"])
                .arg(&target)
                .current_dir(ROOT)
                .output()
                .expect("run cargo build --release");
            assert!(
                build.status.success(),
                "cargo build --release failed:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );
            target.join("release").join("openwiz")
        })
        .clone()
}

/// (exit code, stdout as JSON) of a finished process.
pub fn json_of(output: &Output) -> (Option<i32>, Value) {
    let out = String::from_utf8_lossy(&output.stdout);
    let json = serde_json::from_str(&out).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {out}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code(), json)
}

/// The Unix permission bits of `path`.
pub fn mode_of(path: &Path) -> u32 {
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

// The schema.

pub const MESSAGING_SCHEMA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../contracts/messaging.v1.schema.json"
);

pub fn messaging_schema() -> Value {
    serde_json::from_str(&fs::read_to_string(MESSAGING_SCHEMA).expect("schema")).expect("JSON")
}

/// Validates `value` against `$defs/<def>` of the messaging schema.
pub fn validate_def(def: &str, value: &Value) -> Vec<String> {
    let mut schema = messaging_schema();
    schema["$ref"] = json!(format!("#/$defs/{def}"));
    crate::schema::validate(&schema, value)
}

/// Validates a report (the schema's root).
pub fn validate_report(value: &Value) -> Vec<String> {
    crate::schema::validate(&messaging_schema(), value)
}

/// Every string in `value`, recursively.
pub fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        Value::Object(map) => map.iter().for_each(|(k, v)| {
            out.push(k.clone());
            strings(v, out)
        }),
        _ => {}
    }
}

// Sends in flight.

/// A send of `op_id` claimed by another live sender: a second store, in
/// this process, that holds the op id's lock until it is dropped or
/// released. Its record is in the claimed state.
pub struct Held {
    pub store: FileStore,
    pub record: SendRecord,
}

impl Held {
    pub fn claim(fake: &SendFake, target: &str, mode: Mode, text: &str, op_id: &str) -> Self {
        let store = fake.store();
        let record =
            SendRecord::claimed(op(op_id), Binding::new(sid(target), mode, text), text, T0);
        assert_eq!(store.claim(&record, T0).expect("claim"), Claim::Owned);
        Self { store, record }
    }

    /// Marks the held send dispatching, as just before its one call.
    pub fn dispatching(&mut self) {
        self.record = self.record.dispatching(T0);
        self.store.update(&self.record).expect("mark dispatching");
    }

    /// Finishes the held send with `delivery` and lets go of it.
    pub fn finish(self, delivery: Delivery) {
        let finished = self.record.finished(delivery, T0);
        self.store.update(&finished).expect("finish");
        self.store.release(&self.record.op_id);
    }

    /// The sender stops: its claim is let go and its record left as it is.
    pub fn stop(self) {
        self.store.release(&self.record.op_id);
        wait_lock_free(&self.store, &self.record.op_id);
    }

    /// The sender withdraws its claim, as after a refusal found after it.
    pub fn withdraw(self) {
        self.store.withdraw(&self.record.op_id).expect("withdraw");
        wait_lock_free(&self.store, &self.record.op_id);
    }
}

/// Waits until no process holds `op_id`'s lock. Another test's thread may
/// have forked a child while this process held it, and that child keeps a
/// copy of the lock until it runs its program; a later fork can't copy a
/// lock this process has already let go.
pub fn wait_lock_free(store: &FileStore, op_id: &OpId) {
    let path = store.sends_dir().join(format!("{op_id}.lock"));
    let started = Instant::now();
    loop {
        let file = fs::File::open(&path).expect("open the lock file");
        if file.try_lock().is_ok() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{op_id}'s lock stayed held"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The names of the files in the send record's `sends/` directory, with
/// their contents.
pub fn sends_listing(fake: &SendFake) -> Vec<(String, Vec<u8>)> {
    let Ok(entries) = fs::read_dir(fake.state.join("sends")) else {
        return vec![];
    };
    let mut files: Vec<(String, Vec<u8>)> = entries
        .map(|e| e.expect("entry").path())
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read(&p).unwrap_or_default())
        })
        .collect();
    files.sort();
    files
}
