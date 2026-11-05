//! The new-thread proofs' fake `room-cli` (A1 phase 2): a private copy of
//! the repository's fake per test, a scenario the test writes, a private
//! state directory for the new-thread record, `openwiz new` and `openwiz
//! models` in-process and as the release build, and the self-exec harness.
//!
//! **The scenario is written by the test**, not committed: the fixture scan
//! (`tests/checks/fixture_scan.rs`) accepts no URL outside `example.invalid`
//! and no `cloud`, `status` or `spawn` argument word in a committed
//! scenario, and a create needs a project with a GitHub remote. Every value
//! here is synthetic. The committed stdout files of
//! `room-cli-v127/new-thread/` are served by absolute path.

#![allow(dead_code)]

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_cli::new_thread::{NewContext, run_models, run_new_with};
use openwiz_cli::send::random_op_id;
use openwiz_core::messaging::record::StoreError;
use openwiz_core::new_thread::record::NewThreadStore;
use openwiz_store::{AttachmentStore, NewThreadFileStore};
use serde_json::{Value, json};

use crate::support::{FIXTURES, ROOT, clock};

/// The 0.43.1 inventory scenario the new-thread scenarios extend, for the
/// reads `ls`, `show` and the start make.
pub const NORMAL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/room-cli-0.43.1/normal/scenario.json"
);

/// The project the default spawn answers for, and the receipt's thread.
pub const PROJECT: &str = "prj_syn_0001";
pub const THREAD: &str = "thr_syn_0021";

/// A committed stdout file of the new-thread fixtures, by absolute path.
pub fn file(name: &str) -> String {
    let path = Path::new(FIXTURES).join(name);
    assert!(path.is_file(), "no fixture {path:?}");
    path.to_str().expect("utf-8").to_owned()
}

