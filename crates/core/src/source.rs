//! The interface an adapter implements to supply sessions.
//!
//! Adapters translate their service's data into these records. They never
//! decide grouping, visibility, paging, limits or what counts as complete:
//! that is the core's job.

use std::fmt;
use std::time::Instant;

use crate::envelope::Gap;
use crate::model::{Fact, ProjectId, SessionId, Timestamp};

/// Which sessions to list. Hidden and archived sessions are read only on request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionScope {
    pub include_hidden: bool,
    pub include_archived: bool,
}

/// Records read from one list, plus rows that could not be read at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rows<T> {
    pub records: Vec<T>,
    /// For sessions: one gap for each row that had no readable id (field
    /// problems go in each record's `problems`). Projects and machines have no
    /// `problems` of their own, so their field problems are here too; the
    /// core reports every one of these as a gap.
    pub unreadable: Vec<Gap>,
}

/// Where the source says a session runs, before it is checked against the
/// machine list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    Cloud,
    Host(String),
}

/// One session as the source reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: SessionId,
    pub project_id: Fact<ProjectId>,
    pub title: Fact<String>,
    pub provider: Fact<String>,
    pub status: Fact<String>,
    pub hidden: Fact<bool>,
    pub archived: Fact<bool>,
    pub parent: Fact<Option<SessionId>>,
    pub placement: Fact<Placement>,
    /// `Known(None)` when the source has no purpose for the session.
    pub purpose_text: Fact<Option<String>>,
    pub updated_at: Fact<Timestamp>,
    /// Missing or mistyped fields in this record.
    pub problems: Vec<Gap>,
}

/// One session in detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionDetailRecord {
    pub session: SessionRecord,
    pub model: Fact<String>,
    pub effort: Fact<String>,
}

/// A session's final output as the source reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputRecord {
    /// `Known(None)` when there is no final output yet.
    pub output: Fact<Option<String>>,
    /// An error reported in the output body.
    pub error: Fact<Option<String>>,
    pub problems: Vec<Gap>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: ProjectId,
    pub name: Fact<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineRecord {
    pub id: String,
    pub name: Fact<String>,
}

/// Why a read failed as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceError {
    /// The source said the item does not exist.
    NotFound { subject: String },
    /// The id cannot be passed to the source safely, so nothing was read.
    InvalidId { id: String, detail: String },
    /// The read exited non-zero. `detail` is the source's first error line.
    CommandFailed {
        exit_code: Option<i32>,
        detail: String,
    },
    /// The read did not finish before its deadline.
    TimedOut { after_ms: u64 },
    /// The response was not the expected JSON at all.
    Malformed { detail: String },
    /// The source could not be started.
    Unavailable { detail: String },
}

impl fmt::Display for SourceError {
    /// Reads after the read's name, such as `thread show exited 1: …`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { subject } => write!(f, "reported {subject} not found"),
            Self::InvalidId { id, detail } => write!(f, "was not run for the id {id}: {detail}"),
            Self::CommandFailed {
                exit_code: Some(code),
                detail,
            } => write!(f, "exited {code}: {detail}"),
            Self::CommandFailed {
                exit_code: None,
                detail,
            } => write!(f, "ended without an exit code: {detail}"),
            Self::TimedOut { after_ms } => write!(f, "timed out after {after_ms} ms"),
            Self::Malformed { detail } => write!(f, "returned malformed output: {detail}"),
            Self::Unavailable { detail } => write!(f, "could not be run: {detail}"),
        }
    }
}

/// The reads a source makes, so gaps and messages can name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    Version,
    Sessions,
    Projects,
    Machines,
    SessionDetail,
    SessionOutput,
}

impl Read {
    /// A name for any source. Adapters give their own in
    /// [`SessionSource::read_name`].
    pub fn generic_name(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Sessions => "session list",
            Self::Projects => "project list",
            Self::Machines => "machine list",
            Self::SessionDetail => "session detail",
            Self::SessionOutput => "session output",
        }
    }
}

/// A supplier of sessions, such as the CloudRoom adapter.
///
/// Every read takes a deadline, which the core derives from
/// [`Clock::monotonic`](crate::ops::Clock::monotonic). An implementation
/// returns [`SourceError::TimedOut`] rather than block past it.
pub trait SessionSource: Send + Sync {
    /// The source's name, such as `room-cli`.
    fn name(&self) -> &str;

    /// The version the adapter was tested against.
    fn tested_version(&self) -> &str;

    /// What `read` is called in gaps and messages, such as `thread show`.
    fn read_name(&self, read: Read) -> &str {
        read.generic_name()
    }

    /// The version the source reports now.
    fn version(&self, deadline: Instant) -> Result<String, SourceError>;

    fn list_sessions(
        &self,
        scope: SessionScope,
        deadline: Instant,
    ) -> Result<Rows<SessionRecord>, SourceError>;

    /// Every project, and with `include_hidden` the hidden ones as well.
    fn list_projects(
        &self,
        include_hidden: bool,
        deadline: Instant,
    ) -> Result<Rows<ProjectRecord>, SourceError>;

    fn list_machines(&self, deadline: Instant) -> Result<Rows<MachineRecord>, SourceError>;

    fn session_detail(
        &self,
        id: &SessionId,
        deadline: Instant,
    ) -> Result<SessionDetailRecord, SourceError>;

    fn session_output(
        &self,
        id: &SessionId,
        deadline: Instant,
    ) -> Result<OutputRecord, SourceError>;
}
