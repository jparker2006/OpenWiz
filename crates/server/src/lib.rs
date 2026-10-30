//! `openwiz-server`: the read-only OpenWiz HTTP API, a thin client of the
//! core like the `openwiz` CLI. See `contracts/web-api.v1.md`.
//!
//! The server calls the core's `list_report` and `show_report` with the
//! CLI's options and prints the CLI's `--json`. It listens on loopback
//! only, answers `GET` on three routes, and bounds everything else: heads,
//! connections, core runs and time. With `--web-root`, it also serves the
//! web page's static export on the same origin (`web.rs`). HTTP is the
//! standard library's `TcpListener` and threads: no HTTP crate is approved.

mod api;
mod http;
mod web;

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use openwiz_core::model::Timestamp;
use openwiz_core::ops::Clock;
use openwiz_core::source::SessionSource;

use crate::api::{Route as Matched, Runs};
use crate::http::{Head, HeadError, Refusal, Reply};

pub use crate::web::{
    MAX_FILE_BYTES, MAX_WEB_BYTES, MAX_WEB_FILES, PAGE_CSP, STATIC_CACHE, WEB_ROUTES, WebRoot,
};

/// The program's name, in `--help` and the health body.
pub const NAME: &str = "openwiz-server";

/// The port when `--port` is not given. Provisional.
pub const DEFAULT_PORT: u16 = 7370;

/// The environment variable that may name the one public host name served.
pub const ALLOWED_HOST_ENV: &str = "OPENWIZ_SERVER_ALLOWED_HOST";

/// Connections handled at once. One more is answered `503` and closed.
pub const MAX_CONNECTIONS: usize = 32;

/// Core runs (`list_report` or `show_report`) at once.
pub const MAX_CORE_RUNS: usize = 2;

/// How long a request waits for a core run slot before `503`.
pub const SLOT_WAIT: Duration = Duration::from_secs(5);

/// `Retry-After` on a `503`, in seconds.
pub const RETRY_AFTER_SECS: u64 = 5;

/// From the end of the request head to the response: above the core's
/// 30-second run budget plus [`SLOT_WAIT`].
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(40);

/// The whole request head must arrive within this time.
pub const HEAD_TIMEOUT: Duration = Duration::from_secs(10);

/// The whole response must be written within this time.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest request target (path and query), in bytes.
pub const MAX_TARGET_BYTES: usize = 1024;

/// The largest request head (request line and headers), in bytes.
pub const MAX_HEAD_BYTES: usize = 16 * 1024;

/// The most header lines in one request.
pub const MAX_HEADER_LINES: usize = 64;

/// One route the server answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub method: &'static str,
    pub path: &'static str,
}

/// Every route the server answers. Anything else is `404` or `405`.
pub const ROUTES: &[Route] = &[
    Route {
        method: "GET",
        path: api::INVENTORY,
    },
    Route {
        method: "GET",
        path: api::SESSION_ROUTE,
    },
    Route {
        method: "GET",
        path: api::HEALTH,
    },
];

/// How long a closed response waits for the client's unread bytes.
const LINGER: Duration = Duration::from_secs(1);

/// Methods named in the log; any other is logged as `-`.
const LOGGED_METHODS: &[&str] = &[
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "CONNECT", "TRACE",
];

/// The server's bounds. [`Limits::default`] is the constants above; tests
/// shorten them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_connections: usize,
    pub max_core_runs: usize,
    pub slot_wait: Duration,
    pub retry_after_secs: u64,
    pub request_deadline: Duration,
    pub head_timeout: Duration,
    pub write_timeout: Duration,
    pub max_target_bytes: usize,
    pub max_head_bytes: usize,
    pub max_header_lines: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            max_core_runs: MAX_CORE_RUNS,
            slot_wait: SLOT_WAIT,
            retry_after_secs: RETRY_AFTER_SECS,
            request_deadline: REQUEST_DEADLINE,
            head_timeout: HEAD_TIMEOUT,
            write_timeout: WRITE_TIMEOUT,
            max_target_bytes: MAX_TARGET_BYTES,
            max_head_bytes: MAX_HEAD_BYTES,
            max_header_lines: MAX_HEADER_LINES,
        }
    }
}

