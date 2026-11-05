//! The new-thread record on disk (OpenWiz 0.0.4, A1 phase 2): the
//! `new_thread` record kind, in the same state directory as the send
//! record, on the same rules and through the same [`Store`] code: the
//! directory checks, the claim lock, the atomic writes and the recovery
//! on claim are the send record's.
//!
//! Layout, under the state directory (0700):
//!
//! - `new-threads/` (0700);
//! - `new-threads/<op id>.json` (0600): the record, replaced atomically;
//! - `new-threads/<op id>.lock` (0600): the claim, held by the process that
//!   owns the op id until it finishes or stops.
//!
//! A record is read back only through the core's strict reader
//! ([`NewThreadRecord::read`] over a [`StrictJson`] tree), never a derived
//! one: a record that isn't exactly the contract's shape, or doesn't agree
//! with itself, is refused, and nothing is created from it.

use openwiz_core::messaging::OpId;
use openwiz_core::messaging::record::StoreError;
use openwiz_core::model::Timestamp;
use openwiz_core::new_thread::read::StrictJson;
use openwiz_core::new_thread::record::{
    Claim, NewThreadRecord, NewThreadStore, RecordState, recover,
};

use crate::{Area, Claimed, Kind, Store};

/// The new-thread record (OpenWiz 0.0.4, the implementation): `new-threads/`.
#[derive(Debug)]
pub struct NewThreads;

impl Area for NewThreads {
    const DIR: &'static str = "new-threads";
    const WHAT: &'static str = "new-thread record";
}

impl Kind for NewThreads {
    type Record = NewThreadRecord;

    fn op_id(record: &NewThreadRecord) -> &OpId {
        &record.op_id
    }

    fn is_finished(record: &NewThreadRecord) -> bool {
        record.state == RecordState::Finished
    }

    fn is_claimed(record: &NewThreadRecord) -> bool {
        record.state == RecordState::Claimed
    }

    fn recover(record: &NewThreadRecord, now: Timestamp) -> NewThreadRecord {
        recover(record, now)
    }

    fn read(text: &str) -> Result<NewThreadRecord, String> {
        let tree: StrictJson =
            serde_json::from_str(text).map_err(|_| "not JSON, or a key given twice".to_owned())?;
        NewThreadRecord::read(&tree).map_err(|e| e.0)
    }

    fn body(record: &NewThreadRecord) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(record).map_err(|e| e.to_string())
    }
}

/// The file-backed new-thread record.
pub type NewThreadFileStore = Store<NewThreads>;

impl NewThreadStore for Store<NewThreads> {
    fn claim(&self, record: &NewThreadRecord, now: Timestamp) -> Result<Claim, StoreError> {
        Ok(match self.claim_record(record, now)? {
            Claimed::Owned => Claim::Owned,
            Claimed::Existing(r) => Claim::Existing(r),
            Claimed::InProgress(r) => Claim::InProgress(r),
            Claimed::Pending => Claim::Pending,
        })
    }

    fn update(&self, record: &NewThreadRecord) -> Result<(), StoreError> {
        self.update_record(record)
    }

    fn release(&self, op_id: &OpId) {
        self.release_claim(op_id);
    }

    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<NewThreadRecord>, StoreError> {
        self.inspect_record(op_id, now)
    }

    fn withdraw(&self, op_id: &OpId) -> Result<(), StoreError> {
        self.refuse_if_read_only()?;
        let withdrawn = self.withdraw_claimed(op_id);
        // Dropping the lock releases the claim, after the record is gone.
        self.release_claim(op_id);
        withdrawn
    }
}
