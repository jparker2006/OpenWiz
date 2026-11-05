//! U4: the create is bounded: one project, from
//! the path; the send's body cap, with `Content-Length`, within its time;
//! exactly one JSON shape, with `attachments` empty; one create at a time.
//! Every refusal is before any read: zero `room-cli` calls and no record.
//! Nothing of a request is logged.

use std::thread;
use std::time::Duration;

use openwiz_server::{Limits, MAX_CREATES, MAX_SEND_BODY_BYTES};
use serde_json::json;

use crate::new_support::{
    NewFake, PROJECT, TEXT, body, create, file, post, preview, serve_new, serve_new_limits, threads,
};
use crate::send_support::{good_headers, raw, with};

#[test]
fn the_create_limits_are_the_documented_values() {
    assert_eq!(MAX_CREATES, 1, "one create at a time (A-14)");
    assert_eq!(Limits::default().max_creates, MAX_CREATES);
    assert_eq!(MAX_SEND_BODY_BYTES, 99_328, "the send's body cap");
}

#[test]
fn malformed_bodies_are_refused_before_any_read() {
    let fake = NewFake::new("web_u4_shapes");
    let s = serve_new(&fake);
    let good = body("u4-op-1", TEXT);
    let mut extra = good.clone();
    extra["project"] = json!("prj_syn_0006");
    let mut target = good.clone();
    target["target"] = json!("prj_syn_0006");
    let mut attached = good.clone();
    attached["attachments"] = json!(["att_syn_0001"]);
    let mut attached_object = good.clone();
    attached_object["attachments"] = json!({});
    let mut attached_null = good.clone();
    attached_null["attachments"] = json!(null);
    let mut no_attachments = good.clone();
    no_attachments
        .as_object_mut()
        .unwrap()
        .remove("attachments");
    let mut no_harness = good.clone();
    no_harness.as_object_mut().unwrap().remove("harness");
    let mut number_model = good.clone();
    number_model["model"] = json!(1);
    let mut null_effort = good.clone();
    null_effort["effort"] = json!(null);
    let mut array_text = good.clone();
    array_text["text"] = json!(["Synthetic"]);
    let mut object_op = good.clone();
    object_op["op_id"] = json!({"u4-op-1": null});
    let positional = json!(["u4-op-1", "codex", "syn-codex-model-01", "high", TEXT, []]);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("another key (a second project)", extra.to_string().into_bytes()),
        ("a target key", target.to_string().into_bytes()),
        ("an attachment", attached.to_string().into_bytes()),
        ("attachments as an object", attached_object.to_string().into_bytes()),
        ("attachments null", attached_null.to_string().into_bytes()),
        ("no attachments key", no_attachments.to_string().into_bytes()),
        ("no harness", no_harness.to_string().into_bytes()),
        ("a number model", number_model.to_string().into_bytes()),
        ("a null effort", null_effort.to_string().into_bytes()),
        ("an array text", array_text.to_string().into_bytes()),
        ("an object op id", object_op.to_string().into_bytes()),
        ("a positional array", positional.to_string().into_bytes()),
        ("null", b"null".to_vec()),
        ("not JSON", b"op_id=u4-op-1&text=x".to_vec()),
        ("trailing bytes", format!("{good} x").into_bytes()),
        (
            "a repeated key",
            format!(
                r#"{{"op_id":"u4-op-1","op_id":"u4-op-2","harness":"codex","model":"syn-codex-model-01","effort":"high","text":"{TEXT}","attachments":[]}}"#
            )
            .into_bytes(),
        ),
        ("invalid UTF-8", {
            let mut b = good.to_string().into_bytes();
            let at = b.len() - 30;
            b[at] = 0xff;
            b
        }),
        ("empty", vec![]),
    ];
    for target in [threads(PROJECT), preview(PROJECT)] {
        for (what, payload) in &cases {
            let r = post(&s, &target, &good_headers(), payload);
            assert_eq!(r.status, 400, "{target} {what}: {}", r.text());
            assert_eq!(r.error().0, "invalid_request", "{target} {what}");
            assert!(!r.text().contains(TEXT), "{what}: the text is repeated");
        }
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.any_record());
}

#[test]
fn the_cores_rules_refuse_a_bad_op_id_text_or_setting_before_any_read() {
    let fake = NewFake::new("web_u4_core_rules");
    let s = serve_new(&fake);
    let cases = [
        ("an op id with a space", body("u4 op", TEXT)),
        ("an op id starting with -", body("-u4", TEXT)),
        ("an empty text", body("u4-op-3", "")),
        ("a blank text", body("u4-op-4", "   \n")),
        ("a text with NUL", body("u4-op-5", "Synthetic\u{0}")),
        (
            "a text over 16 KiB",
            body("u4-op-6", &"x".repeat(16 * 1024 + 1)),
        ),
        (
            "an empty harness",
            crate::new_support::body_with("u4-op-7", "", "m", "high", TEXT),
        ),
        (
            "a model with a space",
            crate::new_support::body_with("u4-op-8", "codex", "a b", "high", TEXT),
        ),
        (
            "an effort starting with -",
            crate::new_support::body_with("u4-op-9", "codex", "m", "-x", TEXT),
        ),
    ];
    for target in [threads(PROJECT), preview(PROJECT)] {
        for (what, value) in &cases {
            let r = post(&s, &target, &good_headers(), value.to_string().as_bytes());
            assert_eq!(r.status, 400, "{target} {what}: {}", r.text());
            assert_eq!(r.error().0, "invalid_request", "{target} {what}");
        }
    }
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.any_record());
}

