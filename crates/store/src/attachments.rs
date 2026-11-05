use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use openwiz_core::attachment::{
    AttachmentId, AttachmentRef, Claim, DeleteReason, FailReason, Kind, RemoveRefused, Retention,
    SNIFF_BYTES, STAGING_DIR, SafeName, Sha256, StagedAttachment, StagedPath, Stored, can_remove,
    check, retention, staging_room,
};
use openwiz_core::messaging::Sha256Hasher;
use openwiz_core::messaging::record::{RecordState, SendStore, StoreError};
use openwiz_core::messaging::send::Delivery;
use openwiz_core::model::Timestamp;
use openwiz_core::new_thread::outcome::Outcome;
use openwiz_core::new_thread::read::{StrictJson, op_ref};
use openwiz_core::new_thread::record::{NewThreadStore, RecordState as NewThreadState};
use openwiz_core::prompt::{
    AttachmentProblem, ClaimOutcome, Lookup, OpKind, OpRef, Problem, Refusal, StagedAttachments,
    claimable,
};

use crate::{Area, FileStore, NewThreadFileStore, Store, io_error};

/// The staged attachments: `attachments/`.
#[derive(Debug)]
pub struct Attachments;

impl Area for Attachments {
    const DIR: &'static str = STAGING_DIR;
    const WHAT: &'static str = "attachment store";
}

/// The file-backed staged attachments.
pub type AttachmentStore = Store<Attachments>;

/// The lock every staging shares, in `attachments/`.
pub const STAGING_LOCK: &str = ".staging.lock";
/// One attachment's metadata, in its directory.
pub const META: &str = ".meta.json";
/// The lock every claim, release, settle, removal and sweep holds, in
/// `attachments/` (B2).
pub const CLAIMS_LOCK: &str = ".claims.lock";
/// The operation that claims one attachment, in its directory (B2).
pub const CLAIM: &str = ".claim.json";
/// What an unfinished copy's name ends with.
pub const PARTIAL_SUFFIX: &str = ".partial";
/// How much of the source is read at a time.
pub const COPY_CHUNK_BYTES: usize = 64 * 1024;
/// How many fresh random ids staging tries before it gives up (a clash is
/// never expected: each id has 128 random bits).
const ID_ATTEMPTS: usize = 8;

/// `O_NONBLOCK`, for [`open_nonblocking`]. The standard library has no
/// constant for it, and `libc` isn't a dependency, so the value is recorded
/// here for the targets OpenWiz builds on: Darwin's, and Linux's generic
/// one (x86, x86_64, arm, aarch64, riscv64). Other Linux architectures
/// (MIPS, SPARC, Alpha, PA-RISC) use other values, so any other target
/// fails to build rather than guess.
const O_NONBLOCK: i32 = if cfg!(target_vendor = "apple") {
    0x0004
} else {
    0o4000
};

#[cfg(not(any(
    target_vendor = "apple",
    all(
        target_os = "linux",
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "riscv64"
        )
    )
)))]
compile_error!(
    "O_NONBLOCK's value isn't recorded for this target (crates/store/src/attachments.rs)"
);

/// The free space on the volume that holds `dir`, in bytes.
pub trait FreeSpace {
    /// The bytes free, or why they couldn't be measured. Staging never goes
    /// ahead without a measure.
    fn free_bytes(&self, dir: &Path) -> Result<u64, String>;
}

pub const DF_PROGRAM: &str = "/bin/df";

/// `df`'s arguments before the directory: POSIX output (`-P`), one line
/// per volume, in 1024-byte blocks (`-k`).
pub const DF_ARGS: [&str; 2] = ["-P", "-k"];

/// The real measure: `/bin/df -P -k <dir>`, run with `LC_ALL=C` and nothing
/// else in its environment, stdin closed, and its stderr discarded. It
/// fails closed: a program that can't be run, a non-zero exit, or output
/// [`parse_df`] doesn't take, measures nothing, so nothing is staged.
#[derive(Clone, Copy, Debug, Default)]
pub struct Df;

impl FreeSpace for Df {
    fn free_bytes(&self, dir: &Path) -> Result<u64, String> {
        let out = Command::new(DF_PROGRAM)
            .args(DF_ARGS)
            .arg(dir)
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("{DF_PROGRAM} could not be run: {e}"))?;
        if !out.status.success() {
            return Err(format!("{DF_PROGRAM} failed ({})", out.status));
        }
        parse_df(&out.stdout)
    }
}

/// The header `df -P -k` prints, word by word.
const DF_HEADER: [&str; 7] = [
    "Filesystem",
    "1024-blocks",
    "Used",
    "Available",
    "Capacity",
    "Mounted",
    "on",
];

/// The bytes available in `df -P -k`'s output, which must be exactly two
/// lines: [`DF_HEADER`] and the volume's row in POSIX's form, exactly six
/// fields: its name, size, used and available (whole numbers), capacity (a
/// whole percentage) and mount point (absolute), last. A name or a mount
/// point with spaces makes the fields ambiguous, so it is refused, as is
/// anything else: nothing is guessed.
pub fn parse_df(out: &[u8]) -> Result<u64, String> {
    let refused = || "df's output isn't the POSIX form".to_owned();
    let text = std::str::from_utf8(out).map_err(|_| refused())?;
    let lines: Vec<&str> = text
        .strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect();
    let [header, volume] = lines.as_slice() else {
        return Err(refused());
    };
    if header.split_whitespace().ne(DF_HEADER) {
        return Err(refused());
    }
    let fields: Vec<&str> = volume.split_whitespace().collect();
    let [_name, size, used, available, capacity, mount] = fields.as_slice() else {
        return Err(refused());
    };
    let number = |w: &str| !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit());
    let percent = capacity.strip_suffix('%').is_some_and(number);
    if !([size, used, available].iter().all(|w| number(w)) && percent && mount.starts_with('/')) {
        return Err(refused());
    }
    let blocks: u64 = available.parse().map_err(|_| refused())?;
    blocks.checked_mul(1024).ok_or_else(refused)
}

/// Which operation, if any, has claimed each attachment. B2
/// records the claims; until then nothing can claim one ([`NoClaims`]).
pub trait Claims {
    fn claim(&self, id: &AttachmentId) -> Result<Claim, StoreError>;
}

/// No send or new thread can claim an attachment before B2: every one is
/// unclaimed.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoClaims;

impl Claims for NoClaims {
    fn claim(&self, _id: &AttachmentId) -> Result<Claim, StoreError> {
        Ok(Claim::Unclaimed)
    }
}

/// One step of staging, in order. Each is done, and made durable, before
/// the next starts; only after [`Step::Ready`] is the attachment ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// The source is looked at, and found to be a regular file (a link
    /// followed); not opened yet. Nothing is written until [`Step::Made`].
    Looked,
    /// The attachment's directory is made, and its parent synced.
    Made,
    /// The first bytes are in the `.partial` copy.
    Writing,
    /// Every byte is copied: the size and the hash are known and checked,
    /// and the source is checked unchanged (its file, size, times and
    /// bytes).
    Copied,
    /// The copy is synced.
    Synced,
    /// The copy has its final name.
    Renamed,
    /// The copy is read-only (0400), and its directory synced.
    Sealed,
    /// The metadata is written to a temporary file and synced.
    MetaWritten,
    /// The metadata has its name, and the directory is synced: ready.
    Ready,
}

