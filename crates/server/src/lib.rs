mod api;
mod attachments;
mod http;
mod new_thread;
mod send;
mod web;

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use openwiz_core::messaging::record::{SendStore, StoreError};
use openwiz_core::messaging::send::Messenger;
use openwiz_core::model::Timestamp;
use openwiz_core::new_thread::operation::NewThreadClient;
use openwiz_core::new_thread::record::NewThreadStore;
use openwiz_core::ops::Clock;
use openwiz_core::source::SessionSource;
use openwiz_store::AttachmentStore;
use openwiz_store::attachments::FreeSpace;

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

/// Sends (the core's `send`) at once, apart from the core runs.
pub const MAX_SENDS: usize = 2;

/// Creates (the core's `new_thread`) at once, apart from the sends and the
/// core runs: one.
pub const MAX_CREATES: usize = 1;

/// The largest send body: the core's text cap fully JSON-escaped (6 bytes
/// per byte), plus 1 KiB for the other keys. The core still checks the text.
pub const MAX_SEND_BODY_BYTES: usize = 6 * openwiz_core::messaging::MAX_TEXT_BYTES + 1024;

/// Uploads (staging one file) at once, apart from the sends, creates and
/// core runs.
pub const MAX_UPLOADS: usize = 2;

/// The largest upload body: the core's per-file limit, the largest
/// provider limit. The store still checks the core's rules.
pub const MAX_UPLOAD_BYTES: u64 = openwiz_core::attachment::FILE_MAX_BYTES;

/// From the end of an upload's head to its answer, for the whole body and
/// its verification: under Cloudflare's 125-second proxy read timeout
/// (documented, not observed).
pub const UPLOAD_DEADLINE: Duration = Duration::from_secs(110);

/// The header that carries an upload's file name, percent-encoded UTF-8.
pub const NAME_HEADER: &str = "X-OpenWiz-Name";

/// The longest file name an upload may carry, in bytes, decoded.
pub const MAX_NAME_BYTES: usize = 1024;

/// A send's body must arrive within this time of the end of the head.
pub const BODY_TIMEOUT: Duration = Duration::from_secs(10);

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

/// Every route the server answers. Anything else is `404` or `405`. Of the
/// four `POST`s, the send and the create are the only writes to CloudRoom;
/// the new-thread preview only reads; the upload, like the `DELETE`,
/// changes only the attachment store. There is no retry route, and no route returns an attachment's bytes.
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
        path: api::PREVIEW_ROUTE,
    },
    Route {
        method: "GET",
        path: api::LOOKUP_ROUTE,
    },
    Route {
        method: "GET",
        path: api::HEALTH,
    },
    Route {
        method: "POST",
        path: api::SEND_ROUTE,
    },
    Route {
        method: "GET",
        path: api::CLOUD_OPTIONS,
    },
    Route {
        method: "GET",
        path: api::NEW_THREAD_STATUS_ROUTE,
    },
    Route {
        method: "POST",
        path: api::THREADS_PREVIEW_ROUTE,
    },
    Route {
        method: "POST",
        path: api::THREADS_ROUTE,
    },
    Route {
        method: "GET",
        path: api::ATTACHMENTS_ROUTE,
    },
    Route {
        method: "POST",
        path: api::ATTACHMENTS_ROUTE,
    },
    Route {
        method: "DELETE",
        path: api::ATTACHMENT_ROUTE,
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
    pub max_sends: usize,
    pub max_send_body_bytes: usize,
    pub body_timeout: Duration,
    pub max_creates: usize,
    pub max_uploads: usize,
    pub max_upload_bytes: u64,
    pub upload_deadline: Duration,
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
            max_sends: MAX_SENDS,
            max_send_body_bytes: MAX_SEND_BODY_BYTES,
            body_timeout: BODY_TIMEOUT,
            max_creates: MAX_CREATES,
            max_uploads: MAX_UPLOADS,
            max_upload_bytes: MAX_UPLOAD_BYTES,
            upload_deadline: UPLOAD_DEADLINE,
        }
    }
}

/// Opens the send record for a send (creating it if needed).
pub type OpenStore = Arc<dyn Fn() -> Result<Box<dyn SendStore>, StoreError> + Send + Sync>;

