//! The self-exec harness: this test binary, run again as its own process,
//! calls the core's `new_thread` with the file-backed record and the
//! CloudRoom adapter pointed at the fake, the way a server would (no server
//! route exists until A2). N12 races it against `openwiz new` processes, and
//! N13 stops it at the one step no `room-cli` call marks: right after the
//! dispatching mark.
//!
//! It runs only when [`HARNESS_ENV`] holds its spec (JSON: `state`,
//! `program`, `op_id`, `crash`, `text`, `out`, and B2's optional
//! `attachments`, staged ids passed through the attachment store); in a
//! normal test run `harness_process` returns at once. The report goes to
//! the `out` file, never stdout (the test runner prints there).

use std::path::PathBuf;
use std::process::abort;
use std::time::{Duration, Instant};

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_cli::SystemClock;
use openwiz_core::messaging::OpId;
use openwiz_core::messaging::record::StoreError;
use openwiz_core::model::Timestamp;
use openwiz_core::new_thread::effects::ProjectCandidate;
use openwiz_core::new_thread::operation::{
    NewThreadClient, ObservedThread, SpawnRequest, new_thread_with,
};
use openwiz_core::new_thread::options::Catalogue;
use openwiz_core::new_thread::outcome::SpawnReply;
use openwiz_core::new_thread::record::{Claim, NewThreadRecord, NewThreadStore, RecordState};
use openwiz_core::new_thread::start::ShownThread;
use openwiz_core::new_thread::{NewThreadRequest, ProjectRef, Settings};
use openwiz_core::prompt::{AttachmentId, PromptPayload};
use openwiz_core::source::SourceError;
use openwiz_store::{AttachmentStore, NewThreadFileStore};
use serde_json::Value;

/// The variable that holds the harness's spec.
pub const HARNESS_ENV: &str = "OPENWIZ_NEW_THREAD_HARNESS";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CrashAt {
    None,
    AfterClaim,
    AfterDispatchingMark,
    AfterCall,
}

struct Store {
    inner: NewThreadFileStore,
    crash_at: CrashAt,
}

impl NewThreadStore for Store {
    fn claim(&self, record: &NewThreadRecord, now: Timestamp) -> Result<Claim, StoreError> {
        let claim = self.inner.claim(record, now)?;
        if claim == Claim::Owned && self.crash_at == CrashAt::AfterClaim {
            abort();
        }
        Ok(claim)
    }

    fn update(&self, record: &NewThreadRecord) -> Result<(), StoreError> {
        self.inner.update(record)?;
        if record.state == RecordState::Dispatching
            && self.crash_at == CrashAt::AfterDispatchingMark
        {
            abort();
        }
        Ok(())
    }

    fn release(&self, op_id: &OpId) {
        self.inner.release(op_id);
    }

    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<NewThreadRecord>, StoreError> {
        self.inner.inspect(op_id, now)
    }

    fn withdraw(&self, op_id: &OpId) -> Result<(), StoreError> {
        self.inner.withdraw(op_id)
    }
}

struct Adapter {
    inner: CloudRoomSource,
    crash_at: CrashAt,
}

impl NewThreadClient for Adapter {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn tested_version(&self) -> &str {
        self.inner.tested_version()
    }

    fn cli_levels(&self) -> &[&str] {
        self.inner.cli_levels()
    }

    fn version(&self, deadline: Instant) -> Result<String, SourceError> {
        NewThreadClient::version(&self.inner, deadline)
    }

    fn projects(&self, deadline: Instant) -> Result<Vec<ProjectCandidate>, SourceError> {
        self.inner.projects(deadline)
    }

    fn catalogue(&self, deadline: Instant) -> Result<Catalogue, SourceError> {
        self.inner.catalogue(deadline)
    }

    fn spawn(&self, spawn: &SpawnRequest, deadline: Instant) -> SpawnReply {
        let reply = self.inner.spawn(spawn, deadline);
        if self.crash_at == CrashAt::AfterCall {
            abort();
        }
        reply
    }

    fn show_thread(&self, thread_id: &str, deadline: Instant) -> Result<ShownThread, SourceError> {
        self.inner.show_thread(thread_id, deadline)
    }

    fn threads_since(
        &self,
        project_id: &str,
        since: Timestamp,
        deadline: Instant,
    ) -> Result<Vec<ObservedThread>, SourceError> {
        self.inner.threads_since(project_id, since, deadline)
    }
}

#[test]
fn harness_process() {
    let Ok(spec) = std::env::var(HARNESS_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(&spec).expect("the harness spec");
    let text = |key: &str| spec[key].as_str().expect(key).to_owned();
    let crash_at = match text("crash").as_str() {
        "none" => CrashAt::None,
        "after-claim" => CrashAt::AfterClaim,
        "after-dispatching-mark" => CrashAt::AfterDispatchingMark,
        "after-call" => CrashAt::AfterCall,
        other => panic!("unknown crash point {other}"),
    };
    let state = PathBuf::from(text("state"));
    let store = Store {
        inner: NewThreadFileStore::open(&state).expect("open the record"),
        crash_at,
    };
    let client = Adapter {
        inner: CloudRoomSource::new(text("program")),
        crash_at,
    };
    let mut request = NewThreadRequest::new(
        ProjectRef::Name("Synthetic project one".to_owned()),
        Settings {
            harness: "codex".to_owned(),
            model: "syn-codex-model-01".to_owned(),
            effort: "high".to_owned(),
        },
        PromptPayload::text(text("text")),
        OpId::parse(&text("op_id")).expect("op id"),
    );
    request.budget = Duration::from_secs(20);
    request.prompt.attachments = spec["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|id| AttachmentId::parse(id.as_str().expect("an id")).expect("a valid id"))
        .collect();
    let attachments = AttachmentStore::open(&state).expect("open the attachment store");
    let report =
        new_thread_with(&client, &store, &attachments, &SystemClock, &request).expect("a report");
    std::fs::write(
        text("out"),
        serde_json::to_string(&report).expect("the report serializes"),
    )
    .expect("write the report");
}
