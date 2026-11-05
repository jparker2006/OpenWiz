use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::attachment::{AttachmentId, Kind, PromptAttachment, SafeName, Sha256};
use crate::envelope::SourceInfo;
use crate::messaging::send::UncertainReason;
use crate::messaging::{InvalidRequest, OpId, sha256_hex};
use crate::model::{Fact, Timestamp};
use crate::prompt::{OpKind, OpRef};

use super::effects::{Effect, EffectKind, Effects};
use super::outcome::{FailReason, Gap, Outcome, RefusalReason};
use super::record::{NewThreadRecord, RecordState};
use super::{
    Binding, ProjectRef, ProjectView, RECORD_SCHEMA_ID, Settings, check_setting, request_id,
};

/// A JSON document read with every key at most once per object. Only an
/// integer number is kept: no record field is a fraction.
#[derive(Clone, Debug, PartialEq)]
pub struct StrictJson(Json);

/// One JSON value of a [`StrictJson`] document.
#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Int(i128),
    String(String),
    Array(Vec<Json>),
    /// In document order; no key twice.
    Object(Vec<(String, Json)>),
}

/// Why a stored record could not be read: the path of what was refused, in
/// OpenWiz's words. Never the record's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadError(pub String);

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value with no key given twice and no fractional number")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E: de::Error>(self, b: bool) -> Result<Json, E> {
        Ok(Json::Bool(b))
    }

    fn visit_i64<E: de::Error>(self, n: i64) -> Result<Json, E> {
        Ok(Json::Int(n.into()))
    }

    fn visit_u64<E: de::Error>(self, n: u64) -> Result<Json, E> {
        Ok(Json::Int(n.into()))
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Json, E> {
        Err(E::custom("a fractional number"))
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<Json, E> {
        Ok(Json::String(s.to_owned()))
    }

    fn visit_string<E: de::Error>(self, s: String) -> Result<Json, E> {
        Ok(Json::String(s))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(StrictJson(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut entries: Vec<(String, Json)> = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            if entries.iter().any(|(k, _)| *k == key) {
                return Err(de::Error::custom(format!("the key {key:?} is given twice")));
            }
            let StrictJson(value) = map.next_value()?;
            entries.push((key, value));
        }
        Ok(Json::Object(entries))
    }
}

impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(JsonVisitor).map(StrictJson)
    }
}

/// What reading one value can give.
type Read<T> = Result<T, ReadError>;

fn refused<T>(path: &str, what: &str) -> Read<T> {
    Err(ReadError(format!("{path}: {what}")))
}

/// One JSON object with exactly its keys, each present once.
struct Object<'a> {
    path: String,
    entries: &'a [(String, Json)],
}

impl<'a> Object<'a> {
    fn read(json: &'a Json, path: &str, keys: &[&str]) -> Read<Self> {
        let Json::Object(entries) = json else {
            return refused(path, "not an object");
        };
        if let Some((key, _)) = entries.iter().find(|(k, _)| !keys.contains(&k.as_str())) {
            return refused(path, &format!("the key {key:?} is not one it has"));
        }
        if let Some(key) = keys.iter().find(|k| !entries.iter().any(|(e, _)| e == *k)) {
            return refused(path, &format!("the key {key:?} is missing"));
        }
        Ok(Self {
            path: path.to_owned(),
            entries,
        })
    }

    /// The value of `key`, with its path.
    fn get(&self, key: &str) -> (&'a Json, String) {
        let value = self
            .entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap_or(&Json::Null);
        (value, format!("{}.{key}", self.path))
    }

    fn field<T>(&self, key: &str, read: impl Fn(&Json, &str) -> Read<T>) -> Read<T> {
        let (value, path) = self.get(key);
        read(value, &path)
    }
}

fn string(json: &Json, path: &str) -> Read<String> {
    match json {
        Json::String(s) => Ok(s.clone()),
        _ => refused(path, "not a string"),
    }
}

/// A string of at least one character.
fn text(json: &Json, path: &str) -> Read<String> {
    let s = string(json, path)?;
    if s.is_empty() {
        return refused(path, "empty");
    }
    Ok(s)
}

