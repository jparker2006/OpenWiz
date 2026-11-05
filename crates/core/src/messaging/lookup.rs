//! `lookup`: one op id's send record, as a report, or "no record". For
//! "Check again": it never claims the op id, never calls the provider, and
//! never writes (see [`SendStore::inspect`]).
//!
//! A record whose sender stopped is shown as the recovery rule would finish
//! it (failed before the dispatching mark, uncertain after it), in memory
//! only. Nothing is ever redispatched.

use serde::{Deserialize, Serialize};

use crate::model::SessionId;
use crate::ops::Clock;

use super::OpId;
use super::record::{RecordState, SendStore, StoreError};
use super::send::{SendReport, from_record, in_flight_report};

/// The schema id of a lookup. See `$defs/lookup` in
/// `contracts/messaging.v1.schema.json`.
pub const LOOKUP_SCHEMA_ID: &str = "openwiz.send-lookup.v1";

/// What the send record holds for one op id and one target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LookupResult {
    /// No send with this op id was claimed in this send record.
    NoRecord,
    /// This op id is bound to another target: nothing was sent to this one
    /// with it.
    OtherTarget,
    /// The record's report: its outcome (`replayed`), or in progress.
    Recorded { report: Box<SendReport> },
}

/// The result of `lookup`, shown by the CLI as text or as JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupReport {
    /// Always [`LOOKUP_SCHEMA_ID`].
    pub schema: String,
    pub op_id: OpId,
    pub target: SessionId,
    /// In the core's words; says that no call was made.
    pub summary: String,
    pub result: LookupResult,
}

/// Looks up `op_id`'s send to `target`. See the module documentation.
///
/// `store` is `None` when there is no send record yet (nothing was ever
/// claimed): the answer is then "no record".
pub fn lookup(
    store: Option<&dyn SendStore>,
    clock: &dyn Clock,
    target: &SessionId,
    op_id: &OpId,
) -> Result<LookupReport, StoreError> {
    let record = match store {
        Some(store) => store.inspect(op_id, clock.now())?,
        None => None,
    };
    let (summary, result) = match record {
        None => (
            format!(
                "no record: no send with op {op_id} was claimed in this send record, so none \
                 was sent with it from here; no call was made"
            ),
            LookupResult::NoRecord,
        ),
        Some(record) if record.binding.target != *target => (
            format!(
                "op {op_id} is bound to another target, so nothing was sent to {} with it; no \
                 call was made",
                target.0
            ),
            LookupResult::OtherTarget,
        ),
        Some(record) => {
            let report = match record.state {
                RecordState::Finished => from_record(&record, true, vec![]),
                _ => in_flight_report(&record),
            };
            (
                format!("op {op_id}'s record, as it is now; no call was made"),
                LookupResult::Recorded {
                    report: Box::new(report),
                },
            )
        }
    };
    Ok(LookupReport {
        schema: LOOKUP_SCHEMA_ID.to_owned(),
        op_id: op_id.clone(),
        target: target.clone(),
        summary,
        result,
    })
}
