//! The `new_thread` operation: one new Cloud thread in one existing
//! project, one provider call per op id, and an honest report.
//!
//! In order:
//! 1. **validate** the prompt (text rules; any attachment is refused), the
//!    project reference and the three settings; nothing is defaulted;
//! 2. **claim** the op id durably and atomically. The same op id with the
//!    same binding returns the recorded outcome, or "in progress"; with a
//!    different binding it is refused as conflicting reuse. Neither calls
//!    the provider;
//! 3. read and report the provider's **version** ;
//! 4. **resolve the project** from the project list, in the same operation;
//! 5. **read the options** in the same operation. A harness, model or
//!    effort they don't offer is refused, with what is offered; nothing is
//!    substituted;
//! 6. work out the **effects**, and derive the **request id** from the op
//!    id;
//! 7. mark the record **dispatching**, durably;
//! 8. make **one call**. No retry;
//! 9. map the reply to an **outcome** and record it. The prompt text is kept
//!    until the outcome is Created.
//!
//! [`preview_new_thread`] runs steps 1 and 3 to 6 with the same reads and
//! rules, and claims nothing and calls nothing: the effects shown before
//! anything is created.
//!
//! **Attachments (B2, the implementation).** Before the op id is claimed, the one
//! [`gate`] (shared with `send`) reads every attachment's copy again and
//! refuses unless every one can go; the op id, and so the request id
//! derived from it, binds them too, so one request id never carries two
//! payloads (CloudRoom's own dedup compares the text only, F17). Once the
//! op id is owned, the attachments are claimed for it, all or none, before
//! any read; the one call carries every one; and once the outcome is
//! recorded, a created thread's copies are deleted (the check).

use std::time::Instant;

use serde::Serialize;

use crate::attachment::PromptAttachment;
use crate::envelope::SourceInfo;
use crate::messaging::record::StoreError;
use crate::messaging::send::read_failure;
use crate::messaging::{InvalidRequest, OpId};
use crate::model::{Fact, Timestamp};
use crate::ops::{Clock, version_fact};
use crate::prompt::{
    ClaimOutcome, GateError, NoAttachments, OpKind, OpRef, ReadyPrompt, StagedAttachments,
    attachment_allowance, attachments_digest, gate, recorded,
};
use crate::source::SourceError;

use super::effects::{Effects, ProjectCandidate, effects_for, resolve_project};
use super::options::{Catalogue, NotOfferedField, cloud_options};
use super::outcome::{FailReason, Gap, Outcome, RefusalReason, SpawnReply, Status};
use super::record::{Claim, NewThreadRecord, NewThreadStore, RecordState};
use super::start::{ShownThread, StartView};
use super::{
    Binding, BindingField, NEW_THREAD_SCHEMA_ID, NewThreadRequest, ProjectRef, ProjectView,
    Settings, request_id,
};

/// Exactly what the one call passes: every setting explicit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnRequest {
    pub project_id: String,
    /// The `spawn --provider` id.
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub request_id: String,
    /// The prompt, every attachment through the gate and claimed.
    pub prompt: ReadyPrompt,
}

/// One thread seen in the project after an uncertain create: an observation,
/// never proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservedThread {
    pub id: String,
    pub created_at: Fact<Timestamp>,
}

/// The adapter side of `new_thread`: the reads and the one write.
pub trait NewThreadClient: Send + Sync {
    /// The provider's name, such as `room-cli`.
    fn name(&self) -> &str;

    /// The version the adapter was tested against.
    fn tested_version(&self) -> &str;

    /// The effort levels the provider's CLI verifies.
    fn cli_levels(&self) -> &[&str];

    /// The version the provider reports now.
    fn version(&self, deadline: Instant) -> Result<String, SourceError>;

    /// The default project list (hidden projects are not in it).
    fn projects(&self, deadline: Instant) -> Result<Vec<ProjectCandidate>, SourceError>;

    /// One catalogue read, with only the catalogue keys.
    fn catalogue(&self, deadline: Instant) -> Result<Catalogue, SourceError>;

    /// The one write: exactly one provider call. Never retried.
    fn spawn(&self, spawn: &SpawnRequest, deadline: Instant) -> SpawnReply;

    /// One read of a created thread, for its start stage.
    fn show_thread(&self, thread_id: &str, deadline: Instant) -> Result<ShownThread, SourceError>;

