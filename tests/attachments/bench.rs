//! The phase 2 bench: a private source directory and state directory for
//! each test, outside the repository, the store opened on it, and
//! `openwiz attachment` run in this process with a fixed clock, a faked
//! free-space measure and a faked claim source.
//!
//! Every run's stderr (the only log OpenWiz writes) is scanned for AP12:
//! no synthetic file name (every one starts `syn-`), no synthetic content
//! and no SHA-256 ([`assert_clean_log`]).

#![allow(dead_code)]

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use openwiz_cli::attachment::{AttachmentContext, run_attachment};
use openwiz_core::attachment::{AttachmentId, Claim};
use openwiz_core::messaging::record::StoreError;
use openwiz_core::model::Timestamp;
use openwiz_core::ops::Clock;
use openwiz_store::AttachmentStore;
use openwiz_store::attachments::{Claims, FreeSpace, NoClaims};
use serde_json::Value;

/// The fixed time of every test run, unless a test moves it.
pub const T0: Timestamp = Timestamp(1_800_000_000_000);

/// Free space no test reaches.
pub const PLENTY: u64 = 1 << 50;

/// What every synthetic text file holds.
pub const CONTENT: &str = "SYNTHETIC-CONTENT, reply ok.\n";

/// The content markers AP12 looks for: the text above, and the padding of
/// every synthetic image (`support::padded`, 0x5a).
pub const CONTENT_MARKERS: &[&str] = &["SYNTHETIC-CONTENT", "ZZZZZZZZ"];

/// This run's own directory, under the system's temporary directory, so
/// never inside the repository (the store would refuse it).
pub fn run_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let parent = std::env::temp_dir().join("openwiz-attachments-tests");
        crate::dirs::private_dir_all(&parent).expect("the attachment tests' temp dir");
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = parent.join(format!("{}-{nanos}", std::process::id()));
        crate::dirs::private_dir(&root).expect("this run's own temp dir");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private");
        root
    })
}

pub struct FixedClock(pub Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn monotonic(&self) -> Instant {
        Instant::now()
    }
}

/// A free-space measure that answers `free` and records where it was asked.
pub struct Free {
    pub free: u64,
    pub asked: Mutex<Vec<PathBuf>>,
}

impl Free {
    pub fn new(free: u64) -> Self {
        Self {
            free,
            asked: Mutex::new(vec![]),
        }
    }
}

impl FreeSpace for Free {
    fn free_bytes(&self, dir: &Path) -> Result<u64, String> {
        self.asked.lock().unwrap().push(dir.to_path_buf());
        Ok(self.free)
    }
}

/// A free-space measure that always fails.
pub struct Unmeasurable;

impl FreeSpace for Unmeasurable {
    fn free_bytes(&self, _dir: &Path) -> Result<u64, String> {
        Err("synthetic measure failure".to_owned())
    }
}

/// A claim source with the claims a test sets (B2 records the real ones);
/// every other attachment is unclaimed.
#[derive(Default)]
pub struct ClaimsOf(pub Mutex<HashMap<String, Claim>>);

impl ClaimsOf {
    pub fn set(&self, id: &str, claim: Claim) {
        self.0.lock().unwrap().insert(id.to_owned(), claim);
    }
}

impl Claims for ClaimsOf {
    fn claim(&self, id: &AttachmentId) -> Result<Claim, StoreError> {
        Ok(*self
            .0
            .lock()
            .unwrap()
            .get(id.as_str())
            .unwrap_or(&Claim::Unclaimed))
    }
}

/// One `openwiz attachment` run.
#[derive(Debug)]
pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Run {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| {
            panic!(
                "stdout is not JSON ({e}):\n{}\nstderr: {}",
                self.out, self.err
            )
        })
    }
}

/// A private source directory and state directory for one test.
pub struct Bench {
    pub root: PathBuf,
    pub sources: PathBuf,
    pub state: PathBuf,
}

impl Bench {
    pub fn new(test: &str) -> Self {
        let root = run_root().join(test);
        let _ = fs::remove_dir_all(&root);
        let sources = root.join("sources");
        crate::dirs::private_dir_all(&sources).expect("sources");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private");
        Self {
            state: root.join("state"),
            root,
            sources,
        }
    }

    /// Writes a source file the owner might choose.
    pub fn source(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.sources.join(name);
        fs::write(&path, bytes).expect("write a source");
        path
    }

