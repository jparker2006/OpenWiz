//! The one write: `POST /api/v1/sessions/{id}/messages`, and the send
//! lookup. See `contracts/web-api.v1.md`, "Sending".
//!
//! The server checks only the request's shape: the send `Host`, the
//! cross-site checks (write-security §4: `Origin`, `Sec-Fetch-Site`,
//! `X-OpenWiz-Send`, `Content-Type`), the body's framing, size and JSON,
//! and a cap on sends at once. Then it calls the core's `send`, exactly as
//! `openwiz send` does, and answers with the report the CLI prints. It
//! works out no outcome, effect, label or retry. Sends are never
//! coalesced: each accepted request is its own call of `send`, and only the
//! op id joins two of them. Nothing of a request is logged.

use std::io::{self, Read};
use std::net::TcpStream;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use openwiz_core::attachment::AttachmentId;
use openwiz_core::messaging::lookup;
use openwiz_core::messaging::send::{self, SendError, SendRequest};
use openwiz_core::messaging::{InvalidRequest, Mode, OpId, validate_text};
use openwiz_core::model::SessionId;
use openwiz_core::ops::Clock;
use openwiz_core::prompt::{NoAttachments, StagedAttachments};
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};

use crate::api::{error, ok, pretty};
use crate::http::{Head, Reply};
use crate::{Attaching, Limits, Sending};

/// The custom header a same-origin page sends with a send (C4).
pub(crate) const CUSTOM_HEADER: &str = "X-OpenWiz-Send";

struct Body {
    op_id: String,
    mode: Mode,
    text: String,
    resume_paused_queue: bool,
    accept_possible_duplicate: bool,
    attachments: Vec<String>,
}

impl<'de> Deserialize<'de> for Body {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(BodyShape)
    }
}

/// Reads [`Body`] from a map only: any other JSON form is the visitor's
/// "invalid type" error.
struct BodyShape;

impl<'de> Visitor<'de> for BodyShape {
    type Value = Body;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("one JSON object with op_id, mode and text")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Body, A::Error> {
        fn once<T, E: de::Error>(slot: &mut Option<T>, value: T) -> Result<(), E> {
            if slot.replace(value).is_some() {
                return Err(E::custom("a key is repeated"));
            }
            Ok(())
        }
        let (mut op_id, mut mode, mut text) = (None, None, None);
        let (mut resume, mut accept, mut attachments) = (None, None, None);
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "op_id" => once(&mut op_id, map.next_value::<String>()?)?,
                "mode" => {
                    // The core's own words for its two modes, exactly.
                    let word = map.next_value::<String>()?;
                    let value = [Mode::Queue, Mode::Steer]
                        .into_iter()
                        .find(|mode| mode.as_str() == word)
                        .ok_or_else(|| de::Error::custom("mode is queue or steer"))?;
                    once(&mut mode, value)?;
                }
                "text" => once(&mut text, map.next_value::<String>()?)?,
                "resume_paused_queue" => once(&mut resume, map.next_value::<bool>()?)?,
                "accept_possible_duplicate" => once(&mut accept, map.next_value::<bool>()?)?,
                "attachments" => once(&mut attachments, map.next_value::<Vec<String>>()?)?,
                _ => return Err(de::Error::custom("an unknown key")),
            }
        }
        let missing = || de::Error::custom("a required key is missing");
        Ok(Body {
            op_id: op_id.ok_or_else(missing)?,
            mode: mode.ok_or_else(missing)?,
            text: text.ok_or_else(missing)?,
            resume_paused_queue: resume.unwrap_or(false),
            accept_possible_duplicate: accept.unwrap_or(false),
            attachments: attachments.unwrap_or_default(),
        })
    }
}

const BODY_RULE: &str = "the body must be one JSON object with op_id, mode (queue or steer) and \
                         text, and optionally resume_paused_queue and accept_possible_duplicate \
                         (true or false) and attachments (an array of attachment ids), and \
                         nothing else";

fn record_unavailable() -> Reply {
    error(
        500,
        "record_unavailable",
        "the send record could not be used; nothing was sent",
    )
}

/// The one value of `name`, if it was sent exactly once.
pub(crate) fn single<'h>(head: &'h Head, name: &str) -> Option<&'h str> {
    match head.values(name)[..] {
        [value] => Some(value),
        _ => None,
    }
}

pub(crate) fn cross_site(head: &Head, site: &str, what: &str) -> Option<Reply> {
    if let Some(refusal) = same_site(head, site, what) {
        return Some(refusal);
    }
    if !single(head, "Content-Type").is_some_and(json_type) {
        return Some(error(
            415,
            "unsupported_media_type",
            &format!("{what}'s body must be application/json"),
        ));
    }
    None
}

