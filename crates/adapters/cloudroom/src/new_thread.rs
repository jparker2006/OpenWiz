use std::time::Instant;

use openwiz_core::messaging::InvalidRequest;
use openwiz_core::messaging::send::UncertainReason;
use openwiz_core::model::{Fact, Timestamp};
use openwiz_core::new_thread::effects::ProjectCandidate;
use openwiz_core::new_thread::operation::{NewThreadClient, ObservedThread, SpawnRequest};
use openwiz_core::new_thread::options::{
    AttachmentSupport, Catalogue, CatalogueHarness, CatalogueModel,
};
use openwiz_core::new_thread::outcome::{Gap, ProviderRefusal, SpawnReply};
use openwiz_core::new_thread::start::ShownThread;
use openwiz_core::source::{SessionSource, SourceError};
use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::error::Category;
use serde_json::{Map, Value};
use std::fmt;

use crate::allowlist::{InvalidSpawn, Invocation, SpawnArgs, ThreadId};
use crate::process::{self, SpawnFailure};
use crate::source::CloudRoomSource;

pub const HARNESS_PROVIDERS: &[(&str, &str)] =
    &[("codex", "codex"), ("claude-code", "claude-code")];

pub const CLI_REASONING_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// The only `cloud status` keys OpenWiz keeps (sheet the check). Everything else,
/// the account first, is discarded before anything reads the document.
pub const CATALOGUE_KEYS: &[&str] = &["ready", "harnesses"];

/// The only keys kept on a harness row.
pub const HARNESS_KEYS: &[&str] = &[
    "id",
    "model",
    "reasoning_levels",
    "models",
    "attachments",
    "steer",
];

/// The only keys kept on a model entry.
pub const MODEL_KEYS: &[&str] = &["model", "reasoning_levels"];

/// The only keys kept on a harness's `attachments`.
pub const ATTACHMENT_KEYS: &[&str] = &["files", "images"];

pub fn spawn_argv(spawn: &SpawnRequest) -> Result<Vec<String>, InvalidRequest> {
    spawn_invocation(spawn).map(|invocation| invocation.argv())
}

/// The allowlisted invocation of the one create: every argument checked by
/// [`SpawnArgs::new`] (the text by `send`'s rules), and the staged
/// attachments in the core's dispatch order (B2).
pub fn spawn_invocation(spawn: &SpawnRequest) -> Result<Invocation, InvalidRequest> {
    SpawnArgs::new(
        &spawn.project_id,
        &spawn.provider,
        &spawn.model,
        &spawn.effort,
        &spawn.request_id,
        &spawn.prompt.text,
    )
    .map(|args| Invocation::ThreadSpawn(args.with_attachments(spawn.prompt.dispatch_order())))
    .map_err(|InvalidSpawn(why)| InvalidRequest(why))
}

/// The `spawn` provider id of a catalogue harness, if verified.
pub fn provider_for(harness: &str) -> Option<&'static str> {
    HARNESS_PROVIDERS
        .iter()
        .find(|(h, _)| *h == harness)
        .map(|(_, p)| *p)
}

fn drift(what: &str) -> SourceError {
    SourceError::Malformed {
        detail: format!("cloud status: {what}"),
    }
}

// The strict readers , on B1's pattern: each JSON object
// is read by its own visitor straight from the text, so a key given twice is
// seen (`serde_json::Value` would keep only the last), each kept key must
// have its type, and nothing `room-cli` printed reaches an error message.

/// Refuses a key already seen at this level of an object.
fn first<E: de::Error>(seen: &mut Vec<String>, key: &str) -> Result<(), E> {
    if seen.iter().any(|k| k == key) {
        return Err(E::custom("a key given twice"));
    }
    seen.push(key.to_owned());
    Ok(())
}

fn required<T, E: de::Error>(slot: Option<T>, key: &'static str) -> Result<T, E> {
    slot.ok_or_else(|| E::missing_field(key))
}

/// `cloud status --json`'s top level: `ready` and `harnesses`, typed. Every
/// other key, the account first, is dropped unread (sheet the check).
struct CatalogueDocument(Catalogue);

impl<'de> Deserialize<'de> for CatalogueDocument {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(CatalogueVisitor)
    }
}

struct CatalogueVisitor;