#[test]
fn the_framing_is_bounded() {
    let fake = NewFake::new("web_u4_framing");
    let s = serve_new_limits(
        &fake,
        Limits {
            body_timeout: Duration::from_millis(500),
            ..Limits::default()
        },
    );
    let payload = body("u4-op-10", TEXT).to_string();
    for target in [threads(PROJECT), preview(PROJECT)] {
        // No Content-Length.
        let r = s.send(&raw(
            "POST",
            &target,
            &good_headers(),
            payload.as_bytes(),
            false,
        ));
        assert_eq!(r.status, 411, "{target}: {}", r.text());
        // Transfer-Encoding.
        let r = s.send(&raw(
            "POST",
            &target,
            &with(&good_headers(), "Transfer-Encoding", "chunked"),
            payload.as_bytes(),
            true,
        ));
        assert_eq!(r.status, 400, "{target}: {}", r.text());
        // Over the cap: refused without reading the body.
        let mut headers = good_headers();
        headers.push((
            "Content-Length".to_owned(),
            (MAX_SEND_BODY_BYTES + 1).to_string(),
        ));
        let r = s.send(&raw("POST", &target, &headers, b"", false));
        assert_eq!(r.status, 413, "{target}: {}", r.text());
        assert_eq!(r.error().0, "body_too_large");
        // A body that never arrives in full.
        let mut headers = good_headers();
        headers.push((
            "Content-Length".to_owned(),
            (payload.len() + 10).to_string(),
        ));
        let mut stream = std::net::TcpStream::connect(s.addr).unwrap();
        use std::io::Write as _;
        stream
            .write_all(&raw("POST", &target, &headers, payload.as_bytes(), false))
            .unwrap();
        let r = crate::http::read_response(&mut stream);
        assert_eq!(r.status, 408, "{target}: {}", r.text());
        // A query.
        let r = s.send(&raw(
            "POST",
            &format!("{target}?x=1"),
            &good_headers(),
            payload.as_bytes(),
            true,
        ));
        assert_eq!(r.status, 400, "{target}: {}", r.text());
    }
    // One project per request: a path with more is no route (a `POST` there
    // is `405` with `Allow: GET`, by the contract's order).
    let r = post(
        &s,
        "/api/v1/projects/prj_syn_0001/prj_syn_0006/threads",
        &good_headers(),
        payload.as_bytes(),
    );
    assert_eq!(r.status, 405, "{}", r.text());
    assert_eq!(r.header("Allow"), Some("GET"));
    assert!(fake.invocations().is_empty(), "{:?}", fake.invocations());
    assert!(!fake.any_record());
}

#[test]
fn a_create_at_the_cap_of_the_body_is_read() {
    let fake = NewFake::new("web_u4_at_cap");
    let s = serve_new(&fake);
    // 16 KiB of control characters, each escaped to six bytes: the
    // largest text a page can send, within the cap.
    let text = "\u{1}".repeat(16 * 1024);
    let payload = body("u4-op-11", &text).to_string();
    assert!(payload.len() <= MAX_SEND_BODY_BYTES, "{}", payload.len());
    let r = post(&s, &threads(PROJECT), &good_headers(), payload.as_bytes());
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(fake.spawn_count(), 1);
}

#[test]
fn one_create_at_a_time() {
    let fake = NewFake::with(
        "web_u4_one_at_a_time",
        vec![json!({
            "any_spawn": true,
            "delay_ms": 1500,
            "stdout_file": file("spawn-receipt-created.json"),
            "creates": true,
        })],
    );
    let s = serve_new_limits(
        &fake,
        Limits {
            slot_wait: Duration::from_millis(300),
            ..Limits::default()
        },
    );
    let addr = s.addr;
    let first = thread::spawn(move || {
        crate::http::send(
            addr,
            &raw(
                "POST",
                &threads(PROJECT),
                &good_headers(),
                body("u4-op-12", TEXT).to_string().as_bytes(),
                true,
            ),
        )
    });
    // Wait until the first create is in its spawn call.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while fake.spawn_count() == 0 {
        assert!(
            std::time::Instant::now() < until,
            "the first create never called"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let second = create(&s, PROJECT, &body("u4-op-13", "Synthetic A2 other prompt"));
    assert_eq!(second.status, 503, "{}", second.text());
    assert_eq!(second.error().0, "busy");
    assert_eq!(second.header("Retry-After"), Some("5"));
    assert!(!fake.record_path("u4-op-13").exists(), "never claimed");
    let first = first.join().unwrap();
    assert_eq!(first.status, 200, "{}", first.text());
    assert_eq!(fake.spawn_count(), 1);
}

#[test]
fn nothing_of_a_create_is_logged() {
    let fake = NewFake::new("web_u4_log");
    let s = serve_new(&fake);
    let r = create(&s, PROJECT, &body("u4-op-14", TEXT));
    assert_eq!(r.status, 200, "{}", r.text());
    let r = crate::new_support::preview_json(&s, PROJECT, &body("u4-op-14", TEXT));
    assert_eq!(r.status, 200, "{}", r.text());
    let r = s.get(&crate::new_support::status("u4-op-14"));
    assert_eq!(r.status, 200, "{}", r.text());
    let r = s.get(crate::new_support::OPTIONS);
    assert_eq!(r.status, 200, "{}", r.text());
    let log = s.settled_log();
    for line in [
        "POST /api/v1/projects/{id}/threads 200 ",
        "POST /api/v1/projects/{id}/threads/preview 200 ",
        "GET /api/v1/new-threads/{op_id} 200 ",
        "GET /api/v1/cloud-options 200 ",
    ] {
        assert!(log.contains(line), "no line {line:?} in:\n{log}");
    }
    for secret in [
        TEXT,
        "u4-op-14",
        PROJECT,
        "syn-codex-model-01",
        "codex",
        "high",
    ] {
        assert!(!log.contains(secret), "the log holds {secret:?}:\n{log}");
    }
}
