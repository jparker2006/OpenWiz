//! The CloudRoom side of `send`: the target read and the one write, `thread
//! tell`, translated for the core's [`Messenger`].
//!
//! The PC probe c687079  observed sent (Queue or Steer), busy
//! Queue, and Claude Code queued Steer receipts. Other forms are uncertain.
//!
//! Nothing `room-cli` prints reaches a reason, gap or message from here:
//! receipts and errors are matched against fixed forms and turned into
//! OpenWiz's own words. Only the validated queued message id is passed on,
//! separately from those fixed words.

use std::time::Instant;

use openwiz_core::messaging::effects::{Activity, QueuedWork, TargetRecord};
use openwiz_core::messaging::send::{
    Messenger, Rejection, SendGap, TellReply, UncertainReason, WaitingOn,
};
use openwiz_core::messaging::{InvalidRequest, Mode};
use openwiz_core::model::{Fact, SessionId};
use openwiz_core::prompt::ReadyPrompt;
use openwiz_core::source::{Placement, SessionSource, SourceError};
use serde_json::Value;

use crate::allowlist::{Invocation, ThreadId};
use crate::process;
use crate::source::CloudRoomSource;

/// The documented rejections before acceptance, as the first line `room-cli`
/// prints on stderr. Only an exact match is a rejection; any other error is
/// uncertain, the safe direction.
///
/// - The not-found line was observed for `thread show` and `thread output`
///   in capture C. That `thread tell` prints the same line is **documented,
///   not observed**.
/// - The guide documents that invalid input and unsupported modes are
///   rejected ("Explicit unsupported modes fail; no fallback"), **not their
///   wording**. These two lines are placeholders, labelled synthetic in the
///   fixtures. At the pre-live recheck they are replaced by the real
///   wording; until then a real rejection of either kind reports uncertain.
pub const DOCUMENTED_REJECTIONS: &[(&str, &str)] = &[
    ("Error: HTTP 404: Thread not found", "not_found"),
    ("Error: HTTP 400: Invalid input", "invalid_input"),
    ("Error: HTTP 400: Unsupported mode", "unsupported_mode"),
];

/// How the runner describes a program that could not be started.
const NOT_STARTED: &str = "could not be started";

/// The exact keys observed for sent in either requested mode .
pub const RECEIPT_KEYS: &[&str] = &["threadId", "ok", "delivery", "mode"];

impl Messenger for CloudRoomSource {
    fn name(&self) -> &str {
        crate::PROGRAM
    }

    fn tested_version(&self) -> &str {
        crate::TESTED_VERSION
    }

    fn check_target_id(&self, id: &SessionId) -> Result<(), InvalidRequest> {
        ThreadId::parse(&id.0).map(|_| ()).map_err(|_| {
            InvalidRequest(
                "a session id is 1 to 128 ASCII letters, digits, _ or -, not starting with -"
                    .to_owned(),
            )
        })
    }

    fn version(&self, deadline: Instant) -> Result<String, SourceError> {
        SessionSource::version(self, deadline).map_err(|e| match e {
            // The version read's own malformed details are OpenWiz's fixed
            // words , such as "printed a version that is not N.N.N
            // or v<N> (detail withheld)": kept, as the reads show them.
            SourceError::Malformed { .. } => e,
            other => withhold(other),
        })
    }

    fn read_target(&self, id: &SessionId, deadline: Instant) -> Result<TargetRecord, SourceError> {
        let thread_id = ThreadId::parse(&id.0).map_err(|_| SourceError::InvalidId {
            id: id.0.clone(),
            detail: "not a safe id".to_owned(),
        })?;
        let show = match process::run(self.program(), &Invocation::ThreadShow(thread_id), deadline)
        {
            Ok(out) => out,
            Err(SourceError::CommandFailed { detail, .. })
                if rejection(&detail) == Some(Rejection::NotFound) =>
            {
                return Err(SourceError::NotFound {
                    subject: id.0.clone(),
                });
            }
            Err(e) => return Err(withhold(e)),
        };
        let mut target = parse_show(id, &show)?;
        self.add_list_row(&mut target, deadline);
        self.add_host(&mut target, deadline);
        Ok(target)
    }

