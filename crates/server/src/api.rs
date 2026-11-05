//! The routes: query parsing, the calls into the core, the CLI's outcome
//! mapping, and single-flight with a cap on core runs. The send itself is
//! `send.rs`'s, the create `new_thread.rs`'s, and the attachment routes
//! `attachments.rs`'s: none of them is ever single-flighted.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

use openwiz_core::envelope::Envelope;
use openwiz_core::limits::RUN_BUDGET;
use openwiz_core::messaging::send::Messenger;
use openwiz_core::messaging::{InvalidRequest, Mode, preview};
use openwiz_core::model::SessionId;
use openwiz_core::ops::{self, Clock, Detail, ListRequest, OpError, ShowRequest};
use openwiz_core::source::{Read, SessionSource};
use serde::Serialize;

use crate::http::{API_HEADERS, Reply};
use crate::new_thread::{self, Asked};
use crate::{Attaching, Creating, Limits};

pub(crate) const INVENTORY: &str = "/api/v1/inventory";
pub(crate) const SESSIONS: &str = "/api/v1/sessions/";
pub(crate) const SESSION_ROUTE: &str = "/api/v1/sessions/{id}";
pub(crate) const PREVIEW_ROUTE: &str = "/api/v1/sessions/{id}/preview";
pub(crate) const LOOKUP_ROUTE: &str = "/api/v1/sessions/{id}/messages/{op_id}";
pub(crate) const SEND_ROUTE: &str = "/api/v1/sessions/{id}/messages";
pub(crate) const HEALTH: &str = "/api/v1/health";
pub(crate) const CLOUD_OPTIONS: &str = "/api/v1/cloud-options";
pub(crate) const PROJECTS: &str = "/api/v1/projects/";
pub(crate) const THREADS_ROUTE: &str = "/api/v1/projects/{id}/threads";
pub(crate) const THREADS_PREVIEW_ROUTE: &str = "/api/v1/projects/{id}/threads/preview";
pub(crate) const NEW_THREADS: &str = "/api/v1/new-threads/";
pub(crate) const NEW_THREAD_STATUS_ROUTE: &str = "/api/v1/new-threads/{op_id}";
pub(crate) const ATTACHMENTS_ROUTE: &str = "/api/v1/attachments";
pub(crate) const ATTACHMENTS: &str = "/api/v1/attachments/";
pub(crate) const ATTACHMENT_ROUTE: &str = "/api/v1/attachments/{id}";

/// The route a path names, if any. The id is one path segment, passed to
/// the core as it is: nothing is decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Inventory,
    Session(String),
    /// `GET`: what a send to the session would do.
    Preview(String),
    /// `GET`: the send record of an op id for the session.
    Lookup(String, String),
    /// `POST`: the send.
    Send(String),
    Health,
    /// `GET`: the Cloud options.
    CloudOptions,
    /// `POST`: the create in a project.
    Threads(String),
    /// `POST`, never a write: what a create in a project would do.
    ThreadsPreview(String),
    /// `GET`: the new-thread record of an op id.
    NewThreadStatus(String),
    /// `GET`: the staged attachments; `POST`: one upload.
    Attachments,
    /// `DELETE`: the removal of one staged attachment.
    Attachment(String),
}

impl Route {
    pub fn of(path: &str) -> Option<Self> {
        match path {
            INVENTORY => Some(Self::Inventory),
            HEALTH => Some(Self::Health),
            CLOUD_OPTIONS => Some(Self::CloudOptions),
            ATTACHMENTS_ROUTE => Some(Self::Attachments),
            _ if path.starts_with(ATTACHMENTS) => {
                let id = &path[ATTACHMENTS.len()..];
                (!id.is_empty() && !id.contains('/')).then(|| Self::Attachment(id.to_owned()))
            }
            _ if path.starts_with(PROJECTS) => {
                let parts: Vec<&str> = path[PROJECTS.len()..].split('/').collect();
                match parts[..] {
                    [id, "threads"] if !id.is_empty() => Some(Self::Threads(id.to_owned())),
                    [id, "threads", "preview"] if !id.is_empty() => {
                        Some(Self::ThreadsPreview(id.to_owned()))
                    }
                    _ => None,
                }
            }
            _ if path.starts_with(NEW_THREADS) => {
                let op_id = &path[NEW_THREADS.len()..];
                (!op_id.is_empty() && !op_id.contains('/'))
                    .then(|| Self::NewThreadStatus(op_id.to_owned()))
            }
            _ => {
                let rest = path.strip_prefix(SESSIONS)?;
                let parts: Vec<&str> = rest.split('/').collect();
                if parts.iter().any(|p| p.is_empty()) {
                    return None;
                }
                let id = parts[0].to_owned();
                match parts[1..] {
                    [] => Some(Self::Session(id)),
                    ["preview"] => Some(Self::Preview(id)),
                    ["messages"] => Some(Self::Send(id)),
                    ["messages", op_id] => Some(Self::Lookup(id, op_id.to_owned())),
                    _ => None,
                }
            }
        }
    }