    /// The threads in `project_id` created at or after `since`. Reads only.
    fn threads_since(
        &self,
        project_id: &str,
        since: Timestamp,
        deadline: Instant,
    ) -> Result<Vec<ObservedThread>, SourceError>;
}

/// What this call of `new_thread` came to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewThreadResult {
    /// `replayed` when it comes from the record of an earlier call with the
    /// same op id (no call was made); `recovered` when recovery set it.
    Outcome {
        outcome: Outcome,
        replayed: bool,
        recovered: bool,
    },
    /// Another process owns this op id. No call was made.
    InProgress { state: Option<RecordState> },
    /// This op id is bound to something else. No call was made.
    ConflictingReuse { differs: Vec<BindingField> },
}

/// The result of `new_thread`. See `contracts/new-thread.v1.schema.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NewThreadReport {
    /// Always [`NEW_THREAD_SCHEMA_ID`].
    pub schema: String,
    pub op_id: OpId,
    /// The request id derived from the op id.
    pub request_id: String,
    /// The project as the owner named it.
    pub project_ref: ProjectRef,
    /// The project as resolved, once read.
    pub project: Option<ProjectView>,
    pub source: Option<SourceInfo>,
    /// The requested configuration. Never what CloudRoom reports.
    pub requested: Settings,
    /// The `spawn` provider, once the options were checked.
    pub provider: Option<String>,
    pub status: Status,
    pub summary: String,
    pub result: NewThreadResult,
    pub effects: Option<Effects>,
    pub text_sha256: String,
    pub attachments_digest: String,
    /// The attachments, in the owner's order (B2). **Absent when there are
    /// none**, so a text-only report keeps its A1 shape.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<PromptAttachment>,
    pub text_bytes: u64,
    pub gaps: Vec<Gap>,
    pub claimed_at: Option<Timestamp>,
    pub dispatching_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    /// The start of a created thread, once followed (`openwiz new` follows
    /// it after the create, sheet the check). `new_thread` itself never reads it:
    /// always `None` from here.
    pub start: Option<StartView>,
}

/// `new_thread` could not produce a report. Nothing was created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewThreadError {
    /// The request is invalid. Nothing was claimed, read or sent.
    Invalid(InvalidRequest),
    /// The record could not be used. Nothing was sent.
    Store(StoreError),
}

/// What the reads before a create found: the project, the provider and the
/// effects, or the outcome that stops the create.
struct Look {
    project: Option<ProjectView>,
    /// The effects and the `spawn` provider, or the outcome that stops the
    /// create.
    outcome: Result<(Effects, String), Outcome>,
    gaps: Vec<Gap>,
}

/// Steps 4 to 6: the project, the options and the effects. The same reads
/// and rules for a preview and a create.
fn look(
    client: &dyn NewThreadClient,
    clock: &dyn Clock,
    request: &NewThreadRequest,
    deadline: Instant,
) -> Look {
    let mut look = Look {
        project: None,
        outcome: Err(Outcome::failed(FailReason::ProjectsNotRead)),
        gaps: vec![],
    };
    let projects = match client.projects(deadline) {
        Ok(projects) => projects,
        Err(e) => {
            look.gaps.push(Gap {
                subject: "project list".to_owned(),
                detail: read_failure("project list", &e),
            });
            return look;
        }
    };
    let project = match resolve_project(&request.project, &projects) {
        Ok(project) => project,
        Err(reason) => {
            look.outcome = Err(Outcome::refused(reason));
            return look;
        }
    };
    look.project = Some(project.clone());
    let read_at = clock.now();
    let options = cloud_options(client.catalogue(deadline), client.cli_levels(), read_at);
    let provider = match options.offered(&request.settings) {
        Ok(provider) => provider,
        Err(not) => {
            let reason = match not.field {
                NotOfferedField::Options => RefusalReason::NoOptions,
                NotOfferedField::Harness => RefusalReason::HarnessNotOffered,
                NotOfferedField::Model => RefusalReason::ModelNotOffered,
                NotOfferedField::Effort => RefusalReason::EffortNotOffered,
            };
            look.outcome = Err(Outcome::refused_because(reason, &not.text, not.offered));
            return look;
        }
    };
    look.outcome = Ok((effects_for(&project, read_at), provider));
    look
}

