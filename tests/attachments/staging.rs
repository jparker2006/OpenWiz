//! Staged attachments for the B2 proofs (the implementation), shared by the
//! messaging and new-thread targets (`#[path]`): synthetic files, written at
//! test time beside a test's private state directory (never committed),
//! staged through B1's store, and read back from it.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use openwiz_core::attachment::{AttachmentId, Kind};
use openwiz_core::model::Timestamp;
use openwiz_store::AttachmentStore;
use openwiz_store::attachments::{CLAIM, FreeSpace, ReadyAttachment, Staged};
use serde_json::Value;

/// Room enough on every volume: the free-disk floor is B1's proof, not
/// these.
pub struct Plenty;

impl FreeSpace for Plenty {
    fn free_bytes(&self, _dir: &Path) -> Result<u64, String> {
        Ok(1 << 50)
    }
}

/// A synthetic PNG of exactly `len` bytes.
pub fn png(len: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.resize(len.max(bytes.len()), 0x5a);
    bytes
}

/// A synthetic text file.
pub fn text(content: &str) -> Vec<u8> {
    content.as_bytes().to_vec()
}

/// Where a test's source files go: beside its state directory.
fn sources(state: &Path) -> PathBuf {
    let name = state.file_name().unwrap().to_string_lossy();
    state.with_file_name(format!("{name}-sources"))
}

/// Writes `bytes` as `name` and stages it into the store under `state`,
/// ready at `at`.
pub fn stage(state: &Path, name: &str, bytes: &[u8], at: Timestamp) -> ReadyAttachment {
    let dir = sources(state);
    crate::dirs::private_dir_all(&dir).unwrap();
    let source = dir.join(name);
    fs::write(&source, bytes).unwrap();
    let store = AttachmentStore::open(state).expect("open the attachment store");
    match store.stage(&source, at, &Plenty).expect("stage") {
        Staged::Ready(ready) => ready,
        Staged::Failed(reason) => panic!("{name} failed to stage: {reason:?}"),
    }
}

/// The real time now: what a release `openwiz` process judges a staged
/// attachment's age by (the in-process tests use a fixed clock).
pub fn now() -> Timestamp {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_millis();
    Timestamp(i64::try_from(ms).expect("in range"))
}

/// The id of a staged attachment, as text.
pub fn id_of(ready: &ReadyAttachment) -> String {
    ready.attachment.id.as_str().to_owned()
}

pub fn attachment_id(id: &str) -> AttachmentId {
    AttachmentId::parse(id).unwrap()
}

pub fn store(state: &Path) -> AttachmentStore {
    AttachmentStore::open(state).expect("open the attachment store")
}

/// The staged copy's path: `<state>/attachments/<id>/<safe name>`.
pub fn copy_path(state: &Path, ready: &ReadyAttachment) -> PathBuf {
    state
        .join("attachments")
        .join(ready.attachment.id.as_str())
        .join(ready.attachment.name.as_str())
}

/// Whether the attachment's directory is still in the store.
pub fn kept(state: &Path, ready: &ReadyAttachment) -> bool {
    state
        .join("attachments")
        .join(ready.attachment.id.as_str())
        .is_dir()
}

/// The attachment's claim file, as JSON, if any.
pub fn claim_of(state: &Path, ready: &ReadyAttachment) -> Option<Value> {
    let path = state
        .join("attachments")
        .join(ready.attachment.id.as_str())
        .join(CLAIM);
    let text = fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&text).expect("a claim is JSON"))
}

/// The flag `room-cli` gets for this attachment.
pub fn flag_of(ready: &ReadyAttachment) -> &'static str {
    match ready.attachment.kind {
        Kind::Image => "--image",
        Kind::File => "--file",
    }
}

/// The lines of the fake's `attachments.log` in `fake_dir`.
pub fn uploads(fake_dir: &Path) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(fake_dir.join("attachments.log")) else {
        return vec![];
    };
    text.lines()
        .map(|l| serde_json::from_str(l).expect("a log line is JSON"))
        .collect()
}

/// Makes the staged copy of `ready` writable and rewrites it with `bytes`
/// (a tampered copy: its size or hash no longer match its record).
pub fn tamper(state: &Path, ready: &ReadyAttachment, bytes: &[u8]) {
    use std::os::unix::fs::PermissionsExt;
    let path = copy_path(state, ready);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
}
