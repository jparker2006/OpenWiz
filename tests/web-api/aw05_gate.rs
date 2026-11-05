//! AW5, the server's half: **the gate is the core's, and the
//! server's is the real one.** A request that skips the page (forged, or
//! from an old tab) with an attachment that is not ready (never staged,
//! still uploading, failed, removed, expired or in use by another
//! operation), or with Steer, is refused with the core's words before
//! anything is claimed: zero `room-cli` calls and no record. A ready one
//! goes, as `--file` or `--image`. The page's half (the disabled button)
//! is in `apps/web/test/`.

use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use openwiz_core::messaging::send::STEER_WITH_ATTACHMENTS;
use openwiz_core::model::Timestamp;
use openwiz_server::{Config, Limits};
use serde_json::{Value, json};

use crate::attach_support::{
    ATTACHMENTS, Free, attaching, ready_id, remove, serve, upload, upload_headers,
};
use crate::http::TestServer;
use crate::msg::SendFake;
use crate::new_support::{EFFORT, HARNESS, MODEL, NewFake, PROJECT, clock};
use crate::send_support::{HOST, raw, send_json};
use crate::staging::{id_of, png, stage, text};

/// A server with the send routes and the attachment routes, over `fake`'s
/// state directory, at the new-thread proofs' clock.
pub fn serve_send_attach(fake: &SendFake) -> TestServer {
    let config = Config {
        allowed_host: Some(HOST.to_owned()),
        web_root: None,
        limits: Limits::default(),
        sending: Some(crate::send_support::sending(fake)),
        creating: None,
        attaching: Some(attaching(&fake.state, Free::plenty())),
    };
    serve(
        config,
        Arc::new(fake.source()),
        Arc::new(crate::msg::clock()),
    )
}

/// A server with the new-thread routes and the attachment routes.
pub fn serve_new_attach(fake: &NewFake) -> TestServer {
    let config = Config {
        allowed_host: Some(HOST.to_owned()),
        web_root: None,
        limits: Limits::default(),
        sending: None,
        creating: Some(crate::new_support::creating(fake)),
        attaching: Some(attaching(&fake.state, Free::plenty())),
    };
    serve(config, Arc::new(fake.source()), Arc::new(clock()))
}

