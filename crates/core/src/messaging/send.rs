use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::attachment::PromptAttachment;
use crate::envelope::SourceInfo;
use crate::model::{Fact, SessionId, Timestamp};
use crate::ops::{Clock, version_fact};
use crate::prompt::{
    ClaimOutcome, GateError, NoAttachments, OpKind, OpRef, PromptPayload, ReadyPrompt,
    StagedAttachments, attachment_allowance, attachments_digest, gate, handed_over, recorded,
};
use crate::source::SourceError;

use super::effects::{Effects, TargetRecord};
use super::preview::{held_back, look};
use super::reconcile::{ATTRIBUTION_0_43_1, Reconciliation};
use super::record::{Claim, RecordState, SendRecord, SendStore, StoreError};
use super::{Binding, BindingField, InvalidRequest, MESSAGING_SCHEMA_ID, Mode, OpId, SEND_BUDGET};

/// Why Steer with attachments is refused, before anything is claimed, read
/// or sent: a Cloud steer drops them in source, and Local is unverified.
/// Steer is never turned into Queue.
pub const STEER_WITH_ATTACHMENTS: &str = "Steer can't carry attachments. Use Queue, or remove them";

/// The adapter side of `send`: the version, the target read and the one
/// write.
pub trait Messenger: Send + Sync {
    /// The provider's name, such as `room-cli`.
    fn name(&self) -> &str;

    /// The version the adapter was tested against.
    fn tested_version(&self) -> &str;

    /// Whether `id` can be passed to the provider safely, as one argument.
    fn check_target_id(&self, id: &SessionId) -> Result<(), InvalidRequest>;

    /// The version the provider reports now.
    fn version(&self, deadline: Instant) -> Result<String, SourceError>;

    /// One read of the target, for its existence, visibility and effects.
    fn read_target(&self, id: &SessionId, deadline: Instant) -> Result<TargetRecord, SourceError>;

    /// The one write: exactly one provider call, in `mode`, with `text` as
    /// it is. Never retried.
    fn tell(&self, id: &SessionId, mode: Mode, text: &str, deadline: Instant) -> TellReply;

    /// The one write with a whole prompt: exactly one provider call, in
    /// `mode`, with the text as it is and every attachment, in
    /// [`ReadyPrompt::dispatch_order`]. Never retried, and never with only
    /// some of the attachments.
    ///
    /// The default carries text only: a prompt with any attachment is not
    /// started at all ([`TellReply::NotStarted`]: nothing is sent), so a
    /// messenger that can't carry attachments never drops them.
    fn tell_prompt(
        &self,
        id: &SessionId,
        mode: Mode,
        prompt: &ReadyPrompt,
        deadline: Instant,
    ) -> TellReply {
        if prompt.attachments.is_empty() {
            self.tell(id, mode, &prompt.text, deadline)
        } else {
            TellReply::NotStarted
        }
    }
}

/// Why a queued message waits, as the provider reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitingOn {
    /// The provider reported no waiting reason.
    NotReported,
    Time,
    ThreadBusy,
    TurnStarting,
    Provisioning,
    HostOffline,
    Interaction,
    Plugin,
}

impl WaitingOn {
    pub fn words(self) -> &'static str {
        match self {
            Self::NotReported => "waiting reason not reported",
            Self::Time => "waiting for its scheduled time",
            Self::ThreadBusy => "thread busy",
            Self::TurnStarting => "a turn is starting",
            Self::Provisioning => "provisioning",
            Self::HostOffline => "host offline",
            Self::Interaction => "waiting on an interaction",
            Self::Plugin => "waiting on a plugin",
        }
    }
}

/// A documented rejection before acceptance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    NotFound,
    InvalidInput,
    UnsupportedMode,
}

/// What the adapter made of the provider's reply to the one call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TellReply {
    /// `delivery: "sent"`.
    Sent,
    /// `delivery: "queued"`, with the queued message's reason and id.
    Queued {
        waiting_on: WaitingOn,
        queued_message_id: String,
    },
    /// A documented rejection.
    Rejected(Rejection),
    /// The command could not be started at all: nothing was sent.
    NotStarted,
    /// A timeout, a crash, output that is not JSON, an unexpected receipt
    /// shape or an undocumented error: the message may or may not have been
    /// accepted.
    Unclear {
        reason: UncertainReason,
        gaps: Vec<SendGap>,
    },
}