/// Opens the send record for reading only: creates nothing, and `None`
/// when it does not exist yet.
pub type OpenStoreReadOnly =
    Arc<dyn Fn() -> Result<Option<Box<dyn SendStore>>, StoreError> + Send + Sync>;

/// The one public host name a send may come to. It can never be a
/// loopback name or an IP address: [`SendHost::new`]
/// is the only way to make one, so no configuration can serve a send to a
/// loopback `Host`, whatever the allowed host is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendHost(String);

/// Names that mean this machine.
const LOOPBACK_NAMES: &[&str] = &[
    "localhost",
    "localhost.localdomain",
    "ip6-localhost",
    "ip6-loopback",
];

impl SendHost {
    /// A DNS name ([`valid_host_name`]) that is not a loopback name
    /// (`localhost`, `*.localhost` and [`LOOPBACK_NAMES`]) and not an IP
    /// address: its last label is not all digits (every form a resolver may
    /// read as IPv4, such as `127.1` or `2130706433`) nor hex (`0x…`); IPv6
    /// literals are not DNS names.
    pub fn new(name: &str) -> Result<Self, &'static str> {
        const WHY: &str =
            "the allowed host must be a public DNS name: not a loopback name, not an IP address";
        if !valid_host_name(name) {
            return Err(WHY);
        }
        let lower = name.to_ascii_lowercase();
        let loopback = LOOPBACK_NAMES.contains(&lower.as_str()) || lower.ends_with(".localhost");
        let last = lower.rsplit('.').next().unwrap_or_default();
        let numeric = last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x");
        if loopback || numeric || lower.parse::<std::net::IpAddr>().is_ok() {
            return Err(WHY);
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the send routes need: the provider for the one write, and the send
/// record, opened as `openwiz send` opens it, when each request needs it.
#[derive(Clone)]
pub struct Sending {
    pub messenger: Arc<dyn Messenger>,
    pub open_store: OpenStore,
    pub open_store_read_only: OpenStoreReadOnly,
    /// The host name sends are served on; `None`: every send is `421`.
    pub host: Option<SendHost>,
}

impl std::fmt::Debug for Sending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sending")
    }
}

/// Opens the new-thread record for a create (creating it if needed).
pub type OpenNewThreadStore =
    Arc<dyn Fn() -> Result<Box<dyn NewThreadStore>, StoreError> + Send + Sync>;

/// Opens the new-thread record for reading only: creates nothing, and
/// `None` when it does not exist yet.
pub type OpenNewThreadStoreReadOnly =
    Arc<dyn Fn() -> Result<Option<Box<dyn NewThreadStore>>, StoreError> + Send + Sync>;

/// What the new-thread routes need: the provider's reads and its one
/// create, and the new-thread record, opened as `openwiz new` opens it,
/// when each request needs it.
#[derive(Clone)]
pub struct Creating {
    pub client: Arc<dyn NewThreadClient>,
    pub open_store: OpenNewThreadStore,
    pub open_store_read_only: OpenNewThreadStoreReadOnly,
    /// The host name creates and new-thread previews are served on; `None`:
    /// every one is `421`.
    pub host: Option<SendHost>,
}

impl std::fmt::Debug for Creating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Creating")
    }
}

/// Opens the attachment store (creating it if needed), as `openwiz
/// attachment add` and `rm` open it.
pub type OpenAttachmentStore = Arc<dyn Fn() -> Result<AttachmentStore, StoreError> + Send + Sync>;

/// Opens the attachment store only if it exists, as `openwiz attachment
/// ls` does: `None` creates nothing.
pub type OpenAttachmentStoreReadOnly =
    Arc<dyn Fn() -> Result<Option<AttachmentStore>, StoreError> + Send + Sync>;

/// What the attachment routes need: the attachment store, opened as
/// `openwiz attachment` opens it, when each request needs it, and the
/// free-space measure staging checks the floor with.
#[derive(Clone)]
pub struct Attaching {
    pub open_store: OpenAttachmentStore,
    pub open_store_read_only: OpenAttachmentStoreReadOnly,
    pub free_space: Arc<dyn FreeSpace + Send + Sync>,
    /// The host name uploads and removals are served on; `None`: every one
    /// is `421`.
    pub host: Option<SendHost>,
}

