//! The send record on disk: one owner-only directory of JSON files that
//! implements the core's [`SendStore`]. OpenWiz 0.0.4 adds the
//! `new_thread` record kind beside it, in `new-threads/` ([`new_thread`],
//! the implementation), on the same rules and through the same code: [`Store`] is
//! generic over the record's [`Kind`]. [`FileStore`] is `Store<Sends>`, and
//! [`NewThreadFileStore`] is `Store<NewThreads>`.
//!
//! The staged attachments (OpenWiz 0.0.4, the implementation) live in
//! `attachments/` ([`attachments`]): not a record kind, but an [`Area`] of
//! the same state directory, opened through the same checks.
//! [`AttachmentStore`] is `Store<Attachments>`.
//!
//! Layout, under the state directory (0700), for each kind (`sends/` here;
//! `new-threads/` the same):
//!
//! - `sends/` (0700);
//! - `sends/<op id>.json` (0600): the record, replaced atomically (written
//!   to a temporary file, synced, renamed, and the directory synced);
//! - `sends/<op id>.lock` (0600): the claim. The process that owns an op id
//!   holds an exclusive lock on it until it finishes or stops. The lock is
//!   released by the operating system when the process ends, which is how
//!   a stopped owner is recognised and its record recovered.
//!
//! The state directory is outside the repository, in the owner's per-user
//! state directory (see [`default_state_dir`]). Provisional; the maintainer decides.
//!
//! Standard library only: `OpenOptions::create_new`, `File::try_lock`,
//! `sync_all` and Unix permissions.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::marker::PhantomData;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use openwiz_core::messaging::record::{
    Claim, RecordState, SendRecord, SendStore, StoreError, recover,
};
use openwiz_core::messaging::send::Delivery;
use openwiz_core::messaging::{Binding, OpId};
use openwiz_core::model::Timestamp;

pub mod attachments;
pub mod new_thread;

pub use attachments::{AttachmentStore, Attachments};
pub use new_thread::{NewThreadFileStore, NewThreads};

/// Overrides the state directory, for tests and for owners who want it
/// elsewhere.
pub const STATE_DIR_ENV: &str = "OPENWIZ_STATE_DIR";

/// How long a claimer waits for a live owner's record to appear before it
/// reports the op id as pending.
pub const PENDING_WAIT_MS: u64 = 2_000;

