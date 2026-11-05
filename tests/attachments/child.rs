//! The self-exec child: this test binary, run again as its own process,
//! stages one file through the store, the way `openwiz attachment add`
//! does. AP4 stops it (`abort`) right after each staging step; AP6 measures
//! its peak memory around one staging.
//!
//! It runs only when [`CHILD_ENV`] holds its spec (JSON: `mode` `kill` or
//! `memory`, `state`, `source`, `step`, `out`); in a normal test run
//! `child_process` returns at once. Its result goes to the `out` file,
//! never stdout (the test runner prints there).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use openwiz_store::AttachmentStore;
use openwiz_store::attachments::{Durability, Staged, Step};
use serde_json::{Value, json};

use crate::bench::{Free, PLENTY, T0};

/// The variable that holds the child's spec.
pub const CHILD_ENV: &str = "OPENWIZ_ATTACHMENT_CHILD";

/// Every step, in order, by the name the spec uses.
pub const STEPS: [(&str, Step); 9] = [
    ("looked", Step::Looked),
    ("made", Step::Made),
    ("writing", Step::Writing),
    ("copied", Step::Copied),
    ("synced", Step::Synced),
    ("renamed", Step::Renamed),
    ("sealed", Step::Sealed),
    ("meta-written", Step::MetaWritten),
    ("ready", Step::Ready),
];

/// Real syncs; the process stops right after `at` is done.
struct StopAt(Step);

impl Durability for StopAt {
    fn done(&self, step: Step) {
        if step == self.0 {
            std::process::abort();
        }
    }
}

/// This process's peak resident memory so far, in KiB (Linux:
/// `/proc/self/status`, `VmHWM`). AP6 needs it; it fails without it.
pub fn peak_kib() -> u64 {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_else(|e| {
        panic!(
            "AP6 measures peak memory through /proc/self/status, which this system doesn't \
             have ({e}); run the tests on Linux"
        )
    });
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().strip_suffix("kB"))
        .and_then(|v| v.trim().parse().ok())
        .expect("VmHWM in /proc/self/status")
}

#[test]
fn child_process() {
    let Some(spec) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).expect("spec");
    let path = |key: &str| PathBuf::from(spec[key].as_str().expect(key));
    let store = AttachmentStore::open(&path("state")).expect("open the store");
    let free = Free::new(PLENTY);
    let result = match spec["mode"].as_str() {
        Some("kill") => {
            let step = STEPS
                .iter()
                .find(|(name, _)| Some(*name) == spec["step"].as_str())
                .expect("a step")
                .1;
            let staged = store.stage_with(&path("source"), T0, &free, &StopAt(step));
            json!({"stopped": false, "staged": format!("{staged:?}")})
        }
        Some("memory") => {
            let before = peak_kib();
            let staged = store.stage(&path("source"), T0, &free).expect("stage");
            let after = peak_kib();
            let sha256 = match &staged {
                Staged::Ready(ready) => Some(ready.attachment.sha256.as_str().to_owned()),
                Staged::Failed(_) => None,
            };
            json!({"before_kib": before, "after_kib": after, "sha256": sha256})
        }
        other => panic!("unknown mode {other:?}"),
    };
    fs::write(path("out"), result.to_string()).expect("write the result");
}

/// Runs the child with `spec`, to its end.
pub fn run_child(spec: &Value) -> Output {
    Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "child::child_process",
            "--nocapture",
            "--test-threads=1",
            "-q",
        ])
        .env(CHILD_ENV, spec.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run the child")
}

/// The child's result file, as JSON.
pub fn result(out: &Path) -> Value {
    let text = fs::read_to_string(out).unwrap_or_else(|e| panic!("no child result: {e}"));
    serde_json::from_str(&text).expect("child result JSON")
}