    fn tell(&self, id: &SessionId, mode: Mode, text: &str, deadline: Instant) -> TellReply {
        self.tell_prompt(id, mode, &ReadyPrompt::text(text), deadline)
    }

    fn tell_prompt(
        &self,
        id: &SessionId,
        mode: Mode,
        prompt: &ReadyPrompt,
        deadline: Instant,
    ) -> TellReply {
        let Ok(thread_id) = ThreadId::parse(&id.0) else {
            // Never reached: `send` checks the id first. Nothing is run.
            return TellReply::Rejected(Rejection::InvalidInput);
        };
        if mode == Mode::Steer && !prompt.attachments.is_empty() {
            // Never reached: `send` refuses Steer with attachments first
            // (the check; a Cloud steer drops them, F16). Nothing is run.
            return TellReply::NotStarted;
        }
        let invocation = Invocation::ThreadTell {
            id: thread_id,
            mode,
            text: prompt.text.clone(),
            attachments: prompt.dispatch_order(),
        };
        match process::run(self.program(), &invocation, deadline) {
            Ok(out) => receipt(&out, id, mode),
            Err(SourceError::TimedOut { .. }) => unclear(
                UncertainReason::TimedOut,
                "thread tell did not answer in time; it was stopped and not retried",
            ),
            Err(SourceError::CommandFailed {
                exit_code: None, ..
            }) => unclear(
                UncertainReason::EndedWithoutExitCode,
                "thread tell ended without an exit code",
            ),
            Err(SourceError::CommandFailed {
                exit_code: Some(code),
                detail,
            }) => match rejection(&detail) {
                Some(rejection) => TellReply::Rejected(rejection),
                None => TellReply::Unclear {
                    reason: UncertainReason::UndocumentedError,
                    gaps: vec![gap(format!(
                        "thread tell exited {code} with an error that is not a documented \
                         rejection (its output is withheld)"
                    ))],
                },
            },
            Err(SourceError::Malformed { .. }) => unclear(
                UncertainReason::UnexpectedReceipt,
                "thread tell printed more than OpenWiz reads",
            ),
            // The runner's own words for a program that never started
            // (`process::run`); nothing of room-cli's is matched here.
            Err(SourceError::Unavailable { detail }) if detail.contains(NOT_STARTED) => {
                TellReply::NotStarted
            }
            Err(_) => unclear(
                UncertainReason::CallNotConfirmed,
                "thread tell could not be confirmed to have run cleanly",
            ),
        }
    }
}

impl CloudRoomSource {
    /// `queuedWork` and `hasPendingInteraction`, from the target's row in
    /// `thread list`. Anything not read stays unknown, with a gap.
    fn add_list_row(&self, target: &mut TargetRecord, deadline: Instant) {
        let invocation = Invocation::ThreadList {
            include_hidden: false,
            include_archived: false,
        };
        let row = match read_json(self, &invocation, deadline) {
            Ok(Value::Array(rows)) => rows
                .into_iter()
                .find(|row| row.get("id").and_then(Value::as_str) == Some(target.id.0.as_str())),
            Ok(_) => {
                let why = "thread list is not the documented shape".to_owned();
                target.problems.push(read_gap("thread list", &why));
                return not_listed(target, &why);
            }
            Err(e) => {
                let why = described("thread list", &e);
                target.problems.push(read_gap("thread list", &why));
                return not_listed(target, &why);
            }
        };
        let Some(row) = row else {
            return not_listed(target, "the session is not in thread list");
        };
        let mut fields = Fields::new(&row, "thread list", &mut target.problems);
        target.queued_work = match fields.string("queuedWork") {
            Fact::Known(work) => match work.as_str() {
                "none" => Fact::Known(QueuedWork::None),
                "waiting" => Fact::Known(QueuedWork::Waiting),
                _ => Fact::Unknown(fields.unrecognised("queuedWork")),
            },
            Fact::Unknown(why) => Fact::Unknown(why),
        };
        target.pending_interaction = fields.boolean("hasPendingInteraction");
    }

