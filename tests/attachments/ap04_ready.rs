//! AP4 Ready means verified. `ready` is set only after the copy is synced,
//! renamed, made read-only and its metadata written and synced. Stopping
//! the process right after each step leaves either nothing usable (removed
//! at the next open) or a fully verified attachment, never a ready one with
//! wrong bytes.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openwiz_core::attachment::{AttachmentId, FailReason, Kind};
use openwiz_store::AttachmentStore;
use openwiz_store::attachments::{
    CLAIMS_LOCK, COPY_CHUNK_BYTES, Durability, Listed, META, NoClaims, PARTIAL_SUFFIX,
    STAGING_LOCK, Staged, Step, Verified,
};
use serde_json::json;

use crate::bench::{Bench, Free, PLENTY, T0, mode, sha256_of};
use crate::child::{STEPS, run_child};
use crate::support::{PNG, padded};

/// What the disk held when a sync was asked for, or a step done.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Seen {
    event: String,
    partial: bool,
    copy: Option<u32>,
    meta_temp: bool,
    meta: bool,
    listed: usize,
}

/// Records every sync and step with what the disk held, and at
/// [`Step::Writing`] runs a sweep from a second handle.
struct Recorder {
    bench_attachments: PathBuf,
    name: String,
    seen: Mutex<Vec<Seen>>,
    swept_while_writing: Mutex<Option<usize>>,
}

impl Recorder {
    fn dir(&self) -> Option<PathBuf> {
        fs::read_dir(&self.bench_attachments)
            .ok()?
            .map(|e| e.unwrap().path())
            .find(|p| p.is_dir())
    }

    fn look(&self, event: String) {
        let dir = self.dir();
        let at = |name: &str| dir.as_ref().map(|d| d.join(name));
        let exists = |p: Option<PathBuf>| p.is_some_and(|p| p.exists());
        let store = AttachmentStore::open_read_only(self.bench_attachments.parent().unwrap())
            .unwrap()
            .unwrap();
        self.seen.lock().unwrap().push(Seen {
            event,
            partial: exists(at(&format!("{}{PARTIAL_SUFFIX}", self.name))),
            copy: at(&self.name).filter(|p| p.exists()).map(|p| mode(&p)),
            meta_temp: exists(at(&format!("{META}.tmp"))),
            meta: exists(at(META)),
            listed: store.list(&NoClaims).unwrap().len(),
        });
    }
}

impl Durability for Recorder {
    fn sync_file(&self, file: &fs::File, step: Step) -> std::io::Result<()> {
        self.look(format!("sync file for {step:?}"));
        file.sync_all()
    }

    fn sync_dir(&self, dir: &Path, step: Step) -> std::io::Result<()> {
        self.look(format!("sync dir for {step:?}"));
        fs::File::open(dir)?.sync_all()
    }

    fn done(&self, step: Step) {
        self.look(format!("done {step:?}"));
        if step == Step::Writing {
            // Another process's open: it must leave a copy being written.
            let other = AttachmentStore::open(self.bench_attachments.parent().unwrap()).unwrap();
            let swept = other.sweep(T0, &NoClaims).unwrap();
            *self.swept_while_writing.lock().unwrap() = Some(swept.unfinished);
        }
    }
}

fn seen(event: &str, partial: bool, copy: Option<u32>, meta_temp: bool, meta: bool) -> Seen {
    Seen {
        event: event.to_owned(),
        partial,
        copy,
        meta_temp,
        meta,
        listed: usize::from(meta),
    }
}

