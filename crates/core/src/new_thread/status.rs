//! `new_thread_status`: one op id's record and, once created, its start
//! stage. **Read-only:** it never creates, starts, claims or writes
//! anything, and it never calls `spawn`.
//!
//! For an **Uncertain** record it may also list the threads created in that
//! project since the attempt. These are **observations only**: no
//! documented field ties a listed thread to this request, so they never
//! settle the outcome. It stays Uncertain, and the owner resolves it in
//! CloudRoom (no retry in 0.0.4).

use std::time::Duration;

use serde::Serialize;

use crate::messaging::OpId;
use crate::messaging::record::StoreError;
use crate::messaging::send::read_failure;
use crate::model::Timestamp;
use crate::ops::Clock;

use super::STATUS_SCHEMA_ID;
use super::operation::{
    NewThreadClient, NewThreadReport, ObservedThread, from_record, in_flight_report,
};
use super::outcome::{Gap, Outcome};
use super::record::{NewThreadStore, RecordState};
use super::start::{StartView, start_view};

/// Why observations never settle an uncertain create.
pub const OBSERVATIONS_ONLY: &str = "observations only: no documented field ties a listed thread \
     to this request, so the outcome stays uncertain";

/// Threads seen in the project after an uncertain create.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Observations {
    pub read_at: Timestamp,
    /// Created at or after the attempt's claim.
    pub threads: Vec<ObservedThread>,
    /// Always [`OBSERVATIONS_ONLY`].
    pub text: String,
    pub gaps: Vec<Gap>,
}

/// What a status read found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatusResult {
    /// No record has this op id.
    NoRecord,
    Recorded(Box<Recorded>),
}

/// One op id's record, as a status read found it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Recorded {
    pub report: NewThreadReport,
    /// Only for a created thread.
    pub start: Option<StartView>,
    /// Only for an uncertain create.
    pub observations: Option<Observations>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NewThreadStatus {
    /// Always [`STATUS_SCHEMA_ID`].
    pub schema: String,
    pub op_id: OpId,
    pub result: StatusResult,
}

/// The status of `op_id`, read within `budget`. See the module
/// documentation.
pub fn new_thread_status(
    client: &dyn NewThreadClient,
    store: &dyn NewThreadStore,
    clock: &dyn Clock,
    op_id: &OpId,
    budget: Duration,
) -> Result<NewThreadStatus, StoreError> {
    let deadline = clock.monotonic() + budget;
    let status = |result| NewThreadStatus {
        schema: STATUS_SCHEMA_ID.to_owned(),
        op_id: op_id.clone(),
        result,
    };
    let Some(record) = store.inspect(op_id, clock.now())? else {
        return Ok(status(StatusResult::NoRecord));
    };
    if record.state != RecordState::Finished {
        return Ok(status(StatusResult::Recorded(Box::new(Recorded {
            report: in_flight_report(&record),
            start: None,
            observations: None,
        }))));
    }
    let report = from_record(&record, true, vec![]);
    let (start, observations) = match &record.outcome {
        Some(Outcome::Created { thread_id, .. }) => {
            let provider = record
                .provider
                .as_deref()
                .unwrap_or(&record.requested.harness);
            let read = client.show_thread(thread_id, deadline);
            let requested = (
                provider,
                record.requested.model.as_str(),
                record.requested.effort.as_str(),
            );
            (
                Some(start_view(thread_id, requested, read, clock.now())),
                None,
            )
        }
        Some(Outcome::Uncertain { .. }) => {
            let observations = record.project.as_ref().map(|project| {
                match client.threads_since(&project.id, record.claimed_at, deadline) {
                    Ok(threads) => Observations {
                        read_at: clock.now(),
                        threads,
                        text: OBSERVATIONS_ONLY.to_owned(),
                        gaps: vec![],
                    },
                    Err(e) => Observations {
                        read_at: clock.now(),
                        threads: vec![],
                        text: OBSERVATIONS_ONLY.to_owned(),
                        gaps: vec![Gap {
                            subject: "thread list".to_owned(),
                            detail: read_failure("thread list", &e),
                        }],
                    },
                }
            });
            (None, observations)
        }
        _ => (None, None),
    };
    Ok(status(StatusResult::Recorded(Box::new(Recorded {
        report,
        start,
        observations,
    }))))
}