fn boolean(json: &Json, path: &str) -> Read<bool> {
    match json {
        Json::Bool(b) => Ok(*b),
        _ => refused(path, "not a boolean"),
    }
}

fn time(json: &Json, path: &str) -> Read<Timestamp> {
    match json {
        Json::Int(n) => i64::try_from(*n)
            .map(Timestamp)
            .or_else(|_| refused(path, "out of range")),
        _ => refused(path, "not an integer"),
    }
}

fn count(json: &Json, path: &str) -> Read<u64> {
    match json {
        Json::Int(n) => u64::try_from(*n).or_else(|_| refused(path, "not a count")),
        _ => refused(path, "not an integer"),
    }
}

fn nullable<T>(json: &Json, path: &str, read: impl Fn(&Json, &str) -> Read<T>) -> Read<Option<T>> {
    match json {
        Json::Null => Ok(None),
        other => read(other, path).map(Some),
    }
}

fn list<T>(json: &Json, path: &str, read: impl Fn(&Json, &str) -> Read<T>) -> Read<Vec<T>> {
    let Json::Array(items) = json else {
        return refused(path, "not a list");
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| read(item, &format!("{path}[{i}]")))
        .collect()
}

/// A label from its fixed list: a string, and only one of these.
fn label<T: Copy>(json: &Json, path: &str, labels: &[(&str, T)]) -> Read<T> {
    let Json::String(s) = json else {
        return refused(path, "not a string label");
    };
    labels
        .iter()
        .find(|(l, _)| l == s)
        .map(|(_, value)| *value)
        .map_or_else(|| refused(path, "not one of its labels"), Ok)
}

/// A string, checked by the core's own rule.
fn checked<T>(
    json: &Json,
    path: &str,
    rule: impl Fn(&str) -> Result<T, InvalidRequest>,
) -> Read<T> {
    let s = string(json, path)?;
    rule(&s).or_else(|_| refused(path, "does not follow its rule"))
}

fn sha256(json: &Json, path: &str) -> Read<String> {
    let s = string(json, path)?;
    if s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Ok(s);
    }
    refused(path, "not a lowercase hex SHA-256")
}

fn setting(json: &Json, path: &str) -> Read<String> {
    checked(json, path, |s| {
        check_setting("setting", s).map(|()| s.to_owned())
    })
}

fn fact_string(json: &Json, path: &str) -> Read<Fact<String>> {
    match json {
        Json::Object(entries) if entries.len() == 1 && entries[0].0 == "known" => {
            string(&entries[0].1, &format!("{path}.known")).map(Fact::Known)
        }
        Json::Object(entries) if entries.len() == 1 && entries[0].0 == "unknown" => {
            text(&entries[0].1, &format!("{path}.unknown")).map(Fact::Unknown)
        }
        _ => refused(path, "not {\"known\": …} or {\"unknown\": …}"),
    }
}

const STATES: &[(&str, RecordState)] = &[
    ("claimed", RecordState::Claimed),
    ("dispatching", RecordState::Dispatching),
    ("finished", RecordState::Finished),
];

const EFFECT_KINDS: &[(&str, EffectKind)] = &[
    ("creates_and_starts", EffectKind::CreatesAndStarts),
    ("starts_from_github", EffectKind::StartsFromGithub),
    ("copies_env_files", EffectKind::CopiesEnvFiles),
    ("full_permissions", EffectKind::FullPermissions),
    ("uses_cloud_agent", EffectKind::UsesCloudAgent),
    ("mac_stays_awake", EffectKind::MacStaysAwake),
    ("cloud_room_names_it", EffectKind::CloudRoomNamesIt),
];