/// The default state directory: `$OPENWIZ_STATE_DIR`; else on macOS
/// `~/Library/Application Support/openwiz`; else `$XDG_STATE_HOME/openwiz`
/// (when it is absolute) or `~/.local/state/openwiz`. `var` looks up an
/// environment variable. Without an absolute home, nothing is guessed.
pub fn default_state_dir(var: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf, StoreError> {
    let absolute = |name: &str| {
        var(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if let Some(dir) = var(STATE_DIR_ENV).filter(|v| !v.is_empty()) {
        let dir = PathBuf::from(dir);
        if !dir.is_absolute() {
            return Err(StoreError(format!(
                "{STATE_DIR_ENV} must be an absolute path"
            )));
        }
        return Ok(dir);
    }
    let home = absolute("HOME").ok_or_else(|| {
        StoreError(format!(
            "no state directory: HOME is not an absolute path, and {STATE_DIR_ENV} is not set"
        ))
    })?;
    if cfg!(target_os = "macos") {
        return Ok(home.join("Library/Application Support/openwiz"));
    }
    Ok(absolute("XDG_STATE_HOME")
        .unwrap_or_else(|| home.join(".local/state"))
        .join("openwiz"))
}

/// Syncs one directory: a real `fsync`, or a test's recorder.
type SyncDir<'a> = dyn Fn(&Path) -> io::Result<()> + 'a;

/// A test hook: runs with an op id just before this store tries the lock of
/// a record it has already read.
type BeforeLock = Box<dyn Fn(&OpId) + Send + Sync>;

/// One directory of the state directory: its name, and what it is called
/// in messages. Opening it runs the store's directory checks, the same for
/// every area.
pub trait Area: Send + Sync + 'static {
    /// Its directory under the state directory.
    const DIR: &'static str;
    /// What it holds, in messages.
    const WHAT: &'static str;
}

/// One kind of record the store keeps, in its own [`Area`], and how its
/// records are read, written and recovered. The claim, the lock, the
/// atomic write and the directory checks are the store's own, the same for
/// every kind.
pub trait Kind: Area {
    type Record: Clone + PartialEq;
    fn op_id(record: &Self::Record) -> &OpId;
    fn is_finished(record: &Self::Record) -> bool;
    /// Whether the record is still in the claimed state: nothing was read
    /// or sent for it yet.
    fn is_claimed(record: &Self::Record) -> bool;
    /// The core's recovery rule: never a redispatch.
    fn recover(record: &Self::Record, now: Timestamp) -> Self::Record;
    /// Reads a record file's text; why not, in OpenWiz's words.
    fn read(text: &str) -> Result<Self::Record, String>;
    /// The record's JSON.
    fn body(record: &Self::Record) -> Result<Vec<u8>, String>;
}

/// The send record (OpenWiz 0.0.3, the implementation): `sends/`.
#[derive(Debug)]
pub struct Sends;

impl Area for Sends {
    const DIR: &'static str = "sends";
    const WHAT: &'static str = "send record";
}

impl Kind for Sends {
    type Record = SendRecord;

    fn op_id(record: &SendRecord) -> &OpId {
        &record.op_id
    }

    fn is_finished(record: &SendRecord) -> bool {
        record.state == RecordState::Finished
    }

    fn is_claimed(record: &SendRecord) -> bool {
        record.state == RecordState::Claimed
    }

    fn recover(record: &SendRecord, now: Timestamp) -> SendRecord {
        recover(record, now)
    }

    /// The 0.0.3 reader, and (B2) the record's attachments read strictly
    /// (each with exactly its five keys) and checked against the binding's
    /// digest: a record whose attachments don't agree with what its op id
    /// binds is refused.
    fn read(text: &str) -> Result<SendRecord, String> {
        let record: SendRecord = serde_json::from_str(text).map_err(|_| String::new())?;
        if !record.attachments_agree() {
            return Err("its attachments are not the ones its binding holds".to_owned());
        }
        Ok(record)
    }

    fn body(record: &SendRecord) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(record).map_err(|e| e.to_string())
    }
}

/// The file-backed record of one [`Kind`], or one other [`Area`].
pub struct Store<K: Area> {
    dir: PathBuf,
    /// The claims this process holds: the locked `.lock` file of each.
    held: Mutex<HashMap<OpId, File>>,
    /// Refuses every write: opened by [`Store::open_read_only`].
    read_only: bool,
    before_lock: Option<BeforeLock>,
    kind: PhantomData<K>,
}

/// The file-backed send record.
pub type FileStore = Store<Sends>;

impl<K: Area> std::fmt::Debug for Store<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

/// Creates `dir` and any missing parents (0700), one at a time, syncing each
/// new directory's parent so the new entry itself is durable.
fn create_dir_synced(dir: &Path, sync: &dyn Fn(&Path) -> io::Result<()>) -> Result<(), StoreError> {
    let create_dir = |d: &Path| DirBuilder::new().mode(0o700).create(d);
    let mut missing = Vec::new();
    let mut next = Some(dir);
    while let Some(d) = next.filter(|d| !d.as_os_str().is_empty() && !d.exists()) {
        missing.push(d);
        next = d.parent();
    }
    for d in missing.into_iter().rev() {
        match create_dir(d) {
            Ok(()) => {}
            // Another process made it first; its parent is synced anyway.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_error("make", d, &e)),
        }
        let parent = d
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        sync(parent).map_err(|e| io_error("sync", parent, &e))?;
    }
    Ok(())
}

/// Syncs a directory, so the entries in it are durable.
fn fsync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// The repository `dir` would be inside, if any: the nearest existing
/// ancestor, resolved (symlinks too), or any of its ancestors, holds `.git`.
fn inside_repository(dir: &Path) -> Option<PathBuf> {
    let mut existing = dir;
    while !existing.exists() {
        existing = existing.parent()?;
    }
    let resolved = existing.canonicalize().ok()?;
    resolved
        .ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
}

fn io_error(what: &str, path: &Path, e: &io::Error) -> StoreError {
    StoreError(format!("could not {what} {}: {e}", path.display()))
}

/// What claiming an op id found, for any kind.
enum Claimed<R> {
    Owned,
    Existing(R),
    InProgress(R),
    Pending,
}

impl<K: Area> Store<K> {
    /// Opens (and creates, 0700) the store under `state_dir`. Refuses a
    /// directory that group or others can read, write or enter, and one
    /// inside a repository.
    pub fn open(state_dir: &Path) -> Result<Self, StoreError> {
        Self::open_with_sync(state_dir, &fsync_dir)
    }

    /// [`open`](Self::open), with every directory sync made through `sync`.
    /// Tests pass a recorder; `open` passes a real `fsync`.
    ///
    /// - `state_dir` must be absolute, with no `.` or `..` component;
    /// - it must not be inside a repository, before anything is created, and
    ///   neither may the resolved state directory or the kind's directory
    ///   (`sends/`, `new-threads/`) afterwards;
    /// - the kind's directory must be a real directory, not a symlink;
    /// - every directory from the resolved kind's directory up to the root is
    ///   synced on every open, so a chain another process created and has
    ///   not synced yet is durable before this process dispatches.
    pub fn open_with_sync(
        state_dir: &Path,
        sync: &dyn Fn(&Path) -> io::Result<()>,
    ) -> Result<Self, StoreError> {
        let dir = Self::checked_sends(state_dir, Some(sync))?
            .ok_or_else(|| StoreError(format!("{} was not made", state_dir.display())))?;
        Ok(Self::new(dir, false))
    }

    /// Opens an existing store for reading only, for a lookup or a status
    /// read: nothing is created, synced or written, and every write is
    /// refused. `None` when the state directory or the kind's directory does
    /// not exist yet: nothing was ever claimed there. The same checks as
    /// [`open`](Self::open) otherwise.
    pub fn open_read_only(state_dir: &Path) -> Result<Option<Self>, StoreError> {
        Ok(Self::checked_sends(state_dir, None)?.map(|dir| Self::new(dir, true)))
    }

    fn new(dir: PathBuf, read_only: bool) -> Self {
        Self {
            dir,
            held: Mutex::new(HashMap::new()),
            read_only,
            before_lock: None,
            kind: PhantomData,
        }
    }

    /// The checks of [`open_with_sync`](Self::open_with_sync), and the
    /// kind's directory. With `sync`, missing directories are made and the
    /// chain is synced. Without it nothing is made or synced, and a missing
    /// directory gives `None`. Named for the send record, the store's first
    /// kind; it checks every area's directory the same way.
    fn checked_sends(
        state_dir: &Path,
        sync: Option<&SyncDir<'_>>,
    ) -> Result<Option<PathBuf>, StoreError> {
        let what = K::WHAT;
        let plain = state_dir.is_absolute()
            && state_dir
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
        if !plain {
            return Err(StoreError(format!(
                "{} must be an absolute path with no . or .. component",
                state_dir.display()
            )));
        }
        let in_repository = |dir: &Path| {
            inside_repository(dir).map(|repo| {
                StoreError(format!(
                    "{} is inside the repository at {}; the {what} must be outside every \
                     repository",
                    dir.display(),
                    repo.display()
                ))
            })
        };
        if let Some(refused) = in_repository(state_dir) {
            return Err(refused);
        }
        let records = state_dir.join(K::DIR);
        for dir in [state_dir, &records] {
            if let Some(sync) = sync {
                create_dir_synced(dir, sync)?;
            }
            let meta = match fs::metadata(dir) {
                Ok(meta) => meta,
                Err(e) if sync.is_none() && e.kind() == io::ErrorKind::NotFound => {
                    return Ok(None);
                }
                Err(e) => return Err(io_error("read", dir, &e)),
            };
            if !meta.is_dir() {
                return Err(StoreError(format!("{} is not a directory", dir.display())));
            }
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(StoreError(format!(
                    "{} is open to other users (mode {mode:o}); the {what} must be \
                     readable by its owner only (0700)",
                    dir.display()
                )));
            }
        }
        let records_meta =
            fs::symlink_metadata(&records).map_err(|e| io_error("read", &records, &e))?;
        if records_meta.file_type().is_symlink() {
            return Err(StoreError(format!(
                "{} is a symlink; the {what}'s directory must be a real directory",
                records.display()
            )));
        }
        let resolved = records
            .canonicalize()
            .map_err(|e| io_error("resolve", &records, &e))?;
        for dir in resolved.ancestors() {
            if let Some(refused) = in_repository(dir) {
                return Err(refused);
            }
        }
        if let Some(sync) = sync {
            for dir in resolved.ancestors() {
                sync(dir).map_err(|e| io_error("sync", dir, &e))?;
            }
        }
        Ok(Some(records))
    }

    /// The kind's directory (`sends/`, `new-threads/`), or the area's.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn refuse_if_read_only(&self) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError(format!(
                "the {} was opened for reading only",
                K::WHAT
            )));
        }
        Ok(())
    }
}