/// Creates one Cloud thread, with no attachment store: any attachment is
/// refused before anything is claimed ([`NoAttachments`]). See the module
/// documentation, and [`new_thread_with`].
pub fn new_thread(
    client: &dyn NewThreadClient,
    store: &dyn NewThreadStore,
    clock: &dyn Clock,
    request: &NewThreadRequest,
) -> Result<NewThreadReport, NewThreadError> {
    new_thread_with(client, store, &NoAttachments, clock, request)
}

/// Creates one Cloud thread whose first prompt carries its text and its
/// attachments. See the module documentation.
pub fn new_thread_with(
    client: &dyn NewThreadClient,
    store: &dyn NewThreadStore,
    attachments: &dyn StagedAttachments,
    clock: &dyn Clock,
    request: &NewThreadRequest,
) -> Result<NewThreadReport, NewThreadError> {
    request.validate().map_err(NewThreadError::Invalid)?;
    let mut deadline = clock.monotonic() + request.budget;
    let op = OpRef {
        kind: OpKind::NewThread,
        op_id: request.op_id.clone(),
    };

    // The binding: the attachments' content, in the owner's order, so one
    // request id never carries two payloads. For an op id already
    // recorded, each attachment as its record holds it when it has that id
    // (a created thread's copies are gone, the check), else its copy now. For a
    // new op id, every attachment passes the gate first, so a refusal
    // claims nothing.
    let known = store
        .inspect(&request.op_id, clock.now())
        .map_err(NewThreadError::Store)?;
    let (binding, bound, mut ready) = match &known {
        Some(record) => {
            let ids = &request.prompt.attachments;
            match recorded(attachments, ids, &record.attachments, clock.now())
                .map_err(|e| gate_error(GateError::Store(e)))?
            {
                Some(list) => (Binding::new(request, attachments_digest(&list)), list, None),
                // An attachment that is neither the record's nor a ready
                // copy can't be the recorded payload: conflicting reuse,
                // with no call, so one request id never carries two
                // payloads.
                None => {
                    let binding = Binding::new(request, record.binding.attachments_digest.clone());
                    let mut differs = record.binding.differences(&binding);
                    differs.push(BindingField::Attachments);
                    return Ok(conflicting_reuse(request, &binding, &[], differs));
                }
            }
        }
        None => {
            let ready = pass_gate(attachments, request, &op, clock)?;
            (
                Binding::new(request, ready.attachments_digest()),
                ready.bound(),
                Some(ready),
            )
        }
    };
    let claimed = NewThreadRecord::claimed(
        request.op_id.clone(),
        binding.clone(),
        request.settings.clone(),
        &request.prompt.text,
        clock.now(),
    )
    .with_attachments(bound.clone());
    match store
        .claim(&claimed, clock.now())
        .map_err(NewThreadError::Store)?
    {
        Claim::Owned => {}
        Claim::Existing(record) | Claim::InProgress(record)
            if !record.binding.differences(&binding).is_empty() =>
        {
            let differs = record.binding.differences(&binding);
            return Ok(conflicting_reuse(request, &binding, &bound, differs));
        }
        Claim::Existing(record) => return Ok(from_record(&record, true, vec![])),
        Claim::InProgress(record) => {
            return Ok(in_progress(request, &binding, &bound, Some(&record)));
        }
        Claim::Pending => return Ok(in_progress(request, &binding, &bound, None)),
    }

    // This process owns the op id. Its attachments are claimed for it, all
    // or none, before anything is read; if they can't be, the claim of the
    // op id is withdrawn and nothing is read or sent.
    let ready = match ready.take() {
        Some(ready) => ready,
        // The record seen first is gone: the gate runs now.
        None => match pass_gate(attachments, request, &op, clock) {
            Ok(ready) => ready,
            Err(e) => {
                let _ = store.withdraw(&request.op_id);
                return Err(e);
            }
        },
    };
    let ids = request.prompt.attachments.clone();
    match attachments.claim(&op, &ids, clock.now()) {
        Ok(ClaimOutcome::Claimed { .. }) => {}
        Ok(ClaimOutcome::Refused(refusal)) => {
            let _ = store.withdraw(&request.op_id);
            return Err(NewThreadError::Invalid(InvalidRequest(refusal.words())));
        }
        Err(e) => {
            let _ = store.withdraw(&request.op_id);
            return Err(NewThreadError::Store(StoreError(format!(
                "{e}; the attachments could not be claimed, so nothing was sent"
            ))));
        }
    }
    deadline += attachment_allowance(ready.total_bytes());
    let created = Dispatch {
        client,
        store,
        attachments,
        clock,
        request,
        op: &op,
        ready: &ready,
        deadline,
    }
    .run(claimed);
    store.release(&request.op_id);
    created
}

