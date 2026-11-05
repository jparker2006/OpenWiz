//! Helpers for the attachment routes: an in-process server with
//! the send, new-thread and attachment configurations over one private
//! state directory, a free-space measure the test sets, raw upload, list
//! and removal clients, and `openwiz attachment` in-process with the same
//! clock, store and measure, for AW4's comparisons.
//!
//! **Synthetic files only**, made at test time (never committed): B2's
//! `staging` makers and the few here. Nothing here calls `room-cli`.

#![allow(dead_code)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use openwiz_cli::attachment::{AttachmentContext, run_attachment};
use openwiz_core::messaging::record::StoreError;
use openwiz_core::ops::Clock;
use openwiz_server::{
    Attaching, Config, Limits, Log, OpenAttachmentStore, OpenAttachmentStoreReadOnly, SendHost,
};
use openwiz_store::AttachmentStore;
use openwiz_store::attachments::{FreeSpace, RecordedClaims};
use serde_json::Value;

use crate::http::{Response, TestServer};
use crate::send_support::{HOST, ORIGIN, raw};

pub const ATTACHMENTS: &str = "/api/v1/attachments";

/// The removal route of `id`.
pub fn attachment(id: &str) -> String {
    format!("{ATTACHMENTS}/{id}")
}

/// A free-space measure the test sets: plenty by default.
pub struct Free(pub AtomicU64);

impl Free {
    pub fn plenty() -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(1 << 50)))
    }

    pub fn set(&self, bytes: u64) {
        self.0.store(bytes, Ordering::SeqCst);
    }
}

impl FreeSpace for Free {
    fn free_bytes(&self, _dir: &Path) -> Result<u64, String> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

/// A free-space measure that always fails.
pub struct Unmeasurable;

impl FreeSpace for Unmeasurable {
    fn free_bytes(&self, _dir: &Path) -> Result<u64, String> {
        Err("synthetic measure failure".to_owned())
    }
}

/// The attachment store under `state`, opened as `openwiz attachment`
/// opens it, with the measure `free`.
pub fn attaching(state: &Path, free: Arc<dyn FreeSpace + Send + Sync>) -> Attaching {
    let (a, b) = (state.to_owned(), state.to_owned());
    let open_store: OpenAttachmentStore = Arc::new(move || AttachmentStore::open(&a));
    let open_store_read_only: OpenAttachmentStoreReadOnly =
        Arc::new(move || AttachmentStore::open_read_only(&b));
    Attaching {
        open_store,
        open_store_read_only,
        free_space: free,
        host: Some(SendHost::new(HOST).expect("a public host")),
    }
}

/// A config with only the attachment routes, over `state`.
pub fn attach_config(
    state: &Path,
    free: Arc<dyn FreeSpace + Send + Sync>,
    limits: Limits,
) -> Config {
    Config {
        allowed_host: Some(HOST.to_owned()),
        web_root: None,
        limits,
        sending: None,
        creating: None,
        attaching: Some(attaching(state, free)),
    }
}

/// A private state directory for one test, outside every repository.
pub struct State {
    pub name: String,
    pub dir: PathBuf,
}

impl State {
    pub fn new(test: &str) -> Self {
        let dir = crate::msg::run_root().join(format!("{test}-attach-state"));
        let _ = fs::remove_dir_all(&dir);
        Self {
            name: test.to_owned(),
            dir,
        }
    }

    /// Every attachment directory in the store (finished or not).
    pub fn entries(&self) -> Vec<PathBuf> {
        let Ok(read) = fs::read_dir(self.dir.join("attachments")) else {
            return vec![];
        };
        read.filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    }

    /// Whether nothing is staged at all: no attachment directory, and no
    /// `.partial` anywhere.
    pub fn nothing_staged(&self) -> bool {
        self.entries().is_empty() && self.partials().is_empty()
    }

