//! The Cloud options: which harnesses, models and efforts CloudRoom reports
//! right now, and nothing else.
//!
//! The adapter reads `cloud status --json` once and hands the core a
//! [`Catalogue`]: only the catalogue keys, with the account and every other
//! field discarded at its boundary. [`cloud_options`] then
//! offers:
//! - a harness only if the adapter's verified id table maps it to a `spawn`
//! provider, and the catalogue lists its models;
//! - for each model, only the efforts in both its catalogue list and the
//! CLI's verified levels;
//! - **no default**: nothing is marked default or preselected.
//!
//! When `ready` is false, `harnesses` is null, the read fails or its shape
//! is unexpected, there are **no options**, with the reason. Nothing is
//! guessed.

use serde::Serialize;

use crate::messaging::send::read_failure;
use crate::model::Timestamp;
use crate::source::SourceError;

use super::{OPTIONS_SCHEMA_ID, Settings};

/// Shown with every options read: CloudRoom may answer from a saved copy of
/// unknown age.
pub const SAVED_COPY_LABEL: &str = "as reported by CloudRoom; it may answer from a saved copy";

/// What a harness's catalogue row says about attachments, as reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct AttachmentSupport {
    pub files: bool,
    pub images: bool,
}

/// One model of a harness, as the catalogue lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueModel {
    pub model: String,
    pub reasoning_levels: Vec<String>,
}

/// One harness, as the catalogue lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueHarness {
    /// The catalogue's id, such as `codex`.
    pub id: String,
    /// The `spawn --provider` id, only when the adapter's verified table
    /// maps this harness. `None` means the mapping is unverified.
    pub provider: Option<String>,
    /// `None` when the catalogue reports no models (`null`, or absent).
    pub models: Option<Vec<CatalogueModel>>,
    pub attachments: Option<AttachmentSupport>,
    pub steer: Option<bool>,
}

/// The catalogue keys of one `cloud status` read, and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalogue {
    pub ready: bool,
    /// `None` when `harnesses` is null: unknown.
    pub harnesses: Option<Vec<CatalogueHarness>>,
}

/// One effort level offered for a model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelOption {
    pub model: String,
    /// In the catalogue's order. Never empty.
    pub efforts: Vec<String>,
}

/// One harness that can be chosen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HarnessOption {
    /// The catalogue's id: what the owner chooses.
    pub harness: String,
    /// What OpenWiz passes as `spawn --provider`.
    pub provider: String,
    /// In the catalogue's order. Never empty.
    pub models: Vec<ModelOption>,
    /// As reported; `None` when the row doesn't say.
    pub attachments: Option<AttachmentSupport>,
    pub steer: Option<bool>,
}

/// Why a listed harness can't be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The catalogue lists no models for it.
    CatalogueNotReported,
    /// Its `spawn` provider id isn't in the verified table.
    IdMappingNotVerified,
    /// None of its models has an effort both the catalogue and the CLI
    /// offer.
    NoOfferedEffort,
}

impl UnavailableReason {
    pub fn words(self) -> &'static str {
        match self {
            Self::CatalogueNotReported => "unavailable (catalogue not reported)",
            Self::IdMappingNotVerified => "unavailable (its id mapping is not verified)",
            Self::NoOfferedEffort => {
                "unavailable (no effort is in both the catalogue and the CLI's levels)"
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnavailableHarness {
    pub harness: String,
    pub reason: UnavailableReason,
    pub text: String,
}

/// Why there are no options at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoOptionsReason {
    /// `ready` is false.
    NotReady,
    /// `harnesses` is null.
    HarnessesNotReported,
    /// The read failed.
    ReadFailed,
    /// The read's shape isn't the one observed.
    UnexpectedShape,
}

/// Whether anything is offered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OptionsState {
    /// At least one harness can be chosen.
    Offered,
    /// Nothing can be chosen, and why, in OpenWiz's words.
    NoOptions {
        reason: NoOptionsReason,
        text: String,
    },
}

/// The Cloud options, from one catalogue read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CloudOptions {
    /// Always [`OPTIONS_SCHEMA_ID`].
    pub schema: String,
    pub read_at: Timestamp,
    /// Always [`SAVED_COPY_LABEL`].
    pub label: String,
    pub state: OptionsState,
    pub harnesses: Vec<HarnessOption>,
    pub unavailable: Vec<UnavailableHarness>,
}

/// Which part of a choice isn't offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotOfferedField {
    /// Nothing is offered at all.
    Options,
    Harness,
    Model,
    Effort,
}

/// A choice outside the options: what isn't offered, and what is instead.
/// Nothing is substituted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NotOffered {
    pub field: NotOfferedField,
    /// What is offered in its place: harnesses, the harness's models or the
    /// model's efforts. Empty when nothing is offered.
    pub offered: Vec<String>,
    pub text: String,
}