#[test]
fn each_step_is_durable_before_the_next_and_ready_comes_last() {
    let bench = Bench::new("ap04_order");
    let name = "syn-ap04-order.png";
    // More than one chunk, so the copy is seen part way.
    let bytes = padded(PNG, (COPY_CHUNK_BYTES * 3 + 17) as u64);
    let source = bench.source(name, &bytes);
    let store = bench.store();
    let recorder = Recorder {
        bench_attachments: bench.attachments(),
        name: name.to_owned(),
        seen: Mutex::new(vec![]),
        swept_while_writing: Mutex::new(None),
    };
    let staged = store
        .stage_with(&source, T0, &Free::new(PLENTY), &recorder)
        .unwrap();
    let Staged::Ready(ready) = staged else {
        panic!("not ready: {staged:?}");
    };
    let rw = Some(0o600);
    let ro = Some(0o400);
    assert_eq!(
        recorder.seen.into_inner().unwrap(),
        vec![
            seen("done Looked", false, None, false, false),
            seen("sync dir for Made", false, None, false, false),
            seen("done Made", false, None, false, false),
            seen("done Writing", true, None, false, false),
            seen("done Copied", true, None, false, false),
            seen("sync file for Synced", true, None, false, false),
            seen("done Synced", true, None, false, false),
            seen("done Renamed", false, rw, false, false),
            seen("sync dir for Sealed", false, ro, false, false),
            seen("done Sealed", false, ro, false, false),
            seen("sync file for MetaWritten", false, ro, true, false),
            seen("done MetaWritten", false, ro, true, false),
            seen("sync dir for Ready", false, ro, false, true),
            seen("done Ready", false, ro, false, true),
        ]
    );
    assert_eq!(
        recorder.swept_while_writing.into_inner().unwrap(),
        Some(0),
        "a sweep during staging deleted the copy being written"
    );
    assert_eq!(ready.attachment.sha256.as_str(), sha256_of(&source));
    assert_eq!(ready.attachment.size_bytes, bytes.len() as u64);
    assert_eq!(ready.attachment.kind, Kind::Image);
    assert_eq!(ready.attachment.name.as_str(), name);
    assert_eq!(ready.attachment.display_name, name);
    assert_eq!(ready.ready_at, T0);
    assert_eq!(
        store.verify(&ready.attachment.id).unwrap(),
        Verified::Ready(ready.clone())
    );
}

#[test]
fn stopped_after_any_step_nothing_usable_is_left_or_a_verified_attachment() {
    for (step_name, step) in STEPS {
        let bench = Bench::new(&format!("ap04_stop_{step_name}"));
        let source = bench.source(
            "syn-ap04-stop.png",
            &padded(PNG, (COPY_CHUNK_BYTES * 2 + 5) as u64),
        );
        let before = sha256_of(&source);
        let out = bench.root.join("child.json");
        let child = run_child(&json!({
            "mode": "kill",
            "state": bench.state,
            "source": source,
            "step": step_name,
            "out": out,
        }));
        assert_eq!(
            child.status.signal(),
            Some(6),
            "{step_name}: the child was not stopped there: {child:?}"
        );
        assert!(!out.exists(), "{step_name}: the child ran past its step");
        assert_eq!(sha256_of(&source), before, "the original is unchanged");

        let store = bench.store();
        let listed = store.list(&NoClaims).unwrap();
        let ids = bench.ids();
        if step == Step::Looked {
            // Only looked at: nothing was made, not even the lock.
            assert!(listed.is_empty(), "{step_name}: {listed:?}");
            assert!(
                bench.entries().is_empty(),
                "{step_name}: {:?}",
                bench.entries()
            );
            continue;
        }
        assert_eq!(ids.len(), 1, "{step_name}: one directory was made");
        let id = AttachmentId::parse(&ids[0]).unwrap();
        if step == Step::Ready {
            let [Listed::Ready { attachment, .. }] = listed.as_slice() else {
                panic!("{step_name}: {listed:?}");
            };
            assert_eq!(attachment.attachment.sha256.as_str(), before);
            assert_eq!(
                store.verify(&id).unwrap(),
                Verified::Ready(attachment.clone())
            );
            let swept = store.sweep(T0, &NoClaims).unwrap();
            assert_eq!(swept.unfinished, 0, "{step_name}");
            assert_eq!(bench.ids(), ids, "{step_name}: the ready one is kept");
        } else {
            assert!(listed.is_empty(), "{step_name}: {listed:?}");
            assert_eq!(store.verify(&id).unwrap(), Verified::NotFound);
            let swept = store.sweep(T0, &NoClaims).unwrap();
            assert_eq!(swept.unfinished, 1, "{step_name}");
            // B2 (the implementation): a sweep holds the claims lock while it
            // applies the retention rule, so the lock file is there too.
            assert_eq!(
                bench.entries(),
                [CLAIMS_LOCK, STAGING_LOCK],
                "{step_name}: the unfinished copy is deleted"
            );
        }
    }
}