/// The gate, for this create: a refusal is an invalid request, naming
/// every attachment that can't go.
fn pass_gate(
    attachments: &dyn StagedAttachments,
    request: &NewThreadRequest,
    op: &OpRef,
    clock: &dyn Clock,
) -> Result<ReadyPrompt, NewThreadError> {
    gate(attachments, &request.prompt, op, clock.now()).map_err(gate_error)
}

/// The gate's refusal as an invalid request, naming every attachment that
/// can't go; its store error as the create's.
fn gate_error(e: GateError) -> NewThreadError {
    match e {
        GateError::Refused(refusal) => NewThreadError::Invalid(InvalidRequest(refusal.words())),
        GateError::Store(e) => NewThreadError::Store(StoreError(format!(
            "{e}; the attachments could not be read, so nothing was sent"
        ))),
    }
}

struct Dispatch<'a> {
    client: &'a dyn NewThreadClient,
    store: &'a dyn NewThreadStore,
    attachments: &'a dyn StagedAttachments,
    clock: &'a dyn Clock,
    request: &'a NewThreadRequest,
    op: &'a OpRef,
    /// The prompt, every attachment claimed for this create.
    ready: &'a ReadyPrompt,
    deadline: Instant,
}

impl Dispatch<'_> {
    fn run(&self, mut record: NewThreadRecord) -> Result<NewThreadReport, NewThreadError> {
        let (source, version_gap) = read_source(self.client, self.deadline);
        record.source = Some(source);
        record.gaps.extend(version_gap);

        let look = look(self.client, self.clock, self.request, self.deadline);
        record.project = look.project;
        record.gaps.extend(look.gaps);
        let (effects, provider) = match look.outcome {
            Ok(found) => found,
            Err(outcome) => return self.finish(record, outcome),
        };
        record.effects = Some(effects.clone());
        record.provider = Some(provider.clone());
        if self.clock.monotonic() >= self.deadline {
            return self.finish(record, Outcome::failed(FailReason::BudgetExhausted));
        }

        // The dispatching mark is synced before the one call: a process that
        // stops after this point recovers as uncertain, never as failed.
        let record = record.dispatching(self.clock.now());
        if let Err(e) = self.store.update(&record) {
            return Err(NewThreadError::Store(StoreError(format!(
                "{e}; the dispatching mark could not be written, so nothing was sent"
            ))));
        }
        let spawn = SpawnRequest {
            project_id: effects.project.id.clone(),
            provider,
            model: self.request.settings.model.clone(),
            effort: self.request.settings.effort.clone(),
            request_id: record.request_id.clone(),
            prompt: self.ready.clone(),
        };
        let (outcome, gaps) = Outcome::from_reply(self.client.spawn(&spawn, self.deadline));
        let outcome = outcome.with_attachments(self.ready.attachments.len());
        let mut record = record;
        record.gaps.extend(gaps);
        self.finish(record, outcome)
    }

    /// Records the outcome and reports it. If the record can't be written,
    /// the report says so, and the record recovers as failed (before the
    /// dispatching mark) or uncertain (after it). Only once the outcome is
    /// recorded are the attachments settled: a created thread's copies are
    /// deleted then, never before (the check).
    fn finish(
        &self,
        record: NewThreadRecord,
        outcome: Outcome,
    ) -> Result<NewThreadReport, NewThreadError> {
        let finished = record.finished(outcome, self.clock.now());
        let mut gaps = vec![];
        if let Err(e) = self.store.update(&finished) {
            gaps.push(Gap {
                subject: "new-thread record".to_owned(),
                detail: format!(
                    "the outcome could not be recorded ({e}); the record will recover as {}",
                    match record.state {
                        RecordState::Claimed => "failed (interrupted before dispatch)",
                        _ => "uncertain",
                    }
                ),
            });
        } else if !self.request.prompt.attachments.is_empty()
            && let Err(e) =
                self.attachments
                    .settle(self.op, &self.request.prompt.attachments, self.clock.now())
        {
            gaps.push(Gap {
                subject: "attachments".to_owned(),
                detail: format!(
                    "the attachments could not be settled after the outcome was recorded ({e}); \
                     the next sweep applies the retention rule"
                ),
            });
        }
        Ok(from_record(&finished, false, gaps))
    }
}

