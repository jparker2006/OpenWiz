//! Shared helpers for the new-thread proofs: the fixtures, a fixed clock,
//! an in-memory record store (test-only, never shipped: phase 2 adds the
//! real record kind to `openwiz-store`) and a fake client that answers the
//! core through the adapter's own parsers, so the core and the adapter are
//! proven together on the same fixtures.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use openwiz_adapter_cloudroom::new_thread::{
    CLI_REASONING_LEVELS, parse_catalogue, parse_shown_thread, spawn_argv, spawn_error,
    spawn_receipt,
};
use openwiz_core::messaging::OpId;
use openwiz_core::messaging::record::StoreError;
use openwiz_core::model::{Fact, Timestamp};
use openwiz_core::new_thread::effects::ProjectCandidate;
use openwiz_core::new_thread::operation::{
    NewThreadClient, NewThreadError, NewThreadReport, ObservedThread, SpawnRequest, new_thread,
};
use openwiz_core::new_thread::options::Catalogue;
use openwiz_core::new_thread::outcome::SpawnReply;
use openwiz_core::new_thread::record::{
    Claim, NewThreadRecord, NewThreadStore, RecordState, recover,
};
use openwiz_core::new_thread::start::ShownThread;
use openwiz_core::new_thread::{NewThreadRequest, ProjectRef, Settings};
use openwiz_core::ops::Clock;
use openwiz_core::prompt::PromptPayload;
use openwiz_core::source::SourceError;
use serde_json::Value;

/// The fixed wall-clock time the fixtures are written against.
pub const T0: Timestamp = Timestamp(1_700_000_600_000);

pub const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

pub const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/room-cli-v127/new-thread"
);

pub const SCHEMA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../contracts/new-thread.v1.schema.json"
);

pub const PROMPT_SCHEMA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../contracts/prompt.v1.schema.json"
);

pub struct FixedClock(pub Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn monotonic(&self) -> Instant {
        Instant::now()
    }
}

pub fn clock() -> FixedClock {
    FixedClock(T0)
}

