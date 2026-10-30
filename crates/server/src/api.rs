//! The routes: query parsing, the calls into the core, the CLI's outcome
//! mapping, and single-flight with a cap on core runs.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

use openwiz_core::envelope::Envelope;
use openwiz_core::limits::RUN_BUDGET;
use openwiz_core::model::SessionId;
use openwiz_core::ops::{self, Clock, Detail, ListRequest, OpError, ShowRequest};
use openwiz_core::source::{Read, SessionSource};
use serde::Serialize;

use crate::Limits;
use crate::http::{API_HEADERS, Reply};

pub(crate) const INVENTORY: &str = "/api/v1/inventory";
pub(crate) const SESSIONS: &str = "/api/v1/sessions/";
pub(crate) const SESSION_ROUTE: &str = "/api/v1/sessions/{id}";
pub(crate) const HEALTH: &str = "/api/v1/health";

/// The route a path names, if any. The id is one path segment, passed to
/// the core as it is: nothing is decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Inventory,
    Session(String),
    Health,
}

impl Route {
    pub fn of(path: &str) -> Option<Self> {
        match path {
            INVENTORY => Some(Self::Inventory),
            HEALTH => Some(Self::Health),
            _ => {
                let id = path.strip_prefix(SESSIONS)?;
                (!id.is_empty() && !id.contains('/')).then(|| Self::Session(id.to_owned()))
            }
        }
    }

    /// The route's template, for the log.
    pub fn template(&self) -> &'static str {
        match self {
            Self::Inventory => INVENTORY,
            Self::Session(_) => SESSION_ROUTE,
            Self::Health => HEALTH,
        }
    }
}

/// One core run. Identical calls share a run.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Call {
    /// `ls` (`long == false`) or `ls -l`, with the CLI's options.
    List {
        long: bool,
        hidden: bool,
        archived: bool,
        limit: usize,
        offset: usize,
    },
    /// `show <id>`.
    Show(String),
}

/// Parses the inventory query. Every parameter is optional, at most once,
/// and strict; the defaults are the CLI's. The limit's cap is the core's.
pub(crate) fn list_call(query: Option<&str>) -> Result<Call, &'static str> {
    let defaults = ListRequest::default();
    let mut long = defaults.detail == Detail::Long;
    let (mut hidden, mut archived) = (defaults.include_hidden, defaults.include_archived);
    let (mut limit, mut offset) = (defaults.limit, defaults.offset);
    let mut seen: Vec<&str> = Vec::new();
    for pair in query
        .filter(|q| !q.is_empty())
        .into_iter()
        .flat_map(|q| q.split('&'))
    {
        let Some((key, value)) = pair.split_once('=') else {
            return Err("malformed query: each parameter is name=value");
        };
        match key {
            "detail" => {
                long = match value {
                    "1" => false,
                    "2" => true,
                    _ => return Err("detail is 1 (ls) or 2 (ls -l)"),
                }
            }
            "hidden" => hidden = flag(value).ok_or("hidden is 0 or 1")?,
            "archived" => archived = flag(value).ok_or("archived is 0 or 1")?,
            "limit" => limit = number(value).ok_or("limit needs a whole number")?,
            "offset" => offset = number(value).ok_or("offset needs a whole number")?,
            _ => return Err("unknown query parameter"),
        }
        if seen.contains(&key) {
            return Err("a query parameter is given more than once");
        }
        seen.push(key);
    }
    Ok(Call::List {
        long,
        hidden,
        archived,
        limit,
        offset,
    })
}