impl<K: Kind> Store<K> {
    /// Sets a test hook that runs with an op id just before this store
    /// tries the lock of a record it has already read, so a test can change
    /// the record in between. `open` sets none.
    pub fn with_before_lock(mut self, hook: impl Fn(&OpId) + Send + Sync + 'static) -> Self {
        self.before_lock = Some(Box::new(hook));
        self
    }

    /// The record file of `op_id`.
    pub fn record_path(&self, op_id: &OpId) -> PathBuf {
        self.dir.join(format!("{op_id}.json"))
    }

    fn lock_path(&self, op_id: &OpId) -> PathBuf {
        self.dir.join(format!("{op_id}.lock"))
    }

    /// Opens (creating it, 0600) the lock file of `op_id`, unlocked. Refused
    /// for a read-only store.
    fn lock_file(&self, op_id: &OpId) -> Result<File, StoreError> {
        self.refuse_if_read_only()?;
        let path = self.lock_path(op_id);
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|e| io_error("open", &path, &e))
    }

    /// The record of `op_id` as it is on disk.
    pub fn record(&self, op_id: &OpId) -> Result<Option<K::Record>, StoreError> {
        self.read_record(&self.record_path(op_id))
    }

    fn read_record(&self, path: &Path) -> Result<Option<K::Record>, StoreError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_error("read", path, &e)),
        };
        let record = K::read(&text).map_err(|why| {
            let why = if why.is_empty() {
                String::new()
            } else {
                format!(" ({why})")
            };
            StoreError(format!("{} is not a {}{why}", path.display(), K::WHAT))
        })?;
        // A record is only ever read from its own op id's file.
        if path.file_name() != Some(OsStr::new(&format!("{}.json", K::op_id(&record)))) {
            return Err(StoreError(format!(
                "{} holds the record of another op id",
                path.display()
            )));
        }
        Ok(Some(record))
    }

    /// Writes `record` in place of the old one: to a new temporary file
    /// (0600), synced, renamed over the record, and the directory synced.
    /// Refused for a read-only store.
    fn write_record(&self, record: &K::Record) -> Result<(), StoreError> {
        self.refuse_if_read_only()?;
        let op_id = K::op_id(record);
        let path = self.record_path(op_id);
        let temp = self.dir.join(format!(".{op_id}.json.tmp"));
        let mut body = K::body(record)
            .map_err(|_| StoreError(format!("a {} could not be serialized", K::WHAT)))?;
        body.push(b'\n');
        let _ = fs::remove_file(&temp);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| io_error("make", &temp, &e))?;
        file.write_all(&body)
            .and_then(|()| file.sync_all())
            .map_err(|e| io_error("write", &temp, &e))?;
        drop(file);
        fs::rename(&temp, &path).map_err(|e| io_error("replace", &path, &e))?;
        File::open(&self.dir)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| io_error("sync", &self.dir, &e))
    }

    fn held_by_me(&self, op_id: &OpId) -> bool {
        self.held
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(op_id)
    }

    /// Claims `record`'s op id: atomic across processes, through the lock.
    fn claim_record(
        &self,
        record: &K::Record,
        now: Timestamp,
    ) -> Result<Claimed<K::Record>, StoreError> {
        self.refuse_if_read_only()?;
        let op_id = K::op_id(record);
        let lock = self.lock_file(op_id)?;
        let started = Instant::now();
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {
                    // A live owner. Its record follows its lock within
                    // moments; if the owner lets go first, try again.
                    if let Some(existing) = self.record(op_id)? {
                        return Ok(if K::is_finished(&existing) {
                            Claimed::Existing(existing)
                        } else {
                            Claimed::InProgress(existing)
                        });
                    }
                    if started.elapsed() >= Duration::from_millis(PENDING_WAIT_MS) {
                        return Ok(Claimed::Pending);
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(TryLockError::Error(e)) => {
                    return Err(io_error("lock", &self.lock_path(op_id), &e));
                }
            }
        }
        // This process holds the lock: nobody else is using this op id.
        match self.record(op_id)? {
            None => {
                self.write_record(record)?;
                self.held
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(op_id.clone(), lock);
                Ok(Claimed::Owned)
            }
            Some(existing) if K::is_finished(&existing) => Ok(Claimed::Existing(existing)),
            Some(abandoned) => {
                // Its owner stopped: the lock was free. Never redispatched.
                let recovered = K::recover(&abandoned, now);
                self.write_record(&recovered)?;
                Ok(Claimed::Existing(recovered))
            }
        }
    }

    /// Writes the owner's new version of a claimed record, synced.
    fn update_record(&self, record: &K::Record) -> Result<(), StoreError> {
        self.refuse_if_read_only()?;
        if !self.held_by_me(K::op_id(record)) {
            return Err(StoreError(format!(
                "op {} is not claimed by this process",
                K::op_id(record)
            )));
        }
        self.write_record(record)
    }

    /// Gives up this process's claim on `op_id`.
    fn release_claim(&self, op_id: &OpId) {
        // Dropping the file releases its lock.
        self.held
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(op_id);
    }

    /// The record of `op_id`, never written: a record whose owner stopped
    /// is shown as recovery would finish it, in memory only.
    fn inspect_record(
        &self,
        op_id: &OpId,
        now: Timestamp,
    ) -> Result<Option<K::Record>, StoreError> {
        let Some(record) = self.record(op_id)? else {
            return Ok(None);
        };
        if K::is_finished(&record) || self.held_by_me(op_id) {
            return Ok(Some(record));
        }
        // Is its owner still live? The lock file is opened for reading,
        // never made; without one, the owner can't be ruled out.
        let path = self.lock_path(op_id);
        let lock = match File::open(&path) {
            Ok(lock) => lock,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Some(record)),
            Err(e) => return Err(io_error("open", &path, &e)),
        };
        if let Some(hook) = &self.before_lock {
            hook(op_id);
        }
        match lock.try_lock() {
            // Its owner stopped. Read again under the lock (it may have been
            // finished or withdrawn), and show what recovery would make of
            // it, in memory only: the next use of this op id records it.
            Ok(()) => Ok(self.record(op_id)?.map(|current| K::recover(&current, now))),
            Err(TryLockError::WouldBlock) => Ok(Some(record)),
            Err(TryLockError::Error(e)) => Err(io_error("lock", &path, &e)),
        }
    }
}