    /// Every `.partial` file under the store.
    pub fn partials(&self) -> Vec<PathBuf> {
        let mut out = vec![];
        for dir in self.entries() {
            for f in fs::read_dir(&dir).into_iter().flatten().flatten() {
                if f.file_name().to_string_lossy().ends_with(".partial") {
                    out.push(f.path());
                }
            }
        }
        out
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

/// An in-process server with `config`, the source of `source`, and `clock`.
pub fn serve(
    config: Config,
    source: Arc<dyn openwiz_core::source::SessionSource>,
    clock: Arc<dyn Clock>,
) -> TestServer {
    let listener = openwiz_server::bind(0).expect("bind loopback");
    let log = Arc::new(Mutex::new(Vec::new()));
    let lines = log.clone();
    let sink: Log = Arc::new(move |line: &str| lines.lock().unwrap().push(line.to_owned()));
    let server =
        openwiz_server::start(listener, config, source, clock, sink).expect("start the server");
    let addr = server.local_addr();
    TestServer { server, addr, log }
}

/// A server with only the attachment routes, over `state`, at `T0`.
pub fn serve_attach(
    state: &State,
    free: Arc<dyn FreeSpace + Send + Sync>,
    limits: Limits,
) -> TestServer {
    let fake = crate::fake::Fake::new(&format!("{}_source", state.name), "normal");
    serve(
        attach_config(&state.dir, free, limits),
        Arc::new(fake.source()),
        Arc::new(crate::new_support::clock()),
    )
}

/// What `encodeURIComponent` makes of `name`.
pub fn encode(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The headers a same-origin page sends with an upload of `name`.
pub fn upload_headers(name: &str) -> Vec<(String, String)> {
    [
        ("Host", HOST),
        ("Origin", ORIGIN),
        ("Sec-Fetch-Site", "same-origin"),
        ("X-OpenWiz-Send", "1"),
        ("Content-Type", "application/octet-stream"),
        ("X-Requested-With", "XMLHttpRequest"),
    ]
    .iter()
    .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
    .chain([("X-OpenWiz-Name".to_owned(), encode(name))])
    .collect()
}

/// The headers a same-origin page sends with a removal: the upload's,
/// without a body type or a name.
pub fn remove_headers() -> Vec<(String, String)> {
    [
        ("Host", HOST),
        ("Origin", ORIGIN),
        ("Sec-Fetch-Site", "same-origin"),
        ("X-OpenWiz-Send", "1"),
        ("X-Requested-With", "XMLHttpRequest"),
    ]
    .iter()
    .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
    .collect()
}

/// A same-origin upload of `bytes` as `name`.
pub fn upload(s: &TestServer, name: &str, bytes: &[u8]) -> Response {
    s.send(&raw(
        "POST",
        ATTACHMENTS,
        &upload_headers(name),
        bytes,
        true,
    ))
}

/// An upload with `headers`, `Content-Length` added.
pub fn upload_with(s: &TestServer, headers: &[(String, String)], bytes: &[u8]) -> Response {
    s.send(&raw("POST", ATTACHMENTS, headers, bytes, true))
}

/// `GET /api/v1/attachments` on the public host name.
pub fn list(s: &TestServer) -> Response {
    crate::send_support::get_public(s, ATTACHMENTS)
}

/// A same-origin removal of `id`.
pub fn remove(s: &TestServer, id: &str) -> Response {
    s.send(&raw(
        "DELETE",
        &attachment(id),
        &remove_headers(),
        b"",
        false,
    ))
}

/// The id of an upload's ready attachment.
pub fn ready_id(r: &Response) -> String {
    assert_eq!(r.status, 200, "{}", r.text());
    let body = r.json();
    body["ready"][0]["staged"]["attachment"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("not ready: {body}"))
        .to_owned()
}

/// `openwiz attachment <args>` in this process, over `state`, with `clock`
/// and `free`: (exit code, stdout, stderr).
pub fn cli(
    state: &Path,
    clock: &dyn Clock,
    free: &dyn FreeSpace,
    args: &[&str],
) -> (i32, String, String) {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let (a, b) = (state.to_owned(), state.to_owned());
    let open_store = move || -> Result<AttachmentStore, StoreError> { AttachmentStore::open(&a) };
    let open_store_read_only = move || -> Result<Option<AttachmentStore>, StoreError> {
        AttachmentStore::open_read_only(&b)
    };
    let claims = RecordedClaims::new(state, clock.now());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_attachment(
        &args,
        AttachmentContext {
            open_store: &open_store,
            open_store_read_only: &open_store_read_only,
            clock,
            free_space: free,
            claims: &claims,
        },
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).expect("stdout is UTF-8"),
        String::from_utf8(err).expect("stderr is UTF-8"),
    )
}

/// `text` with every occurrence of each `(from, to)` replaced: random
/// attachment ids and paths, made equal for a comparison.
pub fn normalized(text: &str, pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .fold(text.to_owned(), |t, (from, to)| t.replace(from, to))
}

/// Every attachment id (`att-` and 32 hex digits) in `value`, in order.
pub fn ids_in(value: &Value) -> Vec<String> {
    let text = value.to_string();
    let mut out = vec![];
    let mut rest = text.as_str();
    while let Some(i) = rest.find("att-") {
        let id: String = rest[i..].chars().take(36).collect();
        if id.len() == 36 && id[4..].bytes().all(|b| b.is_ascii_hexdigit()) && !out.contains(&id) {
            out.push(id);
        }
        rest = &rest[i + 4..];
    }
    out
}

/// A synthetic JPEG of exactly `len` bytes.
pub fn jpeg(len: usize) -> Vec<u8> {
    let mut b = vec![
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00,
    ];
    b.resize(len.max(b.len()), 0x5a);
    b
}

/// A synthetic GIF of exactly `len` bytes.
pub fn gif(len: usize) -> Vec<u8> {
    let mut b = b"GIF89a\x01\x00\x01\x00".to_vec();
    b.resize(len.max(b.len()), 0x5a);
    b
}

/// A synthetic WebP of exactly `len` bytes.
pub fn webp(len: usize) -> Vec<u8> {
    let mut b = b"RIFF\x00\x00\x00\x00WEBPVP8 ".to_vec();
    b.resize(len.max(b.len()), 0x5a);
    b
}

/// A synthetic HEIC header (`ftyp` with the `heic` brand).
pub fn heic(len: usize) -> Vec<u8> {
    let mut b = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00mif1heic".to_vec();
    b.resize(len.max(b.len()), 0x5a);
    b
}

/// A synthetic ZIP header.
pub fn zip(len: usize) -> Vec<u8> {
    let mut b = b"PK\x03\x04\x14\x00\x00\x00".to_vec();
    b.resize(len.max(b.len()), 0x5a);
    b
}

/// An SVG with a script: never rendered, only staged as a file.
pub const SVG_WITH_SCRIPT: &str =
    "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>/* synthetic */</script></svg>\n";

/// An HTML page with a script.
pub const HTML_WITH_SCRIPT: &str =
    "<!doctype html><title>Synthetic</title><script>/* synthetic */</script>\n";