pub(crate) fn same_site(head: &Head, site: &str, what: &str) -> Option<Reply> {
    let origin_ok = single(head, "Origin")
        .and_then(|origin| origin.strip_prefix("https://"))
        .is_some_and(|host| host.eq_ignore_ascii_case(site));
    let fetch_ok = single(head, "Sec-Fetch-Site") == Some("same-origin");
    if !origin_ok || !fetch_ok {
        return Some(error(
            403,
            "forbidden_origin",
            &format!("{what} must come from this site's own page"),
        ));
    }
    if single(head, CUSTOM_HEADER) != Some("1") {
        return Some(error(
            403,
            "missing_custom_header",
            &format!("{what} needs the header X-OpenWiz-Send: 1"),
        ));
    }
    None
}

/// `application/json`, optionally with `; charset=utf-8`.
fn json_type(value: &str) -> bool {
    let mut parts = value.split(';').map(str::trim);
    let media = parts.next().unwrap_or_default();
    if !media.eq_ignore_ascii_case("application/json") {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, _) => true,
        (Some(param), None) => param.eq_ignore_ascii_case("charset=utf-8"),
        _ => false,
    }
}

/// The body's length from its framing, or the refusal. `what` names the
/// request ("a send").
pub(crate) fn length(head: &Head, limits: &Limits, what: &str) -> Result<usize, Reply> {
    let cap = u64::try_from(limits.max_send_body_bytes).unwrap_or(u64::MAX);
    // At most the send cap, which is a `usize`.
    length_within(head, cap, what).map(|n| usize::try_from(n).unwrap_or(usize::MAX))
}

/// The body's length from its framing, at most `cap`, or the refusal.
pub(crate) fn length_within(head: &Head, cap: u64, what: &str) -> Result<u64, Reply> {
    if !head.values("Transfer-Encoding").is_empty() {
        return Err(error(
            400,
            "bad_request",
            "Transfer-Encoding is not accepted",
        ));
    }
    let value = match head.values("Content-Length")[..] {
        [] => {
            return Err(error(
                411,
                "length_required",
                &format!("{what} needs Content-Length"),
            ));
        }
        [value] => value,
        _ => return Err(error(400, "bad_request", "malformed request")),
    };
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error(400, "bad_request", "malformed request"));
    }
    // Too many digits to parse is over the cap too.
    match value.parse::<u64>() {
        Ok(n) if n <= cap => Ok(n),
        _ => Err(error(413, "body_too_large", "the body is over its cap")),
    }
}

/// Reads exactly `len` body bytes: those already read with the head
/// (`rest`), then the stream's, within the body timeout.
pub(crate) fn read_body(
    stream: &mut TcpStream,
    rest: &[u8],
    len: usize,
    timeout: Duration,
) -> Result<Vec<u8>, Reply> {
    let late = || error(408, "request_timeout", "the body was not received in time");
    let mut body = rest[..rest.len().min(len)].to_vec();
    let deadline = Instant::now() + timeout;
    let mut chunk = [0u8; 8192];
    while body.len() < len {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return Err(late());
        }
        let want = (len - body.len()).min(chunk.len());
        match stream.read(&mut chunk[..want]) {
            Ok(0) => return Err(error(400, "bad_request", "the body ended early")),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(late()),
        }
    }
    Ok(body)
}