/// How staging makes each step durable, and who is told when one is done.
/// [`Fsync`] syncs for real; the tests record, look at the disk, and stop
/// the process at each step (AP4).
pub trait Durability {
    /// Syncs a file: the copy (for [`Step::Synced`]) or the metadata (for
    /// [`Step::MetaWritten`]).
    fn sync_file(&self, file: &File, _step: Step) -> io::Result<()> {
        file.sync_all()
    }

    /// Syncs a directory, so the entries in it are durable.
    fn sync_dir(&self, dir: &Path, _step: Step) -> io::Result<()> {
        crate::fsync_dir(dir)
    }

    /// `step` is done.
    fn done(&self, _step: Step) {}
}

/// Real syncs, and no one told.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fsync;

impl Durability for Fsync {}

/// One ready attachment, as the store holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadyAttachment {
    pub attachment: AttachmentRef,
    /// When it became ready.
    pub ready_at: Timestamp,
}

/// What staging one file came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Staged {
    Ready(ReadyAttachment),
    /// It can't become ready. Nothing of it is left in the store.
    Failed(FailReason),
}

/// What a sweep deleted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Swept {
    /// Unfinished copies, left by a staging that stopped.
    pub unfinished: usize,
    /// Ready attachments past their time-to-live.
    pub expired: Vec<ReadyAttachment>,
    /// Ready attachments whose operation's accepted outcome is recorded
    /// (none before B2).
    pub released: usize,
}

/// One attachment in the store, as [`Store::list`] finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listed {
    Ready {
        attachment: ReadyAttachment,
        claim: Claim,
    },
    /// Its metadata can't be read, or its copy isn't the size recorded.
    Broken {
        id: AttachmentId,
        reason: FailReason,
    },
}

/// What removing one attachment came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Its bytes and metadata are deleted (`None`: its metadata couldn't be
    /// read).
    Removed(Option<ReadyAttachment>),
    /// No ready or broken attachment has this id.
    NotFound,
    /// Its operation may still use it: nothing was deleted.
    Refused(Claim),
}

/// What verifying one attachment found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verified {
    /// Its copy has the size and SHA-256 recorded.
    Ready(ReadyAttachment),
    Failed(FailReason),
    NotFound,
}

impl Store<Attachments> {
    /// Copies the file at `source` into the store and verifies it: ready,
    /// or failed with the core's reason. `free` measures the state volume
    /// before anything is written.
    pub fn stage(
        &self,
        source: &Path,
        now: Timestamp,
        free: &dyn FreeSpace,
    ) -> Result<Staged, StoreError> {
        self.stage_with(source, now, free, &Fsync)
    }

    /// [`stage`](Self::stage), with every sync made through `durability`,
    /// which is told as each [`Step`] is done.
    ///
    /// The kind, the limits and the free-disk floor are checked before
    /// anything is written. A failure, or a store error, part way leaves
    /// nothing of the attachment (its directory is deleted; if even that
    /// fails, the next [`sweep`](Self::sweep) deletes it).
    pub fn stage_with(
        &self,
        source: &Path,
        now: Timestamp,
        free: &dyn FreeSpace,
        durability: &dyn Durability,
    ) -> Result<Staged, StoreError> {
        self.refuse_if_read_only()?;
        let display_name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = SafeName::from_original(&display_name);
        let looked = match look_at_source(source) {
            Ok(looked) => looked,
            Err(reason) => return Ok(Staged::Failed(reason)),
        };
        durability.done(Step::Looked);
        let (mut file, opened) = match open_source(source, looked) {
            Ok(opened) => opened,
            Err(reason) => return Ok(Staged::Failed(reason)),
        };
        let mut head = vec![0; SNIFF_BYTES];
        let got = match read_full(&mut file, &mut head) {
            Ok(got) => got,
            Err(_) => return Ok(Staged::Failed(FailReason::Unreadable)),
        };
        head.truncate(got);
        let kind = match check(&name, &head, opened.len) {
            Ok(kind) => kind,
            Err(reason) => return Ok(Staged::Failed(reason)),
        };
        // Held until staging ends: no sweep deletes this copy meanwhile.
        let _staging = self.staging_lock(true)?;
        let reservation = match self.reserve(opened.len, free, durability)? {
            Ok(reservation) => reservation,
            Err(reason) => return Ok(Staged::Failed(reason)),
        };
        let copy = Copy {
            id: &reservation.id,
            dir: &reservation.dir,
            name: &name,
            kind,
            len: opened.len,
        };
        let source = Source::File { file, opened };
        let staged = copy.run(source, &head, display_name, now, durability);
        reservation.end(&staged);
        staged
    }

    /// Stages an upload: `len` bytes read from `source`, under the
    /// owner's `display_name` (untrusted; the core makes the staged name),
    /// verified as [`stage`](Self::stage) verifies a file. The kind, the
    /// limits and the free-disk floor are checked on the first bytes and
    /// `len` before anything is written; never more than `len` bytes are
    /// read; a source that ends before `len`, or fails, is `failed
    /// (connection lost)`, and leaves nothing of the attachment.
    pub fn stage_stream(
        &self,
        display_name: &str,
        len: u64,
        source: &mut dyn Read,
        now: Timestamp,
        free: &dyn FreeSpace,
    ) -> Result<Staged, StoreError> {
        self.stage_stream_with(display_name, len, source, now, free, &Fsync)
    }

    /// [`stage_stream`](Self::stage_stream), with every sync made through
    /// `durability`, told as each [`Step`] from [`Step::Made`] on is done.
    pub fn stage_stream_with(
        &self,
        display_name: &str,
        len: u64,
        source: &mut dyn Read,
        now: Timestamp,
        free: &dyn FreeSpace,
        durability: &dyn Durability,
    ) -> Result<Staged, StoreError> {
        self.refuse_if_read_only()?;
        let name = SafeName::from_original(display_name);
        let mut source = source.take(len);
        let want = usize::try_from(len).map_or(SNIFF_BYTES, |l| l.min(SNIFF_BYTES));
        let mut head = vec![0; want];
        let got = match read_full(&mut source, &mut head) {
            Ok(got) if got == want => got,
            _ => return Ok(Staged::Failed(FailReason::ConnectionLost)),
        };
        head.truncate(got);
        let kind = match check(&name, &head, len) {
            Ok(kind) => kind,
            Err(reason) => return Ok(Staged::Failed(reason)),
        };
        // Held until staging ends: no sweep deletes this copy meanwhile.
        let _staging = self.staging_lock(true)?;
        let reservation = match self.reserve(len, free, durability)? {
            Ok(reservation) => reservation,
            Err(reason) => return Ok(Staged::Failed(reason)),
        };
        let copy = Copy {
            id: &reservation.id,
            dir: &reservation.dir,
            name: &name,
            kind,
            len,
        };
        let stream = Source::Stream(&mut source);
        let staged = copy.run(stream, &head, display_name.to_owned(), now, durability);
        reservation.end(&staged);
        staged
    }

