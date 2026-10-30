//! The inventory model: sessions, projects, locations, status, reported
//! configuration, and unknowns that carry a reason.

use serde::Serialize;

/// A value that is either known or explicitly unknown with a reason.
/// Nothing is guessed: a value that was not read, or did not read cleanly,
/// is `Unknown`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fact<T> {
    Known(T),
    /// The reason the value is unknown, such as "not reported by room-cli".
    Unknown(String),
}

impl<T> Fact<T> {
    /// Transforms a known value; an unknown keeps its reason.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Fact<U> {
        match self {
            Self::Known(value) => Fact::Known(f(value)),
            Self::Unknown(reason) => Fact::Unknown(reason),
        }
    }
}

/// Milliseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

/// A session id as reported by the source. Opaque to the core.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

/// A project id as reported by the source. Opaque to the core.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProjectId(pub String);

/// Text cut to a limit. `truncated` says whether anything was cut.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BoundedText {
    pub text: String,
    pub truncated: bool,
}

impl BoundedText {
    /// The first `max` characters (Unicode scalar values) of `text`.
    pub fn chars(text: &str, max: usize) -> Self {
        match text.char_indices().nth(max) {
            Some((end, _)) => Self {
                text: text[..end].to_owned(),
                truncated: true,
            },
            None => Self {
                text: text.to_owned(),
                truncated: false,
            },
        }
    }

    /// At most `max` bytes of `text`, cut on a character boundary.
    pub fn bytes(text: &str, max: usize) -> Self {
        if text.len() <= max {
            return Self {
                text: text.to_owned(),
                truncated: false,
            };
        }
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            text: text[..end].to_owned(),
            truncated: true,
        }
    }
}

/// Where a session runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Location {
    Cloud,
    /// A local session. `machine` is the machine's name, known only when its
    /// host id appears in the machine list read in the same run.
    Local {
        machine: Fact<String>,
    },
}

/// A session's project, only when verified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Grouping {
    /// The session's project id was seen in the project list of the same run.
    Project { id: ProjectId, name: Fact<String> },
    /// Anything else, with the reason it is not grouped.
    Ungrouped { reason: String },
}

/// The one-line recap: `purpose.text`, labelled "purpose, as reported".
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recap {
    PurposeAsReported(BoundedText),
    NoRecapYet,
}

/// Model and effort as reported by the source. Always labelled reported
/// configuration: it is never the serving model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReportedConfiguration {
    pub model: Fact<String>,
    pub effort: Fact<String>,
}

/// One session in the inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Session {
    pub id: SessionId,
    pub title: Fact<String>,
    pub provider: Fact<String>,
    /// The status word as observed, such as `idle`, `active` or `error`.
    pub status: Fact<String>,
    pub location: Fact<Location>,
    pub group: Grouping,
    /// `Known(None)` is a top-level session.
    pub parent: Fact<Option<SessionId>>,
    pub hidden: Fact<bool>,
    pub archived: Fact<bool>,
    pub recap: Fact<Recap>,
    pub reported_configuration: ReportedConfiguration,
    pub updated_at: Fact<Timestamp>,
    /// Snapshot time minus `updated_at`.
    pub age_ms: Fact<i64>,
}

/// The final output of a session, never a recent-log tail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FinalOutput {
    Reported(BoundedText),
    NoFinalOutputYet,
}

/// One page of the inventory, the result of `ls` and `ls -l`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Inventory {
    pub sessions: Vec<Session>,
}

/// One session in detail, the result of `show`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionView {
    pub session: Session,
    pub final_output: Fact<FinalOutput>,
    /// The error the source reports in the output body, if any.
    pub reported_error: Fact<Option<BoundedText>>,
}