    /// For a local session, whether its host is connected, from `machine
    /// list`.
    fn add_host(&self, target: &mut TargetRecord, deadline: Instant) {
        let Fact::Known(Placement::Host(host)) = &target.placement else {
            return;
        };
        let host = host.clone();
        let connected = match read_json(self, &Invocation::MachineList, deadline) {
            Ok(Value::Array(rows)) => {
                match rows
                    .iter()
                    .find(|row| row.get("id").and_then(Value::as_str) == Some(host.as_str()))
                {
                    None => Fact::Unknown("its host is not in machine list".to_owned()),
                    Some(row) => {
                        let mut fields = Fields::new(row, "machine list", &mut target.problems);
                        match fields.string("status") {
                            Fact::Known(status) if status == "connected" => Fact::Known(true),
                            // No other status value was observed or documented.
                            Fact::Known(_) => Fact::Unknown(fields.unrecognised("status")),
                            Fact::Unknown(why) => Fact::Unknown(why),
                        }
                    }
                }
            }
            Ok(_) => {
                let why = "machine list is not the documented shape".to_owned();
                target.problems.push(read_gap("machine list", &why));
                Fact::Unknown(why)
            }
            Err(e) => {
                let why = described("machine list", &e);
                target.problems.push(read_gap("machine list", &why));
                Fact::Unknown(why)
            }
        };
        target.host_connected = Some(connected);
    }
}

fn not_listed(target: &mut TargetRecord, why: &str) {
    target.queued_work = Fact::Unknown(why.to_owned());
    target.pending_interaction = Fact::Unknown(why.to_owned());
}

fn read_json(
    source: &CloudRoomSource,
    invocation: &Invocation,
    deadline: Instant,
) -> Result<Value, SourceError> {
    let out = process::run(source.program(), invocation, deadline).map_err(withhold)?;
    serde_json::from_slice(&out).map_err(|_| SourceError::Malformed {
        detail: "not JSON".to_owned(),
    })
}

/// Drops `room-cli`'s own text from an error, keeping only its category.
fn withhold(error: SourceError) -> SourceError {
    match error {
        SourceError::CommandFailed { exit_code, .. } => SourceError::CommandFailed {
            exit_code,
            detail: "output withheld".to_owned(),
        },
        SourceError::Malformed { .. } => SourceError::Malformed {
            detail: "output withheld".to_owned(),
        },
        SourceError::Unavailable { .. } => SourceError::Unavailable {
            detail: "could not be started".to_owned(),
        },
        other => other,
    }
}

/// A failed read in OpenWiz's own words.
fn described(read: &str, error: &SourceError) -> String {
    match error {
        SourceError::CommandFailed {
            exit_code: Some(code),
            ..
        } => format!("{read} exited {code}"),
        SourceError::CommandFailed { .. } => format!("{read} ended without an exit code"),
        SourceError::TimedOut { after_ms } => format!("{read} timed out after {after_ms} ms"),
        SourceError::Malformed { .. } => format!("{read} returned output that is not JSON"),
        _ => format!("{read} could not be read"),
    }
}

fn read_gap(subject: &str, detail: &str) -> SendGap {
    SendGap {
        subject: subject.to_owned(),
        detail: detail.to_owned(),
    }
}

