//! The send record, the interface of its store, and the recovery rule.
//!
//! One record per op id. It binds the op id to its target, mode and text
//! hash, keeps the text only while the outcome is uncertain or failed, and
//! moves through three states:
//!
//! 1. **claimed**: written durably, atomically across processes, before any
//!    read of the target;
//! 2. **dispatching**: written durably immediately before the one provider
//!    call;
//! 3. **finished**: the outcome.
//!
//! Recovery never redispatches. A record left `claimed` by a process that
//! stopped becomes **failed (interrupted before dispatch)**: nothing was
//! sent, because the dispatching mark is synced before any call. A record
//! left `dispatching` becomes **uncertain**.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::attachment::PromptAttachment;
use crate::envelope::SourceInfo;
use crate::model::{Fact, Timestamp};

use super::effects::Effects;
use super::reconcile::Observation;
use super::send::{Delivery, FailReason, SendGap, UncertainReason};
use super::{Binding, OpId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordState {
    Claimed,
    Dispatching,
    Finished,
}

/// One send, as stored. See `$defs/record` in
/// `contracts/messaging.v1.schema.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRecord {
    /// Always [`RECORD_SCHEMA_ID`](super::RECORD_SCHEMA_ID).
    pub schema: String,
    pub op_id: OpId,
    pub binding: Binding,
    /// The text, kept only while the outcome is not yet known, uncertain or
    /// failed. Erased (`None`) once the delivery is queued or accepted.
    pub text: Option<String>,
    pub text_bytes: u64,
    /// The attachments the send carries, in the owner's order (B2): their
    /// ids, hashes, sizes, kinds and staged names, never their bytes.
    /// **Absent means none**: written only when there are attachments, so
    /// every 0.0.3 record keeps its shape. Read strictly (each entry has
    /// exactly its five keys), and the store checks it against the
    /// binding's digest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<PromptAttachment>,
    pub state: RecordState,
    /// The target's title as read for this send.
    pub target_title: Option<Fact<String>>,

    #[serde(default)]
    pub source: Option<SourceInfo>,
    /// The effects shown before sending, once worked out.
    pub effects: Option<Effects>,
    /// The reported delivery, once finished.
    pub delivery: Option<Delivery>,
    /// Gaps found during the send, in OpenWiz's own words.
    pub gaps: Vec<SendGap>,
    /// Reconciliation observations. Always empty under 0.43.1: no
    /// reconciliation read is allowed.
    pub observations: Vec<Observation>,
    /// True when the outcome was set by recovery, not by the send itself.
    pub recovered: bool,
    pub claimed_at: Timestamp,
    pub dispatching_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

impl SendRecord {
    /// A new record in the claimed state, holding the text.
    pub fn claimed(op_id: OpId, binding: Binding, text: &str, now: Timestamp) -> Self {
        Self {
            schema: super::RECORD_SCHEMA_ID.to_owned(),
            op_id,
            binding,
            text: Some(text.to_owned()),
            text_bytes: text.len() as u64,
            attachments: vec![],
            state: RecordState::Claimed,
            target_title: None,
            source: None,
            effects: None,
            delivery: None,
            gaps: vec![],
            observations: vec![],
            recovered: false,
            claimed_at: now,
            dispatching_at: None,
            finished_at: None,
        }
    }

    /// The same record, carrying `attachments` (in the owner's order). The
    /// binding must already hold their digest.
    pub fn with_attachments(self, attachments: Vec<PromptAttachment>) -> Self {
        Self {
            attachments,
            ..self
        }
    }

    /// Whether the attachments agree with the binding's digest.
    pub fn attachments_agree(&self) -> bool {
        crate::prompt::attachments_digest(&self.attachments) == self.binding.attachments_digest
    }

    /// Marks the record dispatching. The store must sync it before the call.
    pub fn dispatching(&self, now: Timestamp) -> Self {
        Self {
            state: RecordState::Dispatching,
            dispatching_at: Some(now),
            ..self.clone()
        }
    }

    /// Finishes the record with `delivery`. The text is erased when the
    /// delivery is queued or accepted, and kept otherwise.
    pub fn finished(&self, delivery: Delivery, now: Timestamp) -> Self {
        let delivered = matches!(
            delivery,
            Delivery::Queued { .. } | Delivery::Accepted { .. }
        );
        Self {
            text: if delivered { None } else { self.text.clone() },
            state: RecordState::Finished,
            delivery: Some(delivery),
            finished_at: Some(now),
            ..self.clone()
        }
    }
}

/// The recovery rule, for a record whose process stopped before finishing
/// it: claimed becomes failed (interrupted before dispatch), dispatching
/// becomes uncertain. A finished record is returned unchanged. Nothing is
/// ever redispatched.
pub fn recover(record: &SendRecord, now: Timestamp) -> SendRecord {
    let delivery = match record.state {
        RecordState::Finished => return record.clone(),
        RecordState::Claimed => Delivery::failed(FailReason::InterruptedBeforeDispatch),
        RecordState::Dispatching => {
            Delivery::uncertain(UncertainReason::InterruptedAfterDispatchMark)
        }
    };
    SendRecord {
        recovered: true,
        ..record.finished(delivery, now)
    }
}

/// The result of claiming an op id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Claim {
    /// This process now owns the op id. Its claimed record is written and
    /// synced. The store holds the claim until [`SendStore::release`] or the
    /// process ends.
    Owned,
    /// The op id already has a finished record: possibly one the store has
    /// just recovered with [`recover`].
    Existing(SendRecord),
    /// Another live process owns the op id.
    InProgress(SendRecord),
    /// Another live process owns the op id and has not written its record
    /// yet.
    Pending,
}