    /// Measures the free space and reserves `len` bytes of it for a new
    /// attachment, as one step under the claims lock, so stagings at once,
    /// in this process or another, file or stream, never spend the same
    /// room twice: the room every other staging in
    /// progress still needs is counted as used, and the free-disk floor
    /// is checked against what is left. The new attachment's
    /// directory comes with its reservation held; or the reason it can't
    /// fit, with nothing made.
    fn reserve(
        &self,
        len: u64,
        free: &dyn FreeSpace,
        durability: &dyn Durability,
    ) -> Result<Result<Reservation, FailReason>, StoreError> {
        // The store's one exclusive lock for short steps: never held while
        // a copy is written, and a sweep takes it only after letting go of
        // the staging lock, so the two never wait on each other.
        let _claims = self.claims_lock()?;
        // What the other stagings still need, first; the free space after.
        // Copies run without this lock, so one
        // can move on, or finish, between the two looks: then its bytes are
        // counted twice (still needed, and already used), never not at all.
        // Measured the other way round, a copy moving on in between would be
        // counted neither as needed nor as used.
        let outstanding = self.outstanding()?;
        let free_bytes = free.free_bytes(&self.dir).map_err(|why| {
            StoreError(format!(
                "the free space on the attachment store's volume could not be measured ({why}); \
                 nothing was staged"
            ))
        })?;
        if let Err(reason) = staging_room(free_bytes.saturating_sub(outstanding), len) {
            return Ok(Err(reason));
        }
        let (id, dir) = self.make_dir(durability)?;
        let path = reserve_path(&dir);
        let held = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .and_then(|mut file| {
                file.try_lock().map_err(io::Error::from)?;
                file.write_all(len.to_string().as_bytes())?;
                file.sync_all()?;
                Ok(file)
            });
        match held {
            Ok(file) => Ok(Ok(Reservation { id, dir, file })),
            Err(e) => {
                let _ = fs::remove_dir_all(&dir);
                Err(failed("reserve room for", &id, &e))
            }
        }
    }