#[derive(Default)]
pub(crate) struct Slots {
    used: Mutex<usize>,
    freed: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Slots {
    /// Takes a slot within `until`, or `false`.
    pub(crate) fn take(&self, max: usize, until: Instant) -> bool {
        let mut used = lock(&self.used);
        while *used >= max {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            used = self
                .freed
                .wait_timeout(used, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *used += 1;
        true
    }

    pub(crate) fn give(&self) {
        *lock(&self.used) -= 1;
        self.freed.notify_one();
    }

    /// The slots taken now.
    pub(crate) fn used(&self) -> usize {
        *lock(&self.used)
    }
}

/// Everything a send needs from the server.
pub(crate) struct Context<'a> {
    pub sending: &'a Sending,
    /// The attachment store, for a send with attachments.
    pub attaching: Option<&'a Attaching>,
    pub slots: &'a Arc<Slots>,
    pub clock: &'a Arc<dyn Clock>,
    pub limits: &'a Limits,
    pub deadline: Instant,
}

/// `POST /api/v1/sessions/{id}/messages`, after the send `Host`, the method
/// and the query were checked. `site` is the configured host name.
pub(crate) fn post(
    head: &Head,
    stream: &mut TcpStream,
    site: &str,
    id: &str,
    context: &Context<'_>,
) -> Reply {
    if let Some(refusal) = cross_site(head, site, "a send") {
        return refusal;
    }
    let len = match length(head, context.limits, "a send") {
        Ok(len) => len,
        Err(refusal) => return refusal,
    };
    let bytes = match read_body(stream, &head.rest, len, context.limits.body_timeout) {
        Ok(bytes) => bytes,
        Err(refusal) => return refusal,
    };
    let Ok(body) = serde_json::from_slice::<Body>(&bytes) else {
        return error(400, "invalid_request", BODY_RULE);
    };
    // In `openwiz send`'s order, with its words.
    let messenger = context.sending.messenger.as_ref();
    let target = SessionId(id.to_owned());
    // The attachment ids first, as `openwiz send` parses `--attachment`.
    let ids = match attachment_ids(&body.attachments) {
        Ok(ids) => ids,
        Err(refusal) => return refusal,
    };
    let checked = validate_text(&body.text)
        .and_then(|()| messenger.check_target_id(&target))
        .and_then(|()| OpId::parse(&body.op_id));
    let op_id = match checked {
        Ok(op_id) => op_id,
        Err(InvalidRequest(why)) => return error(400, "invalid_request", &why),
    };
    let mut request = SendRequest::new(target, body.text, op_id);
    request.mode = body.mode;
    request.resume_paused_queue = body.resume_paused_queue;
    request.accept_possible_duplicate = body.accept_possible_duplicate;
    request.prompt.attachments = ids;
    let attaching = context.attaching.cloned();

    let slot_until = context
        .deadline
        .min(Instant::now() + context.limits.slot_wait);
    if !context.slots.take(context.limits.max_sends, slot_until) {
        return crate::api::busy(context.limits);
    }
    // The run keeps its slot until the core returns, even past the
    // deadline: it is never cancelled, and records its own outcome.
    let (tx, rx) = mpsc::channel();
    let slots = context.slots.clone();
    let (sending, clock) = (context.sending.clone(), context.clock.clone());
    let spawned = thread::Builder::new()
        .name("openwiz-server send".to_owned())
        .spawn(move || {
            let reply = panic::catch_unwind(AssertUnwindSafe(|| {
                // B2: the attachment store only for a send with
                // attachments, as `openwiz send` opens it.
                let staged = match staged_for(
                    &request.prompt.attachments,
                    attaching.as_ref(),
                    "nothing was sent",
                ) {
                    Ok(staged) => staged,
                    Err(refusal) => return refusal,
                };
                let store = match (sending.open_store)() {
                    Ok(store) => store,
                    Err(_) => return record_unavailable(),
                };
                let sent = send::send_with(
                    sending.messenger.as_ref(),
                    store.as_ref(),
                    staged.as_ref(),
                    clock.as_ref(),
                    &request,
                );
                match sent {
                    Ok(report) => pretty(&report).map_or_else(
                        || error(500, "internal", "the result could not be written"),
                        ok,
                    ),
                    Err(SendError::Invalid(InvalidRequest(why))) => {
                        error(400, "invalid_request", &why)
                    }
                    Err(SendError::Store(_)) => record_unavailable(),
                }
            }))
            .unwrap_or_else(|_| error(500, "internal", "the send failed"));
            slots.give();
            let _ = tx.send(reply);
        });
    if spawned.is_err() {
        context.slots.give();
        return error(500, "internal", "the send could not be started");
    }
    let left = context.deadline.saturating_duration_since(Instant::now());
    rx.recv_timeout(left).unwrap_or_else(|_| {
        error(
            504,
            "deadline",
            "no answer within the request deadline; the send may still happen and is recorded",
        )
    })
}

/// The staged ids of a body, by the core's id rule, in the owner's order:
/// `openwiz send` and `new`'s `--attachment <id>`.
pub(crate) fn attachment_ids(ids: &[String]) -> Result<Vec<AttachmentId>, Reply> {
    ids.iter()
        .map(|id| AttachmentId::parse(id))
        .collect::<Result<_, _>>()
        .map_err(|e| error(400, "invalid_request", &e.0))
}

/// What the core's gate reads the attachments from: nothing for a prompt
/// without any (the store is never opened), the attachment store for one
/// with some, and with no attachment configuration, nothing, so the gate
/// finds none and refuses each. A store that can't be opened is the
/// refusal: nothing is claimed, read or sent.
pub(crate) fn staged_for(
    ids: &[AttachmentId],
    attaching: Option<&Attaching>,
    nothing: &str,
) -> Result<Box<dyn StagedAttachments>, Reply> {
    match attaching {
        Some(attaching) if !ids.is_empty() => match (attaching.open_store)() {
            Ok(store) => Ok(Box::new(store)),
            Err(_) => Err(error(
                500,
                "store_unavailable",
                &format!("the attachment store could not be used; {nothing}"),
            )),
        },
        _ => Ok(Box::new(NoAttachments)),
    }
}

/// `GET /api/v1/sessions/{id}/messages/{op_id}`: the send record of
/// `op_id`, read only. No `room-cli` call.
pub(crate) fn lookup(sending: &Sending, clock: &dyn Clock, id: &str, op_id: &str) -> Reply {
    let target = SessionId(id.to_owned());
    let op_id = match sending
        .messenger
        .check_target_id(&target)
        .and_then(|()| OpId::parse(op_id))
    {
        Ok(op_id) => op_id,
        Err(InvalidRequest(why)) => return error(400, "invalid_request", &why),
    };
    let Ok(store) = (sending.open_store_read_only)() else {
        return record_unavailable();
    };
    match lookup::lookup(store.as_deref(), clock, &target, &op_id) {
        Ok(report) => pretty(&report).map_or_else(
            || error(500, "internal", "the result could not be written"),
            ok,
        ),
        Err(_) => record_unavailable(),
    }
}