/// Why a send failed. Nothing was delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailReason {
    /// The provider said the target does not exist (from the read, or the
    /// call).
    NotFound,
    /// The provider rejected the input.
    InvalidInput,
    /// The provider does not support the requested mode for this target.
    /// No other mode was tried.
    UnsupportedMode,
    /// The process stopped after the claim and before the dispatching mark.
    InterruptedBeforeDispatch,
    /// Not produced the version is never a gate ("send policy b": the version is never a
    /// gate). Kept so that a record written before can still be read; its
    /// replay is a failed send.
    UntestedVersion,
    /// The target could not be read, so nothing was sent.
    TargetNotRead,
    TargetArchived,
    TargetHidden,
    /// Whether the target is archived or hidden could not be read.
    TargetVisibilityUnknown,
    /// The queue appears paused (or that cannot be ruled out), and the owner
    /// did not acknowledge resuming it.
    PausedQueueNotAcknowledged,
    /// The dispatching mark could not be written, so nothing was sent.
    RecordNotWritten,
    /// The send's budget ran out before the call, so nothing was sent.
    BudgetExhausted,
    /// The provider's command could not be started, so nothing was sent.
    CallNotStarted,
}

impl FailReason {
    /// The reason in the core's words.
    pub fn words(self) -> &'static str {
        match self {
            Self::NotFound => "the target was not found",
            Self::InvalidInput => "the provider rejected the input",
            Self::UnsupportedMode => {
                "unsupported mode: the provider does not support the requested mode for this \
                 thread; no other mode was tried"
            }
            Self::InterruptedBeforeDispatch => {
                "interrupted before dispatch: the sender stopped after claiming the op id and \
                 before the dispatching mark, so nothing was sent"
            }
            Self::UntestedVersion => "the provider is not the tested version; nothing was sent",
            Self::TargetNotRead => "the target could not be read; nothing was sent",
            Self::TargetArchived => "the target is archived, and archived targets are refused",
            Self::TargetHidden => "the target is hidden, and hidden targets are refused",
            Self::TargetVisibilityUnknown => {
                "whether the target is archived or hidden could not be read; nothing was sent"
            }
            Self::PausedQueueNotAcknowledged => {
                "the target's queue appears paused, or that cannot be ruled out, and sending \
                 would resume it; nothing was sent without the owner's acknowledgement"
            }
            Self::RecordNotWritten => {
                "the dispatching mark could not be written to the send record; nothing was sent"
            }
            Self::BudgetExhausted => {
                "the send's time budget ran out before the call; nothing was sent"
            }
            Self::CallNotStarted => "the provider's command could not be started; nothing was sent",
        }
    }
}

/// Why a send is uncertain: the message may or may not have been accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UncertainReason {
    /// The call did not answer in time.
    TimedOut,
    /// The call ended without an exit code (a crash or a kill).
    EndedWithoutExitCode,
    /// The call exited 0 with output that is not JSON.
    NotJson,
    /// The receipt is not the documented shape.
    UnexpectedReceipt,
    /// The call failed with an error that is not a documented rejection.
    UndocumentedError,
    /// The call could not be confirmed to have started or ended cleanly.
    CallNotConfirmed,
    /// The process stopped after the dispatching mark and before the outcome
    /// was recorded.
    InterruptedAfterDispatchMark,
}

impl UncertainReason {
    /// The reason in the core's words.
    pub fn words(self) -> &'static str {
        match self {
            Self::TimedOut => "the provider did not answer in time",
            Self::EndedWithoutExitCode => "the provider's command ended without an exit code",
            Self::NotJson => "the provider's reply was not JSON",
            Self::UnexpectedReceipt => "the receipt was not the documented shape",
            Self::UndocumentedError => {
                "the provider failed with an error that is not a documented rejection"
            }
            Self::CallNotConfirmed => "the call could not be confirmed to have run cleanly",
            Self::InterruptedAfterDispatchMark => {
                "interrupted: the sender stopped after the dispatching mark and before \
                 recording an outcome"
            }
        }
    }
}

/// Why whether a steer joined the running turn is unknown.
pub const JOINED_NOT_REPORTED: &str = "not reported by the provider";

