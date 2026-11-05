//! Helpers for the new-thread routes: a private fake `room-cli`
//! and new-thread record per test, an in-process server with the send and
//! new-thread configurations, the fixed clock `openwiz new`'s proofs use,
//! `openwiz new` and `openwiz models` in-process, and raw `POST` clients.
//!
//! **The scenario is written by the test**, never committed (the fixture
//! scan accepts no `cloud`, `status` or `spawn` word in a committed
//! scenario, and a create needs a project with a GitHub remote), as in
//! `tests/new-thread/fake.rs`. Every value is synthetic. The committed
//! stdout files of `room-cli-v127/new-thread/` are served by absolute path.

#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_cli::new_thread::{NewContext, run_models, run_new};
use openwiz_cli::send::random_op_id;
use openwiz_core::messaging::record::StoreError;
use openwiz_core::model::Timestamp;
use openwiz_core::new_thread::record::NewThreadStore;
use openwiz_core::ops::Clock;
use openwiz_server::{
    Config, Creating, Limits, Log, OpenNewThreadStore, OpenNewThreadStoreReadOnly, SendHost,
};
use openwiz_store::NewThreadFileStore;
use serde_json::{Value, json};

use crate::http::{Response, TestServer};
use crate::send_support::{HOST, good_headers, raw};

/// The fixed wall-clock time `openwiz new`'s proofs are written against.
pub const T0: Timestamp = Timestamp(1_700_000_600_000);

pub const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/room-cli-v127/new-thread"
);

/// The 0.43.1 inventory scenario the new-thread scenarios extend.
const NORMAL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/room-cli-0.43.1/normal/scenario.json"
);

/// The ready project (a GitHub remote), the receipt's thread, and an
/// offered choice of the committed catalogue.
pub const PROJECT: &str = "prj_syn_0001";
pub const NO_REMOTE: &str = "prj_syn_0002";
pub const PERSONAL: &str = "prj_syn_0003";
pub const THREAD: &str = "thr_syn_0021";
pub const HARNESS: &str = "codex";
pub const MODEL: &str = "syn-codex-model-01";
pub const EFFORT: &str = "high";
pub const TEXT: &str = "Synthetic A2 prompt: reply ok";

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

/// A committed stdout file of the new-thread fixtures, by absolute path.
pub fn file(name: &str) -> String {
    let path = Path::new(FIXTURES).join(name);
    assert!(path.is_file(), "no fixture {path:?}");
    path.to_str().expect("utf-8").to_owned()
}

fn github(n: &str) -> String {
    format!("https://github.com/synthetic-owner/synthetic-repo-{n}.git")
}

fn project(id: &str, name: &str, kind: &str, remote: Value) -> Value {
    json!({
        "id": id,
        "kind": kind,
        "name": name,
        "gitRemoteUrl": remote,
        "createdAt": 1_699_100_600_000_i64,
        "updatedAt": 1_699_200_600_000_i64,
        "sources": [],
    })
}

/// One ready project, one without a remote, one personal, one whose remote
/// is not GitHub.
pub fn project_list() -> Value {
    json!([
        project(
            PROJECT,
            "Synthetic project one",
            "standard",
            json!(github("0001"))
        ),
        project(NO_REMOTE, "Synthetic project two", "standard", Value::Null),
        project(PERSONAL, "Synthetic personal", "personal", Value::Null),
        project(
            "prj_syn_0006",
            "Synthetic elsewhere",
            "standard",
            json!("https://example.invalid/synthetic/repo-0006.git"),
        ),
    ])
}

/// The default create: the receipt of `thr_syn_0021` in 0001.
pub fn created() -> Value {
    json!({"any_spawn": true, "stdout_file": file("spawn-receipt-created.json"), "creates": true})
}

