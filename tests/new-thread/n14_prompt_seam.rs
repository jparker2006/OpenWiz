//! N14: the prompt seam. A file path in the text is sent as text; the empty
//! list has its fixed digest on every record.
//!
//! **Updated for B2 (the implementation)**, with this recorded reason: B2 relaxes
//! the seam A1 left. So: without an
//! attachment store (`new_thread`, as the server calls it until B3) any
//! attachment is still refused, now by the one gate, naming it, with zero
//! calls and no claim; an attachment reaches the argument builder only as
//! a staged copy (AD1 proves the tail); and the prompt schema allows
//! attachment ids, each once. AD1 to AD13 prove the rest.

use openwiz_adapter_cloudroom::new_thread::spawn_argv;
use openwiz_core::messaging::sha256_hex;
use openwiz_core::new_thread::operation::{NewThreadError, SpawnRequest, preview_new_thread};
use openwiz_core::prompt::{
    AttachmentId, PromptPayload, ReadyPrompt, attachments_digest, empty_attachments_digest,
};
use serde_json::{Value, json};

use crate::support::*;

fn with_attachment() -> openwiz_core::new_thread::NewThreadRequest {
    let mut r = request(offered(), "Synthetic with a file", "op-n14");
    r.prompt.attachments = vec![AttachmentId::parse("att-syn-0001").unwrap()];
    r
}

#[test]
fn without_an_attachment_store_any_attachment_is_refused_with_zero_calls_and_no_claim() {
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let Err(NewThreadError::Invalid(why)) = try_create(&client, &store, &with_attachment()) else {
        panic!("not refused")
    };
    assert!(
        why.0.starts_with("attachment att-syn-0001 is not found")
            && why.0.contains("nothing was sent"),
        "{}",
        why.0
    );
    assert_eq!(client.calls(), 0);
    assert_eq!(store.writes(), 0, "nothing claimed");
    assert!(preview_new_thread(&client, &clock(), &with_attachment()).is_err());
    assert_eq!(client.calls(), 0);
}

#[test]
fn a_text_only_prompt_passes_no_attachment_flag() {
    let spawn = SpawnRequest {
        project_id: "prj_syn_0001".to_owned(),
        provider: "codex".to_owned(),
        model: "syn-codex-model-01".to_owned(),
        effort: "high".to_owned(),
        request_id: "ow1-synthetic".to_owned(),
        prompt: ReadyPrompt::text("Synthetic"),
    };
    // The request id must be a derived one (A1 phase 2): any other is
    // refused, and nothing is built.
    assert!(spawn_argv(&spawn).is_err());
    let spawn = SpawnRequest {
        request_id: openwiz_core::new_thread::request_id(&op("op-n14-seam")),
        ..spawn
    };
    let argv = spawn_argv(&spawn).expect("valid");
    assert!(!argv.iter().any(|w| w == "--file" || w == "--image"));
    assert_eq!(argv.last().map(String::as_str), Some("Synthetic"));
}

#[test]
fn a_file_path_in_the_text_is_sent_as_text() {
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let text = "/synthetic/notes.txt";
    create(&client, &store, &request(offered(), text, "op-n14-path"));
    let argv = client.last_argv();
    assert_eq!(argv.last().map(String::as_str), Some(text));
    assert!(!argv.iter().any(|w| w == "--file" || w == "--image"));
}

#[test]
fn the_empty_list_has_its_fixed_digest_on_every_record() {
    assert_eq!(
        empty_attachments_digest(),
        sha256_hex(b"openwiz.prompt.attachments.v1\n")
    );
    assert_eq!(attachments_digest(&[]), empty_attachments_digest());
    let client = FakeClient::new();
    let store = MemoryStore::new();
    let r = request(offered(), "Synthetic", "op-n14-digest");
    let report = create(&client, &store, &r);
    assert_eq!(report.attachments_digest, empty_attachments_digest());
    assert_eq!(
        store.get(&r.op_id).unwrap().binding.attachments_digest,
        empty_attachments_digest()
    );
}

#[test]
fn an_attachment_id_is_one_safe_segment() {
    for good in ["a", "att-syn-0001", "A_1"] {
        assert!(AttachmentId::parse(good).is_ok(), "{good}");
    }
    let long = "a".repeat(65);
    for bad in ["", "-a", "_a", "a/b", "..", "a b", long.as_str()] {
        assert!(AttachmentId::parse(bad).is_err(), "{bad}");
    }
}

fn prompt_schema() -> Value {
    serde_json::from_str(&std::fs::read_to_string(PROMPT_SCHEMA).unwrap()).unwrap()
}

#[test]
fn the_prompt_schema_allows_attachment_ids_each_once() {
    let schema = prompt_schema();
    assert_eq!(schema["title"], "openwiz.prompt.v1");
    assert_eq!(crate::pattern::lint(&schema), Vec::<String>::new());
    let text = serde_json::to_value(PromptPayload::text("Synthetic")).unwrap();
    assert_eq!(
        crate::pattern::validate(&schema, &text),
        Vec::<String>::new()
    );
    let with = json!({"text": "Synthetic", "attachments": ["att-syn-0001", "att-syn-0002"]});
    assert_eq!(
        crate::pattern::validate(&schema, &with),
        Vec::<String>::new()
    );
    for bad in [
        json!({"text": "Synthetic", "attachments": ["att-syn-0001", "att-syn-0001"]}),
        json!({"text": "Synthetic", "attachments": ["-att"]}),
        json!({"text": "Synthetic", "attachments": ["a/b"]}),
        json!({"text": "Synthetic", "attachments": [1]}),
    ] {
        assert!(!crate::pattern::validate(&schema, &bad).is_empty(), "{bad}");
    }
    let empty_text = json!({"text": "", "attachments": []});
    assert!(!crate::pattern::validate(&schema, &empty_text).is_empty());
}