/// The reported delivery: what the provider said, in the core's words.
/// Separate from the requested [`Mode`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Delivery {
    /// The provider queued the message.
    Queued {
        waiting_on: WaitingOn,
        queued_message_id: String,
        text: String,
    },
    /// `delivery: "sent"`: the provider accepted the message. Never that the
    /// work completed. Whether a steer joined the running turn is not
    /// reported.
    Accepted {
        joined_running_turn: Fact<bool>,
        text: String,
    },
    Failed {
        reason: FailReason,
        text: String,
    },
    /// The message may or may not have been accepted. The text is kept.
    Uncertain {
        reason: UncertainReason,
        text: String,
        reconciliation: Reconciliation,
        attribution: String,
    },
}

impl Delivery {
    /// Failed for `reason`, in the core's words.
    pub fn failed(reason: FailReason) -> Self {
        Self::failed_because(reason, reason.words())
    }

    /// Failed for `reason`, with OpenWiz's own wording of the details.
    pub fn failed_because(reason: FailReason, words: &str) -> Self {
        Self::Failed {
            reason,
            text: format!("failed: {words}"),
        }
    }

    /// Uncertain for `reason`. No reconciliation read is allowed, so
    /// nothing was observed, and no attribution path exists.
    pub fn uncertain(reason: UncertainReason) -> Self {
        Self::Uncertain {
            reason,
            text: format!(
                "uncertain: {}. The message may or may not have been accepted; the text is \
                 kept and nothing is resent",
                reason.words()
            ),
            reconciliation: Reconciliation::NotRead,
            attribution: ATTRIBUTION_0_43_1.to_owned(),
        }
    }

    /// The provider queued the message.
    pub fn queued(waiting_on: WaitingOn, queued_message_id: String) -> Self {
        Self::Queued {
            waiting_on,
            queued_message_id,
            text: format!("queued: {}", waiting_on.words()),
        }
    }

    /// The same delivery, saying that `count` attachments went with it when
    /// it was queued or accepted, and that their delivery to the agent's
    /// machine is not reported. Unchanged for no attachment, or for a
    /// failed or uncertain delivery.
    pub fn with_attachments(self, count: usize) -> Self {
        if count == 0 {
            return self;
        }
        match self {
            Self::Queued {
                waiting_on,
                queued_message_id,
                text,
            } => Self::Queued {
                waiting_on,
                queued_message_id,
                text: format!("{text}{}", handed_over(count)),
            },
            Self::Accepted {
                joined_running_turn,
                text,
            } => Self::Accepted {
                joined_running_turn,
                text: format!("{text}{}", handed_over(count)),
            },
            other => other,
        }
    }

    /// The provider accepted the message (`sent`), in `mode`.
    pub fn accepted(mode: Mode) -> Self {
        let text = match mode {
            Mode::Queue => "sent: the provider accepted the message, not that the work completed",
            Mode::Steer => {
                "sent; whether it joined the running turn is not reported (the provider \
                 accepted the message, not that the work completed)"
            }
        };
        Self::Accepted {
            joined_running_turn: Fact::Unknown(JOINED_NOT_REPORTED.to_owned()),
            text: text.to_owned(),
        }
    }

    /// The reported delivery in the core's words.
    pub fn text(&self) -> &str {
        match self {
            Self::Queued { text, .. }
            | Self::Accepted { text, .. }
            | Self::Failed { text, .. }
            | Self::Uncertain { text, .. } => text,
        }
    }

    pub fn status(&self) -> Status {
        match self {
            Self::Queued { .. } => Status::Queued,
            Self::Accepted { .. } => Status::Accepted,
            Self::Failed { .. } => Status::Failed,
            Self::Uncertain { .. } => Status::Uncertain,
        }
    }
}

/// Something missing or wrong in a send, in OpenWiz's own words. Never the
/// provider's raw output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendGap {
    /// What it concerns, such as `thread show` or `receipt`.
    pub subject: String,
    pub detail: String,
}

/// One send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendRequest {
    /// Exactly one target.
    pub target: SessionId,
    pub mode: Mode,
    /// The text and the attachments (B2: `send` moved from plain text onto
    /// the payload `new_thread` shares).
    pub prompt: PromptPayload,
    pub op_id: OpId,
    /// The owner acknowledges that this send resumes a queue that appears
    /// paused.
    pub resume_paused_queue: bool,
    /// The owner acknowledges that this send may duplicate an earlier one
    /// whose outcome is uncertain.
    pub accept_possible_duplicate: bool,
    pub budget: Duration,
}

