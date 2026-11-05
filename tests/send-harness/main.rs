//! A store-level send harness. Test-only: never built into `openwiz`.
//!
//! It calls the core's `send` directly, the way a server would (there is no
//! server send route until the implementation), with the file-backed send record and
//! the CloudRoom adapter pointed at the fake `room-cli`. M8 races it against
//! `openwiz send` processes on one op id.
//!
//! For M9 it can also stop itself (`abort`) at a step no `room-cli` call
//! marks: right after the claim, right after the dispatching mark, or right
//! after the call returns and before the outcome is recorded. These hooks
//! exist only in this test binary.
//!
//! Usage: `send-harness <state dir> <room-cli> <target> <queue|steer> <op id>
//! <none|after-claim|after-dispatching-mark|after-call> <text>`. Prints the
//! report as JSON on stdout and exits 0, or prints the error on stderr and
//! exits 1.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_cli::SystemClock;
use openwiz_core::messaging::effects::TargetRecord;
use openwiz_core::messaging::record::{Claim, RecordState, SendRecord, SendStore, StoreError};
use openwiz_core::messaging::send::{self, Messenger, SendRequest, TellReply};
use openwiz_core::messaging::{Binding, InvalidRequest, Mode, OpId};
use openwiz_core::model::{SessionId, Timestamp};
use openwiz_core::source::SourceError;
use openwiz_store::FileStore;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CrashAt {
    None,
    AfterClaim,
    AfterDispatchingMark,
    AfterCall,
}

struct Store {
    inner: FileStore,
    crash_at: CrashAt,
}

impl SendStore for Store {
    fn load(&self, op_id: &OpId) -> Result<Option<SendRecord>, StoreError> {
        self.inner.load(op_id)
    }

    fn claim(&self, record: &SendRecord, now: Timestamp) -> Result<Claim, StoreError> {
        let claim = self.inner.claim(record, now)?;
        if claim == Claim::Owned && self.crash_at == CrashAt::AfterClaim {
            std::process::abort();
        }
        Ok(claim)
    }

    fn update(&self, record: &SendRecord) -> Result<(), StoreError> {
        self.inner.update(record)?;
        if record.state == RecordState::Dispatching
            && self.crash_at == CrashAt::AfterDispatchingMark
        {
            std::process::abort();
        }
        Ok(())
    }

    fn release(&self, op_id: &OpId) {
        self.inner.release(op_id);
    }

    fn uncertain_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError> {
        self.inner.uncertain_matching(binding, except, now)
    }

    fn in_flight_matching(
        &self,
        binding: &Binding,
        except: &OpId,
        now: Timestamp,
    ) -> Result<Vec<SendRecord>, StoreError> {
        self.inner.in_flight_matching(binding, except, now)
    }

    fn inspect(&self, op_id: &OpId, now: Timestamp) -> Result<Option<SendRecord>, StoreError> {
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

impl Messenger for Adapter {
    fn name(&self) -> &str {
        Messenger::name(&self.inner)
    }

    fn tested_version(&self) -> &str {
        Messenger::tested_version(&self.inner)
    }

    fn check_target_id(&self, id: &SessionId) -> Result<(), InvalidRequest> {
        self.inner.check_target_id(id)
    }

    fn version(&self, deadline: Instant) -> Result<String, SourceError> {
        Messenger::version(&self.inner, deadline)
    }

    fn read_target(&self, id: &SessionId, deadline: Instant) -> Result<TargetRecord, SourceError> {
        self.inner.read_target(id, deadline)
    }

    fn tell(&self, id: &SessionId, mode: Mode, text: &str, deadline: Instant) -> TellReply {
        let reply = self.inner.tell(id, mode, text, deadline);
        if self.crash_at == CrashAt::AfterCall {
            std::process::abort();
        }
        reply
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [state, program, target, mode, op_id, crash_at, text] = args.as_slice() else {
        eprintln!("send-harness: expected 7 arguments, got {}", args.len());
        return ExitCode::from(2);
    };
    let crash_at = match crash_at.as_str() {
        "none" => CrashAt::None,
        "after-claim" => CrashAt::AfterClaim,
        "after-dispatching-mark" => CrashAt::AfterDispatchingMark,
        "after-call" => CrashAt::AfterCall,
        other => {
            eprintln!("send-harness: unknown crash point {other}");
            return ExitCode::from(2);
        }
    };
    let mode = if mode == "steer" {
        Mode::Steer
    } else {
        Mode::Queue
    };
    let store = match FileStore::open(Path::new(state)) {
        Ok(inner) => Store { inner, crash_at },
        Err(e) => {
            eprintln!("send-harness: {e}");
            return ExitCode::from(1);
        }
    };
    let op_id = match OpId::parse(op_id) {
        Ok(op_id) => op_id,
        Err(e) => {
            eprintln!("send-harness: {e}");
            return ExitCode::from(1);
        }
    };
    let adapter = Adapter {
        inner: CloudRoomSource::new(program),
        crash_at,
    };
    let mut request = SendRequest::new(SessionId(target.clone()), text.clone(), op_id);
    request.mode = mode;
    match send::send(&adapter, &store, &SystemClock, &request) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string(&report).expect("a report serializes")
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("send-harness: {e:?}");
            ExitCode::from(1)
        }
    }
}