fn flag(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// Decimal digits only: no sign, space, point or encoding.
fn number(value: &str) -> Option<usize> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// A JSON body, as `openwiz … --json` prints it.
fn pretty<T: Serialize>(value: &T) -> Option<String> {
    let mut text = serde_json::to_string_pretty(value).ok()?;
    text.push('\n');
    Some(text)
}

pub(crate) fn ok(body: String) -> Reply {
    Reply {
        status: 200,
        headers: API_HEADERS,
        extra: vec![],
        body: body.into_bytes(),
    }
}

/// An error body: `{"error": {"kind": …, "message": …}}`.
pub(crate) fn error(status: u16, kind: &str, message: &str) -> Reply {
    let body = serde_json::json!({ "error": { "kind": kind, "message": message } });
    Reply {
        status,
        headers: API_HEADERS,
        extra: vec![],
        body: pretty(&body).unwrap_or_default().into_bytes(),
    }
}

pub(crate) fn busy(limits: &Limits) -> Reply {
    let mut reply = error(503, "busy", "too many requests in progress; retry later");
    reply
        .extra
        .push(("Retry-After", limits.retry_after_secs.to_string()));
    reply
}

pub(crate) fn health(source: &dyn SessionSource) -> Reply {
    let body = serde_json::json!({
        "server": { "name": crate::NAME, "version": env!("CARGO_PKG_VERSION") },
        "source": { "name": source.name(), "tested_version": source.tested_version() },
    });
    ok(pretty(&body).unwrap_or_default())
}

/// Runs `call` through the core exactly as the CLI does.
fn run(call: &Call, source: &dyn SessionSource, clock: &dyn Clock) -> Reply {
    let body = match call {
        Call::List {
            long,
            hidden,
            archived,
            limit,
            offset,
        } => {
            let request = ListRequest {
                detail: if *long { Detail::Long } else { Detail::Short },
                include_hidden: *hidden,
                include_archived: *archived,
                limit: *limit,
                offset: *offset,
                budget: RUN_BUDGET,
            };
            ops::list_report(source, clock, &request).map(|report| envelope(&report.envelope))
        }
        Call::Show(id) => {
            let request = ShowRequest {
                id: SessionId(id.clone()),
                budget: RUN_BUDGET,
            };
            ops::show_report(source, clock, &request).map(|report| envelope(&report.envelope))
        }
    };
    match body {
        Ok(Some(body)) => ok(body),
        Ok(None) => error(500, "internal", "the result could not be written"),
        Err(e) => failure(source, &e),
    }
}

fn envelope<T: Serialize>(envelope: &Envelope<T>) -> Option<String> {
    pretty(envelope)
}

/// The status, kind and message for an operation error: the CLI's message,
/// under the status for the CLI's exit code (1 → 502, 2 → 400, 3 → 404).
/// The version is never an error.
fn failure(source: &dyn SessionSource, error_: &OpError) -> Reply {
    match error_ {
        OpError::ListFailed(e) => error(
            502,
            "read_failed",
            &format!("{} {e}", source.read_name(Read::Sessions)),
        ),
        OpError::ShowFailed(e) => error(
            502,
            "read_failed",
            &format!("{} {e}", source.read_name(Read::SessionDetail)),
        ),
        OpError::NotFound(id) => error(404, "not_found", &format!("session {} not found", id.0)),
        OpError::InvalidRequest(why) => error(400, "invalid_request", why),
    }
}

/// A shared core run and its result.
#[derive(Default)]
struct Flight {
    result: Mutex<Option<Reply>>,
    done: Condvar,
}

#[derive(Default)]
struct State {
    flights: HashMap<Call, Arc<Flight>>,
    running: usize,
    attached: usize,
}

/// Core runs: at most `max_core_runs` at once, one per distinct call.
#[derive(Default)]
pub(crate) struct Runs {
    state: Mutex<State>,
    freed: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Runs {
    /// (core runs in progress, requests waiting for a result)
    pub fn counts(&self) -> (usize, usize) {
        let state = lock(&self.state);
        (state.running, state.attached)
    }

    /// The result of `call`: from a run already in progress for the same
    /// call, or from a new run once a slot is free. Without a slot within
    /// the slot wait, `503`; without a result by `deadline`, `504`.
    pub fn call(
        self: &Arc<Self>,
        call: Call,
        deadline: Instant,
        limits: &Limits,
        source: &Arc<dyn SessionSource>,
        clock: &Arc<dyn Clock>,
    ) -> Reply {
        let slot_deadline = deadline.min(Instant::now() + limits.slot_wait);
        let mut state = lock(&self.state);
        let flight = loop {
            if let Some(flight) = state.flights.get(&call) {
                break flight.clone();
            }
            if state.running < limits.max_core_runs {
                let flight = Arc::new(Flight::default());
                if !self.spawn(call.clone(), flight.clone(), source.clone(), clock.clone()) {
                    return error(500, "internal", "the core run could not be started");
                }
                state.running += 1;
                state.flights.insert(call.clone(), flight.clone());
                break flight;
            }
            let left = slot_deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return busy(limits);
            }
            state = self
                .freed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        };
        state.attached += 1;
        drop(state);

        let mut result = lock(&flight.result);
        while result.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            result = flight
                .done
                .wait_timeout(result, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        let reply = result.clone().unwrap_or_else(|| {
            error(
                504,
                "deadline",
                "the result was not ready within the request deadline",
            )
        });
        drop(result);
        lock(&self.state).attached -= 1;
        reply
    }

    /// Starts the core run for `call` on its own thread. The caller holds
    /// the state lock, so the run cannot finish before it is registered.
    fn spawn(
        self: &Arc<Self>,
        call: Call,
        flight: Arc<Flight>,
        source: Arc<dyn SessionSource>,
        clock: Arc<dyn Clock>,
    ) -> bool {
        let runs = self.clone();
        thread::Builder::new()
            .name("openwiz-server core run".to_owned())
            .spawn(move || {
                let reply = panic::catch_unwind(AssertUnwindSafe(|| {
                    run(&call, source.as_ref(), clock.as_ref())
                }))
                .unwrap_or_else(|_| error(500, "internal", "the core run failed"));
                // Unregister first, so a request that arrives from now on
                // starts a new run instead of taking this finished result.
                let mut state = lock(&runs.state);
                state.flights.remove(&call);
                state.running -= 1;
                drop(state);
                runs.freed.notify_all();
                *lock(&flight.result) = Some(reply);
                flight.done.notify_all();
            })
            .is_ok()
    }
}
