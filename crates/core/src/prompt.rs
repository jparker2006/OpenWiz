//! The prompt payload that `send` and `new_thread` share (OpenWiz 0.0.4),
//! and the one gate both pass before anything is claimed or called (B2,
//! the implementation).
//!
//! `PromptPayload { text, attachments }`. The text follows `send`'s rules
//! ([`validate_text`]: not empty, no NUL, at most
//! [`MAX_TEXT_BYTES`](crate::messaging::MAX_TEXT_BYTES)). `attachments` is
//! an ordered list of opaque [`AttachmentId`]s, each at most once: B1's
//! staged attachments. No count cap, no total.
//!
//! **One gate** ([`gate`]): before the op id is claimed, every attachment
//! must be ready, its copy read again whole and matching its record, not
//! expired, and not in use by another operation. If any is not, nothing is
//! claimed, read or sent, and the refusal names each one. There is no
//! partial send: an attachment is never dropped to let the rest go.
//!
//! **The binding**: an op id binds the text's SHA-256 and
//! [`attachments_digest`], over the owner's ordered list. **The dispatch
//! order** is files first, then images ([`ReadyPrompt::dispatch_order`]),
//! the order the agent receives them. See
//! `contracts/prompt.v1.schema.json`.

use std::fmt;
use std::time::Duration;

use serde::Serialize;

use crate::attachment::{
    Claim, DeleteReason, FailReason, Kind, PromptAttachment, Retention, StagedAttachment, Stored,
    retention,
};
use crate::messaging::record::StoreError;
use crate::messaging::{InvalidRequest, OpId, sha256_hex, validate_text};
use crate::model::Timestamp;

/// B1's attachment id, reused, never copied: the one opaque,
/// path-safe handle on a staged attachment.
pub use crate::attachment::AttachmentId;

/// The schema id of a prompt payload. See `contracts/prompt.v1.schema.json`.
pub const PROMPT_SCHEMA_ID: &str = "openwiz.prompt.v1";

/// One prompt: its text and its attachments, in the owner's order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptPayload {
    pub text: String,
    pub attachments: Vec<AttachmentId>,
}

impl PromptPayload {
    /// A text-only payload.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: vec![],
        }
    }

    /// The text follows [`validate_text`], and no attachment is listed
    /// twice.
    pub fn validate(&self) -> Result<(), InvalidRequest> {
        validate_text(&self.text)?;
        for (i, id) in self.attachments.iter().enumerate() {
            if self.attachments[..i].contains(id) {
                return Err(InvalidRequest(format!(
                    "attachment {} is listed twice; nothing was claimed, read or sent",
                    id.as_str()
                )));
            }
        }
        Ok(())
    }

    /// Lowercase hex SHA-256 of the text's UTF-8 bytes, as `send` binds it.
    pub fn text_sha256(&self) -> String {
        sha256_hex(self.text.as_bytes())
    }
}

/// The header of the empty list's encoding: the digest every 0.0.3 send
/// record (none) and 0.0.4 record carries.
const EMPTY_HEADER: &str = "openwiz.prompt.attachments.v1\n";

/// The header of a non-empty list's encoding (B2). It differs from
/// [`EMPTY_HEADER`], so no list hashes the same under both encodings.
const ATTACHMENTS_HEADER: &str = "openwiz.prompt.attachments.v2\n";

/// The SHA-256 of the canonical, ordered attachment list. The empty list
/// has a fixed value, [`empty_attachments_digest`]: the header
/// `openwiz.prompt.attachments.v1` alone, as every earlier record has it.
/// A non-empty list is the header `openwiz.prompt.attachments.v2`, then one
/// line per attachment, in the owner's order: `<sha256> <size_bytes> <kind>
/// <safe name>`. Each field is ASCII with no space or line break, so the
/// encoding is unambiguous. The id is not hashed: two staged copies of the
/// same bytes, kind and name are the same payload.
pub fn attachments_digest(attachments: &[PromptAttachment]) -> String {
    if attachments.is_empty() {
        return empty_attachments_digest();
    }
    let mut canonical = String::from(ATTACHMENTS_HEADER);
    for a in attachments {
        canonical.push_str(&format!(
            "{} {} {} {}\n",
            a.sha256.as_str(),
            a.size_bytes,
            kind_label(a.kind),
            a.name.as_str()
        ));
    }
    sha256_hex(canonical.as_bytes())
}

