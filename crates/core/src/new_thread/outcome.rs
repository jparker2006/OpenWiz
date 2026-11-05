//! What a create came to: created, refused, failed or uncertain.
//!
//! | The provider says | Outcome |
//! | --- | --- |
//! | Exit 0 with a thread row | **Created**, with the thread id. CloudRoom recorded the thread and its first prompt. It does **not** mean the sandbox started or the prompt was accepted |
//! | A documented refusal | **Refused**, with the reason. Nothing was created |
//! | A timeout, a crash, output that is not JSON, an unexpected shape or an undocumented error | **Uncertain** |
//!
//! OpenWiz's own refusals (not offered, a project it won't start in) are
//! **Refused** too, with no call. **Failed** means nothing was sent for a
//! reason of OpenWiz's own, such as an interrupted claim.

use serde::Serialize;

use crate::messaging::send::{SendGap, UncertainReason};

/// Something missing or wrong in a create, in OpenWiz's own words. Never
/// the provider's raw output. The same shape as `send`'s.
pub type Gap = SendGap;

/// A refusal `thread spawn` documents, before anything is recorded
/// (source-read, the implementation; not observed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRefusal {
    /// 400, `Cloud supports new …`: an unsupported harness or option.
    Unsupported,
    /// 400, `This model is unavailable on Cloud`.
    ModelUnavailable,
    /// 400, `This reasoning level is unavailable`.
    EffortUnavailable,
    /// 400, `Message is too long`.
    MessageTooLong,
    /// 400, `Message is empty`.
    MessageEmpty,
    /// 404, `Project not found`.
    ProjectNotFound,
    /// 409, `Request ID was already used with different content`.
    RequestIdConflict,
    /// 503, `Cloud model discovery is unavailable`.
    DiscoveryUnavailable,
}

/// What the adapter made of the provider's reply to the one call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnReply {
    /// Exit 0 with a thread row of the expected shape.
    Created { thread_id: String },
    /// A documented refusal.
    Refused(ProviderRefusal),
    /// The command could not be started at all: nothing was sent.
    NotStarted,
    /// The thread may or may not have been created.
    Unclear {
        reason: UncertainReason,
        gaps: Vec<Gap>,
    },
}

/// Why nothing was created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    // OpenWiz's own, before any call.
    /// CloudRoom offers no options at all.
    NoOptions,
    HarnessNotOffered,
    ModelNotOffered,
    EffortNotOffered,
    /// No project has this id or name.
    ProjectNotFound,
    /// More than one project has this name.
    ProjectAmbiguous,
    /// A personal project.
    ProjectPersonal,
    /// Not a `standard` project, or its kind could not be read.
    ProjectNotStandard,
    /// A standard project without a GitHub remote (sheet the check).
    ProjectNoGithubRemote,
    /// Whether the project has a GitHub remote could not be read.
    ProjectRemoteNotConfirmed,
    // The provider's documented refusals.
    ProviderUnsupported,
    ProviderModelUnavailable,
    ProviderEffortUnavailable,
    ProviderMessageTooLong,
    ProviderMessageEmpty,
    ProviderProjectNotFound,
    ProviderRequestIdConflict,
    ProviderDiscoveryUnavailable,
}

impl RefusalReason {
    pub fn words(self) -> &'static str {
        match self {
            Self::NoOptions => "CloudRoom offers no Cloud options right now",
            Self::HarnessNotOffered => "the harness is not offered",
            Self::ModelNotOffered => "the model is not offered for this harness",
            Self::EffortNotOffered => "the effort is not offered for this model",
            Self::ProjectNotFound => "no project has this id or name",
            Self::ProjectAmbiguous => {
                "more than one project has this name; use its id or name~<last 6 of its id>"
            }
            Self::ProjectPersonal => "personal projects are refused",
            Self::ProjectNotStandard => "only standard projects are accepted",
            Self::ProjectNoGithubRemote => {
                "the project has no GitHub remote, and only projects with one are accepted"
            }
            Self::ProjectRemoteNotConfirmed => {
                "whether the project has a GitHub remote could not be read"
            }
            Self::ProviderUnsupported => "CloudRoom refused: Cloud does not support this choice",
            Self::ProviderModelUnavailable => {
                "CloudRoom refused: the model is unavailable on Cloud"
            }
            Self::ProviderEffortUnavailable => "CloudRoom refused: the effort is unavailable",
            Self::ProviderMessageTooLong => "CloudRoom refused: the prompt is too long",
            Self::ProviderMessageEmpty => "CloudRoom refused: the prompt is empty",
            Self::ProviderProjectNotFound => "CloudRoom refused: the project was not found",
            Self::ProviderRequestIdConflict => {
                "CloudRoom refused: this request id was already used with different content"
            }
            Self::ProviderDiscoveryUnavailable => {
                "CloudRoom refused: its Cloud model discovery is unavailable"
            }
        }
    }
}