impl SendRequest {
    /// A text-only, queue-mode request with no acknowledgements and the
    /// default budget.
    pub fn new(target: SessionId, text: impl Into<String>, op_id: OpId) -> Self {
        Self {
            target,
            mode: Mode::Queue,
            prompt: PromptPayload::text(text),
            op_id,
            resume_paused_queue: false,
            accept_possible_duplicate: false,
            budget: SEND_BUDGET,
        }
    }
}

/// The overall status of a send report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Accepted,
    Failed,
    Uncertain,
    InProgress,
    ConflictingReuse,
    PossibleDuplicateRefused,
}

/// An earlier send whose outcome is uncertain, shown before a possible
/// duplicate is refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarlierSend {
    pub op_id: OpId,
    pub mode: Mode,
    pub claimed_at: Timestamp,
    /// True while the earlier send is still claimed or dispatching (same
    /// target, mode and text); false when its outcome is uncertain.
    pub in_flight: bool,
    pub reconciliation: Reconciliation,
}

/// What this call of `send` came to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendResult {
    /// A reported delivery. `replayed` when it comes from the record of an
    /// earlier call with the same op id (no provider call was made);
    /// `recovered` when recovery set it.
    Outcome {
        delivery: Delivery,
        replayed: bool,
        recovered: bool,
    },
    /// Another process owns this op id. No call was made.
    InProgress { state: Option<RecordState> },
    /// This op id is bound to a different target, mode or text. No call was
    /// made.
    ConflictingReuse { differs: Vec<BindingField> },
    /// An earlier send of this text to this target is uncertain, and the
    /// owner did not acknowledge a possible duplicate. Nothing was claimed
    /// or sent.
    PossibleDuplicateRefused { earlier: Vec<EarlierSend> },
}

/// The target as shown in a report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetView {
    pub id: SessionId,
    pub title: Fact<String>,
}

/// The result of `send`, shown by the CLI as text or as
/// `openwiz.messaging.v1` JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendReport {
    /// Always [`MESSAGING_SCHEMA_ID`](super::MESSAGING_SCHEMA_ID).
    pub schema: String,
    pub op_id: OpId,
    pub target: TargetView,

    pub source: Option<SourceInfo>,
    pub requested_mode: Mode,
    pub status: Status,
    /// `<Mode> requested · <reported delivery>`, in the core's words.
    pub summary: String,
    pub result: SendResult,
    /// The effects shown before sending, when the target was read.
    pub effects: Option<Effects>,
    pub text_sha256: String,
    pub text_bytes: u64,
    /// The attachments' digest, as the op id binds it (B2). **Absent when
    /// there are none**, so a text-only report keeps its 0.0.3 shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments_digest: Option<String>,
    /// The attachments, in the owner's order (B2). **Absent when there are
    /// none.**
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<PromptAttachment>,
    pub gaps: Vec<SendGap>,
    pub claimed_at: Option<Timestamp>,
    pub dispatching_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

/// `send` could not produce a report. Nothing was sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// The request is invalid. Nothing was claimed, read or sent.
    Invalid(InvalidRequest),
    /// The send record could not be used. Nothing was sent.
    Store(StoreError),
}

/// Sends one text message to one session, with no attachment store: any
/// attachment is refused before anything is claimed ([`NoAttachments`]).
/// See the module documentation, and [`send_with`].
pub fn send(
    messenger: &dyn Messenger,
    store: &dyn SendStore,
    clock: &dyn Clock,
    request: &SendRequest,
) -> Result<SendReport, SendError> {
    send_with(messenger, store, &NoAttachments, clock, request)
}

