//! The attachment routes: the upload (`POST /api/v1/attachments`),
//! the list (`GET`) and the removal (`DELETE /api/v1/attachments/{id}`). See
//! `contracts/web-api.v1.md`, "Attachments", for route rules and
//! `docs/web-access.md` for access boundaries.
//!
//! The server checks only the request's shape: the write `Host`, the send's
//! cross-site checks, a raw
//! `application/octet-stream` body (never multipart), its framing and size,
//! the name header, a cap of two uploads at a time and the upload deadline.
//! Then it calls the attachment store (`openwiz-store`) and its reports
//! (`openwiz_store::attachments::report`, the one implementation `openwiz
//! attachment add`, `ls` and `rm` print too), and answers with them. The kind, the safe name, the limits, the free-disk floor and
//! retention are the core's and the store's. **No route returns a file's
//! bytes**, and nothing of a request is logged. Nothing here calls
//! `room-cli`.
//!
//! The body streams straight into the store's staging
//! ([`openwiz_store::attachments::AttachmentStore::stage_stream`]): no file
//! is written beside the store.

use std::io::{self, Read};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Instant;

use openwiz_core::attachment::AttachmentId;
use openwiz_core::ops::Clock;
use openwiz_store::attachments::report::{self, RemoveResult};
use serde::Serialize;

use crate::api::{error, ok, pretty};
use crate::http::{Head, Reply};
use crate::send::{Slots, length_within, same_site, single};
use crate::{Attaching, Limits, MAX_NAME_BYTES, NAME_HEADER};

/// Everything an upload or a removal needs from the server.
pub(crate) struct Context<'a> {
    pub attaching: &'a Attaching,
    pub slots: &'a Arc<Slots>,
    pub clock: &'a Arc<dyn Clock>,
    pub limits: &'a Limits,
}

fn store_unavailable(nothing: &str) -> Reply {
    error(
        500,
        "store_unavailable",
        &format!("the attachment store could not be used; {nothing}"),
    )
}

fn written(value: &impl Serialize) -> Reply {
    pretty(value).map_or_else(
        || error(500, "internal", "the result could not be written"),
        ok,
    )
}

/// `application/octet-stream`, exactly, with no parameter.
fn octet_stream(value: &str) -> bool {
    value.eq_ignore_ascii_case("application/octet-stream")
}

/// The file's name from its header: percent-encoded UTF-8, as
/// `encodeURIComponent` makes it, decoded; one file name (no `/`, no NUL,
/// not `.` or `..`, as a name `openwiz attachment add` reads from a path
/// always is), 1 to [`MAX_NAME_BYTES`] bytes. The name is the owner's,
/// untrusted: the core makes the staged name from it.
fn name_of(head: &Head) -> Option<String> {
    let raw = single(head, NAME_HEADER)?;
    let mut bytes = Vec::with_capacity(raw.len());
    let mut it = raw.bytes();
    while let Some(b) = it.next() {
        match b {
            b'%' => {
                let hex = [it.next()?, it.next()?];
                let hex = std::str::from_utf8(&hex).ok()?;
                bytes.push(u8::from_str_radix(hex, 16).ok()?);
            }
            b if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) => bytes.push(b),
            _ => return None,
        }
    }
    let name = String::from_utf8(bytes).ok()?;
    let one = !name.contains(['/', '\0']) && name != "." && name != "..";
    (one && !name.is_empty() && name.len() <= MAX_NAME_BYTES).then_some(name)
}

/// Reads and discards what the client still sends of its body, up to `left`
/// bytes, within `until`, so that a browser still sending can read the
/// answer to a refusal.
fn drain(stream: &mut TcpStream, mut left: u64, until: Instant) {
    let mut sink = [0u8; 64 * 1024];
    while left > 0 {
        let time = until.saturating_duration_since(Instant::now());
        if time.is_zero() || stream.set_read_timeout(Some(time)).is_err() {
            return;
        }
        let want = usize::try_from(left.min(sink.len() as u64)).unwrap_or(sink.len());
        match stream.read(&mut sink[..want]) {
            Ok(0) | Err(_) => return,
            Ok(n) => left -= n as u64,
        }
    }
}