/// The digest every 0.0.4 record carries: that of no attachments.
pub fn empty_attachments_digest() -> String {
    sha256_hex(EMPTY_HEADER.as_bytes())
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => "image",
        Kind::File => "file",
    }
}

/// A prompt whose every attachment passed the [`gate`]: the text and each
/// attachment with its staged copy, in the owner's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadyPrompt {
    pub text: String,
    pub attachments: Vec<StagedAttachment>,
}

impl ReadyPrompt {
    /// A text-only prompt.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: vec![],
        }
    }

    /// The attachments as the op id binds them, in the owner's order.
    pub fn bound(&self) -> Vec<PromptAttachment> {
        self.attachments
            .iter()
            .map(|a| a.attachment.clone())
            .collect()
    }

    /// [`attachments_digest`] of this prompt's attachments.
    pub fn attachments_digest(&self) -> String {
        attachments_digest(&self.bound())
    }

    /// The order `room-cli` is given them, and the agent receives them:
    /// files first, then images, each in the owner's order (F13).
    pub fn dispatch_order(&self) -> Vec<StagedAttachment> {
        let files = self
            .attachments
            .iter()
            .filter(|a| a.attachment.kind == Kind::File);
        let images = self
            .attachments
            .iter()
            .filter(|a| a.attachment.kind == Kind::Image);
        files.chain(images).cloned().collect()
    }

    /// The bytes of every attachment together.
    pub fn total_bytes(&self) -> u64 {
        self.attachments
            .iter()
            .map(|a| a.attachment.size_bytes)
            .fold(0, u64::saturating_add)
    }
}

/// How much the call that carries attachments may take beyond
/// [`READ_TIMEOUT`](crate::limits::READ_TIMEOUT), and its operation beyond
/// its budget: one second per started MiB of attachments, a deliberately
/// slow floor of 1 MiB/s for `room-cli`'s upload to the local CloudRoom
/// server. Measured against the fake process; this is not a measured live
/// throughput guarantee. A timed-out call is uncertain, never retried.
pub fn attachment_allowance(total_bytes: u64) -> Duration {
    Duration::from_secs(total_bytes.div_ceil(ALLOWANCE_BYTES_PER_SECOND))
}

/// The upload rate [`attachment_allowance`] assumes: 1 MiB per second.
pub const ALLOWANCE_BYTES_PER_SECOND: u64 = 1024 * 1024;

/// Which operation claims an attachment: a send (the core's `messaging`)
/// or a new thread, each with its own op ids. In JSON, the module's name:
/// `messaging` or `new_thread`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum OpKind {
    #[serde(rename = "messaging")]
    Send,
    #[serde(rename = "new_thread")]
    NewThread,
}

/// One operation, by kind and op id: what a claim file holds, read back
/// only strictly (`new_thread::read::op_ref`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct OpRef {
    pub kind: OpKind,
    pub op_id: OpId,
}

impl fmt::Display for OpRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            OpKind::Send => write!(f, "send op {}", self.op_id),
            OpKind::NewThread => write!(f, "new thread op {}", self.op_id),
        }
    }
}

/// What the store knows of one attachment now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// Ready: its copy was read again whole, and matches its record. With
    /// when it became ready, and the operation that claims it, if any, with
    /// where that operation stands.
    Ready {
        staged: StagedAttachment,
        ready_at: Timestamp,
        claim: Option<(OpRef, Claim)>,
    },
    /// No ready attachment has this id: never staged, removed, or expired
    /// and deleted.
    NotFound,
    /// Its metadata can't be read, or its copy isn't what was recorded.
    Failed(FailReason),
}

/// Why one attachment can't go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// No ready attachment has this id.
    NotFound,
    /// Its copy is broken, or isn't what was recorded (changed bytes).
    Failed(FailReason),
    /// Unclaimed (or its operation failed), past its time-to-live.
    Expired,
    /// Another operation claims it, and may still use it, or has.
    InUse { op: OpRef, claim: Claim },
}

