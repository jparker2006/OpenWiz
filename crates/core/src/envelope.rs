//! The honest envelope around every result: the source and its version,
//! coverage, freshness and known gaps.

use serde::Serialize;

use crate::model::{Fact, Timestamp};

/// The schema id. See `contracts/inventory.v1.schema.json`.
pub const SCHEMA_ID: &str = "openwiz.inventory.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Envelope<T> {
    /// Always [`SCHEMA_ID`].
    pub schema: String,
    pub source: SourceInfo,
    pub coverage: Coverage,
    pub freshness: Freshness,
    pub gaps: Vec<Gap>,
    pub data: T,
}

/// Which source answered, and whether its version is the tested one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceInfo {
    /// Such as `room-cli`.
    pub name: String,
    pub version: Fact<String>,
    /// The version the last live acceptance ran on: evidence, not a gate.
    pub tested_version: String,
    /// True only when `version` is known and equals `tested_version`.
    pub version_tested: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Coverage {
    /// `Complete` only when every read succeeded and nothing is missing.
    pub state: Completeness,
    /// Session counts. Present for lists; absent for `show`.
    pub counts: Option<Counts>,
}

/// How every listed session is accounted for. They reconcile:
/// `listed = unreadable + excluded_hidden + excluded_archived + matched`,
/// and `shown = min(limit, matched - offset)` (zero when `offset >= matched`).
///
/// The counts cover only what the source returned. Without `--hidden` or
/// `-a`, the source is not asked for hidden or archived sessions at all, so
/// how many exist is not known: `hidden_read` and `archived_read` say so.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Counts {
    /// Rows the source returned.
    pub listed: u64,
    /// Rows without a readable, unique id: no id, an id that cannot be
    /// used, or a second row with an id already listed. Each one has a gap.
    pub unreadable: u64,
    /// Hidden rows the source returned anyway and the core left out.
    pub excluded_hidden: u64,
    /// Archived rows the source returned anyway and the core left out.
    pub excluded_archived: u64,
    /// Whether hidden sessions were asked for (`--hidden`).
    pub hidden_read: bool,
    /// Whether archived sessions were asked for (`-a`).
    pub archived_read: bool,
    /// Sessions that pass the visibility filters, before paging.
    pub matched: u64,
    pub offset: u64,
    pub limit: u64,
    pub shown: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Freshness {
    /// When the source was read. Each row's age is measured from here.
    pub snapshot_at: Timestamp,
}

/// Something known to be missing or wrong in this result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Gap {
    pub kind: GapKind,
    /// What it concerns, such as a session id, a field or a command.
    pub subject: Option<String>,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    /// The source's version could not be read, or is not a recognised
    /// version text. A known but untested version is not a gap: it is shown
    /// by `version_tested` (the version is never a gate).
    UntestedVersion,
    /// A read exited non-zero.
    CommandFailed,
    /// A read did not answer in time.
    TimedOut,
    /// The run budget ran out before every read was made.
    BudgetExhausted,
    /// A response was not valid JSON, or was cut short.
    Malformed,
    /// A required field was absent.
    MissingField,
    /// A field had an unexpected type.
    WrongType,
    /// The source said the item does not exist.
    NotFound,
}