impl<'de> Visitor<'de> for CatalogueVisitor {
    type Value = CatalogueDocument;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the cloud status object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<CatalogueDocument, A::Error> {
        let mut seen = Vec::new();
        let (mut ready, mut harnesses) = (None, None);
        while let Some(key) = map.next_key::<String>()? {
            first(&mut seen, &key)?;
            match key.as_str() {
                "ready" => ready = Some(map.next_value::<bool>()?),
                "harnesses" => {
                    harnesses = Some(map.next_value::<Option<Vec<HarnessRow>>>()?);
                }
                // Not a catalogue key: dropped unread.
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(CatalogueDocument(Catalogue {
            ready: required(ready, "ready")?,
            harnesses: required(harnesses, "harnesses")?
                .map(|rows| rows.into_iter().map(|row| row.0).collect()),
        }))
    }
}

struct HarnessRow(CatalogueHarness);

impl<'de> Deserialize<'de> for HarnessRow {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(HarnessVisitor)
    }
}

struct HarnessVisitor;

impl<'de> Visitor<'de> for HarnessVisitor {
    type Value = HarnessRow;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a harness object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<HarnessRow, A::Error> {
        let mut seen = Vec::new();
        let (mut id, mut model, mut levels) = (None, None, None);
        let (mut models, mut attachments, mut steer) = (None, None, None);
        while let Some(key) = map.next_key::<String>()? {
            first(&mut seen, &key)?;
            match key.as_str() {
                "id" => id = Some(map.next_value::<String>()?),
                "model" => model = Some(map.next_value::<String>()?),
                "reasoning_levels" => levels = Some(map.next_value::<Vec<String>>()?),
                "models" => models = map.next_value::<Option<Vec<ModelEntry>>>()?,
                "attachments" => attachments = map.next_value::<Option<Attachments>>()?,
                "steer" => steer = map.next_value::<Option<bool>>()?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        let id: String = required(id, "id")?;
        // The harness's own default and levels: checked, never used.
        let _: (String, Vec<String>) = (
            required(model, "model")?,
            required(levels, "reasoning_levels")?,
        );
        Ok(HarnessRow(CatalogueHarness {
            provider: provider_for(&id).map(str::to_owned),
            id,
            models: models
                .map(|entries: Vec<ModelEntry>| entries.into_iter().map(|e| e.0).collect()),
            attachments: attachments.map(|a: Attachments| a.0),
            steer,
        }))
    }
}

/// One model entry: [`MODEL_KEYS`], both required and typed; every other
/// key is dropped unread.
struct ModelEntry(CatalogueModel);

impl<'de> Deserialize<'de> for ModelEntry {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(ModelVisitor)
    }
}

struct ModelVisitor;

impl<'de> Visitor<'de> for ModelVisitor {
    type Value = ModelEntry;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a model object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ModelEntry, A::Error> {
        let mut seen = Vec::new();
        let (mut model, mut levels) = (None, None);
        while let Some(key) = map.next_key::<String>()? {
            first(&mut seen, &key)?;
            match key.as_str() {
                "model" => model = Some(map.next_value::<String>()?),
                "reasoning_levels" => levels = Some(map.next_value::<Vec<String>>()?),
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(ModelEntry(CatalogueModel {
            model: required(model, "model")?,
            reasoning_levels: required(levels, "reasoning_levels")?,
        }))
    }
}

/// A harness's `attachments`: [`ATTACHMENT_KEYS`], both required booleans;
/// every other key is dropped unread.
struct Attachments(AttachmentSupport);

impl<'de> Deserialize<'de> for Attachments {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(AttachmentsVisitor)
    }
}

struct AttachmentsVisitor;

impl<'de> Visitor<'de> for AttachmentsVisitor {
    type Value = Attachments;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an attachments object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Attachments, A::Error> {
        let mut seen = Vec::new();
        let (mut files, mut images) = (None, None);
        while let Some(key) = map.next_key::<String>()? {
            first(&mut seen, &key)?;
            match key.as_str() {
                "files" => files = Some(map.next_value::<bool>()?),
                "images" => images = Some(map.next_value::<bool>()?),
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(Attachments(AttachmentSupport {
            files: required(files, "files")?,
            images: required(images, "images")?,
        }))
    }
}

/// The catalogue in `cloud status --json`'s stdout, read strictly: only
/// [`CATALOGUE_KEYS`] (and on each row [`HARNESS_KEYS`], [`MODEL_KEYS`] and
/// [`ATTACHMENT_KEYS`]) are read, each with its type; the account and every
/// other field are dropped unread (sheet the check). A key given twice at any level
/// read, a kept key missing or of another type, or anything but an object is
/// drift, and gives [`SourceError::Malformed`]: no options, and nothing is
/// dispatched (N17).
pub fn parse_catalogue(stdout: &[u8]) -> Result<Catalogue, SourceError> {
    serde_json::from_slice::<CatalogueDocument>(stdout)
        .map(|document| document.0)
        .map_err(|_| {
            drift(
                "not the catalogue's shape: a key given twice, or a catalogue key missing or of \
                 another type (withheld)",
            )
        })
}

/// The start of `room-cli`'s error line for a refused create
/// (source-read, the implementation; not observed).
pub const SPAWN_ERROR_PREFIX: &str = "Error: Failed to create thread: HTTP ";

/// The documented refusals: status, the start of the message, and what it
/// means. Only a line that starts with [`SPAWN_ERROR_PREFIX`], this status,
/// `: ` and this message start is a refusal. **Source-read, not observed.**
pub const DOCUMENTED_REFUSALS: &[(u16, &str, ProviderRefusal)] = &[
    (400, "Cloud supports new ", ProviderRefusal::Unsupported),
    (
        400,
        "This model is unavailable on Cloud",
        ProviderRefusal::ModelUnavailable,
    ),
    (
        400,
        "This reasoning level is unavailable",
        ProviderRefusal::EffortUnavailable,
    ),
    (400, "Message is too long", ProviderRefusal::MessageTooLong),
    (400, "Message is empty", ProviderRefusal::MessageEmpty),
    (404, "Project not found", ProviderRefusal::ProjectNotFound),
    (
        409,
        "Request ID was already used with different content",
        ProviderRefusal::RequestIdConflict,
    ),
    (
        503,
        "Cloud model discovery is unavailable",
        ProviderRefusal::DiscoveryUnavailable,
    ),
];

fn gap(detail: String) -> Gap {
    Gap {
        subject: "thread spawn".to_owned(),
        detail,
    }
}

fn unclear(reason: UncertainReason, detail: &str) -> SpawnReply {
    SpawnReply::Unclear {
        reason,
        gaps: vec![gap(detail.to_owned())],
    }
}

/// The reply to a call that exited non-zero, from its exit code and its
/// first non-empty stderr line. A documented refusal is refused; anything
/// else is uncertain, the safe direction. Nothing of the line is passed on.
pub fn spawn_error(exit_code: Option<i32>, stderr: &str) -> SpawnReply {
    let Some(code) = exit_code else {
        return unclear(
            UncertainReason::EndedWithoutExitCode,
            "thread spawn ended without an exit code",
        );
    };
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let refusal = (code == 1)
        .then(|| line.strip_prefix(SPAWN_ERROR_PREFIX))
        .flatten()
        .and_then(|rest| {
            DOCUMENTED_REFUSALS.iter().find(|(status, message, _)| {
                rest.strip_prefix(&status.to_string())
                    .and_then(|r| r.strip_prefix(": "))
                    .is_some_and(|r| r.starts_with(message))
            })
        });
    match refusal {
        Some((_, _, refusal)) => SpawnReply::Refused(*refusal),
        None => SpawnReply::Unclear {
            reason: UncertainReason::UndocumentedError,
            gaps: vec![gap(format!(
                "thread spawn exited {code} with an error that is not a documented refusal \
                 (its output is withheld)"
            ))],
        },
    }
}

/// The type of one key of the documented thread row.
#[derive(Clone, Copy)]
enum RowType {
    Text,
    TextOrNull,
    Count,
    CountOrNull,
    Flag,
    FlagOrNull,
    /// An object, whose content is not read (its keys still once each), or
    /// null.
    ObjectOrNull,
}

/// Every key the `spawn` receipt may have, and its type: the thread row of
/// `thread list` and `thread show` (capture C), which the receipt is
/// source-read to be (the implementation; not observed). `id` and `projectId` are
/// required; any other key is uncertain.
pub const RECEIPT_KEYS: &[&str] = &[
    "id",
    "projectId",
    "providerId",
    "title",
    "titleFallback",
    "status",
    "visibility",
    "queuedWork",
    "environmentWorkspaceDisplayKind",
    "executionTarget",
    "environmentId",
    "environmentHostId",
    "environmentPath",
    "environmentProviderId",
    "environmentBranchName",
    "environmentName",
    "environmentIsWorktree",
    "parentThreadId",
    "sectionId",
    "sourceThreadId",
    "originKind",
    "originPluginId",
    "pinSortKey",
    "lastError",
    "archivedAt",
    "pinnedAt",
    "deletedAt",
    "lastReadAt",
    "latestAttentionAt",
    "createdAt",
    "updatedAt",
    "activeBackgroundAgentCount",
    "queuedMessageCount",
    "canSpawnChild",
    "hasPendingInteraction",
    "purpose",
    "activity",
    "runtime",
    "projectCopy",
];

/// The type of each of [`RECEIPT_KEYS`] other than `id` and `projectId`.
fn receipt_type(key: &str) -> Option<RowType> {
    Some(match key {
        "providerId" | "status" | "visibility" | "queuedWork" => RowType::Text,
        "title"
        | "titleFallback"
        | "environmentWorkspaceDisplayKind"
        | "executionTarget"
        | "environmentId"
        | "environmentHostId"
        | "environmentPath"
        | "environmentProviderId"
        | "environmentBranchName"
        | "environmentName"
        | "parentThreadId"
        | "sectionId"
        | "sourceThreadId"
        | "originKind"
        | "originPluginId"
        | "pinSortKey"
        | "lastError" => RowType::TextOrNull,
        "createdAt" | "updatedAt" | "activeBackgroundAgentCount" | "queuedMessageCount" => {
            RowType::Count
        }
        "archivedAt" | "pinnedAt" | "deletedAt" | "lastReadAt" | "latestAttentionAt" => {
            RowType::CountOrNull
        }
        "canSpawnChild" | "hasPendingInteraction" => RowType::Flag,
        "environmentIsWorktree" => RowType::FlagOrNull,
        "purpose" | "activity" | "runtime" | "projectCopy" => RowType::ObjectOrNull,
        _ => return None,
    })
}

/// What the receipt says: the new thread and its project.
struct Receipt {
    id: String,
    project_id: String,
}

impl<'de> Deserialize<'de> for Receipt {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(ReceiptVisitor)
    }
}

struct ReceiptVisitor;

impl<'de> Visitor<'de> for ReceiptVisitor {
    type Value = Receipt;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the new thread's row")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Receipt, A::Error> {
        let mut seen = Vec::new();
        let (mut id, mut project_id) = (None, None);
        while let Some(key) = map.next_key::<String>()? {
            first(&mut seen, &key)?;
            match key.as_str() {
                "id" => id = Some(map.next_value::<String>()?),
                "projectId" => project_id = Some(map.next_value::<String>()?),
                other => match receipt_type(other) {
                    Some(RowType::Text) => {
                        map.next_value::<String>()?;
                    }
                    Some(RowType::TextOrNull) => {
                        map.next_value::<Option<String>>()?;
                    }
                    Some(RowType::Count) => {
                        map.next_value::<i64>()?;
                    }
                    Some(RowType::CountOrNull) => {
                        map.next_value::<Option<i64>>()?;
                    }
                    Some(RowType::Flag) => {
                        map.next_value::<bool>()?;
                    }
                    Some(RowType::FlagOrNull) => {
                        map.next_value::<Option<bool>>()?;
                    }
                    Some(RowType::ObjectOrNull) => {
                        map.next_value::<Option<ObjectKeysOnce>>()?;
                    }
                    None => return Err(de::Error::custom("a key the thread row does not have")),
                },
            }
        }
        Ok(Receipt {
            id: required(id, "id")?,
            project_id: required(project_id, "projectId")?,
        })
    }
}

/// Any JSON value with no key given twice in any object, at any depth. Its
/// content is not kept.
struct KeysOnce;

impl<'de> Deserialize<'de> for KeysOnce {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(KeysOnceVisitor)
    }
}

struct KeysOnceVisitor;

impl<'de> Visitor<'de> for KeysOnceVisitor {
    type Value = KeysOnce;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON with no key given twice")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<KeysOnce, E> {
        Ok(KeysOnce)
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<KeysOnce, E> {
        Ok(KeysOnce)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<KeysOnce, E> {
        Ok(KeysOnce)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<KeysOnce, E> {
        Ok(KeysOnce)
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<KeysOnce, E> {
        Ok(KeysOnce)
    }

    fn visit_unit<E: de::Error>(self) -> Result<KeysOnce, E> {
        Ok(KeysOnce)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<KeysOnce, A::Error> {
        while seq.next_element::<KeysOnce>()?.is_some() {}
        Ok(KeysOnce)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<KeysOnce, A::Error> {
        let mut seen = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            first(&mut seen, &key)?;
            map.next_value::<KeysOnce>()?;
        }
        Ok(KeysOnce)
    }
}

/// A JSON object (only) with no key given twice, at any depth. Its content
/// is not kept.
struct ObjectKeysOnce;

impl<'de> Deserialize<'de> for ObjectKeysOnce {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(KeysOnceVisitor)
            .map(|KeysOnce| ObjectKeysOnce)
    }
}

/// Refuses a document that gives any key twice, at any depth: a read of it
/// through `serde_json::Value` would silently keep only the last.
fn keys_once(stdout: &[u8], read: &str) -> Result<(), SourceError> {
    serde_json::from_slice::<KeysOnce>(stdout)
        .map(|_| ())
        .map_err(|_| SourceError::Malformed {
            detail: format!("{read}: not JSON, or a key given twice"),
        })
}

/// The reply to a call that exited 0: its stdout must be the new thread's
/// row, read strictly: exactly the documented row's keys
/// ([`RECEIPT_KEYS`]), each of its type, each once, with `id` a safe id and
/// `projectId` this create's project. Anything else is uncertain (N17): the
/// thread may exist, and the prompt is kept. Nothing of the receipt is
/// passed on but the thread id.
///
/// The receipt's shape is **source-read, not observed** ("`--json` prints
/// the new thread row", written with `status: "pending"`).
pub fn spawn_receipt(stdout: &[u8], project_id: &str) -> SpawnReply {
    let unexpected = |detail: &str| {
        unclear(
            UncertainReason::UnexpectedReceipt,
            &format!("receipt drift: {detail}"),
        )
    };
    let receipt = match serde_json::from_slice::<Receipt>(stdout) {
        Ok(receipt) => receipt,
        Err(e) if e.classify() == Category::Data => {
            return unexpected(
                "the receipt is not the documented thread row: a key given twice, a key the row \
                 does not have or of another type, or no id or projectId (withheld)",
            );
        }
        Err(_) => {
            return unclear(
                UncertainReason::NotJson,
                "thread spawn exited 0 with a reply that is not JSON (withheld)",
            );
        }
    };
    if ThreadId::parse(&receipt.id).is_err() {
        return unexpected(
            "the receipt has no thread id of 1 to 128 ASCII letters, digits, _ or -",
        );
    }
    if receipt.project_id != project_id {
        return unexpected("the receipt names another project");
    }
    SpawnReply::Created {
        thread_id: receipt.id,
    }
}

/// A created thread from its `thread show --json`, for its start stage and
/// reported configuration. A missing or mistyped field is unknown, with a
/// gap naming it, never its value. Fails only when the document is not the
/// thread asked for.
pub fn parse_shown_thread(stdout: &[u8], thread_id: &str) -> Result<ShownThread, SourceError> {
    keys_once(stdout, "thread show")?;
    let malformed = |detail: &str| SourceError::Malformed {
        detail: detail.to_owned(),
    };
    let doc: Value = serde_json::from_slice(stdout).map_err(|_| malformed("not JSON"))?;
    let Some(thread) = doc.get("thread").filter(|t| t.is_object()) else {
        return Err(malformed("no thread object"));
    };
    if thread.get("id").and_then(Value::as_str) != Some(thread_id) {
        return Err(malformed("it describes another thread"));
    }
    let mut problems = Vec::new();
    let mut field = |value: Option<&Value>, name: &str| match value {
        Some(Value::String(s)) => Fact::Known(s.clone()),
        Some(_) => Fact::Unknown(drifted(&mut problems, name, "has an unexpected type")),
        None => Fact::Unknown(drifted(&mut problems, name, "is missing")),
    };
    let status = field(thread.get("status"), "status");
    let provider = field(thread.get("providerId"), "providerId");
    let model = field(doc.pointer("/execution/model"), "execution.model");
    let effort = field(
        doc.pointer("/execution/reasoningLevel"),
        "execution.reasoningLevel",
    );
    let last_error = match thread.get("lastError") {
        None | Some(Value::Null) => Fact::Known(None),
        Some(Value::String(s)) => Fact::Known(Some(s.clone())),
        Some(_) => Fact::Unknown(drifted(
            &mut problems,
            "lastError",
            "has an unexpected type",
        )),
    };
    Ok(ShownThread {
        status,
        last_error,
        provider,
        model,
        effort,
        problems,
    })
}

fn drifted(problems: &mut Vec<Gap>, field: &str, what: &str) -> String {
    let detail = format!("schema drift: thread show {field} {what}");
    problems.push(Gap {
        subject: "thread show".to_owned(),
        detail: detail.clone(),
    });
    detail
}

/// The GitHub remote forms accepted: each prefix, then a path.
const GITHUB_PREFIXES: &[&str] = &[
    "https://github.com/",
    "ssh://git@github.com/",
    "git@github.com:",
];

pub fn is_github_remote(url: &str) -> bool {
    GITHUB_PREFIXES.iter().any(|prefix| {
        url.strip_prefix(prefix).is_some_and(|path| {
            !path.is_empty() && !path.starts_with('/') && path.bytes().all(|b| b.is_ascii_graphic())
        })
    })
}

fn not_a_list(read: &str) -> SourceError {
    SourceError::Malformed {
        detail: format!("{read} is not a JSON list"),
    }
}

fn rows(stdout: &[u8], read: &str) -> Result<Vec<Value>, SourceError> {
    match serde_json::from_slice::<Value>(stdout) {
        Ok(Value::Array(rows)) => Ok(rows),
        _ => Err(not_a_list(read)),
    }
}

/// A string field, or unknown with the reason (the field's name, never its
/// value).
fn string_field(row: &Map<String, Value>, read: &str, field: &str) -> Fact<String> {
    match row.get(field) {
        Some(Value::String(s)) => Fact::Known(s.clone()),
        Some(_) => Fact::Unknown(format!(
            "schema drift: {read} {field} has an unexpected type"
        )),
        None => Fact::Unknown(format!("schema drift: {read} {field} is missing")),
    }
}

/// The projects in `project list --json --include-personal`'s stdout, for a
/// create: id, name, kind, and whether the remote is GitHub. A row with no
/// string id can't be named, and is left out. A missing or mistyped kind
/// or remote is unknown, so the project is refused. A key given twice
/// anywhere refuses the whole list: nothing is created.
pub fn parse_projects(stdout: &[u8]) -> Result<Vec<ProjectCandidate>, SourceError> {
    let read = "project list";
    // A key given twice (a `kind` or a remote) could change which project
    // may be created in: the whole list is refused instead.
    keys_once(stdout, read)?;
    Ok(rows(stdout, read)?
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|row| {
            let Some(Value::String(id)) = row.get("id") else {
                return None;
            };
            let github_remote = match row.get("gitRemoteUrl") {
                Some(Value::Null) => Fact::Known(false),
                Some(Value::String(url)) => Fact::Known(is_github_remote(url)),
                Some(_) => Fact::Unknown(format!(
                    "schema drift: {read} gitRemoteUrl has an unexpected type"
                )),
                None => Fact::Unknown(format!("schema drift: {read} gitRemoteUrl is missing")),
            };
            Some(ProjectCandidate {
                id: id.clone(),
                name: string_field(row, read, "name"),
                kind: string_field(row, read, "kind"),
                github_remote,
            })
        })
        .collect())
}

/// The threads in `project_id` created at or after `since`, from `thread
/// list --json`'s stdout: observations only. A thread in the project whose
/// creation time can't be read is listed, with it unknown, since it can't
/// be ruled out.
pub fn parse_observed_threads(
    stdout: &[u8],
    project_id: &str,
    since: Timestamp,
) -> Result<Vec<ObservedThread>, SourceError> {
    let read = "thread list";
    keys_once(stdout, read)?;
    Ok(rows(stdout, read)?
        .iter()
        .filter_map(Value::as_object)
        .filter(|row| row.get("projectId").and_then(Value::as_str) == Some(project_id))
        .filter_map(|row| {
            let id = row
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| ThreadId::parse(id).is_ok())?;
            let created_at = match row.get("createdAt").and_then(Value::as_i64) {
                Some(at) if at < since.0 => return None,
                Some(at) => Fact::Known(Timestamp(at)),
                None => Fact::Unknown(format!(
                    "schema drift: {read} createdAt is missing or not a time"
                )),
            };
            Some(ObservedThread {
                id: id.to_owned(),
                created_at,
            })
        })
        .collect())
}

/// Drops `room-cli`'s own text from a read's error, keeping its category.
fn withheld(error: SourceError) -> SourceError {
    match error {
        SourceError::CommandFailed { exit_code, .. } => SourceError::CommandFailed {
            exit_code,
            detail: "output withheld".to_owned(),
        },
        SourceError::Unavailable { .. } => SourceError::Unavailable {
            detail: "could not be started".to_owned(),
        },
        other => other,
    }
}

impl NewThreadClient for CloudRoomSource {
    fn name(&self) -> &str {
        crate::PROGRAM
    }

