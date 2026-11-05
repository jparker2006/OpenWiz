//! After creation: the start stage and the reported configuration, from one
//! `thread show` read (no `thread log`).
//!
//! | `thread.status` (and `lastError`) | Start stage |
//! | --- | --- |
//! | `pending` or `starting` | **Waiting to start** |
//! | `active` | **Started:** first turn running, as reported |
//! | `idle` | **Idle, as reported.** Whether the first prompt ran is not confirmed |
//! | `error` | **Start failed**, with `lastError` in CloudRoom's words |
//! | anything else, or a failed read | **unknown**, with the reason |
//!
//! The reported harness, model and effort are **reported configuration**:
//! each is compared with the request and shown as differing when it does,
//! and none is ever presented as the serving model.

use std::time::Duration;

use serde::Serialize;

use crate::limits::cap_field;
use crate::messaging::send::read_failure;
use crate::model::{Fact, Timestamp};
use crate::ops::Clock;
use crate::source::SourceError;

use super::operation::NewThreadClient;
use super::outcome::Gap;

/// The CLI follows the start for at most this many reads.
pub const FOLLOW_MAX_READS: u32 = 10;

///... and for at most this long.
pub const FOLLOW_WINDOW: Duration = Duration::from_secs(60);

/// The wait between two follow reads.
pub const FOLLOW_INTERVAL: Duration = Duration::from_secs(6);

/// The label on every reported configuration.
pub const REPORTED_LABEL: &str =
    "reported configuration: what CloudRoom reports, never the serving model";

/// One new thread as the adapter read it from `thread show`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownThread {
    /// `thread.status`, as reported.
    pub status: Fact<String>,
    /// `thread.lastError`: `Known(None)` when null.
    pub last_error: Fact<Option<String>>,
    /// `thread.providerId`.
    pub provider: Fact<String>,
    /// `execution.model`.
    pub model: Fact<String>,
    /// `execution.reasoningLevel`.
    pub effort: Fact<String>,
    /// Missing or mistyped fields, in OpenWiz's words.
    pub problems: Vec<Gap>,
}

/// Where the start is, as CloudRoom reports it. Separate from creation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartStage {
    WaitingToStart {
        text: String,
    },
    Started {
        text: String,
    },
    IdleAsReported {
        text: String,
    },
    StartFailed {
        /// CloudRoom's own words, cut to the field cap.
        last_error: Fact<String>,
        text: String,
    },
    Unknown {
        reason: String,
        text: String,
    },
}

impl StartStage {
    /// The stage of a thread whose status reads `status`.
    pub fn from_shown(shown: &ShownThread) -> Self {
        let status = match &shown.status {
            Fact::Known(status) => status.as_str(),
            Fact::Unknown(why) => return Self::unknown(why),
        };
        match status {
            "pending" | "starting" => Self::WaitingToStart {
                text: "waiting to start".to_owned(),
            },
            "active" => Self::Started {
                text: "started: its first turn is running, as reported".to_owned(),
            },
            "idle" => Self::IdleAsReported {
                text: "idle, as reported: whether the first prompt ran is not confirmed".to_owned(),
            },
            "error" => {
                let last_error = match &shown.last_error {
                    Fact::Known(Some(error)) => Fact::Known(cap_field(error)),
                    Fact::Known(None) => Fact::Unknown("not reported by CloudRoom".to_owned()),
                    Fact::Unknown(why) => Fact::Unknown(why.clone()),
                };
                Self::StartFailed {
                    last_error,
                    text: "start failed, in CloudRoom's words below; the prompt stays saved in \
                           CloudRoom"
                        .to_owned(),
                }
            }
            _ => Self::unknown("the status is not one OpenWiz knows"),
        }
    }

    pub fn unknown(reason: &str) -> Self {
        Self::Unknown {
            reason: reason.to_owned(),
            text: format!("unknown ({reason})"),
        }
    }

    /// Whether the stage can still change on its own: waiting, or unknown.
    pub fn is_settled(&self) -> bool {
        matches!(
            self,
            Self::Started { .. } | Self::IdleAsReported { .. } | Self::StartFailed { .. }
        )
    }
}

/// One reported setting beside the requested one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReportedSetting {
    pub requested: String,
    pub reported: Fact<String>,
    /// Unknown when the reported value is.
    pub differs_from_requested: Fact<bool>,
    pub text: String,
}