/// `openwiz new <args>` in this process with `fake`'s attachment store.
pub fn new_cli(fake: &NewFake, args: &[&str]) -> (i32, String, String) {
    use openwiz_cli::new_thread::{NewContext, run_new_with};
    use openwiz_core::messaging::record::StoreError;
    use openwiz_core::new_thread::record::NewThreadStore;
    use openwiz_store::{AttachmentStore, NewThreadFileStore};
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let source = fake.source();
    let (a, b, c) = (fake.state.clone(), fake.state.clone(), fake.state.clone());
    let open_store = move || -> Result<Box<dyn NewThreadStore>, StoreError> {
        Ok(Box::new(NewThreadFileStore::open(&a)?))
    };
    let open_store_read_only = move || -> Result<Option<Box<dyn NewThreadStore>>, StoreError> {
        let store = NewThreadFileStore::open_read_only(&b)?;
        Ok(store.map(|s| Box::new(s) as Box<dyn NewThreadStore>))
    };
    let open_attachments =
        move || -> Result<AttachmentStore, StoreError> { AttachmentStore::open(&c) };
    let fixed = clock();
    let wait = |_d: Duration| {};
    let mut input: &[u8] = b"";
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_new_with(
        &args[1..],
        NewContext {
            client: &source,
            open_store: &open_store,
            open_store_read_only: &open_store_read_only,
            clock: &fixed,
            stdin: &mut input,
            stdin_is_terminal: false,
            new_op_id: &openwiz_cli::send::random_op_id,
            wait: &wait,
        },
        &open_attachments,
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

const TARGET: &str = "thr_syn_0001";

fn send_body(op: &str, mode: &str, attachments: Value) -> Value {
    json!({"op_id": op, "mode": mode, "text": "Synthetic AW5 text", "attachments": attachments})
}

/// A queue tell, any attachments, answered as sent.
fn sent() -> Value {
    let mut response =
        crate::msg::tell_response(TARGET, "queue", json!({ "any_attachments": true }));
    let path = crate::msg::send_fixtures().join("tell-sent.json");
    assert!(path.is_file(), "no fixture {path:?}");
    response["stdout_file"] = json!(path.to_str().unwrap());
    response
}

/// Refused with the core's words; no call, and no send record.
fn refused(fake: &SendFake, s: &TestServer, body: &Value, words: &str) {
    let r = send_json(s, TARGET, body);
    assert_eq!(r.status, 400, "{body}: {}", r.text());
    let (kind, message) = r.error();
    assert_eq!(kind, "invalid_request", "{body}");
    assert!(message.contains(words), "{body}: {message}");
    assert_eq!(fake.tell_count(), 0, "{body}: a tell was made");
    let op = body["op_id"].as_str().unwrap();
    assert!(!fake.record_path(op).exists(), "{body}: claimed");
}

#[test]
fn a_forged_send_with_an_attachment_that_is_not_ready_is_refused_with_zero_calls() {
    let fake = SendFake::with("web_aw5_send", vec![sent()]);
    let s = serve_send_attach(&fake);
    // Never staged.
    refused(
        &fake,
        &s,
        &send_body(
            "aw5-op-1",
            "queue",
            json!(["att-00000000000000000000000000000000"]),
        ),
        "not found",
    );
    // Still uploading: no id is ready, so whatever the page could name is
    // not found. While one upload is in flight (it holds a slot), a send
    // naming every attachment directory there is, and a fresh id, is refused.
    let mut stream = TcpStream::connect(s.addr).unwrap();
    let mut headers = upload_headers("aw5-uploading.txt");
    headers.push(("Content-Length".to_owned(), "1000000".to_owned()));
    stream
        .write_all(&raw("POST", ATTACHMENTS, &headers, b"", false))
        .unwrap();
    stream.write_all(&vec![b'u'; 70_000]).unwrap();
    s.wait_for("the upload to hold its slot", |st| st.uploads == 1);
    let mut in_flight: Vec<String> = std::fs::read_dir(fake.state.join("attachments"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    in_flight.push("att-0000000000000000000000000000000a".to_owned());
    refused(
        &fake,
        &s,
        &send_body("aw5-op-2", "queue", json!(in_flight)),
        "not found",
    );
    drop(stream);
    // Removed.
    let id = ready_id(&upload(&s, "aw5-removed.txt", b"Synthetic AW5\n"));
    assert_eq!(remove(&s, &id).status, 200);
    refused(
        &fake,
        &s,
        &send_body("aw5-op-3", "queue", json!([id])),
        "not found",
    );
    // Expired: ready for more than 24 hours, unsent.
    let old = stage(
        &fake.state,
        "aw5-old.txt",
        b"Synthetic old\n",
        Timestamp(crate::msg::clock().0.0 - 25 * 60 * 60 * 1000),
    );
    refused(
        &fake,
        &s,
        &send_body("aw5-op-4", "queue", json!([id_of(&old)])),
        "expired",
    );
    // One bad attachment stops the good ones: no partial send.
    let good = ready_id(&upload(&s, "aw5-good.txt", b"Synthetic AW5 good\n"));
    refused(
        &fake,
        &s,
        &send_body(
            "aw5-op-5",
            "queue",
            json!([good, "att-00000000000000000000000000000001"]),
        ),
        "not found",
    );
    // A repeated id.
    refused(
        &fake,
        &s,
        &send_body("aw5-op-6", "queue", json!([good, good])),
        "listed twice",
    );
    // Steer never carries attachments.
    refused(
        &fake,
        &s,
        &send_body("aw5-op-7", "steer", json!([good])),
        STEER_WITH_ATTACHMENTS,
    );
    // Not an attachment id at all: the core's id rule.
    refused(
        &fake,
        &s,
        &send_body("aw5-op-8", "queue", json!(["../etc/passwd"])),
        "",
    );
    // Then the good one goes, as a file.
    let r = send_json(&s, TARGET, &send_body("aw5-op-9", "queue", json!([good])));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.tell_count(), 1);
    let tell = &fake.tells()[0];
    assert!(tell.iter().any(|a| a == "--file"), "{tell:?}");
}

#[test]
fn an_attachment_in_use_by_another_send_is_refused_with_zero_calls() {
    // The first send's tell answers with no receipt: uncertain, so its
    // attachment stays claimed.
    let lost = crate::msg::tell_response(
        TARGET,
        "queue",
        json!({ "any_attachments": true, "stdout": "not json" }),
    );
    let fake = SendFake::with("web_aw5_in_use", vec![lost]);
    let s = serve_send_attach(&fake);
    let id = ready_id(&upload(&s, "aw5-in-use.txt", b"Synthetic AW5\n"));
    let r = send_json(&s, TARGET, &send_body("aw5-op-10", "queue", json!([id])));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["status"], "uncertain", "{}", r.text());
    assert_eq!(fake.tell_count(), 1);
    let r = send_json(&s, TARGET, &send_body("aw5-op-11", "queue", json!([id])));
    assert_eq!(r.status, 400, "{}", r.text());
    assert!(r.error().1.contains("in use"), "{}", r.text());
    assert_eq!(fake.tell_count(), 1, "no second call");
    // And it can't be removed while it may have been sent.
    let r = remove(&s, &id);
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["ids"][0]["result"], "refused");
}

#[test]
fn the_send_bodys_attachments_must_be_a_list_of_strings() {
    let fake = SendFake::new("web_aw5_shape", "send");
    let s = serve_send_attach(&fake);
    for bad in [
        json!("att-00000000000000000000000000000000"),
        json!([1]),
        json!([null]),
        json!([["att-00000000000000000000000000000000"]]),
        json!({"id": "att-00000000000000000000000000000000"}),
        json!(null),
    ] {
        let r = send_json(&s, TARGET, &send_body("aw5-op-12", "queue", bad.clone()));
        assert_eq!(r.status, 400, "{bad}: {}", r.text());
        assert_eq!(r.error().0, "invalid_request");
        assert!(
            r.error().1.contains("attachments"),
            "{bad}: the body rule names it"
        );
    }
    // Absent is none: 0.0.3's body, unchanged.
    let r = send_json(
        &s,
        "thr_syn_0002",
        &json!({"op_id": "aw5-op-13", "mode": "queue", "text": "Synthetic AW5"}),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.tell_count(), 1);
}

#[test]
fn a_forged_create_with_an_attachment_that_is_not_ready_is_refused_with_zero_calls() {
    let fake = NewFake::new("web_aw5_new");
    let s = serve_new_attach(&fake);
    let body = |op: &str, ids: Value| {
        json!({"op_id": op, "harness": HARNESS, "model": MODEL, "effort": EFFORT,
               "text": "Synthetic AW5 prompt", "attachments": ids})
    };
    for (op, ids, words) in [
        (
            "aw5-op-20",
            json!(["att-00000000000000000000000000000000"]),
            "not found",
        ),
        ("aw5-op-21", json!([1]), "attachments"),
        ("aw5-op-22", json!("x"), "attachments"),
    ] {
        for preview in [true, false] {
            let r = if preview {
                crate::new_support::preview_json(&s, PROJECT, &body(op, ids.clone()))
            } else {
                crate::new_support::create(&s, PROJECT, &body(op, ids.clone()))
            };
            assert_eq!(r.status, 400, "{op} {preview}: {}", r.text());
            assert!(r.error().1.contains(words), "{op}: {}", r.text());
        }
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.any_record());
    // A ready image goes, as an image.
    let id = id_of(&stage(&fake.state, "aw5-new.png", &png(300), clock().0));
    let r = crate::new_support::create(&s, PROJECT, &body("aw5-op-23", json!([id])));
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.spawn_count(), 1);
    let spawn = &fake.spawns()[0];
    assert!(spawn.iter().any(|a| a == "--image"), "{spawn:?}");
    let _ = text("");
}