const REFUSAL_REASONS: &[(&str, RefusalReason)] = &[
    ("no_options", RefusalReason::NoOptions),
    ("harness_not_offered", RefusalReason::HarnessNotOffered),
    ("model_not_offered", RefusalReason::ModelNotOffered),
    ("effort_not_offered", RefusalReason::EffortNotOffered),
    ("project_not_found", RefusalReason::ProjectNotFound),
    ("project_ambiguous", RefusalReason::ProjectAmbiguous),
    ("project_personal", RefusalReason::ProjectPersonal),
    ("project_not_standard", RefusalReason::ProjectNotStandard),
    (
        "project_no_github_remote",
        RefusalReason::ProjectNoGithubRemote,
    ),
    (
        "project_remote_not_confirmed",
        RefusalReason::ProjectRemoteNotConfirmed,
    ),
    ("provider_unsupported", RefusalReason::ProviderUnsupported),
    (
        "provider_model_unavailable",
        RefusalReason::ProviderModelUnavailable,
    ),
    (
        "provider_effort_unavailable",
        RefusalReason::ProviderEffortUnavailable,
    ),
    (
        "provider_message_too_long",
        RefusalReason::ProviderMessageTooLong,
    ),
    (
        "provider_message_empty",
        RefusalReason::ProviderMessageEmpty,
    ),
    (
        "provider_project_not_found",
        RefusalReason::ProviderProjectNotFound,
    ),
    (
        "provider_request_id_conflict",
        RefusalReason::ProviderRequestIdConflict,
    ),
    (
        "provider_discovery_unavailable",
        RefusalReason::ProviderDiscoveryUnavailable,
    ),
];

const FAIL_REASONS: &[(&str, FailReason)] = &[
    (
        "interrupted_before_dispatch",
        FailReason::InterruptedBeforeDispatch,
    ),
    ("projects_not_read", FailReason::ProjectsNotRead),
    ("record_not_written", FailReason::RecordNotWritten),
    ("budget_exhausted", FailReason::BudgetExhausted),
    ("call_not_started", FailReason::CallNotStarted),
];

const UNCERTAIN_REASONS: &[(&str, UncertainReason)] = &[
    ("timed_out", UncertainReason::TimedOut),
    (
        "ended_without_exit_code",
        UncertainReason::EndedWithoutExitCode,
    ),
    ("not_json", UncertainReason::NotJson),
    ("unexpected_receipt", UncertainReason::UnexpectedReceipt),
    ("undocumented_error", UncertainReason::UndocumentedError),
    ("call_not_confirmed", UncertainReason::CallNotConfirmed),
    (
        "interrupted_after_dispatch_mark",
        UncertainReason::InterruptedAfterDispatchMark,
    ),
];

fn project_ref(json: &Json, path: &str) -> Read<ProjectRef> {
    let o = Object::read(json, path, &["kind", "value"])?;
    let value = o.field("value", |v, p| {
        let s = string(v, p)?;
        if s.trim().is_empty() || s.contains('\0') {
            return refused(p, "only whitespace, or a NUL");
        }
        Ok(s)
    })?;
    match o.field("kind", |v, p| label(v, p, &[("id", true), ("name", false)]))? {
        true => Ok(ProjectRef::Id(value)),
        false => Ok(ProjectRef::Name(value)),
    }
}

fn settings(json: &Json, path: &str) -> Read<Settings> {
    let o = Object::read(json, path, &["harness", "model", "effort"])?;
    Ok(Settings {
        harness: o.field("harness", setting)?,
        model: o.field("model", setting)?,
        effort: o.field("effort", setting)?,
    })
}

fn binding(json: &Json, path: &str) -> Read<Binding> {
    let keys = [
        "project",
        "harness",
        "model",
        "effort",
        "text_sha256",
        "attachments_digest",
    ];
    let o = Object::read(json, path, &keys)?;
    Ok(Binding {
        project: o.field("project", project_ref)?,
        harness: o.field("harness", setting)?,
        model: o.field("model", setting)?,
        effort: o.field("effort", setting)?,
        text_sha256: o.field("text_sha256", sha256)?,
        attachments_digest: o.field("attachments_digest", sha256)?,
    })
}

fn project_view(json: &Json, path: &str) -> Read<ProjectView> {
    let o = Object::read(json, path, &["id", "name"])?;
    Ok(ProjectView {
        id: o.field("id", text)?,
        name: o.field("name", fact_string)?,
    })
}

fn source(json: &Json, path: &str) -> Read<SourceInfo> {
    let keys = ["name", "version", "tested_version", "version_tested"];
    let o = Object::read(json, path, &keys)?;
    Ok(SourceInfo {
        name: o.field("name", string)?,
        version: o.field("version", fact_string)?,
        tested_version: o.field("tested_version", string)?,
        version_tested: o.field("version_tested", boolean)?,
    })
}