impl ReportedSetting {
    fn new(name: &str, requested: &str, reported: &Fact<String>) -> Self {
        let (differs, text) = match reported {
            Fact::Known(value) if value == requested => {
                (Fact::Known(false), format!("{name} {value}, as requested"))
            }
            Fact::Known(value) => (
                Fact::Known(true),
                format!("{name} {value}: differs from requested ({requested})"),
            ),
            Fact::Unknown(why) => (
                Fact::Unknown(why.clone()),
                format!("{name} unknown ({why}); requested {requested}"),
            ),
        };
        Self {
            requested: requested.to_owned(),
            reported: reported.clone(),
            differs_from_requested: differs,
            text,
        }
    }
}

/// What CloudRoom reports the new thread runs, beside the request. Never
/// the serving model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReportedConfiguration {
    /// Always [`REPORTED_LABEL`].
    pub label: String,
    /// The reported `providerId`, against the requested `spawn` provider.
    pub provider: ReportedSetting,
    pub model: ReportedSetting,
    pub effort: ReportedSetting,
}

/// The start of one created thread, from one read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StartView {
    pub thread_id: String,
    pub read_at: Timestamp,
    pub stage: StartStage,
    /// `None` when the read failed.
    pub reported: Option<ReportedConfiguration>,
    pub gaps: Vec<Gap>,
}

/// The start view of `thread_id` from one `thread show` read, `read`, made
/// at `read_at`, against the requested `provider`, `model` and `effort`.
pub fn start_view(
    thread_id: &str,
    requested: (&str, &str, &str),
    read: Result<ShownThread, SourceError>,
    read_at: Timestamp,
) -> StartView {
    let (provider, model, effort) = requested;
    match read {
        Ok(shown) => StartView {
            thread_id: thread_id.to_owned(),
            read_at,
            stage: StartStage::from_shown(&shown),
            reported: Some(ReportedConfiguration {
                label: REPORTED_LABEL.to_owned(),
                provider: ReportedSetting::new("provider", provider, &shown.provider),
                model: ReportedSetting::new("model", model, &shown.model),
                effort: ReportedSetting::new("effort", effort, &shown.effort),
            }),
            gaps: shown.problems,
        },
        Err(e) => {
            let why = read_failure("thread show", &e);
            StartView {
                thread_id: thread_id.to_owned(),
                read_at,
                stage: StartStage::unknown(&why),
                reported: None,
                gaps: vec![Gap {
                    subject: "thread show".to_owned(),
                    detail: why,
                }],
            }
        }
    }
}

/// How long to wait before the next follow read, after `reads` reads over
/// `elapsed`, the last showing `stage`; `None` to stop and show the last
/// stage. At most [`FOLLOW_MAX_READS`] reads within [`FOLLOW_WINDOW`].
pub fn follow_next(reads: u32, elapsed: Duration, stage: &StartStage) -> Option<Duration> {
    if stage.is_settled() || reads >= FOLLOW_MAX_READS || elapsed + FOLLOW_INTERVAL > FOLLOW_WINDOW
    {
        return None;
    }
    Some(FOLLOW_INTERVAL)
}

/// Follows the start of the created thread `thread_id`: one
/// `thread show` read, then one every [`FOLLOW_INTERVAL`] until the stage
/// settles, [`FOLLOW_MAX_READS`] reads were made or [`FOLLOW_WINDOW`] has
/// passed. Returns the last read's view and the number of reads. `wait`
/// waits between reads (a real sleep in the CLI, a recorder in the tests).
/// Reads only: it never creates, starts or retries anything.
pub fn follow_start(
    client: &dyn NewThreadClient,
    clock: &dyn Clock,
    thread_id: &str,
    requested: (&str, &str, &str),
    wait: &dyn Fn(Duration),
) -> (StartView, u32) {
    let started = clock.monotonic();
    let deadline = started + FOLLOW_WINDOW;
    let mut waited = Duration::ZERO;
    let mut reads = 0;
    loop {
        let read = client.show_thread(thread_id, deadline);
        reads += 1;
        let view = start_view(thread_id, requested, read, clock.now());
        // A real wait shows on the monotonic clock; a test's recorded one
        // only in `waited`.
        let elapsed = clock
            .monotonic()
            .saturating_duration_since(started)
            .max(waited);
        match follow_next(reads, elapsed, &view.stage) {
            Some(pause) => {
                wait(pause);
                waited += pause;
            }
            None => return (view, reads),
        }
    }
}