/// The server's configuration. Nothing in it is ever committed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// The one public host name served besides loopback, from local
    /// configuration (`--allowed-host` or [`ALLOWED_HOST_ENV`]).
    pub allowed_host: Option<String>,
    /// The web page's build directory (`--web-root`). Without it, every
    /// non-API path is `404`.
    pub web_root: Option<WebRoot>,
    pub limits: Limits,
}

/// Receives one request log line at a time, without a newline: the method,
/// route, status and time only.
pub type Log = Arc<dyn Fn(&str) + Send + Sync>;

/// What the server is doing now. Counts only, never content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Connections being handled.
    pub connections: usize,
    /// Core runs in progress.
    pub core_runs: usize,
    /// Requests waiting for a core run's result, their own or a shared one.
    pub attached: usize,
}

/// The wall clock and monotonic clock of this machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp(i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
    }

    fn monotonic(&self) -> Instant {
        Instant::now()
    }
}

/// Whether `name` may be the allowed public host name: a DNS name of
/// letters, digits, `-` and `.`, without a port.
pub fn valid_host_name(name: &str) -> bool {
    name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Binds the listening socket: 127.0.0.1 only. There is no other address.
pub fn bind(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
}

struct Shared {
    config: Config,
    source: Arc<dyn SessionSource>,
    clock: Arc<dyn Clock>,
    log: Log,
    port: u16,
    connections: AtomicUsize,
    refusing: AtomicUsize,
    runs: Arc<Runs>,
}

/// A running server.
pub struct Server {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept: JoinHandle<()>,
}

impl Server {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stats(&self) -> Stats {
        let (core_runs, attached) = self.shared.runs.counts();
        Stats {
            connections: self.shared.connections.load(Ordering::SeqCst),
            core_runs,
            attached,
        }
    }

    /// Serves until the process ends.
    pub fn join(self) {
        let _ = self.accept.join();
    }
}

/// Starts serving `listener` on its own thread.
pub fn start(
    listener: TcpListener,
    config: Config,
    source: Arc<dyn SessionSource>,
    clock: Arc<dyn Clock>,
    log: Log,
) -> io::Result<Server> {
    let addr = listener.local_addr()?;
    let shared = Arc::new(Shared {
        config,
        source,
        clock,
        log,
        port: addr.port(),
        connections: AtomicUsize::new(0),
        refusing: AtomicUsize::new(0),
        runs: Arc::new(Runs::default()),
    });
    let serving = shared.clone();
    let accept = thread::Builder::new()
        .name("openwiz-server accept".to_owned())
        .spawn(move || accept_loop(&listener, &serving))?;
    Ok(Server {
        addr,
        shared,
        accept,
    })
}

/// Decrements a counter when dropped.
struct Count<'a>(&'a AtomicUsize);

impl Drop for Count<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>) {
    let max = shared.config.limits.max_connections;
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            // Such as too many open files: wait instead of spinning.
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        let accepted = Instant::now();
        let _ = stream.set_nodelay(true);
        // Over the cap, a connection gets a 503 from a separate, equally
        // capped set of threads; past both caps, the accept loop answers it
        // itself, at once and without blocking (never closed unanswered).
        let (counter, over) = if shared.connections.fetch_add(1, Ordering::SeqCst) < max {
            (&shared.connections, false)
        } else {
            shared.connections.fetch_sub(1, Ordering::SeqCst);
            if shared.refusing.fetch_add(1, Ordering::SeqCst) >= max {
                shared.refusing.fetch_sub(1, Ordering::SeqCst);
                answer_now(&stream, shared, accepted);
                continue;
            }
            (&shared.refusing, true)
        };
        // Kept so that a thread that cannot be started still gets its 503.
        let spare = stream.try_clone();
        let worker = shared.clone();
        let spawned = thread::Builder::new()
            .name("openwiz-server connection".to_owned())
            .spawn(move || {
                let counter = if over {
                    &worker.refusing
                } else {
                    &worker.connections
                };
                let _count = Count(counter);
                connection(stream, &worker, accepted, over);
            });
        if spawned.is_err() {
            counter.fetch_sub(1, Ordering::SeqCst);
            if let Ok(spare) = spare {
                answer_now(&spare, shared, accepted);
            }
        }
    }
}