/// Sends one prompt, its text and its attachments, to one session. See the
/// module documentation.
pub fn send_with(
    messenger: &dyn Messenger,
    store: &dyn SendStore,
    attachments: &dyn StagedAttachments,
    clock: &dyn Clock,
    request: &SendRequest,
) -> Result<SendReport, SendError> {
    request.prompt.validate().map_err(SendError::Invalid)?;
    if request.mode == Mode::Steer && !request.prompt.attachments.is_empty() {
        return Err(SendError::Invalid(InvalidRequest(
            STEER_WITH_ATTACHMENTS.to_owned(),
        )));
    }
    messenger
        .check_target_id(&request.target)
        .map_err(SendError::Invalid)?;
    let mut deadline = clock.monotonic() + request.budget;
    let op = OpRef {
        kind: OpKind::Send,
        op_id: request.op_id.clone(),
    };
    let text = &request.prompt.text;

    // The binding: the attachments' content, in the owner's order (other
    // copies of the same bytes, kinds and names are the same payload). For
    // an op id already recorded, each attachment as its record holds it
    // when it has that id (an accepted send's copies are gone), else
    // its copy now. For a new op id, every attachment passes the gate
    // first, so a refusal claims nothing.
    let known = store.load(&request.op_id).map_err(SendError::Store)?;
    let (binding, bound, mut ready) = match &known {
        Some(record) => {
            let ids = &request.prompt.attachments;
            match recorded(attachments, ids, &record.attachments, clock.now())
                .map_err(|e| gate_error(GateError::Store(e)))?
            {
                Some(list) => (
                    Binding::with_attachments(
                        request.target.clone(),
                        request.mode,
                        text,
                        attachments_digest(&list),
                    ),
                    list,
                    None,
                ),
                // An attachment that is neither the record's nor a ready
                // copy can't be the recorded payload: conflicting reuse,
                // with no call.
                None => {
                    let binding = Binding::with_attachments(
                        request.target.clone(),
                        request.mode,
                        text,
                        record.binding.attachments_digest.clone(),
                    );
                    let mut differs = record.binding.differences(&binding);
                    differs.push(BindingField::Attachments);
                    return Ok(conflicting_reuse(request, &binding, &[], differs));
                }
            }
        }
        None => {
            let ready = pass_gate(attachments, request, &op, clock)?;
            (
                Binding::with_attachments(
                    request.target.clone(),
                    request.mode,
                    text,
                    ready.attachments_digest(),
                ),
                ready.bound(),
                Some(ready),
            )
        }
    };

    // A possible duplicate is refused before a new op id is claimed.
    if known.is_none() && !request.accept_possible_duplicate {
        let earlier = possible_duplicates(store, clock, request, &binding)?;
        if !earlier.is_empty() {
            return Ok(refused_as_duplicate(request, &binding, &bound, &earlier));
        }
    }

    let claimed = SendRecord::claimed(request.op_id.clone(), binding.clone(), text, clock.now())
        .with_attachments(bound.clone());
    match store
        .claim(&claimed, clock.now())
        .map_err(SendError::Store)?
    {
        Claim::Owned if request.accept_possible_duplicate => {}
        Claim::Owned => {
            // Again after the claim: a sender that claimed the same send
            // after the first check is caught here. Of two that race, at
            // least one sees the other, since each wrote its claim before
            // looking. The claim is withdrawn, so the op id stays unused.
            let earlier = possible_duplicates(store, clock, request, &binding);
            match earlier {
                Ok(earlier) if earlier.is_empty() => {}
                Ok(earlier) => {
                    let mut report = refused_as_duplicate(request, &binding, &bound, &earlier);
                    if let Err(e) = store.withdraw(&request.op_id) {
                        report.gaps.push(withdraw_gap(&e));
                    }
                    return Ok(report);
                }
                Err(e) => {
                    // Nothing was read or sent: the record recovers as
                    // failed (interrupted before dispatch).
                    store.release(&request.op_id);
                    return Err(e);
                }
            }
        }
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
        // The record seen first is gone (withdrawn): the gate runs now.
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
            return Err(SendError::Invalid(InvalidRequest(refusal.words())));
        }
        Err(e) => {
            let _ = store.withdraw(&request.op_id);
            return Err(SendError::Store(StoreError(format!(
                "{e}; the attachments could not be claimed, so nothing was sent"
            ))));
        }
    }
    deadline += attachment_allowance(ready.total_bytes());
    let sent = Dispatch {
        messenger,
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
    sent
}

/// The gate, for this send: a refusal is an invalid request, naming every
/// attachment that can't go.
fn pass_gate(
    attachments: &dyn StagedAttachments,
    request: &SendRequest,
    op: &OpRef,
    clock: &dyn Clock,
) -> Result<ReadyPrompt, SendError> {
    gate(attachments, &request.prompt, op, clock.now()).map_err(gate_error)
}

