//! The new-thread routes: the create (`POST
//! /api/v1/projects/{id}/threads`), the new-thread preview (`POST
//! /api/v1/projects/{id}/threads/preview`), the Cloud options and the
//! status. See `contracts/web-api.v1.md`, "Creating".
//!
//! The server checks only the request's shape: the create `Host`, the
//! send's cross-site checks, the body's
//! framing, size and JSON, and a cap of one create at a time. Then
//! it calls the core's `new_thread`, `preview_new_thread`,
//! `new_thread_status` or `cloud_options`, exactly as `openwiz new` and
//! `openwiz models` do, and answers with what the CLI prints. It works out
//! no option, effect, outcome, stage or retry. Creates are never
//! coalesced: each accepted request is its own call of `new_thread`, and
//! only the op id joins two of them. There is no retry, and
//! nothing of a request is logged.

use std::net::TcpStream;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use openwiz_core::limits::READ_TIMEOUT;
use openwiz_core::messaging::{InvalidRequest, OpId};
use openwiz_core::new_thread::operation::{
    NewThreadError, NewThreadResult, new_thread_with, preview_new_thread_with,
};
use openwiz_core::new_thread::options::cloud_options;
use openwiz_core::new_thread::outcome::Outcome;
use openwiz_core::new_thread::start::start_view;
use openwiz_core::new_thread::status::{NewThreadStatus, StatusResult, new_thread_status};
use openwiz_core::new_thread::{
    NEW_THREAD_BUDGET, NewThreadRequest, ProjectRef, STATUS_SCHEMA_ID, Settings,
};
use openwiz_core::ops::Clock;
use openwiz_core::prompt::PromptPayload;
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};

use crate::api::{error, ok, pretty};
use crate::http::{Head, Reply};
use crate::send::{Slots, attachment_ids, cross_site, length, read_body, staged_for};
use crate::{Attaching, Creating, Limits};

/// A create's body, and the new-thread preview's: `openwiz new`'s options,
/// without the project, which is the path's. Read as exactly one shape,
/// and nothing else: a JSON
/// object whose keys are exactly these six, each once, each of its own JSON
/// type: `attachments` an array of strings, the staged ids in the owner's
/// order.
struct Body {
    op_id: String,
    harness: String,
    model: String,
    effort: String,
    text: String,
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
        f.write_str("one JSON object with op_id, harness, model, effort, text and attachments")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Body, A::Error> {
        fn once<T, E: de::Error>(slot: &mut Option<T>, value: T) -> Result<(), E> {
            if slot.replace(value).is_some() {
                return Err(E::custom("a key is repeated"));
            }
            Ok(())
        }
        let (mut op_id, mut harness, mut model) = (None, None, None);
        let (mut effort, mut text, mut attachments) = (None, None, None);
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "op_id" => once(&mut op_id, map.next_value::<String>()?)?,
                "harness" => once(&mut harness, map.next_value::<String>()?)?,
                "model" => once(&mut model, map.next_value::<String>()?)?,
                "effort" => once(&mut effort, map.next_value::<String>()?)?,
                "text" => once(&mut text, map.next_value::<String>()?)?,
                "attachments" => once(&mut attachments, map.next_value::<Vec<String>>()?)?,
                _ => return Err(de::Error::custom("an unknown key")),
            }
        }
        let missing = || de::Error::custom("a required key is missing");
        Ok(Body {
            op_id: op_id.ok_or_else(missing)?,
            harness: harness.ok_or_else(missing)?,
            model: model.ok_or_else(missing)?,
            effort: effort.ok_or_else(missing)?,
            text: text.ok_or_else(missing)?,
            attachments: attachments.ok_or_else(missing)?,
        })
    }
}

const BODY_RULE: &str = "the body must be one JSON object with op_id, harness, model, effort and \
                         text (strings) and attachments (an array of attachment ids), and nothing \
                         else";

fn record_unavailable() -> Reply {
    error(
        500,
        "record_unavailable",
        "the new-thread record could not be used; nothing was created",
    )
}

fn written(value: &impl serde::Serialize) -> Reply {
    pretty(value).map_or_else(
        || error(500, "internal", "the result could not be written"),
        ok,
    )
}