    /// The room the stagings in progress still need: for each unfinished
    /// attachment whose reservation a staging still holds, what it reserved
    /// less what it has written. A reservation no staging holds is a
    /// stopped one's: it needs nothing (a later sweep deletes it).
    fn outstanding(&self) -> Result<u64, StoreError> {
        let mut total = 0u64;
        for (_, dir) in self.entries()? {
            if fs::symlink_metadata(dir.join(META)).is_ok() {
                continue;
            }
            let path = reserve_path(&dir);
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_error("open", &path, &e)),
            };
            match file.try_lock() {
                // Nobody holds it: its staging stopped (this lock goes with
                // `file`).
                Ok(()) => continue,
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(e)) => return Err(io_error("lock", &path, &e)),
            }
            let mut text = String::new();
            file.read_to_string(&mut text)
                .map_err(|e| io_error("read", &path, &e))?;
            // Written whole under the claims lock, which this staging holds:
            // anything else fails closed.
            let reserved: u64 = text.parse().map_err(|_| {
                StoreError("a staging's reservation can't be read; nothing was staged".to_owned())
            })?;
            let mut written = 0u64;
            for entry in fs::read_dir(&dir).map_err(|e| io_error("list", &dir, &e))? {
                let entry = entry.map_err(|e| io_error("list", &dir, &e))?;
                if entry.path() == path {
                    continue;
                }
                if let Ok(meta) = entry.metadata()
                    && meta.is_file()
                {
                    written = written.saturating_add(meta.len());
                }
            }
            total = total.saturating_add(reserved.saturating_sub(written));
        }
        Ok(total)
    }

    /// Deletes unfinished copies (unless a staging is in progress), and the
    /// ready attachments the core's retention rule says go.
    pub fn sweep(&self, now: Timestamp, claims: &dyn Claims) -> Result<Swept, StoreError> {
        self.refuse_if_read_only()?;
        let mut swept = Swept::default();
        let lock = self.staging_lock(false)?;
        match lock.try_lock() {
            Ok(()) => {
                // No staging is in progress: a copy without metadata is one
                // that stopped.
                for (id, dir) in self.entries()? {
                    let meta = fs::symlink_metadata(dir.join(META));
                    if meta.is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
                        self.delete(&id)?;
                        swept.unfinished += 1;
                    }
                }
            }
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) => {
                return Err(io_error("lock", &self.dir.join(STAGING_LOCK), &e));
            }
        }
        drop(lock);
        // No claim can be made or settled while the retention rule is
        // applied (B2).
        let _claims = self.claims_lock()?;
        for listed in self.list(claims)? {
            let Listed::Ready { attachment, claim } = listed else {
                continue;
            };
            let age = age(now, attachment.ready_at);
            match retention(Stored::Ready { claim, age }) {
                Retention::Keep | Retention::Delete(DeleteReason::Partial) => {}
                Retention::Delete(DeleteReason::Expired) => {
                    self.delete(&attachment.attachment.id)?;
                    swept.expired.push(attachment);
                }
                Retention::Delete(DeleteReason::Accepted) => {
                    self.delete(&attachment.attachment.id)?;
                    swept.released += 1;
                }
            }
        }
        Ok(swept)
    }

    /// Every ready or broken attachment: the ready ones oldest first, then
    /// the broken ones. Reads only. A copy still being staged, or one that
    /// stopped, isn't listed.
    pub fn list(&self, claims: &dyn Claims) -> Result<Vec<Listed>, StoreError> {
        let (mut ready, mut broken) = (Vec::new(), Vec::new());
        for (id, dir) in self.entries()? {
            let attachment = match read_meta(&dir, &id) {
                Meta::Missing => continue,
                Meta::Unreadable => {
                    broken.push(Listed::Broken {
                        id,
                        reason: FailReason::Unreadable,
                    });
                    continue;
                }
                Meta::Ready(attachment) => attachment,
            };
            let copy = fs::symlink_metadata(dir.join(attachment.attachment.name.as_str()));
            if !copy.is_ok_and(|m| m.is_file() && m.len() == attachment.attachment.size_bytes) {
                broken.push(Listed::Broken {
                    id,
                    reason: FailReason::SizeMismatch,
                });
                continue;
            }
            let claim = claims.claim(&id)?;
            ready.push(Listed::Ready { attachment, claim });
        }
        let key = |listed: &Listed| match listed {
            Listed::Ready { attachment, .. } => {
                (attachment.ready_at, attachment.attachment.id.clone())
            }
            Listed::Broken { id, .. } => (Timestamp(i64::MAX), id.clone()),
        };
        ready.sort_by_key(key);
        broken.sort_by_key(key);
        ready.extend(broken);
        Ok(ready)
    }

    /// Removes one attachment, unless its operation may still use it.
    pub fn remove(&self, id: &AttachmentId, claims: &dyn Claims) -> Result<Removal, StoreError> {
        self.refuse_if_read_only()?;
        let dir = self.dir.join(id.as_str());
        if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            return Ok(Removal::NotFound);
        }
        let attachment = match read_meta(&dir, id) {
            // Unfinished: never ready, and the sweep's to delete.
            Meta::Missing => return Ok(Removal::NotFound),
            Meta::Unreadable => None,
            Meta::Ready(attachment) => Some(attachment),
        };
        // No claim can be made between final validation and deletion (B2).
        let _claims = self.claims_lock()?;
        if let Err(RemoveRefused(claim)) = can_remove(claims.claim(id)?) {
            return Ok(Removal::Refused(claim));
        }
        self.delete(id)?;
        Ok(Removal::Removed(attachment))
    }

    /// Reads one attachment's copy again, whole, and checks its size and
    /// SHA-256 against its metadata. Reads only.
    pub fn verify(&self, id: &AttachmentId) -> Result<Verified, StoreError> {
        let dir = self.dir.join(id.as_str());
        if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            return Ok(Verified::NotFound);
        }
        let attachment = match read_meta(&dir, id) {
            Meta::Missing => return Ok(Verified::NotFound),
            Meta::Unreadable => return Ok(Verified::Failed(FailReason::Unreadable)),
            Meta::Ready(attachment) => attachment,
        };
        let path = dir.join(attachment.attachment.name.as_str());
        // The copy itself, never a link in its place: the file opened must
        // be the one looked at.
        let Ok(looked) = fs::symlink_metadata(&path) else {
            return Ok(Verified::Failed(FailReason::SizeMismatch));
        };
        let Ok(mut file) = open_nonblocking(&path) else {
            return Ok(Verified::Failed(FailReason::Unreadable));
        };
        let Ok(opened) = file.metadata() else {
            return Ok(Verified::Failed(FailReason::Unreadable));
        };
        if !looked.is_file() || (looked.dev(), looked.ino()) != (opened.dev(), opened.ino()) {
            return Ok(Verified::Failed(FailReason::SizeMismatch));
        }
        let mut hasher = Sha256Hasher::new();
        let mut size = 0u64;
        let mut buf = vec![0; COPY_CHUNK_BYTES];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    size += n as u64;
                    hasher.update(&buf[..n]);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Ok(Verified::Failed(FailReason::Unreadable)),
            }
        }
        let expected = &attachment.attachment;
        if size != expected.size_bytes || hasher.finish() != expected.sha256.as_str() {
            return Ok(Verified::Failed(FailReason::SizeMismatch));
        }
        Ok(Verified::Ready(attachment))
    }

    /// Opens (creating it, 0600) and locks, exclusively, the claims lock
    /// (B2): held until the returned file is dropped.
    fn claims_lock(&self) -> Result<File, StoreError> {
        let path = self.dir.join(CLAIMS_LOCK);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|e| io_error("open", &path, &e))?;
        lock.lock().map_err(|e| io_error("lock", &path, &e))?;
        Ok(lock)
    }

    /// The claims recorded beside these attachments, each judged by its
    /// operation's record, as of `now`.
    pub fn recorded_claims(&self, now: Timestamp) -> RecordedClaims {
        RecordedClaims {
            dir: self.dir.clone(),
            now,
        }
    }

    /// Writes `op` as the claim of attachment `id`, atomically, in place of
    /// any claim before it.
    fn write_claim(&self, id: &AttachmentId, op: &OpRef) -> Result<(), StoreError> {
        let dir = self.dir.join(id.as_str());
        let mut body = serde_json::to_vec_pretty(op)
            .map_err(|_| StoreError("a claim could not be serialized".to_owned()))?;
        body.push(b'\n');
        let temp = dir.join(format!("{CLAIM}.tmp"));
        let _ = fs::remove_file(&temp);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| failed("write the claim of", id, &e))?;
        file.write_all(&body)
            .and_then(|()| file.sync_all())
            .map_err(|e| failed("write the claim of", id, &e))?;
        drop(file);
        fs::rename(&temp, dir.join(CLAIM)).map_err(|e| failed("name the claim of", id, &e))?;
        crate::fsync_dir(&dir).map_err(|e| failed("sync the directory of", id, &e))
    }

    /// Opens (creating it, 0600) the staging lock: locked shared for a
    /// staging (waiting out a sweep), or not locked yet, for a sweep.
    fn staging_lock(&self, shared: bool) -> Result<File, StoreError> {
        let path = self.dir.join(STAGING_LOCK);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|e| io_error("open", &path, &e))?;
        if shared {
            lock.lock_shared()
                .map_err(|e| io_error("lock", &path, &e))?;
        }
        Ok(lock)
    }

    /// Makes a new attachment's directory (0700) under a fresh id, and
    /// syncs `attachments/`.
    fn make_dir(&self, durability: &dyn Durability) -> Result<(AttachmentId, PathBuf), StoreError> {
        for _ in 0..ID_ATTEMPTS {
            let id = new_id()?;
            let dir = self.dir.join(id.as_str());
            match DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => {
                    durability
                        .sync_dir(&self.dir, Step::Made)
                        .map_err(|e| io_error("sync", &self.dir, &e))?;
                    durability.done(Step::Made);
                    return Ok((id, dir));
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(failed("make the directory of", &id, &e)),
            }
        }
        Err(StoreError(
            "no unused attachment id could be made".to_owned(),
        ))
    }

    /// The directories in `attachments/` named as attachment ids: real
    /// directories only, never a link. Anything else is left alone.
    fn entries(&self) -> Result<Vec<(AttachmentId, PathBuf)>, StoreError> {
        let read = fs::read_dir(&self.dir).map_err(|e| io_error("list", &self.dir, &e))?;
        let mut found = Vec::new();
        for entry in read {
            let entry = entry.map_err(|e| io_error("list", &self.dir, &e))?;
            let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|n| AttachmentId::parse(n).ok())
            else {
                continue;
            };
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                found.push((id, entry.path()));
            }
        }
        Ok(found)
    }

    /// Deletes one attachment's directory, and syncs `attachments/`.
    fn delete(&self, id: &AttachmentId) -> Result<(), StoreError> {
        match fs::remove_dir_all(self.dir.join(id.as_str())) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(failed("remove", id, &e)),
        }
        crate::fsync_dir(&self.dir).map_err(|e| io_error("sync", &self.dir, &e))
    }
}

/// The claims recorded beside the attachments (B2): which operation claims
/// each one, from its `.claim.json`, and where that claim stands, read from
/// the operation's own record, never written here:
///
/// - no record (the operation was withdrawn: nothing was read or sent for
/// it), or no claim file: unclaimed;
/// - claimed or dispatching by a live process: dispatching;
/// - a record whose process stopped: as recovery would finish it (claimed
/// becomes failed, dispatching uncertain), read only;
/// - finished: queued or sent, or created, is accepted; failed or refused
/// is failed; uncertain is uncertain.
///
/// The send and new-thread records are opened for reading only, at each
/// look, so a record kind made after this was built is still seen.
#[derive(Clone, Debug)]
pub struct RecordedClaims {
    /// `attachments/`.
    dir: PathBuf,
    now: Timestamp,
}

impl RecordedClaims {
    /// The claims recorded in the attachment store under `state_dir`, as of
    /// `now`. Reads nothing until asked.
    pub fn new(state_dir: &Path, now: Timestamp) -> Self {
        Self {
            dir: state_dir.join(STAGING_DIR),
            now,
        }
    }