    fn tested_version(&self) -> &str {
        crate::TESTED_VERSION
    }

    fn cli_levels(&self) -> &[&str] {
        CLI_REASONING_LEVELS
    }

    fn version(&self, deadline: Instant) -> Result<String, SourceError> {
        // The version read's malformed details are OpenWiz's own words.
        SessionSource::version(self, deadline).map_err(withheld)
    }

    fn projects(&self, deadline: Instant) -> Result<Vec<ProjectCandidate>, SourceError> {
        let list = Invocation::ProjectList { hidden: false };
        let out = process::run(self.program(), &list, deadline).map_err(withheld)?;
        parse_projects(&out)
    }

    fn catalogue(&self, deadline: Instant) -> Result<Catalogue, SourceError> {
        let out =
            process::run(self.program(), &Invocation::CloudStatus, deadline).map_err(withheld)?;
        parse_catalogue(&out)
    }

    fn spawn(&self, spawn: &SpawnRequest, deadline: Instant) -> SpawnReply {
        let Ok(invocation) = spawn_invocation(spawn) else {
            // Never reached: the core passes only checked values. Nothing
            // is run.
            return SpawnReply::NotStarted;
        };
        match process::run_spawn(self.program(), &invocation, deadline) {
            Ok(out) => spawn_receipt(&out, &spawn.project_id),
            Err(SpawnFailure::Exited {
                exit_code,
                first_line,
            }) => spawn_error(exit_code, &first_line),
            Err(SpawnFailure::Source(SourceError::TimedOut { .. })) => unclear(
                UncertainReason::TimedOut,
                "thread spawn did not answer in time; it was stopped and not retried",
            ),
            Err(SpawnFailure::Source(SourceError::Malformed { .. })) => unclear(
                UncertainReason::UnexpectedReceipt,
                "thread spawn printed more than OpenWiz reads",
            ),
            // The runner's own words for a program that never started.
            Err(SpawnFailure::Source(SourceError::Unavailable { detail }))
                if detail.contains("could not be started") =>
            {
                SpawnReply::NotStarted
            }
            Err(SpawnFailure::Source(_)) => unclear(
                UncertainReason::CallNotConfirmed,
                "thread spawn could not be confirmed to have run cleanly",
            ),
        }
    }

    fn show_thread(&self, thread_id: &str, deadline: Instant) -> Result<ShownThread, SourceError> {
        let id = ThreadId::parse(thread_id).map_err(|_| SourceError::InvalidId {
            id: thread_id.to_owned(),
            detail: "not a safe id".to_owned(),
        })?;
        let out = match process::run(self.program(), &Invocation::ThreadShow(id), deadline) {
            Ok(out) => out,
            Err(SourceError::CommandFailed { ref detail, .. })
                if detail.starts_with(process::NOT_FOUND_LINE) =>
            {
                return Err(SourceError::NotFound {
                    subject: thread_id.to_owned(),
                });
            }
            Err(e) => return Err(withheld(e)),
        };
        parse_shown_thread(&out, thread_id)
    }

    fn threads_since(
        &self,
        project_id: &str,
        since: Timestamp,
        deadline: Instant,
    ) -> Result<Vec<ObservedThread>, SourceError> {
        let list = Invocation::ThreadList {
            include_hidden: false,
            include_archived: false,
        };
        let out = process::run(self.program(), &list, deadline).map_err(withheld)?;
        parse_observed_threads(&out, project_id, since)
    }
}