/// The responses every scenario ends with: the version, the project lists,
/// the catalogue, one create, the new thread's `thread show` (active: a
/// follow ends after one read) and the thread list.
pub fn defaults() -> Vec<Value> {
    vec![
        json!({"argv": ["--version"], "stdout": "v127\n"}),
        json!({
            "argv": ["project", "list", "--json", "--include-personal"],
            "stdout": project_list().to_string(),
        }),
        json!({
            "argv": ["project", "list", "--json", "--include-personal", "--hidden"],
            "stdout": "[]",
        }),
        json!({"argv": ["cloud", "status", "--json"], "stdout_file": file("cloud-status.json")}),
        created(),
        json!({
            "argv": ["thread", "show", THREAD, "--json"],
            "stdout_file": file("thread-show-active.json"),
        }),
        json!({"argv": ["thread", "list", "--json"], "stdout_file": file("thread-list.json")}),
    ]
}

/// This run's own directory for records, outside every repository.
fn run_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let parent = std::env::temp_dir().join("openwiz-web-new-thread");
        crate::dirs::private_dir_all(&parent).expect("temp parent");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = parent.join(format!("{}-{nanos}", std::process::id()));
        crate::dirs::private_dir(&root).expect("this run's own temp dir");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private");
        root
    })
}

/// A private fake `room-cli` and new-thread record for one test.
pub struct NewFake {
    pub dir: PathBuf,
    pub state: PathBuf,
    /// Every wait the in-process `openwiz new` asked for.
    pub waits: Mutex<Vec<Duration>>,
}

impl NewFake {
    pub fn new(test: &str) -> Self {
        Self::with(test, vec![])
    }