/// The gate's refusal as an invalid request, naming every attachment that
/// can't go; its store error as the send's.
fn gate_error(e: GateError) -> SendError {
    match e {
        GateError::Refused(refusal) => SendError::Invalid(InvalidRequest(refusal.words())),
        GateError::Store(e) => SendError::Store(StoreError(format!(
            "{e}; the attachments could not be read, so nothing was sent"
        ))),
    }
}

/// Earlier sends this one may duplicate: the same send still in flight
/// (same target, mode, text and attachments), or one of the same text to
/// the same target whose outcome is uncertain (any mode, any attachments).
fn possible_duplicates(
    store: &dyn SendStore,
    clock: &dyn Clock,
    request: &SendRequest,
    binding: &Binding,
) -> Result<Vec<SendRecord>, SendError> {
    let mut earlier = store
        .in_flight_matching(binding, &request.op_id, clock.now())
        .map_err(SendError::Store)?;
    earlier.extend(
        store
            .uncertain_matching(binding, &request.op_id, clock.now())
            .map_err(SendError::Store)?,
    );
    earlier.sort_by(|a, b| (a.claimed_at, &a.op_id).cmp(&(b.claimed_at, &b.op_id)));
    earlier.dedup_by(|a, b| a.op_id == b.op_id);
    Ok(earlier)
}

fn withdraw_gap(error: &StoreError) -> SendGap {
    SendGap {
        subject: "send record".to_owned(),
        detail: format!(
            "the claim of this op id could not be withdrawn ({error}); nothing was sent, and \
             it will recover as failed (interrupted before dispatch)"
        ),
    }
}

/// One owned send, from the claim to the recorded outcome.
struct Dispatch<'a> {
    messenger: &'a dyn Messenger,
    store: &'a dyn SendStore,
    attachments: &'a dyn StagedAttachments,
    clock: &'a dyn Clock,
    request: &'a SendRequest,
    op: &'a OpRef,
    /// The prompt, every attachment claimed for this send.
    ready: &'a ReadyPrompt,
    deadline: Instant,
}