    /// The operation that claims `id`, and where its claim stands; `None`
    /// when no operation does.
    pub fn of(&self, id: &AttachmentId) -> Result<Option<(OpRef, Claim)>, StoreError> {
        let path = self.dir.join(id.as_str()).join(CLAIM);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(failed("read the claim of", id, &e)),
        };
        let op = read_claim(&text).ok_or_else(|| {
            StoreError(format!(
                "the claim of attachment {} is not exactly a claim; nothing was claimed or \
                 deleted",
                id.as_str()
            ))
        })?;
        let claim = self.state_of(&op)?;
        Ok((claim != Claim::Unclaimed).then_some((op, claim)))
    }

    /// Where `op`'s claim stands, from its record.
    fn state_of(&self, op: &OpRef) -> Result<Claim, StoreError> {
        let state_dir = self
            .dir
            .parent()
            .ok_or_else(|| StoreError("the attachment store has no state directory".to_owned()))?;
        Ok(match op.kind {
            OpKind::Send => {
                let record = match FileStore::open_read_only(state_dir)? {
                    Some(store) => store.inspect(&op.op_id, self.now)?,
                    None => None,
                };
                match record {
                    None => Claim::Unclaimed,
                    Some(r) => match (r.state, &r.delivery) {
                        (RecordState::Finished, Some(Delivery::Queued { .. }))
                        | (RecordState::Finished, Some(Delivery::Accepted { .. })) => {
                            Claim::Accepted
                        }
                        (RecordState::Finished, Some(Delivery::Failed { .. })) => Claim::Failed,
                        (RecordState::Finished, _) => Claim::Uncertain,
                        (RecordState::Claimed | RecordState::Dispatching, _) => Claim::Dispatching,
                    },
                }
            }
            OpKind::NewThread => {
                let record = match NewThreadFileStore::open_read_only(state_dir)? {
                    Some(store) => store.inspect(&op.op_id, self.now)?,
                    None => None,
                };
                match record {
                    None => Claim::Unclaimed,
                    Some(r) => match (r.state, &r.outcome) {
                        (NewThreadState::Finished, Some(Outcome::Created { .. })) => {
                            Claim::Accepted
                        }
                        (
                            NewThreadState::Finished,
                            Some(Outcome::Refused { .. } | Outcome::Failed { .. }),
                        ) => Claim::Failed,
                        (NewThreadState::Finished, _) => Claim::Uncertain,
                        (NewThreadState::Claimed | NewThreadState::Dispatching, _) => {
                            Claim::Dispatching
                        }
                    },
                }
            }
        })
    }
}

/// A claim file's operation, read by the core's strict reader; `None` when
/// it isn't exactly a claim.
fn read_claim(text: &str) -> Option<OpRef> {
    let tree: StrictJson = serde_json::from_str(text).ok()?;
    op_ref(&tree).ok()
}

impl Claims for RecordedClaims {
    fn claim(&self, id: &AttachmentId) -> Result<Claim, StoreError> {
        Ok(self.of(id)?.map_or(Claim::Unclaimed, |(_, claim)| claim))
    }
}

/// The staged attachments, for the core's gate and its two operations (B2).
impl StagedAttachments for Store<Attachments> {
    fn look_up(&self, id: &AttachmentId, now: Timestamp) -> Result<Lookup, StoreError> {
        let ready = match self.verify(id)? {
            Verified::Ready(ready) => ready,
            Verified::NotFound => return Ok(Lookup::NotFound),
            Verified::Failed(reason) => return Ok(Lookup::Failed(reason)),
        };
        let path = StagedPath::new(&self.dir, id, &ready.attachment.name).map_err(|e| {
            StoreError(format!(
                "the attachment store's path can't be a staged path ({e}); nothing was sent"
            ))
        })?;
        Ok(Lookup::Ready {
            staged: StagedAttachment {
                attachment: (&ready.attachment).into(),
                path,
            },
            ready_at: ready.ready_at,
            claim: self.recorded_claims(now).of(id)?,
        })
    }

    fn claim(
        &self,
        op: &OpRef,
        ids: &[AttachmentId],
        now: Timestamp,
    ) -> Result<ClaimOutcome, StoreError> {
        if ids.is_empty() {
            return Ok(ClaimOutcome::Claimed { new: vec![] });
        }
        self.refuse_if_read_only()?;
        let _lock = self.claims_lock()?;
        let claims = self.recorded_claims(now);
        let (mut problems, mut new) = (Vec::new(), Vec::new());
        for id in ids {
            let dir = self.dir.join(id.as_str());
            let problem = match read_meta(&dir, id) {
                Meta::Missing => Some(Problem::NotFound),
                Meta::Unreadable => Some(Problem::Failed(FailReason::Unreadable)),
                Meta::Ready(ready) => {
                    let copy = fs::symlink_metadata(dir.join(ready.attachment.name.as_str()));
                    if !copy.is_ok_and(|m| m.is_file() && m.len() == ready.attachment.size_bytes) {
                        Some(Problem::Failed(FailReason::SizeMismatch))
                    } else {
                        let current = claims.of(id)?;
                        match claimable(op, current.as_ref(), age(now, ready.ready_at)) {
                            Ok(()) => {
                                if current.is_none_or(|(owner, _)| owner != *op) {
                                    new.push(id.clone());
                                }
                                None
                            }
                            Err(problem) => Some(problem),
                        }
                    }
                }
            };
            if let Some(problem) = problem {
                problems.push(AttachmentProblem {
                    id: id.clone(),
                    problem,
                });
            }
        }
        if !problems.is_empty() {
            return Ok(ClaimOutcome::Refused(Refusal { problems }));
        }
        for id in &new {
            self.write_claim(id, op)?;
        }
        Ok(ClaimOutcome::Claimed { new })
    }

    fn unclaim(&self, op: &OpRef, ids: &[AttachmentId]) -> Result<(), StoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        self.refuse_if_read_only()?;
        let _lock = self.claims_lock()?;
        for id in ids {
            let dir = self.dir.join(id.as_str());
            let path = dir.join(CLAIM);
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if read_claim(&text).is_some_and(|owner| owner == *op) {
                fs::remove_file(&path).map_err(|e| failed("remove the claim of", id, &e))?;
                crate::fsync_dir(&dir).map_err(|e| failed("sync the directory of", id, &e))?;
            }
        }
        Ok(())
    }

    fn settle(&self, op: &OpRef, ids: &[AttachmentId], now: Timestamp) -> Result<(), StoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        self.refuse_if_read_only()?;
        let _lock = self.claims_lock()?;
        let claims = self.recorded_claims(now);
        for id in ids {
            let Meta::Ready(ready) = read_meta(&self.dir.join(id.as_str()), id) else {
                continue;
            };
            let Some((owner, claim)) = claims.of(id)? else {
                continue;
            };
            if owner != *op {
                continue;
            }
            let stored = Stored::Ready {
                claim,
                age: age(now, ready.ready_at),
            };
            match retention(stored) {
                Retention::Keep | Retention::Delete(DeleteReason::Partial) => {}
                Retention::Delete(DeleteReason::Accepted | DeleteReason::Expired) => {
                    self.delete(id)?;
                }
            }
        }
        Ok(())
    }
}