    /// `responses` tried first, then [`defaults`], then the 0.43.1
    /// inventory.
    pub fn with(test: &str, responses: Vec<Value>) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("web-new-thread-fake")
            .join(test);
        let state = run_root().join(format!("{test}-state"));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&state);
        crate::dirs::private_dir_all(&dir).expect("fake dir");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-room-cli"), dir.join("room-cli"))
            .expect("link the fake");
        fs::write(dir.join("fake-room-cli.state"), state.to_str().unwrap()).expect("state");
        let fake = Self {
            dir,
            state,
            waits: Mutex::new(vec![]),
        };
        fake.respond(responses);
        fake
    }

    pub fn respond(&self, mut responses: Vec<Value>) {
        responses.extend(defaults());
        let scenario = json!({
            "scenario": "own",
            "description": "An A2 test's own responses.",
            "extends": Path::new(NORMAL).parent().unwrap().to_str().unwrap(),
            "synthetic_assumptions": ["Written by the test; every value is synthetic."],
            "responses": responses,
        });
        let own = self.dir.join("own");
        crate::dirs::private_dir_all(&own).expect("own dir");
        let file = own.join("scenario.json");
        fs::write(&file, serde_json::to_string_pretty(&scenario).unwrap()).expect("scenario");
        fs::write(
            self.dir.join("fake-room-cli.scenario"),
            file.to_str().unwrap(),
        )
        .expect("scenario pointer");
    }

    pub fn source(&self) -> CloudRoomSource {
        CloudRoomSource::new(self.dir.join("room-cli"))
    }

    /// Every argument list the fake received, in order.
    pub fn invocations(&self) -> Vec<Vec<String>> {
        let Ok(text) = fs::read_to_string(self.dir.join("invocations.log")) else {
            return vec![];
        };
        text.lines()
            .map(|l| serde_json::from_str(l).expect("a JSON line"))
            .collect()
    }

    /// Every `thread spawn` the fake received.
    pub fn spawns(&self) -> Vec<Vec<String>> {
        self.invocations()
            .into_iter()
            .filter(|a| a.get(1).is_some_and(|w| w == "spawn"))
            .collect()
    }

    /// Every write of any kind the fake received (`tell` or `spawn`).
    pub fn writes(&self) -> Vec<Vec<String>> {
        self.invocations()
            .into_iter()
            .filter(|a| a.get(1).is_some_and(|w| w == "spawn" || w == "tell"))
            .collect()
    }

    fn counter(&self, name: &str) -> u64 {
        fs::read_to_string(self.dir.join(name))
            .map(|t| t.trim().parse().expect("a count"))
            .unwrap_or(0)
    }

    /// The spawn calls the fake accepted.
    pub fn spawn_count(&self) -> u64 {
        self.counter("spawn-calls.count")
    }

    /// Threads the fake's request-ID table created.
    pub fn threads_created(&self) -> u64 {
        self.counter("threads-created.count")
    }

    pub fn record_path(&self, op_id: &str) -> PathBuf {
        self.state.join("new-threads").join(format!("{op_id}.json"))
    }

    /// Whether any new-thread record exists at all.
    pub fn any_record(&self) -> bool {
        fs::read_dir(self.state.join("new-threads")).is_ok_and(|mut d| {
            d.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".json")))
        })
    }

    /// `openwiz new|models <args>` in this process, with the fixed clock and
    /// a wait that records instead of sleeping: (exit code, stdout, stderr).
    pub fn cli(&self, args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let source = self.source();
        let clock = clock();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = match args.first().map(String::as_str) {
            Some("new") => {
                let state = self.state.clone();
                let read_only = state.clone();
                let open_store = move || -> Result<Box<dyn NewThreadStore>, StoreError> {
                    Ok(Box::new(NewThreadFileStore::open(&state)?))
                };
                let open_store_read_only =
                    move || -> Result<Option<Box<dyn NewThreadStore>>, StoreError> {
                        let store = NewThreadFileStore::open_read_only(&read_only)?;
                        Ok(store.map(|s| Box::new(s) as Box<dyn NewThreadStore>))
                    };
                let wait = |d: Duration| self.waits.lock().unwrap().push(d);
                let mut input: &[u8] = b"";
                run_new(
                    &args[1..],
                    NewContext {
                        client: &source,
                        open_store: &open_store,
                        open_store_read_only: &open_store_read_only,
                        clock: &clock,
                        stdin: &mut input,
                        stdin_is_terminal: false,
                        new_op_id: &random_op_id,
                        wait: &wait,
                    },
                    &mut out,
                    &mut err,
                )
            }
            Some("models") => run_models(&args[1..], &source, &clock, &mut out, &mut err),
            other => panic!("not a new-thread command: {other:?}"),
        };
        (
            code,
            String::from_utf8(out).expect("stdout is UTF-8"),
            String::from_utf8(err).expect("stderr is UTF-8"),
        )
    }
}

impl Drop for NewFake {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.state);
        }
    }
}

/// The new-thread record and provider of `fake`, opened as `openwiz new`
/// opens them.
pub fn creating(fake: &NewFake) -> Creating {
    let state = fake.state.clone();
    let read_only = fake.state.clone();
    let open_store: OpenNewThreadStore =
        Arc::new(
            move || Ok(Box::new(NewThreadFileStore::open(&state)?) as Box<dyn NewThreadStore>),
        );
    let open_store_read_only: OpenNewThreadStoreReadOnly = Arc::new(
        move || -> Result<Option<Box<dyn NewThreadStore>>, StoreError> {
            let store = NewThreadFileStore::open_read_only(&read_only)?;
            Ok(store.map(|s| Box::new(s) as Box<dyn NewThreadStore>))
        },
    );
    Creating {
        client: Arc::new(fake.source()),
        open_store,
        open_store_read_only,
        host: Some(SendHost::new(HOST).expect("a public host")),
    }
}

/// The config of a new-thread server: the public host name and the record.
pub fn new_config(fake: &NewFake, limits: Limits) -> Config {
    Config {
        allowed_host: Some(HOST.to_owned()),
        web_root: None,
        limits,
        sending: None,
        creating: Some(creating(fake)),
        attaching: None,
    }
}

