//! New threads: OpenWiz 0.0.4's start of one new Cloud thread in one
//! existing project, with the harness, model and effort the owner chose.
//!
//! **Creation** and **start** are reported apart: a created thread is one
//! CloudRoom recorded, never one that started. The **requested
//! configuration** and what CloudRoom **reports** afterwards are kept apart
//! too: the reported one is never shown as the serving model.
//!
//! Like the rest of the core, this module does no I/O. Reads and the one
//! write go through a [`operation::NewThreadClient`] (the adapter), the
//! durable record through a [`record::NewThreadStore`], and time through a
//! [`Clock`](crate::ops::Clock). Op ids are generated outside the core.
//!
//! A1 phase 2 makes it reachable from the CLI (`openwiz new`,
//! `openwiz models`, `openwiz new --status`), with `thread spawn` and `cloud
//! status` on the allowlist and the record in `openwiz-store`. No server
//! route reaches it until A2.
//!
//! - [`options`]: the Cloud catalogue as the adapter reads it, and the
//! options offered from it;
//! - [`operation`]: `new_thread` and its preview: validation, the op-id
//! claim, the project and options checks, the effects, the one call and
//! the outcome;
//! - [`outcome`]: what the provider's reply means;
//! - [`effects`]: what a create does, worked out before creating;
//! - [`start`]: the start stage and the reported configuration, after
//! creation;
//! - [`status`]: one op id's record and start stage, read-only;
//! - [`record`]: the record, the store interface and the recovery rule;
//! - [`read`]: the record read back from storage, strictly.

pub mod effects;
pub mod operation;
pub mod options;
pub mod outcome;
pub mod read;
pub mod record;
pub mod start;
pub mod status;

use std::time::Duration;

use serde::Serialize;

use crate::messaging::{InvalidRequest, OpId, sha256_hex};
use crate::prompt::PromptPayload;

/// The schema id of a new-thread report. See
/// `contracts/new-thread.v1.schema.json`.
pub const NEW_THREAD_SCHEMA_ID: &str = "openwiz.new-thread.v1";

/// The schema id of a new-thread record.
pub const RECORD_SCHEMA_ID: &str = "openwiz.new-thread-record.v1";

/// The schema id of a status read.
pub const STATUS_SCHEMA_ID: &str = "openwiz.new-thread-status.v1";

/// The schema id of the Cloud options.
pub const OPTIONS_SCHEMA_ID: &str = "openwiz.cloud-options.v1";

/// One create, from the claim to the recorded outcome, finishes within this
/// budget (the HTTP 40 s deadline is the route's, around it). The
/// adapter also stops each single call after
/// [`READ_TIMEOUT`](crate::limits::READ_TIMEOUT).
pub const NEW_THREAD_BUDGET: Duration = Duration::from_secs(30);

/// The longest harness, model or effort accepted.
pub const SETTING_MAX_LEN: usize = 128;

/// The prefix of every request id OpenWiz derives.
pub const REQUEST_ID_PREFIX: &str = "ow1-";

/// What the request id hashes before the op id.
pub const REQUEST_ID_DOMAIN: &str = "openwiz.new-thread.request-id.v1:";

/// The request id passed to the provider for `op_id`: [`REQUEST_ID_PREFIX`]
/// and the first 40 hex digits of the SHA-256 of [`REQUEST_ID_DOMAIN`] and
/// the op id. The same op id always gives the same request id, and it always
/// matches `^[a-zA-Z0-9_-]{1,64}$`.
pub fn request_id(op_id: &OpId) -> String {
    let digest = sha256_hex(format!("{REQUEST_ID_DOMAIN}{op_id}").as_bytes());
    format!("{REQUEST_ID_PREFIX}{}", &digest[..40])
}

/// How the owner names the project: its id, or the inventory's display
/// name (`name`, or `name~<last 6 characters of the id>` when names are
/// shared).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProjectRef {
    Id(String),
    Name(String),
}

impl ProjectRef {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Id(s) | Self::Name(s) => s,
        }
    }
}

/// The harness, model and effort the owner chose. All three are always
/// given, and passed as they are: none is defaulted or substituted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Settings {
    /// The catalogue's harness id, such as `codex`.
    pub harness: String,
    pub model: String,
    pub effort: String,
}

/// One create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewThreadRequest {
    /// Exactly one existing project.
    pub project: ProjectRef,
    pub settings: Settings,
    pub prompt: PromptPayload,
    pub op_id: OpId,
    pub budget: Duration,
}

impl NewThreadRequest {
    pub fn new(
        project: ProjectRef,
        settings: Settings,
        prompt: PromptPayload,
        op_id: OpId,
    ) -> Self {
        Self {
            project,
            settings,
            prompt,
            op_id,
            budget: NEW_THREAD_BUDGET,
        }
    }

    /// Everything that can be checked without a read: the prompt (text
    /// rules, no attachment twice), the project reference and the three
    /// settings.
    pub fn validate(&self) -> Result<(), InvalidRequest> {
        self.prompt.validate()?;
        if self.project.as_str().trim().is_empty() {
            return Err(InvalidRequest("the project is empty".to_owned()));
        }
        if self.project.as_str().contains('\0') {
            return Err(InvalidRequest(
                "the project contains a NUL character".to_owned(),
            ));
        }
        for (name, value) in [
            ("harness", &self.settings.harness),
            ("model", &self.settings.model),
            ("effort", &self.settings.effort),
        ] {
            check_setting(name, value)?;
        }
        Ok(())
    }
}