/// One create or preview's request, as `openwiz new --project-id <id>`
/// builds it from the same options. Read once from the body; kept as the
/// body's strings so a preview can be a shared core run.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Asked {
    project: String,
    op_id: String,
    harness: String,
    model: String,
    effort: String,
    text: String,
    attachments: Vec<String>,
}

impl Asked {
    /// The core's request, after its rules, in `openwiz new`'s order: the op
    /// id, then the request (the text, the project and the settings).
    fn request(&self) -> Result<NewThreadRequest, Reply> {
        // The attachment ids first, as `openwiz new` parses `--attachment`.
        let attachments = attachment_ids(&self.attachments)?;
        let invalid = |InvalidRequest(why)| error(400, "invalid_request", &why);
        let op_id = OpId::parse(&self.op_id).map_err(invalid)?;
        let request = NewThreadRequest::new(
            ProjectRef::Id(self.project.clone()),
            Settings {
                harness: self.harness.clone(),
                model: self.model.clone(),
                effort: self.effort.clone(),
            },
            PromptPayload {
                text: self.text.clone(),
                attachments,
            },
            op_id,
        );
        request.validate().map_err(invalid)?;
        Ok(request)
    }
}

/// A create's or preview's headers and body, after the `Host`, the method
/// and the query were checked: the cross-site checks, the framing, the
/// body's arrival and its JSON, then the core's rules. `site` is the
/// configured host name; `what` names the request in a refusal.
pub(crate) fn read_asked(
    head: &Head,
    stream: &mut TcpStream,
    site: &str,
    project: &str,
    limits: &Limits,
    what: &str,
) -> Result<Asked, Reply> {
    if let Some(refusal) = cross_site(head, site, what) {
        return Err(refusal);
    }
    let len = length(head, limits, what)?;
    let bytes = read_body(stream, &head.rest, len, limits.body_timeout)?;
    let Ok(body) = serde_json::from_slice::<Body>(&bytes) else {
        return Err(error(400, "invalid_request", BODY_RULE));
    };
    let asked = Asked {
        project: project.to_owned(),
        op_id: body.op_id,
        harness: body.harness,
        model: body.model,
        effort: body.effort,
        text: body.text,
        attachments: body.attachments,
    };
    asked.request().map(|_| asked)
}

/// Everything a create needs from the server.
pub(crate) struct Context<'a> {
    pub creating: &'a Creating,
    /// The attachment store, for a create with attachments.
    pub attaching: Option<&'a Attaching>,
    pub slots: &'a Arc<Slots>,
    pub clock: &'a Arc<dyn Clock>,
    pub limits: &'a Limits,
    pub deadline: Instant,
}

/// `POST /api/v1/projects/{id}/threads`, after the create `Host`, the
/// method and the query were checked: `openwiz new --project-id <id> …
/// --yes --json`, with a created thread's start read once (the web
/// follows it through the status route).
pub(crate) fn create(
    head: &Head,
    stream: &mut TcpStream,
    site: &str,
    project: &str,
    context: &Context<'_>,
) -> Reply {
    let asked = match read_asked(head, stream, site, project, context.limits, "a create") {
        Ok(asked) => asked,
        Err(refusal) => return refusal,
    };
    let request = match asked.request() {
        Ok(request) => request,
        Err(refusal) => return refusal,
    };
    let slot_until = context
        .deadline
        .min(Instant::now() + context.limits.slot_wait);
    if !context.slots.take(context.limits.max_creates, slot_until) {
        return crate::api::busy(context.limits);
    }
    // The run keeps its slot until the core returns, even past the
    // deadline: it is never cancelled, and records its own outcome.
    let (tx, rx) = mpsc::channel();
    let slots = context.slots.clone();
    let (creating, clock) = (context.creating.clone(), context.clock.clone());
    let attaching = context.attaching.cloned();
    let spawned = thread::Builder::new()
        .name("openwiz-server create".to_owned())
        .spawn(move || {
            let reply = panic::catch_unwind(AssertUnwindSafe(|| {
                run_create(&creating, attaching.as_ref(), clock.as_ref(), &request)
            }))
            .unwrap_or_else(|_| error(500, "internal", "the create failed"));
            slots.give();
            let _ = tx.send(reply);
        });
    if spawned.is_err() {
        context.slots.give();
        return error(500, "internal", "the create could not be started");
    }
    let left = context.deadline.saturating_duration_since(Instant::now());
    rx.recv_timeout(left).unwrap_or_else(|_| {
        error(
            504,
            "deadline",
            "no answer within the request deadline; the create may still happen and is recorded",
        )
    })
}