impl std::fmt::Debug for Attaching {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Attaching")
    }
}

/// The server's configuration. Nothing in it is ever committed.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// The one public host name served besides loopback, from local
    /// configuration (`--allowed-host` or [`ALLOWED_HOST_ENV`]).
    pub allowed_host: Option<String>,
    /// The web page's build directory (`--web-root`). Without it, every
    /// non-API path is `404`.
    pub web_root: Option<WebRoot>,
    pub limits: Limits,
    /// The send routes' provider and send record. `openwiz-server` always
    /// sets it; without it (an in-process server in a test), the send,
    /// preview and lookup routes are `404`.
    pub sending: Option<Sending>,
    /// The new-thread routes' provider and record. `openwiz-server` always
    /// sets it; without it, the four new-thread routes are `404`.
    pub creating: Option<Creating>,
    /// The attachment routes' store. `openwiz-server` always sets it;
    /// without it, the three attachment routes are `404`, and a send or a
    /// create with attachments is refused by the core's gate.
    pub attaching: Option<Attaching>,
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
    /// Uploads holding an upload slot.
    pub uploads: usize,
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
    sends: Arc<send::Slots>,
    creates: Arc<send::Slots>,
    uploads: Arc<send::Slots>,
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
            uploads: self.shared.uploads.used(),
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
        sends: Arc::new(send::Slots::default()),
        creates: Arc::new(send::Slots::default()),
        uploads: Arc::new(send::Slots::default()),
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
                (
                    method,
                    route,
                    respond(&head, &mut stream, shared),
                    head_method,
                )
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
/// (or the web file). The send, create and new-thread preview routes have
/// their own order after the method (`send.rs`, `new_thread.rs`; the
/// contract's "Sending" and "Creating").
fn respond(head: &Head, stream: &mut TcpStream, shared: &Shared) -> Reply {
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
    let matched = Matched::of(path);
    let sending = shared.config.sending.as_ref();
    // Without a send configuration these routes do not exist, for every
    // method.
    if sending.is_none()
        && matches!(
            matched,
            Some(Matched::Send(_) | Matched::Preview(_) | Matched::Lookup(..))
        )
    {
        return api::error(404, "no_route", "no such route");
    }
    // The same for the new-thread routes without a new-thread configuration.
    let creating = shared.config.creating.as_ref();
    if creating.is_none()
        && matches!(
            matched,
            Some(
                Matched::CloudOptions
                    | Matched::Threads(_)
                    | Matched::ThreadsPreview(_)
                    | Matched::NewThreadStatus(_)
            )
        )
    {
        return api::error(404, "no_route", "no such route");
    }
    // The same for the attachment routes without an attachment configuration
    //.
    let attaching = shared.config.attaching.as_ref();
    if attaching.is_none() && matches!(matched, Some(Matched::Attachments | Matched::Attachment(_)))
    {
        return api::error(404, "no_route", "no such route");
    }
    if let Some(Matched::Send(_) | Matched::Threads(_) | Matched::ThreadsPreview(_)) = &matched
        && head.method != "POST"
    {
        let mut reply = api::error(405, "method_not_allowed", "only POST is allowed");
        reply.extra.push(("Allow", "POST".to_owned()));
        return reply;
    }
    if let Some(Matched::Attachments) = &matched
        && head.method != "GET"
        && head.method != "POST"
    {
        let mut reply = api::error(405, "method_not_allowed", "only GET and POST are allowed");
        reply.extra.push(("Allow", "GET, POST".to_owned()));
        return reply;
    }
    if let Some(Matched::Attachment(_)) = &matched
        && head.method != "DELETE"
    {
        let mut reply = api::error(405, "method_not_allowed", "only DELETE is allowed");
        reply.extra.push(("Allow", "DELETE".to_owned()));
        return reply;
    }
    if let (Some(attaching), Some(route @ (Matched::Attachments | Matched::Attachment(_)))) =
        (attaching, &matched)
        && head.method != "GET"
    {
        return attachment_write(head, stream, shared, attaching, host, query, route);
    }
    match &matched {
        Some(Matched::Send(id)) => {
            return send_route(head, stream, shared, host, query, id, deadline);
        }
        Some(route @ (Matched::Threads(_) | Matched::ThreadsPreview(_))) => {
            return create_route(head, stream, shared, host, query, route, deadline);
        }
        _ => {}
    }
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
    let call = match matched {
        None => return api::error(404, "no_route", "no such route"),
        Some(Matched::Preview(id)) => match api::preview_call(&id, query) {
            Ok(call) => call,
            Err(why) => return api::error(400, "invalid_request", why),
        },
        Some(_) if query.is_some() && path != api::INVENTORY => {
            return api::error(400, "invalid_request", "this route takes no query");
        }
        Some(Matched::Health) => return api::health(shared.source.as_ref()),
        Some(Matched::Lookup(id, op_id)) => {
            let Some(sending) = sending else {
                return api::error(404, "no_route", "no such route");
            };
            return send::lookup(sending, shared.clock.as_ref(), &id, &op_id);
        }
        Some(Matched::Session(id)) => api::Call::Show(id),
        Some(Matched::Inventory) => match api::list_call(query) {
            Ok(call) => call,
            Err(why) => return api::error(400, "invalid_request", why),
        },
        Some(Matched::CloudOptions) => api::Call::CloudOptions,
        Some(Matched::Attachments) => {
            let Some(attaching) = attaching else {
                return api::error(404, "no_route", "no such route");
            };
            return attachments::list(attaching, shared.clock.as_ref());
        }
        Some(Matched::NewThreadStatus(op_id)) => {
            if let Err(refusal) = new_thread::status_op_id(&op_id) {
                return refusal;
            }
            api::Call::NewThreadStatus(op_id)
        }
        Some(
            Matched::Send(_)
            | Matched::Threads(_)
            | Matched::ThreadsPreview(_)
            | Matched::Attachment(_),
        ) => {
            unreachable!("answered above")
        }
    };
    shared.runs.call(call, deadline, limits, &core(shared))
}

