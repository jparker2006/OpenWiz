//! M10 The record is synced, readable by the owner only, outside the
//! repository, and correct under concurrent writers.
//!
//! Durability against power loss cannot be shown on fixtures. What is shown:
//! the dispatching mark is on disk, visible to another process, at the
//! moment of the call; and the store syncs both the file and its directory
//! (a static tripwire, like P8(a)).

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use openwiz_core::messaging::Mode;
use openwiz_core::messaging::send::Status;
use openwiz_store::{FileStore, default_state_dir};
use serde_json::json;

use crate::support::*;

#[test]
fn the_record_is_readable_by_the_owner_only() {
    let fake = SendFake::new("m10_owner_only", "send");
    let _ = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        "Synthetic",
        "op-m10-owner",
    ));
    assert_eq!(mode_of(&fake.state), 0o700);
    assert_eq!(mode_of(&fake.state.join("sends")), 0o700);
    assert_eq!(mode_of(&fake.record_path("op-m10-owner")), 0o600);
    assert_eq!(
        mode_of(&fake.state.join("sends").join("op-m10-owner.lock")),
        0o600
    );
    for entry in fs::read_dir(fake.state.join("sends")).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(mode_of(&path), 0o600, "{path:?}");
    }
}

#[test]
fn a_state_directory_others_can_read_is_refused() {
    let fake = SendFake::new("m10_open_dir", "send");
    fs::DirBuilder::new()
        .mode(0o755)
        .create(&fake.state)
        .unwrap();
    fs::set_permissions(&fake.state, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(FileStore::open(&fake.state).is_err());
    let (code, out, err) = fake.cli(&["thr_syn_0001", "--", "Synthetic"], "");
    assert_eq!(code, 10, "{out}{err}");
    assert!(out.is_empty());
    assert!(err.starts_with("openwiz: "), "{err}");
    assert_eq!(fake.tell_count(), 0, "nothing sent");
}

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let pairs: Vec<(String, OsString)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
        .collect();
    move |name| {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

#[test]
fn the_default_state_directory_is_the_owners_and_outside_the_repository() {
    let home = "/synthetic-home/owner";
    let default = default_state_dir(&env(&[("HOME", home)])).unwrap();
    if cfg!(target_os = "macos") {
        assert_eq!(
            default,
            PathBuf::from(home).join("Library/Application Support/openwiz")
        );
    } else {
        assert_eq!(default, PathBuf::from(home).join(".local/state/openwiz"));
        assert_eq!(
            default_state_dir(&env(&[
                ("HOME", home),
                ("XDG_STATE_HOME", "/synthetic-xdg")
            ]))
            .unwrap(),
            PathBuf::from("/synthetic-xdg/openwiz")
        );
        // A relative XDG_STATE_HOME is ignored, as the XDG spec says.
        assert_eq!(
            default_state_dir(&env(&[("HOME", home), ("XDG_STATE_HOME", "relative")])).unwrap(),
            default
        );
    }
    assert_eq!(
        default_state_dir(&env(&[
            ("HOME", home),
            ("OPENWIZ_STATE_DIR", "/synthetic-state")
        ]))
        .unwrap(),
        PathBuf::from("/synthetic-state")
    );
    assert!(default_state_dir(&env(&[])).is_err(), "no HOME: no guess");
    assert!(default_state_dir(&env(&[("HOME", "relative")])).is_err());

    // With the real HOME, the default is not inside this repository.
    if let Some(real) = std::env::var_os("HOME") {
        let real = default_state_dir(&env(&[("HOME", real.to_str().unwrap())])).unwrap();
        let repo = Path::new(ROOT).canonicalize().unwrap();
        assert!(!real.starts_with(&repo), "{real:?} is inside {repo:?}");
    }
}

#[test]
fn the_dispatching_mark_is_on_disk_before_the_call() {
    let fake = SendFake::new("m10_mark", "send");
    let text = "Synthetic durable mark";
    let _ = fake.report(&request("thr_syn_0001", Mode::Queue, text, "op-m10-mark"));
    let at_tell = fake.records_at_tell(1);
    assert_eq!(at_tell.len(), 1);
    let record = &at_tell[0];
    assert_eq!(record["op_id"], "op-m10-mark");
    assert_eq!(record["state"], "dispatching");
    assert!(record["dispatching_at"].is_i64(), "{record}");
    assert_eq!(
        record["text"], text,
        "the text is on disk until the outcome"
    );
    assert!(record["delivery"].is_null());
}

#[test]
fn the_text_is_kept_only_while_uncertain_or_failed() {
    let fake = SendFake::with(
        "m10_retention",
        vec![tell_response(
            "thr_syn_0003",
            "queue",
            json!({"abort": true}),
        )],
    );
    for (target, op_id, status, kept) in [
        ("thr_syn_0001", "op-m10-accepted", Status::Accepted, false),
        ("thr_syn_0006", "op-m10-queued", Status::Queued, false),
        ("thr_syn_0005", "op-m10-failed", Status::Failed, true),
        ("thr_syn_9999", "op-m10-not-found", Status::Failed, true),
        ("thr_syn_0003", "op-m10-uncertain", Status::Uncertain, true),
    ] {
        let text = format!("Synthetic retention {op_id}");
        let report = fake.report(&request(target, Mode::Queue, &text, op_id));
        assert_eq!(report.status, status, "{op_id}");
        let record = fake.record(op_id);
        assert_eq!(
            record["text"],
            if kept { json!(text) } else { json!(null) },
            "{op_id}"
        );
        assert_eq!(
            record["binding"]["text_sha256"],
            openwiz_core::messaging::sha256_hex(text.as_bytes()),
            "the hash stays"
        );
        assert_eq!(record["text_bytes"], text.len());
        let errors = validate_def("record", &record);
        assert!(errors.is_empty(), "{op_id}: {errors:#?}");
    }
}

#[test]
fn concurrent_writers_keep_every_record_whole() {
    let fake = SendFake::new("m10_concurrent", "send");
    let children: Vec<_> = (0..12)
        .map(|i| {
            fake.harness(
                "thr_syn_0001",
                "queue",
                &format!("op-m10-writer-{i}"),
                "none",
                &format!("Synthetic writer {i}"),
            )
            .spawn()
            .unwrap()
        })
        .collect();
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_eq!(fake.tell_count(), 12);
    for i in 0..12 {
        let record = fake.record(&format!("op-m10-writer-{i}"));
        assert_eq!(record["op_id"], format!("op-m10-writer-{i}"));
        assert_eq!(record["state"], "finished");
        assert_eq!(record["delivery"]["kind"], "accepted");
        assert!(validate_def("record", &record).is_empty());
    }
    let leftovers: Vec<_> = fs::read_dir(fake.state.join("sends"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with(".json") && !n.ends_with(".lock"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temporary files left: {leftovers:?}"
    );
}

#[test]
fn every_write_syncs_the_file_and_its_directory() {
    let source = fs::read_to_string(Path::new(ROOT).join("crates/store/src/lib.rs")).unwrap();
    let body = source
        .split("fn write_record")
        .nth(1)
        .expect("the store writes records in one function, write_record");
    let body = &body[..body.find("\n}\n").expect("end of write_record")];
    let order = ["sync_all", "rename", "sync_all"]
        .iter()
        .try_fold(0, |at, word| {
            body[at..].find(word).map(|i| at + i + word.len())
        });
    assert!(
        order.is_some(),
        "write_record syncs the file, renames it into place, then syncs the directory:\n{body}"
    );
}

#[test]
fn a_relative_state_directory_override_is_refused() {
    let home = "/synthetic-home/owner";
    assert!(
        default_state_dir(&env(&[
            ("HOME", home),
            ("OPENWIZ_STATE_DIR", "relative/dir")
        ]))
        .is_err()
    );
}

#[test]
fn a_claimer_takes_a_lock_released_while_it_waits() {
    use openwiz_core::messaging::record::{Claim, SendRecord, SendStore};
    use openwiz_core::messaging::{Binding, OpId};
    let fake = SendFake::new("m10_lock_released", "send");
    let store = fake.store();
    let op_id = OpId::parse("op-m10-released").unwrap();
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(fake.state.join("sends").join("op-m10-released.lock"))
        .unwrap();
    lock.lock().unwrap();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        drop(lock);
    });
    let record = SendRecord::claimed(
        op_id,
        Binding::new(sid("thr_syn_0001"), Mode::Queue, "Synthetic"),
        "Synthetic",
        T0,
    );
    let claim = store.claim(&record, T0).unwrap();
    holder.join().unwrap();
    assert_eq!(claim, Claim::Owned);
}

#[test]
fn an_invalid_op_id_never_becomes_a_path() {
    use openwiz_core::messaging::OpId;
    for bad in ["../../escaped", "", "-x", "a/b"] {
        assert!(
            serde_json::from_value::<OpId>(json!(bad)).is_err(),
            "deserialized {bad:?}"
        );
    }
    assert!(serde_json::from_value::<OpId>(json!("op-ok")).is_ok());

    let fake = SendFake::new("m10_planted_op_id", "send");
    let text = "Synthetic planted record";
    let _ = fake.report(&request(
        "thr_syn_0001",
        Mode::Queue,
        text,
        "op-m10-planted",
    ));
    // A record file whose op id escapes the directory, left claimed.
    let mut planted = fake.record("op-m10-planted");
    planted["op_id"] = json!("../../syn-escaped");
    planted["state"] = json!("claimed");
    planted["delivery"] = json!(null);
    fs::write(
        fake.state.join("sends").join("op-m10-planted.json"),
        serde_json::to_string(&planted).unwrap(),
    )
    .unwrap();
    let mut again = request("thr_syn_0001", Mode::Queue, text, "op-m10-planted-2");
    again.accept_possible_duplicate = false;
    let _ = fake.send(&again);
    let escaped = fake.state.join("syn-escaped.lock");
    let escaped_up = fake.state.parent().unwrap().join("syn-escaped.lock");
    assert!(!escaped.exists() && !escaped_up.exists(), "a path escaped");
}

#[test]
fn every_created_directory_is_synced_in_its_parent() {
    let source = fs::read_to_string(Path::new(ROOT).join("crates/store/src/lib.rs")).unwrap();
    let body = source
        .split("fn create_dir_synced")
        .nth(1)
        .expect("the store creates directories in one function, create_dir_synced");
    let body = &body[..body.find("\n}\n").expect("end of create_dir_synced")];
    assert!(body.contains("create_dir"), "{body}");
    // Round 2: the parent is synced through the store's sync function,
    // which `open` sets to a real fsync.
    assert!(body.contains("sync(parent)"), "{body}");
    let fsync = source.split("fn fsync_dir").nth(1).expect("fsync_dir");
    assert!(fsync[..fsync.find("\n}\n").unwrap()].contains("sync_all"));
    assert!(source.contains("Self::open_with_sync(state_dir, &fsync_dir)"));
    let open = source
        .split("pub fn open_with_sync")
        .nth(1)
        .expect("open_with_sync");
    let open = &open[..open.find("\n    }\n").expect("end of open")];

    assert!(
        open.contains("Self::checked_sends(state_dir, Some(sync))"),
        "{open}"
    );
    let checked = source
        .split("fn checked_sends")
        .nth(1)
        .expect("checked_sends");
    let checked = &checked[..checked.find("\n    }\n").expect("end of checked_sends")];
    assert!(
        checked.contains("create_dir_synced(dir, sync)"),
        "{checked}"
    );
    assert!(
        !checked.contains("DirBuilder") && !open.contains("DirBuilder"),
        "open creates directories only through create_dir_synced"
    );
    let read_only = source
        .split("pub fn open_read_only")
        .nth(1)
        .expect("open_read_only");
    let read_only = &read_only[..read_only.find("\n    }\n").unwrap()];
    assert!(
        read_only.contains("Self::checked_sends(state_dir, None)"),
        "{read_only}"
    );
}

#[test]
fn a_state_directory_inside_a_repository_is_refused() {
    let repo = run_root().join("m10_repo");
    let _ = fs::remove_dir_all(&repo);
    crate::dirs::private_dir_all(repo.join(".git")).unwrap();
    let state = repo.join("nested").join("state");
    assert!(FileStore::open(&state).is_err());
    assert!(!state.exists(), "nothing created inside the repository");
    // Through a symlink from outside, too.
    let link = run_root().join("m10_repo_link");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&repo, &link).unwrap();
    assert!(FileStore::open(&link.join("state")).is_err());
    let _ = fs::remove_file(&link);
    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn every_opener_syncs_the_whole_directory_chain() {
    use std::sync::Mutex;
    let base = run_root().join("m10_chain");
    let _ = fs::remove_dir_all(&base);
    // Another opener made the chain and was stopped before syncing it.
    let state = base.join("a").join("state");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(state.join("sends"))
        .unwrap();
    let synced = Mutex::new(Vec::<PathBuf>::new());
    let record = |dir: &Path| {
        synced.lock().unwrap().push(dir.to_path_buf());
        Ok(())
    };
    let store = FileStore::open_with_sync(&state, &record).unwrap();
    let synced = synced.into_inner().unwrap();
    let resolved = state.join("sends").canonicalize().unwrap();
    for dir in resolved.ancestors() {
        assert!(
            synced
                .iter()
                .any(|s| s.canonicalize().ok().as_deref() == Some(dir)),
            "{dir:?} was not synced: {synced:?}"
        );
    }
    drop(store);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn no_path_trick_puts_the_record_inside_a_repository() {
    let base = run_root().join("m10_repo_tricks");
    let _ = fs::remove_dir_all(&base);
    let repo = base.join("synthetic-repo");
    crate::dirs::private_dir_all(repo.join(".git")).unwrap();
    let inside = repo.join("records");
    fs::DirBuilder::new().mode(0o700).create(&inside).unwrap();

    // 1. An outside state directory whose `sends` is a symlink into it.
    let outside = base.join("outside-state");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&outside)
        .unwrap();
    std::os::unix::fs::symlink(&inside, outside.join("sends")).unwrap();
    assert!(FileStore::open(&outside).is_err(), "a sends symlink");

    // 2. A path that reaches the repository through `..` of a missing
    //    directory.
    let dotted = base
        .join("missing")
        .join("..")
        .join("synthetic-repo")
        .join("state");
    assert!(FileStore::open(&dotted).is_err(), "a .. path");
    assert!(
        !repo.join("state").exists(),
        "nothing made in the repository"
    );
    assert_eq!(
        fs::read_dir(&inside).unwrap().count(),
        0,
        "nothing written in the repository"
    );
    let _ = fs::remove_dir_all(&base);
}