/// The send record could not be read or written. Nothing more is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreError(pub String);

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The durable, per-owner store of send records. `openwiz-store` implements
/// it with files; the core never touches storage itself.
///
/// Required of an implementation:
/// - [`claim`](Self::claim) is atomic across processes: for a new op id
///   exactly one caller gets [`Claim::Owned`];
/// - every write is synced before the call returns;
/// - a record whose owner stopped is recovered with [`recover`] before it is
///   returned, never redispatched;
/// - records are readable by the owner only.
pub trait SendStore: Send + Sync {
    /// The record of `op_id`, if one exists. Read only: no claim, no
    /// recovery.
    fn load(&self, op_id: &OpId) -> Result<Option<SendRecord>, StoreError>;

    /// Claims `record.op_id` with `record` (in the claimed state). `now` is
    /// the time of any recovery.
    fn claim(&self, record: &SendRecord, now: Timestamp) -> Result<Claim, StoreError>;

    /// Writes the owner's new version of a claimed record, synced.
    fn update(&self, record: &SendRecord) -> Result<(), StoreError>;

    /// Gives up this process's claim on `op_id`.
    fn release(&self, op_id: &OpId);

    /// Finished records, other than `except`, whose delivery is uncertain
    /// and whose binding has this target and text hash (any mode). Records
    /// left by stopped processes are recovered first.
    fn uncertain_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError>;

    /// Records, other than `except`, still claimed or dispatching by a live
    /// owner, whose binding equals `binding` (target, mode and text hash).
    /// Records left by stopped processes are recovered first, so they are
    /// never in flight; a record gone under its lock (withdrawn) is skipped.
    ///
    /// No default: the guard fails closed, so every store must answer.
    fn in_flight_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError>;

    /// The record of `op_id`, if one exists, for a lookup. **Never writes**
    /// and never claims it: a record whose owner stopped is returned as
    /// [`recover`] would finish it, in memory only (the next send of that op
    /// id records the recovery); a live owner's record is returned as it is;
    /// a record gone under its lock (withdrawn) is `None`.
    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<SendRecord>, StoreError>;

    /// Gives up this process's claim on `op_id` and removes its record,
    /// which must still be in the claimed state: nothing was read or sent
    /// for it. Used when a possible duplicate is found after the claim, so
    /// the op id stays unused, as when one is found before.
    fn withdraw(&self, op_id: &OpId) -> Result<(), StoreError>;
}