/// The provider and its version, reported and never a gate .
fn read_source(client: &dyn NewThreadClient, deadline: Instant) -> (SourceInfo, Option<Gap>) {
    let name = client.name();
    let tested = client.tested_version();
    let read = format!("{name} --version");
    let (version, version_tested) = version_fact(&read, tested, client.version(deadline));
    let gap = match &version {
        Fact::Unknown(why) => Some(Gap {
            subject: read,
            detail: why.clone(),
        }),
        Fact::Known(_) => None,
    };
    (
        SourceInfo {
            name: name.to_owned(),
            version,
            tested_version: tested.to_owned(),
            version_tested,
        },
        gap,
    )
}

/// The verdict of a preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewVerdict {
    /// A create would go ahead, with these effects.
    Ready { provider: String },
    /// A create would end with this outcome, and create nothing.
    Stopped { outcome: Outcome },
}

/// What a create would do: shown before anything is created. Claims nothing
/// and calls nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NewThreadPreview {
    /// Always [`PREVIEW_SCHEMA_ID`].
    pub schema: String,
    pub op_id: OpId,
    pub request_id: String,
    pub project_ref: ProjectRef,
    pub project: Option<ProjectView>,
    pub source: SourceInfo,
    pub requested: Settings,
    pub verdict: PreviewVerdict,
    pub effects: Option<Effects>,
    pub text_sha256: String,
    pub attachments_digest: String,
    /// The attachments, in the owner's order, each through the gate (B2).
    /// **Absent when there are none.**
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<PromptAttachment>,
    pub text_bytes: u64,
    pub gaps: Vec<Gap>,
}

/// The schema id of a preview.
pub const PREVIEW_SCHEMA_ID: &str = "openwiz.new-thread-preview.v1";

/// What [`new_thread`] would do with `request`, from the same reads and
/// rules: no claim, no record and no call. With no attachment store: any
/// attachment is refused ([`preview_new_thread_with`]).
pub fn preview_new_thread(
    client: &dyn NewThreadClient,
    clock: &dyn Clock,
    request: &NewThreadRequest,
) -> Result<NewThreadPreview, InvalidRequest> {
    preview_new_thread_with(client, &NoAttachments, clock, request).map_err(|e| match e {
        NewThreadError::Invalid(why) => why,
        NewThreadError::Store(StoreError(why)) => InvalidRequest(why),
    })
}

/// [`preview_new_thread`] with the attachment store: every attachment goes
/// through the same gate a create passes (its copy read again whole), and
/// nothing is claimed, recorded or called.
pub fn preview_new_thread_with(
    client: &dyn NewThreadClient,
    attachments: &dyn StagedAttachments,
    clock: &dyn Clock,
    request: &NewThreadRequest,
) -> Result<NewThreadPreview, NewThreadError> {
    request.validate().map_err(NewThreadError::Invalid)?;
    let op = OpRef {
        kind: OpKind::NewThread,
        op_id: request.op_id.clone(),
    };
    let ready = pass_gate(attachments, request, &op, clock)?;
    let deadline = clock.monotonic() + request.budget;
    let (source, version_gap) = read_source(client, deadline);
    let look = look(client, clock, request, deadline);
    let mut gaps: Vec<Gap> = version_gap.into_iter().collect();
    gaps.extend(look.gaps);
    let (verdict, effects) = match look.outcome {
        Ok((effects, provider)) => (PreviewVerdict::Ready { provider }, Some(effects)),
        Err(outcome) => (PreviewVerdict::Stopped { outcome }, None),
    };
    let binding = Binding::new(request, ready.attachments_digest());
    Ok(NewThreadPreview {
        schema: PREVIEW_SCHEMA_ID.to_owned(),
        op_id: request.op_id.clone(),
        request_id: request_id(&request.op_id),
        project_ref: request.project.clone(),
        project: look.project,
        source,
        requested: request.settings.clone(),
        verdict,
        effects,
        text_sha256: binding.text_sha256,
        attachments_digest: binding.attachments_digest,
        attachments: ready.bound(),
        text_bytes: request.prompt.text.len() as u64,
        gaps,
    })
}