impl From<ProviderRefusal> for RefusalReason {
    fn from(refusal: ProviderRefusal) -> Self {
        match refusal {
            ProviderRefusal::Unsupported => Self::ProviderUnsupported,
            ProviderRefusal::ModelUnavailable => Self::ProviderModelUnavailable,
            ProviderRefusal::EffortUnavailable => Self::ProviderEffortUnavailable,
            ProviderRefusal::MessageTooLong => Self::ProviderMessageTooLong,
            ProviderRefusal::MessageEmpty => Self::ProviderMessageEmpty,
            ProviderRefusal::ProjectNotFound => Self::ProviderProjectNotFound,
            ProviderRefusal::RequestIdConflict => Self::ProviderRequestIdConflict,
            ProviderRefusal::DiscoveryUnavailable => Self::ProviderDiscoveryUnavailable,
        }
    }
}

/// Why nothing was sent, for a reason of OpenWiz's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailReason {
    /// The process stopped after the claim and before the dispatching mark.
    InterruptedBeforeDispatch,
    /// The project list could not be read.
    ProjectsNotRead,
    /// The dispatching mark could not be written.
    RecordNotWritten,
    /// The budget ran out before the call.
    BudgetExhausted,
    /// The provider's command could not be started.
    CallNotStarted,
}

impl FailReason {
    pub fn words(self) -> &'static str {
        match self {
            Self::InterruptedBeforeDispatch => {
                "interrupted before dispatch: the process stopped after claiming the op id and \
                 before the dispatching mark, so nothing was sent"
            }
            Self::ProjectsNotRead => "the project list could not be read; nothing was sent",
            Self::RecordNotWritten => "the dispatching mark could not be written; nothing was sent",
            Self::BudgetExhausted => "the time budget ran out before the call; nothing was sent",
            Self::CallNotStarted => "the provider's command could not be started; nothing was sent",
        }
    }
}

/// What the create came to, in the core's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// CloudRoom recorded the thread and its first prompt. **Not** that it
    /// started: the start stage is read separately.
    Created { thread_id: String, text: String },
    /// Nothing was created. `offered` lists what is offered instead, for a
    /// choice that isn't.
    Refused {
        reason: RefusalReason,
        offered: Vec<String>,
        text: String,
    },
    /// Nothing was sent.
    Failed { reason: FailReason, text: String },
    /// The thread may or may not have been created. The prompt is kept, and
    /// nothing is retried.
    Uncertain {
        reason: UncertainReason,
        text: String,
    },
}

/// The overall status of a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Created,
    Refused,
    Failed,
    Uncertain,
    InProgress,
    ConflictingReuse,
}

/// The words of a created outcome.
pub const CREATED_WORDS: &str = "created: CloudRoom recorded the thread and its first prompt. \
     That is not a start: the start stage is shown separately";

impl Outcome {
    pub fn created(thread_id: String) -> Self {
        Self::Created {
            thread_id,
            text: CREATED_WORDS.to_owned(),
        }
    }

    /// The same outcome, saying that `count` attachments went with it when
    /// the thread was created, and that their delivery to the agent's
    /// machine is not reported . Unchanged for no attachment, or any other
    /// outcome.
    pub fn with_attachments(self, count: usize) -> Self {
        match self {
            Self::Created { thread_id, text } if count > 0 => Self::Created {
                thread_id,
                text: format!("{text}{}", crate::prompt::handed_over(count)),
            },
            other => other,
        }
    }

    pub fn refused(reason: RefusalReason) -> Self {
        Self::refused_because(reason, reason.words(), vec![])
    }

    pub fn refused_because(reason: RefusalReason, words: &str, offered: Vec<String>) -> Self {
        Self::Refused {
            reason,
            offered,
            text: format!("refused: {words}; nothing was created"),
        }
    }

    pub fn failed(reason: FailReason) -> Self {
        Self::Failed {
            reason,
            text: format!("failed: {}", reason.words()),
        }
    }

    pub fn uncertain(reason: UncertainReason) -> Self {
        Self::Uncertain {
            reason,
            text: format!(
                "uncertain: {}. The thread may or may not have been created; the prompt is kept \
                 and nothing is retried",
                reason.words()
            ),
        }
    }

    /// The outcome of the provider's reply.
    pub fn from_reply(reply: SpawnReply) -> (Self, Vec<Gap>) {
        match reply {
            SpawnReply::Created { thread_id } => (Self::created(thread_id), vec![]),
            SpawnReply::Refused(refusal) => (Self::refused(refusal.into()), vec![]),
            SpawnReply::NotStarted => (Self::failed(FailReason::CallNotStarted), vec![]),
            SpawnReply::Unclear { reason, gaps } => (Self::uncertain(reason), gaps),
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Created { text, .. }
            | Self::Refused { text, .. }
            | Self::Failed { text, .. }
            | Self::Uncertain { text, .. } => text,
        }
    }

    pub fn status(&self) -> Status {
        match self {
            Self::Created { .. } => Status::Created,
            Self::Refused { .. } => Status::Refused,
            Self::Failed { .. } => Status::Failed,
            Self::Uncertain { .. } => Status::Uncertain,
        }
    }
}