impl Store<Sends> {
    /// The `sends/` directory.
    pub fn sends_dir(&self) -> &Path {
        &self.dir
    }

    /// Recovers `record`, as read from disk, if its owner has stopped:
    /// takes its lock without waiting, reads it again under the lock, and
    /// finishes it with the core's recovery rule. Returns the record as it
    /// now is, and whether its owner is still live; `None` when it is gone
    /// under the lock: its owner withdrew it, and it is never recreated.
    fn recover_if_abandoned(
        &self,
        record: SendRecord,
        now: Timestamp,
    ) -> Result<Option<(SendRecord, bool)>, StoreError> {
        self.refuse_if_read_only()?;
        if record.state == RecordState::Finished {
            return Ok(Some((record, false)));
        }
        if self.held_by_me(&record.op_id) {
            return Ok(Some((record, true)));
        }
        let lock = self.lock_file(&record.op_id)?;
        if let Some(hook) = &self.before_lock {
            hook(&record.op_id);
        }
        match lock.try_lock() {
            Ok(()) => {
                // Read again under the lock: the owner may have finished,
                // or withdrawn it.
                let Some(current) = self.record(&record.op_id)? else {
                    return Ok(None);
                };
                let recovered = recover(&current, now);
                if recovered != current {
                    self.write_record(&recovered)?;
                }
                Ok(Some((recovered, false)))
            }
            Err(TryLockError::WouldBlock) => Ok(Some((record, true))),
            Err(TryLockError::Error(e)) => {
                Err(io_error("lock", &self.lock_path(&record.op_id), &e))
            }
        }
    }