impl Dispatch<'_> {
    fn run(&self, mut record: SendRecord) -> Result<SendReport, SendError> {
        // The version is reported as the reads report it, and never stops
        // the send. Every other safeguard follows.
        let name = self.messenger.name();
        let tested = self.messenger.tested_version();
        let read = format!("{name} --version");
        let (version, version_tested) =
            version_fact(&read, tested, self.messenger.version(self.deadline));
        if let Fact::Unknown(why) = &version {
            record.gaps.push(SendGap {
                subject: read,
                detail: why.clone(),
            });
        }
        record.source = Some(SourceInfo {
            name: name.to_owned(),
            version,
            tested_version: tested.to_owned(),
            version_tested,
        });

        // The same read and rules as a preview, so the two can't drift.
        let look = look(
            self.messenger,
            self.clock,
            &self.request.target,
            self.request.mode,
            self.deadline,
        );
        if let Some(title) = look.title {
            record.target_title = Some(title);
        }
        record.gaps.extend(look.problems);
        let effects = match look.outcome {
            Ok(effects) => effects,
            Err((reason, words)) => {
                return self.finish(record, Delivery::failed_because(reason, &words));
            }
        };
        let held = held_back(
            &effects,
            self.request.resume_paused_queue,
            self.clock,
            self.deadline,
        );
        record.effects = Some(effects);
        if let Some(reason) = held {
            return self.finish(record, Delivery::failed(reason));
        }

        // The dispatching mark is synced before the one call: a sender that
        // stops after this point recovers as uncertain, never as failed.
        let record = record.dispatching(self.clock.now());
        if let Err(e) = self.store.update(&record) {
            return Err(SendError::Store(StoreError(format!(
                "{e}; the dispatching mark could not be written, so nothing was sent"
            ))));
        }
        let reply = self.messenger.tell_prompt(
            &self.request.target,
            self.request.mode,
            self.ready,
            self.deadline,
        );
        let count = self.ready.attachments.len();
        let (delivery, gaps) = match reply {
            TellReply::Sent => (
                Delivery::accepted(self.request.mode).with_attachments(count),
                vec![],
            ),
            TellReply::Queued {
                waiting_on,
                queued_message_id,
            } => (
                Delivery::queued(waiting_on, queued_message_id).with_attachments(count),
                vec![],
            ),
            TellReply::Rejected(rejection) => (
                Delivery::failed(match rejection {
                    Rejection::NotFound => FailReason::NotFound,
                    Rejection::InvalidInput => FailReason::InvalidInput,
                    Rejection::UnsupportedMode => FailReason::UnsupportedMode,
                }),
                vec![],
            ),
            TellReply::NotStarted => (Delivery::failed(FailReason::CallNotStarted), vec![]),
            TellReply::Unclear { reason, gaps } => (Delivery::uncertain(reason), gaps),
        };
        let mut record = record;
        record.gaps.extend(gaps);
        self.finish(record, delivery)
    }

    /// Records the outcome and reports it. If the record cannot be written,
    /// the report says so: the record stays as it was, and recovers as
    /// failed (before the dispatching mark) or uncertain (after it). Only
    /// once the outcome is recorded are the attachments settled: an
    /// accepted send's copies are deleted then, never before.
    fn finish(&self, record: SendRecord, delivery: Delivery) -> Result<SendReport, SendError> {
        let finished = record.finished(delivery, self.clock.now());
        let mut gaps = vec![];
        if let Err(e) = self.store.update(&finished) {
            gaps.push(SendGap {
                subject: "send record".to_owned(),
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
            gaps.push(SendGap {
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

/// A failed read in OpenWiz's own words: never the provider's text.
pub(crate) fn read_failure(read: &str, error: &SourceError) -> String {
    match error {
        SourceError::NotFound { .. } => format!("{read} reported it not found"),
        SourceError::InvalidId { .. } => format!("{read} was not run: the id is not safe to pass"),
        SourceError::CommandFailed {
            exit_code: Some(code),
            ..
        } => format!("{read} exited {code}"),
        SourceError::CommandFailed {
            exit_code: None, ..
        } => format!("{read} ended without an exit code"),
        SourceError::TimedOut { after_ms } => format!("{read} timed out after {after_ms} ms"),
        SourceError::Malformed { .. } => {
            format!("{read} returned output that is not the documented shape")
        }
        SourceError::Unavailable { .. } => format!("{read} could not be run"),
    }
}

/// The report of a finished record.
pub(crate) fn from_record(
    record: &SendRecord,
    replayed: bool,
    extra_gaps: Vec<SendGap>,
) -> SendReport {
    let delivery = record.delivery.clone().unwrap_or_else(|| {
        // A record is only reported once finished; recovery finishes it.
        Delivery::uncertain(UncertainReason::InterruptedAfterDispatchMark)
    });
    let mut gaps = record.gaps.clone();
    gaps.extend(extra_gaps);
    SendReport {
        schema: MESSAGING_SCHEMA_ID.to_owned(),
        op_id: record.op_id.clone(),
        target: TargetView {
            id: record.binding.target.clone(),
            title: record
                .target_title
                .clone()
                .unwrap_or_else(|| Fact::Unknown("not read".to_owned())),
        },
        source: record.source.clone(),
        requested_mode: record.binding.mode,
        status: delivery.status(),
        summary: format!("{} · {}", record.binding.mode.requested(), delivery.text()),
        result: SendResult::Outcome {
            delivery,
            replayed,
            recovered: record.recovered,
        },
        effects: record.effects.clone(),
        text_sha256: record.binding.text_sha256.clone(),
        text_bytes: record.text_bytes,
        attachments_digest: digest_of(&record.attachments, &record.binding),
        attachments: record.attachments.clone(),
        gaps,
        claimed_at: Some(record.claimed_at),
        dispatching_at: record.dispatching_at,
        finished_at: record.finished_at,
    }
}

/// The attachments digest a report shows: only when there are attachments.
fn digest_of(attachments: &[PromptAttachment], binding: &Binding) -> Option<String> {
    (!attachments.is_empty()).then(|| binding.attachments_digest.clone())
}

/// A report for a request that made no call and has no outcome of its own.
/// `bound` is the request's attachments, as its binding holds them.
fn refusal(
    request: &SendRequest,
    binding: &Binding,
    bound: &[PromptAttachment],
    status: Status,
    words: String,
    result: SendResult,
) -> SendReport {
    SendReport {
        schema: MESSAGING_SCHEMA_ID.to_owned(),
        op_id: request.op_id.clone(),
        target: TargetView {
            id: request.target.clone(),
            title: Fact::Unknown("not read: no call was made".to_owned()),
        },
        source: None,
        requested_mode: request.mode,
        status,
        summary: format!("{} · {words}", request.mode.requested()),
        result,
        effects: None,
        text_sha256: binding.text_sha256.clone(),
        text_bytes: request.prompt.text.len() as u64,
        attachments_digest: digest_of(bound, binding),
        attachments: bound.to_vec(),
        gaps: vec![],
        claimed_at: None,
        dispatching_at: None,
        finished_at: None,
    }
}

fn refused_as_duplicate(
    request: &SendRequest,
    binding: &Binding,
    bound: &[PromptAttachment],
    earlier: &[SendRecord],
) -> SendReport {
    let ops: Vec<&str> = earlier.iter().map(|r| r.op_id.as_str()).collect();
    let in_flight = earlier
        .iter()
        .filter(|r| r.state != RecordState::Finished)
        .count();
    let what = if in_flight == 0 {
        "is uncertain"
    } else if in_flight == earlier.len() {
        "is still in progress"
    } else {
        "is uncertain or still in progress"
    };
    let words = format!(
        "refused: an earlier send of this text to this target {what} (op {}), and this one \
         may duplicate it; nothing was sent",
        ops.join(", op ")
    );
    let earlier = earlier
        .iter()
        .map(|r| EarlierSend {
            op_id: r.op_id.clone(),
            mode: r.binding.mode,
            claimed_at: r.claimed_at,
            in_flight: r.state != RecordState::Finished,
            reconciliation: match &r.delivery {
                Some(Delivery::Uncertain { reconciliation, .. }) => reconciliation.clone(),
                _ => Reconciliation::NotRead,
            },
        })
        .collect();
    refusal(
        request,
        binding,
        bound,
        Status::PossibleDuplicateRefused,
        words,
        SendResult::PossibleDuplicateRefused { earlier },
    )
}

fn conflicting_reuse(
    request: &SendRequest,
    binding: &Binding,
    bound: &[PromptAttachment],
    differs: Vec<BindingField>,
) -> SendReport {
    let names: Vec<&str> = differs
        .iter()
        .map(|f| match f {
            BindingField::Target => "target",
            BindingField::Mode => "mode",
            BindingField::Text => "text",
            BindingField::Attachments => "attachments",
        })
        .collect();
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
        SendResult::ConflictingReuse { differs },
    )
}

fn in_progress(
    request: &SendRequest,
    binding: &Binding,
    bound: &[PromptAttachment],
    record: Option<&SendRecord>,
) -> SendReport {
    let state = record.map(|r| r.state);
    let words = format!(
        "in progress: another process is sending op {}; no call was made here",
        request.op_id
    );
    let mut report = refusal(
        request,
        binding,
        bound,
        Status::InProgress,
        words,
        SendResult::InProgress { state },
    );
    if let Some(record) = record {
        report.claimed_at = Some(record.claimed_at);
        report.dispatching_at = record.dispatching_at;
    }
    report
}

/// The report of a record still in flight, for a lookup: claimed or
/// dispatching, no outcome yet.
pub(crate) fn in_flight_report(record: &SendRecord) -> SendReport {
    let state = match record.state {
        RecordState::Claimed => "claimed",
        RecordState::Dispatching => "dispatching",
        RecordState::Finished => "finished",
    };
    SendReport {
        schema: MESSAGING_SCHEMA_ID.to_owned(),
        op_id: record.op_id.clone(),
        target: TargetView {
            id: record.binding.target.clone(),
            title: record
                .target_title
                .clone()
                .unwrap_or_else(|| Fact::Unknown("not read yet".to_owned())),
        },
        source: record.source.clone(),
        requested_mode: record.binding.mode,
        status: Status::InProgress,
        summary: format!(
            "{} · in progress: op {} is {state} by its sender and has no outcome yet; no call \
             was made here",
            record.binding.mode.requested(),
            record.op_id
        ),
        result: SendResult::InProgress {
            state: Some(record.state),
        },
        effects: record.effects.clone(),
        text_sha256: record.binding.text_sha256.clone(),
        text_bytes: record.text_bytes,
        attachments_digest: digest_of(&record.attachments, &record.binding),
        attachments: record.attachments.clone(),
        gaps: record.gaps.clone(),
        claimed_at: Some(record.claimed_at),
        dispatching_at: record.dispatching_at,
        finished_at: None,
    }
}
