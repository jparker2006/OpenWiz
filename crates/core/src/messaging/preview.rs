//! `preview`: what a send to one target in one mode would do, worked out
//! before any text is written. It reads the target and returns the effects
//! `send` would show, with the read's time. No claim, no call.
//!
//! `send` reaches its effects through the same [`look`], so a preview and
//! the send that follows it can't work them out differently. The thread can
//! still change between the preview and the send (see [`effects`]).
//!
//! [`effects`]: super::effects

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::limits::cap_field;
use crate::model::{Fact, SessionId, Timestamp};
use crate::ops::Clock;
use crate::source::SourceError;

use super::effects::{Effects, work_out};
use super::send::{FailReason, Messenger, SendGap, TargetView, read_failure};
use super::{InvalidRequest, Mode, SEND_BUDGET};

/// The schema id of a preview. See `$defs/preview` in
/// `contracts/messaging.v1.schema.json`.
pub const PREVIEW_SCHEMA_ID: &str = "openwiz.send-preview.v1";

/// What a send now would come to before its call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    /// A send would go ahead to its one call.
    Ready,
    /// The queue appears paused, or that cannot be ruled out: a send goes
    /// ahead only with the owner's acknowledgement.
    NeedsAcknowledgement,
    /// A send would be refused before its call: `refusal` says why.
    Refused,
}

/// Why a send now would be refused before its call, in the core's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub reason: FailReason,
    pub text: String,
}

/// The result of `preview`, shown by the CLI as text or as JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewReport {
    /// Always [`PREVIEW_SCHEMA_ID`].
    pub schema: String,
    pub target: TargetView,
    pub requested_mode: Mode,
    pub status: PreviewStatus,
    /// `<Mode> requested · preview: …; nothing was sent`, in the core's
    /// words.
    pub summary: String,
    /// The effects `send` would show, when they were worked out: none for a
    /// target that was not read or is refused, but kept when the budget ran
    /// out after them.
    pub effects: Option<Effects>,
    /// Why a send would be refused, or would need the acknowledgement.
    pub refusal: Option<Refusal>,
    /// When the target read started.
    pub read_at: Timestamp,
    pub gaps: Vec<SendGap>,
}

/// One read of the target and what follows from it: shared by `preview` and
/// `send`.
pub(crate) struct Look {
    /// When the read started: a slow read is never hidden.
    pub read_at: Timestamp,
    /// The title as read, capped; `None` when the target was not read.
    pub title: Option<Fact<String>>,
    /// Fields that could not be read, in OpenWiz's own words.
    pub problems: Vec<SendGap>,
    /// The effects, or why a send is refused before its call (the reason
    /// and its words).
    pub outcome: Result<Effects, (FailReason, String)>,
}

/// Reads `target` and works out the effects of sending in `mode`, or why a
/// send would be refused: not found, not read, archived, hidden, or
/// visibility unknown.
pub(crate) fn look(
    messenger: &dyn Messenger,
    clock: &dyn Clock,
    target: &SessionId,
    mode: Mode,
    deadline: Instant,
) -> Look {
    let read_at = clock.now();
    let refused = |reason: FailReason, words: String| Look {
        read_at,
        title: None,
        problems: vec![],
        outcome: Err((reason, words)),
    };
    let record = match messenger.read_target(target, deadline) {
        Ok(record) => record,
        Err(SourceError::NotFound { .. }) => {
            return refused(
                FailReason::NotFound,
                FailReason::NotFound.words().to_owned(),
            );
        }
        Err(e) => {
            let words = format!(
                "the target could not be read ({}); nothing was sent",
                read_failure("the target read", &e)
            );
            return refused(FailReason::TargetNotRead, words);
        }
    };
    let title = Some(match &record.title {
        Fact::Known(title) => Fact::Known(cap_field(title)),
        Fact::Unknown(why) => Fact::Unknown(cap_field(why)),
    });
    let visibility = match (&record.archived, &record.hidden) {
        (Fact::Known(true), _) => Some(FailReason::TargetArchived),
        (_, Fact::Known(true)) => Some(FailReason::TargetHidden),
        (Fact::Known(false), Fact::Known(false)) => None,
        _ => Some(FailReason::TargetVisibilityUnknown),
    };
    let outcome = match visibility {
        Some(reason) => Err((reason, reason.words().to_owned())),
        None => Ok(work_out(&record, mode, read_at, clock.now())),
    };
    Look {
        read_at,
        title,
        problems: record.problems,
        outcome,
    }
}

/// What stops a send once its effects are worked out and before its call: a
/// queue that appears paused without the owner's acknowledgement, then a
/// budget that ran out. `send` and `preview` both decide here.
pub(crate) fn held_back(
    effects: &Effects,
    resume_paused_queue: bool,
    clock: &dyn Clock,
    deadline: Instant,
) -> Option<FailReason> {
    if effects.needs_acknowledgement && !resume_paused_queue {
        return Some(FailReason::PausedQueueNotAcknowledged);
    }
    if clock.monotonic() >= deadline {
        return Some(FailReason::BudgetExhausted);
    }
    None
}

/// Previews a send to `target` in `mode`. See the module documentation.
pub fn preview(
    messenger: &dyn Messenger,
    clock: &dyn Clock,
    target: &SessionId,
    mode: Mode,
) -> Result<PreviewReport, InvalidRequest> {
    messenger.check_target_id(target)?;
    let deadline = clock.monotonic() + SEND_BUDGET;
    let look = look(messenger, clock, target, mode, deadline);
    let unknown = |why: &str| format!("unknown ({why})");
    let refused = |reason: FailReason, text: String, effects: Option<Effects>| {
        let words = format!("a send would be refused: {text}");
        let refusal = Some(Refusal { reason, text });
        (PreviewStatus::Refused, effects, refusal, words)
    };
    let (status, effects, refusal, words) = match look.outcome {
        Err((reason, text)) => refused(reason, text, None),
        // A preview never acknowledges a paused queue: that is the send's.
        Ok(effects) => match held_back(&effects, false, clock, deadline) {
            None => {
                let words = match &effects.effect {
                    Fact::Known(effect) => format!("a send {}", effect.text),
                    Fact::Unknown(why) => format!("the effect is {}", unknown(why)),
                };
                (PreviewStatus::Ready, Some(effects), None, words)
            }
            Some(reason @ FailReason::PausedQueueNotAcknowledged) => {
                let refusal = Refusal {
                    reason,
                    text: reason.words().to_owned(),
                };
                let paused = match &effects.paused_queue {
                    Fact::Known(paused) => paused.text.clone(),
                    Fact::Unknown(why) => format!("the paused queue is {}", unknown(why)),
                };
                let words =
                    format!("{paused}; a send needs the owner's acknowledgement to resume it");
                let status = PreviewStatus::NeedsAcknowledgement;
                (status, Some(effects), Some(refusal), words)
            }
            // The budget ran out after the effects were worked out: they
            // are kept, as the send records them.
            Some(reason) => refused(reason, reason.words().to_owned(), Some(effects)),
        },
    };
    Ok(PreviewReport {
        schema: PREVIEW_SCHEMA_ID.to_owned(),
        target: TargetView {
            id: target.clone(),
            title: look
                .title
                .unwrap_or_else(|| Fact::Unknown("not read".to_owned())),
        },
        requested_mode: mode,
        status,
        summary: format!("{} · preview: {words}; nothing was sent", mode.requested()),
        effects,
        refusal,
        read_at: look.read_at,
        gaps: look.problems,
    })
}