    pub fn store(&self) -> AttachmentStore {
        AttachmentStore::open(&self.state).expect("open the attachment store")
    }

    pub fn attachments(&self) -> PathBuf {
        self.state.join("attachments")
    }

    /// The names in `attachments/`, sorted.
    pub fn entries(&self) -> Vec<String> {
        let Ok(read) = fs::read_dir(self.attachments()) else {
            return vec![];
        };
        let mut names: Vec<String> = read
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The attachment directories in `attachments/`.
    pub fn ids(&self) -> Vec<String> {
        self.entries()
            .into_iter()
            .filter(|n| !n.starts_with('.'))
            .collect()
    }

    /// `openwiz attachment <args>` at [`T0`], with plenty of space and no
    /// claims.
    pub fn cli(&self, args: &[&str]) -> Run {
        self.cli_at(args, T0)
    }

    pub fn cli_at(&self, args: &[&str], now: Timestamp) -> Run {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        self.cli_with(&args, now, &Free::new(PLENTY), &NoClaims)
    }

    /// `openwiz attachment <args>`, in this process.
    pub fn cli_with(
        &self,
        args: &[OsString],
        now: Timestamp,
        free: &dyn FreeSpace,
        claims: &dyn Claims,
    ) -> Run {
        let state = self.state.clone();
        let read_only = self.state.clone();
        let open_store = move || AttachmentStore::open(&state);
        let open_store_read_only = move || AttachmentStore::open_read_only(&read_only);
        let clock = FixedClock(now);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_attachment(
            args,
            AttachmentContext {
                open_store: &open_store,
                open_store_read_only: &open_store_read_only,
                clock: &clock,
                free_space: free,
                claims,
            },
            &mut out,
            &mut err,
        );
        let run = Run {
            code,
            out: String::from_utf8(out).expect("stdout is UTF-8"),
            err: String::from_utf8(err).expect("stderr is UTF-8"),
        };
        assert_clean_log(&run.err);
        run
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            // The staged copies are read-only; their directories are not.
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

/// AP12: a log names no synthetic file, holds no synthetic content and no
/// SHA-256 (any run of 64 hex digits).
pub fn assert_clean_log(log: &str) {
    assert!(!log.contains("syn-"), "a file name in the log:\n{log}");
    for marker in CONTENT_MARKERS {
        assert!(!log.contains(marker), "content in the log:\n{log}");
    }
    let mut run = 0;
    for c in log.chars() {
        run = if c.is_ascii_hexdigit() { run + 1 } else { 0 };
        assert!(run < 64, "a hash in the log:\n{log}");
    }
}

/// The SHA-256 of a file, read whole: the tests' own check.
pub fn sha256_of(path: &Path) -> String {
    openwiz_core::messaging::sha256_hex(&fs::read(path).expect("read"))
}

/// Every file under `dir`, as paths relative to it, sorted.
pub fn walk(dir: &Path) -> Vec<String> {
    fn go(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(read) = fs::read_dir(dir) else {
            return;
        };
        for entry in read {
            let path = entry.unwrap().path();
            out.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                go(base, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    go(dir, dir, &mut out);
    out.sort();
    out
}

/// The permission bits of a path, not following a symlink.
pub fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

/// Builds the release `openwiz` once per test run, as P10, M14 and N18 do.
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
                .current_dir(crate::support::ROOT)
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

/// Programs no attachment command may run, first on `PATH`: a `df` and a
/// `room-cli` that each leave a mark and fail. Returns the directory.
pub fn tripwires(bench: &Bench) -> PathBuf {
    let dir = bench.root.join("tripwires");
    crate::dirs::private_dir_all(&dir).unwrap();
    for name in ["df", "room-cli"] {
        let program = dir.join(name);
        fs::write(
            &program,
            format!(
                "#!/bin/sh\necho ran >> '{}/{name}.ran'\nexit 1\n",
                dir.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    }
    dir
}

/// The release `openwiz attachment <args>`, as a process: this bench's
/// state directory, `tripwires` first on `PATH`, stdin closed.
pub fn release_run(bench: &Bench, tripwires: &Path, args: &[&str]) -> Output {
    let path = format!(
        "{}:{}",
        tripwires.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(openwiz_release())
        .arg("attachment")
        .args(args)
        .env("PATH", path)
        .env("OPENWIZ_STATE_DIR", &bench.state)
        .current_dir(&bench.root)
        .stdin(Stdio::null())
        .output()
        .expect("run openwiz")
}
