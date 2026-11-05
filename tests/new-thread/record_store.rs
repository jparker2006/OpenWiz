//! The `new_thread` record kind in `openwiz-store` (A1 phase 2): owner-only,
//! outside every repository, beside the send record and apart from it; and
//! read back only through the core's strict reader, so a record on disk
//! that isn't exactly the contract's shape is refused, and nothing is
//! created from it.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use openwiz_core::envelope::SourceInfo;
use openwiz_core::messaging::send::UncertainReason;
use openwiz_core::model::{Fact, Timestamp};
use openwiz_core::new_thread::effects::effects_for;
use openwiz_core::new_thread::outcome::{FailReason, Gap, Outcome, RefusalReason};
use openwiz_core::new_thread::read::StrictJson;
use openwiz_core::new_thread::record::{NewThreadRecord, NewThreadStore, recover};
use openwiz_core::new_thread::{Binding, ProjectRef, ProjectView};
use openwiz_core::prompt::empty_attachments_digest;
use openwiz_store::NewThreadFileStore;
use serde_json::{Value, json};

use crate::fake::*;
use crate::support::{T0, offered, private_dir, request};

fn mode_of(path: &std::path::Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Every record shape the core writes: claimed, dispatching, and finished
/// with each outcome; by name and by id; recovered; with gaps.
pub fn every_record() -> Vec<NewThreadRecord> {
    let r = request(offered(), "Synthetic\nline two", "op-store-every");
    let claimed = NewThreadRecord::claimed(
        r.op_id.clone(),
        Binding::new(&r, empty_attachments_digest()),
        r.settings.clone(),
        &r.prompt.text,
        T0,
    );
    let project = ProjectView {
        id: "prj_syn_0001".to_owned(),
        name: Fact::Known("Synthetic project one".to_owned()),
    };
    let mut looked = claimed.clone();
    looked.project = Some(project.clone());
    looked.provider = Some("codex".to_owned());
    looked.source = Some(SourceInfo {
        name: "room-cli".to_owned(),
        version: Fact::Unknown("room-cli --version timed out after 1 ms".to_owned()),
        tested_version: "v127".to_owned(),
        version_tested: false,
    });
    looked.effects = Some(effects_for(&project, T0));
    looked.gaps.push(Gap {
        subject: "room-cli --version".to_owned(),
        detail: "Synthetic gap".to_owned(),
    });
    let dispatching = looked.dispatching(Timestamp(T0.0 + 1));
    let mut by_name = claimed.clone();
    by_name.binding.project = ProjectRef::Name("Synthetic shared~n_0004".to_owned());
    by_name.project = Some(ProjectView {
        id: "prj_syn_0004".to_owned(),
        name: Fact::Unknown("schema: name".to_owned()),
    });
    let later = Timestamp(T0.0 + 2);
    vec![
        claimed.clone(),
        by_name.clone(),
        dispatching.clone(),
        dispatching.finished(Outcome::created("thr_syn_0021".to_owned()), later),
        claimed.finished(
            Outcome::refused_because(
                RefusalReason::ModelNotOffered,
                "not offered",
                vec!["syn-codex-model-01".to_owned()],
            ),
            later,
        ),
        by_name.finished(Outcome::refused(RefusalReason::ProjectAmbiguous), later),
        looked.finished(Outcome::failed(FailReason::BudgetExhausted), later),
        dispatching.finished(Outcome::uncertain(UncertainReason::TimedOut), later),
        recover(&claimed, later),
        recover(&dispatching, later),
    ]
}

pub fn read(value: &Value) -> Result<NewThreadRecord, String> {
    let tree: StrictJson =
        serde_json::from_str(&value.to_string()).map_err(|e| format!("tree: {e}"))?;
    NewThreadRecord::read(&tree).map_err(|e| e.0)
}

#[test]
fn every_record_the_core_writes_reads_back_the_same() {
    for record in every_record() {
        let value = serde_json::to_value(&record).unwrap();
        assert!(
            crate::support::validate_def("record", &value).is_empty(),
            "{value}"
        );
        assert_eq!(read(&value).as_ref(), Ok(&record), "{value}");
    }
}

#[test]
fn the_record_is_owner_only_outside_the_repository_and_beside_the_send_record() {
    let fake = NewFake::new("store_layout");
    let (code, out, err) = fake.cli(
        &new_args("Synthetic project one", "op-store-layout", &[], "Synthetic"),
        "",
    );
    assert_eq!(code, 0, "{out}{err}");
    let dir = fake.state.join("new-threads");
    assert_eq!(mode_of(&fake.state), 0o700);
    assert_eq!(mode_of(&dir), 0o700);
    assert_eq!(mode_of(&dir.join("op-store-layout.json")), 0o600);
    assert_eq!(mode_of(&dir.join("op-store-layout.lock")), 0o600);
    assert!(
        !fake.state.join("sends").exists(),
        "a create writes no send record"
    );
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}: no temporary file is left");
    // The record holds no text once the thread is created.
    assert!(fake.record("op-store-layout")["text"].is_null());

    // The same refusals as the send record.
    let open = |dir: &std::path::Path| NewThreadFileStore::open(dir).map(|_| ());
    let shared = private_dir("store-shared");
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(open(&shared).is_err(), "group may enter");
    assert!(open(std::path::Path::new("relative/state")).is_err());
    let root = std::path::Path::new(crate::support::ROOT)
        .canonicalize()
        .unwrap();
    assert!(
        open(&root.join("target/nt-state")).is_err(),
        "inside the repository"
    );
    let planted = private_dir("store-planted");
    std::os::unix::fs::symlink(&shared, planted.join("new-threads")).unwrap();
    assert!(
        open(&planted).is_err(),
        "a symlinked new-threads/ is refused"
    );
    // Read-only: nothing is made where nothing was.
    let empty = private_dir("store-empty").join("state");
    assert!(
        NewThreadFileStore::open_read_only(&empty)
            .unwrap()
            .is_none()
    );
    assert!(!empty.exists());
}