/// The accept loop's own `503`: see [`http::send_now`].
fn answer_now(stream: &TcpStream, shared: &Shared, accepted: Instant) {
    let reply = api::busy(&shared.config.limits);
    http::send_now(stream, &reply);
    (shared.log)(&format!(
        "- - {} {}ms",
        reply.status,
        accepted.elapsed().as_millis()
    ));
}

/// Handles one connection: one request, one response, then close.
fn connection(mut stream: TcpStream, shared: &Shared, accepted: Instant, over_cap: bool) {
    let limits = &shared.config.limits;
    let (method, route, reply, head_method) = if over_cap {
        ("-", "-", api::busy(limits), peeks_head(&stream))
    } else {
        match http::read_head(&mut stream, limits) {
            Err(Refusal {
                error: HeadError::Closed,
                ..
            }) => return,
            Err(Refusal { error, head_method }) => ("-", "-", head_error(&error), head_method),
            Ok(head) => {
                let method = LOGGED_METHODS
                    .iter()
                    .find(|m| **m == head.method)
                    .copied()
                    .unwrap_or("-");
                let path = head.target.split('?').next().unwrap_or_default();
                let route = match web_route(path, &shared.config) {
                    Some((_, template)) => template,
                    None => Matched::of(path).map_or("-", |r| r.template()),
                };
                let head_method = head.method == "HEAD";
                (method, route, respond(&head, shared), head_method)
            }
        }
    };
    // HTTP forbids content in a response to HEAD, whatever its status (`200`
    // only for a web file; the API refuses it with `405`).
    http::send(
        &mut stream,
        &reply,
        head_method,
        limits.write_timeout,
        LINGER,
    );
    (shared.log)(&format!(
        "{method} {route} {} {}ms",
        reply.status,
        accepted.elapsed().as_millis()
    ));
}