    /// Every record, other than `except`'s, whose binding has the target and
    /// text hash of `binding` (any mode), as it is on disk.
    fn same_text(&self, binding: &Binding, except: &OpId) -> Result<Vec<SendRecord>, StoreError> {
        let entries = fs::read_dir(&self.dir).map_err(|e| io_error("list", &self.dir, &e))?;
        let mut found = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| io_error("list", &self.dir, &e))?.path();
            let is_record = path.extension().is_some_and(|e| e == "json")
                && !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if !is_record {
                continue;
            }
            let Some(record) = self.read_record(&path)? else {
                continue;
            };
            if record.op_id != *except
                && record.binding.target == binding.target
                && record.binding.text_sha256 == binding.text_sha256
            {
                found.push(record);
            }
        }
        Ok(found)
    }
}

impl<K: Kind> Store<K> {
    /// Removes the record of `op_id`, which this process holds and which is
    /// still in the claimed state, and syncs the directory.
    fn withdraw_claimed(&self, op_id: &OpId) -> Result<(), StoreError> {
        if !self.held_by_me(op_id) {
            return Err(StoreError(format!(
                "op {op_id} is not claimed by this process"
            )));
        }
        let path = self.record_path(op_id);
        match self.read_record(&path)? {
            Some(record) if K::is_claimed(&record) => {}
            Some(_) => {
                return Err(StoreError(format!(
                    "op {op_id} is past its claim, so its record is kept"
                )));
            }
            None => return Ok(()),
        }
        fs::remove_file(&path).map_err(|e| io_error("remove", &path, &e))?;
        File::open(&self.dir)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| io_error("sync", &self.dir, &e))
    }
}