/// A synthetic GitHub remote. Only in test-written scenarios (see the
/// module documentation).
pub fn github(n: &str) -> String {
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

/// The projects `project list --json --include-personal` returns: one ready
/// (0001), one without a remote, one personal, two sharing a name (each
/// with a GitHub remote, in the scp-like and ssh forms), one with a remote
/// that is not GitHub, a lookalike host, and one whose name is shell syntax.
pub fn project_list() -> Value {
    json!([
        project(
            "prj_syn_0001",
            "Synthetic project one",
            "standard",
            json!(github("0001"))
        ),
        project(
            "prj_syn_0002",
            "Synthetic project two",
            "standard",
            Value::Null
        ),
        project(
            "prj_syn_0003",
            "Synthetic personal",
            "personal",
            Value::Null
        ),
        project(
            "prj_syn_0004",
            "Synthetic shared",
            "standard",
            json!("git@github.com:synthetic-owner/synthetic-repo-0004.git"),
        ),
        project(
            "prj_syn_0005",
            "Synthetic shared",
            "standard",
            json!("ssh://git@github.com/synthetic-owner/synthetic-repo-0005.git"),
        ),
        project(
            "prj_syn_0006",
            "Synthetic elsewhere",
            "standard",
            json!("https://example.invalid/synthetic/repo-0006.git"),
        ),
        project(
            "prj_syn_0008",
            "Synthetic lookalike",
            "standard",
            json!("https://github.com.example.invalid/synthetic/repo-0008.git"),
        ),
        project(
            "prj_syn_0009",
            SHELL_SYNTAX_NAME,
            "standard",
            json!(github("0009")),
        ),
    ])
}

/// A project name made of shell syntax : it must reach a
/// printed command quoted, as free text.
pub const SHELL_SYNTAX_NAME: &str = "Synthetic ~ ~root =ls {a,b} * $HOME `x` it's !x";

/// The hidden projects (`--hidden`): OpenWiz never reads them for a create.
pub fn hidden_project_list() -> Value {
    json!([project(
        "prj_syn_0007",
        "Synthetic hidden",
        "standard",
        json!(github("0007"))
    )])
}

/// The default spawn answer: the receipt of `thr_syn_0021` in 0001, a new
/// thread.
pub fn created() -> Value {
    json!({"any_spawn": true, "stdout_file": file("spawn-receipt-created.json"), "creates": true})
}

/// The responses every new-thread scenario starts from: the version, the
/// project lists, the catalogue, one create, and the new thread's `thread
/// show` (active, so a follow ends after one read).
pub fn defaults() -> Vec<Value> {
    vec![
        json!({"argv": ["--version"], "stdout": "v127\n"}),
        json!({
            "argv": ["project", "list", "--json", "--include-personal"],
            "stdout": project_list().to_string(),
        }),
        json!({
            "argv": ["project", "list", "--json", "--include-personal", "--hidden"],
            "stdout": hidden_project_list().to_string(),
        }),
        json!({"argv": ["cloud", "status", "--json"], "stdout_file": file("cloud-status.json")}),
        created(),
        json!({
            "argv": ["thread", "show", THREAD, "--json"],
            "stdout_file": file("thread-show-active.json"),
        }),
    ]
}

/// This test run's own directory under the system temp directory, made
/// once per process; records must be outside every repository.
pub fn run_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let parent = std::env::temp_dir().join("openwiz-new-thread-fake");
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

/// Overrides where the fakes are made: a child run of this test binary
/// (`n18_cli`'s R4 test) gets its own, so it never touches the parent's.
pub const FAKE_ROOT_ENV: &str = "OPENWIZ_NEW_THREAD_FAKE_ROOT";

fn fake_root() -> PathBuf {
    std::env::var_os(FAKE_ROOT_ENV).map_or_else(
        || Path::new(env!("CARGO_TARGET_TMPDIR")).join("new-thread-fake"),
        PathBuf::from,
    )
}

/// A private fake `room-cli` and state directory for one test.
pub struct NewFake {
    pub dir: PathBuf,
    pub state: PathBuf,
    /// Every wait the in-process `new` asked for.
    pub waits: Mutex<Vec<Duration>>,
}

impl NewFake {
    /// The default responses.
    pub fn new(test: &str) -> Self {
        Self::with(test, vec![])
    }

    /// `responses` tried first, then the defaults.
    pub fn with(test: &str, responses: Vec<Value>) -> Self {
        let dir = fake_root().join(test);
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

    /// Replaces the scenario: `responses` first, then the defaults, then the
    /// 0.43.1 inventory.
    pub fn respond(&self, mut responses: Vec<Value>) {
        responses.extend(defaults());
        let scenario = json!({
            "scenario": "own",
            "description": "A new-thread test's own responses.",
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

    pub fn program(&self) -> PathBuf {
        self.dir.join("room-cli")
    }

    pub fn source(&self) -> CloudRoomSource {
        CloudRoomSource::new(self.program())
    }

    pub fn store(&self) -> NewThreadFileStore {
        NewThreadFileStore::open(&self.state).expect("open the new-thread record")
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

    /// The cross-process spawn counter: calls the fake accepted.
    pub fn spawn_count(&self) -> u64 {
        self.counter("spawn-calls.count")
    }

    /// Threads the fake's request-ID table created.
    pub fn threads_created(&self) -> u64 {
        self.counter("threads-created.count")
    }

    /// The new-thread records on disk when the fake received spawn `n`.
    pub fn records_at_spawn(&self, n: u64) -> Vec<Value> {
        let text = fs::read_to_string(self.dir.join(format!("records-at-spawn-{n}.json")))
            .unwrap_or_else(|e| panic!("no records at spawn {n}: {e}"));
        serde_json::from_str(&text).expect("records JSON")
    }

    pub fn record_path(&self, op_id: &str) -> PathBuf {
        self.state.join("new-threads").join(format!("{op_id}.json"))
    }

    /// The record of `op_id`, as JSON.
    pub fn record(&self, op_id: &str) -> Value {
        let text = fs::read_to_string(self.record_path(op_id))
            .unwrap_or_else(|e| panic!("no record for {op_id}: {e}"));
        serde_json::from_str(&text).expect("record JSON")
    }

    /// `openwiz new|models|... <args>` in this process, with the fixed clock
    /// and a wait that records instead of sleeping: (exit code, stdout,
    /// stderr).
    pub fn cli(&self, args: &[&str], stdin: &str) -> (i32, String, String) {
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
                let attachments_state = self.state.clone();
                let open_attachments = move || -> Result<AttachmentStore, StoreError> {
                    AttachmentStore::open(&attachments_state)
                };
                let mut input = stdin.as_bytes();
                run_new_with(
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
                    &open_attachments,
                    &mut out,
                    &mut err,
                )
            }
            Some("models") => run_models(&args[1..], &source, &clock, &mut out, &mut err),
            _ => openwiz_cli::run(&args, &source, &clock, &mut out, &mut err),
        };
        (
            code,
            String::from_utf8(out).expect("stdout is UTF-8"),
            String::from_utf8(err).expect("stderr is UTF-8"),
        )
    }

    /// `cli` with `--json` added before any `--`, parsed.
    pub fn cli_json(&self, args: &[&str], stdin: &str) -> (i32, Value) {
        let mut all: Vec<&str> = args.to_vec();
        match all.iter().position(|a| *a == "--") {
            Some(i) => all.insert(i, "--json"),
            None => all.push("--json"),
        }
        let (code, out, err) = self.cli(&all, stdin);
        let json = serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {out}\nstderr: {err}"));
        (code, json)
    }

    pub fn path(&self) -> String {
        format!(
            "{}:{}",
            self.dir.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    /// The release `openwiz`, with the fake first on `PATH` and this test's
    /// state directory.
    pub fn process(&self, args: &[&str]) -> Command {
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
        let mut child = self.process(args).spawn().expect("start openwiz");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("openwiz output")
    }

    /// The self-exec harness (`crate::harness`), as a process: the core's
    /// `new_thread` with the file-backed record and the adapter, called the
    /// way a server would, with an optional crash point.
    /// Returns the command and the file its report is written to.
    pub fn harness(&self, op_id: &str, crash: &str, text: &str) -> (Command, PathBuf) {
        self.harness_with(op_id, crash, text, &[])
    }

    /// [`harness`](Self::harness) with staged attachment ids (B2).
    pub fn harness_with(
        &self,
        op_id: &str,
        crash: &str,
        text: &str,
        attachments: &[&str],
    ) -> (Command, PathBuf) {
        let out = self.dir.join(format!("harness-{op_id}-{}.json", unique()));
        let spec = json!({
            "state": self.state,
            "program": self.program(),
            "op_id": op_id,
            "crash": crash,
            "text": text,
            "out": out,
            "attachments": attachments,
        });
        let mut command = Command::new(std::env::current_exe().expect("this test binary"));
        command
            .args([
                "--exact",
                "harness::harness_process",
                "--nocapture",
                "--test-threads=1",
                "-q",
            ])
            .env(crate::harness::HARNESS_ENV, spec.to_string())
            .env("OPENWIZ_STATE_DIR", &self.state)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        (command, out)
    }
}

impl Drop for NewFake {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.state);
        }
    }
}

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// Builds the release `openwiz` once per test run, as P10 and M14 do.
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

/// The exact `thread spawn` OpenWiz makes for `op_id`'s create in `project`
/// with codex, `syn-codex-model-01`, `high` and `text`.
pub fn spawn_argv(project: &str, op_id: &str, text: &str) -> Vec<String> {
    let op = openwiz_core::messaging::OpId::parse(op_id).unwrap();
    let rid = openwiz_core::new_thread::request_id(&op);
    [
        "thread",
        "spawn",
        "--project",
        project,
        "--machine",
        "cloud",
        "--provider",
        "codex",
        "--model",
        "syn-codex-model-01",
        "--reasoning-level",
        "high",
        "--request-id",
        &rid,
        "--json",
        "--prompt",
        text,
    ]
    .iter()
    .map(|w| (*w).to_owned())
    .collect()
}

/// `new <project> --harness codex --model syn-codex-model-01 --effort high
/// --op-id <op> --yes`, then `extra`, then `-- <text>`.
pub fn new_args<'a>(
    project: &'a str,
    op_id: &'a str,
    extra: &[&'a str],
    text: &'a str,
) -> Vec<&'a str> {
    let mut args = vec![
        "new",
        project,
        "--harness",
        "codex",
        "--model",
        "syn-codex-model-01",
        "--effort",
        "high",
        "--op-id",
        op_id,
        "--yes",
    ];
    args.extend_from_slice(extra);
    args.push("--");
    args.push(text);
    args
}

/// The same with `--project-id <id>` in place of the project name.
pub fn new_args_by_id<'a>(
    id: &'a str,
    op_id: &'a str,
    extra: &[&'a str],
    text: &'a str,
) -> Vec<&'a str> {
    let mut args = new_args("", op_id, extra, text);
    args.splice(1..2, ["--project-id", id]);
    args
}

/// Whether the argument list `a` starts with `words`.
pub fn starts(a: &[String], words: &[&str]) -> bool {
    a.len() >= words.len() && a.iter().zip(words).all(|(x, w)| x == w)
}