fn effect(json: &Json, path: &str) -> Read<Effect> {
    let o = Object::read(json, path, &["kind", "text"])?;
    Ok(Effect {
        kind: o.field("kind", |v, p| label(v, p, EFFECT_KINDS))?,
        text: o.field("text", text)?,
    })
}

fn effects(json: &Json, path: &str) -> Read<Effects> {
    let o = Object::read(json, path, &["read_at", "project", "effects"])?;
    Ok(Effects {
        read_at: o.field("read_at", time)?,
        project: o.field("project", project_view)?,
        effects: o.field("effects", |v, p| list(v, p, effect))?,
    })
}

fn outcome(json: &Json, path: &str) -> Read<Outcome> {
    let Json::Object(entries) = json else {
        return refused(path, "not an object");
    };
    let kind = match entries.iter().find(|(k, _)| k == "kind") {
        Some((_, kind)) => label(
            kind,
            &format!("{path}.kind"),
            &[
                ("created", 0),
                ("refused", 1),
                ("failed", 2),
                ("uncertain", 3),
            ],
        )?,
        None => return refused(path, "the key \"kind\" is missing"),
    };
    Ok(match kind {
        0 => {
            let o = Object::read(json, path, &["kind", "thread_id", "text"])?;
            Outcome::Created {
                thread_id: o.field("thread_id", text)?,
                text: o.field("text", text)?,
            }
        }
        1 => {
            let o = Object::read(json, path, &["kind", "reason", "offered", "text"])?;
            Outcome::Refused {
                reason: o.field("reason", |v, p| label(v, p, REFUSAL_REASONS))?,
                offered: o.field("offered", |v, p| list(v, p, string))?,
                text: o.field("text", text)?,
            }
        }
        2 => {
            let o = Object::read(json, path, &["kind", "reason", "text"])?;
            Outcome::Failed {
                reason: o.field("reason", |v, p| label(v, p, FAIL_REASONS))?,
                text: o.field("text", text)?,
            }
        }
        _ => {
            let o = Object::read(json, path, &["kind", "reason", "text"])?;
            Outcome::Uncertain {
                reason: o.field("reason", |v, p| label(v, p, UNCERTAIN_REASONS))?,
                text: o.field("text", text)?,
            }
        }
    })
}

/// A string, checked by an attachment type's own rule.
fn attachment_checked<T>(
    json: &Json,
    path: &str,
    rule: impl Fn(&str) -> Result<T, crate::attachment::InvalidAttachment>,
) -> Read<T> {
    let s = string(json, path)?;
    rule(&s).or_else(|_| refused(path, "does not follow its rule"))
}

/// One attachment of the record (B2): exactly its five keys, each value by
/// its own rule.
fn attachment(json: &Json, path: &str) -> Read<PromptAttachment> {
    let o = Object::read(json, path, crate::attachment::PROMPT_ATTACHMENT_KEYS)?;
    Ok(PromptAttachment {
        id: o.field("id", |v, p| attachment_checked(v, p, AttachmentId::parse))?,
        sha256: o.field("sha256", |v, p| attachment_checked(v, p, Sha256::parse))?,
        size_bytes: o.field("size_bytes", count)?,
        kind: o.field("kind", |v, p| label(v, p, Kind::labels()))?,
        name: o.field("name", |v, p| attachment_checked(v, p, SafeName::parse))?,
    })
}

/// The record's attachments, when the key is there (B2): a list of at
/// least one, no id twice. Absent means none.
fn attachments(json: &Json, path: &str) -> Read<Vec<PromptAttachment>> {
    let list = list(json, path, attachment)?;
    if list.is_empty() {
        return refused(path, "empty (absent means none)");
    }
    for (i, a) in list.iter().enumerate() {
        if list[..i].iter().any(|b| b.id == a.id) {
            return refused(&format!("{path}[{i}]"), "an id given twice");
        }
    }
    Ok(list)
}