/// The report of a finished record.
pub(crate) fn from_record(
    record: &NewThreadRecord,
    replayed: bool,
    extra_gaps: Vec<Gap>,
) -> NewThreadReport {
    let outcome = record.outcome.clone().unwrap_or_else(|| {
        // A record is only reported once finished; recovery finishes it.
        Outcome::uncertain(crate::messaging::send::UncertainReason::InterruptedAfterDispatchMark)
    });
    let mut gaps = record.gaps.clone();
    gaps.extend(extra_gaps);
    NewThreadReport {
        schema: NEW_THREAD_SCHEMA_ID.to_owned(),
        op_id: record.op_id.clone(),
        request_id: record.request_id.clone(),
        project_ref: record.binding.project.clone(),
        project: record.project.clone(),
        source: record.source.clone(),
        requested: record.requested.clone(),
        provider: record.provider.clone(),
        status: outcome.status(),
        summary: summary(&record.requested, outcome.text()),
        result: NewThreadResult::Outcome {
            outcome,
            replayed,
            recovered: record.recovered,
        },
        effects: record.effects.clone(),
        text_sha256: record.binding.text_sha256.clone(),
        attachments_digest: record.binding.attachments_digest.clone(),
        attachments: record.attachments.clone(),
        text_bytes: record.text_bytes,
        gaps,
        claimed_at: Some(record.claimed_at),
        dispatching_at: record.dispatching_at,
        finished_at: record.finished_at,
        start: None,
    }
}

/// The report of a record still in flight: no outcome yet.
pub(crate) fn in_flight_report(record: &NewThreadRecord) -> NewThreadReport {
    let state = match record.state {
        RecordState::Claimed => "claimed",
        RecordState::Dispatching => "dispatching",
        RecordState::Finished => "finished",
    };
    NewThreadReport {
        status: Status::InProgress,
        summary: summary(
            &record.requested,
            &format!(
                "in progress: op {} is {state} by its process and has no outcome yet; no call \
                 was made here",
                record.op_id
            ),
        ),
        result: NewThreadResult::InProgress {
            state: Some(record.state),
        },
        finished_at: None,
        ..from_record(record, false, vec![])
    }
}

/// `<harness> · <model> · <effort> requested · <outcome>`.
fn summary(requested: &Settings, words: &str) -> String {
    format!(
        "{} · {} · {} requested · {words}",
        requested.harness, requested.model, requested.effort
    )
}

fn refusal(
    request: &NewThreadRequest,
    binding: &Binding,
    bound: &[PromptAttachment],
    status: Status,
    words: String,
    result: NewThreadResult,
) -> NewThreadReport {
    NewThreadReport {
        schema: NEW_THREAD_SCHEMA_ID.to_owned(),
        op_id: request.op_id.clone(),
        request_id: request_id(&request.op_id),
        project_ref: request.project.clone(),
        project: None,
        source: None,
        requested: request.settings.clone(),
        provider: None,
        status,
        summary: summary(&request.settings, &words),
        result,
        effects: None,
        text_sha256: binding.text_sha256.clone(),
        attachments_digest: binding.attachments_digest.clone(),
        attachments: bound.to_vec(),
        text_bytes: request.prompt.text.len() as u64,
        gaps: vec![],
        claimed_at: None,
        dispatching_at: None,
        finished_at: None,
        start: None,
    }
}

fn conflicting_reuse(
    request: &NewThreadRequest,
    binding: &Binding,
    bound: &[PromptAttachment],
    differs: Vec<BindingField>,
) -> NewThreadReport {
    let names: Vec<&str> = differs.iter().map(|f| f.as_str()).collect();
    let words = format!(
        "refused: op {} is bound to a different {}; no call was made",
        request.op_id,
        names.join(", ")
    );
    refusal(
        request,
        binding,
        bound,
        Status::ConflictingReuse,
        words,
        NewThreadResult::ConflictingReuse { differs },
    )
}

fn in_progress(
    request: &NewThreadRequest,
    binding: &Binding,
    bound: &[PromptAttachment],
    record: Option<&NewThreadRecord>,
) -> NewThreadReport {
    let words = format!(
        "in progress: another process is creating op {}; no call was made here",
        request.op_id
    );
    let mut report = refusal(
        request,
        binding,
        bound,
        Status::InProgress,
        words,
        NewThreadResult::InProgress {
            state: record.map(|r| r.state),
        },
    );
    if let Some(record) = record {
        report.claimed_at = Some(record.claimed_at);
        report.dispatching_at = record.dispatching_at;
    }
    report
}