/// The target as `thread show` reports it. Fields the effects need from
/// other reads start unknown.
fn parse_show(id: &SessionId, out: &[u8]) -> Result<TargetRecord, SourceError> {
    let malformed = |detail: &str| SourceError::Malformed {
        detail: detail.to_owned(),
    };
    let doc: Value = serde_json::from_slice(out).map_err(|_| malformed("not JSON"))?;
    let Some(thread) = doc.get("thread").filter(|t| t.is_object()) else {
        return Err(malformed("no thread object"));
    };
    if thread.get("id").and_then(Value::as_str) != Some(id.0.as_str()) {
        return Err(malformed("it describes another session"));
    }
    let mut problems = Vec::new();
    let mut f = Fields::new(thread, "thread show", &mut problems);
    let title = f.string("title");
    let activity = match f.string("status") {
        Fact::Known(status) => match status.as_str() {
            "idle" => Fact::Known(Activity::Idle),
            "active" => Fact::Known(Activity::Running),
            "error" => Fact::Known(Activity::Errored),
            _ => Fact::Unknown(f.unrecognised("status")),
        },
        Fact::Unknown(why) => Fact::Unknown(why),
    };
    let hidden = match f.string("visibility") {
        Fact::Known(v) if v == "visible" => Fact::Known(false),
        Fact::Known(v) if v == "hidden" => Fact::Known(true),
        Fact::Known(_) => Fact::Unknown(f.unrecognised("visibility")),
        Fact::Unknown(why) => Fact::Unknown(why),
    };
    let archived = match thread.get("archivedAt") {
        Some(Value::Null) => Fact::Known(false),
        Some(v) if v.is_i64() => Fact::Known(true),
        Some(_) => Fact::Unknown(f.drift("archivedAt", "has an unexpected type")),
        None => Fact::Unknown(f.drift("archivedAt", "is missing")),
    };
    let placement = match thread.get("executionTarget") {
        Some(Value::String(t)) if t == "cloud" => Fact::Known(Placement::Cloud),
        Some(_) => Fact::Unknown(f.unrecognised("executionTarget")),
        None => match doc.pointer("/environment/hostId") {
            Some(Value::String(host)) => Fact::Known(Placement::Host(host.clone())),
            _ => Fact::Unknown(f.drift("environment.hostId", "is missing")),
        },
    };
    let queued_message_count = match thread.get("queuedMessageCount").and_then(Value::as_u64) {
        Some(n) => Fact::Known(n),
        None => Fact::Unknown(f.drift("queuedMessageCount", "is missing or not a count")),
    };
    let not_yet = "not read".to_owned();
    Ok(TargetRecord {
        id: id.clone(),
        title,
        activity,
        hidden,
        archived,
        placement,
        host_connected: None,
        queued_work: Fact::Unknown(not_yet.clone()),
        queued_message_count,
        pending_interaction: Fact::Unknown(not_yet),
        problems,
    })
}

/// Typed fields of one JSON object. Each missing, mistyped or unrecognised
/// one becomes a gap in OpenWiz's words, naming the field, never its value.
struct Fields<'a> {
    object: &'a Value,
    read: &'static str,
    problems: &'a mut Vec<SendGap>,
}

impl<'a> Fields<'a> {
    fn new(object: &'a Value, read: &'static str, problems: &'a mut Vec<SendGap>) -> Self {
        Self {
            object,
            read,
            problems,
        }
    }

    fn drift(&mut self, field: &str, what: &str) -> String {
        let detail = format!("schema drift: {} {field} {what}", self.read);
        self.problems.push(read_gap(self.read, &detail));
        detail
    }

    fn unrecognised(&mut self, field: &str) -> String {
        self.drift(field, "has a value room-cli 0.43.1 did not document")
    }

    fn string(&mut self, field: &str) -> Fact<String> {
        match self.object.get(field) {
            Some(Value::String(s)) => Fact::Known(s.clone()),
            Some(_) => Fact::Unknown(self.drift(field, "has an unexpected type")),
            None => Fact::Unknown(self.drift(field, "is missing")),
        }
    }

    fn boolean(&mut self, field: &str) -> Fact<bool> {
        match self.object.get(field) {
            Some(Value::Bool(b)) => Fact::Known(*b),
            Some(_) => Fact::Unknown(self.drift(field, "has an unexpected type")),
            None => Fact::Unknown(self.drift(field, "is missing")),
        }
    }
}