fn gap(json: &Json, path: &str) -> Read<Gap> {
    let o = Object::read(json, path, &["subject", "detail"])?;
    Ok(Gap {
        subject: o.field("subject", text)?,
        detail: o.field("detail", text)?,
    })
}

/// The keys of a claim file (B2, the implementation).
pub const OP_REF_KEYS: &[&str] = &["kind", "op_id"];

/// The operation a claim file names (B2), read strictly: exactly `kind`
/// (`messaging` or `new_thread`) and `op_id` (by its own rule), each once,
/// and nothing else.
pub fn op_ref(json: &StrictJson) -> Result<OpRef, ReadError> {
    let o = Object::read(&json.0, "claim", OP_REF_KEYS)?;
    let kinds = [
        ("messaging", OpKind::Send),
        ("new_thread", OpKind::NewThread),
    ];
    Ok(OpRef {
        kind: o.field("kind", |v, p| label(v, p, &kinds))?,
        op_id: o.field("op_id", |v, p| checked(v, p, OpId::parse))?,
    })
}

/// Every key of a stored record, in the order the core writes them.
const RECORD_KEYS: &[&str] = &[
    "schema",
    "op_id",
    "binding",
    "request_id",
    "requested",
    "provider",
    "text",
    "text_bytes",
    "state",
    "project",
    "source",
    "effects",
    "outcome",
    "gaps",
    "recovered",
    "claimed_at",
    "dispatching_at",
    "finished_at",
];

impl NewThreadRecord {
    /// The record in `json`, read strictly (see the module documentation).
    ///
    /// Beyond the contract's shape, a record must also agree with itself:
    /// its request id is its op id's, a kept text has the binding's hash
    /// and the recorded size, and its attachments (absent means none, B2)
    /// have the binding's digest. A record that doesn't is refused.
    pub fn read(json: &StrictJson) -> Result<Self, ReadError> {
        let path = "record";
        let with_attachments = matches!(&json.0, Json::Object(entries)
            if entries.iter().any(|(k, _)| k == "attachments"));
        let mut keys = RECORD_KEYS.to_vec();
        if with_attachments {
            keys.push("attachments");
        }
        let o = Object::read(&json.0, path, &keys)?;
        if o.field("schema", string)? != RECORD_SCHEMA_ID {
            return refused("record.schema", "not this record's schema");
        }
        let record = Self {
            schema: RECORD_SCHEMA_ID.to_owned(),
            op_id: o.field("op_id", |v, p| checked(v, p, OpId::parse))?,
            binding: o.field("binding", binding)?,
            request_id: o.field("request_id", string)?,
            requested: o.field("requested", settings)?,
            provider: o.field("provider", |v, p| nullable(v, p, string))?,
            text: o.field("text", |v, p| nullable(v, p, string))?,
            text_bytes: o.field("text_bytes", count)?,
            attachments: if with_attachments {
                o.field("attachments", attachments)?
            } else {
                vec![]
            },
            state: o.field("state", |v, p| label(v, p, STATES))?,
            project: o.field("project", |v, p| nullable(v, p, project_view))?,
            source: o.field("source", |v, p| nullable(v, p, source))?,
            effects: o.field("effects", |v, p| nullable(v, p, effects))?,
            outcome: o.field("outcome", |v, p| nullable(v, p, outcome))?,
            gaps: o.field("gaps", |v, p| list(v, p, gap))?,
            recovered: o.field("recovered", boolean)?,
            claimed_at: o.field("claimed_at", time)?,
            dispatching_at: o.field("dispatching_at", |v, p| nullable(v, p, time))?,
            finished_at: o.field("finished_at", |v, p| nullable(v, p, time))?,
        };
        if record.request_id != request_id(&record.op_id) {
            return refused("record.request_id", "not the op id's request id");
        }
        if crate::prompt::attachments_digest(&record.attachments)
            != record.binding.attachments_digest
        {
            return refused("record.attachments", "not the bound attachments");
        }
        if let Some(text) = &record.text
            && (text.len() as u64 != record.text_bytes
                || sha256_hex(text.as_bytes()) != record.binding.text_sha256)
        {
            return refused("record.text", "not the bound text");
        }
        Ok(record)
    }
}
