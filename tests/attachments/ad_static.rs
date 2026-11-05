//! B2 (the implementation): the rules and the static checks. AD8 (no stronger
//! claim), AD11 (one implementation), the attachments digest, the one
//! claim rule, the gate's all-or-nothing on an in-memory store, the
//! dispatch order, the allowance, and the messaging schema's attachments.
//! The end-to-end proofs are in `tests/messaging/ad_send_attachments.rs`
//! and `tests/new-thread/ad_new_thread_attachments.rs`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use openwiz_core::attachment::{
    Claim, FailReason, Kind, PromptAttachment, SafeName, Sha256, StagedAttachment, StagedPath,
};
use openwiz_core::messaging::record::StoreError;
use openwiz_core::messaging::send::{Delivery, WaitingOn};
use openwiz_core::messaging::{Mode, OpId, sha256_hex};
use openwiz_core::model::Timestamp;
use openwiz_core::new_thread::outcome::Outcome;
use openwiz_core::prompt::{
    AttachmentId, ClaimOutcome, GateError, Lookup, NOT_REPORTED, OpKind, OpRef, Problem,
    PromptPayload, ReadyPrompt, StagedAttachments, attachment_allowance, attachments_digest,
    claimable, empty_attachments_digest, gate, handed_over,
};
use serde_json::{Value, json};

use crate::support::ROOT;

const T0: Timestamp = Timestamp(1_700_000_600_000);
const TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(ROOT).join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn sources(dir: &str) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    rust_files(&Path::new(ROOT).join(dir), &mut files);
    assert!(!files.is_empty(), "{dir}");
    files
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).unwrap();
            (p, text)
        })
        .collect()
}

fn op(kind: OpKind, id: &str) -> OpRef {
    OpRef {
        kind,
        op_id: OpId::parse(id).unwrap(),
    }
}

fn attachment(id: &str, name: &str, kind: Kind, bytes: &[u8]) -> PromptAttachment {
    PromptAttachment {
        id: AttachmentId::parse(id).unwrap(),
        sha256: Sha256::parse(&sha256_hex(bytes)).unwrap(),
        size_bytes: bytes.len() as u64,
        kind,
        name: SafeName::parse(name).unwrap(),
    }
}

fn staged(a: PromptAttachment) -> StagedAttachment {
    StagedAttachment {
        path: StagedPath::new(Path::new("/synthetic/state/attachments"), &a.id, &a.name).unwrap(),
        attachment: a,
    }
}

// The attachments digest.

#[test]
fn the_digest_is_the_canonical_list_in_the_owners_order_and_the_empty_one_is_fixed() {
    assert_eq!(
        attachments_digest(&[]),
        sha256_hex(b"openwiz.prompt.attachments.v1\n")
    );
    assert_eq!(empty_attachments_digest(), attachments_digest(&[]));
    let a = attachment("att-syn-1", "syn-notes.txt", Kind::File, b"notes");
    let b = attachment("att-syn-2", "syn-photo.png", Kind::Image, b"photo");
    // Written out by hand, independently of the code.
    let canonical = format!(
        "openwiz.prompt.attachments.v2\n{} 5 file syn-notes.txt\n{} 5 image syn-photo.png\n",
        sha256_hex(b"notes"),
        sha256_hex(b"photo")
    );
    assert_eq!(
        attachments_digest(&[a.clone(), b.clone()]),
        sha256_hex(canonical.as_bytes())
    );
    // The order counts; the id doesn't.
    assert_ne!(
        attachments_digest(&[a.clone(), b.clone()]),
        attachments_digest(&[b.clone(), a.clone()])
    );
    let same_bytes = attachment("att-syn-9", "syn-notes.txt", Kind::File, b"notes");
    assert_eq!(attachments_digest(&[a]), attachments_digest(&[same_bytes]));
}