/// A staging's reservation, in its attachment's directory.
fn reserve_path(dir: &Path) -> PathBuf {
    dir.join(".reserve")
}

/// One staging's reserved room: its directory, and the reservation file it
/// holds locked until the staging ends.
struct Reservation {
    id: AttachmentId,
    dir: PathBuf,
    /// Locked while held; the lock goes when this is dropped (or the
    /// process stops).
    #[allow(dead_code)]
    file: File,
}

impl Reservation {
    /// The staging ended: a ready attachment keeps its directory, without
    /// the reservation; anything else leaves nothing. The lock goes with
    /// `self`.
    fn end(self, staged: &Result<Staged, StoreError>) {
        if matches!(staged, Ok(Staged::Ready(_))) {
            let _ = fs::remove_file(reserve_path(&self.dir));
        } else {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

/// One staging's copy, from its directory onwards.
struct Copy<'a> {
    id: &'a AttachmentId,
    dir: &'a Path,
    name: &'a SafeName,
    kind: Kind,
    /// How many bytes the source holds: a file's size as opened, or an
    /// upload's declared length.
    len: u64,
}

/// What a staging copies from.
enum Source<'s> {
    /// A file, as it was opened: after its copy it is checked unchanged
    /// against `opened`. A read error is `unreadable`; a
    /// size that changes, `size_mismatch`.
    File { file: File, opened: Fingerprint },
    /// An upload's body, read once and never past its length: a read
    /// error, or an end before its length, is the connection lost.
    Stream(&'s mut dyn Read),
}

impl Source<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::File { file, .. } => file.read(buf),
            Self::Stream(stream) => stream.read(buf),
        }
    }

    /// Why a read failed, or the source didn't hold its length.
    fn lost(&self, short: bool) -> FailReason {
        match (self, short) {
            (Self::Stream(_), _) => FailReason::ConnectionLost,
            (Self::File { .. }, true) => FailReason::SizeMismatch,
            (Self::File { .. }, false) => FailReason::Unreadable,
        }
    }
}

impl Copy<'_> {
    /// The steps after [`Step::Made`]: copy, sync, rename, seal, then the
    /// metadata. Ready only at the end.
    fn run(
        &self,
        mut source: Source<'_>,
        head: &[u8],
        display_name: String,
        now: Timestamp,
        durability: &dyn Durability,
    ) -> Result<Staged, StoreError> {
        let partial_path = self
            .dir
            .join(format!("{}{PARTIAL_SUFFIX}", self.name.as_str()));
        let mut partial = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&partial_path)
            .map_err(|e| failed("make the copy of", self.id, &e))?;
        let mut hasher = Sha256Hasher::new();
        let mut copied = 0u64;
        let mut piece = head.to_vec();
        piece.reserve(COPY_CHUNK_BYTES.saturating_sub(piece.len()));
        let mut first = true;
        loop {
            if !piece.is_empty() {
                copied += piece.len() as u64;
                if copied > self.len {
                    // The source grew while it was copied (a stream is
                    // never read past its length).
                    return Ok(Staged::Failed(FailReason::SizeMismatch));
                }
                if let Err(e) = partial.write_all(&piece) {
                    return self.write_failed(&e);
                }
                hasher.update(&piece);
            }
            if first {
                durability.done(Step::Writing);
                first = false;
            }
            piece.resize(COPY_CHUNK_BYTES, 0);
            match source.read(&mut piece) {
                Ok(0) => break,
                Ok(n) => piece.truncate(n),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => piece.clear(),
                Err(_) => return Ok(Staged::Failed(source.lost(false))),
            }
        }
        if copied != self.len {
            return Ok(Staged::Failed(source.lost(true)));
        }
        let sha256 = hasher.finish();

        if let Source::File { file, opened } = &mut source {
            match unchanged(file, opened, &sha256) {
                Ok(true) => {}
                Ok(false) => return Ok(Staged::Failed(FailReason::SizeMismatch)),
                Err(_) => return Ok(Staged::Failed(FailReason::Unreadable)),
            }
        }
        durability.done(Step::Copied);
        durability
            .sync_file(&partial, Step::Synced)
            .map_err(|e| failed("sync the copy of", self.id, &e))?;
        drop(partial);
        durability.done(Step::Synced);

        let copy_path = self.dir.join(self.name.as_str());
        fs::rename(&partial_path, &copy_path)
            .map_err(|e| failed("name the copy of", self.id, &e))?;
        durability.done(Step::Renamed);
        fs::set_permissions(&copy_path, fs::Permissions::from_mode(0o400))
            .map_err(|e| failed("make read-only the copy of", self.id, &e))?;
        durability
            .sync_dir(self.dir, Step::Sealed)
            .map_err(|e| failed("sync the directory of", self.id, &e))?;
        durability.done(Step::Sealed);

        let sha256 = Sha256::parse(&sha256).map_err(|e| StoreError(e.0))?;
        let ready = ReadyAttachment {
            attachment: AttachmentRef {
                id: self.id.clone(),
                sha256,
                size_bytes: copied,
                kind: self.kind,
                name: self.name.clone(),
                display_name,
            },
            ready_at: now,
        };
        let mut body =
            serde_json::to_vec_pretty(&(&ready.attachment, ready.ready_at)).map_err(|_| {
                StoreError("an attachment's metadata could not be serialized".to_owned())
            })?;
        body.push(b'\n');
        let temp = self.dir.join(format!("{META}.tmp"));
        let mut meta = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| failed("write the metadata of", self.id, &e))?;
        if let Err(e) = meta.write_all(&body) {
            return self.write_failed(&e);
        }
        durability
            .sync_file(&meta, Step::MetaWritten)
            .map_err(|e| failed("sync the metadata of", self.id, &e))?;
        drop(meta);
        durability.done(Step::MetaWritten);
        fs::rename(&temp, self.dir.join(META))
            .map_err(|e| failed("name the metadata of", self.id, &e))?;
        durability
            .sync_dir(self.dir, Step::Ready)
            .map_err(|e| failed("sync the directory of", self.id, &e))?;
        durability.done(Step::Ready);
        Ok(Staged::Ready(ready))
    }

    /// A write that failed: the disk is full, or the store can't be used.
    fn write_failed(&self, e: &io::Error) -> Result<Staged, StoreError> {
        if matches!(
            e.kind(),
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded
        ) {
            Ok(Staged::Failed(FailReason::LowDisk))
        } else {
            Err(failed("write the copy of", self.id, e))
        }
    }
}

/// A store error about one attachment: named by its id, never by its file
/// name (AP12).
fn failed(what: &str, id: &AttachmentId, e: &io::Error) -> StoreError {
    StoreError(format!(
        "could not {what} attachment {id}: {e}",
        id = id.as_str()
    ))
}

/// A regular file is staged; a folder (or a link to one) is `folder`, and
/// anything else (a FIFO, a device, a socket, nothing) is `unreadable`.
fn regular(meta: &fs::Metadata) -> Result<(), FailReason> {
    if meta.is_dir() {
        Err(FailReason::Folder)
    } else if meta.is_file() {
        Ok(())
    } else {
        Err(FailReason::Unreadable)
    }
}