/// Over the cap the head is not read; up to 200 ms of peeking at its first
/// bytes tells whether it is, or may be, a HEAD request.
fn peeks_head(stream: &TcpStream) -> bool {
    let until = Instant::now() + Duration::from_millis(200);
    let mut first = [0u8; 5];
    let mut seen = 0;
    while seen < first.len() {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match stream.peek(&mut first) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                seen = n;
                if seen < first.len() {
                    // Fewer bytes so far: wait a moment for the rest.
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
    http::may_be_head(&first[..seen])
}

fn head_error(error: &HeadError) -> Reply {
    match error {
        HeadError::Timeout => api::error(
            408,
            "request_timeout",
            "the request head was not received in time",
        ),
        HeadError::TargetTooLong => {
            api::error(414, "target_too_long", "the request target is too long")
        }
        HeadError::TooLarge => api::error(431, "head_too_large", "the request head is too large"),
        HeadError::Version => {
            api::error(505, "http_version", "only HTTP/1.1 and HTTP/1.0 are served")
        }
        HeadError::Malformed | HeadError::Closed => {
            api::error(400, "bad_request", "malformed request")
        }
    }
}

/// The web root and route template of a web path, when a web root is
/// configured.
fn web_route<'c>(path: &str, config: &'c Config) -> Option<(&'c WebRoot, &'static str)> {
    let root = config.web_root.as_ref()?;
    web::route(path).map(|template| (root, template))
}

/// The checks, in order: `Host`, method, body, route, query; then the call
/// (or the web file).
fn respond(head: &Head, shared: &Shared) -> Reply {
    let limits = &shared.config.limits;
    let deadline = Instant::now() + limits.request_deadline;
    let hosts = head.values("Host");
    let [host] = hosts[..] else {
        return api::error(400, "bad_request", "a request needs exactly one Host");
    };
    let host_syntax = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b));
    if !host_syntax {
        return api::error(400, "bad_request", "the Host is malformed");
    }
    if !host_allowed(host, shared.port, shared.config.allowed_host.as_deref()) {
        return api::error(421, "misdirected", "this host name is not served here");
    }
    let (path, query) = match head.target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (head.target.as_str(), None),
    };
    let web = web_route(path, &shared.config);
    if web.is_some() && head.method != "GET" && head.method != "HEAD" {
        let mut reply = api::error(405, "method_not_allowed", "only GET and HEAD are allowed");
        reply.extra.push(("Allow", "GET, HEAD".to_owned()));
        return reply;
    }
    if web.is_none() && head.method != "GET" {
        let mut reply = api::error(405, "method_not_allowed", "only GET is allowed");
        reply.extra.push(("Allow", "GET".to_owned()));
        return reply;
    }
    let lengths = head.values("Content-Length");
    let empty_body = match lengths[..] {
        [] => true,
        [length] => !length.is_empty() && length.bytes().all(|b| b == b'0'),
        _ => false,
    };
    if !empty_body || !head.values("Transfer-Encoding").is_empty() {
        return api::error(400, "bad_request", "request bodies are not accepted");
    }
    if let Some((root, template)) = web {
        if query.is_some() {
            return api::error(400, "invalid_request", "this route takes no query");
        }
        return web::serve(root, template, path)
            .unwrap_or_else(|| api::error(404, "no_route", "no such route"));
    }
    let call = match Matched::of(path) {
        None => return api::error(404, "no_route", "no such route"),
        Some(_) if query.is_some() && path != api::INVENTORY => {
            return api::error(400, "invalid_request", "this route takes no query");
        }
        Some(Matched::Health) => return api::health(shared.source.as_ref()),
        Some(Matched::Session(id)) => api::Call::Show(id),
        Some(Matched::Inventory) => match api::list_call(query) {
            Ok(call) => call,
            Err(why) => return api::error(400, "invalid_request", why),
        },
    };
    shared
        .runs
        .call(call, deadline, limits, &shared.source, &shared.clock)
}

/// Loopback with this server's port, or the configured public host name.
fn host_allowed(host: &str, port: u16, allowed: Option<&str>) -> bool {
    let loopback = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    loopback.iter().any(|h| h.eq_ignore_ascii_case(host))
        || allowed.is_some_and(|name| name.eq_ignore_ascii_case(host))
}

/// `openwiz-server --help`.
pub fn help() -> String {
    format!(
        "{NAME}: the read-only OpenWiz HTTP API, on loopback only.

Usage:
  {NAME} [--port N] [--allowed-host NAME] [--web-root DIR]
  {NAME} --help

  --port N             listen on 127.0.0.1:N (default {DEFAULT_PORT}; 0 picks a free port)
  --allowed-host NAME  also serve requests whose Host is NAME, the one public
                       host name in front of this server (default: the
                       environment variable {ALLOWED_HOST_ENV}, else none)
  --web-root DIR       also serve the web page from DIR, apps/web's static
                       export (a directory with index.html): GET and HEAD on
                       /, /_next/static/... and /fonts/... (default: none, and
                       every other path is 404)

It always listens on 127.0.0.1 only. Routes (GET only):
  /api/v1/inventory?detail=1|2&hidden=0|1&archived=0|1&limit=N&offset=N
  /api/v1/sessions/<id>
  /api/v1/health
Each answer is what openwiz ls, ls -l or show prints with --json; see
contracts/web-api.v1.md. One line per request goes to stderr: the method,
route, status and time only.
"
    )
}