/// What a core run calls, from the server's configuration.
fn core(shared: &Shared) -> api::Core {
    api::Core {
        source: shared.source.clone(),
        messenger: shared.config.sending.as_ref().map(|s| s.messenger.clone()),
        creating: shared.config.creating.clone(),
        attaching: shared.config.attaching.clone(),
        clock: shared.clock.clone(),
    }
}

/// `POST /api/v1/attachments` and `DELETE /api/v1/attachments/{id}`, after
/// the `Host` and the method: the route, the write `Host` (only the
/// configured host name, never loopback, shared with send), the
/// query, then `attachments.rs`.
fn attachment_write(
    head: &Head,
    stream: &mut TcpStream,
    shared: &Shared,
    attaching: &Attaching,
    host: &str,
    query: Option<&str>,
    route: &Matched,
) -> Reply {
    let Some(site) = attaching
        .host
        .as_ref()
        .map(SendHost::as_str)
        .filter(|site| site.eq_ignore_ascii_case(host))
    else {
        return api::error(
            421,
            "misdirected",
            "uploads and removals are served only on the configured host name",
        );
    };
    if query.is_some() {
        return api::error(400, "invalid_request", "this route takes no query");
    }
    let context = attachments::Context {
        attaching,
        slots: &shared.uploads,
        clock: &shared.clock,
        limits: &shared.config.limits,
    };
    match route {
        Matched::Attachment(id) => attachments::remove(head, site, id, &context),
        _ => attachments::upload(head, stream, site, &context),
    }
}