/// The core's `new_thread`, then, for a created thread, one read of its
/// start: the CLI's first follow read, against the same requested `spawn`
/// provider (or harness), model and effort.
fn run_create(
    creating: &Creating,
    attaching: Option<&Attaching>,
    clock: &dyn Clock,
    request: &NewThreadRequest,
) -> Reply {
    let staged = match staged_for(
        &request.prompt.attachments,
        attaching,
        "nothing was created",
    ) {
        Ok(staged) => staged,
        Err(refusal) => return refusal,
    };
    let store = match (creating.open_store)() {
        Ok(store) => store,
        Err(_) => return record_unavailable(),
    };
    let client = creating.client.as_ref();
    match new_thread_with(client, store.as_ref(), staged.as_ref(), clock, request) {
        Ok(mut report) => {
            if let NewThreadResult::Outcome {
                outcome: Outcome::Created { thread_id, .. },
                ..
            } = &report.result
            {
                let provider = report
                    .provider
                    .as_deref()
                    .unwrap_or(&report.requested.harness);
                let requested = (
                    provider,
                    report.requested.model.as_str(),
                    report.requested.effort.as_str(),
                );
                let read = client.show_thread(thread_id, clock.monotonic() + READ_TIMEOUT);
                report.start = Some(start_view(thread_id, requested, read, clock.now()));
            }
            written(&report)
        }
        Err(NewThreadError::Invalid(InvalidRequest(why))) => error(400, "invalid_request", &why),
        Err(NewThreadError::Store(_)) => record_unavailable(),
    }
}

/// The new-thread preview: `openwiz new --project-id <id> … --json`
/// without `--yes`. Reads only: no claim, no record, no `spawn`.
pub(crate) fn run_preview(
    creating: &Creating,
    attaching: Option<&Attaching>,
    clock: &dyn Clock,
    asked: &Asked,
) -> Reply {
    let request = match asked.request() {
        Ok(request) => request,
        Err(refusal) => return refusal,
    };
    let staged = match staged_for(
        &request.prompt.attachments,
        attaching,
        "nothing was created",
    ) {
        Ok(staged) => staged,
        Err(refusal) => return refusal,
    };
    match preview_new_thread_with(creating.client.as_ref(), staged.as_ref(), clock, &request) {
        Ok(preview) => written(&preview),
        Err(NewThreadError::Invalid(InvalidRequest(why))) => error(400, "invalid_request", &why),
        Err(NewThreadError::Store(_)) => error(
            500,
            "store_unavailable",
            "the attachment store could not be used; nothing was created",
        ),
    }
}

/// The Cloud options: `openwiz models --json`, one catalogue read.
pub(crate) fn run_options(creating: &Creating, clock: &dyn Clock) -> Reply {
    let client = creating.client.as_ref();
    let deadline = clock.monotonic() + READ_TIMEOUT;
    let options = cloud_options(client.catalogue(deadline), client.cli_levels(), clock.now());
    written(&options)
}

/// The op id of a status read, by the core's rule.
pub(crate) fn status_op_id(op_id: &str) -> Result<OpId, Reply> {
    OpId::parse(op_id).map_err(|InvalidRequest(why)| error(400, "invalid_request", &why))
}

/// The status of `op_id`: `openwiz new --status <op_id> --json`, through
/// the read-only opener. Never claims, writes or creates; never `spawn`.
pub(crate) fn run_status(creating: &Creating, clock: &dyn Clock, op_id: &str) -> Reply {
    let op_id = match status_op_id(op_id) {
        Ok(op_id) => op_id,
        Err(refusal) => return refusal,
    };
    let Ok(store) = (creating.open_store_read_only)() else {
        return record_unavailable();
    };
    let status = match store {
        Some(store) => match new_thread_status(
            creating.client.as_ref(),
            store.as_ref(),
            clock,
            &op_id,
            NEW_THREAD_BUDGET,
        ) {
            Ok(status) => status,
            Err(_) => return record_unavailable(),
        },
        // No record yet anywhere: nothing to read.
        None => NewThreadStatus {
            schema: STATUS_SCHEMA_ID.to_owned(),
            op_id,
            result: StatusResult::NoRecord,
        },
    };
    written(&status)
}