impl Problem {
    /// The problem, in the core's words.
    pub fn words(&self) -> String {
        match self {
            Self::NotFound => {
                "not found: it was never staged here, or it was removed or expired".to_owned()
            }
            Self::Failed(reason) => format!("not ready: {}", reason.message()),
            Self::Expired => "expired: it was ready for more than 24 hours unsent".to_owned(),
            Self::InUse { op, claim } => {
                let state = match claim {
                    Claim::Dispatching => "is being sent",
                    Claim::Accepted => "has been sent",
                    Claim::Uncertain => "is uncertain",
                    Claim::Failed | Claim::Unclaimed => "uses it",
                };
                format!("in use: {op} {state}; stage the file again for a new operation")
            }
        }
    }
}

/// One attachment that can't go, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentProblem {
    pub id: AttachmentId,
    pub problem: Problem,
}

/// The gate's refusal: every attachment that can't go, in the owner's
/// order. Nothing was claimed, read or sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub problems: Vec<AttachmentProblem>,
}

impl Refusal {
    /// The refusal, naming each attachment, in the core's words.
    pub fn words(&self) -> String {
        let each: Vec<String> = self
            .problems
            .iter()
            .map(|p| format!("attachment {} is {}", p.id.as_str(), p.problem.words()))
            .collect();
        format!(
            "{}; nothing was sent, and the text and the other attachments are kept as they were",
            each.join("; ")
        )
    }
}

/// Why the gate stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateError {
    /// An attachment can't go. Nothing was claimed, read or sent.
    Refused(Refusal),
    /// The attachment store could not be used. Nothing was sent.
    Store(StoreError),
}

/// What claiming an operation's attachments came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// Every attachment is claimed by the operation. `new` lists those this
    /// call claimed (the others it held already).
    Claimed { new: Vec<AttachmentId> },
    /// At least one can't be claimed: none was.
    Refused(Refusal),
}

/// The staged attachments, as the gate and the operations see them.
/// `openwiz-store` implements it; the core never touches storage.
///
/// Required of an implementation:
/// - [`look_up`](Self::look_up) reads the copy again whole, and writes
/// nothing;
/// - [`claim`](Self::claim) is atomic across processes and all or nothing,
/// and judges each attachment by [`claimable`];
/// - nothing deletes an attachment whose claim [`retention`] keeps.
pub trait StagedAttachments: Send + Sync {
    /// One attachment, its copy read again whole. Never writes.
    fn look_up(&self, id: &AttachmentId, now: Timestamp) -> Result<Lookup, StoreError>;

    /// Claims every one of `ids` for `op`, or none. Called once `op` owns
    /// its op id, before the dispatching mark.
    fn claim(
        &self,
        op: &OpRef,
        ids: &[AttachmentId],
        now: Timestamp,
    ) -> Result<ClaimOutcome, StoreError>;

    /// Gives up `op`'s claim on each of `ids` (the operation is withdrawn:
    /// nothing was read or sent for it).
    fn unclaim(&self, op: &OpRef, ids: &[AttachmentId]) -> Result<(), StoreError>;

    /// Once `op`'s outcome is recorded: applies [`retention`] to each of
    /// `ids` that `op` claims, so an accepted operation's copies are
    /// deleted, and every other one is kept.
    fn settle(&self, op: &OpRef, ids: &[AttachmentId], now: Timestamp) -> Result<(), StoreError>;
}

/// No staged attachment at all: every id is not found, so any attachment
/// is refused. For a caller with no attachment store (the server, until
/// B3); it can never claim or delete anything.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoAttachments;

impl StagedAttachments for NoAttachments {
    fn look_up(&self, _id: &AttachmentId, _now: Timestamp) -> Result<Lookup, StoreError> {
        Ok(Lookup::NotFound)
    }

    fn claim(
        &self,
        _op: &OpRef,
        ids: &[AttachmentId],
        _now: Timestamp,
    ) -> Result<ClaimOutcome, StoreError> {
        if ids.is_empty() {
            return Ok(ClaimOutcome::Claimed { new: vec![] });
        }
        Ok(ClaimOutcome::Refused(Refusal {
            problems: ids
                .iter()
                .map(|id| AttachmentProblem {
                    id: id.clone(),
                    problem: Problem::NotFound,
                })
                .collect(),
        }))
    }

    fn unclaim(&self, _op: &OpRef, _ids: &[AttachmentId]) -> Result<(), StoreError> {
        Ok(())
    }