#[test]
fn files_go_first_then_images_each_in_the_owners_order() {
    let prompt = ReadyPrompt {
        text: "Synthetic".to_owned(),
        attachments: vec![
            staged(attachment("att-i1", "i1.png", Kind::Image, b"1")),
            staged(attachment("att-f1", "f1.txt", Kind::File, b"2")),
            staged(attachment("att-i2", "i2.png", Kind::Image, b"3")),
            staged(attachment("att-f2", "f2.txt", Kind::File, b"4")),
        ],
    };
    let order: Vec<String> = prompt
        .dispatch_order()
        .iter()
        .map(|a| a.attachment.id.as_str().to_owned())
        .collect();
    assert_eq!(order, ["att-f1", "att-f2", "att-i1", "att-i2"]);
    // The binding keeps the owner's order.
    let bound: Vec<String> = prompt
        .bound()
        .iter()
        .map(|a| a.id.as_str().to_owned())
        .collect();
    assert_eq!(bound, ["att-i1", "att-f1", "att-i2", "att-f2"]);
}

#[test]
fn the_allowance_is_one_second_per_started_mib() {
    for (bytes, secs) in [
        (0, 0),
        (1, 1),
        (1024 * 1024, 1),
        (1024 * 1024 + 1, 2),
        (25 * 1024 * 1024, 25),
    ] {
        assert_eq!(
            attachment_allowance(bytes),
            Duration::from_secs(secs),
            "{bytes}"
        );
    }
}

#[test]
fn a_prompt_lists_an_attachment_once() {
    let mut p = PromptPayload::text("Synthetic");
    p.attachments = vec![
        AttachmentId::parse("att-a").unwrap(),
        AttachmentId::parse("att-b").unwrap(),
    ];
    assert!(p.validate().is_ok());
    p.attachments.push(AttachmentId::parse("att-a").unwrap());
    assert!(
        p.validate()
            .unwrap_err()
            .0
            .contains("att-a is listed twice")
    );
}

// The one claim rule.

#[test]
fn the_claim_rule_follows_retention() {
    let me = op(OpKind::Send, "op-me");
    let other = op(OpKind::NewThread, "op-other");
    let fresh = Duration::from_secs(60);
    let old = TTL;
    assert_eq!(claimable(&me, None, fresh), Ok(()));
    assert_eq!(claimable(&me, None, old), Err(Problem::Expired));
    for claim in [
        Claim::Dispatching,
        Claim::Accepted,
        Claim::Uncertain,
        Claim::Failed,
    ] {
        assert_eq!(
            claimable(&me, Some(&(me.clone(), claim)), old),
            Ok(()),
            "{claim:?}"
        );
    }
    assert_eq!(
        claimable(&me, Some(&(other.clone(), Claim::Failed)), fresh),
        Ok(())
    );
    assert_eq!(
        claimable(&me, Some(&(other.clone(), Claim::Failed)), old),
        Err(Problem::Expired)
    );
    for claim in [Claim::Dispatching, Claim::Accepted, Claim::Uncertain] {
        assert_eq!(
            claimable(&me, Some(&(other.clone(), claim)), fresh),
            Err(Problem::InUse {
                op: other.clone(),
                claim
            }),
            "{claim:?}"
        );
    }
    // The same op id under the other kind is another operation.
    let twin = op(OpKind::NewThread, "op-me");
    assert!(claimable(&me, Some(&(twin, Claim::Dispatching)), fresh).is_err());
}

// The gate, on an in-memory store.

/// An in-memory attachment store: each id's lookup, and every claim made.
#[derive(Default)]
struct Memory {
    lookups: BTreeMap<String, Lookup>,
    claims: Mutex<Vec<(OpRef, Vec<AttachmentId>)>>,
    reads: Mutex<Vec<String>>,
}

