//! Helpers: an in-process server over the fake `room-cli`, a raw HTTP/1.1
//! client, and a gated source that holds every core run until released.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use openwiz_core::model::SessionId;
use openwiz_core::source::{
    MachineRecord, OutputRecord, ProjectRecord, Read as SourceRead, Rows, SessionDetailRecord,
    SessionRecord, SessionScope, SessionSource, SourceError,
};
use openwiz_server::{Config, Log, Server};
use serde_json::Value;

use crate::fake::{self, Fake};

/// A server on an ephemeral loopback port, with its log captured.
pub struct TestServer {
    pub server: Server,
    pub addr: SocketAddr,
    pub log: Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    pub fn get(&self, target: &str) -> Response {
        get(self.addr, target)
    }

    pub fn request(&self, method: &str, target: &str) -> Response {
        request(self.addr, method, target)
    }

    pub fn send(&self, raw: &[u8]) -> Response {
        send(self.addr, raw)
    }

    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.addr.port())
    }

    /// Every log line so far, each ending in a newline.
    pub fn log_text(&self) -> String {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|line| format!("{line}\n"))
            .collect()
    }

    /// Waits until `check` holds for the server's counts.
    pub fn wait_for(&self, what: &str, check: impl Fn(openwiz_server::Stats) -> bool) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let stats = self.server.stats();
            if check(stats) {
                return;
            }
            assert!(
                Instant::now() < until,
                "timed out waiting for {what}: {stats:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Serves `source` with the fixed clock the CLI tests use.
pub fn serve_source(source: Arc<dyn SessionSource>, config: Config) -> TestServer {
    let listener = openwiz_server::bind(0).expect("bind loopback");
    let log = Arc::new(Mutex::new(Vec::new()));
    let lines = log.clone();
    let sink: Log = Arc::new(move |line: &str| lines.lock().unwrap().push(line.to_owned()));
    let server = openwiz_server::start(listener, config, source, Arc::new(fake::clock()), sink)
        .expect("start the server");
    let addr = server.local_addr();
    TestServer { server, addr, log }
}

/// Serves the CloudRoom source over this fake, with the default config.
pub fn serve(fake: &Fake) -> TestServer {
    serve_source(Arc::new(fake.source()), Config::default())
}

pub fn serve_with(fake: &Fake, config: Config) -> TestServer {
    serve_source(Arc::new(fake.source()), config)
}

/// A parsed response.
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub status_line: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn headers_named(&self, name: &str) -> usize {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .count()
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.body.clone()).expect("the body is UTF-8")
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("the body is not JSON ({e}): {}", self.text()))
    }

    /// `error.kind` and `error.message` of an error body.
    pub fn error(&self) -> (String, String) {
        let body = self.json();
        let error = &body["error"];
        (
            error["kind"].as_str().expect("error.kind").to_owned(),
            error["message"].as_str().expect("error.message").to_owned(),
        )
    }
}

/// `GET <target>` with a loopback `Host`.
pub fn get(addr: SocketAddr, target: &str) -> Response {
    request(addr, "GET", target)
}

pub fn request(addr: SocketAddr, method: &str, target: &str) -> Response {
    let raw = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUser-Agent: openwiz-tests\r\n\r\n",
        addr.port()
    );
    send(addr, raw.as_bytes())
}

/// Writes `raw`, then reads until the server closes the connection.
pub fn send(addr: SocketAddr, raw: &[u8]) -> Response {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    // The server may answer and close before reading everything.
    let _ = stream.write_all(raw);
    read_response(&mut stream)
}

/// Reads a response until the server closes the connection.
pub fn read_response(stream: &mut TcpStream) -> Response {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(e) if bytes.is_empty() => panic!("no response: {e}"),
            Err(_) => break,
        }
    }
    parse_response(&bytes)
}

pub fn parse_response(bytes: &[u8]) -> Response {
    let end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no head in {:?}", String::from_utf8_lossy(bytes)));
    let head = std::str::from_utf8(&bytes[..end]).expect("the head is ASCII");
    let mut lines = head.split("\r\n");
    let status_line = lines.next().expect("status line").to_owned();
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("bad status line {status_line:?}"));
    let headers = lines
        .map(|l| {
            let (n, v) = l.split_once(':').expect("header has a colon");
            (n.to_owned(), v.trim().to_owned())
        })
        .collect();
    Response {
        status,
        status_line,
        headers,
        body: bytes[end + 4..].to_vec(),
    }
}