/// An in-process server over `fake`, with the fixed clock.
pub fn serve_config(fake: &NewFake, config: Config) -> TestServer {
    let listener = openwiz_server::bind(0).expect("bind loopback");
    let log = Arc::new(Mutex::new(Vec::new()));
    let lines = log.clone();
    let sink: Log = Arc::new(move |line: &str| lines.lock().unwrap().push(line.to_owned()));
    let server = openwiz_server::start(
        listener,
        config,
        Arc::new(fake.source()),
        Arc::new(clock()),
        sink,
    )
    .expect("start the server");
    let addr = server.local_addr();
    TestServer { server, addr, log }
}

pub fn serve_new(fake: &NewFake) -> TestServer {
    serve_config(fake, new_config(fake, Limits::default()))
}

pub fn serve_new_limits(fake: &NewFake, limits: Limits) -> TestServer {
    serve_config(fake, new_config(fake, limits))
}

/// The create route of `project`.
pub fn threads(project: &str) -> String {
    format!("/api/v1/projects/{project}/threads")
}

/// The new-thread preview route of `project`.
pub fn preview(project: &str) -> String {
    format!("/api/v1/projects/{project}/threads/preview")
}

/// The status route of `op_id`.
pub fn status(op_id: &str) -> String {
    format!("/api/v1/new-threads/{op_id}")
}

pub const OPTIONS: &str = "/api/v1/cloud-options";

/// A create body with the offered default choice.
pub fn body(op_id: &str, text: &str) -> Value {
    body_with(op_id, HARNESS, MODEL, EFFORT, text)
}

pub fn body_with(op_id: &str, harness: &str, model: &str, effort: &str, text: &str) -> Value {
    json!({
        "op_id": op_id,
        "harness": harness,
        "model": model,
        "effort": effort,
        "text": text,
        "attachments": [],
    })
}

/// `POST` `payload` to `target` with `headers`.
pub fn post(
    s: &TestServer,
    target: &str,
    headers: &[(String, String)],
    payload: &[u8],
) -> Response {
    s.send(&raw("POST", target, headers, payload, true))
}

/// A same-origin create of `value` in `project`.
pub fn create(s: &TestServer, project: &str, value: &Value) -> Response {
    post(
        s,
        &threads(project),
        &good_headers(),
        value.to_string().as_bytes(),
    )
}

/// A same-origin preview of `value` in `project`.
pub fn preview_json(s: &TestServer, project: &str, value: &Value) -> Response {
    post(
        s,
        &preview(project),
        &good_headers(),
        value.to_string().as_bytes(),
    )
}

/// `GET target` with the public host name.
pub fn get_public(s: &TestServer, target: &str) -> Response {
    s.send(&raw(
        "GET",
        target,
        &[("Host".to_owned(), HOST.to_owned())],
        b"",
        false,
    ))
}

/// The `openwiz new` arguments for a create body in `project`, as the
/// contract maps them: `--yes` for a create, none for a preview.
pub fn cli_args(project: &str, value: &Value, yes: bool) -> Vec<String> {
    let s = |key: &str| value[key].as_str().expect(key).to_owned();
    let mut args = vec![
        "new".to_owned(),
        "--project-id".to_owned(),
        project.to_owned(),
        "--harness".to_owned(),
        s("harness"),
        "--model".to_owned(),
        s("model"),
        "--effort".to_owned(),
        s("effort"),
        "--op-id".to_owned(),
        s("op_id"),
    ];
    if yes {
        args.push("--yes".to_owned());
    }
    args.extend(["--json".to_owned(), "--".to_owned(), s("text")]);
    args
}

/// `openwiz new …` for `args`, parsed: (exit code, stdout as JSON).
pub fn cli_json(fake: &NewFake, args: &[String]) -> (i32, Value) {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, out, err) = fake.cli(&args);
    let json = serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {out}\nstderr: {err}"));
    (code, json)
}