/// What is known of the source: the file it is (device and inode) and,
/// from its open handle, its size and times. Staging refuses the source if
/// any of them changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl Fingerprint {
    fn of(meta: &fs::Metadata) -> Self {
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
            len: meta.len(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
            ctime: (meta.ctime(), meta.ctime_nsec()),
        }
    }

    fn same_file(&self, other: &Self) -> bool {
        (self.dev, self.ino) == (other.dev, other.ino)
    }
}

/// The source, looked at before it is opened (a link followed), so a FIFO
/// or a device that is there now is never opened. What it found, to compare
/// with what is opened.
fn look_at_source(source: &Path) -> Result<Fingerprint, FailReason> {
    let meta = fs::metadata(source).map_err(|_| FailReason::Unreadable)?;
    regular(&meta)?;
    Ok(Fingerprint::of(&meta))
}

fn open_source(source: &Path, looked: Fingerprint) -> Result<(File, Fingerprint), FailReason> {
    let file = open_nonblocking(source).map_err(|_| FailReason::Unreadable)?;
    let meta = file.metadata().map_err(|_| FailReason::Unreadable)?;
    regular(&meta)?;
    let opened = Fingerprint::of(&meta);
    if !opened.same_file(&looked) {
        return Err(FailReason::SizeMismatch);
    }
    Ok((file, opened))
}

/// Whether the open source is still as it was opened, after its copy
/// hashed to `copied`: the same file, size and times,
/// then the same bytes, read again from the start through the same handle,
/// then the same file, size and times again. The bytes are read again
/// because a file's times can miss a rewrite within one tick of the clock.
fn unchanged(source: &mut File, opened: &Fingerprint, copied: &str) -> io::Result<bool> {
    if Fingerprint::of(&source.metadata()?) != *opened {
        return Ok(false);
    }
    source.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256Hasher::new();
    let mut size = 0u64;
    let mut buf = vec![0; COPY_CHUNK_BYTES];
    loop {
        match source.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                size += n as u64;
                hasher.update(&buf[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(size == opened.len
        && hasher.finish() == copied
        && Fingerprint::of(&source.metadata()?) == *opened)
}

/// Opens `path` for reading only, with `O_NONBLOCK`: a FIFO without a
/// writer opens at once (to be refused) instead of blocking. A regular
/// file's reads ignore the flag.
fn open_nonblocking(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)
}

/// Reads until `buf` is full or the source ends; how much was read.
fn read_full(file: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match file.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(got)
}

/// What one attachment's metadata file holds.
enum Meta {
    /// None: the copy is unfinished.
    Missing,
    /// Not exactly the record, or not this directory's.
    Unreadable,
    Ready(ReadyAttachment),
}

/// Reads `.meta.json`: exactly a two-element array, the attachment's
/// verified record (read by the core's strict reader: exactly its six keys,
/// each once, every value checked) and the time it became ready, in Unix
/// milliseconds. The record's id must be its directory's.
fn read_meta(dir: &Path, id: &AttachmentId) -> Meta {
    let text = match fs::read_to_string(dir.join(META)) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Meta::Missing,
        Err(_) => return Meta::Unreadable,
    };
    match serde_json::from_str::<(AttachmentRef, Timestamp)>(&text) {
        Ok((attachment, ready_at)) if attachment.id == *id => Meta::Ready(ReadyAttachment {
            attachment,
            ready_at,
        }),
        _ => Meta::Unreadable,
    }
}

/// A new attachment id: `att-` and 32 random hex digits.
fn new_id() -> Result<AttachmentId, StoreError> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| StoreError(format!("the random source could not be read: {e}")))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    AttachmentId::parse(&format!("att-{hex}")).map_err(|e| StoreError(e.0))
}

/// How long ago `at` was, never below zero.
fn age(now: Timestamp, at: Timestamp) -> Duration {
    Duration::from_millis(u64::try_from(now.0.saturating_sub(at.0)).unwrap_or(0))
}

/// The reports of `openwiz attachment add, ls, rm` (B1 phase 2), and of the
/// web server's attachment routes, which answer with the same JSON:
/// **one implementation for both clients**, in the shapes of
/// `contracts/attachment.v1.schema.json`. The rules a report applies are
/// here once: when an attachment expires, the words for a broken copy, and
/// which removals were expirations. Each client prints a report its own way:
/// the CLI as text or `--json`, the server as its answer's body.
pub mod report {
    use std::collections::HashSet;
    use std::ffi::OsString;
    use std::path::Path;

    use openwiz_core::attachment::{
        AttachmentId, AttachmentRef, Claim, FailReason, Readiness, UNSENT_TTL,
    };
    use openwiz_core::messaging::record::StoreError;
    use openwiz_core::model::Timestamp;
    use serde::Serialize;

    use super::{AttachmentStore, Claims, FreeSpace, Listed, ReadyAttachment, Removal, Staged};

    /// The schema ids of the three reports.
    pub const ADD_SCHEMA_ID: &str = "openwiz.attachment-add.v1";
    pub const LIST_SCHEMA_ID: &str = "openwiz.attachment-list.v1";
    pub const REMOVE_SCHEMA_ID: &str = "openwiz.attachment-remove.v1";

    /// One attachment in the store, as the reports show it.
    #[derive(Clone, Debug, Serialize)]
    pub struct StagedView {
        pub attachment: AttachmentRef,
        pub readiness: Readiness,
        pub ready_at: i64,
        pub age_ms: i64,
        /// When it expires; `None` while its operation may still use it.
        pub expires_at: Option<i64>,
    }

    impl StagedView {
        pub fn new(
            ready: &ReadyAttachment,
            readiness: Readiness,
            claim: Claim,
            now: Timestamp,
        ) -> Self {
            let ttl_ms = i64::try_from(UNSENT_TTL.as_millis()).unwrap_or(i64::MAX);
            let expires_at = matches!(claim, Claim::Unclaimed | Claim::Failed)
                .then(|| ready.ready_at.0.saturating_add(ttl_ms));
            Self {
                attachment: ready.attachment.clone(),
                readiness,
                ready_at: ready.ready_at.0,
                age_ms: now.0.saturating_sub(ready.ready_at.0).max(0),
                expires_at,
            }
        }
    }

    /// Sweeps the store as each command opens it (unfinished copies go;
    /// attachments past their time-to-live, go too) and reports what
    /// expired.
    pub fn sweep(
        store: &AttachmentStore,
        now: Timestamp,
        claims: &dyn Claims,
    ) -> Result<Vec<StagedView>, StoreError> {
        let swept = store.sweep(now, claims)?;
        Ok(swept
            .expired
            .iter()
            .map(|ready| StagedView::new(ready, Readiness::Expired, Claim::Unclaimed, now))
            .collect())
    }

    /// `openwiz attachment add`'s report (`openwiz.attachment-add.v1`): every
    /// source, each in exactly one of `ready`, `failed` and `not_staged`, by
    /// its index; and what this run's open found past its time-to-live.
    #[derive(Clone, Debug, Serialize)]
    pub struct AddReport {
        pub schema: &'static str,
        pub ready: Vec<AddReady>,
        pub failed: Vec<AddFailed>,
        pub not_staged: Vec<NotStaged>,
        /// What this run's open found past its time-to-live, and deleted.
        pub expired: Vec<StagedView>,
    }