pub fn fixture(name: &str) -> Vec<u8> {
    fs::read(Path::new(FIXTURES).join(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

pub fn fixture_json(name: &str) -> Value {
    serde_json::from_slice(&fixture(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

pub fn op(id: &str) -> OpId {
    OpId::parse(id).unwrap_or_else(|e| panic!("op id {id}: {e}"))
}

pub fn settings(harness: &str, model: &str, effort: &str) -> Settings {
    Settings {
        harness: harness.to_owned(),
        model: model.to_owned(),
        effort: effort.to_owned(),
    }
}

/// codex, its first model, high: offered by `cloud-status.json`.
pub fn offered() -> Settings {
    settings("codex", "syn-codex-model-01", "high")
}

/// A create in `prj_syn_0001` with `settings` and `text`.
pub fn request(settings: Settings, text: &str, op_id: &str) -> NewThreadRequest {
    let mut r = NewThreadRequest::new(
        ProjectRef::Id("prj_syn_0001".to_owned()),
        settings,
        PromptPayload::text(text),
        op(op_id),
    );
    r.budget = Duration::from_secs(5);
    r
}

fn candidate(id: &str, name: &str, kind: &str, github: bool) -> ProjectCandidate {
    ProjectCandidate {
        id: id.to_owned(),
        name: Fact::Known(name.to_owned()),
        kind: Fact::Known(kind.to_owned()),
        github_remote: Fact::Known(github),
    }
}

/// The synthetic projects: one ready, one without a GitHub remote, one
/// personal, two sharing a name.
pub fn projects() -> Vec<ProjectCandidate> {
    vec![
        candidate("prj_syn_0001", "Synthetic project one", "standard", true),
        candidate("prj_syn_0002", "Synthetic project two", "standard", false),
        candidate("prj_syn_0003", "Synthetic personal", "personal", false),
        candidate("prj_syn_0004", "Synthetic shared", "standard", true),
        candidate("prj_syn_0005", "Synthetic shared", "standard", true),
    ]
}

/// How the fake answers the one call.
#[derive(Clone, Debug)]
pub enum SpawnAnswer {
    /// Exit 0 with this fixture's bytes, through the adapter's receipt
    /// parser.
    Receipt(&'static str),
    /// A failed call, through the adapter's error parser.
    Error(Option<i32>, String),
    /// A reply as it is (a timeout, a call that never started).
    Reply(SpawnReply),
}

/// A fake client: every answer goes through the adapter's parsers. It
/// counts every call, and keeps the exact `spawn` arguments the adapter
/// builds for each create.
pub struct FakeClient {
    pub version: Result<String, SourceError>,
    pub projects: Result<Vec<ProjectCandidate>, SourceError>,
    pub catalogue: Result<Vec<u8>, SourceError>,
    pub spawn: SpawnAnswer,
    pub shown: BTreeMap<String, Vec<u8>>,
    pub threads: Result<Vec<ObservedThread>, SourceError>,
    pub version_calls: AtomicUsize,
    pub project_calls: AtomicUsize,
    pub catalogue_calls: AtomicUsize,
    pub spawn_calls: AtomicUsize,
    pub show_calls: AtomicUsize,
    pub list_calls: AtomicUsize,
    pub spawned: Mutex<Vec<(SpawnRequest, Vec<String>)>>,
    /// The store's write count at each spawn, when a store is watched.
    pub writes_at_spawn: Mutex<Vec<usize>>,
    pub watched: Option<&'static MemoryStore>,
}

impl FakeClient {
    pub fn new() -> Self {
        Self {
            version: Ok("v127".to_owned()),
            projects: Ok(projects()),
            catalogue: Ok(fixture("cloud-status.json")),
            spawn: SpawnAnswer::Receipt("spawn-receipt-created.json"),
            shown: BTreeMap::new(),
            threads: Ok(vec![]),
            version_calls: AtomicUsize::new(0),
            project_calls: AtomicUsize::new(0),
            catalogue_calls: AtomicUsize::new(0),
            spawn_calls: AtomicUsize::new(0),
            show_calls: AtomicUsize::new(0),
            list_calls: AtomicUsize::new(0),
            spawned: Mutex::new(vec![]),
            writes_at_spawn: Mutex::new(vec![]),
            watched: None,
        }
    }

    pub fn answering(spawn: SpawnAnswer) -> Self {
        Self {
            spawn,
            ..Self::new()
        }
    }

    pub fn with_catalogue(file: &str) -> Self {
        Self {
            catalogue: Ok(fixture(file)),
            ..Self::new()
        }
    }

    pub fn spawns(&self) -> usize {
        self.spawn_calls.load(Ordering::SeqCst)
    }

    /// Every call of any kind.
    pub fn calls(&self) -> usize {
        [
            &self.version_calls,
            &self.project_calls,
            &self.catalogue_calls,
            &self.spawn_calls,
            &self.show_calls,
            &self.list_calls,
        ]
        .iter()
        .map(|c| c.load(Ordering::SeqCst))
        .sum()
    }

    pub fn last_argv(&self) -> Vec<String> {
        self.spawned
            .lock()
            .unwrap()
            .last()
            .expect("a spawn")
            .1
            .clone()
    }
}

impl NewThreadClient for FakeClient {
    fn name(&self) -> &str {
        "room-cli"
    }

    fn tested_version(&self) -> &str {
        "v127"
    }

    fn cli_levels(&self) -> &[&str] {
        CLI_REASONING_LEVELS
    }

    fn version(&self, _deadline: Instant) -> Result<String, SourceError> {
        self.version_calls.fetch_add(1, Ordering::SeqCst);
        self.version.clone()
    }

    fn projects(&self, _deadline: Instant) -> Result<Vec<ProjectCandidate>, SourceError> {
        self.project_calls.fetch_add(1, Ordering::SeqCst);
        self.projects.clone()
    }

    fn catalogue(&self, _deadline: Instant) -> Result<Catalogue, SourceError> {
        self.catalogue_calls.fetch_add(1, Ordering::SeqCst);
        parse_catalogue(&self.catalogue.clone()?)
    }

    fn spawn(&self, spawn: &SpawnRequest, _deadline: Instant) -> SpawnReply {
        self.spawn_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(store) = self.watched {
            self.writes_at_spawn.lock().unwrap().push(store.writes());
        }
        let argv = spawn_argv(spawn).expect("the core passes a valid spawn");
        self.spawned.lock().unwrap().push((spawn.clone(), argv));
        match &self.spawn {
            SpawnAnswer::Receipt(file) => spawn_receipt(&fixture(file), &spawn.project_id),
            SpawnAnswer::Error(code, line) => spawn_error(*code, line),
            SpawnAnswer::Reply(reply) => reply.clone(),
        }
    }

    fn show_thread(&self, thread_id: &str, _deadline: Instant) -> Result<ShownThread, SourceError> {
        self.show_calls.fetch_add(1, Ordering::SeqCst);
        match self.shown.get(thread_id) {
            Some(bytes) => parse_shown_thread(bytes, thread_id),
            None => Err(SourceError::NotFound {
                subject: thread_id.to_owned(),
            }),
        }
    }

    fn threads_since(
        &self,
        _project_id: &str,
        _since: Timestamp,
        _deadline: Instant,
    ) -> Result<Vec<ObservedThread>, SourceError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        self.threads.clone()
    }
}

/// An in-memory record store: claims are atomic under one lock. A record is
/// "live" while its claimer holds it; [`MemoryStore::stop`] simulates a
/// process that stopped. Test-only.
#[derive(Default)]
pub struct MemoryStore {
    records: Mutex<BTreeMap<OpId, (NewThreadRecord, bool)>>,
    writes: AtomicUsize,
    /// Every version written, in order.
    pub history: Mutex<Vec<NewThreadRecord>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Leaked, so a fake client can watch it for the whole test.
    pub fn leaked() -> &'static Self {
        Box::leak(Box::new(Self::new()))
    }

    pub fn writes(&self) -> usize {
        self.writes.load(Ordering::SeqCst)
    }

    pub fn get(&self, op_id: &OpId) -> Option<NewThreadRecord> {
        self.records
            .lock()
            .unwrap()
            .get(op_id)
            .map(|(r, _)| r.clone())
    }

    /// Puts `record` in place, held by a live process when `live`.
    pub fn seed(&self, record: NewThreadRecord, live: bool) {
        self.records
            .lock()
            .unwrap()
            .insert(record.op_id.clone(), (record, live));
    }

    fn write(&self, record: &NewThreadRecord, live: bool) {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.history.lock().unwrap().push(record.clone());
        self.records
            .lock()
            .unwrap()
            .insert(record.op_id.clone(), (record.clone(), live));
    }
}

impl NewThreadStore for MemoryStore {
    fn claim(&self, record: &NewThreadRecord, now: Timestamp) -> Result<Claim, StoreError> {
        let existing = self.records.lock().unwrap().get(&record.op_id).cloned();
        match existing {
            None => {
                self.write(record, true);
                Ok(Claim::Owned)
            }
            Some((r, true)) if r.state != RecordState::Finished => Ok(Claim::InProgress(r)),
            Some((r, _)) if r.state == RecordState::Finished => Ok(Claim::Existing(r)),
            Some((r, _)) => {
                let recovered = recover(&r, now);
                self.write(&recovered, false);
                Ok(Claim::Existing(recovered))
            }
        }
    }

    fn update(&self, record: &NewThreadRecord) -> Result<(), StoreError> {
        self.write(record, true);
        Ok(())
    }

    fn release(&self, op_id: &OpId) {
        if let Some((_, live)) = self.records.lock().unwrap().get_mut(op_id) {
            *live = false;
        }
    }

    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<NewThreadRecord>, StoreError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .get(op_id)
            .map(|(r, live)| if *live { r.clone() } else { recover(r, now) }))
    }

    fn withdraw(&self, op_id: &OpId) -> Result<(), StoreError> {
        let mut records = self.records.lock().unwrap();
        match records.get(op_id) {
            Some((r, _)) if r.state == RecordState::Claimed => {
                records.remove(op_id);
                Ok(())
            }
            Some(_) => Err(StoreError(format!("op {op_id} is past its claim"))),
            None => Ok(()),
        }
    }
}

/// `new_thread` with the fixed clock, expecting a report.
pub fn create(
    client: &FakeClient,
    store: &MemoryStore,
    request: &NewThreadRequest,
) -> NewThreadReport {
    new_thread(client, store, &clock(), request).expect("a report")
}

/// `new_thread` with the fixed clock.
pub fn try_create(
    client: &FakeClient,
    store: &MemoryStore,
    request: &NewThreadRequest,
) -> Result<NewThreadReport, NewThreadError> {
    new_thread(client, store, &clock(), request)
}

pub fn schema() -> Value {
    serde_json::from_str(&fs::read_to_string(SCHEMA).expect("schema")).expect("JSON")
}

/// Validates `value` against `$defs/<def>` of the new-thread schema.
pub fn validate_def(def: &str, value: &Value) -> Vec<String> {
    let mut schema = schema();
    schema["$ref"] = serde_json::json!(format!("#/$defs/{def}"));
    crate::pattern::validate(&schema, value)
}

/// Every string in `value`, keys included, recursively.
pub fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        Value::Object(map) => map.iter().for_each(|(k, v)| {
            out.push(k.clone());
            strings(v, out)
        }),
        _ => {}
    }
}

/// This run's own private directory, outside any repository.
pub fn private_dir(name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let parent = std::env::temp_dir().join("openwiz-new-thread-tests");
    crate::dirs::private_dir_all(&parent).expect("temp parent");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = parent.join(format!("{name}-{}-{nanos}", std::process::id()));
    crate::dirs::private_dir(&dir).expect("private dir");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("private");
    dir
}