/// `POST /api/v1/projects/{id}/threads` and `…/threads/preview`, after the
/// `Host` and the method: the route, the create `Host` (only the configured
/// host name, never loopback, shared with send), the query, then
/// `new_thread.rs`. The preview, which only reads, is a shared core run.
fn create_route(
    head: &Head,
    stream: &mut TcpStream,
    shared: &Shared,
    host: &str,
    query: Option<&str>,
    route: &Matched,
    deadline: Instant,
) -> Reply {
    let (project, preview) = match route {
        Matched::Threads(project) => (project.as_str(), false),
        Matched::ThreadsPreview(project) => (project.as_str(), true),
        _ => return api::error(404, "no_route", "no such route"),
    };
    let Some(creating) = shared.config.creating.as_ref() else {
        return api::error(404, "no_route", "no such route");
    };
    let Some(site) = creating
        .host
        .as_ref()
        .map(SendHost::as_str)
        .filter(|site| site.eq_ignore_ascii_case(host))
    else {
        return api::error(
            421,
            "misdirected",
            "creates are served only on the configured host name",
        );
    };
    if query.is_some() {
        return api::error(400, "invalid_request", "this route takes no query");
    }
    let limits = &shared.config.limits;
    if preview {
        let asked = match new_thread::read_asked(head, stream, site, project, limits, "a preview") {
            Ok(asked) => asked,
            Err(refusal) => return refusal,
        };
        return shared.runs.call(
            api::Call::NewThreadPreview(Box::new(asked)),
            deadline,
            limits,
            &core(shared),
        );
    }
    let context = new_thread::Context {
        creating,
        attaching: shared.config.attaching.as_ref(),
        slots: &shared.creates,
        clock: &shared.clock,
        limits,
        deadline,
    };
    new_thread::create(head, stream, site, project, &context)
}

/// `POST /api/v1/sessions/{id}/messages`, after the `Host` and the method:
/// the route, the send `Host` (only the configured host name, never
/// loopback: ), the query, then `send.rs`.
fn send_route(
    head: &Head,
    stream: &mut TcpStream,
    shared: &Shared,
    host: &str,
    query: Option<&str>,
    id: &str,
    deadline: Instant,
) -> Reply {
    let Some(sending) = shared.config.sending.as_ref() else {
        return api::error(404, "no_route", "no such route");
    };
    let Some(site) = sending
        .host
        .as_ref()
        .map(SendHost::as_str)
        .filter(|site| site.eq_ignore_ascii_case(host))
    else {
        return api::error(
            421,
            "misdirected",
            "sends are served only on the configured host name",
        );
    };
    if query.is_some() {
        return api::error(400, "invalid_request", "this route takes no query");
    }
    let context = send::Context {
        sending,
        attaching: shared.config.attaching.as_ref(),
        slots: &shared.sends,
        clock: &shared.clock,
        limits: &shared.config.limits,
        deadline,
    };
    send::post(head, stream, site, id, &context)
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
        "{NAME}: the OpenWiz HTTP API, on loopback only, with two writes: send and
create (a new Cloud thread); and attachments, staged on this machine only.

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

It always listens on 127.0.0.1 only. Routes (GET):
  /api/v1/inventory?detail=1|2&hidden=0|1&archived=0|1&limit=N&offset=N
  /api/v1/sessions/<id>
  /api/v1/sessions/<id>/preview?mode=queue|steer
  /api/v1/sessions/<id>/messages/<op id>
  /api/v1/health
  /api/v1/cloud-options
  /api/v1/new-threads/<op id>
  /api/v1/attachments                (the staged attachments; never their bytes)
and, only with --allowed-host and only on that host, four POSTs: the two
writes, the new-thread preview, which only reads, and an upload (a raw body,
at most 25 MiB, staged as openwiz attachment add stages a file), and one
DELETE, of a staged attachment:
  /api/v1/sessions/<id>/messages
  /api/v1/projects/<project id>/threads
  /api/v1/projects/<project id>/threads/preview
  /api/v1/attachments
  DELETE /api/v1/attachments/<attachment id>
The send, preview and lookup answers are what openwiz send prints with
--json; they use the same send record as openwiz send ($OPENWIZ_STATE_DIR,
else the owner's default state directory). The create, new-thread preview,
status and options answers are what openwiz new (with --yes, without it,
with --status) and openwiz models print with --json; they use the same
new-thread record as openwiz new. There is no retry. The attachment answers
are what openwiz attachment add, ls and rm print with --json, from the same
attachment store; no route returns an attachment's bytes.
The inventory and session answers are what openwiz ls, ls -l or show prints
with --json; health is the server's own metadata, with no CLI equivalent and
no room-cli read. See contracts/web-api.v1.md. One line per request goes to
stderr: the method, route, status and time only.
"
    )
}