    /// The route's template, for the log.
    pub fn template(&self) -> &'static str {
        match self {
            Self::Inventory => INVENTORY,
            Self::Session(_) => SESSION_ROUTE,
            Self::Preview(_) => PREVIEW_ROUTE,
            Self::Lookup(..) => LOOKUP_ROUTE,
            Self::Send(_) => SEND_ROUTE,
            Self::Health => HEALTH,
            Self::CloudOptions => CLOUD_OPTIONS,
            Self::Threads(_) => THREADS_ROUTE,
            Self::ThreadsPreview(_) => THREADS_PREVIEW_ROUTE,
            Self::NewThreadStatus(_) => NEW_THREAD_STATUS_ROUTE,
            Self::Attachments => ATTACHMENTS_ROUTE,
            Self::Attachment(_) => ATTACHMENT_ROUTE,
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
    /// `send <id> --preview`, with `--steer` when `steer`.
    Preview { id: String, steer: bool },
    /// `models`.
    CloudOptions,
    /// `new --project-id <id> …` without `--yes`: the new-thread preview.
    NewThreadPreview(Box<Asked>),
    /// `new --status <op_id>`.
    NewThreadStatus(String),
}

/// Parses the preview query: `mode=queue` or `mode=steer`, at most once.
pub(crate) fn preview_call(id: &str, query: Option<&str>) -> Result<Call, &'static str> {
    let steer = match query {
        None | Some("") | Some("mode=queue") => false,
        Some("mode=steer") => true,
        Some(_) => return Err("the preview takes only mode=queue or mode=steer"),
    };
    Ok(Call::Preview {
        id: id.to_owned(),
        steer,
    })
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
pub(crate) fn pretty<T: Serialize>(value: &T) -> Option<String> {
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

/// What a core run calls: the source, the send provider and the
/// new-thread provider (when configured), and the clock.
#[derive(Clone)]
pub(crate) struct Core {
    pub source: Arc<dyn SessionSource>,
    pub messenger: Option<Arc<dyn Messenger>>,
    pub creating: Option<Creating>,
    pub attaching: Option<Attaching>,
    pub clock: Arc<dyn Clock>,
}

/// Runs `call` through the core exactly as the CLI does.
fn run(call: &Call, core: &Core) -> Reply {
    let (source, clock) = (core.source.as_ref(), core.clock.as_ref());
    let messenger = core.messenger.as_deref();
    let creating = core.creating.as_ref();
    let body = match call {
        Call::CloudOptions | Call::NewThreadPreview(_) | Call::NewThreadStatus(_) => {
            let Some(creating) = creating else {
                return error(404, "no_route", "no such route");
            };
            return match call {
                Call::CloudOptions => new_thread::run_options(creating, clock),
                Call::NewThreadPreview(asked) => {
                    new_thread::run_preview(creating, core.attaching.as_ref(), clock, asked)
                }
                Call::NewThreadStatus(op_id) => new_thread::run_status(creating, clock, op_id),
                _ => unreachable!("matched above"),
            };
        }
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
        Call::Preview { id, steer } => {
            let Some(messenger) = messenger else {
                return error(404, "no_route", "no such route");
            };
            let mode = if *steer { Mode::Steer } else { Mode::Queue };
            return match preview::preview(messenger, clock, &SessionId(id.clone()), mode) {
                Ok(report) => pretty(&report).map_or_else(
                    || error(500, "internal", "the result could not be written"),
                    ok,
                ),
                Err(InvalidRequest(why)) => error(400, "invalid_request", &why),
            };
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
        core: &Core,
    ) -> Reply {
        let slot_deadline = deadline.min(Instant::now() + limits.slot_wait);
        let mut state = lock(&self.state);
        let flight = loop {
            if let Some(flight) = state.flights.get(&call) {
                break flight.clone();
            }
            if state.running < limits.max_core_runs {
                let flight = Arc::new(Flight::default());
                let spawned = self.spawn(call.clone(), flight.clone(), core.clone());
                if !spawned {
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
    fn spawn(self: &Arc<Self>, call: Call, flight: Arc<Flight>, core: Core) -> bool {
        let runs = self.clone();
        thread::Builder::new()
            .name("openwiz-server core run".to_owned())
            .spawn(move || {
                let reply = panic::catch_unwind(AssertUnwindSafe(|| run(&call, &core)))
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