/// A documented rejection, from the first stderr line, by exact match.
fn rejection(first_line: &str) -> Option<Rejection> {
    let (_, kind) = DOCUMENTED_REJECTIONS
        .iter()
        .find(|(line, _)| *line == first_line)?;
    Some(match *kind {
        "not_found" => Rejection::NotFound,
        "invalid_input" => Rejection::InvalidInput,
        _ => Rejection::UnsupportedMode,
    })
}

fn gap(detail: String) -> SendGap {
    SendGap {
        subject: "thread tell".to_owned(),
        detail,
    }
}

fn unclear(reason: UncertainReason, detail: &str) -> TellReply {
    TellReply::Unclear {
        reason,
        gaps: vec![gap(detail.to_owned())],
    }
}

fn unexpected(detail: &str) -> TellReply {
    unclear(
        UncertainReason::UnexpectedReceipt,
        &format!("receipt drift: {detail}"),
    )
}

/// Read the original JSON once, rejecting repeated keys at every depth.
/// A Value parse alone would silently replace a repeated key.
struct ReceiptJson(Value);

impl<'de> serde::Deserialize<'de> for ReceiptJson {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ReceiptJsonVisitor)
    }
}

struct ReceiptJsonVisitor;

impl<'de> serde::de::Visitor<'de> for ReceiptJsonVisitor {
    type Value = ReceiptJson;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("receipt JSON with each object key once")
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<ReceiptJson, E> {
        Ok(ReceiptJson(Value::Null))
    }
    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<ReceiptJson, E> {
        Ok(ReceiptJson(Value::Bool(v)))
    }
    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<ReceiptJson, E> {
        Ok(ReceiptJson(Value::from(v)))
    }
    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<ReceiptJson, E> {
        Ok(ReceiptJson(Value::from(v)))
    }
    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<ReceiptJson, E> {
        Ok(ReceiptJson(Value::from(v)))
    }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<ReceiptJson, E> {
        Ok(ReceiptJson(Value::String(v.to_owned())))
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<ReceiptJson, A::Error> {
        let mut items = Vec::new();
        while let Some(ReceiptJson(v)) = a.next_element()? {
            items.push(v);
        }
        Ok(ReceiptJson(Value::Array(items)))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<ReceiptJson, A::Error> {
        let mut object = serde_json::Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(serde::de::Error::custom("a receipt key is repeated"));
            }
            let ReceiptJson(value) = a.next_value()?;
            object.insert(key, value);
        }
        Ok(ReceiptJson(Value::Object(object)))
    }
}

fn exact_object<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a serde_json::Map<String, Value>> {
    value
        .as_object()
        .filter(|o| o.len() == keys.len() && keys.iter().all(|key| o.contains_key(*key)))
}

/// Only PC-observed sent and queued shapes (c687079, v141 W1-M08),
/// bound to this request.
/// Required null fields are checked as keys too, never treated as missing.
fn receipt(out: &[u8], target: &SessionId, mode: Mode) -> TellReply {
    let value = match serde_json::from_slice::<ReceiptJson>(out) {
        Ok(ReceiptJson(value)) => value,
        Err(e) if e.classify() == serde_json::error::Category::Data => {
            return unexpected("an unobserved receipt shape or repeated key");
        }
        Err(_) => {
            return unclear(
                UncertainReason::NotJson,
                "thread tell exited 0 with a reply that is not JSON (withheld)",
            );
        }
    };
    let Some(object) = value.as_object() else {
        return unexpected("an unobserved receipt that is not an object");
    };
    if object.get("threadId").and_then(Value::as_str) != Some(&target.0)
        || object.get("ok") != Some(&Value::Bool(true))
        || object.get("mode").and_then(Value::as_str) != Some(mode.as_str())
    {
        return unexpected("an unobserved receipt that does not confirm this target and mode");
    }
    match object.get("delivery").and_then(Value::as_str) {
        Some("sent") if exact_object(&value, RECEIPT_KEYS).is_some() => TellReply::Sent,
        Some("queued")
            if exact_object(
                &value,
                &["threadId", "ok", "delivery", "mode", "queuedMessage"],
            )
            .is_some() =>
        {
            queued_receipt(&object["queuedMessage"], target, mode)
        }
        _ => unexpected("an unobserved receipt shape or delivery"),
    }
}