/// An address of this machine that is not loopback, if it has one: the
/// address it would use to reach a documentation-only network. No packet
/// is sent.
pub fn non_loopback_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(("192.0.2.1", 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// Builds the `openwiz-server` binary into the test's target directory and
/// returns its path.
pub fn server_binary() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let target = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .expect("the target directory")
        .to_path_buf();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let build = Command::new(cargo)
        .args(["build", "--locked", "-p", "openwiz-server"])
        .args(["--bin", "openwiz-server", "--target-dir"])
        .arg(&target)
        .current_dir(&root)
        .output()
        .expect("run cargo build");
    assert!(
        build.status.success(),
        "cargo build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let program = target.join("debug").join("openwiz-server");
    assert!(program.is_file(), "no {program:?}");
    program
}

/// The CLI's exit code for each kind of error, and the HTTP status the
/// contract gives it.
pub const CLI_EXIT_TO_HTTP: &[(i32, u16, &str)] = &[
    (1, 502, "read_failed"),
    (2, 400, "invalid_request"),
    (3, 404, "not_found"),
];

/// Deletes the fake's invocation log, so the next run's reads stand alone.
pub fn clear_invocations(fake: &Fake) {
    let _ = std::fs::remove_file(fake.dir.join("invocations.log"));
}

/// A source that holds every core run at its version read until
/// [`Gated::open`], and counts the runs. Everything else is the inner
/// source's.
pub struct Gated {
    inner: Arc<dyn SessionSource>,
    open: Mutex<bool>,
    opened: Condvar,
    runs: AtomicUsize,
    held: AtomicUsize,
    max_held: AtomicUsize,
}

impl Gated {
    pub fn new(inner: Arc<dyn SessionSource>) -> Self {
        Self {
            inner,
            open: Mutex::new(false),
            opened: Condvar::new(),
            runs: AtomicUsize::new(0),
            held: AtomicUsize::new(0),
            max_held: AtomicUsize::new(0),
        }
    }

    pub fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.opened.notify_all();
    }

    /// Core runs started (version reads).
    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }

    /// Runs held at the gate now.
    pub fn held(&self) -> usize {
        self.held.load(Ordering::SeqCst)
    }

    /// The most runs ever held at the gate at once.
    pub fn max_held(&self) -> usize {
        self.max_held.load(Ordering::SeqCst)
    }

    pub fn wait_held(&self, n: usize) {
        let until = Instant::now() + Duration::from_secs(10);
        while self.held() != n {
            assert!(
                Instant::now() < until,
                "timed out waiting for {n} held runs; {} held",
                self.held()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl SessionSource for Gated {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn tested_version(&self) -> &str {
        self.inner.tested_version()
    }

    fn read_name(&self, read: SourceRead) -> &str {
        self.inner.read_name(read)
    }

    fn version(&self, deadline: Instant) -> Result<String, SourceError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let now = self.held.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_held.fetch_max(now, Ordering::SeqCst);
        let mut open = self.open.lock().unwrap();
        while !*open {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            open = self.opened.wait_timeout(open, left).unwrap().0;
        }
        drop(open);
        self.held.fetch_sub(1, Ordering::SeqCst);
        self.inner.version(deadline)
    }

    fn list_sessions(
        &self,
        scope: SessionScope,
        deadline: Instant,
    ) -> Result<Rows<SessionRecord>, SourceError> {
        self.inner.list_sessions(scope, deadline)
    }

    fn list_projects(
        &self,
        include_hidden: bool,
        deadline: Instant,
    ) -> Result<Rows<ProjectRecord>, SourceError> {
        self.inner.list_projects(include_hidden, deadline)
    }

    fn list_machines(&self, deadline: Instant) -> Result<Rows<MachineRecord>, SourceError> {
        self.inner.list_machines(deadline)
    }

    fn session_detail(
        &self,
        id: &SessionId,
        deadline: Instant,
    ) -> Result<SessionDetailRecord, SourceError> {
        self.inner.session_detail(id, deadline)
    }

    fn session_output(
        &self,
        id: &SessionId,
        deadline: Instant,
    ) -> Result<OutputRecord, SourceError> {
        self.inner.session_output(id, deadline)
    }
}