/// A setting is 1 to [`SETTING_MAX_LEN`] printable ASCII characters, with no
/// space and not starting with `-`. None is ever defaulted.
fn check_setting(name: &str, value: &str) -> Result<(), InvalidRequest> {
    if value.is_empty() {
        return Err(InvalidRequest(format!(
            "the {name} is not given; nothing is defaulted"
        )));
    }
    let valid = value.len() <= SETTING_MAX_LEN
        && !value.starts_with('-')
        && value.bytes().all(|b| b.is_ascii_graphic());
    if valid {
        Ok(())
    } else {
        Err(InvalidRequest(format!(
            "the {name} is 1 to {SETTING_MAX_LEN} printable ASCII characters, with no space and \
             not starting with -"
        )))
    }
}

/// What an op id is bound to: the project, the three settings, the text's
/// SHA-256 and the attachments digest. The text itself is not part of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Binding {
    /// The project as the owner named it, before any read.
    pub project: ProjectRef,
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub text_sha256: String,
    pub attachments_digest: String,
}

/// A part of a binding, named when a reused op id differs in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingField {
    Project,
    Harness,
    Model,
    Effort,
    Text,
    Attachments,
}

impl BindingField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Harness => "harness",
            Self::Model => "model",
            Self::Effort => "effort",
            Self::Text => "text",
            Self::Attachments => "attachments",
        }
    }
}

impl Binding {
    /// The binding of `request`, whose attachments have this digest (B2:
    /// from the gate, or from the op id's record).
    pub fn new(request: &NewThreadRequest, attachments_digest: String) -> Self {
        Self {
            project: request.project.clone(),
            harness: request.settings.harness.clone(),
            model: request.settings.model.clone(),
            effort: request.settings.effort.clone(),
            text_sha256: request.prompt.text_sha256(),
            attachments_digest,
        }
    }

    /// The fields in which `other` differs from this binding.
    pub fn differences(&self, other: &Binding) -> Vec<BindingField> {
        [
            (BindingField::Project, self.project == other.project),
            (BindingField::Harness, self.harness == other.harness),
            (BindingField::Model, self.model == other.model),
            (BindingField::Effort, self.effort == other.effort),
            (BindingField::Text, self.text_sha256 == other.text_sha256),
            (
                BindingField::Attachments,
                self.attachments_digest == other.attachments_digest,
            ),
        ]
        .into_iter()
        .filter(|(_, same)| !same)
        .map(|(field, _)| field)
        .collect()
    }
}

/// The project as the owner sees it in a report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectView {
    pub id: String,
    pub name: crate::model::Fact<String>,
}

/// Phase 1 reads no JSON: every type here and in
/// [`prompt`](crate::prompt) (except B1's `AttachmentId`, whose strict
/// string reader is B1's) is only ever written, so none has a reader, and
/// no reader can accept a shape its contract forbids. A reader added later
/// (phase 2's store record, A2's route body) must be a strict, hand-written
/// one. This test does not compile while any listed type implements
/// serde's reader trait: both impls of `Readable` would apply, and the call is
/// ambiguous.
#[cfg(test)]
mod no_readers {
    trait Readable<Which> {
        fn check() {}
    }

    impl<T: ?Sized> Readable<()> for T {}

    struct HasReader;

    impl<T: serde::de::DeserializeOwned> Readable<HasReader> for T {}

    macro_rules! no_reader {
        ($($t:ty),* $(,)?) => {
            $( <$t as Readable<_>>::check(); )*
        };
    }

    #[test]
    fn no_new_thread_or_prompt_type_has_a_reader() {
        use super::effects::{Effect, EffectKind, Effects};
        use super::operation::{
            NewThreadPreview, NewThreadReport, NewThreadResult, ObservedThread, PreviewVerdict,
        };
        use super::options::{
            AttachmentSupport, CloudOptions, HarnessOption, ModelOption, NoOptionsReason,
            NotOffered, NotOfferedField, OptionsState, UnavailableHarness, UnavailableReason,
        };
        use super::outcome::{FailReason, Outcome, ProviderRefusal, RefusalReason, Status};
        use super::record::{NewThreadRecord, RecordState};
        use super::start::{ReportedConfiguration, ReportedSetting, StartStage, StartView};
        use super::status::{NewThreadStatus, Observations, Recorded, StatusResult};
        use super::{Binding, BindingField, ProjectRef, ProjectView, Settings};
        use crate::prompt::PromptPayload;
        no_reader!(
            PromptPayload,
            Settings,
            ProjectRef,
            Binding,
            BindingField,
            ProjectView,
            AttachmentSupport,
            ModelOption,
            HarnessOption,
            UnavailableReason,
            UnavailableHarness,
            NoOptionsReason,
            OptionsState,
            CloudOptions,
            NotOfferedField,
            NotOffered,
            EffectKind,
            Effect,
            Effects,
            ProviderRefusal,
            RefusalReason,
            FailReason,
            Outcome,
            Status,
            RecordState,
            NewThreadRecord,
            ObservedThread,
            NewThreadResult,
            NewThreadReport,
            PreviewVerdict,
            NewThreadPreview,
            StartStage,
            ReportedSetting,
            ReportedConfiguration,
            StartView,
            Observations,
            StatusResult,
            Recorded,
            NewThreadStatus,
        );
    }
}