/// The common sixteen keys; only the Local busy Queue adds hardQueue (false).
const QUEUED_KEYS: &[&str] = &[
    "id",
    "initiator",
    "senderThreadId",
    "threadId",
    "content",
    "model",
    "reasoningLevel",
    "permissionMode",
    "serviceTier",
    "sendAt",
    "waitingOn",
    "failureReason",
    "payload",
    "editable",
    "createdAt",
    "updatedAt",
];

fn queued_receipt(value: &Value, target: &SessionId, mode: Mode) -> TellReply {
    let bad = || unexpected("an unobserved queued receipt shape or value");
    let Some(q) = value.as_object() else {
        return bad();
    };
    // The queued shape follows the host: Cloud uses the same row for
    // Queue and Steer. The Local busy row remains Queue-only.
    let busy = q.contains_key("hardQueue");
    if (busy && mode != Mode::Queue)
        || q.len() != QUEUED_KEYS.len() + usize::from(busy)
        || !QUEUED_KEYS.iter().all(|key| q.contains_key(*key))
        || (busy && q.get("hardQueue") != Some(&Value::Bool(false)))
    {
        return bad();
    }
    if q["threadId"].as_str() != Some(&target.0)
        || q["editable"] != Value::Bool(true)
        || !q["sendAt"].is_null()
        || !q["failureReason"].is_null()
        || !["model", "reasoningLevel", "permissionMode", "serviceTier"]
            .iter()
            .all(|key| q[*key].is_string())
        || !["createdAt", "updatedAt"]
            .iter()
            .all(|key| q[*key].as_i64().is_some())
    {
        return bad();
    }
    let Some(content) = q["content"].as_array().filter(|a| a.len() == 1) else {
        return bad();
    };
    let Some(block) = exact_object(&content[0], &["type", "text", "mentions"]) else {
        return bad();
    };
    if !block["type"].is_string()
        || !block["text"].is_string()
        || !block["mentions"].as_array().is_some_and(Vec::is_empty)
        || !exact_object(&q["payload"], &["kind"])
            .is_some_and(|p| p["kind"].as_str() == Some("inline"))
    {
        return bad();
    }
    let sender = q["senderThreadId"].as_str();
    let agent = q["initiator"].as_str() == Some("agent")
        && sender.is_some_and(|id| {
            id.strip_prefix("thr_").is_some_and(|tail| !tail.is_empty())
                && ThreadId::parse(id).is_ok()
        });
    let user = q["initiator"].as_str() == Some("user") && q["senderThreadId"].is_null();
    let Some(id) = q["id"].as_str() else {
        return bad();
    };
    let waiting_on = if busy {
        if !(agent || user)
            || !id
                .strip_prefix("qmsg_")
                .is_some_and(|tail| !tail.is_empty())
            || ThreadId::parse(id).is_err()
            || !exact_object(&q["waitingOn"], &["kind"])
                .is_some_and(|w| w["kind"].as_str() == Some("thread-busy"))
        {
            return bad();
        }
        WaitingOn::ThreadBusy
    } else {
        if !user || !q["waitingOn"].is_null() || !queued_uuid(id) {
            return bad();
        }
        WaitingOn::NotReported
    };
    TellReply::Queued {
        waiting_on,
        queued_message_id: id.to_owned(),
    }
}

/// The observed Cloud queued id is a UUID. Validate only its wire grammar;
/// no live identifier is embedded in code or fixtures.
fn queued_uuid(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}