#[test]
fn a_record_that_is_not_exactly_its_shape_is_refused_and_nothing_is_created() {
    let fake = NewFake::new("store_refused");
    let args = new_args("Synthetic project one", "op-store-bad", &[], "Synthetic");
    let (code, _, _) = fake.cli(&args, "");
    assert_eq!(code, 0);
    let path = fake.record_path("op-store-bad");
    let good = fs::read_to_string(&path).unwrap();
    let value: Value = serde_json::from_str(&good).unwrap();
    let duplicate = good.replacen(
        "\"recovered\": false",
        "\"recovered\": false,\n  \"recovered\": false",
        1,
    );
    assert_ne!(duplicate, good);
    let mut extra = value.clone();
    extra["extra"] = json!(1);
    let mut tagged = value.clone();
    tagged["state"] = json!({"finished": null});
    let positional = Value::Array(value.as_object().unwrap().values().cloned().collect());
    for bad in [
        duplicate,
        extra.to_string(),
        tagged.to_string(),
        positional.to_string(),
        "not JSON".to_owned(),
    ] {
        fs::write(&path, &bad).unwrap();
        let before = fake.spawn_count();
        let (code, out, err) = fake.cli(&args, "");
        assert_eq!(code, 10, "{bad}\n{out}{err}");
        assert!(err.contains("is not a new-thread record"), "{err}");
        let (code, _, err) = fake.cli(&["new", "--status", "op-store-bad"], "");
        assert_eq!(code, 10, "{err}");
        assert_eq!(fake.spawn_count(), before, "nothing was created");
    }

    // A record in another op id's file is refused too.
    let mut moved = value.clone();
    moved["op_id"] = json!("op-store-other");
    moved["request_id"] = json!(openwiz_core::new_thread::request_id(
        &openwiz_core::messaging::OpId::parse("op-store-other").unwrap()
    ));
    fs::write(&path, moved.to_string()).unwrap();
    let (code, _, err) = fake.cli(&args, "");
    assert_eq!(code, 10, "{err}");
    assert!(err.contains("another op id"), "{err}");
}

#[test]
fn the_store_claims_once_and_recovers_a_stopped_owner() {
    let state = private_dir("store-claim").join("state");
    let store = NewThreadFileStore::open(&state).unwrap();
    let records = every_record();
    let claimed = &records[0];
    assert_eq!(
        store.claim(claimed, T0).unwrap(),
        openwiz_core::new_thread::record::Claim::Owned
    );
    // A second store in this process is another claimer: in progress.
    let other = NewThreadFileStore::open(&state).unwrap();
    assert!(matches!(
        other.claim(claimed, T0).unwrap(),
        openwiz_core::new_thread::record::Claim::InProgress(_)
    ));
    assert_eq!(
        other.inspect(&claimed.op_id, T0).unwrap().as_ref(),
        Some(claimed)
    );
    // The owner lets go without finishing: recovered as failed, once.
    store.release(&claimed.op_id);
    let later = Timestamp(T0.0 + 5);
    let shown = other.inspect(&claimed.op_id, later).unwrap().unwrap();
    assert_eq!(
        shown,
        recover(claimed, later),
        "shown as recovery would make it"
    );
    assert_eq!(
        other.record(&claimed.op_id).unwrap().as_ref(),
        Some(claimed),
        "not written"
    );
    match other.claim(claimed, later).unwrap() {
        openwiz_core::new_thread::record::Claim::Existing(r) => {
            assert_eq!(r, recover(claimed, later));
        }
        claim => panic!("{claim:?}"),
    }
    assert!(other.update(&records[2]).is_err(), "only the owner writes");
}
