//! The new-thread record, the interface of its store, and the recovery rule.
//!
//! One record per op id, as for `send`. It binds the op id to its project,
//! harness, model, effort, text hash and attachments digest, keeps the
//! prompt text until the outcome is **Created**, and moves through three
//! states: **claimed** (durably, atomically across processes, before any
//! read), **dispatching** (durably, immediately before the one call) and
//! **finished**.
//!
//! Recovery never redispatches: a record left claimed becomes **failed
//! (interrupted before dispatch)**, and one left dispatching becomes
//! **uncertain**.
//!
//! The core defines the interface only. `openwiz-store` implements it
//! (A1 phase 2: the `new_thread` record kind, read back strictly with
//! [`read`](super::read)); phase 1's tests use an in-memory fake.

use serde::Serialize;

use crate::attachment::PromptAttachment;
use crate::envelope::SourceInfo;
use crate::messaging::OpId;
use crate::messaging::record::StoreError;
use crate::messaging::send::UncertainReason;
use crate::model::Timestamp;

use super::effects::Effects;
use super::outcome::{FailReason, Gap, Outcome};
use super::{Binding, ProjectView, RECORD_SCHEMA_ID, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordState {
    Claimed,
    Dispatching,
    Finished,
}

/// One create, as stored. See `$defs/record` in
/// `contracts/new-thread.v1.schema.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NewThreadRecord {
    /// Always [`RECORD_SCHEMA_ID`].
    pub schema: String,
    pub op_id: OpId,
    pub binding: Binding,
    /// The request id passed to the provider: derived from the op id.
    pub request_id: String,
    /// The requested configuration, as the owner chose it.
    pub requested: Settings,
    /// The `spawn` provider the options map the harness to, once checked.
    pub provider: Option<String>,
    /// The prompt text, kept until the outcome is Created.
    pub text: Option<String>,
    pub text_bytes: u64,
    /// The attachments the first prompt carries, in the owner's order (B2):
    /// ids, hashes, sizes, kinds and staged names, never bytes. **Absent
    /// means none**, so every A1 record keeps its shape; the reader checks
    /// it against the binding's digest.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<PromptAttachment>,
    pub state: RecordState,
    /// The project as resolved for this create.
    pub project: Option<ProjectView>,
    /// The provider and the version read for this create.
    pub source: Option<SourceInfo>,
    /// The effects worked out before the call.
    pub effects: Option<Effects>,
    pub outcome: Option<Outcome>,
    pub gaps: Vec<Gap>,
    /// True when recovery set the outcome.
    pub recovered: bool,
    pub claimed_at: Timestamp,
    pub dispatching_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

impl NewThreadRecord {
    /// A new record in the claimed state, holding the text.
    pub fn claimed(
        op_id: OpId,
        binding: Binding,
        requested: Settings,
        text: &str,
        now: Timestamp,
    ) -> Self {
        Self {
            schema: RECORD_SCHEMA_ID.to_owned(),
            request_id: super::request_id(&op_id),
            op_id,
            binding,
            requested,
            provider: None,
            text: Some(text.to_owned()),
            text_bytes: text.len() as u64,
            attachments: vec![],
            state: RecordState::Claimed,
            project: None,
            source: None,
            effects: None,
            outcome: None,
            gaps: vec![],
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

    /// Marks the record dispatching. The store must sync it before the call.
    pub fn dispatching(&self, now: Timestamp) -> Self {
        Self {
            state: RecordState::Dispatching,
            dispatching_at: Some(now),
            ..self.clone()
        }
    }

    /// Finishes the record with `outcome`. The text is erased only when the
    /// thread was created.
    pub fn finished(&self, outcome: Outcome, now: Timestamp) -> Self {
        let created = matches!(outcome, Outcome::Created { .. });
        Self {
            text: if created { None } else { self.text.clone() },
            state: RecordState::Finished,
            outcome: Some(outcome),
            finished_at: Some(now),
            ..self.clone()
        }
    }
}

/// The recovery rule, for a record whose process stopped before finishing
/// it. A finished record is returned unchanged. Nothing is redispatched.
pub fn recover(record: &NewThreadRecord, now: Timestamp) -> NewThreadRecord {
    let outcome = match record.state {
        RecordState::Finished => return record.clone(),
        RecordState::Claimed => Outcome::failed(FailReason::InterruptedBeforeDispatch),
        RecordState::Dispatching => {
            Outcome::uncertain(UncertainReason::InterruptedAfterDispatchMark)
        }
    };
    NewThreadRecord {
        recovered: true,
        ..record.finished(outcome, now)
    }
}

/// The result of claiming an op id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Claim {
    /// This process now owns the op id; its claimed record is written and
    /// synced.
    Owned,
    /// The op id already has a finished record (possibly just recovered).
    Existing(NewThreadRecord),
    /// Another live process owns the op id.
    InProgress(NewThreadRecord),
    /// Another live process owns the op id and has not written its record
    /// yet.
    Pending,
}

/// The durable, per-owner store of new-thread records. The core never
/// touches storage itself.
///
/// Required of an implementation, as of `send`'s store:
/// - [`claim`](Self::claim) is atomic across processes: for a new op id
///   exactly one caller gets [`Claim::Owned`];
/// - every write is synced before the call returns;
/// - a record whose owner stopped is recovered with [`recover`] before it is
///   returned, never redispatched;
/// - records are readable by the owner only.
pub trait NewThreadStore: Send + Sync {
    /// Claims `record.op_id` with `record` (in the claimed state). `now` is
    /// the time of any recovery.
    fn claim(&self, record: &NewThreadRecord, now: Timestamp) -> Result<Claim, StoreError>;

    /// Writes the owner's new version of a claimed record, synced.
    fn update(&self, record: &NewThreadRecord) -> Result<(), StoreError>;

    /// Gives up this process's claim on `op_id`.
    fn release(&self, op_id: &OpId);

    /// The record of `op_id`, if one exists. **Never writes** and never
    /// claims: a record whose owner stopped is returned as [`recover`] would
    /// finish it, in memory only; a live owner's record as it is.
    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<NewThreadRecord>, StoreError>;

    /// Gives up this process's claim on `op_id` and removes its record,
    /// which must still be in the claimed state: nothing was read or sent
    /// for it (B2: its attachments could not be claimed). The op id stays
    /// unused.
    fn withdraw(&self, op_id: &OpId) -> Result<(), StoreError>;
}