    fn settle(
        &self,
        _op: &OpRef,
        _ids: &[AttachmentId],
        _now: Timestamp,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Whether `op` may claim an attachment that became ready `age` ago and is
/// claimed now as `current`: the one rule the gate and the store's claim
/// both apply.
///
/// - claimed by `op` itself: yes (the same operation, again);
/// - unclaimed, or claimed by an operation that failed: yes, unless past
/// its time-to-live, when it is expired;
/// - claimed by another operation that is being sent, was accepted, or is
/// uncertain: no, it is in use (kept for that operation).
pub fn claimable(
    op: &OpRef,
    current: Option<&(OpRef, Claim)>,
    age: Duration,
) -> Result<(), Problem> {
    let claim = match current {
        Some((owner, _)) if owner == op => return Ok(()),
        Some((owner, claim @ (Claim::Dispatching | Claim::Accepted | Claim::Uncertain))) => {
            return Err(Problem::InUse {
                op: owner.clone(),
                claim: *claim,
            });
        }
        Some((_, claim)) => *claim,
        None => Claim::Unclaimed,
    };
    match retention(Stored::Ready { claim, age }) {
        Retention::Delete(DeleteReason::Expired) => Err(Problem::Expired),
        _ => Ok(()),
    }
}

/// How long ago `at` was, never below zero.
pub fn age(now: Timestamp, at: Timestamp) -> Duration {
    Duration::from_millis(now.0.saturating_sub(at.0).max(0).unsigned_abs())
}

/// **The one gate** for a send and a new thread: every attachment of
/// `payload` must be ready (its copy read again whole and matching its
/// record) and [`claimable`] by `op`. All of them, or nothing: the refusal
/// names every attachment that can't go, and nothing is claimed.
pub fn gate(
    attachments: &dyn StagedAttachments,
    payload: &PromptPayload,
    op: &OpRef,
    now: Timestamp,
) -> Result<ReadyPrompt, GateError> {
    let mut staged = Vec::with_capacity(payload.attachments.len());
    let mut problems = Vec::new();
    for id in &payload.attachments {
        let problem = match attachments.look_up(id, now).map_err(GateError::Store)? {
            Lookup::NotFound => Problem::NotFound,
            Lookup::Failed(reason) => Problem::Failed(reason),
            Lookup::Ready {
                staged: ready,
                ready_at,
                claim,
            } => {
                // The store's record must be this id's: never another copy.
                if ready.attachment.id != *id {
                    Problem::Failed(FailReason::Unreadable)
                } else {
                    match claimable(op, claim.as_ref(), age(now, ready_at)) {
                        Ok(()) => {
                            staged.push(ready);
                            continue;
                        }
                        Err(problem) => problem,
                    }
                }
            }
        };
        problems.push(AttachmentProblem {
            id: id.clone(),
            problem,
        });
    }
    if !problems.is_empty() {
        return Err(GateError::Refused(Refusal { problems }));
    }
    Ok(ReadyPrompt {
        text: payload.text.clone(),
        attachments: staged,
    })
}

/// The attachments `ids` stand for, for comparing a request with its op
/// id's record (the binding): each one as the record holds it when the
/// record has that id (its copy may be gone: an accepted operation's copies
/// are deleted), otherwise as its staged copy is now, read again whole.
/// `None` when an id is neither in the record nor a ready copy: then it
/// can't be shown to be the recorded payload. Claims nothing, sends nothing.
pub fn recorded(
    attachments: &dyn StagedAttachments,
    ids: &[AttachmentId],
    record: &[PromptAttachment],
    now: Timestamp,
) -> Result<Option<Vec<PromptAttachment>>, StoreError> {
    let mut found = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(kept) = record.iter().find(|a| a.id == *id) {
            found.push(kept.clone());
            continue;
        }
        match attachments.look_up(id, now)? {
            Lookup::Ready { staged, .. } if staged.attachment.id == *id => {
                found.push(staged.attachment);
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(found))
}

/// The words a queued, sent or created outcome adds when it carried
/// attachments: never a stronger claim.
pub fn handed_over(count: usize) -> String {
    let noun = if count == 1 {
        "attachment"
    } else {
        "attachments"
    };
    format!(", with {count} {noun} handed to CloudRoom. {NOT_REPORTED}")
}

/// What OpenWiz never claims about attachments, said once under every
/// accepted outcome that carried them.
pub const NOT_REPORTED: &str = "Delivery to the agent's machine is not reported";