impl StagedAttachments for Memory {
    fn look_up(&self, id: &AttachmentId, _now: Timestamp) -> Result<Lookup, StoreError> {
        self.reads.lock().unwrap().push(id.as_str().to_owned());
        Ok(self
            .lookups
            .get(id.as_str())
            .cloned()
            .unwrap_or(Lookup::NotFound))
    }
    fn claim(
        &self,
        op: &OpRef,
        ids: &[AttachmentId],
        _now: Timestamp,
    ) -> Result<ClaimOutcome, StoreError> {
        self.claims.lock().unwrap().push((op.clone(), ids.to_vec()));
        Ok(ClaimOutcome::Claimed { new: ids.to_vec() })
    }
    fn unclaim(&self, _op: &OpRef, _ids: &[AttachmentId]) -> Result<(), StoreError> {
        Ok(())
    }
    fn settle(
        &self,
        _op: &OpRef,
        _ids: &[AttachmentId],
        _now: Timestamp,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

fn ready_lookup(a: PromptAttachment, at: Timestamp, claim: Option<(OpRef, Claim)>) -> Lookup {
    Lookup::Ready {
        staged: staged(a),
        ready_at: at,
        claim,
    }
}

#[test]
fn the_gate_passes_every_attachment_or_names_every_one_that_cant_go() {
    let me = op(OpKind::Send, "op-gate");
    let other = op(OpKind::Send, "op-other");
    let mut memory = Memory::default();
    let good = attachment("att-good", "good.txt", Kind::File, b"good");
    memory
        .lookups
        .insert("att-good".into(), ready_lookup(good.clone(), T0, None));
    memory.lookups.insert(
        "att-expired".into(),
        ready_lookup(
            attachment("att-expired", "e.txt", Kind::File, b"e"),
            Timestamp(T0.0 - 25 * 3_600_000),
            None,
        ),
    );
    memory.lookups.insert(
        "att-busy".into(),
        ready_lookup(
            attachment("att-busy", "b.txt", Kind::File, b"b"),
            T0,
            Some((other.clone(), Claim::Dispatching)),
        ),
    );
    memory.lookups.insert(
        "att-broken".into(),
        Lookup::Failed(FailReason::SizeMismatch),
    );
    let mut payload = PromptPayload::text("Synthetic");
    payload.attachments = [
        "att-good",
        "att-missing",
        "att-expired",
        "att-busy",
        "att-broken",
    ]
    .iter()
    .map(|id| AttachmentId::parse(id).unwrap())
    .collect();
    let Err(GateError::Refused(refusal)) = gate(&memory, &payload, &me, T0) else {
        panic!("not refused")
    };
    let named: Vec<(&str, &Problem)> = refusal
        .problems
        .iter()
        .map(|p| (p.id.as_str(), &p.problem))
        .collect();
    assert_eq!(
        named,
        [
            ("att-missing", &Problem::NotFound),
            ("att-expired", &Problem::Expired),
            (
                "att-busy",
                &Problem::InUse {
                    op: other,
                    claim: Claim::Dispatching
                }
            ),
            ("att-broken", &Problem::Failed(FailReason::SizeMismatch)),
        ]
    );
    // Every attachment was looked at, and nothing was claimed.
    assert_eq!(memory.reads.lock().unwrap().len(), 5);
    assert!(memory.claims.lock().unwrap().is_empty());
    // Only the ready one: it passes, with its staged copy.
    payload.attachments = vec![AttachmentId::parse("att-good").unwrap()];
    let ready = gate(&memory, &payload, &me, T0).unwrap();
    assert_eq!(ready.bound(), vec![good]);
    assert!(
        memory.claims.lock().unwrap().is_empty(),
        "the gate claims nothing"
    );
}

// AD8: no stronger claim.

#[test]
fn ad08_the_wording_table_and_no_stronger_claim_anywhere() {
    assert_eq!(
        handed_over(1),
        ", with 1 attachment handed to CloudRoom. Delivery to the agent's machine is not reported"
    );
    assert_eq!(
        handed_over(3),
        ", with 3 attachments handed to CloudRoom. Delivery to the agent's machine is not reported"
    );
    assert_eq!(
        NOT_REPORTED,
        "Delivery to the agent's machine is not reported"
    );
    let queued = Delivery::queued(WaitingOn::ThreadBusy, "qm_syn_1".to_owned()).with_attachments(2);
    assert_eq!(
        queued.text(),
        "queued: thread busy, with 2 attachments handed to CloudRoom. Delivery to the agent's \
         machine is not reported"
    );
    let sent = Delivery::accepted(Mode::Queue).with_attachments(1);
    assert!(sent.text().ends_with(&handed_over(1)), "{}", sent.text());
    let created = Outcome::created("thr_syn_1".to_owned()).with_attachments(2);
    assert!(
        created.text().ends_with(&handed_over(2)),
        "{}",
        created.text()
    );
    // A failed or uncertain outcome never says it handed anything over.
    for unchanged in [
        Delivery::failed(openwiz_core::messaging::send::FailReason::InvalidInput)
            .with_attachments(2),
        Delivery::uncertain(openwiz_core::messaging::send::UncertainReason::TimedOut)
            .with_attachments(2),
    ] {
        assert!(
            !unchanged.text().contains("CloudRoom"),
            "{}",
            unchanged.text()
        );
    }
    // No source says more than the table.
    for dir in [
        "crates/core/src",
        "crates/cli/src",
        "crates/adapters/cloudroom/src",
    ] {
        for (path, text) in sources(dir) {
            let lower = text.to_lowercase();
            for phrase in [
                "delivered to the worker",
                "delivered to the agent",
                "delivered to the cloud",
                "read by the agent",
                "visible to the agent",
                "the agent received",
                "the agent has the",
                "reached the agent",
                "uploaded to the worker",
                "uploaded to the sandbox",
            ] {
                assert!(!lower.contains(phrase), "{path:?} says {phrase:?}");
            }
        }
    }
}

// AD11: one implementation.

#[test]
fn ad11_one_attachment_args_and_no_other_place_builds_file_or_image() {
    let mut builders = 0;
    for (path, text) in sources("crates") {
        builders += text.matches("fn attachment_args(").count();
        let is_allowlist = path.ends_with("adapters/cloudroom/src/allowlist.rs");
        for flag in ["\"--file\"", "\"--image\""] {
            let n = text.matches(flag).count();
            if is_allowlist {
                assert_eq!(n, 1, "{flag} once in the allowlist");
                let builder = text
                    .split("pub fn attachment_args(")
                    .nth(1)
                    .and_then(|rest| rest.split("\n}\n").next())
                    .unwrap();
                assert!(builder.contains(flag), "{flag} outside attachment_args");
            } else {
                assert_eq!(n, 0, "{path:?} spells {flag}");
            }
        }
    }
    assert_eq!(builders, 1, "one attachment_args");
    // Both writes take their tail from it, once, in argv.
    let allowlist = read("crates/adapters/cloudroom/src/allowlist.rs");
    let argv = allowlist.split("pub fn argv(&self)").nth(1).unwrap();
    assert_eq!(argv.matches("attachment_args(").count(), 1);
    for variant in ["Self::ThreadTell {", "Self::ThreadSpawn("] {
        let arm = argv.split(variant).nth(1).unwrap();
        let arm = &arm[..arm.find("\n            }").unwrap()];
        assert!(arm.contains("tail"), "{variant} without the tail");
    }
}

#[test]
fn ad11_send_and_new_thread_share_the_one_gate() {
    let mut gates = 0;
    for (path, text) in sources("crates") {
        gates += text.matches("pub fn gate(").count();
        let builds_ready = text.contains("ReadyPrompt {");
        if builds_ready {
            assert!(
                path.ends_with("core/src/prompt.rs"),
                "{path:?} builds a ReadyPrompt"
            );
        }
    }
    assert_eq!(gates, 1, "one gate");
    for file in [
        "crates/core/src/messaging/send.rs",
        "crates/core/src/new_thread/operation.rs",
    ] {
        let text: String = read(file).split_whitespace().collect();
        // The gate, in one place (`pass_gate`), before the claim: at the op
        // id's first use, and again when the record seen first is gone by
        // the claim; and, for a new thread, in its preview. Nothing else
        // calls it.
        assert_eq!(
            text.matches("gate(attachments,&request.prompt,").count(),
            1,
            "{file}"
        );
        let uses = if file.ends_with("send.rs") { 2 } else { 3 };
        assert_eq!(
            text.matches("pass_gate(attachments,request,&op,clock)")
                .count(),
            uses,
            "{file}"
        );
        assert!(
            text.contains("attachments.claim(&op"),
            "{file} claims for its op"
        );
        assert!(
            text.contains("self.attachments.settle("),
            "{file} settles after the outcome"
        );
    }
    // In prompt.rs, a ReadyPrompt is built by name only in the gate (its
    // text-only constructor builds `Self` with no attachment).
    let prompt = read("crates/core/src/prompt.rs");
    let built = prompt.matches("ReadyPrompt {").count()
        - prompt.matches("impl ReadyPrompt {").count()
        - prompt.matches("pub struct ReadyPrompt {").count();
    assert_eq!(built, 1, "one ReadyPrompt built by name");
    let gate_fn = prompt.split("pub fn gate(").nth(1).unwrap();
    let gate_fn = &gate_fn[..gate_fn.find("\n}\n").unwrap()];
    assert!(gate_fn.contains("ReadyPrompt {"), "the gate builds it");
}

// The messaging schema's attachments (AD13's contract half for send).

fn messaging_schema() -> Value {
    serde_json::from_str(&read("contracts/messaging.v1.schema.json")).unwrap()
}

fn validate_messaging(def: &str, value: &Value) -> Vec<String> {
    let mut schema = messaging_schema();
    schema["$ref"] = json!(format!("#/$defs/{def}"));
    crate::schema::validate(&schema, value)
}

#[test]
fn the_messaging_schema_takes_attachments_only_in_their_shape() {
    let a = json!({
        "id": "att-syn-1",
        "sha256": "a".repeat(64),
        "size_bytes": 5,
        "kind": "file",
        "name": "syn-notes.txt",
    });
    assert_eq!(
        validate_messaging("prompt_attachment", &a),
        Vec::<String>::new()
    );
    for (key, bad) in [
        ("kind", json!("photo")),
        ("size_bytes", json!(-1)),
        ("sha256", json!("ABC")),
        ("name", json!("")),
    ] {
        let mut v = a.clone();
        v[key] = bad;
        assert!(
            !validate_messaging("prompt_attachment", &v).is_empty(),
            "{key}"
        );
    }
    let mut extra = a.clone();
    extra["display_name"] = json!("Synthetic.txt");
    assert!(!validate_messaging("prompt_attachment", &extra).is_empty());
    // The checklist's "the contract and the code refuse the same values":
    // the same good and bad values through the core's strict reader.
    let read = |v: &Value| serde_json::from_value::<PromptAttachment>(v.clone());
    assert!(read(&a).is_ok());
    for (key, bad) in [
        ("kind", json!("photo")),
        ("size_bytes", json!(-1)),
        ("sha256", json!("ABC")),
        ("name", json!("")),
    ] {
        let mut v = a.clone();
        v[key] = bad;
        assert!(read(&v).is_err(), "{key}");
    }
    assert!(read(&extra).is_err(), "an extra key");
    let mut missing = a.clone();
    missing.as_object_mut().unwrap().remove("name");
    assert!(read(&missing).is_err(), "a missing key");
    assert!(!validate_messaging("prompt_attachment", &missing).is_empty());
    // A key given twice, which no JSON value can hold, is refused too.
    let twice = r#"{"id": "att-syn-1", "id": "att-syn-2", "sha256": "SHA", "size_bytes": 5, "kind": "file", "name": "syn-notes.txt"}"#
        .replace("SHA", &"a".repeat(64));
    assert!(serde_json::from_str::<PromptAttachment>(&twice).is_err());
    let binding = json!({
        "target": "thr_syn_0001",
        "mode": "queue",
        "text_sha256": "b".repeat(64),
        "attachments_digest": "c".repeat(64),
    });
    assert_eq!(
        validate_messaging("binding", &binding),
        Vec::<String>::new()
    );
    let mut old = binding.clone();
    old.as_object_mut().unwrap().remove("attachments_digest");
    assert_eq!(
        validate_messaging("binding", &old),
        Vec::<String>::new(),
        "absent means none"
    );
    let schema = messaging_schema();
    let report = &schema["$defs"]["report"]["properties"];
    assert!(report.get("attachments").is_some() && report.get("attachments_digest").is_some());
    let required = schema["$defs"]["report"]["required"].as_array().unwrap();
    assert!(
        !required.contains(&json!("attachments")),
        "absent when there are none"
    );
    let differs = &schema["$defs"]["result"];
    assert!(
        differs.to_string().contains("\"attachments\""),
        "differs names attachments"
    );
}