impl CloudOptions {
    /// The `spawn` provider id for `settings`, if CloudRoom offers exactly
    /// that harness, model and effort. Otherwise what isn't offered.
    pub fn offered(&self, settings: &Settings) -> Result<String, NotOffered> {
        if let OptionsState::NoOptions { text, .. } = &self.state {
            return Err(NotOffered {
                field: NotOfferedField::Options,
                offered: vec![],
                text: format!("not offered: no Cloud options ({text})"),
            });
        }
        let Some(harness) = self
            .harnesses
            .iter()
            .find(|h| h.harness == settings.harness)
        else {
            return Err(not_offered(
                NotOfferedField::Harness,
                &settings.harness,
                self.harnesses.iter().map(|h| h.harness.clone()).collect(),
            ));
        };
        let Some(model) = harness.models.iter().find(|m| m.model == settings.model) else {
            return Err(not_offered(
                NotOfferedField::Model,
                &settings.model,
                harness.models.iter().map(|m| m.model.clone()).collect(),
            ));
        };
        if !model.efforts.contains(&settings.effort) {
            return Err(not_offered(
                NotOfferedField::Effort,
                &settings.effort,
                model.efforts.clone(),
            ));
        }
        Ok(harness.provider.clone())
    }
}

fn not_offered(field: NotOfferedField, chosen: &str, offered: Vec<String>) -> NotOffered {
    let what = match field {
        NotOfferedField::Options => "option",
        NotOfferedField::Harness => "harness",
        NotOfferedField::Model => "model",
        NotOfferedField::Effort => "effort",
    };
    NotOffered {
        field,
        text: format!(
            "not offered: the {what} {chosen} is not in CloudRoom's options; offered: {}. \
             Nothing was substituted",
            if offered.is_empty() {
                "none".to_owned()
            } else {
                offered.join(", ")
            }
        ),
        offered,
    }
}

/// The options from one catalogue read, `read`, made at `read_at`.
/// `cli_levels` are the effort levels the CLI's help verifies.
pub fn cloud_options(
    read: Result<Catalogue, SourceError>,
    cli_levels: &[&str],
    read_at: Timestamp,
) -> CloudOptions {
    let none = |reason: NoOptionsReason, text: String| CloudOptions {
        schema: OPTIONS_SCHEMA_ID.to_owned(),
        read_at,
        label: SAVED_COPY_LABEL.to_owned(),
        state: OptionsState::NoOptions { reason, text },
        harnesses: vec![],
        unavailable: vec![],
    };
    let catalogue = match read {
        Ok(catalogue) => catalogue,
        Err(SourceError::Malformed { .. }) => {
            return none(
                NoOptionsReason::UnexpectedShape,
                "cloud status returned a shape OpenWiz does not recognise".to_owned(),
            );
        }
        Err(e) => {
            return none(
                NoOptionsReason::ReadFailed,
                read_failure("cloud status", &e),
            );
        }
    };
    if !catalogue.ready {
        return none(
            NoOptionsReason::NotReady,
            "CloudRoom reports that Cloud is not ready".to_owned(),
        );
    }
    let Some(rows) = catalogue.harnesses else {
        return none(
            NoOptionsReason::HarnessesNotReported,
            "CloudRoom reports no harness catalogue".to_owned(),
        );
    };
    let mut harnesses = Vec::new();
    let mut unavailable = Vec::new();
    for row in rows {
        let refuse = |reason: UnavailableReason| UnavailableHarness {
            harness: row.id.clone(),
            reason,
            text: reason.words().to_owned(),
        };
        let Some(models) = &row.models else {
            unavailable.push(refuse(UnavailableReason::CatalogueNotReported));
            continue;
        };
        let Some(provider) = &row.provider else {
            unavailable.push(refuse(UnavailableReason::IdMappingNotVerified));
            continue;
        };
        let models: Vec<ModelOption> = models
            .iter()
            .filter_map(|m| {
                let efforts: Vec<String> = m
                    .reasoning_levels
                    .iter()
                    .filter(|l| cli_levels.contains(&l.as_str()))
                    .cloned()
                    .collect();
                (!efforts.is_empty()).then(|| ModelOption {
                    model: m.model.clone(),
                    efforts,
                })
            })
            .collect();
        if models.is_empty() {
            unavailable.push(refuse(UnavailableReason::NoOfferedEffort));
            continue;
        }
        harnesses.push(HarnessOption {
            harness: row.id.clone(),
            provider: provider.clone(),
            models,
            attachments: row.attachments,
            steer: row.steer,
        });
    }
    if harnesses.is_empty() {
        return CloudOptions {
            unavailable,
            ..none(
                NoOptionsReason::HarnessesNotReported,
                "no harness in CloudRoom's catalogue can be offered".to_owned(),
            )
        };
    }
    CloudOptions {
        schema: OPTIONS_SCHEMA_ID.to_owned(),
        read_at,
        label: SAVED_COPY_LABEL.to_owned(),
        state: OptionsState::Offered,
        harnesses,
        unavailable,
    }
}
