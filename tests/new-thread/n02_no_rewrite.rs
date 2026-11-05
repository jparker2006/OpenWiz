//! N2 (sheet the check): no rewrite. Every `room-cli` call runs without
//! `ROOM_THREAD_ID`, `ROOM_PROJECT_ID` or any `BB_*` variable, and always
//! keeps `ROOM_SERVER_URL`; `send`'s calls included. The model passed is the
//! model chosen.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Command, Stdio};

use openwiz_adapter_cloudroom::new_thread::spawn_argv;
use openwiz_adapter_cloudroom::{KEPT_ENV, SCRUBBED_ENV, is_scrubbed};
use openwiz_core::new_thread::operation::SpawnRequest;
use openwiz_core::prompt::ReadyPrompt;
use serde_json::Value;

use crate::support::*;

#[test]
fn the_scrub_list_is_the_sheets() {
    for name in [
        "ROOM_THREAD_ID",
        "ROOM_PROJECT_ID",
        "BB_AUTOMATION_RUN_ID",
        "BB_SYNTHETIC",
        "BB_",
    ] {
        assert!(is_scrubbed(name), "{name} is removed");
    }
    for name in [
        "ROOM_SERVER_URL",
        "PATH",
        "HOME",
        "OPENWIZ_STATE_DIR",
        "XBB_SYNTHETIC",
        "room_thread_id",
    ] {
        assert!(!is_scrubbed(name), "{name} is kept");
    }
    assert_eq!(KEPT_ENV, "ROOM_SERVER_URL");
    assert!(SCRUBBED_ENV.contains(&"ROOM_THREAD_ID"));
}

/// The send harness, as its own process with the thread-context variables
/// set, reads the target and sends through the fake: every call it makes,
/// `thread tell` included, must arrive without them.
#[test]
fn every_call_runs_without_the_thread_context() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("new-thread-n02");
    let _ = fs::remove_dir_all(&dir);
    crate::dirs::private_dir_all(&dir).unwrap();
    symlink(env!("CARGO_BIN_EXE_fake-room-cli"), dir.join("room-cli")).unwrap();
    let scenario =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/room-cli-0.43.1/send/scenario.json");
    fs::write(
        dir.join("fake-room-cli.scenario"),
        scenario.to_str().unwrap(),
    )
    .unwrap();
    let state = private_dir("n02-state");
    let output = Command::new(env!("CARGO_BIN_EXE_send-harness"))
        .args([
            state.to_str().unwrap(),
            dir.join("room-cli").to_str().unwrap(),
            "thr_syn_0001",
            "queue",
            "op-n02-scrub",
            "none",
            "Synthetic scrub check",
        ])
        .env("OPENWIZ_STATE_DIR", &state)
        .env("ROOM_THREAD_ID", "thr_syn_0001")
        .env("ROOM_PROJECT_ID", "prj_syn_0001")
        .env("BB_AUTOMATION_RUN_ID", "syn-run")
        .env("BB_SYNTHETIC", "syn")
        .env("ROOM_SERVER_URL", "http://127.0.0.1:9")
        .stdin(Stdio::null())
        .output()
        .expect("run the send harness");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls: Vec<Vec<String>> = fs::read_to_string(dir.join("invocations.log"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        calls
            .iter()
            .any(|c| c.get(1).map(String::as_str) == Some("tell")),
        "the write ran: {calls:?}"
    );
    let envs: Vec<Value> = fs::read_to_string(dir.join("environment.log"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(envs.len(), calls.len(), "one environment line per call");
    // What this test's own environment passes through (a CloudRoom sandbox
    // sets `CLOUDROOM_*` names, which the sheet's list keeps), plus
    // `ROOM_SERVER_URL`; never a scrubbed name.
    let mut kept: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|n| {
            ["ROOM_", "BB_", "CLOUDROOM_"]
                .iter()
                .any(|p| n.starts_with(p))
        })
        .filter(|n| !is_scrubbed(n))
        .chain([KEPT_ENV.to_owned()])
        .collect();
    kept.sort();
    kept.dedup();
    for env in envs {
        assert_eq!(env, serde_json::json!(kept), "{calls:?}");
        let names: Vec<&str> = env
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for scrubbed in [
            "ROOM_THREAD_ID",
            "ROOM_PROJECT_ID",
            "BB_AUTOMATION_RUN_ID",
            "BB_SYNTHETIC",
        ] {
            assert!(!names.contains(&scrubbed), "{scrubbed} reached room-cli");
        }
    }
    let _ = fs::remove_dir_all(&state);
}

#[test]
fn the_model_passed_is_the_model_chosen() {
    for model in ["syn-claude-model-01", "syn-claude-model-01[1m]"] {
        let spawn = SpawnRequest {
            project_id: "prj_syn_0001".to_owned(),
            provider: "claude-code".to_owned(),
            model: model.to_owned(),
            effort: "high".to_owned(),
            // A1 phase 2: the allowlist takes only a derived request id.
            request_id: openwiz_core::new_thread::request_id(&op("op-n02-model")),
            prompt: ReadyPrompt::text("Synthetic"),
        };
        let argv = spawn_argv(&spawn).unwrap();
        let i = argv.iter().position(|w| w == "--model").unwrap();
        assert_eq!(argv[i + 1], model);
    }
}
