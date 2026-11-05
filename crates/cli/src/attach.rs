//! `--attach <path>` and `--attachment <id>` on `openwiz send` and `openwiz
//! new` (OpenWiz 0.0.4, B2, the implementation): one parser and one preparation
//! for both commands.
//!
//! Each flag may be given any number of times; together they keep their
//! command-line order, which is the owner's order the op id binds (files
//! still go to `room-cli` first, then images). `--attach` stages the file
//! first, exactly as `openwiz attachment add` does (B1): **if any path
//! fails, nothing is read or sent**, every path is reported, and the ones
//! that did stage stay ready for a later `--attachment`. `--attachment`
//! names a file staged before. A path in the text is never an attachment.

use std::ffi::OsString;

use openwiz_core::attachment::{AttachmentId, Kind, PromptAttachment};
use openwiz_core::messaging::record::StoreError;
use openwiz_core::model::Timestamp;
use openwiz_store::attachments::{AttachmentStore, FreeSpace};

use crate::UsageError;
use crate::attachment::{AddReport, stage_paths};

/// Stages a file and attaches it.
pub const ATTACH: &str = "--attach";
/// Attaches a file staged before, by its id.
pub const ATTACHMENT: &str = "--attachment";

/// One `--attach` or `--attachment`, in command-line order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachArg {
    /// A path to stage first.
    Path(String),
    /// A staged attachment's id.
    Id(AttachmentId),
}

/// The attachment flag `flag` with its value, or `None` when `flag` is
/// neither attachment flag.
pub fn parse_flag(flag: &str, value: String) -> Option<Result<AttachArg, UsageError>> {
    match flag {
        ATTACH => Some(if value.is_empty() {
            Err(UsageError(format!("{ATTACH} needs a path")))
        } else {
            Ok(AttachArg::Path(value))
        }),
        ATTACHMENT => Some(AttachmentId::parse(&value).map(AttachArg::Id).map_err(|_| {
            UsageError(format!(
                "{ATTACHMENT} takes an attachment id, as openwiz attachment add or ls shows it"
            ))
        })),
        _ => None,
    }
}

/// Whether `flag` is one of the two attachment flags.
pub fn is_flag(flag: &str) -> bool {
    matches!(flag, ATTACH | ATTACHMENT)
}

/// What preparing the attachments came to.
pub enum Prepared {
    /// Every attachment, staged or named, in command-line order.
    Ids(Vec<AttachmentId>),
    /// A path failed (or the store failed on it): nothing is read or sent.
    /// `attachment add`'s report of every path.
    Failed(AddReport),
}

/// Stages every `--attach` path (all of them, so every one is reported),
/// then gives the ids in command-line order, or the report if any path
/// failed. Nothing here reads a target or sends anything.
pub fn prepare(
    args: &[AttachArg],
    store: &AttachmentStore,
    now: Timestamp,
    free_space: &dyn FreeSpace,
) -> Prepared {
    let paths: Vec<OsString> = args
        .iter()
        .filter_map(|a| match a {
            AttachArg::Path(p) => Some(OsString::from(p)),
            AttachArg::Id(_) => None,
        })
        .collect();
    let report = stage_paths(store, &paths, now, free_space);
    if !report.failed.is_empty() || !report.not_staged.is_empty() {
        return Prepared::Failed(report);
    }
    let mut staged = report.ready.iter();
    let ids = args
        .iter()
        .map(|a| match a {
            AttachArg::Id(id) => id.clone(),
            // In order: the n-th path is the n-th ready one.
            AttachArg::Path(_) => staged
                .next()
                .map(|r| r.staged.attachment.id.clone())
                .expect("every path is ready"),
        })
        .collect();
    Prepared::Ids(ids)
}

/// The exit code of a failed preparation: a path failed (5), or the store
/// failed on one (10).
pub fn failed_code(report: &AddReport) -> i32 {
    report.exit_code()
}

/// The text of a failed preparation: every path, then why nothing went.
pub fn failed_text(report: &AddReport, nothing: &str) -> String {
    format!(
        "{}{nothing}: every attachment must be ready first; the ready ones are kept for \
         --attachment\n",
        report.text()
    )
}

/// One line per attachment, in the owner's order.
pub fn lines(attachments: &[PromptAttachment]) -> String {
    attachments
        .iter()
        .map(|a| {
            let kind = match a.kind {
                Kind::Image => "image",
                Kind::File => "file",
            };
            format!(
                "attachment: {} · {kind} · {} · {} bytes · SHA-256 {}\n",
                a.id.as_str(),
                a.name.as_str(),
                a.size_bytes,
                a.sha256.as_str()
            )
        })
        .collect()
}

/// A store error, as the commands report it.
pub fn store_unusable(error: &StoreError) -> String {
    format!(
        "the attachment store could not be used: {}",
        crate::clean(&error.0)
    )
}