impl SendStore for Store<Sends> {
    fn load(&self, op_id: &OpId) -> Result<Option<SendRecord>, StoreError> {
        self.record(op_id)
    }

    fn claim(&self, record: &SendRecord, now: Timestamp) -> Result<Claim, StoreError> {
        Ok(match self.claim_record(record, now)? {
            Claimed::Owned => Claim::Owned,
            Claimed::Existing(r) => Claim::Existing(r),
            Claimed::InProgress(r) => Claim::InProgress(r),
            Claimed::Pending => Claim::Pending,
        })
    }

    fn update(&self, record: &SendRecord) -> Result<(), StoreError> {
        self.update_record(record)
    }

    fn release(&self, op_id: &OpId) {
        self.release_claim(op_id);
    }
    fn uncertain_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError> {
        // Recovery may write: never through a read-only store.
        self.refuse_if_read_only()?;
        let same_text = |r: &SendRecord| {
            r.binding.target == binding.target && r.binding.text_sha256 == binding.text_sha256
        };
        let mut found = Vec::new();
        for record in self.same_text(binding, except)? {
            let Some((record, _live)) = self.recover_if_abandoned(record, now)? else {
                continue;
            };
            // Matched again: it may have been read again under its lock.
            if same_text(&record) && matches!(record.delivery, Some(Delivery::Uncertain { .. })) {
                found.push(record);
            }
        }
        found.sort_by(|a, b| (a.claimed_at, &a.op_id).cmp(&(b.claimed_at, &b.op_id)));
        Ok(found)
    }

    fn in_flight_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError> {
        // Recovery may write: never through a read-only store.
        self.refuse_if_read_only()?;
        let mut found = Vec::new();
        for record in self.same_text(binding, except)? {
            if record.binding != *binding {
                continue;
            }
            let Some((record, live)) = self.recover_if_abandoned(record, now)? else {
                continue;
            };
            // Matched again: it may have been read again under its lock.
            if live && record.state != RecordState::Finished && record.binding == *binding {
                found.push(record);
            }
        }
        found.sort_by(|a, b| (a.claimed_at, &a.op_id).cmp(&(b.claimed_at, &b.op_id)));
        Ok(found)
    }

    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<SendRecord>, StoreError> {
        self.inspect_record(op_id, now)
    }

    fn withdraw(&self, op_id: &OpId) -> Result<(), StoreError> {
        self.refuse_if_read_only()?;
        let withdrawn = self.withdraw_claimed(op_id);
        // Dropping the lock releases the claim, after the record is gone.
        self.release(op_id);
        withdrawn
    }
}