    #[derive(Clone, Debug, Serialize)]
    pub struct AddReady {
        /// The source's place in the arguments, from 0.
        pub index: usize,
        pub path: String,
        pub staged: StagedView,
    }

    #[derive(Clone, Debug, Serialize)]
    pub struct AddFailed {
        pub index: usize,
        pub path: String,
        pub readiness: Readiness,
        pub message: String,
    }

    /// A source the store failed on: nothing of it was kept.
    #[derive(Clone, Debug, Serialize)]
    pub struct NotStaged {
        pub index: usize,
        pub path: String,
        pub error: String,
    }

    impl AddReport {
        /// No source yet, and nothing expired.
        pub fn new() -> Self {
            Self {
                schema: ADD_SCHEMA_ID,
                ready: vec![],
                failed: vec![],
                not_staged: vec![],
                expired: vec![],
            }
        }

        /// One source's staging, at `index`, shown as `path`: ready, failed
        /// with the core's reason and words, or not staged (the store's
        /// error).
        pub fn push(
            &mut self,
            index: usize,
            path: String,
            staged: Result<Staged, StoreError>,
            now: Timestamp,
        ) {
            match staged {
                Ok(Staged::Ready(ready)) => self.ready.push(AddReady {
                    index,
                    path,
                    staged: StagedView::new(&ready, Readiness::Ready, Claim::Unclaimed, now),
                }),
                Ok(Staged::Failed(reason)) => self.failed.push(AddFailed {
                    index,
                    path,
                    readiness: Readiness::Failed { reason },
                    message: reason.message().to_owned(),
                }),
                Err(StoreError(why)) => self.not_staged.push(NotStaged {
                    index,
                    path,
                    error: why,
                }),
            }
        }
    }

    impl Default for AddReport {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Stages each of `paths` into `store`, in order, and reports every one:
    /// `attachment add`'s work, shared with `send` and `new`'s `--attach`.
    pub fn stage_paths(
        store: &AttachmentStore,
        paths: &[OsString],
        now: Timestamp,
        free_space: &dyn FreeSpace,
    ) -> AddReport {
        let mut report = AddReport::new();
        for (index, path) in paths.iter().enumerate() {
            let shown = path.to_string_lossy().into_owned();
            report.push(
                index,
                shown,
                store.stage(Path::new(path), now, free_space),
                now,
            );
        }
        report
    }

    /// `openwiz attachment ls`'s report (`openwiz.attachment-list.v1`).
    #[derive(Clone, Debug, Serialize)]
    pub struct ListReport {
        pub schema: &'static str,
        /// The ready attachments, oldest first.
        pub attachments: Vec<StagedView>,
        pub broken: Vec<Broken>,
        pub expired: Vec<StagedView>,
    }

    /// An attachment whose metadata can't be read, or whose copy isn't the
    /// size recorded: not ready.
    #[derive(Clone, Debug, Serialize)]
    pub struct Broken {
        pub id: AttachmentId,
        pub readiness: Readiness,
        pub message: String,
    }

    impl ListReport {
        /// Nothing staged, and nothing expired.
        pub fn new() -> Self {
            Self {
                schema: LIST_SCHEMA_ID,
                attachments: vec![],
                broken: vec![],
                expired: vec![],
            }
        }
    }

    impl Default for ListReport {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Every ready or broken attachment in `store`, with `expired`, what
    /// this run's open swept. Reads only.
    pub fn list(
        store: &AttachmentStore,
        claims: &dyn Claims,
        now: Timestamp,
        expired: Vec<StagedView>,
    ) -> Result<ListReport, StoreError> {
        let mut report = ListReport::new();
        report.expired = expired;
        for listed in store.list(claims)? {
            match listed {
                Listed::Ready { attachment, claim } => report.attachments.push(StagedView::new(
                    &attachment,
                    Readiness::Ready,
                    claim,
                    now,
                )),
                Listed::Broken { id, reason } => report.broken.push(Broken {
                    id,
                    readiness: Readiness::Failed { reason },
                    message: broken_message(reason).to_owned(),
                }),
            }
        }
        Ok(report)
    }

    fn broken_message(reason: FailReason) -> &'static str {
        match reason {
            FailReason::SizeMismatch => {
                "its copy isn't the size recorded, so it isn't ready; attachment rm deletes it"
            }
            _ => "its metadata can't be read, so it isn't ready; attachment rm deletes it",
        }
    }

    /// What `rm` did with one id.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    pub enum RemoveResult {
        Removed,
        /// This run's open found it past its time-to-live, and deleted it.
        Expired,
        Refused,
        NotFound,
        /// The store failed on it.
        NotRemoved,
    }

    #[derive(Clone, Debug, Serialize)]
    pub struct RemoveEntry {
        pub index: usize,
        pub id: AttachmentId,
        pub result: RemoveResult,
        pub message: String,
    }

    /// `openwiz attachment rm`'s report (`openwiz.attachment-remove.v1`).
    #[derive(Clone, Debug, Serialize)]
    pub struct RemoveReport {
        pub schema: &'static str,
        pub ids: Vec<RemoveEntry>,
        pub expired: Vec<StagedView>,
    }

    /// Removes each of `ids` from `store`, in order, unless its operation may
    /// still use it; `expired` is what this run's open swept (an id among
    /// them was already deleted, as expired).
    pub fn remove(
        store: &AttachmentStore,
        ids: &[AttachmentId],
        claims: &dyn Claims,
        expired: Vec<StagedView>,
    ) -> RemoveReport {
        let gone: HashSet<&AttachmentId> = expired.iter().map(|e| &e.attachment.id).collect();
        let mut results = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            let (result, message) = if gone.contains(id) {
                (
                    RemoveResult::Expired,
                    "it was past its time-to-live (24 hours): its copy and its metadata were \
                     deleted"
                        .to_owned(),
                )
            } else {
                match store.remove(id, claims) {
                    Ok(Removal::Removed(_)) => (
                        RemoveResult::Removed,
                        "its copy and its metadata are deleted".to_owned(),
                    ),
                    Ok(Removal::NotFound) => (
                        RemoveResult::NotFound,
                        "no staged attachment has this id".to_owned(),
                    ),
                    Ok(Removal::Refused(claim)) => (
                        RemoveResult::Refused,
                        format!(
                            "its operation is {}, and may still use it, so nothing was removed",
                            claim_word(claim)
                        ),
                    ),
                    Err(StoreError(why)) => (RemoveResult::NotRemoved, why),
                }
            };
            results.push(RemoveEntry {
                index,
                id: id.clone(),
                result,
                message,
            });
        }
        RemoveReport {
            schema: REMOVE_SCHEMA_ID,
            ids: results,
            expired,
        }
    }

    fn claim_word(claim: Claim) -> &'static str {
        match claim {
            Claim::Unclaimed => "unclaimed",
            Claim::Dispatching => "dispatching",
            Claim::Accepted => "accepted",
            Claim::Failed => "failed",
            Claim::Uncertain => "uncertain",
        }
    }
}