/// Why a body stopped before its `Content-Length`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cut {
    /// It did not arrive within the upload deadline.
    Late,
    /// It ended early, or the connection failed: the client has gone.
    Short,
}

/// An upload's body, as the store reads it: the bytes read with the head,
/// then the stream's, never past `left`, each read within the deadline. Why
/// it stopped early is kept for the answer.
struct Body<'a> {
    stream: &'a mut TcpStream,
    rest: &'a [u8],
    left: u64,
    deadline: Instant,
    cut: Option<Cut>,
}

impl Read for Body<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let want = usize::try_from(self.left).map_or(buf.len(), |l| l.min(buf.len()));
        if want == 0 {
            return Ok(0);
        }
        if !self.rest.is_empty() {
            let n = want.min(self.rest.len());
            buf[..n].copy_from_slice(&self.rest[..n]);
            self.rest = &self.rest[n..];
            self.left -= n as u64;
            return Ok(n);
        }
        loop {
            let time = self.deadline.saturating_duration_since(Instant::now());
            if time.is_zero() || self.stream.set_read_timeout(Some(time)).is_err() {
                self.cut = Some(Cut::Late);
                return Err(io::ErrorKind::TimedOut.into());
            }
            match self.stream.read(&mut buf[..want]) {
                Ok(0) => {
                    self.cut = Some(Cut::Short);
                    return Ok(0);
                }
                Ok(n) => {
                    self.left -= n as u64;
                    return Ok(n);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    let late = matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    );
                    self.cut = Some(if late { Cut::Late } else { Cut::Short });
                    return Err(e);
                }
            }
        }
    }
}

/// `POST /api/v1/attachments`, after the upload `Host`, the method and the
/// query were checked. `site` is the configured host name.
pub(crate) fn upload(
    head: &Head,
    stream: &mut TcpStream,
    site: &str,
    context: &Context<'_>,
) -> Reply {
    let limits = context.limits;
    let deadline = Instant::now() + limits.upload_deadline;
    if let Some(refusal) = same_site(head, site, "an upload") {
        return refusal;
    }
    if !single(head, "Content-Type").is_some_and(octet_stream) {
        return error(
            415,
            "unsupported_media_type",
            "an upload's body must be application/octet-stream: the file's bytes, never a form",
        );
    }
    let len = match length_within(head, limits.max_upload_bytes, "an upload") {
        Ok(len) => len,
        Err(mut refusal) => {
            if refusal.status == 413 {
                refusal = error(
                    413,
                    "body_too_large",
                    "the file is over the upload cap of 25 MiB (26 214 400 bytes); nothing was staged",
                );
            }
            return refusal;
        }
    };
    let already = u64::try_from(head.rest.len()).unwrap_or(u64::MAX).min(len);
    let Some(name) = name_of(head) else {
        drain(stream, len - already, deadline);
        return error(
            400,
            "invalid_request",
            "an upload needs one X-OpenWiz-Name header: the file's name, percent-encoded UTF-8, \
             one name of 1 to 1024 bytes",
        );
    };
    let slot_until = deadline.min(Instant::now() + limits.slot_wait);
    if !context.slots.take(limits.max_uploads, slot_until) {
        drain(stream, len - already, deadline);
        return crate::api::busy(limits);
    }
    let reply = staged(head, stream, &name, len, deadline, context);
    context.slots.give();
    reply
}