#[test]
fn a_copy_changed_after_ready_is_never_verified() {
    let bench = Bench::new("ap04_changed");
    let bytes = padded(PNG, 4096);
    let source = bench.source("syn-ap04-changed.png", &bytes);
    let store = bench.store();
    let Staged::Ready(ready) = store.stage(&source, T0, &Free::new(PLENTY)).unwrap() else {
        panic!("not ready");
    };
    let id = ready.attachment.id.clone();
    let copy = bench
        .attachments()
        .join(id.as_str())
        .join("syn-ap04-changed.png");
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o600)).unwrap();

    // The same size, other bytes: only the hash shows it.
    let mut other = bytes.clone();
    *other.last_mut().unwrap() ^= 1;
    fs::write(&copy, &other).unwrap();
    assert_eq!(
        store.verify(&id).unwrap(),
        Verified::Failed(FailReason::SizeMismatch)
    );

    // Another size: listing shows it too.
    fs::write(&copy, &bytes[..100]).unwrap();
    assert_eq!(
        store.verify(&id).unwrap(),
        Verified::Failed(FailReason::SizeMismatch)
    );
    assert_eq!(
        store.list(&NoClaims).unwrap(),
        vec![Listed::Broken {
            id: id.clone(),
            reason: FailReason::SizeMismatch
        }]
    );

    // A link in place of the copy is not the copy.
    fs::remove_file(&copy).unwrap();
    std::os::unix::fs::symlink(&source, &copy).unwrap();
    assert_eq!(
        store.verify(&id).unwrap(),
        Verified::Failed(FailReason::SizeMismatch)
    );
    assert!(matches!(
        store.list(&NoClaims).unwrap().as_slice(),
        [Listed::Broken { .. }]
    ));
}

#[test]
fn metadata_that_is_not_exactly_the_record_is_never_ready() {
    let bench = Bench::new("ap04_meta");
    let source = bench.source("syn-ap04-meta.txt", crate::bench::CONTENT.as_bytes());
    let store = bench.store();
    let Staged::Ready(ready) = store.stage(&source, T0, &Free::new(PLENTY)).unwrap() else {
        panic!("not ready");
    };
    let id = ready.attachment.id.clone();
    let meta = bench.attachments().join(id.as_str()).join(META);
    let good: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&meta).unwrap()).unwrap();
    let mut other_id = good.clone();
    other_id[0]["id"] = json!("att-synthetic-other");
    let mut extra = good.clone();
    extra[0]["extra"] = json!(true);
    let mut fraction = good.clone();
    fraction[1] = json!(1.5);
    let text = good.to_string();
    let twice = text.replacen("{\"", "{\"kind\":\"file\",\"", 1);
    assert!(twice.contains("\"kind\":\"file\"") && twice.matches("\"kind\"").count() == 2);
    for bad in [
        other_id.to_string(),
        extra.to_string(),
        fraction.to_string(),
        json!([good[0]]).to_string(),
        json!([good[0], good[1], 0]).to_string(),
        json!({"attachment": good[0], "ready_at": good[1]}).to_string(),
        twice,
        "not JSON".to_owned(),
    ] {
        fs::write(&meta, &bad).unwrap();
        assert_eq!(
            store.list(&NoClaims).unwrap(),
            vec![Listed::Broken {
                id: id.clone(),
                reason: FailReason::Unreadable
            }],
            "{bad}"
        );
        assert_eq!(
            store.verify(&id).unwrap(),
            Verified::Failed(FailReason::Unreadable),
            "{bad}"
        );
    }
    fs::write(&meta, &text).unwrap();
    assert_eq!(store.verify(&id).unwrap(), Verified::Ready(ready));
}

/// Rewrites the source in place (the same inode, the same length) once its
/// first bytes are copied.
struct RewriteWhileCopying(std::path::PathBuf, Vec<u8>);

impl Durability for RewriteWhileCopying {
    fn done(&self, step: Step) {
        if step == Step::Writing {
            fs::write(&self.0, &self.1).unwrap();
        }
    }
}

#[test]
fn a_source_rewritten_while_it_is_copied_is_never_ready() {
    let bench = Bench::new("ap04_rewritten");
    let store = bench.store();
    let len = COPY_CHUNK_BYTES * 2 + 4096;
    for (name, old, new) in [
        ("syn-ap04-rewritten.txt", vec![b'A'; len], vec![b'B'; len]),
        // Only the first byte differs, in the part already copied: the
        // length and every later byte agree, so only the bytes show it.
        ("syn-ap04-first-byte.txt", vec![b'A'; len], {
            let mut bytes = vec![b'A'; len];
            bytes[0] = b'B';
            bytes
        }),
    ] {
        let source = bench.source(name, &old);
        let staged = store
            .stage_with(
                &source,
                T0,
                &Free::new(PLENTY),
                &RewriteWhileCopying(source.clone(), new.clone()),
            )
            .unwrap();
        assert_eq!(staged, Staged::Failed(FailReason::SizeMismatch), "{name}");
        assert!(bench.ids().is_empty(), "{name}: nothing was staged");
        assert_eq!(
            fs::read(&source).unwrap(),
            new,
            "{name}: the source is left as it is"
        );
    }
}