fn staged(
    head: &Head,
    stream: &mut TcpStream,
    name: &str,
    len: u64,
    deadline: Instant,
    context: &Context<'_>,
) -> Reply {
    let now = context.clock.now();
    let Ok(store) = (context.attaching.open_store)() else {
        return store_unavailable("nothing was staged");
    };
    let claims = store.recorded_claims(now);
    let Ok(expired) = report::sweep(&store, now, &claims) else {
        return store_unavailable("nothing was staged");
    };
    let rest =
        &head.rest[..usize::try_from(len).map_or(head.rest.len(), |l| head.rest.len().min(l))];
    let mut body = Body {
        stream,
        rest,
        left: len,
        deadline,
        cut: None,
    };
    let free = context.attaching.free_space.as_ref();
    let result = store.stage_stream(name, len, &mut body, now, free);
    match body.cut {
        Some(Cut::Late) => {
            return error(
                408,
                "request_timeout",
                "the file did not arrive within the upload deadline; nothing was staged",
            );
        }
        Some(Cut::Short) => {
            return error(
                400,
                "bad_request",
                "the body ended before its Content-Length; nothing was staged",
            );
        }
        None => {}
    }
    // The store stopped before the end (the core refused the first bytes, or
    // the floor): read what the client still sends, so it can read the answer.
    let left = body.left;
    drain(body.stream, left, deadline);
    let mut add = report::AddReport::new();
    add.expired = expired;
    add.push(0, name.to_owned(), result, now);
    // The CLI's `not_staged`: a store error, which can carry a path, so
    // never in an answer.
    if !add.not_staged.is_empty() {
        return store_unavailable("nothing was staged");
    }
    written(&add)
}

/// `GET /api/v1/attachments`: `openwiz attachment ls --json`.
pub(crate) fn list(attaching: &Attaching, clock: &dyn Clock) -> Reply {
    let now = clock.now();
    // Nothing was ever staged: nothing is made.
    match (attaching.open_store_read_only)() {
        Ok(None) => return written(&report::ListReport::new()),
        Ok(Some(_)) => {}
        Err(_) => return store_unavailable("nothing was read"),
    }
    let Ok(store) = (attaching.open_store)() else {
        return store_unavailable("nothing was read");
    };
    let claims = store.recorded_claims(now);
    let Ok(expired) = report::sweep(&store, now, &claims) else {
        return store_unavailable("nothing was read");
    };
    match report::list(&store, &claims, now, expired) {
        Ok(listed) => written(&listed),
        Err(_) => store_unavailable("nothing was read"),
    }
}

/// `DELETE /api/v1/attachments/{id}`, after the removal `Host`, the method
/// and the query were checked: `openwiz attachment rm <id> --json`.
pub(crate) fn remove(head: &Head, site: &str, id: &str, context: &Context<'_>) -> Reply {
    if let Some(refusal) = same_site(head, site, "a removal") {
        return refusal;
    }
    let lengths = head.values("Content-Length");
    let empty = match lengths[..] {
        [] => true,
        [length] => !length.is_empty() && length.bytes().all(|b| b == b'0'),
        _ => false,
    };
    if !empty || !head.values("Transfer-Encoding").is_empty() {
        return error(400, "bad_request", "a removal has no body");
    }
    let Ok(id) = AttachmentId::parse(id) else {
        return error(
            400,
            "invalid_request",
            "attachment rm takes attachment ids, as attachment ls shows them",
        );
    };
    let now = context.clock.now();
    let Ok(store) = (context.attaching.open_store)() else {
        return store_unavailable("nothing was removed");
    };
    let claims = store.recorded_claims(now);
    let Ok(expired) = report::sweep(&store, now, &claims) else {
        return store_unavailable("nothing was removed");
    };
    let removed = report::remove(&store, std::slice::from_ref(&id), &claims, expired);
    // The CLI's `not_removed`: a store error, never in an answer.
    if removed
        .ids
        .iter()
        .any(|entry| entry.result == RemoveResult::NotRemoved)
    {
        return store_unavailable("nothing was removed");
    }
    written(&removed)
}
